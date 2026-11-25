use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const MAX_SKILL_FILE_BYTES: u64 = 2_000_000;
pub const MAX_SKILL_TOTAL_BYTES: u64 = 25_000_000;
pub const MAX_SKILL_FILES: usize = 500;
pub const MAX_SKILL_CONTEXT_CHARS: usize = 120_000;
pub const AUTO_SELECTION_THRESHOLD: i32 = 12;

pub const REQUIRED_SECTIONS: [&str; 7] = [
    "## When to use",
    "## Inputs",
    "## Rules",
    "## Workflow",
    "## Output",
    "## Assets and scripts",
    "## Validation",
];

fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val = serde_yaml::Value::deserialize(deserializer)?;
    match val {
        serde_yaml::Value::Sequence(seq) => Ok(seq
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect()),
        serde_yaml::Value::String(s) => Ok(s
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect()),
        _ => Ok(Vec::new()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    #[serde(default)]
    pub title: String,
    pub description: String,
    pub command: Option<String>,
    #[serde(default = "default_version")]
    pub version: String,
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default, rename = "allowed-tools", deserialize_with = "deserialize_string_or_vec")]
    pub allowed_tools: Vec<String>,
    #[serde(default, rename = "disable-model-invocation")]
    pub disable_model_invocation: bool,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub trusted: bool,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPackage {
    pub metadata: SkillMetadata,
    pub root_dir: String,
    pub skill_markdown: String,
    pub body_content: String,
    pub has_assets: bool,
    pub has_scripts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillScore {
    pub skill_name: String,
    pub score: i32,
    pub matched_keywords: Vec<String>,
    pub matched_tags: Vec<String>,
    pub matched_name: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillValidationReport {
    pub is_valid: bool,
    pub missing_sections: Vec<String>,
    pub error_message: Option<String>,
}

pub fn parse_skill_markdown(raw_content: &str) -> Result<(SkillMetadata, String), String> {
    let trimmed = raw_content.trim_start();
    if !trimmed.starts_with("---") {
        return Err("Missing YAML frontmatter prefix '---'".to_string());
    }

    let rest = &trimmed[3..];
    let end_idx = rest.find("---").ok_or_else(|| "Unclosed YAML frontmatter delimiter '---'".to_string())?;

    let yaml_str = &rest[..end_idx];
    let body_str = rest[end_idx + 3..].trim().to_string();

    let mut meta: SkillMetadata = serde_yaml::from_str(yaml_str)
        .map_err(|e| format!("YAML frontmatter parsing error: {}", e))?;
    if meta.title.trim().is_empty() {
        meta.title = meta.name.clone();
    }
    if meta.name.trim().is_empty() || meta.description.trim().is_empty() {
        return Err("Skill frontmatter must define non-empty name and description".to_string());
    }

    Ok((meta, body_str))
}

pub fn validate_skill_sections(body_content: &str) -> SkillValidationReport {
    let mut missing = Vec::new();
    let lower_body = body_content.to_lowercase();

    for req in &REQUIRED_SECTIONS {
        let req_clean = req.to_lowercase();
        if !lower_body.contains(&req_clean) {
            missing.push(req.to_string());
        }
    }

    let is_valid = missing.is_empty();
    let error_message = if !is_valid {
        Some(format!("Missing required sections: {}", missing.join(", ")))
    } else {
        None
    };

    SkillValidationReport {
        is_valid,
        missing_sections: missing,
        error_message,
    }
}

pub fn calculate_skill_score(query: &str, skill: &SkillPackage) -> SkillScore {
    let q_lower = query.to_lowercase();
    let q_words: HashSet<&str> = q_lower.split_whitespace().collect();

    let mut score = 0i32;
    let mut matched_keywords = Vec::new();
    let mut matched_tags = Vec::new();
    let mut matched_name = false;

    for kw in &skill.metadata.keywords {
        let kw_lower = kw.to_lowercase();
        if q_lower.contains(&kw_lower) {
            let kw_len_bonus = (kw_lower.len() as i32).min(10);
            score += 20 + kw_len_bonus;
            matched_keywords.push(kw.clone());
        }
    }

    for tag in &skill.metadata.tags {
        let tag_lower = tag.to_lowercase();
        if q_words.contains(tag_lower.as_str()) || q_lower.contains(&tag_lower) {
            score += 7;
            matched_tags.push(tag.clone());
        }
    }

    let name_lower = skill.metadata.name.to_lowercase();
    if q_lower.contains(&name_lower) {
        score += 10;
        matched_name = true;
    } else if let Some(ref cmd) = skill.metadata.command {
        let cmd_clean = cmd.trim_start_matches('/').to_lowercase();
        if q_lower.contains(&cmd_clean) {
            score += 10;
            matched_name = true;
        }
    }

    let desc_lower = skill.metadata.description.to_lowercase();
    for word in desc_lower.split_whitespace() {
        if word.len() > 3 && q_words.contains(word) {
            score += 1;
        }
    }

    SkillScore {
        skill_name: skill.metadata.name.clone(),
        score,
        matched_keywords,
        matched_tags,
        matched_name,
    }
}

pub fn select_applicable_skills(query: &str, skills: &[SkillPackage]) -> Vec<(SkillPackage, SkillScore)> {
    let mut scored: Vec<(SkillPackage, SkillScore)> = skills
        .iter()
        .filter(|s| s.metadata.enabled)
        .map(|s| {
            let sc = calculate_skill_score(query, s);
            (s.clone(), sc)
        })
        .filter(|(_, sc)| sc.score >= AUTO_SELECTION_THRESHOLD)
        .collect();

    scored.sort_by(|a, b| b.1.score.cmp(&a.1.score));
    scored
}

pub fn load_skill_from_directory(dir_path: &Path) -> Result<SkillPackage, String> {
    if !dir_path.is_dir() {
        return Err(format!("Skill path is not a directory: {:?}", dir_path));
    }

    let skill_md_path = dir_path.join("SKILL.md");
    if !skill_md_path.exists() {
        return Err(format!("SKILL.md not found in {:?}", dir_path));
    }

    let md_meta = fs::metadata(&skill_md_path).map_err(|e| e.to_string())?;
    if md_meta.len() > MAX_SKILL_FILE_BYTES {
        return Err(format!(
            "SKILL.md exceeds maximum allowed size of {} bytes",
            MAX_SKILL_FILE_BYTES
        ));
    }

    let raw_content = fs::read_to_string(&skill_md_path).map_err(|e| e.to_string())?;
    let (metadata, body_content) = parse_skill_markdown(&raw_content)?;



    let has_assets = dir_path.join("assets").is_dir();
    let has_scripts = dir_path.join("scripts").is_dir();

    Ok(SkillPackage {
        metadata,
        root_dir: dir_path.to_string_lossy().to_string(),
        skill_markdown: raw_content,
        body_content,
        has_assets,
        has_scripts,
    })
}

pub fn load_all_skills(skills_dir: &Path) -> Result<Vec<SkillPackage>, String> {
    if !skills_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    let entries = fs::read_dir(skills_dir).map_err(|e| e.to_string())?;

    for entry in entries.filter_map(Result::ok) {
        let p = entry.path();
        if p.is_dir() {
            if let Ok(skill) = load_skill_from_directory(&p) {
                skills.push(skill);
            }
        }
    }

    skills.sort_by(|a, b| a.metadata.name.cmp(&b.metadata.name));
    Ok(skills)
}


pub fn remove_dir_all_force(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_file() || path.is_symlink() {
        if let Ok(meta) = path.symlink_metadata() {
            let mut perms = meta.permissions();
            if perms.readonly() {
                perms.set_readonly(false);
                let _ = fs::set_permissions(path, perms);
            }
        }
        return fs::remove_file(path);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if let Ok(meta) = entry_path.symlink_metadata() {
            let mut perms = meta.permissions();
            if perms.readonly() {
                perms.set_readonly(false);
                let _ = fs::set_permissions(&entry_path, perms);
            }
        }
        if entry_path.is_dir() {
            remove_dir_all_force(&entry_path)?;
        } else {
            let _ = fs::remove_file(&entry_path);
        }
    }
    if let Ok(meta) = path.symlink_metadata() {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
    fs::remove_dir(path)
}

pub fn copy_dir_force(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let dest_path = dst.join(entry.file_name());
        if entry_path.is_dir() {
            copy_dir_force(&entry_path, &dest_path)?;
        } else {
            let _ = fs::copy(&entry_path, &dest_path)?;
            if let Ok(meta) = dest_path.symlink_metadata() {
                let mut perms = meta.permissions();
                if perms.readonly() {
                    perms.set_readonly(false);
                    let _ = fs::set_permissions(&dest_path, perms);
                }
            }
        }
    }
    Ok(())
}

pub fn migrate_legacy_external_skills(skills_dir: &Path) {
    let mut candidate_roots = Vec::new();
    let mut home_candidates = Vec::new();
    if let Some(h) = dirs::home_dir() {
        home_candidates.push(h);
    }
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let h = PathBuf::from(home);
        if !home_candidates.iter().any(|c| c == &h) {
            home_candidates.push(h);
        }
    }
    for h in home_candidates {
        candidate_roots.push(h.join(".agents").join("skills"));
        candidate_roots.push(h.join(".claude").join("skills"));
        candidate_roots.push(h.join(".gemini").join("skills"));
        candidate_roots.push(h.join(".gemini").join("config").join("skills"));
        candidate_roots.push(h.join(".omni").join("skills"));
    }

    for root in candidate_roots {
        if !root.is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name.eq_ignore_ascii_case("java-patcher") {
                    continue;
                }
                if path.join("SKILL.md").exists() {
                    let dest = skills_dir.join(&name);
                    if !dest.exists() {
                        let _ = copy_dir_force(&path, &dest);
                    }
                }
            }
        }
    }
}

pub fn skill_search_roots(_workspace_root: &str) -> Vec<PathBuf> {
    vec![crate::state::AppState::resolve_skills_dir()]
}

fn builtin_system_skills() -> Vec<SkillPackage> {
    let env_body = r#"## When to use
Load when you need full knowledge of the OmniHarness runtime environment, directories, persistence, tools, checkpoints, or error recovery.

## Environment Layout
- App Data & SQLite DB: %LOCALAPPDATA%\.omni-harness\omniharness.db (WAL mode: chats, messages, memory_entries, checkpoints, events, plan_states, session_artifacts, compaction_events, token_cache)
- Workspace Root: active project directory. All file tools (read_file, edit_file, patch_file, write_file, glob, grep) are strictly sandboxed to allowed workspace roots after canonicalize() — UNC paths and Windows reserved device names (CON, NUL, PRN, AUX, COM1-9, LPT1-9) are blocked.
- %LOCALAPPDATA%\.omni-harness\projects\<hash>\objects\: content-addressed checkpoint blobs (never created inside project directory).
- %LOCALAPPDATA%\.omni-harness\projects\<hash>\tmp\tool_<id>.log: spilled large tool outputs (>300 lines or >24KB), readable via read_file/grep by absolute path.
- %LOCALAPPDATA%\.omni-harness\projects\<hash>\tg_uploads\: incoming files sent via Telegram bot, readable via read_file by absolute path.
- %LOCALAPPDATA%\.omni-harness\projects\<hash>\PROJECT_SUMMARY.md: Markdown project summary (managed via project_summary tool, never inside project dir).
- skills/: User skills (%LOCALAPPDATA%\.omni-harness\skills).

## Tool Contracts & Safety
- Always pass valid JSON matching each tool's schema (exact parameter names, no XML tags or raw wrappers).
- File edits: call read_file before edit_file/patch_file/write_file (enforced by blake3+mtime FileReadGuard). edit_file requires unique old_text unless replace_all=true.
- Terminal: terminal_exec uses redirected stdout/stderr pipes for foreground commands (returns exit_code, stdout, stderr, duration_ms, spill_file) with Win32 Job Object process-tree kill on timeout; background=true uses PTY (monitor with terminal_io).
- Memory & Summary: use memory_tool for cross-session facts/preferences and project_summary for the project Markdown summary.
- Telegram: use tg_send_file(action="send|status|cancel") for background non-blocking file delivery.
- Subagents: use agent(subagent_type="Explore|Plan|general-purpose|verification") for isolated/parallel work."#;

    vec![SkillPackage {
        metadata: SkillMetadata {
            name: "system:omni-environment".to_string(),
            title: "OmniHarness Runtime Environment".to_string(),
            description: "Internal system reference for OmniHarness directories, SQLite tables, checkpoints, tool contracts, and runtime behavior (hidden from user UI).".to_string(),
            command: None,
            version: "1.0.0".to_string(),
            author: Some("OmniHarness".to_string()),
            tags: vec!["system".to_string(), "environment".to_string()],
            keywords: vec!["omniharness".to_string(), "environment".to_string(), "harness".to_string()],
            triggers: vec![],
            paths: vec![],
            allowed_tools: vec![],
            disable_model_invocation: false,
            context: None,
            auto_start: false,
            enabled: true,
            trusted: true,
        },
        root_dir: String::new(),
        skill_markdown: env_body.to_string(),
        body_content: env_body.to_string(),
        has_assets: false,
        has_scripts: false,
    }]
}

pub fn discover_all_skills(workspace_root: &str) -> Vec<SkillPackage> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for sys_sk in builtin_system_skills() {
        let key = sys_sk.metadata.name.to_ascii_lowercase();
        if seen.insert(key) {
            result.push(sys_sk);
        }
    }
    for root in skill_search_roots(workspace_root) {
        if !root.is_dir() {
            continue;
        }
        if let Ok(list) = load_all_skills(&root) {
            for sk in list {
                let key = sk.metadata.name.to_ascii_lowercase();
                if seen.insert(key) {
                    result.push(sk);
                }
            }
        }
    }
    result.sort_by(|a, b| a.metadata.name.cmp(&b.metadata.name));
    result
}

pub fn list_skill_resources(skill_dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for sub in ["scripts", "references", "assets"] {
        let dir = skill_dir.join(sub);
        if !dir.is_dir() {
            continue;
        }
        let walker = ignore::WalkBuilder::new(&dir)
            .max_depth(Some(3))
            .hidden(true)
            .build();
        for entry in walker.filter_map(Result::ok) {
            let p = entry.path();
            if p.is_file() {
                if let Ok(rel) = p.strip_prefix(skill_dir) {
                    files.push(rel.to_string_lossy().replace('\\', "/"));
                    if files.len() >= 80 {
                        return files;
                    }
                }
            }
        }
    }
    files
}

pub fn render_skill_activation(skill: &SkillPackage, args: Option<&str>) -> String {
    let mut body = skill.body_content.clone();
    let trimmed_args = args.map(|s| s.trim()).unwrap_or("");
    if !trimmed_args.is_empty() {
        if body.contains("$ARGUMENTS") {
            body = body.replace("$ARGUMENTS", trimmed_args);
        } else {
            body.push_str(&format!("\n\nUser Arguments: {}", trimmed_args));
        }
        let parts: Vec<&str> = trimmed_args.split_whitespace().collect();
        for (i, part) in parts.iter().enumerate() {
            body = body.replace(&format!("${}", i + 1), part);
        }
    }
    let dir_path = Path::new(&skill.root_dir);
    let resources = list_skill_resources(dir_path);
    let mut out = format!(
        "<skill_content name=\"{}\">\n{}\n\nSkill directory: {}\nExecution rules:\n- Relative paths (scripts/*, references/*, assets/*) resolve against Skill directory above, NOT the project working directory.\n- Run scripts via terminal_exec using their full path (or cd to Skill directory first) instead of reading script source into context unless internals inspection is required. Use --help if flags are unclear.",
        skill.metadata.name,
        body.trim(),
        skill.root_dir
    );
    if !resources.is_empty() {
        out.push_str("\n<skill_resources>");
        for r in resources {
            out.push_str(&format!("\n<file>{}</file>", r));
        }
        out.push_str("\n</skill_resources>");
    }
    out.push_str("\n</skill_content>");
    out
}

pub fn build_skills_catalog_for_workspace(workspace_root: &str) -> String {
    let skills = discover_all_skills(workspace_root);
    let visible: Vec<SkillPackage> = skills
        .into_iter()
        .filter(|s| s.metadata.enabled && !s.metadata.disable_model_invocation)
        .collect();
    if visible.is_empty() {
        return String::new();
    }
    let mut out = String::from("<available_skills>\n");
    let max_chars = 15_000usize;
    for sk in visible {
        let cmd_line = sk
            .metadata
            .command
            .as_deref()
            .filter(|c| !c.trim().is_empty())
            .map(|c| format!("\n    <command>{}</command>", c.trim()))
            .unwrap_or_default();
        let loc_line = if sk.root_dir.trim().is_empty() {
            String::new()
        } else {
            format!("\n    <location>{}</location>", Path::new(&sk.root_dir).join("SKILL.md").to_string_lossy())
        };
        let entry = format!(
            "  <skill>\n    <name>{}</name>\n    <description>{}</description>{}{}\n  </skill>\n",
            sk.metadata.name,
            sk.metadata.description.trim(),
            cmd_line,
            loc_line
        );
        if out.len() + entry.len() > max_chars {
            break;
        }
        out.push_str(&entry);
    }
    out.push_str("</available_skills>");
    out
}

pub fn deploy_builtin_skills(skills_dir: &Path) -> Result<(), String> {
    if !skills_dir.exists() {
        fs::create_dir_all(skills_dir).map_err(|e| e.to_string())?;
    }

    let creator_dir = skills_dir.join("skill-creator");
    if !creator_dir.exists() {
        fs::create_dir_all(&creator_dir).map_err(|e| e.to_string())?;
        let skill_content = r#"---
name: skill-creator
title: Skill Authoring & Validation Utility
description: Specialized assistant for authoring, linting, validating, and testing custom Omni-Harness skills.
command: /skill-create
version: 1.0.0
author: System
tags:
  - meta
  - skill
  - workflow
keywords:
  - make skill
  - create skill
  - new skill
  - skill validator
auto_start: false
enabled: true
trusted: true
---

# Skill Creator

## When to use
Activate this skill whenever the user asks to create, update, or validate an Omni-Harness skill bundle.

## Inputs
- Desired skill name, command shortcut, and domain description.
- Workflows, tool requirements, and expected operational parameters.

## Rules
- Every skill must have valid YAML frontmatter between triple dashes.
- All 7 mandatory H2 headings must be present.
- Total skill file size must not exceed safety thresholds.

## Workflow
1. Gather the target requirements from the user request.
2. Draft the YAML frontmatter with tags and matching keywords.
3. Complete each of the seven required sections thoroughly.
4. Verify section headers against REQUIRED_SECTIONS.
5. Save the generated SKILL.md into the designated skills subfolder.

## Output
A validated SKILL.md file and optional helper scripts inside scripts/ directory.

## Assets and scripts
- scripts/validate_skill.py
- assets/template_skill.md

## Validation
Ensure that `validate_skill_sections` passes with 0 missing sections and file size is under 2 MB.
"#;
        fs::write(creator_dir.join("SKILL.md"), skill_content.trim().as_bytes())
            .map_err(|e| e.to_string())?;
    }

    let patcher_dir = skills_dir.join("java-patcher");
    if patcher_dir.exists() {
        let _ = remove_dir_all_force(&patcher_dir);
    }

    migrate_legacy_external_skills(skills_dir);

    Ok(())
}

pub fn export_skill_zip(skill_name: &str, skills_root: &Path) -> Result<Vec<u8>, String> {
    let skill_dir = skills_root.join(skill_name);
    if !skill_dir.is_dir() {
        return Err(format!("Skill directory not found: {:?}", skill_dir));
    }

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buffer);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o755);

        let walker = ignore::WalkBuilder::new(&skill_dir)
            .hidden(false)
            .git_ignore(false)
            .build();

        for entry in walker.filter_map(Result::ok) {
            let p = entry.path();
            if p == skill_dir {
                continue;
            }

            let rel = p
                .strip_prefix(&skill_dir)
                .map_err(|e| e.to_string())?;
            let rel_str = rel
                .to_str()
                .ok_or_else(|| "Invalid UTF-8 in path".to_string())?
                .replace('\\', "/");

            if p.is_file() {
                zip.start_file(rel_str, options).map_err(|e| e.to_string())?;
                let mut f = File::open(p).map_err(|e| e.to_string())?;
                let mut data = Vec::new();
                f.read_to_end(&mut data).map_err(|e| e.to_string())?;
                zip.write_all(&data).map_err(|e| e.to_string())?;
            } else if p.is_dir() {
                let dir_str = if rel_str.ends_with('/') {
                    rel_str
                } else {
                    format!("{}/", rel_str)
                };
                zip.add_directory(dir_str, options).map_err(|e| e.to_string())?;
            }
        }
        zip.finish().map_err(|e| e.to_string())?;
    }

    Ok(buffer.into_inner())
}

pub fn import_skill_zip(zip_bytes: &[u8], skills_root: &Path) -> Result<String, String> {
    if zip_bytes.len() as u64 > MAX_SKILL_TOTAL_BYTES {
        return Err(format!(
            "Zip archive exceeds maximum allowed size of {} bytes",
            MAX_SKILL_TOTAL_BYTES
        ));
    }

    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| e.to_string())?;

    if archive.len() > MAX_SKILL_FILES {
        return Err(format!(
            "Zip archive contains {} files, exceeding limit of {}",
            archive.len(),
            MAX_SKILL_FILES
        ));
    }

    let mut skill_md_content = None;
    let mut skill_folder_name = None;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();

        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') || name.contains(':') {
            return Err(format!("Unsafe path detected in zip archive: {}", name));
        }

        if entry.size() > MAX_SKILL_FILE_BYTES {
            return Err(format!(
                "File {} exceeds maximum per-file limit of {} bytes",
                name, MAX_SKILL_FILE_BYTES
            ));
        }

        let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty()).collect();
        let is_skill_md = parts.last().map(|s| *s == "SKILL.md").unwrap_or(false);
        if is_skill_md {
            let mut content = String::new();
            entry.read_to_string(&mut content).map_err(|e| e.to_string())?;
            skill_md_content = Some(content);

            if parts.len() > 1 {
                skill_folder_name = Some(parts[0].to_string());
            }
        }
    }

    let raw_md = skill_md_content.ok_or_else(|| "Zip archive does not contain SKILL.md".to_string())?;
    let (metadata, body) = parse_skill_markdown(&raw_md)?;
    let validation = validate_skill_sections(&body);
    if !validation.is_valid {
        return Err(validation.error_message.unwrap_or_else(|| "Invalid skill format in zip".to_string()));
    }

    let raw_folder = skill_folder_name.as_ref().cloned().unwrap_or_else(|| metadata.name.clone());
    let target_name: String = raw_folder
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if target_name.is_empty() {
        return Err("Invalid skill folder name".to_string());
    }
    let dest_dir = skills_root.join(&target_name);
    fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let raw_name = entry.name().replace('\\', "/");
        let stripped = if let Some(prefix) = &skill_folder_name {
            raw_name.strip_prefix(prefix).unwrap_or(&raw_name).trim_start_matches('/')
        } else {
            &raw_name
        };

        if stripped.is_empty() {
            continue;
        }

        let out_path = dest_dir.join(stripped);
        for comp in out_path.components() {
            if let Component::ParentDir = comp {
                return Err("Path traversal attempt in archive extraction".to_string());
            }
        }

        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out_path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
            }
            let mut outfile = File::create(&out_path).map_err(|e| e.to_string())?;
            let mut limited = entry.take(MAX_SKILL_FILE_BYTES + 1);
            let copied = std::io::copy(&mut limited, &mut outfile).map_err(|e| e.to_string())?;
            if copied > MAX_SKILL_FILE_BYTES {
                let _ = fs::remove_file(&out_path);
                return Err(format!("Decompressed file exceeds limit: {}", raw_name));
            }
        }
    }

    Ok(metadata.name)
}

pub fn is_always_apply_skill(meta: &SkillMetadata) -> bool {
    if meta.disable_model_invocation || !meta.enabled {
        return false;
    }
    if meta.auto_start {
        return true;
    }
    let name_l = meta.name.to_lowercase();
    let desc_l = meta.description.to_lowercase();
    name_l == "personal-profile"
        || desc_l.contains("всегда при начале нового разговора")
        || desc_l.contains("при начале нового разговора")
        || desc_l.contains("в начале каждого чата")
        || desc_l.contains("активируй этот навык всегда")
        || desc_l.contains("always activate")
        || desc_l.contains("always at the start")
        || desc_l.contains("always apply")
}

pub fn build_preactivated_skills_block(workspace_path: &str) -> String {
    let all = discover_all_skills(workspace_path);
    let mut out = String::new();
    for sk in all {
        if !is_always_apply_skill(&sk.metadata) {
            continue;
        }
        let body = sk.body_content.trim();
        if body.is_empty() {
            continue;
        }
        if out.is_empty() {
            out.push_str("
<preactivated_skills>
The following user skills are ALWAYS ACTIVE for every conversation. Follow them strictly without needing to call tool_hub:
");
        }
        out.push_str(&format!("
### Skill: {}
{}
", sk.metadata.name, body));
    }
    if !out.is_empty() {
        out.push_str("</preactivated_skills>
");
    }
    out
}
