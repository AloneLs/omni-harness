use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone)]
pub struct FileReadStamp {
    pub mtime_nanos: u128,
    pub hash: u64,
}

#[derive(Debug, Clone, Default)]
pub struct FileReadGuard {
    stamps: Arc<RwLock<HashMap<PathBuf, FileReadStamp>>>,
}

impl FileReadGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hash_bytes(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    pub fn file_mtime_nanos(path: &Path) -> u128 {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    pub fn record_read(&self, path: &Path, content_bytes: &[u8]) {
        let stamp = FileReadStamp {
            mtime_nanos: Self::file_mtime_nanos(path),
            hash: Self::hash_bytes(content_bytes),
        };
        if let Ok(mut guard) = self.stamps.write() {
            guard.insert(path.to_path_buf(), stamp);
        }
    }

    pub fn verify_before_write(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let stamp = {
            let guard = self.stamps.read().map_err(|_| "FileReadGuard lock poisoned".to_string())?;
            guard.get(path).cloned()
        };
        let Some(prev) = stamp else {
            return Err(format!(
                "Read-Before-Write Guard: call read_file on '{}' before modifying an existing file.",
                path.display()
            ));
        };
        let current_mtime = Self::file_mtime_nanos(path);
        if current_mtime != prev.mtime_nanos {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            let current_hash = Self::hash_bytes(&bytes);
            if current_hash != prev.hash {
                return Err(format!(
                    "Stale File Guard: '{}' was modified on disk since last read_file. Re-read it before editing.",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    pub fn scan_external_changes(&self) -> Vec<String> {
        let mut changed = Vec::new();
        let Ok(mut guard) = self.stamps.write() else {
            return changed;
        };
        let paths: Vec<PathBuf> = guard.keys().cloned().collect();
        for p in paths {
            if !p.exists() {
                continue;
            }
            let Some(prev) = guard.get(&p).cloned() else {
                continue;
            };
            let current_mtime = Self::file_mtime_nanos(&p);
            if current_mtime != prev.mtime_nanos {
                if let Ok(bytes) = std::fs::read(&p) {
                    let current_hash = Self::hash_bytes(&bytes);
                    if current_hash != prev.hash {
                        changed.push(p.to_string_lossy().to_string());
                        guard.insert(
                            p,
                            FileReadStamp {
                                mtime_nanos: current_mtime,
                                hash: current_hash,
                            },
                        );
                    } else {
                        guard.insert(
                            p,
                            FileReadStamp {
                                mtime_nanos: current_mtime,
                                hash: prev.hash,
                            },
                        );
                    }
                }
            }
        }
        changed
    }
}

pub struct SecretShield;

impl SecretShield {
    pub fn check_path(path: &Path) -> Result<(), String> {
        let lower = path.to_string_lossy().to_lowercase().replace('\\', "/");
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        if file_name == ".env"
            || file_name.starts_with(".env.")
            || file_name.ends_with(".pem")
            || file_name.ends_with(".key")
            || file_name == "id_rsa"
            || file_name == "id_ed25519"
            || lower.contains("/.ssh/")
            || lower.contains("/.aws/")
            || lower.contains("/.omniharness/security")
        {
            return Err(format!(
                "Secret Shield: access to sensitive credential file '{}' is blocked.",
                path.display()
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPolicyLevel {
    Green,
    Yellow,
    Red,
}

pub struct ShellPolicyGuard;

impl ShellPolicyGuard {
    pub fn classify(command: &str, plan_mode: bool) -> Result<CommandPolicyLevel, String> {
        let lower = command.to_lowercase();
        let red_patterns = [
            "rm -rf /",
            "rm -rf /*",
            "mkfs",
            "format c:",
            "dd if=",
            ":(){ :|:& };:",
            "env | curl",
            "printenv | curl",
            "curl.*id_rsa",
            "powershell -enc",
        ];
        for pat in red_patterns {
            if lower.contains(pat) {
                return Err(format!("Policy Guard (RED): blocked dangerous command pattern '{}'", pat));
            }
        }

        if plan_mode {
            let segments: Vec<&str> = lower
                .split(['|', ';', '\n'])
                .flat_map(|s| s.split("&&"))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();

            let allowed_ro = [
                "dir", "ls", "cat", "type", "head", "tail", "find", "where", "which",
                "grep", "rg", "git status", "git log", "git diff", "git branch",
                "cargo check", "cargo test", "npm test", "pwd", "echo", "tree",
            ];
            for seg in segments {
                if !allowed_ro.iter().any(|p| seg.starts_with(p)) {
                    return Err(format!(
                        "Plan Mode Guard: command '{}' is blocked in read-only Plan mode.",
                        seg
                    ));
                }
            }
            return Ok(CommandPolicyLevel::Green);
        }

        let yellow_patterns = ["git push", "npm publish", "cargo publish", "rm -rf", "del /s"];
        if yellow_patterns.iter().any(|p| lower.contains(p)) {
            return Ok(CommandPolicyLevel::Yellow);
        }

        Ok(CommandPolicyLevel::Green)
    }
}

pub fn record_shadow_checkpoint(workspace_root: &str, label: &str) {
    let ws = Path::new(workspace_root);
    if !ws.is_dir() {
        return;
    }
    let shadow_dir = crate::state::AppState::resolve_chat_temp_dir(None).join("shadow-git");
    let git_dir_str = shadow_dir.to_string_lossy().to_string();

    let run_git = |args: &[&str]| -> bool {
        let mut cmd = std::process::Command::new("git");
        cmd.arg("--git-dir").arg(&git_dir_str).arg("--work-tree").arg(workspace_root).args(args);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    };

    if !shadow_dir.exists() {
        let _ = std::fs::create_dir_all(&shadow_dir);
        if !run_git(&["init", "-q"]) {
            return;
        }
        let _ = run_git(&["config", "user.email", "shadow@omniharness.local"]);
        let _ = run_git(&["config", "user.name", "OmniHarness Shadow"]);
    }

    let _ = run_git(&["add", "-A", "--", ".", ":!.omniharness", ":!node_modules", ":!target"]);
    let msg = format!("checkpoint: {}", label);
    let _ = run_git(&["commit", "-q", "-m", &msg, "--allow-empty"]);
}
