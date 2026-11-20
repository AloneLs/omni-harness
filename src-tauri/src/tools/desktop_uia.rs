use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub hwnd: usize,
    pub pid: u32,
    pub process_name: String,
    pub title: String,
    pub class_name: String,
    pub is_visible: bool,
    pub is_minimized: bool,
    pub is_offscreen: bool,
    pub bounds: RectBounds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;

    pub type HWND = *mut c_void;
    pub type HDC = *mut c_void;
    pub type HBITMAP = *mut c_void;
    pub type HGDIOBJ = *mut c_void;
    pub type HANDLE = *mut c_void;
    pub type HDESK = *mut c_void;
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

    pub const GA_ROOT: UINT = 2;
    pub const GWL_EXSTYLE: i32 = -20;
    pub const WS_EX_TOOLWINDOW: isize = 0x00000080;
    pub const WS_EX_NOACTIVATE: isize = 0x08000000;
    pub const SW_SHOWNOACTIVATE: i32 = 4;
    pub const SWP_NOACTIVATE: UINT = 0x0010;
    pub const SWP_SHOWWINDOW: UINT = 0x0040;
    pub const SWP_NOOWNERZORDER: UINT = 0x0200;
    pub const SWP_NOSENDCHANGING: UINT = 0x0400;

    pub const SM_XVIRTUALSCREEN: i32 = 76;
    pub const SM_YVIRTUALSCREEN: i32 = 77;
    pub const SM_CXVIRTUALSCREEN: i32 = 78;
    pub const SM_CYVIRTUALSCREEN: i32 = 79;

    pub const SRCCOPY: DWORD = 0x00CC0020;
    pub const PW_RENDERFULLCONTENT: UINT = 0x00000002;
    pub const WM_CLOSE: UINT = 0x0010;
    pub const WM_PRINT: UINT = 0x0317;
    pub const PRF_NONCLIENT: LPARAM = 0x02;
    pub const PRF_CLIENT: LPARAM = 0x04;
    pub const PRF_ERASEBKGND: LPARAM = 0x08;
    pub const PRF_CHILDREN: LPARAM = 0x10;
    pub const SMTO_ABORTIFHUNG: UINT = 0x0002;
    pub const PROCESS_QUERY_LIMITED_INFORMATION: DWORD = 0x1000;
    pub const DWMWA_CLOAKED: DWORD = 14;

    extern "system" {
        pub fn IsWindow(hWnd: HWND) -> BOOL;
        pub fn IsWindowVisible(hWnd: HWND) -> BOOL;
        pub fn IsIconic(hWnd: HWND) -> BOOL;
        pub fn GetAncestor(hwnd: HWND, gaFlags: UINT) -> HWND;
        pub fn EnumWindows(
            lpEnumFunc: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>,
            lParam: LPARAM,
        ) -> BOOL;
        pub fn EnumChildWindows(
            hWndParent: HWND,
            lpEnumFunc: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>,
            lParam: LPARAM,
        ) -> BOOL;
        pub fn EnumDesktopWindows(
            hDesktop: HDESK,
            lpfn: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>,
            lParam: LPARAM,
        ) -> BOOL;
        pub fn OpenDesktopW(
            lpszDesktop: *const u16,
            dwFlags: DWORD,
            fInherit: BOOL,
            dwDesiredAccess: DWORD,
        ) -> HDESK;
        pub fn CloseDesktop(hDesktop: HDESK) -> BOOL;
        pub fn GetWindowTextW(hWnd: HWND, lpString: *mut u16, nMaxCount: i32) -> i32;
        pub fn GetClassNameW(hWnd: HWND, lpClassName: *mut u16, nMaxCount: i32) -> i32;
        pub fn GetWindowRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
        pub fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> isize;
        pub fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dwNewLong: isize) -> isize;
        pub fn SetWindowPos(
            hWnd: HWND,
            hWndInsertAfter: HWND,
            X: i32,
            Y: i32,
            cx: i32,
            cy: i32,
            uFlags: UINT,
        ) -> BOOL;
        pub fn ShowWindowAsync(hWnd: HWND, nCmdShow: i32) -> BOOL;
        pub fn SetForegroundWindow(hWnd: HWND) -> BOOL;
        pub fn PostMessageW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> BOOL;
        pub fn SendMessageTimeoutW(
            hWnd: HWND,
            Msg: UINT,
            wParam: WPARAM,
            lParam: LPARAM,
            fuFlags: UINT,
            uTimeout: UINT,
            lpdwResult: *mut usize,
        ) -> LRESULT;
        pub fn GetWindowThreadProcessId(hWnd: HWND, lpdwProcessId: *mut DWORD) -> DWORD;
        pub fn OpenProcess(dwDesiredAccess: DWORD, bInheritHandle: BOOL, dwProcessId: DWORD) -> HANDLE;
        pub fn QueryFullProcessImageNameW(
            hProcess: HANDLE,
            dwFlags: DWORD,
            lpExeName: *mut u16,
            lpdwSize: *mut DWORD,
        ) -> BOOL;
        pub fn CloseHandle(hObject: HANDLE) -> BOOL;
        pub fn DwmGetWindowAttribute(
            hwnd: HWND,
            dwAttribute: DWORD,
            pvAttribute: *mut c_void,
            cbAttribute: DWORD,
        ) -> i32;
        pub fn GetSystemMetrics(nIndex: i32) -> i32;
        pub fn GetAsyncKeyState(vKey: i32) -> i16;
        pub fn GetDC(hWnd: HWND) -> HDC;
        pub fn ReleaseDC(hWnd: HWND, hDC: HDC) -> i32;
        pub fn CreateCompatibleDC(hDC: HDC) -> HDC;
        pub fn DeleteDC(hDC: HDC) -> BOOL;
        pub fn CreateCompatibleBitmap(hDC: HDC, cx: i32, cy: i32) -> HBITMAP;
        pub fn SelectObject(hDC: HDC, h: HGDIOBJ) -> HGDIOBJ;
        pub fn DeleteObject(ho: HGDIOBJ) -> BOOL;
        pub fn BitBlt(
            hdc: HDC,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            hdcSrc: HDC,
            x1: i32,
            y1: i32,
            rop: DWORD,
        ) -> BOOL;
        pub fn PrintWindow(hwnd: HWND, hdcBlt: HDC, nFlags: UINT) -> BOOL;
        pub fn GetDIBits(
            hdc: HDC,
            hbm: HBITMAP,
            start: UINT,
            cLines: UINT,
            lpvBits: *mut c_void,
            lpbmi: *mut BITMAPINFO,
            usage: UINT,
        ) -> i32;
    }
}

pub fn is_panic_hotkey_active() -> bool {
    #[cfg(windows)]
    unsafe {
        (win32::GetAsyncKeyState(0x11) as u16 & 0x8000) != 0
            && (win32::GetAsyncKeyState(0x12) as u16 & 0x8000) != 0
            && (win32::GetAsyncKeyState(0x10) as u16 & 0x8000) != 0
            && (win32::GetAsyncKeyState(0x1B) as u16 & 0x8000) != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn is_window_valid(hwnd: usize) -> bool {
    #[cfg(windows)]
    unsafe {
        hwnd != 0 && win32::IsWindow(hwnd as win32::HWND) != 0
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}

pub fn resolve_root_hwnd(hwnd: usize) -> usize {
    #[cfg(windows)]
    unsafe {
        if hwnd == 0 {
            return 0;
        }
        let root = win32::GetAncestor(hwnd as win32::HWND, win32::GA_ROOT);
        if root.is_null() {
            hwnd
        } else {
            root as usize
        }
    }
    #[cfg(not(windows))]
    {
        hwnd
    }
}

pub fn get_window_ex_style(hwnd: usize) -> isize {
    #[cfg(windows)]
    unsafe {
        win32::GetWindowLongPtrW(hwnd as win32::HWND, win32::GWL_EXSTYLE)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        0
    }
}

pub fn compute_offscreen_origin() -> (i32, i32) {
    #[cfg(windows)]
    unsafe {
        let vx = win32::GetSystemMetrics(win32::SM_XVIRTUALSCREEN);
        let vy = win32::GetSystemMetrics(win32::SM_YVIRTUALSCREEN);
        let vw = win32::GetSystemMetrics(win32::SM_CXVIRTUALSCREEN).max(1920);
        (vx + vw + 180, vy + 60)
    }
    #[cfg(not(windows))]
    {
        (3200, 60)
    }
}

#[cfg(windows)]
unsafe fn is_rect_offscreen(r: &win32::RECT) -> bool {
    let vx = win32::GetSystemMetrics(win32::SM_XVIRTUALSCREEN);
    let vy = win32::GetSystemMetrics(win32::SM_YVIRTUALSCREEN);
    let vr = vx + win32::GetSystemMetrics(win32::SM_CXVIRTUALSCREEN).max(1);
    let vb = vy + win32::GetSystemMetrics(win32::SM_CYVIRTUALSCREEN).max(1);
    r.right <= vx + 8 || r.left >= vr - 8 || r.bottom <= vy + 8 || r.top >= vb - 8
}

#[cfg(windows)]
unsafe fn is_window_dwm_cloaked(hwnd: win32::HWND) -> bool {
    let mut cloaked: u32 = 0;
    let hr = win32::DwmGetWindowAttribute(
        hwnd,
        win32::DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut c_void,
        size_of::<u32>() as u32,
    );
    hr == 0 && cloaked != 0
}

#[cfg(windows)]
unsafe fn query_process_basename(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    let handle = win32::OpenProcess(win32::PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if handle.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 512];
    let mut sz = buf.len() as u32;
    let ok = win32::QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut sz);
    win32::CloseHandle(handle);
    if ok == 0 || sz == 0 {
        return String::new();
    }
    let full = String::from_utf16_lossy(&buf[..sz as usize]);
    std::path::Path::new(&full)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&full)
        .to_string()
}

#[cfg(windows)]
unsafe extern "system" fn find_corewindow_pid_cb(
    hwnd: win32::HWND,
    lparam: win32::LPARAM,
) -> win32::BOOL {
    let out_pid = &mut *(lparam as *mut u32);
    let mut cbuf = [0u16; 128];
    let clen = win32::GetClassNameW(hwnd, cbuf.as_mut_ptr(), 128);
    let cname = String::from_utf16_lossy(&cbuf[..clen.max(0) as usize]);
    if cname.contains("CoreWindow") {
        let mut child_pid = 0u32;
        win32::GetWindowThreadProcessId(hwnd, &mut child_pid);
        if child_pid != 0 {
            *out_pid = child_pid;
            return 0;
        }
    }
    1
}

pub fn query_window_info(hwnd: usize) -> Option<WindowInfo> {
    #[cfg(windows)]
    unsafe {
        let h = hwnd as win32::HWND;
        if hwnd == 0 || win32::IsWindow(h) == 0 {
            return None;
        }
        let mut tbuf = [0u16; 512];
        let tlen = win32::GetWindowTextW(h, tbuf.as_mut_ptr(), 512);
        let title = String::from_utf16_lossy(&tbuf[..tlen.max(0) as usize]);

        let mut cbuf = [0u16; 256];
        let clen = win32::GetClassNameW(h, cbuf.as_mut_ptr(), 256);
        let class_name = String::from_utf16_lossy(&cbuf[..clen.max(0) as usize]);

        let mut pid = 0u32;
        win32::GetWindowThreadProcessId(h, &mut pid);
        if class_name == "ApplicationFrameWindow" {
            let mut inner_pid = 0u32;
            win32::EnumChildWindows(
                h,
                Some(find_corewindow_pid_cb),
                &mut inner_pid as *mut u32 as win32::LPARAM,
            );
            if inner_pid != 0 {
                pid = inner_pid;
            }
        }
        let process_name = query_process_basename(pid);
        let is_visible = win32::IsWindowVisible(h) != 0;
        let is_minimized = win32::IsIconic(h) != 0;

        let mut rect = win32::RECT::default();
        win32::GetWindowRect(h, &mut rect);
        let is_offscreen = !is_minimized && is_rect_offscreen(&rect);

        Some(WindowInfo {
            hwnd,
            pid,
            process_name,
            title,
            class_name,
            is_visible,
            is_minimized,
            is_offscreen,
            bounds: RectBounds {
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
            },
        })
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        None
    }
}

#[cfg(windows)]
unsafe extern "system" fn enum_top_windows_cb(
    hwnd: win32::HWND,
    lparam: win32::LPARAM,
) -> win32::BOOL {
    let vec_out = &mut *(lparam as *mut Vec<usize>);
    vec_out.push(hwnd as usize);
    1
}

pub fn list_controllable_windows(
    title_filter: Option<&str>,
    process_filter: Option<&str>,
) -> Vec<WindowInfo> {
    #[cfg(windows)]
    unsafe {
        let mut raw_hwnds: Vec<usize> = Vec::with_capacity(128);
        win32::EnumWindows(
            Some(enum_top_windows_cb),
            &mut raw_hwnds as *mut Vec<usize> as win32::LPARAM,
        );

        let t_flt = title_filter.map(|s| s.to_lowercase());
        let p_flt = process_filter.map(|s| s.to_lowercase());
        let self_pid = std::process::id();
        let mut out = Vec::new();

        for h in raw_hwnds {
            let staged = crate::tools::phantom_session::is_hwnd_staged(h);
            if !staged && win32::IsWindowVisible(h as win32::HWND) == 0 {
                continue;
            }
            if !staged && is_window_dwm_cloaked(h as win32::HWND) {
                continue;
            }
            let Some(info) = query_window_info(h) else {
                continue;
            };
            if info.pid == self_pid {
                continue;
            }
            let cn = info.class_name.as_str();
            if matches!(
                cn,
                "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "IME" | "MSCTFIME UI"
            ) {
                continue;
            }
            if !staged && info.title.trim().is_empty() {
                continue;
            }
            if !staged && !info.is_minimized && (info.bounds.width < 40 || info.bounds.height < 30) {
                continue;
            }
            if let Some(ref tf) = t_flt {
                if !info.title.to_lowercase().contains(tf) {
                    continue;
                }
            }
            if let Some(ref pf) = p_flt {
                if !info.process_name.to_lowercase().contains(pf)
                    && !info.class_name.to_lowercase().contains(pf)
                {
                    continue;
                }
            }
            out.push(info);
        }
        out
    }
    #[cfg(not(windows))]
    {
        let _ = (title_filter, process_filter);
        Vec::new()
    }
}

pub fn list_isolated_desktop_windows(desktop_name: &str) -> Vec<WindowInfo> {
    #[cfg(windows)]
    unsafe {
        let wide: Vec<u16> = desktop_name.encode_utf16().chain(std::iter::once(0)).collect();
        let hdesk = win32::OpenDesktopW(wide.as_ptr(), 0, 0, 0x01FF);
        if hdesk.is_null() {
            return Vec::new();
        }
        let mut raw_hwnds: Vec<usize> = Vec::new();
        win32::EnumDesktopWindows(
            hdesk,
            Some(enum_top_windows_cb),
            &mut raw_hwnds as *mut Vec<usize> as win32::LPARAM,
        );
        win32::CloseDesktop(hdesk);

        let self_pid = std::process::id();
        raw_hwnds
            .into_iter()
            .filter_map(query_window_info)
            .filter(|w| w.pid != self_pid && (w.is_visible || w.bounds.width >= 80))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = desktop_name;
        Vec::new()
    }
}

pub fn find_windows(title_filter: Option<&str>, class_filter: Option<&str>) -> Vec<WindowInfo> {
    list_controllable_windows(title_filter, class_filter)
}

pub fn find_visible_windows_for_pid(target_pid: u32) -> Vec<WindowInfo> {
    #[cfg(windows)]
    unsafe {
        if target_pid == 0 || target_pid == std::process::id() {
            return Vec::new();
        }
        let mut raw_hwnds: Vec<usize> = Vec::with_capacity(128);
        win32::EnumWindows(
            Some(enum_top_windows_cb),
            &mut raw_hwnds as *mut Vec<usize> as win32::LPARAM,
        );
        raw_hwnds
            .into_iter()
            .filter(|&h| win32::IsWindowVisible(h as win32::HWND) != 0)
            .filter_map(query_window_info)
            .filter(|w| {
                w.pid == target_pid
                    && w.bounds.width >= 80
                    && w.bounds.height >= 50
                    && !matches!(w.class_name.as_str(), "IME" | "MSCTFIME UI")
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = target_pid;
        Vec::new()
    }
}

#[cfg(windows)]
unsafe extern "system" fn enum_child_callback(
    hwnd: win32::HWND,
    lparam: win32::LPARAM,
) -> win32::BOOL {
    let list = &mut *(lparam as *mut Vec<ControlInfo>);
    let mut title_buf = [0u16; 512];
    let len = win32::GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 512);
    let text = String::from_utf16_lossy(&title_buf[..len.max(0) as usize]);

    let mut class_buf = [0u16; 256];
    let class_len = win32::GetClassNameW(hwnd, class_buf.as_mut_ptr(), 256);
    let class_name = String::from_utf16_lossy(&class_buf[..class_len.max(0) as usize]);

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
            .filter(|c| match f {
                Some(ref flt) => {
                    c.text.to_lowercase().contains(flt) || c.class_name.to_lowercase().contains(flt)
                }
                None => true,
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = (parent_hwnd, text_filter);
        Vec::new()
    }
}

pub fn unminimize_no_activate(hwnd: usize) {
    #[cfg(windows)]
    unsafe {
        let h = hwnd as win32::HWND;
        if win32::IsIconic(h) != 0 {
            win32::ShowWindowAsync(h, win32::SW_SHOWNOACTIVATE);
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
    }
}

pub fn move_window_to_offscreen_stage(
    hwnd: usize,
    width: i32,
    height: i32,
    spawned_by_omni: bool,
) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        let h = hwnd as win32::HWND;
        if win32::IsWindow(h) == 0 {
            return Err("Target window handle is invalid.".to_string());
        }
        if win32::IsIconic(h) != 0 {
            win32::ShowWindowAsync(h, win32::SW_SHOWNOACTIVATE);
            std::thread::sleep(std::time::Duration::from_millis(90));
        }
        let mut ex = win32::GetWindowLongPtrW(h, win32::GWL_EXSTYLE);
        if spawned_by_omni {
            ex |= win32::WS_EX_NOACTIVATE | win32::WS_EX_TOOLWINDOW;
        }
        win32::SetWindowLongPtrW(h, win32::GWL_EXSTYLE, ex);

        let (off_x, off_y) = compute_offscreen_origin();
        let hwnd_bottom = 1isize as win32::HWND;
        let ok = win32::SetWindowPos(
            h,
            hwnd_bottom,
            off_x,
            off_y,
            width.max(320),
            height.max(240),
            win32::SWP_NOACTIVATE
                | win32::SWP_NOOWNERZORDER
                | win32::SWP_NOSENDCHANGING
                | win32::SWP_SHOWWINDOW,
        );
        if ok == 0 {
            return Err("SetWindowPos failed while staging window off-screen (target process may be elevated).".to_string());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, width, height, spawned_by_omni);
        Err("Off-screen staging is only supported on Windows.".to_string())
    }
}

pub fn restore_window_placement(
    hwnd: usize,
    orig: &RectBounds,
    orig_ex_style: isize,
) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        let h = hwnd as win32::HWND;
        if win32::IsWindow(h) == 0 {
            return Err("Target window is closed.".to_string());
        }
        let clean_ex = orig_ex_style & !(win32::WS_EX_NOACTIVATE | win32::WS_EX_TOOLWINDOW);
        win32::SetWindowLongPtrW(h, win32::GWL_EXSTYLE, clean_ex);
        win32::ShowWindowAsync(h, win32::SW_SHOWNOACTIVATE);

        let sw = win32::GetSystemMetrics(0).max(1280);
        let sh = win32::GetSystemMetrics(1).max(720);
        let rw = if orig.width >= 300 && orig.width <= sw {
            orig.width
        } else {
            1160.min(sw - 80)
        };
        let rh = if orig.height >= 220 && orig.height <= sh {
            orig.height
        } else {
            760.min(sh - 80)
        };
        let rx = if orig.x >= -20 && orig.x + 120 < sw {
            orig.x
        } else {
            ((sw - rw) / 2).max(40)
        };
        let ry = if orig.y >= 0 && orig.y + 80 < sh {
            orig.y
        } else {
            ((sh - rh) / 2).max(40)
        };

        win32::SetWindowPos(
            h,
            null_mut(),
            rx,
            ry,
            rw,
            rh,
            win32::SWP_NOACTIVATE | win32::SWP_NOOWNERZORDER | win32::SWP_SHOWWINDOW,
        );
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, orig, orig_ex_style);
        Err("Window restore is only supported on Windows.".to_string())
    }
}

pub fn close_window_background(hwnd: usize) -> Result<Value, String> {
    #[cfg(windows)]
    unsafe {
        let root = resolve_root_hwnd(hwnd);
        let info = query_window_info(root).ok_or_else(|| "Target window not found.".to_string())?;
        if info.pid == std::process::id() {
            return Err("Refusing to close OmniHarness's own window.".to_string());
        }
        win32::PostMessageW(root as win32::HWND, win32::WM_CLOSE, 0, 0);
        Ok(json!({
            "status": "close_sent",
            "hwnd": root,
            "title": info.title,
            "processName": info.process_name,
        }))
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("Window close is only supported on Windows.".to_string())
    }
}

pub fn foreground_window_explicit(hwnd: usize) -> Result<Value, String> {
    #[cfg(windows)]
    unsafe {
        let root = resolve_root_hwnd(hwnd);
        let info = query_window_info(root).ok_or_else(|| "Target window not found.".to_string())?;
        win32::SetForegroundWindow(root as win32::HWND);
        Ok(json!({
            "focused": true,
            "hwnd": root,
            "title": info.title,
            "bounds": info.bounds,
        }))
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("Window focus is only supported on Windows.".to_string())
    }
}

fn draw_virtual_cursor_reticle(bgra: &mut [u8], width: i32, height: i32, cx: i32, cy: i32) {
    if cx < 0 || cy < 0 || cx >= width || cy >= height {
        return;
    }
    let set_px = |buf: &mut [u8], x: i32, y: i32, b: u8, g: u8, r: u8| {
        if x >= 0 && x < width && y >= 0 && y < height {
            let idx = ((y * width + x) * 4) as usize;
            if idx + 3 < buf.len() {
                buf[idx] = b;
                buf[idx + 1] = g;
                buf[idx + 2] = r;
                buf[idx + 3] = 255;
            }
        }
    };
    for d in -9..=9i32 {
        if d.abs() >= 3 {
            set_px(bgra, cx + d, cy - 1, 18, 18, 22);
            set_px(bgra, cx + d, cy + 1, 18, 18, 22);
            set_px(bgra, cx + d, cy, 246, 92, 139);
            set_px(bgra, cx - 1, cy + d, 18, 18, 22);
            set_px(bgra, cx + 1, cy + d, 18, 18, 22);
            set_px(bgra, cx, cy + d, 246, 92, 139);
        }
    }
    for dy in -1..=1 {
        for dx in -1..=1 {
            set_px(bgra, cx + dx, cy + dy, 255, 255, 255);
        }
    }
}

fn encode_png_compressed(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity((width * height) as usize / 2);
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn is_bgra_blank(bgra: &[u8]) -> bool {
    bgra.chunks_exact(4)
        .step_by(37)
        .all(|c| c[0] == 0 && c[1] == 0 && c[2] == 0)
}

pub fn capture_window_or_desktop_png(
    target_hwnd: Option<usize>,
    virtual_cursor: Option<(i32, i32)>,
    region_norm: Option<[i32; 4]>,
) -> Result<Value, String> {
    #[cfg(windows)]
    unsafe {
        if is_panic_hotkey_active() {
            return Err("Computer use aborted by panic hotkey (Ctrl+Alt+Shift+Esc).".to_string());
        }

        let mut win_meta: Option<WindowInfo> = None;
        let (hwnd, width, height) = match target_hwnd.filter(|&h| h != 0) {
            Some(raw_h) => {
                let root = resolve_root_hwnd(raw_h);
                let mut info = query_window_info(root)
                    .ok_or_else(|| format!("Window {} is no longer open.", raw_h))?;
                if info.is_minimized {
                    unminimize_no_activate(root);
                    if let Some(refreshed) = query_window_info(root) {
                        info = refreshed;
                    }
                }
                let w = info.bounds.width.clamp(1, 7680);
                let h = info.bounds.height.clamp(1, 4320);
                win_meta = Some(info);
                (root as win32::HWND, w, h)
            }
            None => (
                null_mut(),
                win32::GetSystemMetrics(0).max(1),
                win32::GetSystemMetrics(1).max(1),
            ),
        };

        let hdc_screen = win32::GetDC(null_mut());
        if hdc_screen.is_null() {
            return Err("Failed to acquire screen device context.".to_string());
        }
        let hdc_mem = win32::CreateCompatibleDC(hdc_screen);
        let hbm = win32::CreateCompatibleBitmap(hdc_screen, width, height);
        let old_obj = win32::SelectObject(hdc_mem, hbm);

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
        let mut capture_method = "BitBlt";

        if !hwnd.is_null() {
            let pw_full = win32::PrintWindow(hwnd, hdc_mem, win32::PW_RENDERFULLCONTENT) != 0;
            if pw_full {
                win32::GetDIBits(
                    hdc_mem,
                    hbm,
                    0,
                    height as u32,
                    bgra.as_mut_ptr() as *mut c_void,
                    &mut bmi,
                    0,
                );
                capture_method = "PrintWindow_FullContent";
            }

            if !pw_full || is_bgra_blank(&bgra) {
                let pw_basic = win32::PrintWindow(hwnd, hdc_mem, 0) != 0;
                if pw_basic {
                    win32::GetDIBits(
                        hdc_mem,
                        hbm,
                        0,
                        height as u32,
                        bgra.as_mut_ptr() as *mut c_void,
                        &mut bmi,
                        0,
                    );
                    capture_method = "PrintWindow_GDI";
                }
            }

            if is_bgra_blank(&bgra) {
                let mut dummy = 0usize;
                let _ = win32::SendMessageTimeoutW(
                    hwnd,
                    win32::WM_PRINT,
                    hdc_mem as usize,
                    win32::PRF_NONCLIENT
                        | win32::PRF_CLIENT
                        | win32::PRF_CHILDREN
                        | win32::PRF_ERASEBKGND,
                    win32::SMTO_ABORTIFHUNG,
                    120,
                    &mut dummy,
                );
                win32::GetDIBits(
                    hdc_mem,
                    hbm,
                    0,
                    height as u32,
                    bgra.as_mut_ptr() as *mut c_void,
                    &mut bmi,
                    0,
                );
                capture_method = "WM_PRINT";
            }

            if is_bgra_blank(&bgra) {
                if let Some(ref wm) = win_meta {
                    if !wm.is_offscreen {
                        win32::BitBlt(
                            hdc_mem,
                            0,
                            0,
                            width,
                            height,
                            hdc_screen,
                            wm.bounds.x,
                            wm.bounds.y,
                            win32::SRCCOPY,
                        );
                        win32::GetDIBits(
                            hdc_mem,
                            hbm,
                            0,
                            height as u32,
                            bgra.as_mut_ptr() as *mut c_void,
                            &mut bmi,
                            0,
                        );
                        capture_method = "Screen_BitBlt_Fallback";
                    }
                }
            }
        } else {
            win32::BitBlt(hdc_mem, 0, 0, width, height, hdc_screen, 0, 0, win32::SRCCOPY);
            win32::GetDIBits(
                hdc_mem,
                hbm,
                0,
                height as u32,
                bgra.as_mut_ptr() as *mut c_void,
                &mut bmi,
                0,
            );
        }

        win32::SelectObject(hdc_mem, old_obj);
        win32::DeleteObject(hbm);
        win32::DeleteDC(hdc_mem);
        win32::ReleaseDC(null_mut(), hdc_screen);

        if let Some((vx, vy)) = virtual_cursor {
            draw_virtual_cursor_reticle(&mut bgra, width, height, vx, vy);
        }

        let (crop_x, crop_y, crop_w, crop_h) = if let Some([rx, ry, rw, rh]) = region_norm {
            let cx = (((rx.clamp(0, 999) as f64 / 1000.0) * width as f64) as i32).clamp(0, width - 1);
            let cy = (((ry.clamp(0, 999) as f64 / 1000.0) * height as f64) as i32).clamp(0, height - 1);
            let cw = (((rw.clamp(10, 1000) as f64 / 1000.0) * width as f64) as i32).clamp(1, width - cx);
            let ch = (((rh.clamp(10, 1000) as f64 / 1000.0) * height as f64) as i32).clamp(1, height - cy);
            (cx, cy, cw, ch)
        } else {
            (0, 0, width, height)
        };

        let downscale = region_norm.is_none() && (crop_w > 1560 || crop_h > 1200);
        let out_w = if downscale { (crop_w / 2).max(1) } else { crop_w } as u32;
        let out_h = if downscale { (crop_h / 2).max(1) } else { crop_h } as u32;
        let mut rgba = Vec::with_capacity((out_w * out_h * 4) as usize);

        if downscale {
            for oy in 0..out_h as i32 {
                let sy0 = crop_y + oy * 2;
                let sy1 = (sy0 + 1).min(height - 1);
                for ox in 0..out_w as i32 {
                    let sx0 = crop_x + ox * 2;
                    let sx1 = (sx0 + 1).min(width - 1);
                    let i00 = ((sy0 * width + sx0) * 4) as usize;
                    let i10 = ((sy0 * width + sx1) * 4) as usize;
                    let i01 = ((sy1 * width + sx0) * 4) as usize;
                    let i11 = ((sy1 * width + sx1) * 4) as usize;
                    let b = ((bgra[i00] as u16 + bgra[i10] as u16 + bgra[i01] as u16 + bgra[i11] as u16) >> 2) as u8;
                    let g = ((bgra[i00 + 1] as u16 + bgra[i10 + 1] as u16 + bgra[i01 + 1] as u16 + bgra[i11 + 1] as u16) >> 2) as u8;
                    let r = ((bgra[i00 + 2] as u16 + bgra[i10 + 2] as u16 + bgra[i01 + 2] as u16 + bgra[i11 + 2] as u16) >> 2) as u8;
                    rgba.push(r);
                    rgba.push(g);
                    rgba.push(b);
                    rgba.push(255);
                }
            }
        } else {
            for oy in 0..out_h as i32 {
                let sy = crop_y + oy;
                for ox in 0..out_w as i32 {
                    let sx = crop_x + ox;
                    let idx = ((sy * width + sx) * 4) as usize;
                    rgba.push(bgra[idx + 2]);
                    rgba.push(bgra[idx + 1]);
                    rgba.push(bgra[idx]);
                    rgba.push(255);
                }
            }
        }

        let png_bytes = encode_png_compressed(out_w, out_h, &rgba)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);

        Ok(json!({
            "hwnd": win_meta.as_ref().map(|w| w.hwnd),
            "title": win_meta.as_ref().map(|w| w.title.clone()),
            "processName": win_meta.as_ref().map(|w| w.process_name.clone()),
            "offscreen": win_meta.as_ref().map(|w| w.is_offscreen).unwrap_or(false),
            "captureMethod": capture_method,
            "targetWidth": width,
            "targetHeight": height,
            "imageWidth": out_w,
            "imageHeight": out_h,
            "regionNorm": region_norm,
            "dataUrl": format!("data:image/png;base64,{}", b64),
        }))
    }
    #[cfg(not(windows))]
    {
        let _ = (target_hwnd, virtual_cursor, region_norm);
        Err("Screen capture is only supported on Windows.".to_string())
    }
}

pub fn capture_screen_png(target_hwnd: Option<usize>) -> Result<Value, String> {
    capture_window_or_desktop_png(target_hwnd, None, None)
}
