use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;
use wait_timeout::ChildExt;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessItem {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub cmd: Vec<String>,
    pub exe_path: Option<String>,
    pub parent_pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

pub struct PtySessionInner {
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    pub exit_code: Option<i32>,
}

pub struct PtySession {
    pub session_id: String,
    pub chat_id: Option<String>,
    pub inner: Arc<Mutex<PtySessionInner>>,
    pub buffer: Arc<Mutex<Vec<u8>>>,
    pub is_alive: Arc<AtomicBool>,
}

#[cfg(windows)]
struct WinJobHandle(Option<isize>);

#[cfg(windows)]
impl WinJobHandle {
    fn attach(raw_handle: Option<std::os::windows::io::RawHandle>) -> Self {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        unsafe {
            let Ok(job) = CreateJobObjectW(None, None) else {
                return Self(None);
            };
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let _ = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if let Some(rh) = raw_handle {
                let _ = AssignProcessToJobObject(job, HANDLE(rh));
            }
            Self(Some(job.0 as isize))
        }
    }

    fn terminate(&mut self) {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::JobObjects::TerminateJobObject;
        if let Some(raw) = self.0.take() {
            unsafe {
                let h = HANDLE(raw as *mut std::ffi::c_void);
                let _ = TerminateJobObject(h, 1);
                let _ = CloseHandle(h);
            }
        }
    }
}

#[cfg(windows)]
impl Drop for WinJobHandle {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        if let Some(raw) = self.0.take() {
            unsafe {
                let h = HANDLE(raw as *mut std::ffi::c_void);
                let _ = CloseHandle(h);
            }
        }
    }
}

struct TaskLogBuffer {
    bytes: Vec<u8>,
    base_offset: usize,
    read_cursor: usize,
}

impl TaskLogBuffer {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            base_offset: 0,
            read_cursor: 0,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > 2_000_000 {
            let drain_n = self.bytes.len() - 1_500_000;
            self.bytes.drain(0..drain_n);
            self.base_offset += drain_n;
            if self.read_cursor < self.base_offset {
                self.read_cursor = self.base_offset;
            }
        }
    }

    fn read_unread(&mut self, max_bytes: usize) -> Vec<u8> {
        let total_end = self.base_offset + self.bytes.len();
        if self.read_cursor >= total_end {
            return Vec::new();
        }
        let start_rel = self.read_cursor.saturating_sub(self.base_offset);
        let avail = self.bytes.len().saturating_sub(start_rel);
        let take_n = avail.min(max_bytes);
        self.read_cursor = self.base_offset + start_rel + take_n;
        self.bytes[start_rel..start_rel + take_n].to_vec()
    }

    fn tail(&self, max_bytes: usize) -> Vec<u8> {
        if self.bytes.len() <= max_bytes {
            self.bytes.clone()
        } else {
            self.bytes[self.bytes.len() - max_bytes..].to_vec()
        }
    }
}

pub struct ManagedTask {
    pub task_id: String,
    pub chat_id: Option<String>,
    pub command: String,
    pub cwd: String,
    pub started_at: Instant,
    pub pid: Option<u32>,
    log_buf: Arc<Mutex<TaskLogBuffer>>,
    stdout_buf: Arc<Mutex<Vec<u8>>>,
    stderr_buf: Arc<Mutex<Vec<u8>>>,
    stdin_tx: Arc<tokio::sync::Mutex<Option<tokio::process::ChildStdin>>>,
    live_delta: Arc<Mutex<Option<(tokio::sync::mpsc::Sender<crate::llm::StreamEvent>, String)>>>,
    pub is_alive: Arc<AtomicBool>,
    pub was_killed: Arc<AtomicBool>,
    pub exit_code: Arc<Mutex<Option<i32>>>,
    done_notify: Arc<tokio::sync::Notify>,
    #[cfg(windows)]
    job_handle: Arc<Mutex<WinJobHandle>>,
}

pub enum TaskExecOutcome {
    Completed {
        task_id: String,
        exit_code: i32,
        stdout: String,
        stderr: String,
        output: String,
        duration_ms: u64,
    },
    Backgrounded {
        task_id: String,
        output_so_far: String,
        duration_ms: u64,
    },
    Canceled {
        duration_ms: u64,
    },
}

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, PtySession>>>,
    tasks: Arc<RwLock<HashMap<String, Arc<ManagedTask>>>>,
}

fn create_cmd(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }
    cmd
}

pub fn list_processes(filter: Option<&str>) -> Vec<ProcessItem> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let filter_lower = filter.map(|f| f.to_lowercase());
    let mut results = Vec::new();

    for (pid, proc) in sys.processes() {
        let name = proc.name().to_string_lossy().to_string();
        if let Some(ref f) = filter_lower {
            if !name.to_lowercase().contains(f) {
                continue;
            }
        }

        let cmd: Vec<String> = proc
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        let exe_path = proc.exe().map(|p| p.to_string_lossy().to_string());
        let parent_pid = proc.parent().map(|p| p.as_u32());

        results.push(ProcessItem {
            pid: pid.as_u32(),
            name,
            cpu_usage: proc.cpu_usage(),
            memory_bytes: proc.memory(),
            cmd,
            exe_path,
            parent_pid,
        });
    }

    results.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
    results
}

pub fn inspect_process(target_pid: u32) -> Result<ProcessItem, String> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let pid = Pid::from_u32(target_pid);
    if let Some(proc) = sys.process(pid) {
        let name = proc.name().to_string_lossy().to_string();
        let cmd: Vec<String> = proc
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        let exe_path = proc.exe().map(|p| p.to_string_lossy().to_string());
        let parent_pid = proc.parent().map(|p| p.as_u32());

        Ok(ProcessItem {
            pid: target_pid,
            name,
            cpu_usage: proc.cpu_usage(),
            memory_bytes: proc.memory(),
            cmd,
            exe_path,
            parent_pid,
        })
    } else {
        Err(format!("Process with PID {} not found", target_pid))
    }
}

pub fn kill_process(target_pid: u32, force: bool) -> Result<bool, String> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let pid = Pid::from_u32(target_pid);
    if let Some(proc) = sys.process(pid) {
        if proc.kill() {
            return Ok(true);
        }
    }

    #[cfg(windows)]
    {
        let pid_str = target_pid.to_string();
        let mut cmd = create_cmd("taskkill");
        if force {
            cmd.args(["/F", "/T", "/PID", &pid_str]);
        } else {
            cmd.args(["/PID", &pid_str]);
        }
        let out = cmd.output().map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn strip_ansi(input: &str) -> String {
    let cleaned_clixml;
    let src: &str = if input.contains("#< CLIXML") {
        let mut buf = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(pos) = rest.find("#< CLIXML") {
            buf.push_str(&rest[..pos]);
            let after = &rest[pos..];
            if let Some(end_objs) = after.find("</Objs>") {
                rest = &after[end_objs + 7..];
                if rest.starts_with("\r\n") {
                    rest = &rest[2..];
                } else if rest.starts_with('\n') {
                    rest = &rest[1..];
                }
            } else {
                // Incomplete CLIXML fragment at end of stream; drop the raw XML noise
                rest = "";
                break;
            }
        }
        buf.push_str(rest);
        cleaned_clixml = buf;
        &cleaned_clixml
    } else {
        input
    };
    let chars: Vec<char> = src.chars().collect();
    let len = chars.len();
    let mut lines: Vec<String> = Vec::new();
    let mut cur_line: Vec<char> = Vec::new();
    let mut cursor: usize = 0;
    let mut i = 0usize;

    while i < len {
        let ch = chars[i];
        if ch == '\x1b' {
            i += 1;
            if i >= len {
                break;
            }
            let next = chars[i];
            match next {
                '[' => {
                    i += 1;
                    let param_start = i;
                    while i < len && (chars[i] as u32) >= 0x30 && (chars[i] as u32) <= 0x3F {
                        i += 1;
                    }
                    let param_end = i;
                    while i < len && (chars[i] as u32) >= 0x20 && (chars[i] as u32) <= 0x2F {
                        i += 1;
                    }
                    if i < len && (chars[i] as u32) >= 0x40 && (chars[i] as u32) <= 0x7E {
                        let final_ch = chars[i];
                        let params: String = chars[param_start..param_end].iter().collect();
                        if final_ch == 'K' {
                            match params.as_str() {
                                "2" => {
                                    cur_line.clear();
                                    cursor = 0;
                                }
                                "1" => {
                                    let end = cursor.min(cur_line.len());
                                    for slot in &mut cur_line[..end] {
                                        *slot = ' ';
                                    }
                                }
                                _ => {
                                    if cursor < cur_line.len() {
                                        cur_line.truncate(cursor);
                                    }
                                }
                            }
                        } else if final_ch == 'G' {
                            let col = params.parse::<usize>().unwrap_or(1).saturating_sub(1);
                            cursor = col;
                        }
                        i += 1;
                    }
                }
                ']' | 'P' | '_' | '^' | 'X' => {
                    i += 1;
                    while i < len {
                        if chars[i] == '\x07' {
                            i += 1;
                            break;
                        }
                        if chars[i] == '\x1b' && i + 1 < len && chars[i + 1] == '\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                '(' | ')' | '*' | '+' | '#' => {
                    i += 2;
                }
                _ => {
                    i += 1;
                }
            }
            continue;
        }

        match ch {
            '\r' => {
                if i + 1 < len && chars[i + 1] == '\n' {
                    i += 1;
                    continue;
                }
                cursor = 0;
            }
            '\n' => {
                let s: String = cur_line.iter().collect();
                lines.push(s.trim_end_matches(' ').to_string());
                cur_line.clear();
                cursor = 0;
            }
            '\x08' => {
                cursor = cursor.saturating_sub(1);
            }
            '\t' => {
                if cursor < cur_line.len() {
                    cur_line[cursor] = '\t';
                } else {
                    while cur_line.len() < cursor {
                        cur_line.push(' ');
                    }
                    cur_line.push('\t');
                }
                cursor += 1;
            }
            c if (c as u32) < 0x20 || c == '\x7f' => {}
            c => {
                if cursor < cur_line.len() {
                    cur_line[cursor] = c;
                } else {
                    while cur_line.len() < cursor {
                        cur_line.push(' ');
                    }
                    cur_line.push(c);
                }
                cursor += 1;
            }
        }
        i += 1;
    }

    let ended_with_nl = src.ends_with('\n');
    if !cur_line.is_empty() {
        let s: String = cur_line.iter().collect();
        lines.push(s.trim_end_matches(' ').to_string());
    }
    let mut joined = lines.join("\n");
    if ended_with_nl && (!joined.is_empty() || !lines.is_empty()) {
        joined.push('\n');
    }
    joined
}

pub fn run_command_direct(
    executable: &str,
    args: &[&str],
    cwd: Option<&str>,
) -> Result<BuildOutput, String> {
    let start = Instant::now();
    let mut cmd = create_cmd(executable);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Command execution failed: {}", e))?;

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut r) = stdout_pipe {
            let _ = r.read_to_end(&mut buf);
        }
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut r) = stderr_pipe {
            let _ = r.read_to_end(&mut buf);
        }
        buf
    });

    match child.wait_timeout(Duration::from_secs(60)) {
        Ok(Some(status)) => {
            let out_bytes = out_handle.join().unwrap_or_default();
            let err_bytes = err_handle.join().unwrap_or_default();
            let duration_ms = start.elapsed().as_millis();
            Ok(BuildOutput {
                success: status.success(),
                exit_code: status.code(),
                stdout: strip_ansi(&String::from_utf8_lossy(&out_bytes)),
                stderr: strip_ansi(&String::from_utf8_lossy(&err_bytes)),
                duration_ms,
            })
        }
        Ok(None) => {
            let _ = child.kill();
            let _ = out_handle.join();
            let _ = err_handle.join();
            Err("Command timed out after 60 seconds".to_string())
        }
        Err(e) => {
            let _ = child.kill();
            Err(format!("Process wait error: {}", e))
        }
    }
}

pub fn run_cargo(args: &[&str], cwd: Option<&str>, _timeout_secs: u64) -> Result<BuildOutput, String> {
    run_command_direct("cargo", args, cwd)
}

pub fn run_gradle(args: &[&str], cwd: Option<&str>, _timeout_secs: u64) -> Result<BuildOutput, String> {
    #[cfg(windows)]
    let bin = if cwd.map(|d| PathBuf::from(d).join("gradlew.bat").exists()).unwrap_or(false) {
        "gradlew.bat"
    } else {
        "gradle"
    };
    #[cfg(not(windows))]
    let bin = "./gradlew";

    run_command_direct(bin, args, cwd)
}

pub fn run_maven(args: &[&str], cwd: Option<&str>, _timeout_secs: u64) -> Result<BuildOutput, String> {
    #[cfg(windows)]
    let bin = if cwd.map(|d| PathBuf::from(d).join("mvnw.cmd").exists()).unwrap_or(false) {
        "mvnw.cmd"
    } else {
        "mvn"
    };
    #[cfg(not(windows))]
    let bin = "./mvnw";

    run_command_direct(bin, args, cwd)
}

pub fn run_npm(args: &[&str], cwd: Option<&str>, _timeout_secs: u64) -> Result<BuildOutput, String> {
    #[cfg(windows)]
    let bin = "npm.cmd";
    #[cfg(not(windows))]
    let bin = "npm";

    run_command_direct(bin, args, cwd)
}

pub fn run_pytest(args: &[&str], cwd: Option<&str>, _timeout_secs: u64) -> Result<BuildOutput, String> {
    let mut pytest_args = vec!["-m", "pytest"];
    pytest_args.extend_from_slice(args);
    run_command_direct("python", &pytest_args, cwd)
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn spawn_managed_task(
        &self,
        command: &str,
        cwd: &str,
        chat_id: Option<&str>,
        live_delta: Option<(tokio::sync::mpsc::Sender<crate::llm::StreamEvent>, String)>,
    ) -> Result<String, String> {
        let mut cmd = if cfg!(target_os = "windows") {
            use base64::Engine;
            let full_script = format!(
                "$ProgressPreference = 'SilentlyContinue'; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::InputEncoding = [System.Text.Encoding]::UTF8; $OutputEncoding = [System.Text.Encoding]::UTF8;\n{}",
                command
            );
            let utf16_bytes: Vec<u8> = full_script
                .encode_utf16()
                .flat_map(|u| u.to_le_bytes())
                .collect();
            let encoded = base64::engine::general_purpose::STANDARD.encode(&utf16_bytes);
            let mut c = tokio::process::Command::new("powershell.exe");
            c.args(["-NoProfile", "-OutputFormat", "Text", "-EncodedCommand", &encoded]);
            c
        } else {
            let mut c = tokio::process::Command::new("sh");
            c.args(["-c", command]);
            c
        };

        cmd.current_dir(cwd)
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(false);

        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000);
        }

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn process: {}", e))?;
        let pid = child.id();

        #[cfg(windows)]
        let job_handle = Arc::new(Mutex::new(WinJobHandle::attach(child.raw_handle())));

        let stdin_pipe = child.stdin.take();
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();

        let short_hex = Uuid::new_v4().simple().to_string();
        let task_id = format!("task_{}", &short_hex[..8]);

        let log_buf = Arc::new(Mutex::new(TaskLogBuffer::new()));
        let stdout_buf = Arc::new(Mutex::new(Vec::new()));
        let stderr_buf = Arc::new(Mutex::new(Vec::new()));
        let stdin_tx = Arc::new(tokio::sync::Mutex::new(stdin_pipe));
        let live_delta_arc = Arc::new(Mutex::new(live_delta));
        let is_alive = Arc::new(AtomicBool::new(true));
        let was_killed = Arc::new(AtomicBool::new(false));
        let exit_code = Arc::new(Mutex::new(None));
        let done_notify = Arc::new(tokio::sync::Notify::new());

        let log_out = log_buf.clone();
        let out_acc = stdout_buf.clone();
        let delta_out = live_delta_arc.clone();
        let out_join = tokio::spawn(async move {
            if let Some(ref mut r) = stdout_pipe {
                let mut buf = [0u8; 8192];
                while let Ok(n) = r.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    if let Ok(mut lb) = log_out.lock() {
                        lb.push(&buf[..n]);
                    }
                    if let Ok(mut ob) = out_acc.lock() {
                        ob.extend_from_slice(&buf[..n]);
                        if ob.len() > 2_000_000 {
                            let excess = ob.len() - 1_500_000;
                            ob.drain(0..excess);
                        }
                    }
                    let forward = delta_out.lock().ok().and_then(|g| g.clone());
                    if let Some((tx, cid)) = forward {
                        let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                        if !chunk.is_empty() {
                            let _ = tx
                                .send(crate::llm::StreamEvent::ToolOutputDelta { id: cid, chunk })
                                .await;
                        }
                    }
                }
            }
        });

        let log_err = log_buf.clone();
        let err_acc = stderr_buf.clone();
        let delta_err = live_delta_arc.clone();
        let err_join = tokio::spawn(async move {
            if let Some(ref mut r) = stderr_pipe {
                let mut buf = [0u8; 8192];
                while let Ok(n) = r.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    if let Ok(mut lb) = log_err.lock() {
                        lb.push(&buf[..n]);
                    }
                    if let Ok(mut eb) = err_acc.lock() {
                        eb.extend_from_slice(&buf[..n]);
                        if eb.len() > 2_000_000 {
                            let excess = eb.len() - 1_500_000;
                            eb.drain(0..excess);
                        }
                    }
                    let forward = delta_err.lock().ok().and_then(|g| g.clone());
                    if let Some((tx, cid)) = forward {
                        let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                        if !chunk.is_empty() {
                            let _ = tx
                                .send(crate::llm::StreamEvent::ToolOutputDelta { id: cid, chunk })
                                .await;
                        }
                    }
                }
            }
        });

        let alive_w = is_alive.clone();
        let ec_w = exit_code.clone();
        let stdin_w = stdin_tx.clone();
        let done_w = done_notify.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
            let _ = tokio::time::timeout(Duration::from_millis(250), async {
                let _ = out_join.await;
                let _ = err_join.await;
            })
            .await;
            {
                let mut guard = stdin_w.lock().await;
                *guard = None;
            }
            if let Ok(mut ec) = ec_w.lock() {
                *ec = Some(code);
            }
            alive_w.store(false, Ordering::SeqCst);
            done_w.notify_waiters();
        });

        let task = Arc::new(ManagedTask {
            task_id: task_id.clone(),
            chat_id: chat_id.map(|s| s.to_string()),
            command: command.to_string(),
            cwd: cwd.to_string(),
            started_at: Instant::now(),
            pid,
            log_buf,
            stdout_buf,
            stderr_buf,
            stdin_tx,
            live_delta: live_delta_arc,
            is_alive,
            was_killed,
            exit_code,
            done_notify,
            #[cfg(windows)]
            job_handle,
        });

        {
            let mut map = self.tasks.write().map_err(|e| e.to_string())?;
            if map.len() > 64 {
                let finished_ids: Vec<String> = map
                    .iter()
                    .filter(|(_, t)| !t.is_alive.load(Ordering::SeqCst))
                    .map(|(k, _)| k.clone())
                    .take(16)
                    .collect();
                for fid in finished_ids {
                    map.remove(&fid);
                }
            }
            map.insert(task_id.clone(), task);
        }

        Ok(task_id)
    }

    pub async fn wait_or_background(
        &self,
        task_id: &str,
        wait_ms: u64,
        cancel_rx: Option<tokio::sync::watch::Receiver<bool>>,
    ) -> Result<TaskExecOutcome, String> {
        let task = {
            let map = self.tasks.read().map_err(|e| e.to_string())?;
            map.get(task_id)
                .cloned()
                .ok_or_else(|| format!("Task not found: {}", task_id))?
        };

        let wait_dur = Duration::from_millis(wait_ms.clamp(100, 120_000));
        let notified = task.done_notify.notified();
        tokio::pin!(notified);

        if task.is_alive.load(Ordering::SeqCst) {
            if let Some(mut rx) = cancel_rx {
                let already_canceled = *rx.borrow();
                if already_canceled {
                    let _ = self.kill_task(task_id).await;
                    return Ok(TaskExecOutcome::Canceled {
                        duration_ms: task.started_at.elapsed().as_millis() as u64,
                    });
                }
                let canceled = tokio::select! {
                    _ = async {
                        loop {
                            if *rx.borrow() {
                                break;
                            }
                            if rx.changed().await.is_err() {
                                break;
                            }
                        }
                    } => true,
                    _ = &mut notified => false,
                    _ = tokio::time::sleep(wait_dur) => false,
                };
                if canceled {
                    let _ = self.kill_task(task_id).await;
                    return Ok(TaskExecOutcome::Canceled {
                        duration_ms: task.started_at.elapsed().as_millis() as u64,
                    });
                }
            } else {
                tokio::select! {
                    _ = &mut notified => {}
                    _ = tokio::time::sleep(wait_dur) => {}
                }
            }
        }

        if let Ok(mut ld) = task.live_delta.lock() {
            *ld = None;
        }

        let duration_ms = task.started_at.elapsed().as_millis() as u64;
        if task.is_alive.load(Ordering::SeqCst) {
            let so_far_bytes = task
                .log_buf
                .lock()
                .map(|mut lb| lb.read_unread(65536))
                .unwrap_or_default();
            let output_so_far = strip_ansi(&String::from_utf8_lossy(&so_far_bytes));
            return Ok(TaskExecOutcome::Backgrounded {
                task_id: task_id.to_string(),
                output_so_far,
                duration_ms,
            });
        }

        let out_bytes = task.stdout_buf.lock().map(|b| b.clone()).unwrap_or_default();
        let err_bytes = task.stderr_buf.lock().map(|b| b.clone()).unwrap_or_default();
        let _ = task.log_buf.lock().map(|mut lb| lb.read_unread(usize::MAX));
        let stdout = strip_ansi(&String::from_utf8_lossy(&out_bytes));
        let stderr = strip_ansi(&String::from_utf8_lossy(&err_bytes));
        let mut output = String::new();
        if !stdout.is_empty() {
            output.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&stderr);
        }
        let exit_code = task.exit_code.lock().ok().and_then(|ec| *ec).unwrap_or(0);

        Ok(TaskExecOutcome::Completed {
            task_id: task_id.to_string(),
            exit_code,
            stdout,
            stderr,
            output,
            duration_ms,
        })
    }

    pub fn list_tasks(&self, chat_id: Option<&str>) -> Vec<Value> {
        let mut items = Vec::new();
        if let Ok(map) = self.tasks.read() {
            for t in map.values() {
                if let Some(cid) = chat_id.filter(|c| !c.is_empty()) {
                    if t.chat_id.as_deref() != Some(cid) {
                        continue;
                    }
                }
                let alive = t.is_alive.load(Ordering::SeqCst);
                let killed = t.was_killed.load(Ordering::SeqCst);
                let ec = t.exit_code.lock().ok().and_then(|g| *g);
                let status = if alive {
                    "running"
                } else if killed {
                    "killed"
                } else {
                    "completed"
                };
                items.push(json!({
                    "task_id": t.task_id,
                    "command": t.command,
                    "cwd": t.cwd,
                    "status": status,
                    "running": alive,
                    "exit_code": ec,
                    "elapsed_ms": t.started_at.elapsed().as_millis() as u64,
                }));
            }
        }
        if let Ok(pty_map) = self.sessions.read() {
            for s in pty_map.values() {
                if let Some(cid) = chat_id.filter(|c| !c.is_empty()) {
                    if s.chat_id.as_deref() != Some(cid) {
                        continue;
                    }
                }
                let alive = s.is_alive.load(Ordering::SeqCst);
                let ec = s.inner.lock().ok().and_then(|g| g.exit_code);
                items.push(json!({
                    "task_id": s.session_id,
                    "status": if alive { "running" } else { "completed" },
                    "running": alive,
                    "exit_code": ec,
                }));
            }
        }
        items
    }

    pub async fn task_status(&self, task_id: &str, wait_ms: u64) -> Result<Value, String> {
        let task_opt = self.tasks.read().ok().and_then(|m| m.get(task_id).cloned());
        if let Some(task) = task_opt {
            let has_unread = task
                .log_buf
                .lock()
                .map(|lb| lb.read_cursor < lb.base_offset + lb.bytes.len())
                .unwrap_or(false);
            if task.is_alive.load(Ordering::SeqCst) && !has_unread && wait_ms > 0 {
                let notified = task.done_notify.notified();
                tokio::select! {
                    _ = notified => {}
                    _ = tokio::time::sleep(Duration::from_millis(wait_ms.min(10_000))) => {}
                }
            }
            let alive = task.is_alive.load(Ordering::SeqCst);
            let killed = task.was_killed.load(Ordering::SeqCst);
            let ec = task.exit_code.lock().ok().and_then(|g| *g);
            let (new_bytes, tail_bytes) = task
                .log_buf
                .lock()
                .map(|mut lb| (lb.read_unread(65536), lb.tail(16384)))
                .unwrap_or_default();
            let new_output = strip_ansi(&String::from_utf8_lossy(&new_bytes));
            let recent_output = strip_ansi(&String::from_utf8_lossy(&tail_bytes));
            let status = if alive {
                "running"
            } else if killed {
                "killed"
            } else {
                "completed"
            };
            return Ok(json!({
                "task_id": task.task_id,
                "command": task.command,
                "cwd": task.cwd,
                "status": status,
                "running": alive,
                "exit_code": ec,
                "elapsed_ms": task.started_at.elapsed().as_millis() as u64,
                "new_output": new_output,
                "recent_output": recent_output,
            }));
        }

        if self.sessions.read().ok().and_then(|m| m.get(task_id).map(|_| ())).is_some() {
            let alive = self.is_session_alive(task_id);
            let ec = self.get_exit_code(task_id);
            let raw = self.read_pty(task_id, 65536).unwrap_or_default();
            let text = strip_ansi(&String::from_utf8_lossy(&raw));
            return Ok(json!({
                "task_id": task_id,
                "status": if alive { "running" } else { "completed" },
                "running": alive,
                "exit_code": ec,
                "new_output": text,
                "recent_output": text,
            }));
        }

        Err(self.task_not_found_err(task_id))
    }

    fn task_not_found_err(&self, task_id: &str) -> String {
        let mut active: Vec<String> = self
            .tasks
            .read()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        if let Ok(sm) = self.sessions.read() {
            for k in sm.keys() {
                if !active.contains(k) {
                    active.push(k.clone());
                }
            }
        }
        if active.is_empty() {
            format!(
                "Task not found: '{}' (no active or recent background tasks in memory — it may have been cleared on app restart).",
                task_id
            )
        } else {
            format!(
                "Task not found: '{}' (it may have been cleared on app restart). Known task_ids: {}",
                task_id,
                active.join(", ")
            )
        }
    }

    pub async fn send_task_input(&self, task_id: &str, input: &str) -> Result<Value, String> {
        let task_opt = self.tasks.read().ok().and_then(|m| m.get(task_id).cloned());
        if let Some(task) = task_opt {
            if !task.is_alive.load(Ordering::SeqCst) {
                return Err(format!("Task '{}' is no longer running.", task_id));
            }
            let payload = {
                #[cfg(windows)]
                {
                    if input.contains('\n') && !input.contains("\r\n") {
                        input.replace('\n', "\r\n")
                    } else {
                        input.to_string()
                    }
                }
                #[cfg(not(windows))]
                {
                    input.to_string()
                }
            };
            let bytes = payload.as_bytes();
            {
                let mut guard = task.stdin_tx.lock().await;
                let Some(ref mut stdin) = *guard else {
                    return Err(format!("Stdin is closed for task '{}'.", task_id));
                };
                stdin.write_all(bytes).await.map_err(|e| e.to_string())?;
                stdin.flush().await.map_err(|e| e.to_string())?;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            let new_bytes = task
                .log_buf
                .lock()
                .map(|mut lb| lb.read_unread(32768))
                .unwrap_or_default();
            let new_output = strip_ansi(&String::from_utf8_lossy(&new_bytes));
            return Ok(json!({
                "task_id": task_id,
                "bytes_written": bytes.len(),
                "running": task.is_alive.load(Ordering::SeqCst),
                "new_output": new_output,
            }));
        }

        let payload = {
            #[cfg(windows)]
            {
                if input.contains('\n') && !input.contains("\r\n") {
                    input.replace('\n', "\r\n")
                } else {
                    input.to_string()
                }
            }
            #[cfg(not(windows))]
            {
                input.to_string()
            }
        };
        let n = self.write_pty(task_id, payload.as_bytes())?;
        tokio::time::sleep(Duration::from_millis(250)).await;
        let raw = self.read_pty(task_id, 32768).unwrap_or_default();
        Ok(json!({
            "task_id": task_id,
            "bytes_written": n,
            "running": self.is_session_alive(task_id),
            "new_output": strip_ansi(&String::from_utf8_lossy(&raw)),
        }))
    }

    pub async fn kill_task(&self, task_id: &str) -> Result<Value, String> {
        let task_opt = self.tasks.read().ok().and_then(|m| m.get(task_id).cloned());
        if let Some(task) = task_opt {
            task.was_killed.store(true, Ordering::SeqCst);
            task.is_alive.store(false, Ordering::SeqCst);
            if let Ok(mut ld) = task.live_delta.lock() {
                *ld = None;
            }
            {
                let mut guard = task.stdin_tx.lock().await;
                *guard = None;
            }
            #[cfg(windows)]
            {
                if let Ok(mut jh) = task.job_handle.lock() {
                    jh.terminate();
                }
                if let Some(pid) = task.pid {
                    let mut tk = create_cmd("taskkill");
                    tk.args(["/F", "/T", "/PID", &pid.to_string()]);
                    let _ = tk.output();
                }
            }
            #[cfg(not(windows))]
            {
                if let Some(pid) = task.pid {
                    let _ = kill_process(pid, true);
                }
            }
            task.done_notify.notify_waiters();
            let tail_bytes = task
                .log_buf
                .lock()
                .map(|lb| lb.tail(16384))
                .unwrap_or_default();
            return Ok(json!({
                "task_id": task_id,
                "killed": true,
                "status": "killed",
                "final_output": strip_ansi(&String::from_utf8_lossy(&tail_bytes)),
            }));
        }

        self.kill_pty(task_id)?;
        Ok(json!({
            "task_id": task_id,
            "killed": true,
            "status": "killed",
        }))
    }

    pub fn spawn_pty(
        &self,
        cols: u16,
        rows: u16,
        command: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: Option<HashMap<String, String>>,
    ) -> Result<String, String> {
        self.spawn_pty_for_chat(cols, rows, command, args, cwd, env, None)
    }

    pub fn spawn_pty_for_chat(
        &self,
        cols: u16,
        rows: u16,
        command: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: Option<HashMap<String, String>>,
        chat_id: Option<&str>,
    ) -> Result<String, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        let mut cmd_builder = CommandBuilder::new(command);
        for arg in args {
            cmd_builder.arg(arg);
        }
        if let Some(dir) = cwd {
            cmd_builder.cwd(dir);
        }
        if let Some(env_map) = env {
            for (k, v) in env_map {
                cmd_builder.env(k, v);
            }
        }

        let child = pair
            .slave
            .spawn_command(cmd_builder)
            .map_err(|e| format!("Failed to spawn PTY child: {}", e))?;

        let session_id = Uuid::new_v4().to_string();
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to clone PTY reader: {}", e))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to take PTY writer: {}", e))?;

        let buffer = Arc::new(Mutex::new(Vec::new()));
        let is_alive = Arc::new(AtomicBool::new(true));

        let buf_clone = buffer.clone();
        let alive_clone = is_alive.clone();

        std::thread::spawn(move || {
            let mut read_chunk = [0u8; 4096];
            while alive_clone.load(Ordering::SeqCst) {
                match reader.read(&mut read_chunk) {
                    Ok(0) => {
                        alive_clone.store(false, Ordering::SeqCst);
                        break;
                    }
                    Ok(n) => {
                        if let Ok(mut lock) = buf_clone.lock() {
                            lock.extend_from_slice(&read_chunk[..n]);
                            if lock.len() > 2_000_000 {
                                let excess = lock.len() - 1_500_000;
                                lock.drain(0..excess);
                            }
                        }
                    }
                    Err(_) => {
                        alive_clone.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }
        });

        let session = PtySession {
            session_id: session_id.clone(),
            chat_id: chat_id.map(|s| s.to_string()),
            inner: Arc::new(Mutex::new(PtySessionInner {
                master: pair.master,
                writer,
                child,
                exit_code: None,
            })),
            buffer,
            is_alive,
        };

        self.sessions
            .write()
            .map_err(|e| e.to_string())?
            .insert(session_id.clone(), session);

        Ok(session_id)
    }

    pub fn write_pty(&self, session_id: &str, data: &[u8]) -> Result<usize, String> {
        let sessions = self.sessions.read().map_err(|e| e.to_string())?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("PTY session not found: {}", session_id))?;

        let mut inner = session.inner.lock().map_err(|e| e.to_string())?;
        inner.writer.write_all(data).map_err(|e| e.to_string())?;
        inner.writer.flush().map_err(|e| e.to_string())?;
        Ok(data.len())
    }

    pub fn read_pty(&self, session_id: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
        let sessions = self.sessions.read().map_err(|e| e.to_string())?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("PTY session not found: {}", session_id))?;

        let mut buf = session.buffer.lock().map_err(|e| e.to_string())?;
        let take_len = buf.len().min(max_bytes);
        let drained: Vec<u8> = buf.drain(0..take_len).collect();
        Ok(drained)
    }

    pub fn resize_pty(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self.sessions.read().map_err(|e| e.to_string())?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("PTY session not found: {}", session_id))?;

        let inner = session.inner.lock().map_err(|e| e.to_string())?;
        inner
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Failed to resize PTY: {}", e))?;
        Ok(())
    }

    pub fn is_session_alive(&self, session_id: &str) -> bool {
        if let Ok(guard) = self.tasks.read() {
            if let Some(t) = guard.get(session_id) {
                return t.is_alive.load(Ordering::SeqCst);
            }
        }
        if let Ok(guard) = self.sessions.read() {
            if let Some(s) = guard.get(session_id) {
                if !s.is_alive.load(Ordering::SeqCst) {
                    return false;
                }
                if let Ok(mut inner) = s.inner.lock() {
                    if let Ok(Some(status)) = inner.child.try_wait() {
                        inner.exit_code = Some(status.exit_code() as i32);
                        s.is_alive.store(false, Ordering::SeqCst);
                        return false;
                    }
                }
                return true;
            }
        }
        false
    }

    pub fn get_exit_code(&self, session_id: &str) -> Option<i32> {
        if let Ok(guard) = self.tasks.read() {
            if let Some(t) = guard.get(session_id) {
                return t.exit_code.lock().ok().and_then(|ec| *ec);
            }
        }
        let guard = self.sessions.read().ok()?;
        let s = guard.get(session_id)?;
        if let Ok(mut inner) = s.inner.lock() {
            if inner.exit_code.is_some() {
                return inner.exit_code;
            }
            if let Ok(Some(status)) = inner.child.try_wait() {
                let code = status.exit_code() as i32;
                inner.exit_code = Some(code);
                s.is_alive.store(false, Ordering::SeqCst);
                return Some(code);
            }
        }
        None
    }

    pub fn kill_pty(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
        if let Some(session) = sessions.get(session_id) {
            session.is_alive.store(false, Ordering::SeqCst);
            if let Ok(mut inner) = session.inner.lock() {
                #[cfg(windows)]
                if let Some(pid) = inner.child.process_id() {
                    let mut tk = create_cmd("taskkill");
                    tk.args(["/F", "/T", "/PID", &pid.to_string()]);
                    let _ = tk.output();
                }
                let _ = inner.child.kill();
            }
            if sessions.len() > 64 {
                let dead_ids: Vec<String> = sessions
                    .iter()
                    .filter(|(id, s)| *id != session_id && !s.is_alive.load(Ordering::SeqCst))
                    .map(|(id, _)| id.clone())
                    .take(16)
                    .collect();
                for id in dead_ids {
                    sessions.remove(&id);
                }
            }
        }
        Ok(())
    }

    pub fn kill_all_for_chat(&self, chat_id: &str) -> Result<(), String> {
        if chat_id.trim().is_empty() {
            return Ok(());
        }
        if let Ok(mut tasks) = self.tasks.write() {
            let tids: Vec<String> = tasks
                .iter()
                .filter(|(_, t)| chat_id.is_empty() || t.chat_id.as_deref() == Some(chat_id))
                .map(|(k, _)| k.clone())
                .collect();
            for tid in tids {
                if let Some(t) = tasks.remove(&tid) {
                    t.was_killed.store(true, Ordering::SeqCst);
                    t.is_alive.store(false, Ordering::SeqCst);
                    #[cfg(windows)]
                    {
                        if let Ok(mut jh) = t.job_handle.lock() {
                            jh.terminate();
                        }
                        if let Some(pid) = t.pid {
                            let mut tk = create_cmd("taskkill");
                            tk.args(["/F", "/T", "/PID", &pid.to_string()]);
                            let _ = tk.output();
                        }
                    }
                    t.done_notify.notify_waiters();
                }
            }
        }
        let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
        let target_ids: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| chat_id.is_empty() || s.chat_id.as_deref() == Some(chat_id))
            .map(|(k, _)| k.clone())
            .collect();
        for sid in target_ids {
            if let Some(session) = sessions.remove(&sid) {
                session.is_alive.store(false, Ordering::SeqCst);
                if let Ok(mut inner) = session.inner.lock() {
                    let _ = inner.child.kill();
                }
            }
        }
        Ok(())
    }

    pub fn kill_all(&self) -> Result<(), String> {
        if let Ok(mut tasks) = self.tasks.write() {
            for (_, t) in tasks.drain() {
                t.was_killed.store(true, Ordering::SeqCst);
                t.is_alive.store(false, Ordering::SeqCst);
                #[cfg(windows)]
                {
                    if let Ok(mut jh) = t.job_handle.lock() {
                        jh.terminate();
                    }
                    if let Some(pid) = t.pid {
                        let mut tk = create_cmd("taskkill");
                        tk.args(["/F", "/T", "/PID", &pid.to_string()]);
                        let _ = tk.output();
                    }
                }
                t.done_notify.notify_waiters();
            }
        }
        let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
        for (_, session) in sessions.drain() {
            session.is_alive.store(false, Ordering::SeqCst);
            if let Ok(mut inner) = session.inner.lock() {
                let _ = inner.child.kill();
            }
        }
        Ok(())
    }

    pub fn is_alive(&self, session_id: &str) -> bool {
        if let Ok(tasks) = self.tasks.read() {
            if let Some(t) = tasks.get(session_id) {
                return t.is_alive.load(Ordering::SeqCst);
            }
        }
        if let Ok(sessions) = self.sessions.read() {
            if let Some(session) = sessions.get(session_id) {
                return session.is_alive.load(Ordering::SeqCst);
            }
        }
        false
    }
}
