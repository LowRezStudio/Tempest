//! Address hooks: detour any function in the process by address and run Lua
//! handlers for it; plus raw calls into arbitrary addresses.
//!
//! Every hooked address gets a slot that lives for the rest of the process
//! (disabled when its last handler goes, re-enabled by the next hook on the
//! same address). A detour jumps to a bare function pointer with no room
//! for context, so each slot owns a tiny stub generated at runtime (see
//! `stubs`) that passes the slot id to the shared `common` entry (assembly
//! below), which spills the arguments and calls [`dispatch`].
//!
//! Handlers on one address chain: the newest runs first and reaches the next
//! one, or the real function, through the `original` it is handed.
//!
//! Argument positions map to machine locations through [`locate`]:
//!
//! * **x64** (one convention): position i is RCX/RDX/R8/R9 for i < 4 and the
//!   caller's stack slots above, [`MAX_ARGS`] of them; a float at a register
//!   position is read from XMM i instead, and a float result travels in XMM0.
//!   Reading past what the caller passed only picks up its stack, and passing
//!   the extra values back to the original is harmless.
//! * **x86** needs the convention ([`Conv`]): `cdecl`/`stdcall` put every
//!   argument on the stack, `thiscall` passes `this` in ECX, `fastcall` the
//!   first two integers in ECX/EDX. Floats are always on the stack (an `f64`
//!   takes two dword slots) and a float result comes back in ST(0). The
//!   callee cleans the stack in every convention but `cdecl`, so the hook
//!   entry must know how many bytes to pop: that is why the convention and
//!   the argument kinds are fixed per hooked address.
//!
//! The target must be a function entry point: the detour relocates the
//! first instructions, and refuses prologues it cannot move.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Integer argument positions on x64, stack dword slots on x86.
pub const MAX_ARGS: usize = 8;
pub type Args = [usize; MAX_ARGS];
/// Raw bits of XMM0..XMM3 at entry (x64 register positions of float arguments).
pub type Floats = [u64; 4];

/// How a hook handler reads an argument position (or the result).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArgKind {
    #[default]
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

    /// Dword stack slots the kind occupies on x86.
    pub fn stack_slots(self) -> usize {
        match self {
            ArgKind::F64 => 2,
            _ => 1,
        }
    }

    /// Code for the x86 assembly entries: 0 = integer, 1 = f32, 2 = f64.
    #[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
    fn code(self) -> u32 {
        match self {
            ArgKind::Int => 0,
            ArgKind::F32 => 1,
            ArgKind::F64 => 2,
        }
    }
}

/// x86 calling convention of a hooked or called function. There is only one
/// on x64, where this is accepted and ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Conv {
    #[default]
    Cdecl,
    Stdcall,
    Thiscall,
    Fastcall,
}

impl Conv {
    pub fn parse(s: &str) -> Option<Conv> {
        match s.to_ascii_lowercase().as_str() {
            "cdecl" | "c" => Some(Conv::Cdecl),
            "stdcall" | "winapi" | "system" => Some(Conv::Stdcall),
            "thiscall" | "this" | "method" => Some(Conv::Thiscall),
            "fastcall" => Some(Conv::Fastcall),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Conv::Cdecl => "cdecl",
            Conv::Stdcall => "stdcall",
            Conv::Thiscall => "thiscall",
            Conv::Fastcall => "fastcall",
        }
    }

    /// Integer arguments passed in ECX (and EDX) before the stack.
    fn register_args(self) -> usize {
        match self {
            Conv::Cdecl | Conv::Stdcall => 0,
            Conv::Thiscall => 1,
            Conv::Fastcall => 2,
        }
    }

    /// Does the callee pop its own stack arguments?
    pub fn callee_cleans(self) -> bool {
        self != Conv::Cdecl
    }
}

/// Where one argument position lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loc {
    /// x64 integer slot `i` (RCX, RDX, R8, R9, then the stack); a float at
    /// `i < 4` is XMM `i`.
    Int(usize),
    /// x86 ECX (`thiscall` `this`, first `fastcall` integer).
    Ecx,
    /// x86 EDX (second `fastcall` integer).
    Edx,
    /// x86 dword stack slot (0 = `[esp+4]` at entry).
    Stack(usize),
}

/// Machine locations of argument positions `kinds` under `conv`.
pub fn locate(kinds: &[ArgKind], conv: Conv) -> Result<Vec<Loc>, String> {
    if cfg!(target_pointer_width = "64") {
        if kinds.len() > MAX_ARGS {
            return Err(format!("at most {MAX_ARGS} arguments are visible"));
        }
        return Ok((0..kinds.len()).map(Loc::Int).collect());
    }
    let mut regs = conv.register_args();
    let mut slot = 0;
    let mut out = Vec::with_capacity(kinds.len());
    for &k in kinds {
        if k == ArgKind::Int && regs > 0 {
            out.push(if regs == conv.register_args() { Loc::Ecx } else { Loc::Edx });
            regs -= 1;
        } else {
            out.push(Loc::Stack(slot));
            slot += k.stack_slots();
        }
    }
    if slot > MAX_ARGS {
        return Err(format!("at most {MAX_ARGS} stack dwords of arguments are visible (an f64 takes two)"));
    }
    Ok(out)
}

/// Dword stack slots `kinds` occupy under `conv` (x86).
pub fn stack_slots(kinds: &[ArgKind], conv: Conv) -> usize {
    locate(kinds, conv)
        .unwrap_or_default()
        .iter()
        .zip(kinds)
        .filter_map(|(l, k)| matches!(l, Loc::Stack(_)).then_some(k.stack_slots()))
        .sum()
}

/// Fixed per hooked address: how the machine code is entered and left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shape {
    pub conv: Conv,
    /// Dword stack slots the function takes (x86).
    pub stack_slots: usize,
    /// How the result travels (x86 decides between EAX and ST(0) from it).
    pub ret: ArgKind,
}

impl Shape {
    pub fn new(kinds: &[ArgKind], conv: Conv, ret: ArgKind) -> Shape {
        Shape { conv, stack_slots: stack_slots(kinds, conv), ret }
    }

    /// Bytes the hook entry pops on return (x86, callee-cleans conventions).
    #[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
    fn cleanup_bytes(&self) -> u32 {
        if self.conv.callee_cleans() { (self.stack_slots * 4) as u32 } else { 0 }
    }
}

/// Everything one call carries, written by the assembly entry and read back
/// on return.
#[cfg(target_pointer_width = "64")]
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Regs {
    /// Integer slots: registers then stack.
    pub ints: Args,
    /// XMM0..XMM3.
    pub xmm: Floats,
    /// XMM0 result bits set by a float-returning handler.
    pub fret: u64,
}

/// Everything one call carries, written by the assembly entry and read back
/// on return.
#[cfg(target_pointer_width = "32")]
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Regs {
    /// Caller's stack dwords above the return address.
    pub stack: Args,
    pub ecx: usize,
    pub edx: usize,
    /// ST(0) result bits (f32 in the low dword, or an f64) for the entry to load.
    pub fret: u64,
    /// 0 = integer result, 1 = load `fret` as f32, 2 = as f64.
    pub fret_kind: u32,
    /// Bytes of arguments to pop on return.
    pub cleanup: u32,
}

impl Regs {
    /// The machine word at `loc`.
    pub fn word(&self, loc: Loc) -> usize {
        #[cfg(target_pointer_width = "64")]
        match loc {
            Loc::Int(i) => self.ints.get(i).copied().unwrap_or(0),
            _ => 0,
        }
        #[cfg(target_pointer_width = "32")]
        match loc {
            Loc::Ecx => self.ecx,
            Loc::Edx => self.edx,
            Loc::Stack(i) => self.stack.get(i).copied().unwrap_or(0),
            Loc::Int(_) => 0,
        }
    }

    pub fn set_word(&mut self, loc: Loc, w: usize) {
        #[cfg(target_pointer_width = "64")]
        if let Loc::Int(i) = loc {
            if let Some(s) = self.ints.get_mut(i) {
                *s = w;
            }
        }
        #[cfg(target_pointer_width = "32")]
        match loc {
            Loc::Ecx => self.ecx = w,
            Loc::Edx => self.edx = w,
            Loc::Stack(i) => {
                if let Some(s) = self.stack.get_mut(i) {
                    *s = w;
                }
            }
            Loc::Int(_) => {}
        }
    }

    /// The value at `loc` as the handler should see it.
    pub fn get(&self, loc: Loc, kind: ArgKind) -> f64 {
        #[cfg(target_pointer_width = "64")]
        let bits = match loc {
            Loc::Int(i) if i < 4 && kind != ArgKind::Int => self.xmm[i],
            l => self.word(l) as u64,
        };
        #[cfg(target_pointer_width = "32")]
        let bits = match (loc, kind) {
            (Loc::Stack(i), ArgKind::F64) => {
                let lo = self.stack.get(i).copied().unwrap_or(0) as u64;
                let hi = self.stack.get(i + 1).copied().unwrap_or(0) as u64;
                lo | (hi << 32)
            }
            (l, _) => self.word(l) as u64,
        };
        kind.decode(bits)
    }

    /// Set `loc` from a number of `kind`.
    pub fn set(&mut self, loc: Loc, kind: ArgKind, v: f64) {
        let bits = kind.encode(v);
        #[cfg(target_pointer_width = "64")]
        {
            self.set_word(loc, bits as usize);
            if let Loc::Int(i) = loc {
                if i < 4 {
                    self.xmm[i] = bits;
                }
            }
        }
        #[cfg(target_pointer_width = "32")]
        {
            match (loc, kind) {
                (Loc::Stack(i), ArgKind::F64) => {
                    self.set_word(Loc::Stack(i), bits as u32 as usize);
                    self.set_word(Loc::Stack(i + 1), (bits >> 32) as usize);
                }
                (l, _) => self.set_word(l, bits as usize),
            }
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
    /// Where each position lives.
    pub locs: Vec<Loc>,
    /// How the handler's return value is delivered.
    pub ret: ArgKind,
    pub conv: Conv,
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

/// What a call returns: the integer register, and the float result bits
/// (XMM0 on x64, ST(0) on x86 when the shape says the function returns one).
#[derive(Debug, Clone, Copy, Default)]
pub struct Ret {
    pub int: usize,
    pub fbits: u64,
}

static HOOKS: Mutex<Vec<Hook>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Runs the Lua chain; returns the value the caller gets back.
static DISPATCHER: OnceLock<fn(Call) -> Ret> = OnceLock::new();

pub fn set_dispatcher(f: fn(Call) -> Ret) {
    let _ = DISPATCHER.set(f);
}

/// Detour `addr` (or join its existing chain). Returns the hook id.
pub fn add(addr: usize, owner: &str, kinds: Vec<ArgKind>, ret: ArgKind, conv: Conv) -> Result<u64, String> {
    if addr == 0 {
        return Err("null address".into());
    }
    let locs = locate(&kinds, conv)?;
    let slot = detour::ensure(addr, Shape::new(&kinds, conv, ret))?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    HOOKS.lock().unwrap().insert(
        0,
        Hook {
            id,
            addr,
            owner: owner.to_string(),
            kinds,
            locs,
            ret,
            conv,
        },
    );
    let tag = if owner.is_empty() { String::new() } else { format!("[{owner}] ") };
    let conv_tag = if cfg!(target_pointer_width = "32") { format!(", {}", conv.name()) } else { String::new() };
    crate::log::line("hook", &format!("{tag}address hook #{id} on 0x{addr:x} (slot {slot}{conv_tag})"));
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

/// Call any address with the arguments in `regs`, entered and left as
/// `shape` says. `None` on the host build or for a null address.
pub fn call_raw(addr: usize, regs: &Regs, shape: &Shape) -> Option<Ret> {
    if addr == 0 {
        return None;
    }
    detour::call_raw(addr, regs, shape)
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
    use super::{Regs, Ret, Shape, MAX_ARGS};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::Mutex;

    struct Slot {
        addr: usize,
        detour: retour::RawDetour,
        shape: Shape,
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
    // [rsp+0x60]: xmm, [rsp+0x80]: float result) and call
    // `dispatch_entry(slot, &regs)`; RAX comes back as is and XMM0 is loaded
    // from the result slot. SEH directives describe the frame so an
    // exception raised below unwinds through it.
    #[cfg(target_arch = "x86_64")]
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
    #[cfg(target_arch = "x86_64")]
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

    // Shared entry for every stub on x86. On entry EAX holds the slot id
    // (set by the stub), ECX/EDX the register arguments of thiscall /
    // fastcall functions, [esp] the return address and [esp+4...] the stack
    // arguments. Spill them into a `Regs` on the stack (stack dwords at
    // [esp], ECX at +0x20, EDX at +0x24, float result at +0x28, its kind at
    // +0x30, the bytes to pop at +0x34) and call `dispatch_entry(slot,
    // &regs)`. EAX comes back as is; a float result is loaded into ST(0)
    // from the result slot; then the caller's arguments are popped when the
    // convention says the callee does that, by lifting the return address
    // over them.
    #[cfg(target_arch = "x86")]
    std::arch::global_asm!(
        ".text",
        ".p2align 4",
        ".globl _tempest_hook_common",
        "_tempest_hook_common:",
        "    push ebp",
        "    mov ebp, esp",
        "    sub esp, 0x40",
        "    mov [esp + 0x20], ecx",
        "    mov [esp + 0x24], edx",
        "    mov [esp + 0x38], eax",
        "    mov ecx, [ebp + 0x8]",
        "    mov [esp], ecx",
        "    mov ecx, [ebp + 0xc]",
        "    mov [esp + 0x4], ecx",
        "    mov ecx, [ebp + 0x10]",
        "    mov [esp + 0x8], ecx",
        "    mov ecx, [ebp + 0x14]",
        "    mov [esp + 0xc], ecx",
        "    mov ecx, [ebp + 0x18]",
        "    mov [esp + 0x10], ecx",
        "    mov ecx, [ebp + 0x1c]",
        "    mov [esp + 0x14], ecx",
        "    mov ecx, [ebp + 0x20]",
        "    mov [esp + 0x18], ecx",
        "    mov ecx, [ebp + 0x24]",
        "    mov [esp + 0x1c], ecx",
        "    xor ecx, ecx",
        "    mov [esp + 0x28], ecx",
        "    mov [esp + 0x2c], ecx",
        "    mov [esp + 0x30], ecx",
        "    mov [esp + 0x34], ecx",
        "    mov edx, esp",
        "    push edx",
        "    push dword ptr [esp + 0x3c]",
        "    call {dispatch}",
        "    add esp, 8",
        "    mov ecx, [esp + 0x30]",
        "    test ecx, ecx",
        "    jz 2f",
        "    cmp ecx, 1",
        "    jne 1f",
        "    fld dword ptr [esp + 0x28]",
        "    jmp 2f",
        "1:  fld qword ptr [esp + 0x28]",
        "2:  mov ecx, [esp + 0x34]",
        "    mov esp, ebp",
        "    pop ebp",
        "    test ecx, ecx",
        "    jz 3f",
        "    pop edx",
        "    add esp, ecx",
        "    jmp edx",
        "3:  ret",
        dispatch = sym dispatch_entry,
    );

    // `tempest_call_regs(f, regs, nslots, fret_kind, fbits_out)` (cdecl):
    // push `nslots` stack dwords of `regs` (last first), load ECX/EDX from
    // it, call `f`, then restore ESP whether or not the callee popped its
    // arguments. EAX is returned; when `fret_kind` is 1 or 2 ST(0) is popped
    // into `fbits_out` as an f32 or f64.
    #[cfg(target_arch = "x86")]
    std::arch::global_asm!(
        ".text",
        ".p2align 4",
        ".globl _tempest_call_regs",
        "_tempest_call_regs:",
        "    push ebp",
        "    mov ebp, esp",
        "    push ebx",
        "    push esi",
        "    push edi",
        "    mov esi, [ebp + 0xc]",
        "    mov ecx, [ebp + 0x10]",
        "    mov ebx, esp",
        "4:  test ecx, ecx",
        "    jz 5f",
        "    dec ecx",
        "    push dword ptr [esi + ecx*4]",
        "    jmp 4b",
        "5:  mov eax, [ebp + 0x8]",
        "    mov ecx, [esi + 0x20]",
        "    mov edx, [esi + 0x24]",
        "    call eax",
        "    mov esp, ebx",
        "    mov ecx, [ebp + 0x14]",
        "    test ecx, ecx",
        "    jz 7f",
        "    mov edi, [ebp + 0x18]",
        "    cmp ecx, 1",
        "    jne 6f",
        "    fstp dword ptr [edi]",
        "    jmp 7f",
        "6:  fstp qword ptr [edi]",
        "7:  pop edi",
        "    pop esi",
        "    pop ebx",
        "    pop ebp",
        "    ret",
    );

    extern "C" {
        fn tempest_hook_common();
        #[cfg(target_arch = "x86_64")]
        fn tempest_call_regs(f: usize, ints: *const usize, xmm: *const u64, xmm_out: *mut u64) -> usize;
        #[cfg(target_arch = "x86")]
        fn tempest_call_regs(f: usize, regs: *const Regs, nslots: usize, fret_kind: u32, fbits_out: *mut u64) -> usize;
    }

    /// Call `f` with `regs` as `shape` says; the result carries the integer
    /// register and the float result bits.
    fn call_regs(f: usize, regs: &Regs, shape: &Shape) -> Ret {
        let mut fbits = 0u64;
        // SAFETY: `f` is a function entry the caller vouches for; the asm
        // helpers follow the platform conventions for up to MAX_ARGS slots.
        #[cfg(target_arch = "x86_64")]
        let int = {
            let _ = shape;
            unsafe { tempest_call_regs(f, regs.ints.as_ptr(), regs.xmm.as_ptr(), &mut fbits) }
        };
        #[cfg(target_arch = "x86")]
        let int = unsafe { tempest_call_regs(f, regs, shape.stack_slots.min(MAX_ARGS), shape.ret.code(), &mut fbits) };
        Ret { int, fbits }
    }

    /// Called from the assembly entry with the slot id and the spilled registers.
    extern "C" fn dispatch_entry(slot: usize, regs: *mut Regs) -> usize {
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
        regs_ref.fret = r.fbits;
        #[cfg(target_arch = "x86")]
        if let Some(s) = self::slot(slot) {
            regs_ref.fret_kind = s.shape.ret.code();
            regs_ref.cleanup = s.shape.cleanup_bytes();
        }
        r.int
    }

    fn slot(i: usize) -> Option<&'static Slot> {
        SLOTS.lock().unwrap().get(i).copied()
    }

    pub fn addr_of(i: usize) -> usize {
        slot(i).map(|s| s.addr).unwrap_or(0)
    }

    /// Slot bound to `addr`, detour enabled: reused when it exists (and the
    /// shape agrees), created otherwise.
    pub fn ensure(addr: usize, shape: Shape) -> Result<usize, String> {
        let _guard = INSTALL.lock().unwrap();
        let existing = SLOTS.lock().unwrap().iter().position(|s| s.addr == addr);
        if let Some(i) = existing {
            let s = slot(i).unwrap();
            if cfg!(target_pointer_width = "32") && s.shape != shape {
                return Err(format!(
                    "0x{addr:x} is already hooked as {} with {} stack dword(s) returning {:?}; every hook on one address must declare the same convention, arguments and result",
                    s.shape.conv.name(),
                    s.shape.stack_slots,
                    s.shape.ret
                ));
            }
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
        // the full argument set, so any signature up to MAX_ARGS works.
        let detour = unsafe { retour::RawDetour::new(addr as *const (), stub as *const ()) }
            .map_err(|e| format!("cannot detour 0x{addr:x}: {e}"))?;
        unsafe { detour.enable() }.map_err(|e| format!("enable detour at 0x{addr:x}: {e}"))?;
        SLOTS.lock().unwrap().push(Box::leak(Box::new(Slot { addr, detour, shape })));
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
            Some(s) => call_regs(s.detour.trampoline() as *const () as usize, regs, &s.shape),
            None => Ret::default(),
        }
    }

    pub fn call_raw(addr: usize, regs: &Regs, shape: &Shape) -> Option<Ret> {
        Some(call_regs(addr, regs, shape))
    }

    const _: () = assert!(MAX_ARGS == 8, "the assembly entries spill exactly eight slots");
    #[cfg(target_arch = "x86")]
    const _: () = assert!(std::mem::size_of::<Regs>() == 0x38, "the x86 assembly entry lays Regs out by hand");
    #[cfg(target_arch = "x86_64")]
    const _: () = assert!(std::mem::size_of::<Regs>() == 0x68, "the x64 assembly entry lays Regs out by hand");
}

#[cfg(not(windows))]
mod detour {
    use super::{Regs, Ret, Shape};

    pub fn addr_of(_slot: usize) -> usize {
        0
    }
    pub fn ensure(_addr: usize, _shape: Shape) -> Result<usize, String> {
        Err("host build".into())
    }
    pub fn disable(_addr: usize) {}
    pub fn call_original(_slot: usize, _r: &Regs) -> Ret {
        Ret::default()
    }
    pub fn call_raw(_addr: usize, _r: &Regs, _shape: &Shape) -> Option<Ret> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_refuses_hooks_and_keeps_bookkeeping_sane() {
        assert!(add(0, "t", vec![ArgKind::Int; 4], ArgKind::Int, Conv::Cdecl).is_err());
        assert!(add(0x1000, "t", vec![ArgKind::Int; 4], ArgKind::Int, Conv::Cdecl).is_err());
        assert!(add(0x1000, "t", vec![ArgKind::Int; 9], ArgKind::Int, Conv::Cdecl).is_err());
        assert!(list().is_empty());
        assert!(!remove(99));
        assert!(clear_of("t").is_empty());
        assert!(call_raw(0, &Regs::default(), &Shape::default()).is_none());
    }

    #[test]
    fn locations_follow_the_convention() {
        let kinds = [ArgKind::Int, ArgKind::F64, ArgKind::Int, ArgKind::F32];
        let locs = locate(&kinds, Conv::Thiscall).unwrap();
        if cfg!(target_pointer_width = "64") {
            assert_eq!(locs, vec![Loc::Int(0), Loc::Int(1), Loc::Int(2), Loc::Int(3)]);
        } else {
            // `this` in ECX, then the stack: f64 (2 dwords), int, f32.
            assert_eq!(locs, vec![Loc::Ecx, Loc::Stack(0), Loc::Stack(2), Loc::Stack(3)]);
            assert_eq!(stack_slots(&kinds, Conv::Thiscall), 4);
            assert_eq!(locate(&kinds, Conv::Fastcall).unwrap(), vec![Loc::Ecx, Loc::Stack(0), Loc::Edx, Loc::Stack(2)]);
            assert_eq!(locate(&kinds, Conv::Cdecl).unwrap(), vec![Loc::Stack(0), Loc::Stack(1), Loc::Stack(3), Loc::Stack(4)]);
            assert!(locate(&[ArgKind::F64; 5], Conv::Cdecl).is_err());
            assert!(Shape::new(&kinds, Conv::Stdcall, ArgKind::Int).conv.callee_cleans());
        }
        assert_eq!(Conv::parse("thiscall"), Some(Conv::Thiscall));
        assert_eq!(Conv::parse("nope"), None);
    }

    #[test]
    fn regs_place_values_by_location() {
        let mut r = Regs::default();
        let locs = locate(&[ArgKind::Int, ArgKind::F32, ArgKind::Int, ArgKind::Int, ArgKind::Int, ArgKind::F64], Conv::Cdecl).unwrap();
        r.set(locs[0], ArgKind::Int, 7.0);
        r.set(locs[1], ArgKind::F32, 1.5);
        r.set(locs[5], ArgKind::F64, -2.25);
        assert_eq!(r.word(locs[0]), 7);
        assert_eq!(r.get(locs[1], ArgKind::F32), 1.5);
        assert_eq!(r.get(locs[5], ArgKind::F64), -2.25);
        assert_eq!(r.get(locs[0], ArgKind::Int), 7.0);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(r.xmm[1], (1.5f32).to_bits() as u64);
        assert_eq!(ArgKind::parse("double"), Some(ArgKind::F64));
        assert_eq!(ArgKind::parse("nope"), None);
    }
}
