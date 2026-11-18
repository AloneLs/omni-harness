use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

struct LoggerState {
    paths: Vec<PathBuf>,
    trace_ring: VecDeque<String>,
}

static LOGGER: OnceLock<Mutex<LoggerState>> = OnceLock::new();

fn state() -> &'static Mutex<LoggerState> {
    LOGGER.get_or_init(|| {
        Mutex::new(LoggerState {
            paths: Vec::new(),
            trace_ring: VecDeque::with_capacity(120),
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
    let header = format!(
        "=== OmniHarness Run Log started at {} ===\n",
        ts()
    );
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
        st.paths = active_paths;
        st.trace_ring.clear();
    }
}

pub fn get_log_path() -> String {
    if let Ok(st) = state().lock() {
        if let Some(p) = st.paths.first() {
            return p.to_string_lossy().to_string();
        }
    }
    String::new()
}

fn append_raw(line: &str) {
    if let Ok(mut st) = state().lock() {
        if st.trace_ring.len() >= 100 {
            st.trace_ring.pop_front();
        }
        st.trace_ring.push_back(line.to_string());
        for p in &st.paths {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
                let _ = f.write_all(line.as_bytes());
                let _ = f.write_all(b"\n");
            }
        }
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
        for p in &st.paths {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
                let _ = f.write_all(dump.as_bytes());
            }
        }
        if st.trace_ring.len() >= 100 {
            st.trace_ring.pop_front();
        }
        st.trace_ring.push_back(err_line);
    }
}
