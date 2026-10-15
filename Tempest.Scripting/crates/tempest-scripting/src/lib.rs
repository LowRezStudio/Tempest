//! Tempest.Scripting — a small Lua modding DLL for Paladins 8.1 (UE3, Win64).
//!
//! Boot sequence, all on a worker thread spawned from `DllMain`:
//!
//! 1. [`log::init`] — `%TEMP%\tempest-scripting.log`, debugger output, and
//!    (unless `TEMPEST_SCRIPT_CONSOLE=0`) a console window.
//! 2. [`unreal::resolve`] — WolperMod's five Win64 signatures give GObjects,
//!    GNames, the engine slot, `ProcessEvent` and the anti-cheat XOR mask.
//!    Then it waits for the game to populate the arrays (`GNames[0] == "None"`).
//! 3. [`unreal::install_hook`] — detours `ProcessEvent`.
//! 4. [`unreal::wait_for_engine`] — waits for `*GEngine`, i.e. the Engine
//!    package is loaded (injection happens while only `Core` exists), then
//!    resolves `ProcessInternal` and detours it (script→script hooks).
//! 5. [`loader::run_all`] — runs every `scripts/*.lua`.
//!
//! Everything degrades to "unresolved + a note in `tempest.status()`" rather
//! than crashing; the host (macOS/Linux) build has no game image and is only
//! used for `cargo test`.

#[cfg(all(windows, not(target_pointer_width = "64")))]
compile_error!("Tempest.Scripting targets the Win64 client only");

pub mod input;
pub mod loader;
pub mod log;
pub mod lua_api;
pub mod memory;
pub mod tls;
pub mod unreal;

use std::sync::OnceLock;

use mlua::Lua;

static LUA: OnceLock<Lua> = OnceLock::new();

/// Process-wide Lua VM, created (and the API registered) on first use.
pub fn lua() -> &'static Lua {
    LUA.get_or_init(|| {
        let lua = Lua::new();
        if let Err(e) = lua_api::register(&lua) {
            log::line("lua", &format!("API registration failed: {e}"));
        }
        lua
    })
}

/// Full startup. Safe to call from the host build (everything stubs out).
pub fn boot() {
    log::init();
    if !claim_runtime() {
        log::line("boot", "another Tempest.Scripting instance already runs in this process; this copy stays idle");
        return;
    }
    log::line(
        "boot",
        &format!(
            "Tempest.Scripting {} pid={} log={}",
            env!("CARGO_PKG_VERSION"),
            std::process::id(),
            log::path().unwrap_or_default()
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
    loader::run_all(lua());
    log::line("boot", "ready");
    repl();
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
    if std::env::var("TEMPEST_SCRIPT_CONSOLE").as_deref() == Ok("0") {
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
