use crate::tools::desktop_uia::{self, RectBounds, WindowInfo};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUsePolicy {
    pub stealth_mode: bool,
    pub auto_stage_offscreen: bool,
    pub draw_virtual_cursor: bool,
    pub default_width: i32,
    pub default_height: i32,
}

impl Default for ComputerUsePolicy {
    fn default() -> Self {
        Self {
            stealth_mode: true,
            auto_stage_offscreen: true,
            draw_virtual_cursor: true,
            default_width: 1280,
            default_height: 820,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedWindowRecord {
    pub hwnd: usize,
    pub pid: u32,
    pub process_name: String,
    pub title: String,
    pub class_name: String,
    pub original_rect: RectBounds,
    pub original_ex_style: isize,
    pub was_minimized: bool,
    pub spawned_by_omni: bool,
    pub isolated_desktop: bool,
    pub staged_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedUiaElement {
    pub id: String,
    pub control_type: String,
    pub name: String,
    pub automation_id: String,
    pub class_name: String,
    pub native_hwnd: usize,
    pub bounds_screen: RectBounds,
    pub cx_norm: i32,
    pub cy_norm: i32,
}

#[derive(Debug, Clone, Default)]
pub struct ChatPhantomTarget {
    pub bound_hwnd: Option<usize>,
    pub virtual_x: Option<i32>,
    pub virtual_y: Option<i32>,
    pub cdp_port: Option<u16>,
    pub isolated_desktop: bool,
    pub last_uia_elements: Vec<CachedUiaElement>,
}

struct PhantomState {
    policy: ComputerUsePolicy,
    chats: HashMap<String, ChatPhantomTarget>,
    staged: HashMap<usize, StagedWindowRecord>,
    isolated_desktop_ready: bool,
}

static PHANTOM_STATE: OnceLock<Mutex<PhantomState>> = OnceLock::new();

fn policy_file_path() -> PathBuf {
    crate::state::AppState::resolve_data_dir().join("computer_use_policy.json")
}

fn staged_file_path() -> PathBuf {
    crate::state::AppState::resolve_data_dir().join("staged_windows.json")
}

fn load_saved_policy() -> ComputerUsePolicy {
    let p = policy_file_path();
    if let Ok(raw) = std::fs::read_to_string(p) {
        if let Ok(parsed) = serde_json::from_str::<ComputerUsePolicy>(&raw) {
            return parsed;
        }
    }
    ComputerUsePolicy::default()
}

fn persist_policy(policy: &ComputerUsePolicy) {
    if let Ok(raw) = serde_json::to_string_pretty(policy) {
        let _ = std::fs::write(policy_file_path(), raw);
    }
}

fn persist_staged(staged: &HashMap<usize, StagedWindowRecord>) {
    let user_records: Vec<&StagedWindowRecord> =
        staged.values().filter(|r| !r.spawned_by_omni).collect();
    if let Ok(raw) = serde_json::to_string_pretty(&user_records) {
        let _ = std::fs::write(staged_file_path(), raw);
    }
}

fn state() -> &'static Mutex<PhantomState> {
    PHANTOM_STATE.get_or_init(|| {
        let initial = PhantomState {
            policy: load_saved_policy(),
            chats: HashMap::new(),
            staged: HashMap::new(),
            isolated_desktop_ready: false,
        };
        Mutex::new(initial)
    })
}

pub fn recover_orphaned_staged_windows() {
    #[cfg(windows)]
    {
        let path = staged_file_path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(records) = serde_json::from_str::<Vec<StagedWindowRecord>>(&raw) else {
            let _ = std::fs::remove_file(&path);
            return;
        };
        for rec in records {
            if desktop_uia::is_window_valid(rec.hwnd) {
                if let Some(info) = desktop_uia::query_window_info(rec.hwnd) {
                    if info.class_name == rec.class_name && info.is_offscreen {
                        let _ = desktop_uia::restore_window_placement(
                            rec.hwnd,
                            &rec.original_rect,
                            rec.original_ex_style,
                        );
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&path);
    }
}

pub fn get_policy() -> ComputerUsePolicy {
    state()
        .lock()
        .map(|g| g.policy.clone())
        .unwrap_or_default()
}

pub fn set_policy(mut next: ComputerUsePolicy) -> ComputerUsePolicy {
    next.default_width = next.default_width.clamp(640, 3840);
    next.default_height = next.default_height.clamp(420, 2160);
    if let Ok(mut g) = state().lock() {
        g.policy = next.clone();
    }
    persist_policy(&next);
    next
}

pub fn is_hwnd_staged(hwnd: usize) -> bool {
    state()
        .lock()
        .map(|g| g.staged.contains_key(&hwnd))
        .unwrap_or(false)
}

pub fn list_bound_hwnds() -> HashSet<usize> {
    let Ok(g) = state().lock() else {
        return HashSet::new();
    };
    g.chats.values().filter_map(|c| c.bound_hwnd).collect()
}

pub fn get_chat_target(chat_id: Option<&str>) -> ChatPhantomTarget {
    let key = chat_id.unwrap_or("_default");
    state()
        .lock()
        .ok()
        .and_then(|g| g.chats.get(key).cloned())
        .unwrap_or_default()
}

pub fn update_virtual_cursor(chat_id: Option<&str>, hwnd: usize, vx: i32, vy: i32) {
    let key = chat_id.unwrap_or("_default").to_string();
    if let Ok(mut g) = state().lock() {
        let entry = g.chats.entry(key).or_default();
        if entry.bound_hwnd.is_none() {
            entry.bound_hwnd = Some(hwnd);
        }
        entry.virtual_x = Some(vx);
        entry.virtual_y = Some(vy);
    }
}

pub fn store_uia_cache(chat_id: Option<&str>, hwnd: usize, elements: Vec<CachedUiaElement>) {
    let key = chat_id.unwrap_or("_default").to_string();
    if let Ok(mut g) = state().lock() {
        let entry = g.chats.entry(key).or_default();
        if entry.bound_hwnd.is_none() {
            entry.bound_hwnd = Some(hwnd);
        }
        entry.last_uia_elements = elements;
    }
}

pub fn lookup_cached_uia_element(chat_id: Option<&str>, element_id: &str) -> Option<CachedUiaElement> {
    let key = chat_id.unwrap_or("_default");
    let clean = element_id.trim().to_ascii_lowercase();
    state().lock().ok().and_then(|g| {
        g.chats.get(key).and_then(|t| {
            t.last_uia_elements
                .iter()
                .find(|e| e.id.to_ascii_lowercase() == clean)
                .cloned()
        })
    })
}

pub fn stage_window_offscreen(
    hwnd: usize,
    spawned_by_omni: bool,
    width_override: Option<i32>,
    height_override: Option<i32>,
) -> Result<WindowInfo, String> {
    let root = desktop_uia::resolve_root_hwnd(hwnd);
    if !desktop_uia::is_window_valid(root) {
        return Err(format!("Window handle {} is no longer valid.", hwnd));
    }
    let info = desktop_uia::query_window_info(root)
        .ok_or_else(|| "Failed to query target window info.".to_string())?;
    if info.pid == std::process::id() {
        return Err("Refusing to stage OmniHarness's own window off-screen.".to_string());
    }

    let policy = get_policy();
    let target_w = width_override.unwrap_or_else(|| {
        if info.bounds.width >= 500 && !info.is_minimized {
            info.bounds.width
        } else {
            policy.default_width
        }
    });
    let target_h = height_override.unwrap_or_else(|| {
        if info.bounds.height >= 360 && !info.is_minimized {
            info.bounds.height
        } else {
            policy.default_height
        }
    });

    let orig_ex = desktop_uia::get_window_ex_style(root);
    {
        let mut g = state_lock()?;
        g.staged.entry(root).or_insert_with(|| StagedWindowRecord {
            hwnd: root,
            pid: info.pid,
            process_name: info.process_name.clone(),
            title: info.title.clone(),
            class_name: info.class_name.clone(),
            original_rect: info.bounds.clone(),
            original_ex_style: orig_ex,
            was_minimized: info.is_minimized,
            spawned_by_omni,
            isolated_desktop: false,
            staged_at_ms: chrono::Utc::now().timestamp_millis(),
        });
        persist_staged(&g.staged);
    }

    desktop_uia::move_window_to_offscreen_stage(root, target_w, target_h, spawned_by_omni)?;
    desktop_uia::query_window_info(root)
        .ok_or_else(|| "Window closed while staging off-screen.".to_string())
}

fn state_lock() -> Result<std::sync::MutexGuard<'static, PhantomState>, String> {
    state().lock().map_err(|_| "Phantom state lock poisoned".to_string())
}

pub fn restore_staged_window(hwnd: usize) -> Result<WindowInfo, String> {
    let root = desktop_uia::resolve_root_hwnd(hwnd);
    if !desktop_uia::is_window_valid(root) {
        if let Ok(mut g) = state_lock() {
            g.staged.remove(&root);
            persist_staged(&g.staged);
        }
        return Err(format!("Window {} is no longer open.", root));
    }

    let rec_opt = {
        let mut g = state_lock()?;
        let r = g.staged.remove(&root);
        persist_staged(&g.staged);
        r
    };

    if let Some(rec) = rec_opt {
        desktop_uia::restore_window_placement(root, &rec.original_rect, rec.original_ex_style)?;
    } else {
        let default_rect = RectBounds {
            x: 120,
            y: 80,
            width: 1180,
            height: 760,
        };
        let ex = desktop_uia::get_window_ex_style(root);
        desktop_uia::restore_window_placement(root, &default_rect, ex)?;
    }

    desktop_uia::query_window_info(root)
        .ok_or_else(|| "Failed to query restored window.".to_string())
}

pub fn restore_all_staged_windows(only_user_windows: bool) -> usize {
    let records: Vec<StagedWindowRecord> = {
        let Ok(mut g) = state_lock() else {
            return 0;
        };
        let keys: Vec<usize> = g
            .staged
            .iter()
            .filter(|(_, r)| !only_user_windows || !r.spawned_by_omni)
            .map(|(k, _)| *k)
            .collect();
        let mut out = Vec::new();
        for k in keys {
            if let Some(r) = g.staged.remove(&k) {
                out.push(r);
            }
        }
        persist_staged(&g.staged);
        out
    };

    let mut count = 0usize;
    for rec in records {
        if desktop_uia::is_window_valid(rec.hwnd) {
            if desktop_uia::restore_window_placement(
                rec.hwnd,
                &rec.original_rect,
                rec.original_ex_style,
            )
            .is_ok()
            {
                count += 1;
            }
        }
    }
    count
}

pub fn bind_window_for_chat(
    chat_id: Option<&str>,
    hwnd: usize,
    stage_offscreen: bool,
    cdp_port: Option<u16>,
) -> Result<WindowInfo, String> {
    let root = desktop_uia::resolve_root_hwnd(hwnd);
    if !desktop_uia::is_window_valid(root) {
        return Err(format!("Window {} does not exist.", hwnd));
    }
    let mut info = desktop_uia::query_window_info(root)
        .ok_or_else(|| "Unable to inspect target window.".to_string())?;
    if info.pid == std::process::id() {
        return Err("Refusing to bind OmniHarness's own window.".to_string());
    }

    if stage_offscreen {
        info = stage_window_offscreen(root, false, None, None)?;
    } else if info.is_minimized {
        desktop_uia::unminimize_no_activate(root);
        if let Some(updated) = desktop_uia::query_window_info(root) {
            info = updated;
        }
    }

    let key = chat_id.unwrap_or("_default").to_string();
    {
        let mut g = state_lock()?;
        let entry = g.chats.entry(key).or_default();
        entry.bound_hwnd = Some(root);
        if let Some(port) = cdp_port {
            entry.cdp_port = Some(port);
        }
    }
    Ok(info)
}

pub fn resolve_target_hwnd(
    chat_id: Option<&str>,
    explicit_hwnd: Option<usize>,
    title_query: Option<&str>,
    process_query: Option<&str>,
) -> Result<usize, String> {
    if let Some(h) = explicit_hwnd.filter(|&h| h != 0) {
        let root = desktop_uia::resolve_root_hwnd(h);
        if !desktop_uia::is_window_valid(root) {
            return Err(format!("Window handle {} is no longer open. Call list_windows or launch_app.", h));
        }
        let info = desktop_uia::query_window_info(root)
            .ok_or_else(|| "Failed to inspect target window.".to_string())?;
        if info.pid == std::process::id() {
            return Err("Refusing to target OmniHarness's own window.".to_string());
        }
        let key = chat_id.unwrap_or("_default").to_string();
        if let Ok(mut g) = state_lock() {
            g.chats.entry(key).or_default().bound_hwnd = Some(root);
        }
        return Ok(root);
    }

    let t_q = title_query.map(|s| s.trim()).filter(|s| !s.is_empty());
    let p_q = process_query.map(|s| s.trim()).filter(|s| !s.is_empty());
    if t_q.is_some() || p_q.is_some() {
        let candidates = desktop_uia::list_controllable_windows(t_q, p_q);
        let picked = candidates
            .into_iter()
            .find(|w| w.pid != std::process::id())
            .ok_or_else(|| {
                format!(
                    "No open window matched title={:?} process={:?}. Use list_windows or launch_app.",
                    t_q, p_q
                )
            })?;
        let key = chat_id.unwrap_or("_default").to_string();
        if let Ok(mut g) = state_lock() {
            g.chats.entry(key).or_default().bound_hwnd = Some(picked.hwnd);
        }
        return Ok(picked.hwnd);
    }

    let key = chat_id.unwrap_or("_default");
    let bound_opt = state_lock()
        .ok()
        .and_then(|g| g.chats.get(key).and_then(|c| c.bound_hwnd));
    if let Some(bound) = bound_opt {
        if desktop_uia::is_window_valid(bound) {
            return Ok(bound);
        }
        if let Ok(mut g) = state_lock() {
            if let Some(entry) = g.chats.get_mut(key) {
                entry.bound_hwnd = None;
            }
        }
    }

    Err("No target window is bound to this session. Call computer_use with action='list_windows', 'bind_window', or 'launch_app' first so background input targets a specific app without touching the user's desktop.".to_string())
}

pub fn get_status_snapshot() -> serde_json::Value {
    let policy = get_policy();
    let bound_set = list_bound_hwnds();
    let mut staged_list = Vec::new();
    if let Ok(mut g) = state_lock() {
        g.staged.retain(|&h, _| desktop_uia::is_window_valid(h));
        for rec in g.staged.values() {
            let live_title = desktop_uia::query_window_info(rec.hwnd)
                .map(|w| w.title)
                .unwrap_or_else(|| rec.title.clone());
            staged_list.push(json!({
                "hwnd": rec.hwnd,
                "pid": rec.pid,
                "processName": rec.process_name,
                "title": live_title,
                "className": rec.class_name,
                "spawnedByOmni": rec.spawned_by_omni,
                "isolatedDesktop": rec.isolated_desktop,
                "isBound": bound_set.contains(&rec.hwnd),
                "stagedAtMs": rec.staged_at_ms,
            }));
        }
    }

    let open_windows: Vec<serde_json::Value> = desktop_uia::list_controllable_windows(None, None)
        .into_iter()
        .take(40)
        .map(|w| {
            json!({
                "hwnd": w.hwnd,
                "pid": w.pid,
                "processName": w.process_name,
                "title": w.title,
                "className": w.class_name,
                "bounds": w.bounds,
                "isMinimized": w.is_minimized,
                "isOffscreen": w.is_offscreen,
                "isBound": bound_set.contains(&w.hwnd),
                "isStaged": is_hwnd_staged(w.hwnd),
            })
        })
        .collect();

    json!({
        "policy": policy,
        "stagedWindows": staged_list,
        "windows": open_windows,
    })
}

fn resolve_executable_path(app: &str) -> String {
    let trimmed = app.trim().trim_matches('"');
    if trimmed.is_empty() {
        return String::new();
    }
    let direct = Path::new(trimmed);
    if direct.is_file() {
        return direct.to_string_lossy().to_string();
    }
    if direct.extension().is_none() {
        let with_exe = PathBuf::from(format!("{}.exe", trimmed));
        if with_exe.is_file() {
            return with_exe.to_string_lossy().to_string();
        }
    }

    let exe_name = if trimmed.to_ascii_lowercase().ends_with(".exe")
        || trimmed.to_ascii_lowercase().ends_with(".cmd")
        || trimmed.to_ascii_lowercase().ends_with(".bat")
    {
        trimmed.to_string()
    } else {
        format!("{}.exe", trimmed)
    };

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(&exe_name);
            if candidate.is_file() {
                return candidate.to_string_lossy().to_string();
            }
            let raw_candidate = dir.join(trimmed);
            if raw_candidate.is_file() {
                return raw_candidate.to_string_lossy().to_string();
            }
        }
    }

    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let common = [
            Path::new(&local).join("Programs").join(trimmed).join(&exe_name),
            Path::new(&local)
                .join("Programs")
                .join("Microsoft VS Code")
                .join(&exe_name),
            Path::new(&local).join("Telegram Desktop").join(&exe_name),
            Path::new(&local).join("Discord").join("Update.exe"),
        ];
        for c in common {
            if c.is_file() && c.file_name().and_then(|n| n.to_str()).map(|n| n.eq_ignore_ascii_case(&exe_name)).unwrap_or(false) {
                return c.to_string_lossy().to_string();
            }
        }
    }

    trimmed.to_string()
}

#[cfg(windows)]
fn ensure_isolated_desktop() -> Result<String, String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, CreateDesktopW, DESKTOP_CONTROL_FLAGS,
    };

    let desk_name = "OmniPhantomDesktop";
    let wide: Vec<u16> = desk_name.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let hdesk = CreateDesktopW(
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            None,
            DESKTOP_CONTROL_FLAGS(0),
            0x01FF,
            None,
        )
        .map_err(|e| format!("CreateDesktopW failed: {}", e))?;
        let _ = CloseDesktop(hdesk);
    }
    if let Ok(mut g) = state_lock() {
        g.isolated_desktop_ready = true;
    }
    Ok(format!("winsta0\\{}", desk_name))
}

pub fn launch_background_app(
    chat_id: Option<&str>,
    app: &str,
    args: &[String],
    cwd: Option<&str>,
    offscreen_opt: Option<bool>,
    isolated_desktop: bool,
    width_opt: Option<i32>,
    height_opt: Option<i32>,
    cdp_port: Option<u16>,
) -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    {
        use std::ptr::null_mut;
        use windows::core::{PCWSTR, PWSTR};
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            CreateProcessW, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTF_USEPOSITION,
            STARTF_USESIZE, STARTF_USESHOWWINDOW, STARTUPINFOW,
        };

        let policy = get_policy();
        let stage_offscreen = offscreen_opt.unwrap_or(policy.auto_stage_offscreen);
        let target_w = width_opt.unwrap_or(policy.default_width).clamp(640, 3840);
        let target_h = height_opt.unwrap_or(policy.default_height).clamp(420, 2160);
        let (off_x, off_y) = desktop_uia::compute_offscreen_origin();

        let resolved_exe = resolve_executable_path(app);
        if resolved_exe.is_empty() {
            return Err("Missing 'app' executable name or path.".to_string());
        }

        let pre_hwnds: HashSet<usize> = desktop_uia::list_controllable_windows(None, None)
            .into_iter()
            .map(|w| w.hwnd)
            .collect();

        let mut cmd_parts = vec![format!("\"{}\"", resolved_exe)];
        for a in args {
            if a.contains(' ') && !a.starts_with('"') {
                cmd_parts.push(format!("\"{}\"", a));
            } else {
                cmd_parts.push(a.clone());
            }
        }
        if let Some(port) = cdp_port {
            let flag = format!("--remote-debugging-port={}", port);
            if !cmd_parts.iter().any(|p| p.contains("--remote-debugging-port")) {
                cmd_parts.push(flag);
            }
        }
        if isolated_desktop && !cmd_parts.iter().any(|p| p.contains("--disable-gpu")) {
            let lower_exe = resolved_exe.to_ascii_lowercase();
            if lower_exe.contains("chrome")
                || lower_exe.contains("msedge")
                || lower_exe.contains("brave")
                || lower_exe.contains("code")
            {
                cmd_parts.push("--disable-gpu".to_string());
            }
        }

        let cmd_line = cmd_parts.join(" ");
        let mut cmd_wide: Vec<u16> = cmd_line.encode_utf16().chain(std::iter::once(0)).collect();

        let cwd_wide: Option<Vec<u16>> = cwd
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && Path::new(s).is_dir())
            .map(|s| s.encode_utf16().chain(std::iter::once(0)).collect());

        let desktop_wide: Option<Vec<u16>> = if isolated_desktop {
            let d = ensure_isolated_desktop()?;
            Some(d.encode_utf16().chain(std::iter::once(0)).collect())
        } else {
            None
        };

        let mut si = STARTUPINFOW::default();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = 4;
        if stage_offscreen && !isolated_desktop {
            si.dwFlags |= STARTF_USEPOSITION | STARTF_USESIZE;
            si.dwX = off_x as u32;
            si.dwY = off_y as u32;
            si.dwXSize = target_w as u32;
            si.dwYSize = target_h as u32;
        }
        if let Some(ref dw) = desktop_wide {
            si.lpDesktop = PWSTR(dw.as_ptr() as *mut u16);
        }

        let mut pi = PROCESS_INFORMATION::default();
        let spawned_ok = unsafe {
            CreateProcessW(
                PCWSTR::null(),
                PWSTR(cmd_wide.as_mut_ptr()),
                None,
                None,
                false,
                PROCESS_CREATION_FLAGS(0),
                None,
                cwd_wide
                    .as_ref()
                    .map(|v| PCWSTR(v.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                &si,
                &mut pi,
            )
        };

        if let Err(err) = spawned_ok {
            return Err(format!("Failed to launch '{}': {}", resolved_exe, err));
        }

        let spawned_pid = pi.dwProcessId;
        unsafe {
            let _ = CloseHandle(pi.hThread);
            let _ = CloseHandle(pi.hProcess);
        }

        let stem = Path::new(&resolved_exe)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(app)
            .to_ascii_lowercase();

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(5500);
        let mut found_win: Option<WindowInfo> = None;

        while std::time::Instant::now() < deadline {
            let current = if isolated_desktop {
                desktop_uia::list_isolated_desktop_windows("OmniPhantomDesktop")
            } else {
                desktop_uia::list_controllable_windows(None, None)
            };

            let matched = desktop_uia::find_visible_windows_for_pid(spawned_pid)
                .into_iter()
                .next()
                .or_else(|| {
                    current
                        .iter()
                        .find(|w| w.pid == spawned_pid && w.bounds.width >= 80 && w.bounds.height >= 50)
                        .cloned()
                })
                .or_else(|| {
                    current
                        .into_iter()
                        .find(|w| {
                            !pre_hwnds.contains(&w.hwnd)
                                && w.pid != std::process::id()
                                && w.bounds.width >= 100
                                && w.bounds.height >= 60
                                && (w.process_name.to_ascii_lowercase().contains(&stem)
                                    || w.title.to_ascii_lowercase().contains(&stem)
                                    || stem == "calc" && w.process_name.to_ascii_lowercase().contains("calc")
                                    || stem == "notepad" && w.process_name.to_ascii_lowercase().contains("notepad"))
                        })
                });

            if let Some(win) = matched {
                if stage_offscreen && !isolated_desktop {
                    if let Ok(staged) = stage_window_offscreen(win.hwnd, true, Some(target_w), Some(target_h)) {
                        found_win = Some(staged);
                    } else {
                        found_win = Some(win);
                    }
                } else {
                    found_win = Some(win);
                }
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(40));
        }

        if let Some(win) = found_win {
            let key = chat_id.unwrap_or("_default").to_string();
            if let Ok(mut g) = state_lock() {
                let entry = g.chats.entry(key).or_default();
                entry.bound_hwnd = Some(win.hwnd);
                entry.cdp_port = cdp_port;
                entry.isolated_desktop = isolated_desktop;
            }
            std::thread::sleep(std::time::Duration::from_millis(160));
            Ok(json!({
                "status": "launched",
                "hwnd": win.hwnd,
                "pid": win.pid,
                "processName": win.process_name,
                "title": win.title,
                "className": win.class_name,
                "bounds": win.bounds,
                "offscreen": stage_offscreen,
                "isolatedDesktop": isolated_desktop,
                "bound": true,
            }))
        } else {
            let _ = null_mut::<u8>();
            Ok(json!({
                "status": "spawned_pending_window",
                "pid": spawned_pid,
                "executable": resolved_exe,
                "offscreen": stage_offscreen,
                "isolatedDesktop": isolated_desktop,
                "note": "Process started; call list_windows or bind_window if its window initializes asynchronously."
            }))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (chat_id, app, args, cwd, offscreen_opt, isolated_desktop, width_opt, height_opt, cdp_port);
        Err("launch_app is only supported on Windows.".to_string())
    }
}
