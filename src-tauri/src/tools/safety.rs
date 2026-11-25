use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone)]
pub struct FileReadStamp {
    pub path: PathBuf,
    pub mtime_nanos: u128,
    pub hash: String,
}

#[derive(Debug, Clone, Default)]
pub struct FileReadGuard {
    stamps: Arc<RwLock<HashMap<String, FileReadStamp>>>,
}

impl FileReadGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hash_bytes(bytes: &[u8]) -> String {
        blake3::hash(bytes).to_hex().to_string()
    }

    pub fn get_last_hash(&self, path: &Path) -> Option<String> {
        let key = crate::tools::path_norm::path_key(path);
        self.stamps.read().ok().and_then(|g| g.get(&key).map(|s| s.hash.clone()))
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
        let clean_path = crate::tools::path_norm::strip_verbatim_prefix(path);
        let key = crate::tools::path_norm::path_key(&clean_path);
        let stamp = FileReadStamp {
            path: clean_path,
            mtime_nanos: Self::file_mtime_nanos(path),
            hash: Self::hash_bytes(content_bytes),
        };
        if let Ok(mut guard) = self.stamps.write() {
            guard.insert(key, stamp);
        }
    }

    pub fn verify_before_write(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let key = crate::tools::path_norm::path_key(path);
        let stamp = {
            let guard = self.stamps.read().map_err(|_| "FileReadGuard lock poisoned".to_string())?;
            guard.get(&key).cloned()
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
        let keys: Vec<String> = guard.keys().cloned().collect();
        for k in keys {
            let Some(prev) = guard.get(&k).cloned() else {
                continue;
            };
            if !prev.path.exists() {
                continue;
            }
            let current_mtime = Self::file_mtime_nanos(&prev.path);
            if current_mtime != prev.mtime_nanos {
                if let Ok(bytes) = std::fs::read(&prev.path) {
                    let current_hash = Self::hash_bytes(&bytes);
                    if current_hash != prev.hash {
                        changed.push(prev.path.to_string_lossy().to_string());
                        guard.insert(
                            k,
                            FileReadStamp {
                                path: prev.path,
                                mtime_nanos: current_mtime,
                                hash: current_hash,
                            },
                        );
                    } else {
                        guard.insert(
                            k,
                            FileReadStamp {
                                path: prev.path,
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
        let raw_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        let base_name = raw_name.split(':').next().unwrap_or(&raw_name);
        let file_name = base_name.trim_end_matches(['.', ' ']);

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
            "powershell -enc",
        ];
        for pat in red_patterns {
            if lower.contains(pat) {
                return Err(format!("Policy Guard (RED): blocked dangerous command pattern '{}'", pat));
            }
        }
        if lower.contains("curl") && lower.contains("id_rsa") {
            return Err("Policy Guard (RED): blocked dangerous command pattern 'curl + id_rsa'".to_string());
        }

        if plan_mode {
            let mut in_single = false;
            let mut in_double = false;
            let chars: Vec<char> = lower.chars().collect();
            for (idx, &ch) in chars.iter().enumerate() {
                if !in_double && ch == '\'' {
                    in_single = !in_single;
                    continue;
                }
                if !in_single && ch == '"' {
                    in_double = !in_double;
                    continue;
                }
                if in_single {
                    continue;
                }
                if ch == '`' || (ch == '$' && chars.get(idx + 1) == Some(&'(')) {
                    return Err("Plan Mode Guard: command substitution is blocked in read-only Plan mode.".to_string());
                }
                if !in_double && ch == '>' {
                    return Err("Plan Mode Guard: output redirection is blocked in read-only Plan mode.".to_string());
                }
            }

            let forbidden_in_plan = [
                "out-file", "set-content", "add-content", "new-item",
                "remove-item", "move-item", "copy-item", "rename-item",
                "-outfile", "-exec", "-delete", "tee ",
            ];
            for pat in forbidden_in_plan {
                if lower.contains(pat) {
                    return Err(format!(
                        "Plan Mode Guard: write/redirect pattern '{}' is blocked in read-only Plan mode.",
                        pat.trim()
                    ));
                }
            }
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
                "get-content", "get-childitem", "select-string",
                "get-item", "get-itemproperty", "test-path", "measure-object",
                "resolve-path", "split-path", "join-path",
                "select-object", "where-object", "sort-object", "format-table", "format-list",
            ];
            for seg in segments {
                let ok = allowed_ro.iter().any(|p| {
                    seg == *p || seg.strip_prefix(p).map(|r| r.starts_with(char::is_whitespace)).unwrap_or(false)
                });
                if !ok {
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
