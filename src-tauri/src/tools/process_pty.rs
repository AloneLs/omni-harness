use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};
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

pub struct PtySession {
    pub session_id: String,
    pub chat_id: Option<String>,
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    pub buffer: Arc<Mutex<Vec<u8>>>,
    pub is_alive: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, PtySession>>>,
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
            cmd.args(&["/F", "/T", "/PID", &pid_str]);
        } else {
            cmd.args(&["/PID", &pid_str]);
        }
        let out = cmd.output().map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(true);
        }
    }

    Ok(false)
}

#[allow(dead_code)]
fn execute_build_tool(
    executable: &str,
    args: &[&str],
    cwd: Option<&str>,
    timeout_secs: u64,
) -> Result<BuildOutput, String> {
    let start = Instant::now();
    let mut cmd = create_cmd(executable);
    cmd.args(args);

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", executable, e))?;

    let timeout = Duration::from_secs(if timeout_secs == 0 { 300 } else { timeout_secs });
    let output = child.wait_timeout(timeout);

    match output {
        Ok(Some(status)) => {
            let duration_ms = start.elapsed().as_millis();
            Ok(BuildOutput {
                success: status.success(),
                exit_code: status.code(),
                stdout: String::new(),
                stderr: String::new(),
                duration_ms,
            })
        }
        Ok(None) => Err(format!(
            "Execution of {} timed out after {} seconds",
            executable, timeout_secs
        )),
        Err(e) => Err(format!("Execution failed: {}", e)),
    }
}

pub fn run_command_direct(
    executable: &str,
    args: &[&str],
    cwd: Option<&str>,
) -> Result<BuildOutput, String> {
    let start = Instant::now();
    let mut cmd = create_cmd(executable);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let out = cmd
        .output()
        .map_err(|e| format!("Command execution failed: {}", e))?;
    let duration_ms = start.elapsed().as_millis();

    Ok(BuildOutput {
        success: out.status.success(),
        exit_code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        duration_ms,
    })
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
        }
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
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
            child: Arc::new(Mutex::new(child)),
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

        let mut writer = session.writer.lock().map_err(|e| e.to_string())?;
        writer.write_all(data).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
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

        let master = session.master.lock().map_err(|e| e.to_string())?;
        master
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
        if let Ok(guard) = self.sessions.read() {
            if let Some(s) = guard.get(session_id) {
                if !s.is_alive.load(Ordering::SeqCst) {
                    return false;
                }
                if let Ok(mut ch) = s.child.lock() {
                    if let Ok(Some(_)) = ch.try_wait() {
                        s.is_alive.store(false, Ordering::SeqCst);
                        return false;
                    }
                }
                return true;
            }
        }
        false
    }

    pub fn kill_pty(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
        if let Some(session) = sessions.remove(session_id) {
            session.is_alive.store(false, Ordering::SeqCst);
            if let Ok(mut ch) = session.child.lock() {
                let _ = ch.kill();
            }
        }
        Ok(())
    }

    pub fn kill_all_for_chat(&self, chat_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
        let target_ids: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| chat_id.is_empty() || s.chat_id.as_deref() == Some(chat_id))
            .map(|(k, _)| k.clone())
            .collect();
        for sid in target_ids {
            if let Some(session) = sessions.remove(&sid) {
                session.is_alive.store(false, Ordering::SeqCst);
                if let Ok(mut ch) = session.child.lock() {
                    let _ = ch.kill();
                }
            }
        }
        Ok(())
    }

    pub fn kill_all(&self) -> Result<(), String> {
        let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
        for (_, session) in sessions.drain() {
            session.is_alive.store(false, Ordering::SeqCst);
            if let Ok(mut ch) = session.child.lock() {
                let _ = ch.kill();
            }
        }
        Ok(())
    }

    pub fn is_alive(&self, session_id: &str) -> bool {
        if let Ok(sessions) = self.sessions.read() {
            if let Some(session) = sessions.get(session_id) {
                return session.is_alive.load(Ordering::SeqCst);
            }
        }
        false
    }
}
