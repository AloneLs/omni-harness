use base64::Engine;
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub hwnd: usize,
    pub title: String,
    pub class_name: String,
    pub is_visible: bool,
    pub bounds: RectBounds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlInfo {
    pub hwnd: usize,
    pub parent_hwnd: usize,
    pub text: String,
    pub class_name: String,
    pub bounds: RectBounds,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RectBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureResult {
    pub width: u32,
    pub height: u32,
    pub base64_image: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyAction {
    pub vk_code: u16,
    pub is_up: bool,
    pub is_unicode: bool,
    pub unicode_char: Option<char>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseAction {
    pub x: i32,
    pub y: i32,
    pub move_cursor: bool,
    pub click_button: Option<MouseButton>,
    pub is_down: Option<bool>,
    pub scroll_delta: Option<i32>,
}

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;

    pub type HWND = *mut c_void;
    pub type HDC = *mut c_void;
    pub type HBITMAP = *mut c_void;
    pub type HGDIOBJ = *mut c_void;
    pub type HGLOBAL = *mut c_void;
    pub type BOOL = i32;
    pub type DWORD = u32;
    pub type WORD = u16;
    pub type UINT = u32;
    pub type LONG = i32;
    pub type LPARAM = isize;
    pub type WPARAM = usize;
    pub type LRESULT = isize;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct RECT {
        pub left: LONG,
        pub top: LONG,
        pub right: LONG,
        pub bottom: LONG,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    #[allow(non_snake_case)]
    pub struct MOUSEINPUT {
        pub dx: LONG,
        pub dy: LONG,
        pub mouseData: DWORD,
        pub dwFlags: DWORD,
        pub time: DWORD,
        pub dwExtraInfo: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    #[allow(non_snake_case)]
    pub struct KEYBDINPUT {
        pub wVk: WORD,
        pub wScan: WORD,
        pub dwFlags: DWORD,
        pub time: DWORD,
        pub dwExtraInfo: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    #[allow(non_snake_case)]
    pub struct HARDWAREINPUT {
        pub uMsg: DWORD,
        pub wParamL: WORD,
        pub wParamH: WORD,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub union INPUT_UNION {
        pub mi: MOUSEINPUT,
        pub ki: KEYBDINPUT,
        pub hi: HARDWAREINPUT,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct INPUT {
        pub type_: DWORD,
        pub u: INPUT_UNION,
    }

    pub const INPUT_MOUSE: DWORD = 0;
    pub const INPUT_KEYBOARD: DWORD = 1;

    pub const MOUSEEVENTF_MOVE: DWORD = 0x0001;
    pub const MOUSEEVENTF_LEFTDOWN: DWORD = 0x0002;
    pub const MOUSEEVENTF_LEFTUP: DWORD = 0x0004;
    pub const MOUSEEVENTF_RIGHTDOWN: DWORD = 0x0008;
    pub const MOUSEEVENTF_RIGHTUP: DWORD = 0x0010;
    pub const MOUSEEVENTF_MIDDLEDOWN: DWORD = 0x0020;
    pub const MOUSEEVENTF_MIDDLEUP: DWORD = 0x0040;
    pub const MOUSEEVENTF_WHEEL: DWORD = 0x0800;
    pub const MOUSEEVENTF_VIRTUALDESK: DWORD = 0x4000;
    pub const MOUSEEVENTF_ABSOLUTE: DWORD = 0x8000;
    pub const SM_XVIRTUALSCREEN: i32 = 76;
    pub const SM_YVIRTUALSCREEN: i32 = 77;
    pub const SM_CXVIRTUALSCREEN: i32 = 78;
    pub const SM_CYVIRTUALSCREEN: i32 = 79;

    pub const KEYEVENTF_KEYUP: DWORD = 0x0002;
    pub const KEYEVENTF_UNICODE: DWORD = 0x0004;

    pub const SRCCOPY: DWORD = 0x00CC0020;
    pub const PW_RENDERFULLCONTENT: DWORD = 0x00000002;
    pub const CF_UNICODETEXT: UINT = 13;
    pub const CF_DIB: UINT = 8;
    pub const GMEM_MOVEABLE: UINT = 0x0002;

    pub const WM_SETTEXT: UINT = 0x000C;
    pub const BM_CLICK: UINT = 0x00F5;

    #[repr(C)]
    #[allow(non_snake_case)]
    pub struct BITMAPINFOHEADER {
        pub biSize: DWORD,
        pub biWidth: LONG,
        pub biHeight: LONG,
        pub biPlanes: WORD,
        pub biBitCount: WORD,
        pub biCompression: DWORD,
        pub biSizeImage: DWORD,
        pub biXPelsPerMeter: LONG,
        pub biYPelsPerMeter: LONG,
        pub biClrUsed: DWORD,
        pub biClrImportant: DWORD,
    }

    #[repr(C)]
    #[allow(non_snake_case)]
    pub struct BITMAPINFO {
        pub bmiHeader: BITMAPINFOHEADER,
        pub bmiColors: [u32; 1],
    }

    extern "system" {
        pub fn EnumWindows(lpEnumFunc: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>, lParam: LPARAM) -> BOOL;
        pub fn EnumChildWindows(hWndParent: HWND, lpEnumFunc: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>, lParam: LPARAM) -> BOOL;
        pub fn GetWindowTextW(hWnd: HWND, lpString: *mut u16, nMaxCount: i32) -> i32;
        pub fn GetClassNameW(hWnd: HWND, lpClassName: *mut u16, nMaxCount: i32) -> i32;
        pub fn IsWindowVisible(hWnd: HWND) -> BOOL;
        pub fn GetWindowRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
        pub fn SetForegroundWindow(hWnd: HWND) -> BOOL;
        pub fn SendMessageW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        pub fn SendInput(cInputs: UINT, pInputs: *const INPUT, cbSize: i32) -> UINT;
        pub fn GetSystemMetrics(nIndex: i32) -> i32;
        pub fn SetProcessDPIAware() -> BOOL;
        pub fn GetAsyncKeyState(vKey: i32) -> i16;
        pub fn GetDC(hWnd: HWND) -> HDC;
        pub fn ReleaseDC(hWnd: HWND, hDC: HDC) -> i32;
        pub fn CreateCompatibleDC(hDC: HDC) -> HDC;
        pub fn DeleteDC(hDC: HDC) -> BOOL;
        pub fn CreateCompatibleBitmap(hDC: HDC, cx: i32, cy: i32) -> HBITMAP;
        pub fn SelectObject(hDC: HDC, h: HGDIOBJ) -> HGDIOBJ;
        pub fn DeleteObject(ho: HGDIOBJ) -> BOOL;
        pub fn BitBlt(hdc: HDC, x: i32, y: i32, cx: i32, cy: i32, hdcSrc: HDC, x1: i32, y1: i32, rop: DWORD) -> BOOL;
        pub fn PrintWindow(hwnd: HWND, hdcBlt: HDC, nFlags: UINT) -> BOOL;
        pub fn OpenClipboard(hWndNewOwner: HWND) -> BOOL;
        pub fn CloseClipboard() -> BOOL;
        pub fn EmptyClipboard() -> BOOL;
        pub fn GetClipboardData(uFormat: UINT) -> *mut c_void;
        pub fn SetClipboardData(uFormat: UINT, hMem: *mut c_void) -> *mut c_void;
        pub fn GlobalAlloc(uFlags: UINT, dwBytes: usize) -> HGLOBAL;
        pub fn GlobalLock(hMem: HGLOBAL) -> *mut c_void;
        pub fn GlobalUnlock(hMem: HGLOBAL) -> BOOL;
        pub fn GlobalSize(hMem: HGLOBAL) -> usize;
        pub fn GetDIBits(hdc: HDC, hbm: HBITMAP, start: UINT, cLines: UINT, lpvBits: *mut c_void, lpbmi: *mut BITMAPINFO, usage: UINT) -> i32;
    }
}

#[cfg(windows)]
unsafe extern "system" fn enum_windows_callback(hwnd: win32::HWND, lparam: win32::LPARAM) -> win32::BOOL {
    let list = &mut *(lparam as *mut Vec<WindowInfo>);
    let mut title_buf = [0u16; 512];
    let len = win32::GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 512);
    let title = String::from_utf16_lossy(&title_buf[..len as usize]);

    let mut class_buf = [0u16; 256];
    let class_len = win32::GetClassNameW(hwnd, class_buf.as_mut_ptr(), 256);
    let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);

    let is_visible = win32::IsWindowVisible(hwnd) != 0;
    let mut rect = win32::RECT::default();
    win32::GetWindowRect(hwnd, &mut rect);

    list.push(WindowInfo {
        hwnd: hwnd as usize,
        title,
        class_name,
        is_visible,
        bounds: RectBounds {
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        },
    });
    1
}

#[cfg(windows)]
unsafe extern "system" fn enum_child_callback(hwnd: win32::HWND, lparam: win32::LPARAM) -> win32::BOOL {
    let list = &mut *(lparam as *mut Vec<ControlInfo>);
    let mut title_buf = [0u16; 512];
    let len = win32::GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 512);
    let text = String::from_utf16_lossy(&title_buf[..len as usize]);

    let mut class_buf = [0u16; 256];
    let class_len = win32::GetClassNameW(hwnd, class_buf.as_mut_ptr(), 256);
    let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);

    let mut rect = win32::RECT::default();
    win32::GetWindowRect(hwnd, &mut rect);

    list.push(ControlInfo {
        hwnd: hwnd as usize,
        parent_hwnd: 0,
        text,
        class_name,
        bounds: RectBounds {
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        },
    });
    1
}

pub fn find_windows(title_filter: Option<&str>, class_filter: Option<&str>) -> Vec<WindowInfo> {
    #[cfg(windows)]
    unsafe {
        let mut list: Vec<WindowInfo> = Vec::new();
        win32::EnumWindows(
            Some(enum_windows_callback),
            &mut list as *mut Vec<WindowInfo> as win32::LPARAM,
        );

        let t_filter = title_filter.map(|t| t.to_lowercase());
        let c_filter = class_filter.map(|c| c.to_lowercase());

        list.into_iter()
            .filter(|w| {
                if let Some(ref t) = t_filter {
                    if !w.title.to_lowercase().contains(t) {
                        return false;
                    }
                }
                if let Some(ref c) = c_filter {
                    if !w.class_name.to_lowercase().contains(c) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

pub fn find_controls(parent_hwnd: usize, text_filter: Option<&str>) -> Vec<ControlInfo> {
    #[cfg(windows)]
    unsafe {
        let mut list: Vec<ControlInfo> = Vec::new();
        win32::EnumChildWindows(
            parent_hwnd as win32::HWND,
            Some(enum_child_callback),
            &mut list as *mut Vec<ControlInfo> as win32::LPARAM,
        );

        for item in &mut list {
            item.parent_hwnd = parent_hwnd;
        }

        let f = text_filter.map(|s| s.to_lowercase());
        list.into_iter()
            .filter(|c| {
                if let Some(ref filter) = f {
                    c.text.to_lowercase().contains(filter)
                } else {
                    true
                }
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

pub fn click_control(hwnd: usize) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        let h = hwnd as win32::HWND;
        win32::SetForegroundWindow(h);
        win32::SendMessageW(h, win32::BM_CLICK, 0, 0);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Unsupported platform".to_string())
    }
}

pub fn set_control_text(hwnd: usize, text: &str) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        let h = hwnd as win32::HWND;
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        win32::SendMessageW(h, win32::WM_SETTEXT, 0, wide.as_ptr() as win32::LPARAM);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Unsupported platform".to_string())
    }
}

fn encode_bmp_32bpp(width: i32, height: i32, bgra_pixels: &[u8]) -> Vec<u8> {
    let header_size = 54u32;
    let image_size = (width * height * 4) as u32;
    let file_size = header_size + image_size;

    let mut out = Vec::with_capacity(file_size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&header_size.to_le_bytes());

    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&(-height).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&image_size.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    out.extend_from_slice(bgra_pixels);
    out
}

#[cfg(windows)]
fn ensure_dpi_aware() {
    static DPI_ONCE: std::sync::Once = std::sync::Once::new();
    DPI_ONCE.call_once(|| unsafe {
        win32::SetProcessDPIAware();
    });
}

#[cfg(windows)]
fn is_panic_hotkey_pressed() -> bool {
    unsafe {
        (win32::GetAsyncKeyState(0x11) as u16 & 0x8000) != 0
            && (win32::GetAsyncKeyState(0x12) as u16 & 0x8000) != 0
            && (win32::GetAsyncKeyState(0x10) as u16 & 0x8000) != 0
            && (win32::GetAsyncKeyState(0x1B) as u16 & 0x8000) != 0
    }
}

fn encode_png_rgba(width: u32, height: u32, bgra_pixels: &[u8]) -> Result<Vec<u8>, String> {
    let mut rgba = Vec::with_capacity(bgra_pixels.len());
    for chunk in bgra_pixels.chunks_exact(4) {
        rgba.push(chunk[2]);
        rgba.push(chunk[1]);
        rgba.push(chunk[0]);
        rgba.push(255);
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

pub fn capture_screen(target_hwnd: Option<usize>) -> Result<CaptureResult, String> {
    #[cfg(windows)]
    unsafe {
        ensure_dpi_aware();
        if is_panic_hotkey_pressed() {
            return Err("Computer use aborted by panic hotkey (Ctrl+Alt+Shift+Esc).".to_string());
        }

        let (hwnd, win_x, win_y, width, height) = match target_hwnd {
            Some(h) => {
                let win_h = h as win32::HWND;
                let mut r = win32::RECT::default();
                win32::GetWindowRect(win_h, &mut r);
                let w = (r.right - r.left).max(1);
                let h_val = (r.bottom - r.top).max(1);
                (win_h, r.left, r.top, w, h_val)
            }
            None => {
                let vx = win32::GetSystemMetrics(win32::SM_XVIRTUALSCREEN);
                let vy = win32::GetSystemMetrics(win32::SM_YVIRTUALSCREEN);
                let vw = win32::GetSystemMetrics(win32::SM_CXVIRTUALSCREEN).max(1);
                let vh = win32::GetSystemMetrics(win32::SM_CYVIRTUALSCREEN).max(1);
                (null_mut(), vx, vy, vw, vh)
            }
        };

        let hdc_screen = win32::GetDC(null_mut());
        if hdc_screen.is_null() {
            return Err("Failed to get screen DC".to_string());
        }

        let hdc_mem = win32::CreateCompatibleDC(hdc_screen);
        let hbm = win32::CreateCompatibleBitmap(hdc_screen, width, height);
        let old_obj = win32::SelectObject(hdc_mem, hbm);

        let mut captured_via_print = false;
        if !hwnd.is_null() {
            let res = win32::PrintWindow(hwnd, hdc_mem, win32::PW_RENDERFULLCONTENT);
            captured_via_print = res != 0;
        }
        if !captured_via_print {
            win32::BitBlt(hdc_mem, 0, 0, width, height, hdc_screen, win_x, win_y, win32::SRCCOPY);
        }

        let mut bmi = win32::BITMAPINFO {
            bmiHeader: win32::BITMAPINFOHEADER {
                biSize: size_of::<win32::BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: (width * height * 4) as u32,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [0; 1],
        };

        let mut pixel_buf: Vec<u8> = vec![0u8; (width * height * 4) as usize];
        win32::GetDIBits(
            hdc_mem,
            hbm,
            0,
            height as u32,
            pixel_buf.as_mut_ptr() as *mut c_void,
            &mut bmi,
            0,
        );

        if captured_via_print {
            let all_black = pixel_buf.chunks_exact(4).step_by(64).all(|c| c[0] == 0 && c[1] == 0 && c[2] == 0);
            if all_black {
                win32::BitBlt(hdc_mem, 0, 0, width, height, hdc_screen, win_x, win_y, win32::SRCCOPY);
                win32::GetDIBits(
                    hdc_mem,
                    hbm,
                    0,
                    height as u32,
                    pixel_buf.as_mut_ptr() as *mut c_void,
                    &mut bmi,
                    0,
                );
            }
        }

        win32::SelectObject(hdc_mem, old_obj);
        win32::DeleteObject(hbm);
        win32::DeleteDC(hdc_mem);
        win32::ReleaseDC(null_mut(), hdc_screen);

        let (bytes, mime_type) = match encode_png_rgba(width as u32, height as u32, &pixel_buf) {
            Ok(png_bytes) => (png_bytes, "image/png".to_string()),
            Err(_) => (encode_bmp_32bpp(width, height, &pixel_buf), "image/bmp".to_string()),
        };
        let base64_image = base64::engine::general_purpose::STANDARD.encode(&bytes);

        Ok(CaptureResult {
            width: width as u32,
            height: height as u32,
            base64_image,
            mime_type,
        })
    }
    #[cfg(not(windows))]
    {
        Err("Screen capture only implemented on Windows".to_string())
    }
}

pub fn send_keyboard_chords(keys: &[KeyAction]) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        if is_panic_hotkey_pressed() {
            return Err("Computer use aborted by panic hotkey (Ctrl+Alt+Shift+Esc).".to_string());
        }
        let mut inputs: Vec<win32::INPUT> = Vec::with_capacity(keys.len());

        for action in keys {
            let mut dw_flags = 0u32;
            if action.is_up {
                dw_flags |= win32::KEYEVENTF_KEYUP;
            }
            if action.is_unicode {
                dw_flags |= win32::KEYEVENTF_UNICODE;
            }

            let scan = if let Some(ch) = action.unicode_char {
                ch as u16
            } else {
                0
            };

            let ki = win32::KEYBDINPUT {
                wVk: action.vk_code,
                wScan: scan,
                dwFlags: dw_flags,
                time: 0,
                dwExtraInfo: 0,
            };

            inputs.push(win32::INPUT {
                type_: win32::INPUT_KEYBOARD,
                u: win32::INPUT_UNION { ki },
            });
        }

        let sent = win32::SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<win32::INPUT>() as i32,
        );

        if sent != inputs.len() as u32 {
            return Err("Failed to send all keyboard inputs".to_string());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("SendInput only supported on Windows".to_string())
    }
}

pub fn send_mouse_input(action: MouseAction) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        ensure_dpi_aware();
        if is_panic_hotkey_pressed() {
            return Err("Computer use aborted by panic hotkey (Ctrl+Alt+Shift+Esc).".to_string());
        }
        let vx = win32::GetSystemMetrics(win32::SM_XVIRTUALSCREEN);
        let vy = win32::GetSystemMetrics(win32::SM_YVIRTUALSCREEN);
        let vw = win32::GetSystemMetrics(win32::SM_CXVIRTUALSCREEN).max(1);
        let vh = win32::GetSystemMetrics(win32::SM_CYVIRTUALSCREEN).max(1);

        let mut flags = 0u32;
        let mut data = 0u32;

        if action.move_cursor {
            flags |= win32::MOUSEEVENTF_MOVE | win32::MOUSEEVENTF_ABSOLUTE | win32::MOUSEEVENTF_VIRTUALDESK;
        }

        if let Some(btn) = action.click_button {
            match btn {
                MouseButton::Left => {
                    match action.is_down {
                        Some(true) => flags |= win32::MOUSEEVENTF_LEFTDOWN,
                        Some(false) => flags |= win32::MOUSEEVENTF_LEFTUP,
                        None => flags |= win32::MOUSEEVENTF_LEFTDOWN | win32::MOUSEEVENTF_LEFTUP,
                    }
                }
                MouseButton::Right => {
                    match action.is_down {
                        Some(true) => flags |= win32::MOUSEEVENTF_RIGHTDOWN,
                        Some(false) => flags |= win32::MOUSEEVENTF_RIGHTUP,
                        None => flags |= win32::MOUSEEVENTF_RIGHTDOWN | win32::MOUSEEVENTF_RIGHTUP,
                    }
                }
                MouseButton::Middle => {
                    match action.is_down {
                        Some(true) => flags |= win32::MOUSEEVENTF_MIDDLEDOWN,
                        Some(false) => flags |= win32::MOUSEEVENTF_MIDDLEUP,
                        None => flags |= win32::MOUSEEVENTF_MIDDLEDOWN | win32::MOUSEEVENTF_MIDDLEUP,
                    }
                }
            }
        }

        if let Some(scroll) = action.scroll_delta {
            flags |= win32::MOUSEEVENTF_WHEEL;
            data = scroll as u32;
        }

        let norm_x = (((action.x - vx) as f64 / vw as f64) * 65535.0).round() as i32;
        let norm_y = (((action.y - vy) as f64 / vh as f64) * 65535.0).round() as i32;

        let mi = win32::MOUSEINPUT {
            dx: norm_x,
            dy: norm_y,
            mouseData: data,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: 0,
        };

        let input = win32::INPUT {
            type_: win32::INPUT_MOUSE,
            u: win32::INPUT_UNION { mi },
        };

        let sent = win32::SendInput(1, &input, size_of::<win32::INPUT>() as i32);
        if sent != 1 {
            return Err("Failed to send mouse input".to_string());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("SendInput only supported on Windows".to_string())
    }
}

pub fn get_clipboard_text() -> Result<String, String> {
    #[cfg(windows)]
    unsafe {
        if win32::OpenClipboard(null_mut()) == 0 {
            return Err("Failed to open clipboard".to_string());
        }

        let handle = win32::GetClipboardData(win32::CF_UNICODETEXT);
        if handle.is_null() {
            win32::CloseClipboard();
            return Ok(String::new());
        }

        let locked = win32::GlobalLock(handle as win32::HGLOBAL) as *const u16;
        if locked.is_null() {
            win32::CloseClipboard();
            return Err("Failed to lock global memory".to_string());
        }

        let mut len = 0;
        while *locked.add(len) != 0 {
            len += 1;
        }

        let slice = std::slice::from_raw_parts(locked, len);
        let result = String::from_utf16_lossy(slice);

        win32::GlobalUnlock(handle as win32::HGLOBAL);
        win32::CloseClipboard();

        Ok(result)
    }
    #[cfg(not(windows))]
    {
        Err("Clipboard only supported on Windows".to_string())
    }
}

pub fn set_clipboard_text(text: &str) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        if win32::OpenClipboard(null_mut()) == 0 {
            return Err("Failed to open clipboard".to_string());
        }

        win32::EmptyClipboard();

        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes_len = wide.len() * 2;

        let h_mem = win32::GlobalAlloc(win32::GMEM_MOVEABLE, bytes_len);
        if h_mem.is_null() {
            win32::CloseClipboard();
            return Err("Failed to allocate global memory".to_string());
        }

        let locked = win32::GlobalLock(h_mem) as *mut u16;
        std::ptr::copy_nonoverlapping(wide.as_ptr(), locked, wide.len());
        win32::GlobalUnlock(h_mem);

        win32::SetClipboardData(win32::CF_UNICODETEXT, h_mem as *mut c_void);
        win32::CloseClipboard();

        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Clipboard only supported on Windows".to_string())
    }
}

pub fn get_clipboard_image_base64() -> Result<Option<String>, String> {
    #[cfg(windows)]
    unsafe {
        if win32::OpenClipboard(null_mut()) == 0 {
            return Err("Failed to open clipboard".to_string());
        }

        let handle = win32::GetClipboardData(win32::CF_DIB);
        if handle.is_null() {
            win32::CloseClipboard();
            return Ok(None);
        }

        let size = win32::GlobalSize(handle as win32::HGLOBAL);
        let locked = win32::GlobalLock(handle as win32::HGLOBAL) as *const u8;
        if locked.is_null() {
            win32::CloseClipboard();
            return Err("Failed to lock clipboard memory".to_string());
        }

        let slice = std::slice::from_raw_parts(locked, size);
        let header_size = 14u32;
        let total_file_size = (header_size as usize) + size;

        let mut bmp_data = Vec::with_capacity(total_file_size);
        bmp_data.extend_from_slice(b"BM");
        bmp_data.extend_from_slice(&(total_file_size as u32).to_le_bytes());
        bmp_data.extend_from_slice(&0u32.to_le_bytes());
        bmp_data.extend_from_slice(&54u32.to_le_bytes());
        bmp_data.extend_from_slice(slice);

        win32::GlobalUnlock(handle as win32::HGLOBAL);
        win32::CloseClipboard();

        let b64 = base64::engine::general_purpose::STANDARD.encode(&bmp_data);
        Ok(Some(format!("data:image/bmp;base64,{}", b64)))
    }
    #[cfg(not(windows))]
    {
        Err("Clipboard only supported on Windows".to_string())
    }
}


fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB88320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for &b in data {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    (s2 << 16) | s1
}

fn append_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32_ieee(&crc_input).to_be_bytes());
}

fn encode_png_rgba_raw(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let stride = (width * 4) as usize;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0);
        let row_start = y * stride;
        raw.extend_from_slice(&rgba[row_start..row_start + stride]);
    }

    let mut zlib = Vec::with_capacity(raw.len() + 64);
    zlib.push(0x78);
    zlib.push(0x01);
    let mut pos = 0;
    while pos < raw.len() {
        let end = (pos + 65535).min(raw.len());
        let is_last = if end == raw.len() { 1u8 } else { 0u8 };
        let len = (end - pos) as u16;
        zlib.push(is_last);
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(&raw[pos..end]);
        pos = end;
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = Vec::with_capacity(zlib.len() + 64);
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8);
    ihdr.push(6);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    append_png_chunk(&mut png, b"IHDR", &ihdr);
    append_png_chunk(&mut png, b"IDAT", &zlib);
    append_png_chunk(&mut png, b"IEND", &[]);
    png
}

pub fn capture_screen_png(target_hwnd: Option<usize>) -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    unsafe {
        let (hwnd, width, height) = match target_hwnd {
            Some(h) => {
                let win_h = h as win32::HWND;
                let mut r = win32::RECT::default();
                win32::GetWindowRect(win_h, &mut r);
                (win_h, (r.right - r.left).max(1), (r.bottom - r.top).max(1))
            }
            None => (null_mut(), win32::GetSystemMetrics(0).max(1), win32::GetSystemMetrics(1).max(1)),
        };

        let hdc_src = win32::GetDC(hwnd);
        if hdc_src.is_null() {
            return Err("Failed to get DC".to_string());
        }
        let hdc_mem = win32::CreateCompatibleDC(hdc_src);
        let hbm = win32::CreateCompatibleBitmap(hdc_src, width, height);
        let old_obj = win32::SelectObject(hdc_mem, hbm);

        if !hwnd.is_null() {
            if win32::PrintWindow(hwnd, hdc_mem, win32::PW_RENDERFULLCONTENT) == 0 {
                win32::BitBlt(hdc_mem, 0, 0, width, height, hdc_src, 0, 0, win32::SRCCOPY);
            }
        } else {
            win32::BitBlt(hdc_mem, 0, 0, width, height, hdc_src, 0, 0, win32::SRCCOPY);
        }

        let mut bmi = win32::BITMAPINFO {
            bmiHeader: win32::BITMAPINFOHEADER {
                biSize: size_of::<win32::BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: (width * height * 4) as u32,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [0; 1],
        };

        let mut bgra: Vec<u8> = vec![0u8; (width * height * 4) as usize];
        win32::GetDIBits(hdc_mem, hbm, 0, height as u32, bgra.as_mut_ptr() as *mut c_void, &mut bmi, 0);

        win32::SelectObject(hdc_mem, old_obj);
        win32::DeleteObject(hbm);
        win32::DeleteDC(hdc_mem);
        win32::ReleaseDC(hwnd, hdc_src);

        let scale = if width > 1440 { 2 } else { 1 };
        let out_w = (width / scale).max(1) as u32;
        let out_h = (height / scale).max(1) as u32;
        let mut rgba = Vec::with_capacity((out_w * out_h * 4) as usize);

        for y in 0..out_h as i32 {
            for x in 0..out_w as i32 {
                let src_idx = (((y * scale) * width + (x * scale)) * 4) as usize;
                rgba.push(bgra[src_idx + 2]);
                rgba.push(bgra[src_idx + 1]);
                rgba.push(bgra[src_idx]);
                rgba.push(255);
            }
        }

        let png_bytes = encode_png_rgba_raw(out_w, out_h, &rgba);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
        let windows: Vec<serde_json::Value> = find_windows(None, None)
            .into_iter()
            .filter(|w| w.is_visible && !w.title.trim().is_empty() && w.bounds.width > 50 && w.bounds.height > 50)
            .take(15)
            .map(|w| serde_json::json!({ "hwnd": w.hwnd, "title": w.title, "bounds": w.bounds }))
            .collect();

        Ok(serde_json::json!({
            "screenWidth": width,
            "screenHeight": height,
            "imageWidth": out_w,
            "imageHeight": out_h,
            "dataUrl": format!("data:image/png;base64,{}", b64),
            "windows": windows,
        }))
    }
    #[cfg(not(windows))]
    {
        let _ = target_hwnd;
        Err("Computer Use screenshot only supported on Windows".to_string())
    }
}

#[cfg(windows)]
fn resolve_screen_xy(x: i32, y: i32, normalized: bool) -> (i32, i32) {
    unsafe {
        let sw = win32::GetSystemMetrics(0).max(1);
        let sh = win32::GetSystemMetrics(1).max(1);
        if normalized {
            let rx = ((x.clamp(0, 1000) as f64 / 1000.0) * sw as f64).round() as i32;
            let ry = ((y.clamp(0, 1000) as f64 / 1000.0) * sh as f64).round() as i32;
            (rx, ry)
        } else {
            (x, y)
        }
    }
}

pub fn cu_mouse_action(
    action: &str,
    x: i32,
    y: i32,
    to_x: Option<i32>,
    to_y: Option<i32>,
    delta: Option<i32>,
    normalized: bool,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let (sx, sy) = resolve_screen_xy(x, y, normalized);
        match action {
            "move" => send_mouse_input(MouseAction { x: sx, y: sy, move_cursor: true, click_button: None, is_down: None, scroll_delta: None }),
            "click" => send_mouse_input(MouseAction { x: sx, y: sy, move_cursor: true, click_button: Some(MouseButton::Left), is_down: None, scroll_delta: None }),
            "double_click" => {
                send_mouse_input(MouseAction { x: sx, y: sy, move_cursor: true, click_button: Some(MouseButton::Left), is_down: None, scroll_delta: None })?;
                std::thread::sleep(std::time::Duration::from_millis(60));
                send_mouse_input(MouseAction { x: sx, y: sy, move_cursor: false, click_button: Some(MouseButton::Left), is_down: None, scroll_delta: None })
            }
            "right_click" => send_mouse_input(MouseAction { x: sx, y: sy, move_cursor: true, click_button: Some(MouseButton::Right), is_down: None, scroll_delta: None }),
            "scroll" => send_mouse_input(MouseAction { x: sx, y: sy, move_cursor: true, click_button: None, is_down: None, scroll_delta: Some(delta.unwrap_or(-120)) }),
            "drag" => {
                let (ex, ey) = resolve_screen_xy(to_x.unwrap_or(x), to_y.unwrap_or(y), normalized);
                send_mouse_input(MouseAction { x: sx, y: sy, move_cursor: true, click_button: Some(MouseButton::Left), is_down: Some(true), scroll_delta: None })?;
                std::thread::sleep(std::time::Duration::from_millis(80));
                send_mouse_input(MouseAction { x: ex, y: ey, move_cursor: true, click_button: Some(MouseButton::Left), is_down: Some(false), scroll_delta: None })
            }
            other => Err(format!("Unsupported mouse action: {}", other)),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (action, x, y, to_x, to_y, delta, normalized);
        Err("Mouse input only supported on Windows".to_string())
    }
}

pub fn cu_type_text(text: &str) -> Result<(), String> {
    let mut actions = Vec::new();
    for unit in text.encode_utf16() {
        actions.push(KeyAction {
            vk_code: 0,
            is_up: false,
            is_unicode: true,
            unicode_char: std::char::from_u32(unit as u32),
        });
        actions.push(KeyAction {
            vk_code: 0,
            is_up: true,
            is_unicode: true,
            unicode_char: std::char::from_u32(unit as u32),
        });
    }
    if actions.is_empty() {
        return Ok(());
    }
    send_keyboard_chords(&actions)
}

fn map_key_name_to_vk(name: &str) -> Option<u16> {
    match name.trim().to_lowercase().as_str() {
        "ctrl" | "control" => Some(0x11),
        "alt" | "menu" => Some(0x12),
        "shift" => Some(0x10),
        "win" | "meta" | "super" => Some(0x5B),
        "enter" | "return" => Some(0x0D),
        "esc" | "escape" => Some(0x1B),
        "tab" => Some(0x09),
        "space" => Some(0x20),
        "backspace" => Some(0x08),
        "delete" | "del" => Some(0x2E),
        "up" => Some(0x26),
        "down" => Some(0x28),
        "left" => Some(0x25),
        "right" => Some(0x27),
        "home" => Some(0x24),
        "end" => Some(0x23),
        "pageup" => Some(0x21),
        "pagedown" => Some(0x22),
        "f1" => Some(0x70),
        "f2" => Some(0x71),
        "f3" => Some(0x72),
        "f4" => Some(0x73),
        "f5" => Some(0x74),
        "f6" => Some(0x75),
        "f7" => Some(0x76),
        "f8" => Some(0x77),
        "f9" => Some(0x78),
        "f10" => Some(0x79),
        "f11" => Some(0x7A),
        "f12" => Some(0x7B),
        s if s.len() == 1 => {
            let b = s.as_bytes()[0].to_ascii_uppercase();
            if b.is_ascii_alphanumeric() { Some(b as u16) } else { None }
        }
        _ => None,
    }
}

pub fn cu_hotkey(combo: &str) -> Result<(), String> {
    let mut vks = Vec::new();
    for part in combo.split('+') {
        let vk = map_key_name_to_vk(part).ok_or_else(|| format!("Unknown key in hotkey: {}", part))?;
        vks.push(vk);
    }
    if vks.is_empty() {
        return Err("Empty hotkey".to_string());
    }
    let mut actions = Vec::with_capacity(vks.len() * 2);
    for &vk in &vks {
        actions.push(KeyAction { vk_code: vk, is_up: false, is_unicode: false, unicode_char: None });
    }
    for &vk in vks.iter().rev() {
        actions.push(KeyAction { vk_code: vk, is_up: true, is_unicode: false, unicode_char: None });
    }
    send_keyboard_chords(&actions)
}

pub fn cu_focus_window(title_query: Option<&str>, hwnd: Option<usize>) -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    unsafe {
        let target = if let Some(h) = hwnd {
            find_windows(None, None).into_iter().find(|w| w.hwnd == h)
        } else if let Some(q) = title_query {
            find_windows(Some(q), None).into_iter().find(|w| w.is_visible)
        } else {
            None
        };
        let win = target.ok_or_else(|| "Target window not found".to_string())?;
        win32::SetForegroundWindow(win.hwnd as win32::HWND);
        Ok(serde_json::json!({ "focused": true, "hwnd": win.hwnd, "title": win.title, "bounds": win.bounds }))
    }
    #[cfg(not(windows))]
    {
        let _ = (title_query, hwnd);
        Err("Window focus only supported on Windows".to_string())
    }
}
