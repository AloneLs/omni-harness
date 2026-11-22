use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, OnceLock};

enum LogCmd {
    SetPaths(Vec<PathBuf>),
    Line(String),
}

struct LoggerState {
    primary_path: String,
    trace_ring: VecDeque<String>,
    tx: SyncSender<LogCmd>,
}

static LOGGER: OnceLock<Mutex<LoggerState>> = OnceLock::new();

fn state() -> &'static Mutex<LoggerState> {
    LOGGER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<LogCmd>(1024);
        std::thread::spawn(move || {
            let mut files: Vec<File> = Vec::new();
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    LogCmd::SetPaths(paths) => {
                        files.clear();
                        for p in paths {
                            if let Ok(f) = OpenOptions::new().create(true).append(true).open(&p) {
                                files.push(f);
                            }
                        }
                    }
                    LogCmd::Line(text) => {
                        for f in &mut files {
                            let _ = f.write_all(text.as_bytes());
                        }
                    }
                }
            }
        });
        Mutex::new(LoggerState {
            primary_path: String::new(),
            trace_ring: VecDeque::with_capacity(120),
            tx,
        })
    })
}

fn ts() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

pub fn init(primary_dir: PathBuf) {
    let mut candidates = vec![primary_dir.join("omni-harness.log")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("omni-harness.log");
            if !candidates.contains(&p) {
                candidates.push(p);
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let p = cwd.join("omni-harness.log");
        if !candidates.contains(&p) {
            candidates.push(p);
        }
    }

    let mut active_paths = Vec::new();
    let header = format!("=== OmniHarness Run Log started at {} ===\n", ts());
    for path in candidates {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut f) = File::create(&path) {
            let _ = f.write_all(header.as_bytes());
            active_paths.push(path);
        }
    }

    if let Ok(mut st) = state().lock() {
        st.primary_path = active_paths
            .first()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        st.trace_ring.clear();
        let _ = st.tx.try_send(LogCmd::SetPaths(active_paths));
    }
}

pub fn get_log_path() -> String {
    if let Ok(st) = state().lock() {
        return st.primary_path.clone();
    }
    String::new()
}

fn append_raw(line: &str) {
    if let Ok(mut st) = state().lock() {
        if st.trace_ring.len() >= 100 {
            st.trace_ring.pop_front();
        }
        st.trace_ring.push_back(line.to_string());
        let mut payload = String::with_capacity(line.len() + 1);
        payload.push_str(line);
        payload.push('\n');
        let _ = st.tx.try_send(LogCmd::Line(payload));
    }
}

pub fn info(tag: &str, msg: &str) {
    append_raw(&format!("[{}] [INFO] [{}] {}", ts(), tag, msg));
}

pub fn warn(tag: &str, msg: &str) {
    append_raw(&format!("[{}] [WARN] [{}] {}", ts(), tag, msg));
}

pub fn record_http(tag: &str, method: &str, url: &str, status: u16, detail: &str) {
    let clean_detail: String = detail.chars().take(600).collect();
    append_raw(&format!(
        "[{}] [HTTP] [{}] {} {} -> {} | {}",
        ts(),
        tag,
        method,
        url,
        status,
        clean_detail
    ));
}

pub fn error_with_trace(tag: &str, msg: &str) {
    let err_line = format!("[{}] [ERROR] [{}] {}", ts(), tag, msg);
    if let Ok(mut st) = state().lock() {
        let mut dump = String::new();
        dump.push_str(&err_line);
        dump.push('\n');
        dump.push_str("--- FULL PRE-ERROR TRACE DUMP ---\n");
        for item in &st.trace_ring {
            dump.push_str(item);
            dump.push('\n');
        }
        dump.push_str("--- END TRACE DUMP ---\n");
        let _ = st.tx.try_send(LogCmd::Line(dump));
        if st.trace_ring.len() >= 100 {
            st.trace_ring.pop_front();
        }
        st.trace_ring.push_back(err_line);
    }
}
