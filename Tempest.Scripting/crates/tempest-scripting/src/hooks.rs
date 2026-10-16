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
//! Only integer/pointer arguments are visible. The stub ABI carries
//! [`MAX_ARGS`] integer parameters, which on Win64 means RCX, RDX, R8, R9
//! and then the caller's stack slots, whatever the real function takes;
//! reading past what the caller passed only picks up its stack, and passing
//! the extra values back to the original is harmless. Floating-point
//! arguments and results (XMM registers) are not captured.
//!
//! The target must be a function entry point: the detour relocates the
//! first instructions, and refuses prologues it cannot move.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

pub const MAX_ARGS: usize = 8;
pub type Args = [usize; MAX_ARGS];

#[derive(Debug, Clone)]
pub struct Hook {
    pub id: u64,
    pub addr: usize,
    /// Mod that installed it (`""` outside any mod).
    pub owner: String,
    /// How many arguments the Lua handler receives.
    pub nargs: usize,
}

/// One intercepted call, handed to the dispatcher. `handlers` is the chain
/// for this address, newest first.
#[derive(Debug, Clone)]
pub struct Call {
    pub slot: usize,
    pub addr: usize,
    pub args: Args,
    pub handlers: Vec<Hook>,
}

static HOOKS: Mutex<Vec<Hook>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Runs the Lua chain; returns the value the caller gets back.
static DISPATCHER: OnceLock<fn(Call) -> usize> = OnceLock::new();

pub fn set_dispatcher(f: fn(Call) -> usize) {
    let _ = DISPATCHER.set(f);
}

/// Detour `addr` (or join its existing chain). Returns the hook id.
pub fn add(addr: usize, owner: &str, nargs: usize) -> Result<u64, String> {
    if addr == 0 {
        return Err("null address".into());
    }
    let slot = detour::ensure(addr)?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    HOOKS.lock().unwrap().insert(
        0,
        Hook {
            id,
            addr,
            owner: owner.to_string(),
            nargs: nargs.min(MAX_ARGS),
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
pub fn call_original(slot: usize, args: Args) -> usize {
    detour::call_original(slot, args)
}

/// Call any address with up to [`MAX_ARGS`] integer arguments. `None` on the
/// host build or for a null address.
pub fn call_raw(addr: usize, args: Args) -> Option<usize> {
    if addr == 0 {
        return None;
    }
    detour::call_raw(addr, args)
}

/// Stub entry: snapshot the chain and hand it to the dispatcher, or run
/// the original straight away when nothing is subscribed any more.
#[cfg_attr(not(windows), allow(dead_code))]
fn dispatch(slot: usize, args: Args) -> usize {
    let addr = detour::addr_of(slot);
    let handlers: Vec<Hook> = HOOKS.lock().unwrap().iter().filter(|h| h.addr == addr).cloned().collect();
    if handlers.is_empty() {
        return detour::call_original(slot, args);
    }
    match DISPATCHER.get() {
        Some(run) => run(Call { slot, addr, args, handlers }),
        None => detour::call_original(slot, args),
    }
}

#[cfg(windows)]
mod detour {
    use super::{Args, MAX_ARGS};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::Mutex;

    type Fn8 = extern "system" fn(usize, usize, usize, usize, usize, usize, usize, usize) -> usize;

    struct Slot {
        addr: usize,
        detour: retour::GenericDetour<Fn8>,
    }

    /// Slot id → slot. Boxes are leaked on purpose: a trampoline may be
    /// executing on another thread at any moment, so nothing here is ever
    /// freed or moved.
    static SLOTS: Mutex<Vec<&'static Slot>> = Mutex::new(Vec::new());
    static INSTALL: Mutex<()> = Mutex::new(());

    // Shared entry for every stub. On entry R10 holds the slot id (set by
    // the stub), RCX/RDX/R8/R9 the first four arguments and the caller's
    // stack the rest: return address at [rsp], 32 bytes of shadow space,
    // then argument 5 at [rsp+0x28]. Spill all eight into a local array and
    // call `dispatch_entry(slot, &args)`. SEH directives describe the frame
    // so an exception raised below unwinds through it.
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
        "    sub rsp, 0x60",
        "    .seh_stackalloc 0x60",
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
        "    mov rcx, r10",
        "    lea rdx, [rsp + 0x20]",
        "    call {dispatch}",
        "    mov rsp, rbp",
        "    pop rbp",
        "    ret",
        ".seh_endproc",
        dispatch = sym dispatch_entry,
    );

    extern "system" {
        fn tempest_hook_common();
    }

    /// Called from the assembly entry with the slot id and the spilled arguments.
    extern "system" fn dispatch_entry(slot: usize, args: *const usize) -> usize {
        // SAFETY: the assembly entry passes a pointer to MAX_ARGS words it just wrote.
        let args: Args = unsafe { *(args as *const Args) };
        match catch_unwind(AssertUnwindSafe(|| super::dispatch(slot, args))) {
            Ok(r) => r,
            Err(_) => {
                crate::log::line("hook", &format!("address hook on 0x{:x} panicked; running the original", addr_of(slot)));
                call_original(slot, args)
            }
        }
    }

    /// Per-slot entry stubs, written into executable pages as needed:
    /// `mov r10d, <slot>` (6 bytes), `mov rax, <common>` (10), `jmp rax` (2).
    mod stubs {
        use std::ffi::c_void;
        use std::sync::Mutex;

        const SIZE: usize = 32;
        const PAGE: usize = 4096;
        /// Current page and bytes used in it.
        static CURRENT: Mutex<(usize, usize)> = Mutex::new((0, PAGE));

        pub fn make(slot: usize, common: usize) -> Result<usize, String> {
            let slot32 = u32::try_from(slot).map_err(|_| "too many hook slots".to_string())?;
            let mut cur = CURRENT.lock().unwrap();
            if cur.1 + SIZE > PAGE {
                const MEM_COMMIT_RESERVE: u32 = 0x1000 | 0x2000;
                const PAGE_EXECUTE_READWRITE: u32 = 0x40;
                let page = unsafe { VirtualAlloc(std::ptr::null_mut(), PAGE, MEM_COMMIT_RESERVE, PAGE_EXECUTE_READWRITE) };
                if page.is_null() {
                    return Err("VirtualAlloc for a hook stub page failed".into());
                }
                *cur = (page as usize, 0);
            }
            let stub = cur.0 + cur.1;
            let mut code = Vec::with_capacity(SIZE);
            code.extend_from_slice(&[0x41, 0xBA]);
            code.extend_from_slice(&slot32.to_le_bytes());
            code.extend_from_slice(&[0x48, 0xB8]);
            code.extend_from_slice(&(common as u64).to_le_bytes());
            code.extend_from_slice(&[0xFF, 0xE0]);
            code.resize(SIZE, 0xCC);
            // SAFETY: `stub` is inside a page this module allocated RWX and has not handed out yet.
            unsafe {
                std::ptr::copy_nonoverlapping(code.as_ptr(), stub as *mut u8, SIZE);
                FlushInstructionCache(GetCurrentProcess(), stub as *const c_void, SIZE);
            }
            cur.1 += SIZE;
            Ok(stub)
        }

        #[link(name = "kernel32")]
        extern "system" {
            fn VirtualAlloc(addr: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
            fn FlushInstructionCache(process: *mut c_void, addr: *const c_void, size: usize) -> i32;
            fn GetCurrentProcess() -> *mut c_void;
        }
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
        let stub = stubs::make(i, tempest_hook_common as *const () as usize)?;
        // SAFETY: the caller asserts `addr` is the entry of a function taking
        // integer arguments; the stub and the detour share the Fn8 ABI, which
        // is a superset of any smaller integer-argument signature on Win64.
        let original: Fn8 = unsafe { std::mem::transmute(addr) };
        let hook: Fn8 = unsafe { std::mem::transmute(stub) };
        let detour = unsafe { retour::GenericDetour::new(original, hook) }
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

    pub fn call_original(i: usize, a: Args) -> usize {
        match slot(i) {
            Some(s) => s.detour.call(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]),
            None => 0,
        }
    }

    pub fn call_raw(addr: usize, a: Args) -> Option<usize> {
        // SAFETY: the caller supplies a function address; same ABI note as `ensure`.
        let f: Fn8 = unsafe { std::mem::transmute(addr) };
        Some(f(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]))
    }

    const _: () = assert!(MAX_ARGS == 8, "the assembly entry spills exactly eight arguments");
}

#[cfg(not(windows))]
mod detour {
    use super::Args;

    pub fn addr_of(_slot: usize) -> usize {
        0
    }
    pub fn ensure(_addr: usize) -> Result<usize, String> {
        Err("host build".into())
    }
    pub fn disable(_addr: usize) {}
    pub fn call_original(_slot: usize, _a: Args) -> usize {
        0
    }
    pub fn call_raw(_addr: usize, _a: Args) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_refuses_hooks_and_keeps_bookkeeping_sane() {
        assert!(add(0, "t", 4).is_err());
        assert!(add(0x1000, "t", 4).is_err());
        assert!(list().is_empty());
        assert!(!remove(99));
        assert!(clear_of("t").is_empty());
        assert!(call_raw(0, [0; MAX_ARGS]).is_none());
    }
}
