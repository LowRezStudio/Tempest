//! Logging that survives a game crash.
//!
//! Every line goes to three sinks, each flushed immediately so the last line
//! before a crash is preserved:
//!
//! 1. `%TEMP%\tempest-scripting.log` (override: `TEMPEST_SCRIPT_LOG=<path>`)
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
    std::env::temp_dir()
        .join("tempest-scripting.log")
        .to_string_lossy()
        .into_owned()
}

/// Open the sinks. Idempotent.
pub fn init() {
    let _ = start();
    let path = PATH.get_or_init(default_path).clone();
    if FILE.get().is_none() {
        if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = FILE.set(Mutex::new(f));
        }
    }
    #[cfg(windows)]
    {
        if std::env::var("TEMPEST_SCRIPT_CONSOLE").as_deref() != Ok("0") {
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
    const CONTINUE_SEARCH: i32 = 0;
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
    let addr = (*rec).address as usize;
    let (p0, p1) = ((*rec).info[0], (*rec).info[1]);
    emit(&format!(
        "[VEH][crash] {name} code=0x{code:08X} at=0x{addr:x} info=(0x{p0:x}, 0x{p1:x})\n"
    ));
    CONTINUE_SEARCH
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
}
