//! Tempest.Scripting — a small Lua modding DLL for Paladins 8.1 (UE3), built
//! for the Win64 client (`Tempest.Scripting64.dll`) and the Win32 client
//! (`Tempest.Scripting32.dll`) from the same source; `target_pointer_width`
//! selects the layouts, signatures and calling conventions.
//!
//! Boot sequence, all on a worker thread spawned from `DllMain`:
//!
//! 1. [`log::init`] — `%TEMP%\tempest-scripting.log`, debugger output, and
//!    a console window when the `console` option is on (launcher config).
//! 2. [`unreal::resolve`] — the client's signatures give GObjects, GNames,
//!    the engine slot, `ProcessEvent` and the anti-cheat XOR mask.
//!    Then it waits for the game to populate the arrays (`GNames[0] == "None"`).
//! 3. [`unreal::install_hook`] — detours `ProcessEvent`.
//! 4. [`unreal::wait_for_engine`] — waits for `*GEngine`, i.e. the Engine
//!    package is loaded (injection happens while only `Core` exists), then
//!    resolves `ProcessInternal` and detours it (script→script hooks).
//! 5. [`overlay::install_async`] — hooks Present on the game's D3D9/D3D11
//!    device (polling until the renderer exists) for the Lua `ui` module.
//! 6. [`loader::run_all`] — runs every `scripts/*.lua`.
//!
//! Everything degrades to "unresolved + a note in `tempest.status()`" rather
//! than crashing; the host (macOS/Linux) build has no game image and is only
//! used for `cargo test`.

#[cfg(all(windows, not(any(target_arch = "x86_64", target_arch = "x86"))))]
compile_error!("Tempest.Scripting targets the Win64 and Win32 clients only");

// The Win32 DLL is built with `panic = "abort"` (see `.cargo/config.toml`:
// the MinGW toolchain Homebrew ships for i686 uses SJLJ exceptions, which
// Rust's DWARF-unwinding std cannot link against). std's precompiled
// objects still carry landing pads that reference `_Unwind_Resume`; with
// abort-on-panic nothing ever unwinds, so a stub satisfies the linker and is
// never reached (`ud2` ends the process if it ever were). Defined in
// assembly rather than as a `#[no_mangle]` fn so it does not become a DLL export.
#[cfg(all(windows, target_arch = "x86"))]
std::arch::global_asm!(".text", ".globl __Unwind_Resume", "__Unwind_Resume:", "    ud2");

pub mod config;
pub mod hooks;
mod imgui_syms;
pub mod input;
pub mod loader;
pub mod log;
pub mod lua_api;
pub mod memory;
pub mod overlay;
pub mod stubs;
pub mod tls;
pub mod unreal;
pub mod watch;

use std::sync::OnceLock;

use mlua::Lua;

static LUA: OnceLock<Lua> = OnceLock::new();

/// Process-wide Lua VM, created (and the API registered) on first use.
pub fn lua() -> &'static Lua {
    LUA.get_or_init(|| {
        // Every standard library, `ffi`, `jit` and `debug` included: scripts
        // already have raw memory access through `memory.*`, so the sandbox
        // mlua's safe default would give adds nothing here.
        // SAFETY: the unsafe libraries are the point; nothing untrusted runs.
        let lua = unsafe { Lua::unsafe_new() };
        overlay::init();
        if !config::get().jit {
            let _ = lua.load("jit.off()").exec();
        }
        if let Err(e) = lua_api::register(&lua) {
            log::line("lua", &format!("API registration failed: {e}"));
        }
        lua
    })
}

/// `"x64"` or `"x86"`: which client this DLL targets.
pub fn arch() -> &'static str {
    if cfg!(target_pointer_width = "64") { "x64" } else { "x86" }
}

/// Full startup. Safe to call from the host build (everything stubs out).
pub fn boot() {
    // The first runtime in the process starts a fresh log; a second copy
    // (another mod shipping the DLL) only appends its one line.
    let primary = claim_runtime();
    log::init(primary);
    if !primary {
        log::line("boot", "another Tempest.Scripting instance already runs in this process; this copy stays idle");
        return;
    }
    for note in &config::get().notes {
        log::line("config", note);
    }
    if let Some(p) = &config::get().path {
        log::line("config", &format!("settings: {}", p.display()));
    }
    log::line(
        "boot",
        &format!(
            "Tempest.Scripting {} ({}) pid={} log={} dll={}",
            env!("CARGO_PKG_VERSION"),
            arch(),
            std::process::id(),
            log::path().unwrap_or_default(),
            log::module_offset(boot as *const () as usize)
        ),
    );
    let g = unreal::resolve();
    if g.process_event != 0 {
        unreal::install_hook();
    }
    if g.gnames_ok {
        unreal::wait_for_engine(std::time::Duration::from_secs(120));
        unreal::install_script_hook();
    }
    // The renderer is up by now (or soon); the UI hooks poll for it.
    overlay::install_async();
    run_scripts();
    if let Some(what) = watch::start() {
        log::line("hot", &format!("watching {what}"));
    }
    if let Some(types) = loader::mod_dir().map(|m| m.join("types")) {
        let written = config::seed_dev_types(&types);
        if !written.is_empty() {
            log::line("config", &format!("editor types for the dev folder: {}", written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")));
        }
    }
    log::line("boot", "ready");
    repl();
}

/// Run every mod's scripts on the game thread. Scripts walk the object
/// table (class lookups, `tempest.objects`), and the game thread grows and
/// reallocates that table while a map loads; iterating it from this thread
/// at the same time reads freed memory. The game thread picks the job up at
/// its next `ProcessEvent`; without the hook (or no event for a minute) the
/// scripts run here instead.
fn run_scripts() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    let run = || loader::run_all(lua());
    if !unreal::globals().hook_installed {
        run();
        return;
    }
    let claimed = Arc::new(AtomicBool::new(false));
    let done = Arc::new((Mutex::new(false), Condvar::new()));
    {
        let (claimed, done) = (claimed.clone(), done.clone());
        unreal::queue(Box::new(move || {
            if claimed.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                run();
            }
            *done.0.lock().unwrap() = true;
            done.1.notify_all();
        }));
    }
    let guard = done.0.lock().unwrap();
    let (_g, _) = done
        .1
        .wait_timeout_while(guard, std::time::Duration::from_secs(60), |finished| !*finished)
        .unwrap();
    if claimed.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
        log::line("boot", "no game-thread event within 60 s; running scripts on the boot thread");
        run();
    }
}

/// One runtime per process: several mods may ship this DLL (different
/// paths, so Windows loads each), but only the first may hook. It loads
/// every mod's scripts, so the others have nothing to do.
fn claim_runtime() -> bool {
    #[cfg(windows)]
    unsafe {
        const ERROR_ALREADY_EXISTS: u32 = 183;
        let name: Vec<u16> = format!("Local\\TempestScripting-{}", std::process::id())
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // Intentionally leaked: the mutex lives as long as the process.
        let h = CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr());
        !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
    #[cfg(not(windows))]
    true
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(attrs: *mut std::ffi::c_void, initial_owner: i32, name: *const u16) -> *mut std::ffi::c_void;
    fn GetLastError() -> u32;
}

/// Console REPL: every line typed into the console window runs as a Lua
/// chunk (`=expr` prints the result). This is the iteration loop while the
/// game is running: `tempest.reload()`, `=tempest.status().notes`, etc.
/// Chunks run on the game thread (queued until the next event) so calling
/// into the engine from the console is safe; if no event fires within 3 s
/// the chunk runs inline instead. Returns when stdin closes.
fn repl() {
    use std::io::BufRead;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    if !log::console_wanted() {
        return;
    }
    let run = |chunk: String| {
        if let Err(e) = lua().load(&chunk).set_name("=console").exec() {
            log::line("console", &e.to_string());
        }
    };
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let chunk = match line.strip_prefix('=') {
            Some(expr) => format!("print({expr})"),
            None => line.to_string(),
        };
        let claimed = Arc::new(AtomicBool::new(false));
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        {
            let (claimed, done, chunk) = (claimed.clone(), done.clone(), chunk.clone());
            unreal::queue(Box::new(move || {
                if claimed.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                    run(chunk);
                }
                *done.0.lock().unwrap() = true;
                done.1.notify_all();
            }));
        }
        let guard = done.0.lock().unwrap();
        let (_g, _) = done
            .1
            .wait_timeout_while(guard, std::time::Duration::from_secs(3), |finished| !*finished)
            .unwrap();
        if claimed.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
            log::line("console", "no game-thread event within 3 s; running inline");
            run(chunk);
        }
    }
}

#[cfg(windows)]
mod entry {
    use std::ffi::c_void;

    const DLL_PROCESS_ATTACH: u32 = 1;

    #[link(name = "kernel32")]
    extern "system" {
        fn DisableThreadLibraryCalls(hmodule: *mut c_void) -> i32;
    }

    /// Spawns the boot thread so nothing runs under the loader lock.
    #[no_mangle]
    pub unsafe extern "system" fn DllMain(
        hmodule: *mut c_void,
        reason: u32,
        _reserved: *mut c_void,
    ) -> i32 {
        if reason == DLL_PROCESS_ATTACH {
            DisableThreadLibraryCalls(hmodule);
            std::thread::spawn(|| {
                if std::panic::catch_unwind(super::boot).is_err() {
                    super::log::line("boot", "aborted by panic (see above)");
                }
            });
        }
        1
    }
}
