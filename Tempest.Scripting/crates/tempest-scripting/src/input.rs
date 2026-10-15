//! Keyboard input: a `WndProc` hook on the game's main window (Windows only).
//!
//! Installed lazily on the first `tempest.on_key`. Key-down messages (first
//! press only, not auto-repeat) reach the prelude's `tempest._keydown(vk,
//! ctrl, shift, alt)`, which dispatches to the registered bindings; a
//! binding returning `true` swallows the key. Dispatch happens right in the
//! window procedure when it runs on the game thread (UE3 pumps messages on
//! the thread that ticks the game), otherwise through the game-thread queue.

use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

static HWND: AtomicUsize = AtomicUsize::new(0);
#[cfg_attr(not(windows), allow(dead_code))]
static ORIGINAL: AtomicIsize = AtomicIsize::new(0);

pub fn installed() -> bool {
    HWND.load(Ordering::Acquire) != 0
}

/// Set by the first `tempest.on_key`; the hook itself is (re)installed from
/// the game thread by [`maintain`], because the real game window appears
/// well after scripts load (and may be recreated on video changes).
#[cfg_attr(not(windows), allow(dead_code))]
static WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ask for the hook. Idempotent; the actual install happens in `maintain`.
pub fn install() -> Result<(), String> {
    #[cfg(windows)]
    {
        WANTED.store(true, Ordering::Release);
        maintain(true);
        Ok(())
    }
    #[cfg(not(windows))]
    Err("host build".into())
}

/// Called on the game thread (throttled): hook the game window if wanted
/// and not yet hooked, or re-hook when the window changed or the game put
/// its own procedure back.
pub fn maintain(force: bool) {
    #[cfg(windows)]
    {
        use std::sync::atomic::AtomicU64;
        use std::time::{Duration, Instant};
        static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
        static WARNED: AtomicU64 = AtomicU64::new(0);
        if !WANTED.load(Ordering::Acquire) {
            return;
        }
        if !force {
            let mut last = LAST.lock().unwrap();
            if last.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
                return;
            }
            *last = Some(Instant::now());
        }
        let Some(hwnd) = find_game_window() else {
            if WARNED.fetch_add(1, Ordering::Relaxed) == 0 {
                crate::log::line("input", "no game window yet; will keep looking");
            }
            return;
        };
        let current = HWND.load(Ordering::Acquire);
        let ours = wndproc as *const () as isize;
        let live = unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) };
        if current == hwnd && live == ours {
            return;
        }
        unsafe {
            SetLastError(0);
            let prev = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours);
            if prev == 0 {
                if WARNED.fetch_add(1, Ordering::Relaxed) < 3 {
                    crate::log::line("input", &format!("SetWindowLongPtr failed on 0x{hwnd:x} (error {})", GetLastError()));
                }
                return;
            }
            // Never chain to ourselves if the game re-applied our proc.
            if prev != ours {
                ORIGINAL.store(prev, Ordering::Release);
            }
        }
        HWND.store(hwnd, Ordering::Release);
        crate::log::line(
            "input",
            &format!("WndProc hook installed on window 0x{hwnd:x} ({}){}", window_class(hwnd), if current != 0 { " [re-hooked]" } else { "" }),
        );
    }
    #[cfg(not(windows))]
    let _ = force;
}

#[cfg(windows)]
fn window_class(hwnd: usize) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

/// The UE3 game window (`LaunchUnrealUWindowsClient`), else the largest
/// visible top-level window of this process.
#[cfg(windows)]
fn find_game_window() -> Option<usize> {
    let mut best = Best { pid: std::process::id(), hwnd: 0, area: 0, class_hit: 0 };
    unsafe { EnumWindows(each_window, &mut best as *mut Best as isize) };
    if best.class_hit != 0 {
        Some(best.class_hit)
    } else {
        (best.hwnd != 0).then_some(best.hwnd)
    }
}

#[cfg(windows)]
struct Best {
    pid: u32,
    hwnd: usize,
    area: i64,
    class_hit: usize,
}

#[cfg(windows)]
unsafe extern "system" fn each_window(hwnd: usize, lparam: isize) -> i32 {
    let best = &mut *(lparam as *mut Best);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != best.pid || IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    if window_class(hwnd).starts_with("LaunchUnrealUWindowsClient") {
        best.class_hit = hwnd;
    }
    let mut r = [0i32; 4];
    if GetWindowRect(hwnd, r.as_mut_ptr()) != 0 {
        let area = (r[2] - r[0]) as i64 * (r[3] - r[1]) as i64;
        if area > best.area {
            best.area = area;
            best.hwnd = hwnd;
        }
    }
    1
}

#[cfg(windows)]
const GWLP_WNDPROC: i32 = -4;

#[cfg(windows)]
unsafe extern "system" fn wndproc(hwnd: usize, msg: u32, wparam: usize, lparam: isize) -> isize {
    // (vk, is_down, is_repeat) for the messages we forward; mouse buttons map
    // to their VK codes (1,2,4,5,6) and the wheel to synthetic 0xFF01/0xFF02.
    let event: Option<(u32, bool, bool)> = match msg {
        0x100 | 0x104 => Some((wparam as u32, true, lparam & (1 << 30) != 0)), // WM_KEYDOWN / WM_SYSKEYDOWN
        0x101 | 0x105 => Some((wparam as u32, false, false)),                   // WM_KEYUP / WM_SYSKEYUP
        0x201 => Some((1, true, false)),
        0x202 => Some((1, false, false)),
        0x204 => Some((2, true, false)),
        0x205 => Some((2, false, false)),
        0x207 => Some((4, true, false)),
        0x208 => Some((4, false, false)),
        0x20B | 0x20C => {
            let which = ((wparam >> 16) & 0xFFFF) as u32; // XBUTTON1 = 1, XBUTTON2 = 2
            Some((4 + which, msg == 0x20B, false))
        }
        0x20A => {
            let delta = ((wparam >> 16) & 0xFFFF) as u16 as i16;
            Some((if delta > 0 { 0xFF01 } else { 0xFF02 }, true, false))
        }
        _ => None,
    };
    if let Some((vk, is_down, is_repeat)) = event {
        let down = |k: i32| GetKeyState(k) as u16 & 0x8000 != 0;
        let (ctrl, shift, alt) = (down(0x11), down(0x10), down(0x12));
        if crate::unreal::on_game_thread() {
            if key_event(vk, ctrl, shift, alt, is_down, is_repeat) {
                return 0;
            }
        } else {
            crate::unreal::queue(Box::new(move || {
                key_event(vk, ctrl, shift, alt, is_down, is_repeat);
            }));
        }
    }
    let original = ORIGINAL.load(Ordering::Acquire);
    CallWindowProcW(original, hwnd, msg, wparam, lparam)
}

/// Hand the event to Lua; `true` = a binding swallowed it.
#[cfg_attr(not(windows), allow(dead_code))]
fn key_event(vk: u32, ctrl: bool, shift: bool, alt: bool, is_down: bool, is_repeat: bool) -> bool {
    let r = std::panic::catch_unwind(|| {
        let lua = crate::lua();
        let tempest: mlua::Table = lua.globals().get("tempest").ok()?;
        let f: mlua::Function = tempest.get("_key").ok()?;
        match f.call::<bool>((vk, ctrl, shift, alt, is_down, is_repeat)) {
            Ok(b) => Some(b),
            Err(e) => {
                crate::log::line("input", &format!("keydown handler failed: {e}"));
                None
            }
        }
    });
    matches!(r, Ok(Some(true)))
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn SetWindowLongPtrW(hwnd: usize, index: i32, value: isize) -> isize;
    fn GetWindowLongPtrW(hwnd: usize, index: i32) -> isize;
    fn GetClassNameW(hwnd: usize, buf: *mut u16, len: i32) -> i32;
    fn CallWindowProcW(prev: isize, hwnd: usize, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn GetKeyState(vk: i32) -> i16;
    fn EnumWindows(cb: unsafe extern "system" fn(usize, isize) -> i32, lparam: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: usize, pid: *mut u32) -> u32;
    fn IsWindowVisible(hwnd: usize) -> i32;
    fn GetWindowRect(hwnd: usize, rect: *mut i32) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn SetLastError(code: u32);
    fn GetLastError() -> u32;
}
