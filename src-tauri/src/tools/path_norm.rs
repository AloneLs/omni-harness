use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

fn clean_raw_input(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("file:///") {
        urlencoding::decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.replace("%20", " "))
    } else if let Some(rest) = trimmed.strip_prefix("file://") {
        urlencoding::decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.replace("%20", " "))
    } else {
        trimmed.to_string()
    }
}

pub fn is_network_unc_path(raw: &str) -> bool {
    let s = raw.trim();
    let upper = s.to_ascii_uppercase();
    if upper.starts_with(r"\\?\UNC\") || upper.starts_with("//?/UNC/") {
        return true;
    }
    if s.starts_with(r"\\?\") || s.starts_with("//?/") {
        let rest = &s[4..];
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return false;
        }
        return true;
    }
    if s.starts_with(r"\\.\") || s.starts_with("//./") {
        return true;
    }
    s.starts_with(r"\\") || s.starts_with("//")
}

pub fn strip_verbatim_str(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.starts_with(r"\\?\") || trimmed.starts_with("//?/") {
        let rest = &trimmed[4..];
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            #[cfg(target_os = "windows")]
            return rest.replace('/', "\\");
            #[cfg(not(target_os = "windows"))]
            return rest.to_string();
        }
    }
    #[cfg(target_os = "windows")]
    {
        trimmed.replace('/', "\\")
    }
    #[cfg(not(target_os = "windows"))]
    {
        trimmed.to_string()
    }
}

pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    PathBuf::from(strip_verbatim_str(&path.to_string_lossy()))
}

pub fn is_reserved_windows_name(path: &Path) -> bool {
    for comp in path.components() {
        if let Component::Normal(seg) = comp {
            let s = seg.to_string_lossy();
            let stem = s
                .split('.')
                .next()
                .unwrap_or("")
                .trim_end_matches(' ')
                .to_ascii_uppercase();
            if matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL"
                    | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9"
                    | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"
            ) {
                return true;
            }
        }
    }
    false
}

pub fn lexical_clean(path: &Path) -> PathBuf {
    let stripped = strip_verbatim_prefix(path);
    let mut prefix: Option<OsString> = None;
    let mut has_root = false;
    let mut parts: Vec<OsString> = Vec::new();

    for comp in stripped.components() {
        match comp {
            Component::Prefix(p) => {
                prefix = Some(p.as_os_str().to_os_string());
            }
            Component::RootDir => {
                has_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !parts.is_empty() {
                    parts.pop();
                } else if !has_root && prefix.is_none() {
                    parts.push(OsString::from(".."));
                }
            }
            Component::Normal(seg) => {
                parts.push(seg.to_os_string());
            }
        }
    }

    let mut out = PathBuf::new();
    if let Some(p) = prefix {
        out.push(p);
    }
    if has_root {
        #[cfg(target_os = "windows")]
        out.push("\\");
        #[cfg(not(target_os = "windows"))]
        out.push("/");
    }
    for seg in parts {
        out.push(seg);
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

pub fn canonicalize_best_effort(target: &Path) -> Result<PathBuf, String> {
    let clean = lexical_clean(target);
    if clean.exists() {
        let can = clean.canonicalize().map_err(|e| e.to_string())?;
        return Ok(strip_verbatim_prefix(&can));
    }

    let mut cur: Option<&Path> = Some(clean.as_path());
    let mut missing_tail: Vec<OsString> = Vec::new();

    while let Some(p) = cur {
        if p.exists() {
            let mut base = strip_verbatim_prefix(&p.canonicalize().map_err(|e| e.to_string())?);
            for seg in missing_tail.iter().rev() {
                base.push(seg);
            }
            return Ok(lexical_clean(&base));
        }
        if let Some(name) = p.file_name() {
            missing_tail.push(name.to_os_string());
            cur = p.parent();
        } else {
            break;
        }
    }

    Err(format!("Invalid or non-existent root drive for path '{}'", target.display()))
}

pub fn resolve_against_base(raw_str: &str, working_dir: Option<&Path>) -> Result<PathBuf, String> {
    let cleaned_input = clean_raw_input(raw_str);
    if cleaned_input.is_empty() {
        return Err("Invalid path: path cannot be empty.".to_string());
    }
    if is_network_unc_path(&cleaned_input) {
        return Err(format!("Access denied: UNC or device path '{}' is not allowed.", raw_str));
    }

    let local_str = strip_verbatim_str(&cleaned_input);
    let raw_path = Path::new(&local_str);
    if is_reserved_windows_name(raw_path) {
        return Err(format!("Invalid path: '{}' uses a reserved Windows device name.", raw_str));
    }

    let base_fallback;
    let effective_base: Option<&Path> = match working_dir {
        Some(b) if !b.as_os_str().is_empty() => Some(b),
        _ => {
            base_fallback = std::env::current_dir().ok();
            base_fallback.as_deref()
        }
    };

    let joined = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        #[cfg(target_os = "windows")]
        {
            if local_str.starts_with('\\') || local_str.starts_with('/') {
                if let Some(base) = effective_base {
                    let base_clean = strip_verbatim_prefix(base);
                    if let Some(Component::Prefix(pref)) = base_clean.components().next() {
                        let mut rooted = PathBuf::from(pref.as_os_str());
                        rooted.push(&local_str);
                        rooted
                    } else {
                        raw_path.to_path_buf()
                    }
                } else {
                    raw_path.to_path_buf()
                }
            } else if let Some(base) = effective_base {
                strip_verbatim_prefix(base).join(raw_path)
            } else {
                raw_path.to_path_buf()
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            if let Some(base) = effective_base {
                base.join(raw_path)
            } else {
                raw_path.to_path_buf()
            }
        }
    };

    canonicalize_best_effort(&joined)
}

pub fn path_key(path: &Path) -> String {
    let resolved = canonicalize_best_effort(path).unwrap_or_else(|_| lexical_clean(path));
    #[cfg(target_os = "windows")]
    {
        let mut s = resolved.to_string_lossy().replace('/', "\\").to_lowercase();
        while s.len() > 3 && s.ends_with('\\') {
            s.pop();
        }
        s
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut s = resolved.to_string_lossy().to_string();
        while s.len() > 1 && s.ends_with('/') {
            s.pop();
        }
        s
    }
}

pub fn is_within_root(target: &Path, root: &Path) -> bool {
    if root.as_os_str().is_empty() {
        return false;
    }
    let t_key = path_key(target);
    let r_key = path_key(root);
    #[cfg(target_os = "windows")]
    {
        let r_trim = r_key.trim_end_matches('\\');
        if r_trim.len() <= 2 {
            let drive_root = format!("{}\\", r_trim);
            return t_key == drive_root || t_key.starts_with(&drive_root);
        }
        t_key == r_trim || t_key.starts_with(&format!("{}\\", r_trim))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let r_trim = r_key.trim_end_matches('/');
        if r_trim.is_empty() {
            return t_key.starts_with('/');
        }
        t_key == r_trim || t_key.starts_with(&format!("{}/", r_trim))
    }
}

pub fn validate_path_with_base(
    path_str: &str,
    working_dir: Option<&Path>,
    allowed_roots: &[PathBuf],
) -> Result<PathBuf, String> {
    let base = working_dir.or_else(|| allowed_roots.first().map(|p| p.as_path()));
    let resolved = resolve_against_base(path_str, base)?;
    if allowed_roots.is_empty() {
        return Ok(resolved);
    }
    for root in allowed_roots {
        if is_within_root(&resolved, root) {
            return Ok(resolved);
        }
    }
    Err(format!(
        "Access denied: path '{}' resolves outside allowed workspace root.",
        path_str
    ))
}

pub fn validate_read_path_with_base(
    path_str: &str,
    working_dir: Option<&Path>,
    allowed_roots: &[PathBuf],
) -> Result<PathBuf, String> {
    if let Ok(ok) = validate_path_with_base(path_str, working_dir, allowed_roots) {
        return Ok(ok);
    }
    let base = working_dir.or_else(|| allowed_roots.first().map(|p| p.as_path()));
    let resolved = resolve_against_base(path_str, base)?;
    if allowed_roots.is_empty() {
        return Ok(resolved);
    }
    for root in allowed_roots {
        let norm_root = canonicalize_best_effort(root).unwrap_or_else(|_| lexical_clean(root));
        if is_within_root(&norm_root, &resolved) && path_key(&resolved).len() > 3 {
            return Ok(resolved);
        }
        let mut cur = norm_root.as_path();
        for _ in 0..3 {
            if let Some(par) = cur.parent() {
                if path_key(par).len() > 3 && is_within_root(&resolved, par) {
                    return Ok(resolved);
                }
                cur = par;
            }
        }
    }
    Err(format!(
        "Access denied: path '{}' resolves outside allowed workspace root.",
        path_str
    ))
}
