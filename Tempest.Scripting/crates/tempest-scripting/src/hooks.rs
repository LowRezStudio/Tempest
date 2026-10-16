//! Address hooks: detour any function in the process by address and run Lua
//! handlers for it; plus raw calls into arbitrary addresses.
//!
//! Every hooked address gets a slot that lives for the rest of the process
//! (disabled when its last handler goes, re-enabled by the next hook on the
//! same address). A detour jumps to a bare function pointer with no room
//! for context, so each slot owns a tiny stub generated at runtime: `mov
//! r10d, <slot>; jmp common`. The shared `common` entry (assembly below)
//! spills the integer arguments and calls [`dispatch`] with the slot id.
//! Stubs are written into executable pages allocated on demand, so there is
//! no fixed number of them.
//!
//! Handlers on one address chain: the newest runs first and reaches the next
//! one, or the real function, through the `original` it is handed.
//!
//! The stub ABI carries [`MAX_ARGS`] integer parameters, which on Win64
//! means RCX, RDX, R8, R9 and then the caller's stack slots, whatever the
//! real function takes; reading past what the caller passed only picks up
//! its stack, and passing the extra values back to the original is harmless.
//! XMM0..XMM3 are captured too, so a hook that declares a float at
//! position i (`ArgKind::F32`/`F64`) reads it from XMM i (register
//! positions) or the stack slot (positions 4 and up), and a float result
//! is returned in XMM0.
//!
//! The target must be a function entry point: the detour relocates the
//! first instructions, and refuses prologues it cannot move.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

pub const MAX_ARGS: usize = 8;
pub type Args = [usize; MAX_ARGS];
/// Raw bits of XMM0..XMM3 at entry (positions 0..4 of a float argument).
pub type Floats = [u64; 4];

/// How a hook handler reads an argument position (or the result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    Int,
    F32,
    F64,
}

impl ArgKind {
    pub fn parse(s: &str) -> Option<ArgKind> {
        match s.to_ascii_lowercase().as_str() {
            "int" | "i64" | "u64" | "ptr" | "pointer" | "bool" | "i32" | "u32" => Some(ArgKind::Int),
            "f32" | "float" => Some(ArgKind::F32),
            "f64" | "double" => Some(ArgKind::F64),
            _ => None,
        }
    }

    /// Bits of a float value as it travels in a register or stack slot.
    pub fn encode(self, v: f64) -> u64 {
        match self {
            ArgKind::Int => v as i64 as u64,
            ArgKind::F32 => (v as f32).to_bits() as u64,
            ArgKind::F64 => v.to_bits(),
        }
    }

    pub fn decode(self, bits: u64) -> f64 {
        match self {
            ArgKind::Int => bits as i64 as f64,
            ArgKind::F32 => f32::from_bits(bits as u32) as f64,
            ArgKind::F64 => f64::from_bits(bits),
        }
    }
}

/// Everything one call carries: integer slots (registers then stack), the
/// four XMM registers, and the XMM0 result bits set by a float-returning
/// handler. Written by the assembly entry and read back on return.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Regs {
    pub ints: Args,
    pub xmm: Floats,
    pub xmm_ret: u64,
}

impl Regs {
    /// The value at position `i` as the handler should see it.
    pub fn get(&self, i: usize, kind: ArgKind) -> f64 {
        let bits = if i < 4 && kind != ArgKind::Int { self.xmm[i] } else { self.ints[i] as u64 };
        kind.decode(bits)
    }

    /// Set position `i` from a number of `kind` (both the integer slot and,
    /// for register positions, the XMM register).
    pub fn set(&mut self, i: usize, kind: ArgKind, v: f64) {
        let bits = kind.encode(v);
        self.ints[i] = bits as usize;
        if i < 4 {
            self.xmm[i] = bits;
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hook {
    pub id: u64,
    pub addr: usize,
    /// Mod that installed it (`""` outside any mod).
    pub owner: String,
    /// What the Lua handler receives, position by position.
    pub kinds: Vec<ArgKind>,
    /// How the handler's return value is delivered.
    pub ret: ArgKind,
}

/// One intercepted call, handed to the dispatcher. `handlers` is the chain
/// for this address, newest first.
#[derive(Debug, Clone)]
pub struct Call {
    pub slot: usize,
    pub addr: usize,
    pub regs: Regs,
    pub handlers: Vec<Hook>,
}

/// What a call returns: RAX, and XMM0 for float results.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ret {
    pub int: usize,
    pub xmm0: u64,
}

static HOOKS: Mutex<Vec<Hook>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Runs the Lua chain; returns the value the caller gets back.
static DISPATCHER: OnceLock<fn(Call) -> Ret> = OnceLock::new();

pub fn set_dispatcher(f: fn(Call) -> Ret) {
    let _ = DISPATCHER.set(f);
}

/// Detour `addr` (or join its existing chain). Returns the hook id.
pub fn add(addr: usize, owner: &str, kinds: Vec<ArgKind>, ret: ArgKind) -> Result<u64, String> {
    if addr == 0 {
        return Err("null address".into());
    }
    if kinds.len() > MAX_ARGS {
        return Err(format!("at most {MAX_ARGS} arguments are visible"));
    }
    let slot = detour::ensure(addr)?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    HOOKS.lock().unwrap().insert(
        0,
        Hook {
            id,
            addr,
            owner: owner.to_string(),
            kinds,
            ret,
        },
    );
    let tag = if owner.is_empty() { String::new() } else { format!("[{owner}] ") };
    crate::log::line("hook", &format!("{tag}address hook #{id} on 0x{addr:x} (slot {slot})"));
    Ok(id)
}

/// Drop one hook; the detour is disabled when the address has no handler left.
pub fn remove(id: u64) -> bool {
    let addr = {
        let mut hooks = HOOKS.lock().unwrap();
        let Some(pos) = hooks.iter().position(|h| h.id == id) else { return false };
        hooks.remove(pos).addr
    };
    release(addr);
    true
}

/// Drop every hook owned by `owner`; returns their ids.
pub fn clear_of(owner: &str) -> Vec<u64> {
    let gone: Vec<Hook> = {
        let mut hooks = HOOKS.lock().unwrap();
        let (gone, keep): (Vec<Hook>, Vec<Hook>) = hooks.drain(..).partition(|h| h.owner.eq_ignore_ascii_case(owner));
        *hooks = keep;
        gone
    };
    for h in &gone {
        release(h.addr);
    }
    gone.into_iter().map(|h| h.id).collect()
}

pub fn clear_all() -> Vec<u64> {
    let gone: Vec<Hook> = std::mem::take(&mut *HOOKS.lock().unwrap());
    for h in &gone {
        release(h.addr);
    }
    gone.into_iter().map(|h| h.id).collect()
}

fn release(addr: usize) {
    if !HOOKS.lock().unwrap().iter().any(|h| h.addr == addr) {
        detour::disable(addr);
    }
}

pub fn list() -> Vec<Hook> {
    HOOKS.lock().unwrap().clone()
}

/// Call the un-hooked function behind `slot` (the trampoline).
pub fn call_original(slot: usize, regs: &Regs) -> Ret {
    detour::call_original(slot, regs)
}

/// Call any address with up to [`MAX_ARGS`] arguments (integers in `ints`,
/// floats mirrored in `xmm`). `None` on the host build or for a null address.
pub fn call_raw(addr: usize, regs: &Regs) -> Option<Ret> {
    if addr == 0 {
        return None;
    }
    detour::call_raw(addr, regs)
}

/// Stub entry: snapshot the chain and hand it to the dispatcher, or run
/// the original straight away when nothing is subscribed any more.
#[cfg_attr(not(windows), allow(dead_code))]
fn dispatch(slot: usize, regs: Regs) -> Ret {
    let addr = detour::addr_of(slot);
    let handlers: Vec<Hook> = HOOKS.lock().unwrap().iter().filter(|h| h.addr == addr).cloned().collect();
    if handlers.is_empty() {
        return detour::call_original(slot, &regs);
    }
    match DISPATCHER.get() {
        Some(run) => run(Call { slot, addr, regs, handlers }),
        None => detour::call_original(slot, &regs),
    }
}

#[cfg(windows)]
mod detour {
    use super::{Regs, Ret, MAX_ARGS};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::Mutex;

    struct Slot {
        addr: usize,
        detour: retour::RawDetour,
    }

    /// Slot id → slot. Boxes are leaked on purpose: a trampoline may be
    /// executing on another thread at any moment, so nothing here is ever
    /// freed or moved.
    static SLOTS: Mutex<Vec<&'static Slot>> = Mutex::new(Vec::new());
    static INSTALL: Mutex<()> = Mutex::new(());

    // Shared entry for every stub. On entry R10 holds the slot id (set by
    // the stub), RCX/RDX/R8/R9 the first four integer arguments, XMM0..3
    // the first four float ones, and the caller's stack the rest: return
    // address at [rsp], 32 bytes of shadow space, then argument 5 at
    // [rsp+0x28]. Spill them into a `Regs` on the stack ([rsp+0x20]: ints,
    // [rsp+0x60]: xmm, [rsp+0x80]: xmm result) and call
    // `dispatch_entry(slot, &regs)`; RAX comes back as is and XMM0 is loaded
    // from the result slot. SEH directives describe the frame so an
    // exception raised below unwinds through it.
    std::arch::global_asm!(
        ".text",
        ".p2align 4",
        ".globl tempest_hook_common",
        ".seh_proc tempest_hook_common",
        "tempest_hook_common:",
        "    push rbp",
        "    .seh_pushreg rbp",
        "    mov rbp, rsp",
        "    .seh_setframe rbp, 0",
        "    sub rsp, 0x90",
        "    .seh_stackalloc 0x90",
        "    .seh_endprologue",
        "    mov [rsp + 0x20], rcx",
        "    mov [rsp + 0x28], rdx",
        "    mov [rsp + 0x30], r8",
        "    mov [rsp + 0x38], r9",
        "    mov rax, [rbp + 0x30]",
        "    mov [rsp + 0x40], rax",
        "    mov rax, [rbp + 0x38]",
        "    mov [rsp + 0x48], rax",
        "    mov rax, [rbp + 0x40]",
        "    mov [rsp + 0x50], rax",
        "    mov rax, [rbp + 0x48]",
        "    mov [rsp + 0x58], rax",
        "    movq [rsp + 0x60], xmm0",
        "    movq [rsp + 0x68], xmm1",
        "    movq [rsp + 0x70], xmm2",
        "    movq [rsp + 0x78], xmm3",
        "    mov qword ptr [rsp + 0x80], 0",
        "    mov rcx, r10",
        "    lea rdx, [rsp + 0x20]",
        "    call {dispatch}",
        "    movq xmm0, [rsp + 0x80]",
        "    mov rsp, rbp",
        "    pop rbp",
        "    ret",
        ".seh_endproc",
        dispatch = sym dispatch_entry,
    );

    // Call `fn` with the integer slots of `regs` in RCX/RDX/R8/R9 and the
    // stack, XMM0..3 loaded from its float slots; RAX is returned and XMM0
    // is stored through `xmm_out`.
    std::arch::global_asm!(
        ".text",
        ".p2align 4",
        ".globl tempest_call_regs",
        ".seh_proc tempest_call_regs",
        "tempest_call_regs:",
        "    push rbp",
        "    .seh_pushreg rbp",
        "    mov rbp, rsp",
        "    .seh_setframe rbp, 0",
        "    push rbx",
        "    .seh_pushreg rbx",
        "    push rsi",
        "    .seh_pushreg rsi",
        "    push rdi",
        "    .seh_pushreg rdi",
        "    sub rsp, 0x48",
        "    .seh_stackalloc 0x48",
        "    .seh_endprologue",
        "    mov rbx, rcx",
        "    mov rsi, rdx",
        "    mov rdi, r8",
        "    mov [rsp + 0x40], r9",
        "    mov rax, [rsi + 0x20]",
        "    mov [rsp + 0x20], rax",
        "    mov rax, [rsi + 0x28]",
        "    mov [rsp + 0x28], rax",
        "    mov rax, [rsi + 0x30]",
        "    mov [rsp + 0x30], rax",
        "    mov rax, [rsi + 0x38]",
        "    mov [rsp + 0x38], rax",
        "    mov rcx, [rsi]",
        "    mov rdx, [rsi + 8]",
        "    mov r8, [rsi + 0x10]",
        "    mov r9, [rsi + 0x18]",
        "    movq xmm0, [rdi]",
        "    movq xmm1, [rdi + 8]",
        "    movq xmm2, [rdi + 0x10]",
        "    movq xmm3, [rdi + 0x18]",
        "    call rbx",
        "    mov rcx, [rsp + 0x40]",
        "    movq [rcx], xmm0",
        "    add rsp, 0x48",
        "    pop rdi",
        "    pop rsi",
        "    pop rbx",
        "    pop rbp",
        "    ret",
        ".seh_endproc",
    );

    extern "system" {
        fn tempest_hook_common();
        fn tempest_call_regs(f: usize, ints: *const usize, xmm: *const u64, xmm_out: *mut u64) -> usize;
    }

    /// Call `f` with `regs`; the result carries RAX and XMM0.
    fn call_regs(f: usize, regs: &Regs) -> Ret {
        let mut xmm0 = 0u64;
        // SAFETY: `f` is a function entry the caller vouches for; the asm
        // helper follows the Win64 convention for up to MAX_ARGS arguments.
        let int = unsafe { tempest_call_regs(f, regs.ints.as_ptr(), regs.xmm.as_ptr(), &mut xmm0) };
        Ret { int, xmm0 }
    }

    /// Called from the assembly entry with the slot id and the spilled registers.
    extern "system" fn dispatch_entry(slot: usize, regs: *mut Regs) -> usize {
        // SAFETY: the assembly entry passes a pointer to the `Regs` it just wrote.
        let regs_ref = unsafe { &mut *regs };
        let snapshot = *regs_ref;
        let r = match catch_unwind(AssertUnwindSafe(|| super::dispatch(slot, snapshot))) {
            Ok(r) => r,
            Err(_) => {
                crate::log::line("hook", &format!("address hook on 0x{:x} panicked; running the original", addr_of(slot)));
                call_original(slot, &snapshot)
            }
        };
        regs_ref.xmm_ret = r.xmm0;
        r.int
    }

    fn slot(i: usize) -> Option<&'static Slot> {
        SLOTS.lock().unwrap().get(i).copied()
    }

    pub fn addr_of(i: usize) -> usize {
        slot(i).map(|s| s.addr).unwrap_or(0)
    }

    /// Slot bound to `addr`, detour enabled: reused when it exists, created otherwise.
    pub fn ensure(addr: usize) -> Result<usize, String> {
        let _guard = INSTALL.lock().unwrap();
        let existing = SLOTS.lock().unwrap().iter().position(|s| s.addr == addr);
        if let Some(i) = existing {
            let s = slot(i).unwrap();
            if !s.detour.is_enabled() {
                unsafe { s.detour.enable() }.map_err(|e| format!("re-enable detour at 0x{addr:x}: {e}"))?;
            }
            return Ok(i);
        }
        if !crate::memory::is_readable(addr, 16) {
            return Err(format!("0x{addr:x} is not readable code"));
        }
        let i = SLOTS.lock().unwrap().len();
        let stub = crate::stubs::make(i, tempest_hook_common as *const () as usize)?;
        // SAFETY: the caller asserts `addr` is a function entry; the stub
        // preserves every register the real function expects and forwards
        // the full Win64 argument set, so any signature up to MAX_ARGS works.
        let detour = unsafe { retour::RawDetour::new(addr as *const (), stub as *const ()) }
            .map_err(|e| format!("cannot detour 0x{addr:x}: {e}"))?;
        unsafe { detour.enable() }.map_err(|e| format!("enable detour at 0x{addr:x}: {e}"))?;
        SLOTS.lock().unwrap().push(Box::leak(Box::new(Slot { addr, detour })));
        Ok(i)
    }

    /// Keep the slot (its trampoline may still be executing) but stop intercepting.
    pub fn disable(addr: usize) {
        let _guard = INSTALL.lock().unwrap();
        let found = SLOTS.lock().unwrap().iter().find(|s| s.addr == addr).copied();
        if let Some(s) = found {
            if s.detour.is_enabled() {
                if let Err(e) = unsafe { s.detour.disable() } {
                    crate::log::line("hook", &format!("disable detour at 0x{addr:x}: {e}"));
                }
            }
        }
    }

    pub fn call_original(i: usize, regs: &Regs) -> Ret {
        match slot(i) {
            Some(s) => call_regs(s.detour.trampoline() as *const () as usize, regs),
            None => Ret::default(),
        }
    }

    pub fn call_raw(addr: usize, regs: &Regs) -> Option<Ret> {
        Some(call_regs(addr, regs))
    }

    const _: () = assert!(MAX_ARGS == 8, "the assembly entry spills exactly eight arguments");
}

#[cfg(not(windows))]
mod detour {
    use super::{Regs, Ret};

    pub fn addr_of(_slot: usize) -> usize {
        0
    }
    pub fn ensure(_addr: usize) -> Result<usize, String> {
        Err("host build".into())
    }
    pub fn disable(_addr: usize) {}
    pub fn call_original(_slot: usize, _r: &Regs) -> Ret {
        Ret::default()
    }
    pub fn call_raw(_addr: usize, _r: &Regs) -> Option<Ret> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_refuses_hooks_and_keeps_bookkeeping_sane() {
        assert!(add(0, "t", vec![ArgKind::Int; 4], ArgKind::Int).is_err());
        assert!(add(0x1000, "t", vec![ArgKind::Int; 4], ArgKind::Int).is_err());
        assert!(list().is_empty());
        assert!(!remove(99));
        assert!(clear_of("t").is_empty());
        assert!(call_raw(0, &Regs::default()).is_none());
    }

    #[test]
    fn regs_place_floats_by_position() {
        let mut r = Regs::default();
        r.set(0, ArgKind::Int, 7.0);
        r.set(1, ArgKind::F32, 1.5);
        r.set(5, ArgKind::F64, -2.25);
        assert_eq!(r.ints[0], 7);
        assert_eq!(r.xmm[1], (1.5f32).to_bits() as u64);
        assert_eq!(r.get(1, ArgKind::F32), 1.5);
        assert_eq!(r.get(5, ArgKind::F64), -2.25);
        assert_eq!(r.get(0, ArgKind::Int), 7.0);
        assert_eq!(ArgKind::parse("double"), Some(ArgKind::F64));
        assert_eq!(ArgKind::parse("nope"), None);
    }
}
