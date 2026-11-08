//! A picture of a Studio window for the MCP server's `studio_screenshot`,
//! taken by the OS: `screencapture -l` on macOS, `PrintWindow` on Windows.
//! Linux has none yet.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// The OS's id of a window: its window number on macOS, its HWND on Windows.
pub fn window_id(window: &gpui::Window) -> Option<i64> {
    let h = HasWindowHandle::window_handle(window).ok()?;
    match h.as_raw() {
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(h) => {
            use objc2::{msg_send, runtime::AnyObject};
            let view = h.ns_view.as_ptr() as *mut AnyObject;
            // SAFETY: the view is the window's live NSView, asked on the UI thread.
            unsafe {
                let win: *mut AnyObject = msg_send![&*view, window];
                if win.is_null() {
                    return None;
                }
                let n: isize = msg_send![&*win, windowNumber];
                Some(n as i64)
            }
        }
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as i64),
        _ => None,
    }
}

/// The window as a PNG, scaled down to `max_width` pixels when wider; with its size.
pub fn capture(id: i64, max_width: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let img = grab(id)?;
    let img = if max_width > 0 && img.width() > max_width {
        let h = (img.height() as u64 * max_width as u64 / img.width() as u64).max(1) as u32;
        image::imageops::resize(&img, max_width, h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img.clone()).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok((png, img.width(), img.height()))
}

#[cfg(target_os = "macos")]
fn grab(id: i64) -> Result<image::RgbaImage, String> {
    let file = std::env::temp_dir().join(format!("tempest-studio-shot-{}-{id}.png", std::process::id()));
    let out = std::process::Command::new("screencapture").args(["-x", "-o", "-t", "png", &format!("-l{id}")]).arg(&file).output().map_err(|e| format!("screencapture: {e}"))?;
    let bytes = std::fs::read(&file).map_err(|_| format!("screencapture made no picture ({}); the app may need the Screen Recording permission", String::from_utf8_lossy(&out.stderr).trim()))?;
    let _ = std::fs::remove_file(&file);
    image::load_from_memory(&bytes).map(|i| i.to_rgba8()).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn grab(id: i64) -> Result<image::RgbaImage, String> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
    use windows_sys::Win32::Storage::Xps::PrintWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;
    let hwnd = id as isize as windows_sys::Win32::Foundation::HWND;
    // SAFETY: plain GDI calls on the Studio's own window; every handle made here is freed here.
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut r) == 0 {
            return Err("the window has no size".into());
        }
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        if w <= 0 || h <= 0 {
            return Err("the window has no size".into());
        }
        let screen = GetDC(hwnd);
        let dc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(dc, bmp);
        // PW_CLIENTONLY | PW_RENDERFULLCONTENT: the DirectX content too.
        let ok = PrintWindow(hwnd, dc, 1 | 2);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, ..std::mem::zeroed() };
        let mut px = vec![0u8; w as usize * h as usize * 4];
        let rows = GetDIBits(dc, bmp, 0, h as u32, px.as_mut_ptr().cast(), &mut info, DIB_RGB_COLORS);
        SelectObject(dc, old);
        DeleteObject(bmp);
        DeleteDC(dc);
        ReleaseDC(hwnd, screen);
        if ok == 0 || rows == 0 {
            return Err("PrintWindow failed".into());
        }
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        image::RgbaImage::from_raw(w as u32, h as u32, px).ok_or_else(|| "bad picture size".to_string())
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn grab(_id: i64) -> Result<image::RgbaImage, String> {
    Err("screenshots are not supported on this system yet".into())
}
