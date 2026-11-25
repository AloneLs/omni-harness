use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Vec<ProjectNode>,
    pub size_bytes: u64,
    pub detected_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStructure {
    pub root_path: String,
    pub project_kind: String,
    pub tree: ProjectNode,
    pub total_files: usize,
    pub total_directories: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolItem {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub line_number: usize,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportNode {
    pub file_path: String,
    pub imports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportGraph {
    pub nodes: Vec<ImportNode>,
    pub internal_edges: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotItem {
    pub file_path: String,
    pub change_count: usize,
    pub line_count: usize,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatusResult {
    pub branch: String,
    pub is_clean: bool,
    pub modified: Vec<String>,
    pub added: Vec<String>,
    pub deleted: Vec<String>,
    pub untracked: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitInfo {
    pub commit_hash: String,
    pub author_name: String,
    pub author_email: String,
    pub date: String,
    pub message: String,
}

fn create_cmd(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }
    cmd
}

fn detect_project_kind(root: &Path) -> String {
    if root.join("Cargo.toml").exists() {
        return "Rust (Cargo)".to_string();
    }
    if root.join("package.json").exists() {
        return "Node.js (NPM/Yarn/PNPM)".to_string();
    }
    if root.join("build.gradle").exists() || root.join("build.gradle.kts").exists() {
        return "Java/Kotlin (Gradle)".to_string();
    }
    if root.join("pom.xml").exists() {
        return "Java (Maven)".to_string();
    }
    if root.join("pyproject.toml").exists() || root.join("requirements.txt").exists() {
        return "Python".to_string();
    }
    if root.join("go.mod").exists() {
        return "Go".to_string();
    }
    if root.join("CMakeLists.txt").exists() {
        return "C/C++ (CMake)".to_string();
    }
    if root.join("Makefile").exists() {
        return "C/C++ (Make)".to_string();
    }
    "Generic".to_string()
}

fn build_tree(path: &Path, current_depth: usize, max_depth: usize) -> Option<ProjectNode> {
    if current_depth > max_depth {
        return None;
    }

    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    if path.is_file() {
        let size_bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();
        return Some(ProjectNode {
            name: file_name,
            path: path.to_string_lossy().to_string(),
            is_dir: false,
            children: Vec::new(),
            size_bytes,
            detected_type: Some(ext),
        });
    }

    if path.is_dir() {
        let mut children = Vec::new();
        let mut total_size = 0u64;

        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.filter_map(Result::ok) {
                let p = entry.path();
                let name = p
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                if name.starts_with('.') && name != ".env" {
                    continue;
                }
                if name == "target" || name == "node_modules" || name == "vendor" {
                    continue;
                }
                if let Some(child_node) = build_tree(&p, current_depth + 1, max_depth) {
                    total_size += child_node.size_bytes;
                    children.push(child_node);
                }
            }
        }

        children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        return Some(ProjectNode {
            name: file_name,
            path: path.to_string_lossy().to_string(),
            is_dir: true,
            children,
            size_bytes: total_size,
            detected_type: None,
        });
    }

    None
}

fn count_nodes(node: &ProjectNode) -> (usize, usize) {
    if !node.is_dir {
        return (1, 0);
    }
    let mut files = 0;
    let mut dirs = 1;
    for child in &node.children {
        let (f, d) = count_nodes(child);
        files += f;
        dirs += d;
    }
    (files, dirs)
}

pub fn code_structure_map(root_path: &str, max_depth: usize) -> Result<ProjectStructure, String> {
    let root = Path::new(root_path);
    if !root.exists() {
        return Err(format!("Directory does not exist: {}", root_path));
    }

    let project_kind = detect_project_kind(root);
    let tree = build_tree(root, 0, max_depth).ok_or_else(|| "Failed to build project tree".to_string())?;
    let (total_files, total_directories) = count_nodes(&tree);

    Ok(ProjectStructure {
        root_path: root.to_string_lossy().to_string(),
        project_kind,
        tree,
        total_files,
        total_directories,
    })
}

pub fn symbol_search(
    root_path: &str,
    query: &str,
    symbol_types: Option<Vec<String>>,
) -> Result<Vec<SymbolItem>, String> {
    let root = Path::new(root_path);
    if !root.exists() {
        return Err(format!("Path does not exist: {}", root_path));
    }

    let query_lower = query.to_lowercase();
    let type_filters: Option<HashSet<String>> = symbol_types
        .map(|list| list.into_iter().map(|s| s.to_lowercase()).collect());

    let rust_fn_re = Regex::new(r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([a-zA-Z0-9_]+)\s*(?:<[^>]*>)?\s*\(").unwrap();
    let rust_struct_re = Regex::new(r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:struct|enum|trait|union|type|mod)\s+([a-zA-Z0-9_]+)").unwrap();
    let rust_impl_re = Regex::new(r"(?m)^\s*impl(?:<[^>]*>)?\s+(?:([a-zA-Z0-9_:]+)\s+for\s+)?([a-zA-Z0-9_:]+)").unwrap();
    let rust_const_re = Regex::new(r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:const|static)\s+([A-Z0-9_]{2,})\s*:").unwrap();
    let ts_fn_re = Regex::new(r"(?m)^\s*(?:export\s+(?:default\s+)?)?(?:async\s+)?function\s+([a-zA-Z0-9_]+)").unwrap();
    let ts_arrow_re = Regex::new(r"(?m)^\s*(?:export\s+)?(?:const|let)\s+([a-zA-Z0-9_]+)\s*=\s*(?:async\s*)?(?:\([^)]*\)|[a-zA-Z0-9_]+)\s*=>").unwrap();
    let ts_class_re = Regex::new(r"(?m)^\s*(?:export\s+)?(?:class|interface|type|enum)\s+([a-zA-Z0-9_]+)").unwrap();
    let py_fn_re = Regex::new(r"(?m)^\s*(?:async\s+)?def\s+([a-zA-Z0-9_]+)\s*\(").unwrap();
    let py_class_re = Regex::new(r"(?m)^\s*class\s+([a-zA-Z0-9_]+)").unwrap();
    let go_fn_re = Regex::new(r"(?m)^\s*func\s+(?:\([^)]*\)\s*)?([a-zA-Z0-9_]+)\s*\(").unwrap();
    let go_type_re = Regex::new(r"(?m)^\s*type\s+([a-zA-Z0-9_]+)\s+(?:struct|interface)\b").unwrap();
    let cpp_class_re = Regex::new(r"(?m)^\s*(?:class|struct|enum(?:\s+class)?|namespace)\s+([a-zA-Z0-9_]+)").unwrap();

    let mut results = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            n != ".git" && n != "node_modules" && n != "target"
        })
        .build();

    for entry in walker.filter_map(Result::ok) {
        if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
            continue;
        }

        let p = entry.path();
        let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let is_rust = ext == "rs";
        let is_ts_js = matches!(ext.as_str(), "ts" | "tsx" | "js" | "jsx" | "svelte" | "mjs");
        let is_py = ext == "py";
        let is_go = ext == "go";
        let is_cpp = matches!(ext.as_str(), "c" | "h" | "cpp" | "hpp" | "cc" | "hh");

        if !is_rust && !is_ts_js && !is_py && !is_go && !is_cpp {
            continue;
        }

        let Ok(content) = fs::read_to_string(p) else {
            continue;
        };

        let file_path_str = p.to_string_lossy().to_string();

        let check_and_push = |results: &mut Vec<SymbolItem>, name: &str, kind: &str, line_num: usize, sig: &str| {
            if !query_lower.is_empty() && !name.to_lowercase().contains(&query_lower) {
                return;
            }
            if let Some(ref filters) = type_filters {
                if !filters.contains(kind) {
                    return;
                }
            }
            if results.len() < 400 {
                results.push(SymbolItem {
                    name: name.to_string(),
                    kind: kind.to_string(),
                    file_path: file_path_str.clone(),
                    line_number: line_num,
                    signature: sig.to_string(),
                });
            }
        };

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            if is_rust {
                if let Some(caps) = rust_fn_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "function", line_num, line.trim());
                } else if let Some(caps) = rust_struct_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "type", line_num, line.trim());
                } else if let Some(caps) = rust_impl_re.captures(line) {
                    let target = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                    if !target.is_empty() {
                        check_and_push(&mut results, target, "impl", line_num, line.trim());
                    }
                } else if let Some(caps) = rust_const_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "const", line_num, line.trim());
                }
            } else if is_ts_js {
                if let Some(caps) = ts_fn_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "function", line_num, line.trim());
                } else if let Some(caps) = ts_arrow_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "function", line_num, line.trim());
                } else if let Some(caps) = ts_class_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "class", line_num, line.trim());
                }
            } else if is_py {
                if let Some(caps) = py_fn_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "function", line_num, line.trim());
                } else if let Some(caps) = py_class_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "class", line_num, line.trim());
                }
            } else if is_go {
                if let Some(caps) = go_fn_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "function", line_num, line.trim());
                } else if let Some(caps) = go_type_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "type", line_num, line.trim());
                }
            } else if is_cpp {
                if let Some(caps) = cpp_class_re.captures(line) {
                    check_and_push(&mut results, &caps[1], "type", line_num, line.trim());
                }
            }
        }
        if results.len() >= 400 {
            break;
        }
    }

    Ok(results)
}

pub fn import_graph(root_path: &str) -> Result<ImportGraph, String> {
    let root = Path::new(root_path);
    if !root.exists() {
        return Err(format!("Path does not exist: {}", root_path));
    }

    let rust_use_re = Regex::new(r"(?m)^\s*(?:pub\s+)?use\s+([^;]+);").unwrap();
    let ts_import_re = Regex::new(r#"(?m)^\s*import\s+.*?\s+from\s+['"]([^'"]+)['"]"#).unwrap();
    let py_import_re = Regex::new(r"(?m)^\s*(?:from\s+([a-zA-Z0-9_\.]+)\s+import|import\s+([a-zA-Z0-9_\.]+))").unwrap();
    let cpp_include_re = Regex::new(r#"^\s*#include\s*[<"]([^>"]+)[>"]"#).unwrap();

    let mut nodes = Vec::new();
    let mut file_set = HashSet::new();

    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .build();

    for entry in walker.filter_map(Result::ok) {
        if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
            continue;
        }

        let p = entry.path();
        let ext = p.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        let Ok(content) = fs::read_to_string(p) else {
            continue;
        };

        let file_path_str = p.to_string_lossy().to_string();
        file_set.insert(file_path_str.clone());

        let mut imports = Vec::new();
        for line in content.lines() {
            if ext == "rs" {
                if let Some(caps) = rust_use_re.captures(line) {
                    imports.push(caps[1].trim().to_string());
                }
            } else if ext == "ts" || ext == "tsx" || ext == "js" || ext == "svelte" {
                if let Some(caps) = ts_import_re.captures(line) {
                    imports.push(caps[1].trim().to_string());
                }
            } else if ext == "py" {
                if let Some(caps) = py_import_re.captures(line) {
                    if let Some(m) = caps.get(1).or_else(|| caps.get(2)) {
                        imports.push(m.as_str().trim().to_string());
                    }
                }
            } else if ext == "cpp" || ext == "hpp" || ext == "c" || ext == "h" {
                if let Some(caps) = cpp_include_re.captures(line) {
                    imports.push(caps[1].trim().to_string());
                }
            }
        }

        nodes.push(ImportNode {
            file_path: file_path_str,
            imports,
        });
    }

    let mut internal_edges = Vec::new();
    for node in &nodes {
        let parent_dir = Path::new(&node.file_path).parent().unwrap_or_else(|| Path::new(""));
        for imp in &node.imports {
            if imp.starts_with("./") || imp.starts_with("../") {
                let resolved = parent_dir.join(imp);
                for ext in &["", ".ts", ".tsx", ".js", ".jsx", ".svelte", ".rs", ".py"] {
                    let cand = resolved.with_extension(ext.trim_start_matches('.'));
                    let cand_str = cand.to_string_lossy().to_string();
                    if file_set.contains(&cand_str) {
                        internal_edges.push((node.file_path.clone(), cand_str));
                        break;
                    }
                }
            }
        }
    }

    Ok(ImportGraph {
        nodes,
        internal_edges,
    })
}

pub fn hotspot_detection(root_path: &str, max_entries: usize) -> Result<Vec<HotspotItem>, String> {
    let root = Path::new(root_path);
    if !root.exists() {
        return Err(format!("Path does not exist: {}", root_path));
    }

    let mut churn_map: HashMap<String, usize> = HashMap::new();

    let output = create_cmd("git")
        .current_dir(root)
        .args(&["log", "--name-only", "--pretty=format:", "-n", "200"])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let log_text = String::from_utf8_lossy(&out.stdout);
            for line in log_text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    *churn_map.entry(trimmed.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    let mut items = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .build();

    for entry in walker.filter_map(Result::ok) {
        if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
            continue;
        }

        let p = entry.path();
        let rel_path = p.strip_prefix(root).unwrap_or(p).to_string_lossy().to_string();
        let rel_normalized = rel_path.replace('\\', "/");

        let change_count = churn_map.get(&rel_normalized).copied().unwrap_or(1);
        let line_count = fs::read_to_string(p)
            .map(|c| c.lines().count())
            .unwrap_or(0);

        if line_count == 0 {
            continue;
        }

        let score = (change_count as f64) * (line_count as f64).log10().max(1.0);

        items.push(HotspotItem {
            file_path: rel_normalized,
            change_count,
            line_count,
            score,
        });
    }

    items.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    items.truncate(max_entries);

    Ok(items)
}

pub fn git_status(repo_path: &str) -> Result<GitStatusResult, String> {
    let out = create_cmd("git")
        .current_dir(repo_path)
        .args(&["status", "--porcelain=v1", "-b"])
        .output()
        .map_err(|e| format!("Git status failed: {}", e))?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut branch = String::new();
    let mut modified = Vec::new();
    let mut added = Vec::new();
    let mut deleted = Vec::new();
    let mut untracked = Vec::new();

    for line in stdout.lines() {
        if line.starts_with("##") {
            branch = line.trim_start_matches("##").trim().to_string();
        } else if line.len() >= 3 {
            let status_code = &line[0..2];
            let file_name = line[3..].trim().to_string();

            if status_code.contains('M') {
                modified.push(file_name);
            } else if status_code.contains('A') {
                added.push(file_name);
            } else if status_code.contains('D') {
                deleted.push(file_name);
            } else if status_code == "??" {
                untracked.push(file_name);
            }
        }
    }

    let is_clean = modified.is_empty() && added.is_empty() && deleted.is_empty() && untracked.is_empty();

    Ok(GitStatusResult {
        branch,
        is_clean,
        modified,
        added,
        deleted,
        untracked,
    })
}

pub fn git_diff(repo_path: &str, staged: bool, file_path: Option<&str>) -> Result<String, String> {
    let mut cmd = create_cmd("git");
    cmd.current_dir(repo_path).arg("diff");

    if staged {
        cmd.arg("--staged");
    }
    if let Some(fp) = file_path {
        cmd.arg("--").arg(fp);
    }

    let out = cmd.output().map_err(|e| format!("Git diff failed: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }

    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn git_log(repo_path: &str, max_count: usize) -> Result<Vec<GitCommitInfo>, String> {
    let format_arg = "--pretty=format:%H%x1f%an%x1f%ae%x1f%ad%x1f%s%x1e";
    let count_str = max_count.to_string();

    let out = create_cmd("git")
        .current_dir(repo_path)
        .args(&["log", format_arg, "-n", &count_str])
        .output()
        .map_err(|e| format!("Git log failed: {}", e))?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut commits = Vec::new();

    for block in stdout.split('\x1e') {
        let trimmed = block.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split('\x1f').collect();
        if parts.len() >= 5 {
            commits.push(GitCommitInfo {
                commit_hash: parts[0].to_string(),
                author_name: parts[1].to_string(),
                author_email: parts[2].to_string(),
                date: parts[3].to_string(),
                message: parts[4].to_string(),
            });
        }
    }

    Ok(commits)
}

pub fn git_branch(repo_path: &str) -> Result<Vec<String>, String> {
    let out = create_cmd("git")
        .current_dir(repo_path)
        .args(&["branch", "-a"])
        .output()
        .map_err(|e| format!("Git branch failed: {}", e))?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let branches = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(branches)
}
