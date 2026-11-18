use crate::tools::desktop_uia;
use crate::tools::phantom_session;
use serde_json::{json, Value};
use std::sync::mpsc::{self, SyncSender};
use std::sync::OnceLock;

enum GhostCommand {
    Mouse {
        chat_id: Option<String>,
        root_hwnd: usize,
        explicit_child: Option<usize>,
        action: String,
        x: i32,
        y: i32,
        to_x: Option<i32>,
        to_y: Option<i32>,
        delta: Option<i32>,
        normalized: bool,
        reply: SyncSender<Result<Value, String>>,
    },
    TypeText {
        chat_id: Option<String>,
        root_hwnd: usize,
        explicit_child: Option<usize>,
        text: String,
        clear_first: bool,
        submit_enter: bool,
        reply: SyncSender<Result<Value, String>>,
    },
    Hotkey {
        chat_id: Option<String>,
        root_hwnd: usize,
        combo: String,
        reply: SyncSender<Result<Value, String>>,
    },
}

static GHOST_WORKER: OnceLock<SyncSender<GhostCommand>> = OnceLock::new();

fn worker() -> &'static SyncSender<GhostCommand> {
    GHOST_WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<GhostCommand>(64);
        std::thread::Builder::new()
            .name("omni-ghost-input".to_string())
            .spawn(move || {
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        GhostCommand::Mouse {
                            chat_id,
                            root_hwnd,
                            explicit_child,
                            action,
                            x,
                            y,
                            to_x,
                            to_y,
                            delta,
                            normalized,
                            reply,
                        } => {
                            let res = handle_mouse_on_worker(
                                chat_id.as_deref(),
                                root_hwnd,
                                explicit_child,
                                &action,
                                x,
                                y,
                                to_x,
                                to_y,
                                delta,
                                normalized,
                            );
                            let _ = reply.send(res);
                        }
                        GhostCommand::TypeText {
                            chat_id,
                            root_hwnd,
                            explicit_child,
                            text,
                            clear_first,
                            submit_enter,
                            reply,
                        } => {
                            let res = handle_type_on_worker(
                                chat_id.as_deref(),
                                root_hwnd,
                                explicit_child,
                                &text,
                                clear_first,
                                submit_enter,
                            );
                            let _ = reply.send(res);
                        }
                        GhostCommand::Hotkey {
                            chat_id,
                            root_hwnd,
                            combo,
                            reply,
                        } => {
                            let res = handle_hotkey_on_worker(chat_id.as_deref(), root_hwnd, &combo);
                            let _ = reply.send(res);
                        }
                    }
                }
            })
            .expect("failed to spawn ghost input worker");
        tx
    })
}

pub fn ghost_mouse_action(
    chat_id: Option<&str>,
    root_hwnd: usize,
    explicit_child: Option<usize>,
    action: &str,
    x: i32,
    y: i32,
    to_x: Option<i32>,
    to_y: Option<i32>,
    delta: Option<i32>,
    normalized: bool,
) -> Result<Value, String> {
    let (rtx, rrx) = mpsc::sync_channel(1);
    worker()
        .send(GhostCommand::Mouse {
            chat_id: chat_id.map(|s| s.to_string()),
            root_hwnd,
            explicit_child,
            action: action.to_string(),
            x,
            y,
            to_x,
            to_y,
            delta,
            normalized,
            reply: rtx,
        })
        .map_err(|e| format!("Ghost worker unavailable: {}", e))?;
    rrx.recv()
        .map_err(|e| format!("Ghost worker dropped reply: {}", e))?
}

pub fn ghost_type_text(
    chat_id: Option<&str>,
    root_hwnd: usize,
    explicit_child: Option<usize>,
    text: &str,
    clear_first: bool,
    submit_enter: bool,
) -> Result<Value, String> {
    let (rtx, rrx) = mpsc::sync_channel(1);
    worker()
        .send(GhostCommand::TypeText {
            chat_id: chat_id.map(|s| s.to_string()),
            root_hwnd,
            explicit_child,
            text: text.to_string(),
            clear_first,
            submit_enter,
            reply: rtx,
        })
        .map_err(|e| format!("Ghost worker unavailable: {}", e))?;
    rrx.recv()
        .map_err(|e| format!("Ghost worker dropped reply: {}", e))?
}

pub fn ghost_send_hotkey(
    chat_id: Option<&str>,
    root_hwnd: usize,
    combo: &str,
) -> Result<Value, String> {
    let (rtx, rrx) = mpsc::sync_channel(1);
    worker()
        .send(GhostCommand::Hotkey {
            chat_id: chat_id.map(|s| s.to_string()),
            root_hwnd,
            combo: combo.to_string(),
            reply: rtx,
        })
        .map_err(|e| format!("Ghost worker unavailable: {}", e))?;
    rrx.recv()
        .map_err(|e| format!("Ghost worker dropped reply: {}", e))?
}

#[cfg(windows)]
mod win_msg {
    use std::ffi::c_void;

    pub type HWND = *mut c_void;
    pub type BOOL = i32;
    pub type UINT = u32;
    pub type DWORD = u32;
    pub type WPARAM = usize;
    pub type LPARAM = isize;
    pub type LRESULT = isize;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct RECT {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    pub const WM_SETFOCUS: UINT = 0x0007;
    pub const WM_ACTIVATE: UINT = 0x0006;
    pub const WM_NCACTIVATE: UINT = 0x0086;
    pub const WM_SETCURSOR: UINT = 0x0020;
    pub const WM_CONTEXTMENU: UINT = 0x007B;
    pub const WM_KEYDOWN: UINT = 0x0100;
    pub const WM_KEYUP: UINT = 0x0101;
    pub const WM_CHAR: UINT = 0x0102;
    pub const WM_SYSKEYDOWN: UINT = 0x0104;
    pub const WM_SYSKEYUP: UINT = 0x0105;
    pub const WM_SYSCHAR: UINT = 0x0106;
    pub const WM_MOUSEMOVE: UINT = 0x0200;
    pub const WM_LBUTTONDOWN: UINT = 0x0201;
    pub const WM_LBUTTONUP: UINT = 0x0202;
    pub const WM_LBUTTONDBLCLK: UINT = 0x0203;
    pub const WM_RBUTTONDOWN: UINT = 0x0204;
    pub const WM_RBUTTONUP: UINT = 0x0205;
    pub const WM_MBUTTONDOWN: UINT = 0x0207;
    pub const WM_MBUTTONUP: UINT = 0x0208;
    pub const WM_MOUSEWHEEL: UINT = 0x020A;
    pub const WM_MOUSEHWHEEL: UINT = 0x020E;
    pub const EM_SETSEL: UINT = 0x00B1;
    pub const WM_CLEAR: UINT = 0x0303;

    pub const MK_LBUTTON: WPARAM = 0x0001;
    pub const MK_RBUTTON: WPARAM = 0x0002;
    pub const MK_MBUTTON: WPARAM = 0x0010;
    pub const WA_ACTIVE: WPARAM = 1;
    pub const SMTO_ABORTIFHUNG: UINT = 0x0002;
    pub const GWL_STYLE: i32 = -16;
    pub const WS_DISABLED: isize = 0x08000000;

    extern "system" {
        pub fn IsWindow(hWnd: HWND) -> BOOL;
        pub fn IsWindowVisible(hWnd: HWND) -> BOOL;
        pub fn GetWindowRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
        pub fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> isize;
        pub fn GetClassNameW(hWnd: HWND, lpClassName: *mut u16, nMaxCount: i32) -> i32;
        pub fn EnumChildWindows(
            hWndParent: HWND,
            lpEnumFunc: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>,
            lParam: LPARAM,
        ) -> BOOL;
        pub fn ScreenToClient(hWnd: HWND, lpPoint: *mut POINT) -> BOOL;
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
        pub fn GetForegroundWindow() -> HWND;
        pub fn GetWindowThreadProcessId(hWnd: HWND, lpdwProcessId: *mut DWORD) -> DWORD;
        pub fn GetCurrentThreadId() -> DWORD;
        pub fn AttachThreadInput(idAttach: DWORD, idAttachTo: DWORD, fAttach: BOOL) -> BOOL;
        pub fn GetKeyboardState(lpKeyState: *mut u8) -> BOOL;
        pub fn SetKeyboardState(lpKeyState: *const u8) -> BOOL;
        pub fn MapVirtualKeyW(uCode: UINT, uMapType: UINT) -> UINT;
    }
}

#[cfg(windows)]
fn pack_lparam(x: i32, y: i32) -> isize {
    let lx = (x as i16 as u16) as u32;
    let ly = (y as i16 as u16) as u32;
    ((ly << 16) | lx) as isize
}

#[cfg(windows)]
struct ChildHitCandidate {
    hwnd: usize,
    area: i64,
    priority_bonus: i32,
}

#[cfg(windows)]
struct ChildSearchCtx {
    sx: i32,
    sy: i32,
    candidates: Vec<ChildHitCandidate>,
}

#[cfg(windows)]
unsafe extern "system" fn enum_hit_children_cb(
    hwnd: win_msg::HWND,
    lparam: win_msg::LPARAM,
) -> win_msg::BOOL {
    let ctx = &mut *(lparam as *mut ChildSearchCtx);
    if win_msg::IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let style = win_msg::GetWindowLongPtrW(hwnd, win_msg::GWL_STYLE);
    if (style & win_msg::WS_DISABLED) != 0 {
        return 1;
    }
    let mut r = win_msg::RECT::default();
    if win_msg::GetWindowRect(hwnd, &mut r) == 0 {
        return 1;
    }
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w <= 1 || h <= 1 {
        return 1;
    }
    if ctx.sx < r.left || ctx.sx >= r.right || ctx.sy < r.top || ctx.sy >= r.bottom {
        return 1;
    }

    let mut cbuf = [0u16; 128];
    let clen = win_msg::GetClassNameW(hwnd, cbuf.as_mut_ptr(), 128);
    let cname = String::from_utf16_lossy(&cbuf[..clen.max(0) as usize]).to_ascii_lowercase();
    if cname == "button" && (style & 0x0F) == 0x07 {
        return 1;
    }

    let bonus = if cname.contains("renderwidgethost")
        || cname.contains("corewindow")
        || cname.contains("edit")
        || cname.contains("scintilla")
        || cname.contains("directui")
    {
        1
    } else {
        0
    };

    ctx.candidates.push(ChildHitCandidate {
        hwnd: hwnd as usize,
        area: (w as i64) * (h as i64),
        priority_bonus: bonus,
    });
    1
}

#[cfg(windows)]
unsafe fn resolve_child_and_client_coords(
    root_hwnd: usize,
    explicit_child: Option<usize>,
    screen_x: i32,
    screen_y: i32,
) -> (usize, i32, i32) {
    let chosen = if let Some(ch) = explicit_child.filter(|&h| h != 0 && win_msg::IsWindow(h as win_msg::HWND) != 0) {
        ch
    } else {
        let mut ctx = ChildSearchCtx {
            sx: screen_x,
            sy: screen_y,
            candidates: Vec::new(),
        };
        win_msg::EnumChildWindows(
            root_hwnd as win_msg::HWND,
            Some(enum_hit_children_cb),
            &mut ctx as *mut ChildSearchCtx as win_msg::LPARAM,
        );
        ctx.candidates.sort_by(|a, b| {
            a.area
                .cmp(&b.area)
                .then_with(|| b.priority_bonus.cmp(&a.priority_bonus))
        });
        ctx.candidates
            .first()
            .map(|c| c.hwnd)
            .unwrap_or(root_hwnd)
    };

    let mut pt = win_msg::POINT {
        x: screen_x,
        y: screen_y,
    };
    win_msg::ScreenToClient(chosen as win_msg::HWND, &mut pt);
    (chosen, pt.x, pt.y)
}

#[cfg(windows)]
unsafe fn send_background_activation_hint(root_hwnd: usize, child_hwnd: usize) -> Result<(u32, bool), String> {
    let mut target_pid = 0u32;
    let target_tid = win_msg::GetWindowThreadProcessId(child_hwnd as win_msg::HWND, &mut target_pid);
    if target_pid == std::process::id() {
        return Err("Refusing to send ghost input to OmniHarness's own window.".to_string());
    }
    let fg_hwnd = win_msg::GetForegroundWindow();
    let fg_tid = if fg_hwnd.is_null() {
        0
    } else {
        win_msg::GetWindowThreadProcessId(fg_hwnd, std::ptr::null_mut())
    };
    let is_background = target_tid != 0 && target_tid != fg_tid;
    if is_background {
        let mut dummy = 0usize;
        let _ = win_msg::SendMessageTimeoutW(
            root_hwnd as win_msg::HWND,
            win_msg::WM_NCACTIVATE,
            1,
            0,
            win_msg::SMTO_ABORTIFHUNG,
            45,
            &mut dummy,
        );
        let _ = win_msg::SendMessageTimeoutW(
            root_hwnd as win_msg::HWND,
            win_msg::WM_ACTIVATE,
            win_msg::WA_ACTIVE,
            0,
            win_msg::SMTO_ABORTIFHUNG,
            45,
            &mut dummy,
        );
        let _ = win_msg::SendMessageTimeoutW(
            child_hwnd as win_msg::HWND,
            win_msg::WM_SETFOCUS,
            0,
            0,
            win_msg::SMTO_ABORTIFHUNG,
            45,
            &mut dummy,
        );
    }
    Ok((target_tid, is_background))
}

#[cfg(windows)]
fn resolve_window_relative_xy(
    win_x: i32,
    win_y: i32,
    win_w: i32,
    win_h: i32,
    x: i32,
    y: i32,
    normalized: bool,
) -> (i32, i32, i32, i32) {
    let (rel_x, rel_y) = if normalized {
        let rx = ((x.clamp(0, 1000) as f64 / 1000.0) * win_w.max(1) as f64).round() as i32;
        let ry = ((y.clamp(0, 1000) as f64 / 1000.0) * win_h.max(1) as f64).round() as i32;
        (rx.clamp(0, (win_w - 1).max(0)), ry.clamp(0, (win_h - 1).max(0)))
    } else if x >= 0 && x <= win_w && y >= 0 && y <= win_h {
        (x, y)
    } else if x >= win_x && x <= win_x + win_w && y >= win_y && y <= win_y + win_h {
        (x - win_x, y - win_y)
    } else {
        (x.clamp(0, (win_w - 1).max(0)), y.clamp(0, (win_h - 1).max(0)))
    };
    (rel_x, rel_y, win_x + rel_x, win_y + rel_y)
}

fn handle_mouse_on_worker(
    chat_id: Option<&str>,
    root_hwnd: usize,
    explicit_child: Option<usize>,
    action: &str,
    x: i32,
    y: i32,
    to_x: Option<i32>,
    to_y: Option<i32>,
    delta: Option<i32>,
    normalized: bool,
) -> Result<Value, String> {
    #[cfg(windows)]
    unsafe {
        if desktop_uia::is_panic_hotkey_active() {
            return Err("Computer use aborted by panic hotkey (Ctrl+Alt+Shift+Esc).".to_string());
        }
        let root = desktop_uia::resolve_root_hwnd(root_hwnd);
        let win_info = desktop_uia::query_window_info(root)
            .ok_or_else(|| "Target window is no longer valid.".to_string())?;
        let (rel_x, rel_y, sx, sy) = resolve_window_relative_xy(
            win_info.bounds.x,
            win_info.bounds.y,
            win_info.bounds.width,
            win_info.bounds.height,
            x,
            y,
            normalized,
        );

        let (child_hwnd, cx, cy) =
            resolve_child_and_client_coords(root, explicit_child, sx, sy);
        send_background_activation_hint(root, child_hwnd)?;

        let ch = child_hwnd as win_msg::HWND;
        let client_lp = pack_lparam(cx, cy);
        let screen_lp = pack_lparam(sx, sy);
        let mut dummy = 0usize;

        match action {
            "move" => {
                win_msg::PostMessageW(ch, win_msg::WM_MOUSEMOVE, 0, client_lp);
                phantom_session::update_virtual_cursor(chat_id, root, rel_x, rel_y);
            }
            "click" => {
                let _ = win_msg::SendMessageTimeoutW(
                    ch,
                    win_msg::WM_SETCURSOR,
                    child_hwnd,
                    (1 | ((win_msg::WM_MOUSEMOVE as isize) << 16)) as isize,
                    win_msg::SMTO_ABORTIFHUNG,
                    35,
                    &mut dummy,
                );
                win_msg::PostMessageW(ch, win_msg::WM_MOUSEMOVE, 0, client_lp);
                win_msg::PostMessageW(ch, win_msg::WM_LBUTTONDOWN, win_msg::MK_LBUTTON, client_lp);
                std::thread::sleep(std::time::Duration::from_millis(18));
                win_msg::PostMessageW(ch, win_msg::WM_LBUTTONUP, 0, client_lp);
                phantom_session::update_virtual_cursor(chat_id, root, rel_x, rel_y);
            }
            "double_click" => {
                win_msg::PostMessageW(ch, win_msg::WM_MOUSEMOVE, 0, client_lp);
                win_msg::PostMessageW(ch, win_msg::WM_LBUTTONDOWN, win_msg::MK_LBUTTON, client_lp);
                win_msg::PostMessageW(ch, win_msg::WM_LBUTTONUP, 0, client_lp);
                std::thread::sleep(std::time::Duration::from_millis(24));
                win_msg::PostMessageW(ch, win_msg::WM_LBUTTONDBLCLK, win_msg::MK_LBUTTON, client_lp);
                std::thread::sleep(std::time::Duration::from_millis(16));
                win_msg::PostMessageW(ch, win_msg::WM_LBUTTONUP, 0, client_lp);
                phantom_session::update_virtual_cursor(chat_id, root, rel_x, rel_y);
            }
            "right_click" => {
                win_msg::PostMessageW(ch, win_msg::WM_MOUSEMOVE, 0, client_lp);
                win_msg::PostMessageW(ch, win_msg::WM_RBUTTONDOWN, win_msg::MK_RBUTTON, client_lp);
                std::thread::sleep(std::time::Duration::from_millis(18));
                win_msg::PostMessageW(ch, win_msg::WM_RBUTTONUP, 0, client_lp);
                win_msg::PostMessageW(ch, win_msg::WM_CONTEXTMENU, child_hwnd, screen_lp);
                phantom_session::update_virtual_cursor(chat_id, root, rel_x, rel_y);
            }
            "middle_click" => {
                win_msg::PostMessageW(ch, win_msg::WM_MOUSEMOVE, 0, client_lp);
                win_msg::PostMessageW(ch, win_msg::WM_MBUTTONDOWN, win_msg::MK_MBUTTON, client_lp);
                std::thread::sleep(std::time::Duration::from_millis(18));
                win_msg::PostMessageW(ch, win_msg::WM_MBUTTONUP, 0, client_lp);
                phantom_session::update_virtual_cursor(chat_id, root, rel_x, rel_y);
            }
            "scroll" | "hscroll" => {
                let wheel_delta = delta.unwrap_or(-240).clamp(-4800, 4800) as i16 as u16 as usize;
                let wp = wheel_delta << 16;
                let msg = if action == "hscroll" {
                    win_msg::WM_MOUSEHWHEEL
                } else {
                    win_msg::WM_MOUSEWHEEL
                };
                win_msg::PostMessageW(ch, win_msg::WM_MOUSEMOVE, 0, client_lp);
                win_msg::PostMessageW(ch, msg, wp, screen_lp);
                phantom_session::update_virtual_cursor(chat_id, root, rel_x, rel_y);
            }
            "drag" => {
                let (end_rel_x, end_rel_y, ex, ey) = resolve_window_relative_xy(
                    win_info.bounds.x,
                    win_info.bounds.y,
                    win_info.bounds.width,
                    win_info.bounds.height,
                    to_x.unwrap_or(x),
                    to_y.unwrap_or(y),
                    normalized,
                );
                let mut end_pt = win_msg::POINT { x: ex, y: ey };
                win_msg::ScreenToClient(ch, &mut end_pt);
                win_msg::PostMessageW(ch, win_msg::WM_MOUSEMOVE, 0, client_lp);
                win_msg::PostMessageW(ch, win_msg::WM_LBUTTONDOWN, win_msg::MK_LBUTTON, client_lp);
                let steps = 8;
                for i in 1..=steps {
                    let ix = cx + ((end_pt.x - cx) * i) / steps;
                    let iy = cy + ((end_pt.y - cy) * i) / steps;
                    win_msg::PostMessageW(
                        ch,
                        win_msg::WM_MOUSEMOVE,
                        win_msg::MK_LBUTTON,
                        pack_lparam(ix, iy),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                win_msg::PostMessageW(
                    ch,
                    win_msg::WM_LBUTTONUP,
                    0,
                    pack_lparam(end_pt.x, end_pt.y),
                );
                phantom_session::update_virtual_cursor(chat_id, root, end_rel_x, end_rel_y);
            }
            other => return Err(format!("Unsupported mouse action '{}'.", other)),
        }

        Ok(json!({
            "action": action,
            "mode": "ghost_message",
            "hwnd": root,
            "targetChildHwnd": child_hwnd,
            "windowTitle": win_info.title,
            "x": x,
            "y": y,
            "clientX": cx,
            "clientY": cy,
            "offscreen": win_info.is_offscreen,
        }))
    }
    #[cfg(not(windows))]
    {
        let _ = (chat_id, root_hwnd, explicit_child, action, x, y, to_x, to_y, delta, normalized);
        Err("Ghost mouse input is only supported on Windows.".to_string())
    }
}

#[cfg(windows)]
unsafe fn post_vk_tap(hwnd: win_msg::HWND, vk: u16) {
    let scan = win_msg::MapVirtualKeyW(vk as u32, 0) & 0xFF;
    let ext = if matches!(
        vk,
        0x21 | 0x22 | 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x28 | 0x2D | 0x2E
    ) {
        1isize << 24
    } else {
        0
    };
    let down_lp = 1isize | ((scan as isize) << 16) | ext;
    let up_lp = 1isize | ((scan as isize) << 16) | ext | (3isize << 30);
    win_msg::PostMessageW(hwnd, win_msg::WM_KEYDOWN, vk as usize, down_lp);
    if vk == 0x0D {
        win_msg::PostMessageW(hwnd, win_msg::WM_CHAR, 0x0D, down_lp);
    } else if vk == 0x09 {
        win_msg::PostMessageW(hwnd, win_msg::WM_CHAR, 0x09, down_lp);
    }
    win_msg::PostMessageW(hwnd, win_msg::WM_KEYUP, vk as usize, up_lp);
}

fn handle_type_on_worker(
    chat_id: Option<&str>,
    root_hwnd: usize,
    explicit_child: Option<usize>,
    text: &str,
    clear_first: bool,
    submit_enter: bool,
) -> Result<Value, String> {
    #[cfg(windows)]
    unsafe {
        if desktop_uia::is_panic_hotkey_active() {
            return Err("Computer use aborted by panic hotkey (Ctrl+Alt+Shift+Esc).".to_string());
        }
        let root = desktop_uia::resolve_root_hwnd(root_hwnd);
        let win_info = desktop_uia::query_window_info(root)
            .ok_or_else(|| "Target window is no longer valid.".to_string())?;

        let target_state = phantom_session::get_chat_target(chat_id);
        let vx = target_state
            .virtual_x
            .unwrap_or(win_info.bounds.width / 2);
        let vy = target_state
            .virtual_y
            .unwrap_or(win_info.bounds.height / 2);
        let (child_hwnd, _, _) = resolve_child_and_client_coords(
            root,
            explicit_child,
            win_info.bounds.x + vx,
            win_info.bounds.y + vy,
        );
        send_background_activation_hint(root, child_hwnd)?;

        let ch = child_hwnd as win_msg::HWND;
        if clear_first {
            let mut dummy = 0usize;
            let _ = win_msg::SendMessageTimeoutW(
                ch,
                win_msg::EM_SETSEL,
                0,
                -1,
                win_msg::SMTO_ABORTIFHUNG,
                40,
                &mut dummy,
            );
            let _ = win_msg::SendMessageTimeoutW(
                ch,
                win_msg::WM_CLEAR,
                0,
                0,
                win_msg::SMTO_ABORTIFHUNG,
                40,
                &mut dummy,
            );
            let _ = handle_hotkey_on_worker(chat_id, root, "ctrl+a");
            let _ = handle_hotkey_on_worker(chat_id, root, "backspace");
            std::thread::sleep(std::time::Duration::from_millis(15));
        }

        let mut dummy = 0usize;
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut idx = 0usize;
        while idx < units.len() {
            let u = units[idx];
            if u == 0x000D {
                if idx + 1 < units.len() && units[idx + 1] == 0x000A {
                    idx += 1;
                }
                post_vk_tap(ch, 0x0D);
            } else if u == 0x000A {
                post_vk_tap(ch, 0x0D);
            } else if u == 0x0009 {
                post_vk_tap(ch, 0x09);
            } else {
                let res = win_msg::SendMessageTimeoutW(
                    ch,
                    win_msg::WM_CHAR,
                    u as usize,
                    1,
                    win_msg::SMTO_ABORTIFHUNG,
                    35,
                    &mut dummy,
                );
                if res == 0 {
                    win_msg::PostMessageW(ch, win_msg::WM_CHAR, u as usize, 1);
                }
            }
            idx += 1;
        }

        if submit_enter {
            std::thread::sleep(std::time::Duration::from_millis(12));
            post_vk_tap(ch, 0x0D);
        }

        Ok(json!({
            "mode": "ghost_wm_char",
            "hwnd": root,
            "targetChildHwnd": child_hwnd,
            "typedChars": text.chars().count(),
            "submittedEnter": submit_enter,
            "offscreen": win_info.is_offscreen,
        }))
    }
    #[cfg(not(windows))]
    {
        let _ = (chat_id, root_hwnd, explicit_child, text, clear_first, submit_enter);
        Err("Ghost text input is only supported on Windows.".to_string())
    }
}

#[cfg(windows)]
fn parse_key_token(token: &str) -> Option<u16> {
    match token.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "lctrl" | "rctrl" => Some(0x11),
        "alt" | "menu" | "lalt" | "ralt" => Some(0x12),
        "shift" | "lshift" | "rshift" => Some(0x10),
        "win" | "meta" | "super" | "lwin" => Some(0x5B),
        "enter" | "return" => Some(0x0D),
        "esc" | "escape" => Some(0x1B),
        "tab" => Some(0x09),
        "space" | "spacebar" => Some(0x20),
        "backspace" | "bksp" => Some(0x08),
        "delete" | "del" => Some(0x2E),
        "insert" | "ins" => Some(0x2D),
        "up" | "arrowup" => Some(0x26),
        "down" | "arrowdown" => Some(0x28),
        "left" | "arrowleft" => Some(0x25),
        "right" | "arrowright" => Some(0x27),
        "home" => Some(0x24),
        "end" => Some(0x23),
        "pageup" | "pgup" => Some(0x21),
        "pagedown" | "pgdn" => Some(0x22),
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
        "plus" | "=" => Some(0xBB),
        "minus" | "-" => Some(0xBD),
        "," | "comma" => Some(0xBC),
        "." | "period" => Some(0xBE),
        "/" | "slash" => Some(0xBF),
        ";" | "semicolon" => Some(0xBA),
        "'" | "quote" => Some(0xDE),
        "[" | "lbracket" => Some(0xDB),
        "]" | "rbracket" => Some(0xDD),
        "\\" | "backslash" => Some(0xDC),
        "`" | "grave" | "tilde" => Some(0xC0),
        s if s.len() == 1 => {
            let b = s.as_bytes()[0].to_ascii_uppercase();
            if b.is_ascii_alphanumeric() {
                Some(b as u16)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn handle_hotkey_on_worker(
    chat_id: Option<&str>,
    root_hwnd: usize,
    combo: &str,
) -> Result<Value, String> {
    #[cfg(windows)]
    unsafe {
        if desktop_uia::is_panic_hotkey_active() {
            return Err("Computer use aborted by panic hotkey (Ctrl+Alt+Shift+Esc).".to_string());
        }
        let root = desktop_uia::resolve_root_hwnd(root_hwnd);
        let win_info = desktop_uia::query_window_info(root)
            .ok_or_else(|| "Target window is no longer valid.".to_string())?;

        let mut has_ctrl = false;
        let mut has_alt = false;
        let mut has_shift = false;
        let mut main_vks: Vec<u16> = Vec::new();

        for part in combo.split('+') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            let vk = parse_key_token(trimmed)
                .ok_or_else(|| format!("Unknown key '{}' in hotkey '{}'.", trimmed, combo))?;
            match vk {
                0x11 => has_ctrl = true,
                0x12 => has_alt = true,
                0x10 => has_shift = true,
                other => main_vks.push(other),
            }
        }

        if main_vks.is_empty() && !has_ctrl && !has_alt && !has_shift {
            return Err("Empty hotkey combination.".to_string());
        }

        let target_state = phantom_session::get_chat_target(chat_id);
        let vx = target_state
            .virtual_x
            .unwrap_or(win_info.bounds.width / 2);
        let vy = target_state
            .virtual_y
            .unwrap_or(win_info.bounds.height / 2);
        let (child_hwnd, _, _) = resolve_child_and_client_coords(
            root,
            None,
            win_info.bounds.x + vx,
            win_info.bounds.y + vy,
        );
        let (target_tid, is_background) = send_background_activation_hint(root, child_hwnd)?;

        let ch = child_hwnd as win_msg::HWND;
        let worker_tid = win_msg::GetCurrentThreadId();
        let has_mods = has_ctrl || has_alt || has_shift;

        let mut attached = false;
        let mut saved_kb = [0u8; 256];
        if is_background && has_mods && target_tid != 0 && target_tid != worker_tid {
            if win_msg::AttachThreadInput(worker_tid, target_tid, 1) != 0 {
                attached = true;
                win_msg::GetKeyboardState(saved_kb.as_mut_ptr());
                let mut mod_kb = saved_kb;
                if has_ctrl {
                    mod_kb[0x11] = 0x80;
                    mod_kb[0xA2] = 0x80;
                }
                if has_shift {
                    mod_kb[0x10] = 0x80;
                    mod_kb[0xA0] = 0x80;
                }
                if has_alt {
                    mod_kb[0x12] = 0x80;
                    mod_kb[0xA4] = 0x80;
                }
                win_msg::SetKeyboardState(mod_kb.as_ptr());
            }
        }

        let alt_flag = if has_alt { 1isize << 29 } else { 0 };
        let down_msg = if has_alt && !has_ctrl {
            win_msg::WM_SYSKEYDOWN
        } else {
            win_msg::WM_KEYDOWN
        };
        let up_msg = if has_alt && !has_ctrl {
            win_msg::WM_SYSKEYUP
        } else {
            win_msg::WM_KEYUP
        };

        if has_ctrl {
            let sc = (win_msg::MapVirtualKeyW(0x11, 0) & 0xFF) as isize;
            win_msg::PostMessageW(ch, win_msg::WM_KEYDOWN, 0x11, 1 | (sc << 16));
        }
        if has_shift {
            let sc = (win_msg::MapVirtualKeyW(0x10, 0) & 0xFF) as isize;
            win_msg::PostMessageW(ch, win_msg::WM_KEYDOWN, 0x10, 1 | (sc << 16));
        }
        if has_alt {
            let sc = (win_msg::MapVirtualKeyW(0x12, 0) & 0xFF) as isize;
            win_msg::PostMessageW(ch, win_msg::WM_SYSKEYDOWN, 0x12, 1 | (sc << 16) | alt_flag);
        }

        for &vk in &main_vks {
            let sc = (win_msg::MapVirtualKeyW(vk as u32, 0) & 0xFF) as isize;
            let ext = if matches!(
                vk,
                0x21 | 0x22 | 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x28 | 0x2D | 0x2E
            ) {
                1isize << 24
            } else {
                0
            };
            let down_lp = 1isize | (sc << 16) | ext | alt_flag;
            win_msg::PostMessageW(ch, down_msg, vk as usize, down_lp);
            if has_alt && !has_ctrl && (0x30..=0x5A).contains(&vk) {
                let ch_lower = (vk as u8 as char).to_ascii_lowercase() as usize;
                win_msg::PostMessageW(ch, win_msg::WM_SYSCHAR, ch_lower, down_lp);
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(20));

        for &vk in main_vks.iter().rev() {
            let sc = (win_msg::MapVirtualKeyW(vk as u32, 0) & 0xFF) as isize;
            let ext = if matches!(
                vk,
                0x21 | 0x22 | 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x28 | 0x2D | 0x2E
            ) {
                1isize << 24
            } else {
                0
            };
            let up_lp = 1isize | (sc << 16) | ext | alt_flag | (3isize << 30);
            win_msg::PostMessageW(ch, up_msg, vk as usize, up_lp);
        }

        if has_alt {
            let sc = (win_msg::MapVirtualKeyW(0x12, 0) & 0xFF) as isize;
            win_msg::PostMessageW(ch, win_msg::WM_SYSKEYUP, 0x12, 1 | (sc << 16) | (3isize << 30));
        }
        if has_shift {
            let sc = (win_msg::MapVirtualKeyW(0x10, 0) & 0xFF) as isize;
            win_msg::PostMessageW(ch, win_msg::WM_KEYUP, 0x10, 1 | (sc << 16) | (3isize << 30));
        }
        if has_ctrl {
            let sc = (win_msg::MapVirtualKeyW(0x11, 0) & 0xFF) as isize;
            win_msg::PostMessageW(ch, win_msg::WM_KEYUP, 0x11, 1 | (sc << 16) | (3isize << 30));
        }

        if attached {
            std::thread::sleep(std::time::Duration::from_millis(8));
            win_msg::SetKeyboardState(saved_kb.as_ptr());
            win_msg::AttachThreadInput(worker_tid, target_tid, 0);
        }

        Ok(json!({
            "hotkey": combo,
            "mode": "ghost_key_message",
            "hwnd": root,
            "targetChildHwnd": child_hwnd,
            "offscreen": win_info.is_offscreen,
        }))
    }
    #[cfg(not(windows))]
    {
        let _ = (chat_id, root_hwnd, combo);
        Err("Ghost hotkey input is only supported on Windows.".to_string())
    }
}
