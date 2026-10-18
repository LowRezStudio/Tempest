//! Logging that survives a game crash.
//!
//! Every line goes to three sinks, each flushed immediately so the last line
//! before a crash is preserved:
//!
//! 1. `%TEMP%\tempest-scripting.log` (override: `TEMPEST_SCRIPT_LOG=<path>`),
//!    started fresh on every run; the previous run is kept as `*.prev.log`
//! 2. `OutputDebugStringA` (DebugView / WinDbg)
//! 3. the console allocated at init (skip with `TEMPEST_SCRIPT_CONSOLE=0`)
//!
//! A panic hook logs Rust panics, and on Windows a vectored exception handler
//! logs hard faults (access violation, illegal instruction, stack overflow)
//! with their address before the process dies. Logging never panics.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static FILE: OnceLock<Mutex<File>> = OnceLock::new();
static PATH: OnceLock<String> = OnceLock::new();

fn start() -> &'static Instant {
    START.get_or_init(Instant::now)
}

fn default_path() -> String {
    if let Ok(p) = std::env::var("TEMPEST_SCRIPT_LOG") {
        if !p.is_empty() {
            return p;
        }
    }
    if let Some(p) = &crate::config::get().log {
        return p.clone();
    }
    std::env::temp_dir()
        .join("tempest-scripting.log")
        .to_string_lossy()
        .into_owned()
}

/// Open the sinks. Idempotent. `fresh` starts a new log file (the previous
/// one is kept as `<name>.prev.log`); an idle second runtime appends instead.
pub fn init(fresh: bool) {
    let _ = start();
    let path = PATH.get_or_init(default_path).clone();
    if FILE.get().is_none() {
        if fresh {
            let prev = match path.rsplit_once(".log") {
                Some((stem, "")) => format!("{stem}.prev.log"),
                _ => format!("{path}.prev"),
            };
            let _ = std::fs::rename(&path, prev);
        }
        let mut opts = OpenOptions::new();
        opts.create(true);
        if fresh {
            opts.write(true).truncate(true);
        } else {
            opts.append(true);
        }
        if let Ok(f) = opts.open(&path) {
            let _ = FILE.set(Mutex::new(f));
        }
    }
    #[cfg(windows)]
    {
        if console_wanted() {
            unsafe { AllocConsole() };
        }
        install_veh();
    }
    install_panic_hook();
    line("log", "---- session start ----");
}

pub fn path() -> Option<String> {
    PATH.get().cloned()
}

/// Console window + REPL: `[runtime] console` unless `TEMPEST_SCRIPT_CONSOLE=0`.
pub fn console_wanted() -> bool {
    std::env::var("TEMPEST_SCRIPT_CONSOLE").as_deref() != Ok("0") && crate::config::get().console
}

/// Write one tagged line to every sink.
pub fn line(tag: &str, msg: &str) {
    let text = format!(
        "[{:>8.2}s][{}][{tag}] {msg}\n",
        start().elapsed().as_secs_f64(),
        thread_tag()
    );
    emit(&text);
}

fn emit(text: &str) {
    #[cfg(windows)]
    unsafe {
        console_write(text);
        let mut c = text.as_bytes().to_vec();
        c.push(0);
        OutputDebugStringA(c.as_ptr());
    }
    #[cfg(not(windows))]
    {
        let _ = std::io::stderr().write_all(text.as_bytes());
    }
    if let Some(m) = FILE.get() {
        // try_lock: the exception handler may run while this thread already
        // holds the lock; dropping a line beats deadlocking the game.
        if let Ok(mut f) = m.try_lock() {
            let _ = f.write_all(text.as_bytes());
            let _ = f.flush();
        }
    }
}

fn thread_tag() -> String {
    #[cfg(windows)]
    unsafe {
        format!("t{}", GetCurrentThreadId())
    }
    #[cfg(not(windows))]
    {
        format!("{:?}", std::thread::current().id())
    }
}

fn install_panic_hook() {
    static DONE: OnceLock<()> = OnceLock::new();
    if DONE.set(()).is_err() {
        return;
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic>".into());
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "?".into());
        line("panic", &format!("{payload} at {loc}"));
        prev(info);
    }));
}

// ---------------------------------------------------------------------------
// Windows sinks + vectored exception handler
// ---------------------------------------------------------------------------

#[cfg(windows)]
unsafe fn console_write(text: &str) {
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    let h = GetStdHandle(STD_OUTPUT_HANDLE);
    if h.is_null() || h as isize == -1 {
        return;
    }
    let mut written = 0u32;
    WriteFile(
        h,
        text.as_ptr(),
        text.len() as u32,
        &mut written,
        std::ptr::null_mut(),
    );
}

#[cfg(windows)]
fn install_veh() {
    static DONE: OnceLock<()> = OnceLock::new();
    if DONE.set(()).is_err() {
        return;
    }
    // First-chance, leaked for the process lifetime.
    unsafe { AddVectoredExceptionHandler(1, Some(veh_handler)) };
}

#[cfg(windows)]
#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *mut std::ffi::c_void,
    address: *mut std::ffi::c_void,
    num_params: u32,
    info: [usize; 15],
}

#[cfg(windows)]
#[repr(C)]
struct ExceptionPointers {
    record: *mut ExceptionRecord,
    context: *mut std::ffi::c_void,
}

#[cfg(windows)]
unsafe extern "system" fn veh_handler(ptrs: *mut std::ffi::c_void) -> i32 {
    use std::sync::atomic::{AtomicBool, Ordering};
    const CONTINUE_SEARCH: i32 = 0;
    // A fault while reporting (bad stack, unmapped frame) must not recurse.
    static BUSY: AtomicBool = AtomicBool::new(false);
    if ptrs.is_null() {
        return CONTINUE_SEARCH;
    }
    let rec = (*(ptrs as *const ExceptionPointers)).record;
    if rec.is_null() {
        return CONTINUE_SEARCH;
    }
    let code = (*rec).code;
    let name = match code {
        0xC000_0005 => "ACCESS_VIOLATION",
        0xC000_001D => "ILLEGAL_INSTRUCTION",
        0xC000_0096 => "PRIVILEGED_INSTRUCTION",
        0xC000_00FD => "STACK_OVERFLOW",
        0xC000_0094 => "INT_DIVIDE_BY_ZERO",
        0xC000_0409 => "FAIL_FAST",
        // C++ exceptions, breakpoints, guard pages: the game's business.
        _ => return CONTINUE_SEARCH,
    };
    if BUSY.swap(true, Ordering::AcqRel) {
        return CONTINUE_SEARCH;
    }
    let addr = (*rec).address as usize;
    let (p0, p1) = ((*rec).info[0], (*rec).info[1]);
    // The header goes out before the stack walk, so a fault while walking
    // (which the BUSY guard swallows) still leaves the crash on record.
    emit(&format!(
        "[VEH][crash] {name} code=0x{code:08X} at={} info=(0x{p0:x}, 0x{p1:x}) thread=t{}\n",
        module_offset(addr),
        GetCurrentThreadId()
    ));
    let ctx = (*(ptrs as *const ExceptionPointers)).context;
    if !ctx.is_null() {
        let mut text = String::from("[VEH][stack]");
        for (i, pc) in backtrace(ctx).into_iter().enumerate() {
            text.push_str(&format!(" #{i} {}", module_offset(pc)));
        }
        text.push('\n');
        emit(&text);
    }
    BUSY.store(false, Ordering::Release);
    CONTINUE_SEARCH
}

/// `module.dll+0x1234` for an address inside a loaded module, else the raw address.
#[cfg(windows)]
pub fn module_offset(addr: usize) -> String {
    const FROM_ADDRESS: u32 = 0x4;
    const UNCHANGED_REFCOUNT: u32 = 0x2;
    unsafe {
        let mut module: *mut std::ffi::c_void = std::ptr::null_mut();
        if addr != 0 && GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, addr as *const u16, &mut module) != 0 && !module.is_null() {
            let mut buf = [0u16; 512];
            let n = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize;
            let path = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
            let file = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string();
            return format!("{file}+0x{:x}", addr - module as usize);
        }
    }
    format!("0x{addr:x}")
}

#[cfg(not(windows))]
pub fn module_offset(addr: usize) -> String {
    format!("0x{addr:x}")
}

/// Return addresses above the faulting frame, from the exception CONTEXT,
/// unwound with the loader's own tables (`RtlVirtualUnwind`).
#[cfg(all(windows, target_arch = "x86_64"))]
unsafe fn backtrace(ctx: *mut std::ffi::c_void) -> Vec<usize> {
    const CONTEXT_SIZE: usize = 0x4D0;
    const RSP: usize = 0x98;
    const RIP: usize = 0xF8;
    #[repr(C, align(16))]
    struct Ctx([u8; CONTEXT_SIZE]);
    let mut c = Ctx([0; CONTEXT_SIZE]);
    std::ptr::copy_nonoverlapping(ctx as *const u8, c.0.as_mut_ptr(), CONTEXT_SIZE);
    let get = |c: &Ctx, off: usize| std::ptr::read_unaligned(c.0.as_ptr().add(off) as *const u64) as usize;
    let set = |c: &mut Ctx, off: usize, v: usize| std::ptr::write_unaligned(c.0.as_mut_ptr().add(off) as *mut u64, v as u64);
    let mut out = Vec::new();
    for _ in 0..24 {
        let pc = get(&c, RIP);
        if pc == 0 {
            break;
        }
        out.push(pc);
        let mut image_base: u64 = 0;
        let entry = RtlLookupFunctionEntry(pc as u64, &mut image_base, std::ptr::null_mut());
        if entry.is_null() {
            // Leaf function: the return address sits at [rsp].
            let rsp = get(&c, RSP);
            if !crate::memory::is_readable(rsp, 8) {
                break;
            }
            let ret = *(rsp as *const u64) as usize;
            set(&mut c, RIP, ret);
            set(&mut c, RSP, rsp + 8);
        } else {
            let mut handler_data: *mut std::ffi::c_void = std::ptr::null_mut();
            let mut establisher: u64 = 0;
            RtlVirtualUnwind(0, image_base, pc as u64, entry, c.0.as_mut_ptr() as *mut std::ffi::c_void, &mut handler_data, &mut establisher, std::ptr::null_mut());
        }
    }
    out
}

/// Return addresses above the faulting frame on x86. There is no unwind
/// table to follow and frame-pointer omission is common, so this is a stack
/// scan: every dword from ESP upwards that points just past a `call` into a
/// loaded module's code. Noisy (stale frames show up too) but it never
/// misses the live frames; the faulting EIP comes first.
#[cfg(all(windows, target_arch = "x86"))]
unsafe fn backtrace(ctx: *mut std::ffi::c_void) -> Vec<usize> {
    const ESP: usize = 0xC4;
    const EIP: usize = 0xB8;
    let get = |off: usize| std::ptr::read_unaligned((ctx as *const u8).add(off) as *const u32) as usize;
    let mut out = vec![get(EIP)];
    let esp = get(ESP);
    if esp == 0 || esp % 4 != 0 {
        return out;
    }
    let mut addr = esp;
    let end = esp + 0x2000;
    while addr < end && out.len() < 40 {
        if !crate::memory::is_readable(addr, 4) {
            break;
        }
        let v = *(addr as *const u32) as usize;
        addr += 4;
        if v < 0x10000 || !looks_like_return_address(v) {
            continue;
        }
        out.push(v);
    }
    out
}

/// Is `v` inside a loaded module and preceded by a `call` instruction
/// (`E8 rel32`, or `FF /2` with the usual ModRM forms)?
#[cfg(all(windows, target_arch = "x86"))]
unsafe fn looks_like_return_address(v: usize) -> bool {
    const FROM_ADDRESS: u32 = 0x4;
    const UNCHANGED_REFCOUNT: u32 = 0x2;
    let mut module: *mut std::ffi::c_void = std::ptr::null_mut();
    if GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, v as *const u16, &mut module) == 0 || module.is_null() {
        return false;
    }
    if !crate::memory::is_readable(v - 7, 7) {
        return false;
    }
    let b = |off: usize| *((v - off) as *const u8);
    b(5) == 0xE8 // call rel32
        || (b(2) == 0xFF && b(1) & 0xF8 == 0xD0) // call reg
        || (b(3) == 0xFF && b(2) & 0xF8 == 0x50 && b(2) != 0x54) // call [reg+disp8]
        || (b(6) == 0xFF && b(5) & 0xF8 == 0x90) // call [reg+disp32]
        || (b(6) == 0xFF && b(5) == 0x15) // call [disp32]
        || (b(4) == 0xFF && b(3) == 0x54) // call [reg+reg*s+disp8]
        || (b(7) == 0xFF && b(6) == 0x94) // call [reg+reg*s+disp32]
        || (b(3) == 0xFF && b(2) == 0x14) // call [reg+reg*s]
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn AllocConsole() -> i32;
    fn GetStdHandle(id: u32) -> *mut std::ffi::c_void;
    fn WriteFile(
        h: *mut std::ffi::c_void,
        buf: *const u8,
        n: u32,
        written: *mut u32,
        overlapped: *mut std::ffi::c_void,
    ) -> i32;
    fn OutputDebugStringA(s: *const u8);
    fn GetCurrentThreadId() -> u32;
    fn AddVectoredExceptionHandler(
        first: u32,
        handler: Option<unsafe extern "system" fn(*mut std::ffi::c_void) -> i32>,
    ) -> *mut std::ffi::c_void;
    fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut *mut std::ffi::c_void) -> i32;
    fn GetModuleFileNameW(module: *mut std::ffi::c_void, buf: *mut u16, len: u32) -> u32;
    #[cfg(target_arch = "x86_64")]
    fn RtlLookupFunctionEntry(pc: u64, image_base: *mut u64, history: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    #[cfg(target_arch = "x86_64")]
    fn RtlVirtualUnwind(
        handler_type: u32,
        image_base: u64,
        pc: u64,
        entry: *mut std::ffi::c_void,
        context: *mut std::ffi::c_void,
        handler_data: *mut *mut std::ffi::c_void,
        establisher: *mut u64,
        pointers: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
}
