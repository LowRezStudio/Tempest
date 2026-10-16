//! In-game UI: Dear ImGui on the game's own D3D9 or D3D11 device.
//!
//! The native half (`native/overlay.cpp`) owns the ImGui context, the
//! renderer/platform backends and the vtable discovery; this module owns the
//! detours (`IDXGISwapChain::Present`/`ResizeBuffers` or
//! `IDirect3DDevice9::Present`/`Reset`), the game-thread frame cadence and
//! the bridge to the Lua `ui` module.
//!
//! Every Present asks for one frame; the game thread builds it at its next
//! `ProcessEvent` (`maybe_build_frame` from the queue drain), running the Lua
//! draw handlers where game objects are safe to touch. The render thread
//! then draws the parked copy of that frame, so Lua never runs on the render
//! thread and the render thread never waits on Lua.

use std::ffi::{c_char, c_void, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};

use mlua::Function;

extern "C" {
    fn tempest_overlay_set_log(f: extern "C" fn(*const c_char));
    fn tempest_overlay_backend() -> *const c_char;
    fn tempest_overlay_ready() -> i32;
    fn tempest_overlay_hwnd() -> *mut c_void;
    fn tempest_overlay_stats(built: *mut u32, rendered: *mut u32, vertices: *mut i32, lists: *mut i32);
    fn tempest_overlay_set_capture(on: i32);
    fn tempest_overlay_get_capture() -> i32;
    fn tempest_overlay_want(mouse: *mut i32, keyboard: *mut i32, text: *mut i32);
    fn tempest_overlay_set_scale(s: f32);
    fn tempest_overlay_get_scale() -> f32;
    fn tempest_overlay_run_handler(cb: extern "C" fn(*mut c_void) -> i32, ud: *mut c_void) -> i32;
    fn tempest_overlay_build_frame(draw: extern "C" fn(*mut c_void), ud: *mut c_void) -> i32;
    fn tempest_overlay_init_headless(w: f32, h: f32) -> i32;
    fn tempest_overlay_headless_upload() -> i32;
    fn tempest_overlay_set_ini(path: *const c_char);
    fn tempest_overlay_add_font(path: *const c_char, size: f32, merge: i32, ranges: *const u16) -> *mut c_void;
    fn tempest_overlay_set_default_font(font: *mut c_void);
    fn tempest_overlay_fonts_dirty() -> i32;
    fn tempest_overlay_load_image_file(path: *const c_char, w: *mut i32, h: *mut i32) -> i32;
    fn tempest_overlay_load_image_rgba(rgba: *const u8, w: i32, h: i32) -> i32;
    fn tempest_overlay_texture_id(id: i32) -> u64;
    fn tempest_overlay_free_texture(id: i32);
    #[cfg_attr(not(windows), allow(dead_code))]
    fn tempest_overlay_wndproc(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> i32;
    #[cfg_attr(not(windows), allow(dead_code))]
    fn tempest_overlay_find_dxgi(present: *mut *mut c_void, resize: *mut *mut c_void) -> i32;
    #[cfg_attr(not(windows), allow(dead_code))]
    fn tempest_overlay_find_d3d9(present: *mut *mut c_void, reset: *mut *mut c_void) -> i32;
    #[cfg_attr(not(windows), allow(dead_code))]
    fn tempest_overlay_present11(swapchain: *mut c_void, flags: u32);
    #[cfg_attr(not(windows), allow(dead_code))]
    fn tempest_overlay_resize11();
    #[cfg_attr(not(windows), allow(dead_code))]
    fn tempest_overlay_present9(device: *mut c_void, hwnd_fallback: *mut c_void);
    #[cfg_attr(not(windows), allow(dead_code))]
    fn tempest_overlay_reset9_before();
}

extern "C" fn log_sink(msg: *const c_char) {
    if msg.is_null() {
        return;
    }
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    crate::log::line("ui", &text);
}

/// Route native log lines into the log file. Called once from `lua()`.
pub fn init() {
    unsafe { tempest_overlay_set_log(log_sink) };
}

pub fn ready() -> bool {
    unsafe { tempest_overlay_ready() != 0 }
}

pub fn backend() -> String {
    let p = unsafe { tempest_overlay_backend() };
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

pub fn hwnd() -> usize {
    unsafe { tempest_overlay_hwnd() as usize }
}

/// `(frames built, frames rendered, vertices in the parked frame, draw lists)`.
pub fn stats() -> (u32, u32, i32, i32) {
    let (mut b, mut r, mut v, mut l) = (0u32, 0u32, 0i32, 0i32);
    unsafe { tempest_overlay_stats(&mut b, &mut r, &mut v, &mut l) };
    (b, r, v, l)
}

pub fn set_capture(on: bool) {
    unsafe { tempest_overlay_set_capture(on as i32) };
}

pub fn capturing() -> bool {
    unsafe { tempest_overlay_get_capture() != 0 }
}

/// ImGui's `(WantCaptureMouse, WantCaptureKeyboard, WantTextInput)`.
pub fn want() -> (bool, bool, bool) {
    let (mut m, mut k, mut t) = (0i32, 0i32, 0i32);
    unsafe { tempest_overlay_want(&mut m, &mut k, &mut t) };
    (m != 0, k != 0, t != 0)
}

pub fn set_scale(s: f32) {
    unsafe { tempest_overlay_set_scale(s) };
}

pub fn scale() -> f32 {
    unsafe { tempest_overlay_get_scale() }
}

/// Host tests: an ImGui context with no backend, frames build and park.
pub fn init_headless(w: f32, h: f32) -> bool {
    unsafe { tempest_overlay_init_headless(w, h) != 0 }
}

/// Host tests: mark pending textures as uploaded. Returns how many.
pub fn headless_upload() -> i32 {
    unsafe { tempest_overlay_headless_upload() }
}

/// Where ImGui persists window layouts (`""` = nowhere).
pub fn set_ini(path: &str) {
    let c = std::ffi::CString::new(path).unwrap_or_default();
    unsafe { tempest_overlay_set_ini(c.as_ptr()) };
}

/// Add a TTF (`path`), or the built-in font when `path` is empty, at `size`
/// pixels; `ranges` are (first, last) glyph pairs. Returns the `ImFont*`,
/// 0 when deferred (called mid-frame) or failed.
pub fn add_font(path: &str, size: f32, merge: bool, ranges: &[u16]) -> usize {
    let c = std::ffi::CString::new(path).unwrap_or_default();
    let mut r: Vec<u16> = ranges.to_vec();
    r.push(0);
    r.push(0);
    unsafe { tempest_overlay_add_font(c.as_ptr(), size, merge as i32, if ranges.is_empty() { std::ptr::null() } else { r.as_ptr() }) as usize }
}

pub fn set_default_font(font: usize) {
    unsafe { tempest_overlay_set_default_font(font as *mut c_void) };
}

pub fn fonts_dirty() -> bool {
    unsafe { tempest_overlay_fonts_dirty() != 0 }
}

/// Decode an image file; `(id, width, height)` or `None`.
pub fn load_image(path: &str) -> Option<(i32, i32, i32)> {
    let c = std::ffi::CString::new(path).ok()?;
    let (mut w, mut h) = (0i32, 0i32);
    let id = unsafe { tempest_overlay_load_image_file(c.as_ptr(), &mut w, &mut h) };
    (id > 0).then_some((id, w, h))
}

/// Raw RGBA8 pixels; `id` or `None`.
pub fn load_image_rgba(rgba: &[u8], w: i32, h: i32) -> Option<i32> {
    if w <= 0 || h <= 0 || rgba.len() < (w as usize) * (h as usize) * 4 {
        return None;
    }
    let id = unsafe { tempest_overlay_load_image_rgba(rgba.as_ptr(), w, h) };
    (id > 0).then_some(id)
}

pub fn texture_id(id: i32) -> u64 {
    unsafe { tempest_overlay_texture_id(id) }
}

pub fn free_texture(id: i32) {
    unsafe { tempest_overlay_free_texture(id) };
}

// ---------------------------------------------------------------------------
// Frame cadence
// ---------------------------------------------------------------------------

static FRAME_WANTED: AtomicBool = AtomicBool::new(false);

/// Game thread, every event: build one frame if a Present asked for one.
pub fn maybe_build_frame() {
    if !FRAME_WANTED.swap(false, Ordering::AcqRel) {
        return;
    }
    build_frame();
}

/// Build a frame right now on the calling thread (the game thread, or a
/// test). `false` when the backend is not ready or a frame is in progress.
pub fn build_frame() -> bool {
    unsafe { tempest_overlay_build_frame(draw_trampoline, std::ptr::null_mut()) != 0 }
}

/// Inside NewFrame/Render: run `ui._frame()`, which runs every draw handler.
extern "C" fn draw_trampoline(_: *mut c_void) {
    let _ = catch_unwind(|| {
        let lua = crate::lua();
        let Ok(ui) = lua.globals().get::<mlua::Table>("ui") else { return };
        let Ok(frame) = ui.get::<Function>("_frame") else { return };
        if let Err(e) = frame.call::<()>(()) {
            crate::log::line("ui", &format!("ui._frame failed: {e}"));
        }
    });
}

struct HandlerCall<'a> {
    f: &'a Function,
    owner: &'a str,
    err: Option<String>,
}

extern "C" fn handler_trampoline(ud: *mut c_void) -> i32 {
    // SAFETY: `ud` is the `HandlerCall` that `run_handler` built on its stack
    // and only lives for the duration of this call.
    let call = unsafe { &mut *(ud as *mut HandlerCall) };
    let r = catch_unwind(AssertUnwindSafe(|| crate::loader::with_mod(call.owner, || call.f.call::<()>(()))));
    match r {
        Ok(Ok(())) => 1,
        Ok(Err(e)) => {
            call.err = Some(e.to_string());
            0
        }
        Err(_) => {
            call.err = Some("panic in draw handler".into());
            0
        }
    }
}

/// Run one Lua draw handler under ImGui's error recovery (so a handler that
/// raised mid-window does not leave Begin/End unbalanced). The error text,
/// if any, goes back to Lua for per-handler reporting.
pub fn run_handler(f: &Function, owner: &str) -> Option<String> {
    let mut call = HandlerCall { f, owner, err: None };
    unsafe { tempest_overlay_run_handler(handler_trampoline, &mut call as *mut HandlerCall as *mut c_void) };
    call.err
}

/// Window procedure hook: feed ImGui; `true` when the UI swallows the message.
#[cfg(windows)]
pub fn wndproc(hwnd: usize, msg: u32, wparam: usize, lparam: isize) -> bool {
    catch_unwind(|| unsafe { tempest_overlay_wndproc(hwnd as *mut c_void, msg, wparam, lparam) != 0 }).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Detours (Windows only)
// ---------------------------------------------------------------------------

/// Hook whichever of D3D9 / D3D11 the game has loaded, polling until one of
/// them appears (the DLL is injected before the renderer initialises).
/// Returns immediately; progress goes to the log under `ui`.
pub fn install_async() {
    #[cfg(windows)]
    std::thread::spawn(|| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        let mut logged_wait = false;
        loop {
            let (dx11, dx9) = (hooks::try_dxgi(), hooks::try_d3d9());
            if dx11 || dx9 {
                crate::log::line("ui", &format!("render hooks installed: d3d11={dx11} d3d9={dx9}; the backend initialises on the first Present"));
                // ImGui needs the window messages even if no mod binds a key.
                crate::input::request();
                return;
            }
            if std::time::Instant::now() > deadline {
                crate::log::line("ui", "no D3D9/D3D11 device could be hooked within 180 s; UI disabled");
                return;
            }
            if !logged_wait {
                logged_wait = true;
                crate::log::line("ui", "waiting for d3d9.dll or d3d11.dll to load");
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

#[cfg(windows)]
mod hooks {
    use super::*;
    use std::sync::OnceLock;

    type Present11 = extern "system" fn(*mut c_void, u32, u32) -> i32;
    type Resize11 = extern "system" fn(*mut c_void, u32, u32, u32, u32, u32) -> i32;
    type Present9 = extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut c_void, *const c_void) -> i32;
    type Reset9 = extern "system" fn(*mut c_void, *mut c_void) -> i32;

    static PRESENT11: OnceLock<retour::GenericDetour<Present11>> = OnceLock::new();
    static RESIZE11: OnceLock<retour::GenericDetour<Resize11>> = OnceLock::new();
    static PRESENT9: OnceLock<retour::GenericDetour<Present9>> = OnceLock::new();
    static RESET9: OnceLock<retour::GenericDetour<Reset9>> = OnceLock::new();

    extern "system" fn present11(sc: *mut c_void, sync: u32, flags: u32) -> i32 {
        let Some(h) = PRESENT11.get() else { return 0 };
        let _ = catch_unwind(|| unsafe { tempest_overlay_present11(sc, flags) });
        FRAME_WANTED.store(true, Ordering::Release);
        h.call(sc, sync, flags)
    }

    extern "system" fn resize11(sc: *mut c_void, count: u32, w: u32, h: u32, format: u32, flags: u32) -> i32 {
        let Some(d) = RESIZE11.get() else { return 0 };
        let _ = catch_unwind(|| unsafe { tempest_overlay_resize11() });
        d.call(sc, count, w, h, format, flags)
    }

    extern "system" fn present9(dev: *mut c_void, src: *const c_void, dst: *const c_void, hwnd: *mut c_void, dirty: *const c_void) -> i32 {
        let Some(h) = PRESENT9.get() else { return 0 };
        // Only the first call (backend init) needs a window to fall back on.
        let fallback = if ready() { std::ptr::null_mut() } else { crate::input::game_window().unwrap_or(0) as *mut c_void };
        let _ = catch_unwind(|| unsafe { tempest_overlay_present9(dev, fallback) });
        FRAME_WANTED.store(true, Ordering::Release);
        h.call(dev, src, dst, hwnd, dirty)
    }

    extern "system" fn reset9(dev: *mut c_void, params: *mut c_void) -> i32 {
        let Some(h) = RESET9.get() else { return 0 };
        let _ = catch_unwind(|| unsafe { tempest_overlay_reset9_before() });
        h.call(dev, params)
    }

    fn detour<F: retour::Function>(target: *mut c_void, hook: F, slot: &OnceLock<retour::GenericDetour<F>>, what: &str) -> bool {
        if slot.get().is_some() {
            return true;
        }
        // SAFETY: `target` is a vtable entry read from a live device of the
        // same interface; the hook shares its calling convention.
        let original: F = unsafe { std::mem::transmute_copy(&target) };
        let d = match unsafe { retour::GenericDetour::new(original, hook) } {
            Ok(d) => d,
            Err(e) => {
                crate::log::line("ui", &format!("cannot detour {what} at 0x{:x}: {e}", target as usize));
                return false;
            }
        };
        if let Err(e) = unsafe { d.enable() } {
            crate::log::line("ui", &format!("cannot enable {what} detour: {e}"));
            return false;
        }
        crate::log::line("ui", &format!("{what} detour live at 0x{:x}", target as usize));
        slot.set(d).is_ok()
    }

    pub fn try_dxgi() -> bool {
        if PRESENT11.get().is_some() {
            return true;
        }
        let (mut present, mut resize) = (std::ptr::null_mut(), std::ptr::null_mut());
        if unsafe { tempest_overlay_find_dxgi(&mut present, &mut resize) } == 0 || present.is_null() {
            return false;
        }
        detour(present, present11 as Present11, &PRESENT11, "IDXGISwapChain::Present")
            && detour(resize, resize11 as Resize11, &RESIZE11, "IDXGISwapChain::ResizeBuffers")
    }

    pub fn try_d3d9() -> bool {
        if PRESENT9.get().is_some() {
            return true;
        }
        let (mut present, mut reset) = (std::ptr::null_mut(), std::ptr::null_mut());
        if unsafe { tempest_overlay_find_d3d9(&mut present, &mut reset) } == 0 || present.is_null() {
            return false;
        }
        detour(present, present9 as Present9, &PRESENT9, "IDirect3DDevice9::Present")
            && detour(reset, reset9 as Reset9, &RESET9, "IDirect3DDevice9::Reset")
    }
}
