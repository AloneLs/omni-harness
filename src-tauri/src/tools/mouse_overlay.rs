use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickVisualKind {
    Left,
    Right,
    Double,
    Middle,
}

#[derive(Debug, Clone)]
pub struct ElementHoverInfo {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub label: String,
}

#[derive(Debug, Clone)]
pub enum OverlayEvent {
    Move {
        target_hwnd: usize,
        x: i32,
        y: i32,
        hover: Option<ElementHoverInfo>,
    },
    Click {
        target_hwnd: usize,
        x: i32,
        y: i32,
        kind: ClickVisualKind,
        action_label: String,
        hover: Option<ElementHoverInfo>,
    },
    Type {
        target_hwnd: usize,
        x: i32,
        y: i32,
        text: String,
        hover: Option<ElementHoverInfo>,
    },
    Hotkey {
        target_hwnd: usize,
        x: i32,
        y: i32,
        keys: String,
        hover: Option<ElementHoverInfo>,
    },
    SetEnabled(bool),
    Hide,
}

static OVERLAY_SENDER: OnceLock<mpsc::SyncSender<OverlayEvent>> = OnceLock::new();
static OVERLAY_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_overlay_enabled(enabled: bool) {
    OVERLAY_ENABLED.store(enabled, Ordering::SeqCst);
    if let Some(tx) = OVERLAY_SENDER.get() {
        let _ = tx.send(OverlayEvent::SetEnabled(enabled));
    }
}

pub fn is_overlay_enabled() -> bool {
    OVERLAY_ENABLED.load(Ordering::SeqCst)
}

pub fn notify_move(target_hwnd: usize, x: i32, y: i32, hover: Option<ElementHoverInfo>) {
    if !is_overlay_enabled() || target_hwnd == 0 {
        return;
    }
    let tx = get_or_start_overlay();
    let _ = tx.send(OverlayEvent::Move {
        target_hwnd,
        x,
        y,
        hover,
    });
}

pub fn notify_click(
    target_hwnd: usize,
    x: i32,
    y: i32,
    kind: ClickVisualKind,
    action_label: &str,
    hover: Option<ElementHoverInfo>,
) {
    if !is_overlay_enabled() || target_hwnd == 0 {
        return;
    }
    let tx = get_or_start_overlay();
    let _ = tx.send(OverlayEvent::Click {
        target_hwnd,
        x,
        y,
        kind,
        action_label: action_label.to_string(),
        hover,
    });
}

pub fn notify_type(
    target_hwnd: usize,
    x: i32,
    y: i32,
    text: &str,
    hover: Option<ElementHoverInfo>,
) {
    if !is_overlay_enabled() || target_hwnd == 0 {
        return;
    }
    let tx = get_or_start_overlay();
    let _ = tx.send(OverlayEvent::Type {
        target_hwnd,
        x,
        y,
        text: text.to_string(),
        hover,
    });
}

pub fn notify_hotkey(
    target_hwnd: usize,
    x: i32,
    y: i32,
    keys: &str,
    hover: Option<ElementHoverInfo>,
) {
    if !is_overlay_enabled() || target_hwnd == 0 {
        return;
    }
    let tx = get_or_start_overlay();
    let _ = tx.send(OverlayEvent::Hotkey {
        target_hwnd,
        x,
        y,
        keys: keys.to_string(),
        hover,
    });
}

pub fn notify_hide() {
    if let Some(tx) = OVERLAY_SENDER.get() {
        let _ = tx.send(OverlayEvent::Hide);
    }
}

fn get_or_start_overlay() -> &'static mpsc::SyncSender<OverlayEvent> {
    OVERLAY_SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<OverlayEvent>(128);
        #[cfg(target_os = "windows")]
        {
            std::thread::Builder::new()
                .name("omni-agent-cursor-overlay".to_string())
                .spawn(move || {
                    win_overlay_thread(rx);
                })
                .expect("Failed to start agent cursor overlay thread");
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::thread::spawn(move || {
                while let Ok(_) = rx.recv() {}
            });
        }
        tx
    })
}

#[cfg(target_os = "windows")]
mod win_ffi {
    use std::os::raw::{c_int, c_uint, c_void};

    pub type HWND = *mut c_void;
    pub type HDC = *mut c_void;
    pub type HBITMAP = *mut c_void;
    pub type HGDIOBJ = *mut c_void;
    pub type HINSTANCE = *mut c_void;
    pub type HBRUSH = *mut c_void;
    pub type HCURSOR = *mut c_void;
    pub type HICON = *mut c_void;
    pub type ATOM = u16;
    pub type BOOL = c_int;
    pub type UINT = c_uint;
    pub type DWORD = u32;
    pub type LPARAM = isize;
    pub type WPARAM = usize;
    pub type LRESULT = isize;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct SIZE {
        pub cx: i32,
        pub cy: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct RECT {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    #[repr(C)]
    pub struct WNDCLASSEXW {
        pub cbSize: UINT,
        pub style: UINT,
        pub lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>,
        pub cbClsExtra: c_int,
        pub cbWndExtra: c_int,
        pub hInstance: HINSTANCE,
        pub hIcon: HICON,
        pub hCursor: HCURSOR,
        pub hbrBackground: HBRUSH,
        pub lpszMenuName: *const u16,
        pub lpszClassName: *const u16,
        pub hIconSm: HICON,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct BLENDFUNCTION {
        pub BlendOp: u8,
        pub BlendFlags: u8,
        pub SourceConstantAlpha: u8,
        pub AlphaFormat: u8,
    }

    #[repr(C)]
    pub struct BITMAPINFOHEADER {
        pub biSize: DWORD,
        pub biWidth: i32,
        pub biHeight: i32,
        pub biPlanes: u16,
        pub biBitCount: u16,
        pub biCompression: DWORD,
        pub biSizeImage: DWORD,
        pub biXPelsPerMeter: i32,
        pub biYPelsPerMeter: i32,
        pub biClrUsed: DWORD,
        pub biClrImportant: DWORD,
    }

    #[repr(C)]
    pub struct BITMAPINFO {
        pub bmiHeader: BITMAPINFOHEADER,
        pub bmiColors: [DWORD; 1],
    }

    #[repr(C)]
    pub struct MSG {
        pub hwnd: HWND,
        pub message: UINT,
        pub wParam: WPARAM,
        pub lParam: LPARAM,
        pub time: DWORD,
        pub pt: POINT,
    }

    pub const WS_POPUP: DWORD = 0x80000000;
    pub const WS_EX_TOPMOST: DWORD = 0x00000008;
    pub const WS_EX_TRANSPARENT: DWORD = 0x00000020;
    pub const WS_EX_TOOLWINDOW: DWORD = 0x00000080;
    pub const WS_EX_LAYERED: DWORD = 0x00080000;
    pub const WS_EX_NOACTIVATE: DWORD = 0x08000000;

    pub const SW_HIDE: c_int = 0;
    pub const SW_SHOWNOACTIVATE: c_int = 8;
    pub const SWP_NOSIZE: UINT = 0x0001;
    pub const SWP_NOMOVE: UINT = 0x0002;
    pub const SWP_NOZORDER: UINT = 0x0004;
    pub const SWP_NOACTIVATE: UINT = 0x0010;
    pub const SWP_SHOWWINDOW: UINT = 0x0040;
    pub const HWND_TOPMOST: HWND = -1isize as HWND;

    pub const ULW_ALPHA: DWORD = 0x00000002;
    pub const AC_SRC_OVER: u8 = 0x00;
    pub const AC_SRC_ALPHA: u8 = 0x01;
    pub const BI_RGB: DWORD = 0;
    pub const DIB_RGB_COLORS: UINT = 0;

    pub const WDA_EXCLUDEFROMCAPTURE: DWORD = 0x00000011;
    pub const PM_REMOVE: UINT = 0x0001;

    extern "system" {
        pub fn GetModuleHandleW(lpModuleName: *const u16) -> HINSTANCE;
        pub fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> ATOM;
        pub fn CreateWindowExW(
            dwExStyle: DWORD,
            lpClassName: *const u16,
            lpWindowName: *const u16,
            dwStyle: DWORD,
            X: c_int,
            Y: c_int,
            nWidth: c_int,
            nHeight: c_int,
            hWndParent: HWND,
            hMenu: *mut c_void,
            hInstance: HINSTANCE,
            lpParam: *mut c_void,
        ) -> HWND;
        pub fn DestroyWindow(hWnd: HWND) -> BOOL;
        pub fn DefWindowProcW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        pub fn SetWindowDisplayAffinity(hWnd: HWND, dwAffinity: DWORD) -> BOOL;
        pub fn ShowWindow(hWnd: HWND, nCmdShow: c_int) -> BOOL;
        pub fn SetWindowPos(
            hWnd: HWND,
            hWndInsertAfter: HWND,
            X: c_int,
            Y: c_int,
            cx: c_int,
            cy: c_int,
            uFlags: UINT,
        ) -> BOOL;
        pub fn GetWindowRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
        pub fn IsWindow(hWnd: HWND) -> BOOL;
        pub fn IsWindowVisible(hWnd: HWND) -> BOOL;
        pub fn CreateCompatibleDC(hdc: HDC) -> HDC;
        pub fn DeleteDC(hdc: HDC) -> BOOL;
        pub fn CreateDIBSection(
            hdc: HDC,
            pbmi: *const BITMAPINFO,
            usage: UINT,
            ppvBits: *mut *mut c_void,
            hSection: *mut c_void,
            offset: DWORD,
        ) -> HBITMAP;
        pub fn SelectObject(hdc: HDC, h: HGDIOBJ) -> HGDIOBJ;
        pub fn DeleteObject(ho: HGDIOBJ) -> BOOL;
        pub fn UpdateLayeredWindow(
            hWnd: HWND,
            hdcDst: HDC,
            pptDst: *const POINT,
            psize: *const SIZE,
            hdcSrc: HDC,
            pptSrc: *const POINT,
            crKey: DWORD,
            pblend: *const BLENDFUNCTION,
            dwFlags: DWORD,
        ) -> BOOL;
        pub fn PeekMessageW(
            lpMsg: *mut MSG,
            hWnd: HWND,
            wMsgFilterMin: UINT,
            wMsgFilterMax: UINT,
            wRemoveMsg: UINT,
        ) -> BOOL;
        pub fn TranslateMessage(lpMsg: *const MSG) -> BOOL;
        pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn overlay_wnd_proc(
    hwnd: win_ffi::HWND,
    msg: win_ffi::UINT,
    wparam: win_ffi::WPARAM,
    lparam: win_ffi::LPARAM,
) -> win_ffi::LRESULT {
    win_ffi::DefWindowProcW(hwnd, msg, wparam, lparam)
}

struct OverlaySurface {
    mem_dc: win_ffi::HDC,
    dib_bitmap: win_ffi::HBITMAP,
    dib_bits: *mut u8,
    old_gdi_obj: win_ffi::HGDIOBJ,
    win_w: i32,
    win_h: i32,
}

impl OverlaySurface {
    fn new() -> Self {
        Self {
            mem_dc: std::ptr::null_mut(),
            dib_bitmap: std::ptr::null_mut(),
            dib_bits: std::ptr::null_mut(),
            old_gdi_obj: std::ptr::null_mut(),
            win_w: 0,
            win_h: 0,
        }
    }

    fn resize(&mut self, w: i32, h: i32) -> bool {
        let target_w = w.clamp(64, 3840);
        let target_h = h.clamp(64, 2160);

        unsafe {
            if !self.mem_dc.is_null() {
                if !self.old_gdi_obj.is_null() {
                    win_ffi::SelectObject(self.mem_dc, self.old_gdi_obj);
                    self.old_gdi_obj = std::ptr::null_mut();
                }
                if !self.dib_bitmap.is_null() {
                    win_ffi::DeleteObject(self.dib_bitmap as win_ffi::HGDIOBJ);
                    self.dib_bitmap = std::ptr::null_mut();
                    self.dib_bits = std::ptr::null_mut();
                }
            } else {
                self.mem_dc = win_ffi::CreateCompatibleDC(std::ptr::null_mut());
                if self.mem_dc.is_null() {
                    return false;
                }
            }

            let bmi = win_ffi::BITMAPINFO {
                bmiHeader: win_ffi::BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<win_ffi::BITMAPINFOHEADER>() as win_ffi::DWORD,
                    biWidth: target_w,
                    biHeight: -target_h, // top-down DIB
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: win_ffi::BI_RGB,
                    biSizeImage: (target_w * target_h * 4) as win_ffi::DWORD,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [0],
            };

            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            self.dib_bitmap = win_ffi::CreateDIBSection(
                self.mem_dc,
                &bmi,
                win_ffi::DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );

            if self.dib_bitmap.is_null() || bits.is_null() {
                return false;
            }

            self.dib_bits = bits as *mut u8;
            self.old_gdi_obj = win_ffi::SelectObject(self.mem_dc, self.dib_bitmap as win_ffi::HGDIOBJ);
            self.win_w = target_w;
            self.win_h = target_h;
            true
        }
    }
}

#[cfg(target_os = "windows")]
fn win_overlay_thread(rx: mpsc::Receiver<OverlayEvent>) {
    use std::ptr;
    use win_ffi::*;

    let class_name: Vec<u16> = "OmniAgentMouseOverlayClass\0".encode_utf16().collect();
    let hinst = unsafe { GetModuleHandleW(ptr::null()) };

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as UINT,
        style: 0,
        lpfnWndProc: Some(overlay_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: ptr::null_mut(),
        hCursor: ptr::null_mut(),
        hbrBackground: ptr::null_mut(),
        lpszMenuName: ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: ptr::null_mut(),
    };

    unsafe {
        RegisterClassExW(&wc);
    }

    let ex_style = WS_EX_LAYERED
        | WS_EX_TRANSPARENT
        | WS_EX_NOACTIVATE
        | WS_EX_TOPMOST
        | WS_EX_TOOLWINDOW;

    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            ptr::null(),
            WS_POPUP,
            0,
            0,
            100,
            100,
            ptr::null_mut(),
            ptr::null_mut(),
            hinst,
            ptr::null_mut(),
        )
    };

    if hwnd.is_null() {
        return;
    }

    // Exclude this overlay from screen captures (WDA_EXCLUDEFROMCAPTURE = 0x00000011).
    // This guarantees that any screenshots captured by computer_use will never see this overlay.
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    }

    let mut current_target_hwnd: usize = 0;
    let mut overlay_rect = RECT {
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
    };
    let mut surface = OverlaySurface::new();
    surface.resize(100, 100);

    let mut cur_x: f32 = 50.0;
    let mut cur_y: f32 = 50.0;
    let mut target_x: f32 = 50.0;
    let mut target_y: f32 = 50.0;

    let mut click_anim: Option<(ClickVisualKind, Instant, f32, f32)> = None;
    let mut status_badge: Option<(String, Instant)> = None;
    let mut active_hover: Option<ElementHoverInfo> = None;
    let mut last_action_time = Instant::now();
    let mut overlay_visible = false;
    let mut master_enabled = true;

    loop {
        // Process Windows message queue
        unsafe {
            let mut msg = MSG {
                hwnd: ptr::null_mut(),
                message: 0,
                wParam: 0,
                lParam: 0,
                time: 0,
                pt: POINT { x: 0, y: 0 },
            };
            while PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Drain incoming agent events
        while let Ok(event) = rx.try_recv() {
            match event {
                OverlayEvent::SetEnabled(en) => {
                    master_enabled = en;
                    if !en && overlay_visible {
                        unsafe {
                            ShowWindow(hwnd, SW_HIDE);
                        }
                        overlay_visible = false;
                    }
                }
                OverlayEvent::Hide => {
                    if overlay_visible {
                        unsafe {
                            ShowWindow(hwnd, SW_HIDE);
                        }
                        overlay_visible = false;
                    }
                }
                OverlayEvent::Move {
                    target_hwnd: th,
                    x,
                    y,
                    hover,
                } => {
                    current_target_hwnd = th;
                    target_x = x as f32;
                    target_y = y as f32;
                    active_hover = hover;
                    last_action_time = Instant::now();
                }
                OverlayEvent::Click {
                    target_hwnd: th,
                    x,
                    y,
                    kind,
                    action_label,
                    hover,
                } => {
                    current_target_hwnd = th;
                    target_x = x as f32;
                    target_y = y as f32;
                    cur_x = target_x;
                    cur_y = target_y;
                    click_anim = Some((kind, Instant::now(), cur_x, cur_y));
                    status_badge = Some((action_label, Instant::now()));
                    active_hover = hover;
                    last_action_time = Instant::now();
                }
                OverlayEvent::Type {
                    target_hwnd: th,
                    x,
                    y,
                    text,
                    hover,
                } => {
                    current_target_hwnd = th;
                    target_x = x as f32;
                    target_y = y as f32;
                    cur_x = target_x;
                    cur_y = target_y;
                    let display_text = if text.chars().count() > 16 {
                        format!("⌨ \"{}...\"", text.chars().take(14).collect::<String>())
                    } else {
                        format!("⌨ \"{}\"", text)
                    };
                    status_badge = Some((display_text, Instant::now()));
                    active_hover = hover;
                    last_action_time = Instant::now();
                }
                OverlayEvent::Hotkey {
                    target_hwnd: th,
                    x,
                    y,
                    keys,
                    hover,
                } => {
                    current_target_hwnd = th;
                    target_x = x as f32;
                    target_y = y as f32;
                    cur_x = target_x;
                    cur_y = target_y;
                    status_badge = Some((format!("[{}]", keys), Instant::now()));
                    active_hover = hover;
                    last_action_time = Instant::now();
                }
            }
        }

        if !master_enabled {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }

        let elapsed_since_action = last_action_time.elapsed().as_secs_f32();
        if elapsed_since_action > 2.5 {
            if overlay_visible {
                unsafe {
                    ShowWindow(hwnd, SW_HIDE);
                }
                overlay_visible = false;
            }
            std::thread::sleep(Duration::from_millis(30));
            continue;
        }

        // Validate and bound to the target window
        if current_target_hwnd != 0 {
            let mut target_rect = RECT::default();
            let is_valid = unsafe {
                IsWindow(current_target_hwnd as HWND) != 0
                    && GetWindowRect(current_target_hwnd as HWND, &mut target_rect) != 0
            };

            if is_valid {
                let tw = (target_rect.right - target_rect.left).max(64);
                let th = (target_rect.bottom - target_rect.top).max(64);

                let moved = target_rect.left != overlay_rect.left
                    || target_rect.top != overlay_rect.top
                    || tw != surface.win_w
                    || th != surface.win_h;

                if moved {
                    surface.resize(tw, th);
                    overlay_rect = target_rect;
                    unsafe {
                        SetWindowPos(
                            hwnd,
                            HWND_TOPMOST,
                            target_rect.left,
                            target_rect.top,
                            tw,
                            th,
                            SWP_NOACTIVATE | SWP_SHOWWINDOW,
                        );
                    }
                }
            }
        }

        if !overlay_visible {
            unsafe {
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            overlay_visible = true;
        }

        // Smooth trajectory interpolation
        let dx = target_x - cur_x;
        let dy = target_y - cur_y;
        if dx.abs() > 0.5 || dy.abs() > 0.5 {
            cur_x += dx * 0.35;
            cur_y += dy * 0.35;
        } else {
            cur_x = target_x;
            cur_y = target_y;
        }

        // Compute global fade alpha
        let global_alpha: f32 = if elapsed_since_action < 1.4 {
            1.0
        } else {
            ((2.5 - elapsed_since_action) / 1.1).clamp(0.0, 1.0)
        };

        if !surface.dib_bits.is_null() && surface.win_w > 0 && surface.win_h > 0 {
            let total_bytes = (surface.win_w * surface.win_h * 4) as usize;
            unsafe {
                std::ptr::write_bytes(surface.dib_bits, 0, total_bytes);
            }

            let slice = unsafe { std::slice::from_raw_parts_mut(surface.dib_bits, total_bytes) };

            // 1. Draw Element Hover Box if target element bounds exist
            if let Some(ref h) = active_hover {
                draw_element_hover_box(slice, surface.win_w, surface.win_h, h, global_alpha);
            }

            // 2. Draw Click Ripple Animation if active
            if let Some((kind, start_time, cx, cy)) = click_anim {
                let t = start_time.elapsed().as_secs_f32();
                if t < 0.45 {
                    let progress = (t / 0.45).clamp(0.0, 1.0);
                    draw_click_rings(slice, surface.win_w, surface.win_h, cx, cy, kind, progress, global_alpha);
                } else {
                    click_anim = None;
                }
            }

            // 3. Draw Virtual Agent Cursor (Obsidian + Neon Purple silhouette)
            draw_agent_cursor(slice, surface.win_w, surface.win_h, cur_x, cur_y, global_alpha);

            // 4. Draw Status Badge / Pill
            if let Some((ref text, start_time)) = status_badge {
                let t = start_time.elapsed().as_secs_f32();
                if t < 1.6 {
                    let badge_alpha = if t < 1.1 {
                        1.0
                    } else {
                        ((1.6 - t) / 0.5).clamp(0.0, 1.0)
                    } * global_alpha;

                    draw_status_badge(slice, surface.win_w, surface.win_h, cur_x + 16.0, cur_y + 14.0, text, badge_alpha);
                } else {
                    status_badge = None;
                }
            }

            // Update layered window with the rendered premultiplied 32-bit ARGB surface
            let pt_src = POINT { x: 0, y: 0 };
            let pt_dst = POINT {
                x: overlay_rect.left,
                y: overlay_rect.top,
            };
            let sz = SIZE {
                cx: surface.win_w,
                cy: surface.win_h,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA,
            };

            unsafe {
                UpdateLayeredWindow(
                    hwnd,
                    ptr::null_mut(),
                    &pt_dst,
                    &sz,
                    surface.mem_dc,
                    &pt_src,
                    0,
                    &blend,
                    ULW_ALPHA,
                );
            }
        }

        std::thread::sleep(Duration::from_millis(16)); // ~60 FPS
    }
}

// Blend a premultiplied pixel into the 32-bit BGRA buffer
#[inline(always)]
fn blend_pixel(buf: &mut [u8], w: i32, h: i32, x: i32, y: i32, r: u8, g: u8, b: u8, a: u8) {
    if x < 0 || y < 0 || x >= w || y >= h || a == 0 {
        return;
    }
    let idx = ((y * w + x) * 4) as usize;
    if idx + 3 >= buf.len() {
        return;
    }

    let sa = a as u32;
    let inv_sa = 255 - sa;

    // Premultiplied source color components
    let sr = (r as u32 * sa) / 255;
    let sg = (g as u32 * sa) / 255;
    let sb = (b as u32 * sa) / 255;

    let db = buf[idx] as u32;
    let dg = buf[idx + 1] as u32;
    let dr = buf[idx + 2] as u32;
    let da = buf[idx + 3] as u32;

    buf[idx] = (sb + (db * inv_sa) / 255).min(255) as u8;
    buf[idx + 1] = (sg + (dg * inv_sa) / 255).min(255) as u8;
    buf[idx + 2] = (sr + (dr * inv_sa) / 255).min(255) as u8;
    buf[idx + 3] = (sa + (da * inv_sa) / 255).min(255) as u8;
}

// Draw the distinct, futuristic agent pointer (Obsidian body with vibrant purple neon outline and core)
fn draw_agent_cursor(buf: &mut [u8], w: i32, h: i32, cx: f32, cy: f32, alpha: f32) {
    if alpha <= 0.01 {
        return;
    }
    let ox = cx.round() as i32;
    let oy = cy.round() as i32;

    // 22x22 custom agent arrow mask
    // 0: transparent, 1: outer purple glow, 2: dark obsidian body, 3: core neon dot
    #[rustfmt::skip]
    let cursor_mask: [[u8; 18]; 22] = [
        [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [1,2,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [1,2,2,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [1,2,2,2,1,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [1,2,3,2,2,1,0,0,0,0,0,0,0,0,0,0,0,0],
        [1,2,3,3,2,2,1,0,0,0,0,0,0,0,0,0,0,0],
        [1,2,2,3,3,2,2,1,0,0,0,0,0,0,0,0,0,0],
        [1,2,2,2,3,2,2,2,1,0,0,0,0,0,0,0,0,0],
        [1,2,2,2,2,2,2,2,2,1,0,0,0,0,0,0,0,0],
        [1,2,2,2,2,2,2,2,2,2,1,0,0,0,0,0,0,0],
        [1,2,2,2,2,2,1,1,1,1,1,1,0,0,0,0,0,0],
        [1,2,2,1,2,2,1,0,0,0,0,0,0,0,0,0,0,0],
        [1,2,1,0,1,2,2,1,0,0,0,0,0,0,0,0,0,0],
        [1,1,0,0,1,2,2,1,0,0,0,0,0,0,0,0,0,0],
        [1,0,0,0,0,1,2,2,1,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,1,2,2,1,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,1,2,2,1,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,1,2,2,1,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,1,1,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
    ];

    for (row, line) in cursor_mask.iter().enumerate() {
        for (col, &pixel) in line.iter().enumerate() {
            let px = ox + col as i32;
            let py = oy + row as i32;
            match pixel {
                1 => {
                    // Outer neon violet edge
                    let a = (240.0 * alpha) as u8;
                    blend_pixel(buf, w, h, px, py, 168, 85, 247, a); // #A855F7
                }
                2 => {
                    // Dark obsidian body
                    let a = (245.0 * alpha) as u8;
                    blend_pixel(buf, w, h, px, py, 18, 18, 22, a); // #121216
                }
                3 => {
                    // Core neon accent point
                    let a = (255.0 * alpha) as u8;
                    blend_pixel(buf, w, h, px, py, 216, 180, 254, a); // #D8B4FE
                }
                _ => {}
            }
        }
    }
}

// Draw expanding click rings (LMB = Purple/Cyan, RMB = Amber/Rose, Double = Dual Ring)
fn draw_click_rings(
    buf: &mut [u8],
    w: i32,
    h: i32,
    cx: f32,
    cy: f32,
    kind: ClickVisualKind,
    progress: f32,
    alpha: f32,
) {
    let (r, g, b) = match kind {
        ClickVisualKind::Left => (168, 85, 247),       // Purple #A855F7
        ClickVisualKind::Right => (245, 158, 11),     // Amber #F59E0B
        ClickVisualKind::Double => (56, 189, 248),    // Sky Cyan #38BDF8
        ClickVisualKind::Middle => (147, 51, 234),    // Violet
    };

    let max_radius = match kind {
        ClickVisualKind::Double => 36.0,
        _ => 28.0,
    };

    let current_radius = 4.0 + (max_radius - 4.0) * progress;
    let ring_alpha = ((1.0 - progress).powf(1.4) * 240.0 * alpha) as u8;

    if ring_alpha > 0 {
        draw_circle_outline(buf, w, h, cx, cy, current_radius, 2.0, r, g, b, ring_alpha);
        if kind == ClickVisualKind::Double && progress > 0.15 {
            let inner_radius = 4.0 + (max_radius - 4.0) * (progress - 0.15);
            let inner_alpha = ((1.0 - progress).powf(1.2) * 200.0 * alpha) as u8;
            draw_circle_outline(buf, w, h, cx, cy, inner_radius, 1.6, 192, 132, 252, inner_alpha);
        }
    }
}

fn draw_circle_outline(
    buf: &mut [u8],
    w: i32,
    h: i32,
    cx: f32,
    cy: f32,
    radius: f32,
    thickness: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    let r_min = (radius - thickness).max(0.0);
    let r_max = radius + thickness;
    let min_x = (cx - r_max).floor() as i32;
    let max_x = (cx + r_max).ceil() as i32;
    let min_y = (cy - r_max).floor() as i32;
    let max_y = (cy + r_max).ceil() as i32;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist >= r_min && dist <= r_max {
                let factor = 1.0 - ((dist - radius).abs() / thickness).clamp(0.0, 1.0);
                let pixel_a = (a as f32 * factor) as u8;
                blend_pixel(buf, w, h, x, y, r, g, b, pixel_a);
            }
        }
    }
}

// Draw Element Hover Bounding Box with rounded corners and top badge
fn draw_element_hover_box(
    buf: &mut [u8],
    w: i32,
    h: i32,
    hover: &ElementHoverInfo,
    alpha: f32,
) {
    if alpha <= 0.01 || hover.width <= 0 || hover.height <= 0 {
        return;
    }

    let x1 = hover.x;
    let y1 = hover.y;
    let x2 = hover.x + hover.width;
    let y2 = hover.y + hover.height;

    let border_a = (160.0 * alpha) as u8;
    let fill_a = (22.0 * alpha) as u8;

    // Fill semi-transparent hover background
    for y in y1.max(0)..y2.min(h) {
        for x in x1.max(0)..x2.min(w) {
            blend_pixel(buf, w, h, x, y, 168, 85, 247, fill_a);
        }
    }

    // Outline borders
    for x in x1.max(0)..x2.min(w) {
        blend_pixel(buf, w, h, x, y1, 168, 85, 247, border_a);
        blend_pixel(buf, w, h, x, y2.min(h - 1), 168, 85, 247, border_a);
    }
    for y in y1.max(0)..y2.min(h) {
        blend_pixel(buf, w, h, x1, y, 168, 85, 247, border_a);
        blend_pixel(buf, w, h, x2.min(w - 1), y, 168, 85, 247, border_a);
    }

    // Element badge if label exists
    if !hover.label.is_empty() {
        let max_x = (w - 80).max(4);
        let badge_x = (x1 + 4).clamp(4, max_x) as f32;
        let badge_y = (y1 - 18).max(4) as f32;
        draw_status_badge(buf, w, h, badge_x, badge_y, &hover.label, alpha * 0.9);
    }
}

// Draw Status Badge / Pill with high readability
fn draw_status_badge(
    buf: &mut [u8],
    w: i32,
    h: i32,
    bx: f32,
    by: f32,
    text: &str,
    alpha: f32,
) {
    if alpha <= 0.01 || text.is_empty() {
        return;
    }

    let char_count = text.chars().count().min(24);
    let pad_x = 7;
    let pad_y = 4;
    let char_w = 6;
    let char_h = 9;

    let badge_w = pad_x * 2 + char_count as i32 * char_w;
    let badge_h = pad_y * 2 + char_h;

    let x0 = (bx.round() as i32).clamp(4, (w - badge_w - 4).max(4));
    let y0 = (by.round() as i32).clamp(4, (h - badge_h - 4).max(4));

    let bg_a = (220.0 * alpha) as u8;
    let stroke_a = (180.0 * alpha) as u8;
    let text_a = (255.0 * alpha) as u8;

    // Draw dark pill background
    for y in y0..(y0 + badge_h) {
        for x in x0..(x0 + badge_w) {
            let is_corner = (x == x0 || x == x0 + badge_w - 1) && (y == y0 || y == y0 + badge_h - 1);
            if !is_corner {
                blend_pixel(buf, w, h, x, y, 15, 15, 20, bg_a);
            }
        }
    }

    // Draw purple stroke border
    for x in (x0 + 1)..(x0 + badge_w - 1) {
        blend_pixel(buf, w, h, x, y0, 168, 85, 247, stroke_a);
        blend_pixel(buf, w, h, x, y0 + badge_h - 1, 168, 85, 247, stroke_a);
    }
    for y in (y0 + 1)..(y0 + badge_h - 1) {
        blend_pixel(buf, w, h, x0, y, 168, 85, 247, stroke_a);
        blend_pixel(buf, w, h, x0 + badge_w - 1, y, 168, 85, 247, stroke_a);
    }

    // Draw text characters using embedded clean 5x7 font
    let mut cursor_x = x0 + pad_x;
    let cursor_y = y0 + pad_y;

    for ch in text.chars().take(char_count) {
        draw_font_glyph(buf, w, h, cursor_x, cursor_y, ch, text_a);
        cursor_x += char_w;
    }
}

// 5x7 compact monospace font renderer for sharp transparent text
fn draw_font_glyph(buf: &mut [u8], w: i32, h: i32, gx: i32, gy: i32, ch: char, alpha: u8) {
    let rows: [u8; 7] = match ch {
        'A' | 'a' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x00],
        'B' | 'b' => [0x1E, 0x11, 0x1E, 0x11, 0x11, 0x1E, 0x00],
        'C' | 'c' => [0x0E, 0x11, 0x10, 0x10, 0x11, 0x0E, 0x00],
        'D' | 'd' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x1E, 0x00],
        'E' | 'e' => [0x1F, 0x10, 0x1E, 0x10, 0x10, 0x1F, 0x00],
        'F' | 'f' => [0x1F, 0x10, 0x1E, 0x10, 0x10, 0x10, 0x00],
        'G' | 'g' => [0x0E, 0x11, 0x10, 0x13, 0x11, 0x0F, 0x00],
        'H' | 'h' => [0x11, 0x11, 0x1F, 0x11, 0x11, 0x11, 0x00],
        'I' | 'i' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x0E, 0x00],
        'J' | 'j' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x0C, 0x00],
        'K' | 'k' => [0x11, 0x12, 0x1C, 0x12, 0x11, 0x11, 0x00],
        'L' | 'l' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x1F, 0x00],
        'M' | 'm' => [0x11, 0x1B, 0x15, 0x11, 0x11, 0x11, 0x00],
        'N' | 'n' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x00],
        'O' | 'o' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x0E, 0x00],
        'P' | 'p' => [0x1E, 0x11, 0x1E, 0x10, 0x10, 0x10, 0x00],
        'Q' | 'q' => [0x0E, 0x11, 0x11, 0x15, 0x12, 0x0D, 0x00],
        'R' | 'r' => [0x1E, 0x11, 0x1E, 0x14, 0x12, 0x11, 0x00],
        'S' | 's' => [0x0F, 0x10, 0x0E, 0x01, 0x01, 0x1E, 0x00],
        'T' | 't' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x00],
        'U' | 'u' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0E, 0x00],
        'V' | 'v' => [0x11, 0x11, 0x11, 0x11, 0x0A, 0x04, 0x00],
        'W' | 'w' => [0x11, 0x11, 0x11, 0x15, 0x1B, 0x11, 0x00],
        'X' | 'x' => [0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11, 0x00],
        'Y' | 'y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x00],
        'Z' | 'z' => [0x1F, 0x02, 0x04, 0x08, 0x10, 0x1F, 0x00],
        '0' => [0x0E, 0x13, 0x15, 0x19, 0x11, 0x0E, 0x00],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x0E, 0x00],
        '2' => [0x0E, 0x11, 0x02, 0x04, 0x08, 0x1F, 0x00],
        '3' => [0x1F, 0x02, 0x06, 0x01, 0x11, 0x0E, 0x00],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x00],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x1E, 0x00],
        '6' => [0x06, 0x08, 0x1E, 0x11, 0x11, 0x0E, 0x00],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x00],
        '8' => [0x0E, 0x11, 0x0E, 0x11, 0x11, 0x0E, 0x00],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x0E, 0x00],
        '[' => [0x0E, 0x08, 0x08, 0x08, 0x08, 0x0E, 0x00],
        ']' => [0x0E, 0x02, 0x02, 0x02, 0x02, 0x0E, 0x00],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x04, 0x02, 0x00],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x04, 0x08, 0x00],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x06],
        ':' => [0x00, 0x06, 0x06, 0x00, 0x06, 0x06, 0x00],
        '"' => [0x0A, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00],
        '\'' => [0x04, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    };

    for (row, byte) in rows.iter().enumerate() {
        for col in 0..5 {
            if (byte & (0x10 >> col)) != 0 {
                blend_pixel(buf, w, h, gx + col, gy + row as i32, 237, 237, 239, alpha);
            }
        }
    }
}
