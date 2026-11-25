use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const MAX_BLOB_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_CHECKPOINTS_PER_SESSION: usize = 100;
pub const MAX_STORE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobEntry {
    pub path: String,
    pub blob_hash: String,
    pub mode: u32,
    pub size: u64,
    pub mtime_nanos: u128,
    pub inode: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
    pub skipped_large: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointManifest {
    pub checkpoint_id: String,
    pub workspace_root: String,
    pub kind: String,
    pub entries: Vec<BlobEntry>,
    pub skipped_large_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewindResult {
    pub checkpoint_id: String,
    pub mode: String,
    pub restored_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub skipped_large_files: Vec<String>,
    pub dirty_external_files: Vec<String>,
}

pub fn objects_dir(workspace_root: &str) -> PathBuf {
    crate::state::AppState::resolve_project_data_dir(workspace_root).join("objects")
}

pub fn hash_file_streaming(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn hash_bytes_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn read_mode(meta: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return meta.permissions().mode();
    }
    #[cfg(not(unix))]
    {
        if meta.permissions().readonly() { 0o444 } else { 0o644 }
    }
}

fn read_inode(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return meta.ino();
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}

fn read_mtime_nanos(meta: &fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn write_blob_object(obj_dir: &Path, hash_hex: &str, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(obj_dir).map_err(|e| e.to_string())?;
    let obj_path = obj_dir.join(hash_hex);
    if obj_path.exists() {
        return Ok(());
    }
    let tmp_path = obj_dir.join(format!(".tmp.{}", uuid::Uuid::new_v4()));
    {
        let mut f = File::create(&tmp_path).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    if let Err(e) = fs::rename(&tmp_path, &obj_path) {
        let _ = fs::remove_file(&tmp_path);
        if !obj_path.exists() {
            return Err(e.to_string());
        }
    }
    Ok(())
}

pub fn capture_workspace_manifest(
    workspace_root: &str,
    checkpoint_id: &str,
    kind: &str,
    store_blobs: bool,
) -> Result<CheckpointManifest, String> {
    let root = Path::new(workspace_root);
    if !root.is_dir() {
        return Err(format!("Workspace root is not a directory: {}", workspace_root));
    }
    let obj_dir = objects_dir(workspace_root);
    let mut entries = Vec::new();
    let mut skipped_large_files = Vec::new();

    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != ".git"
                && name != ".harness"
                && name != ".omniharness"
                && name != "node_modules"
                && name != "target"
        })
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        let sym_meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if sym_meta.is_dir() {
            continue;
        }

        let is_symlink = sym_meta.file_type().is_symlink();
        let link_target = if is_symlink {
            fs::read_link(path).ok().map(|p| p.to_string_lossy().to_string())
        } else {
            None
        };

        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }

        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let size = meta.len();
        let mode = read_mode(&meta);
        let mtime_nanos = read_mtime_nanos(&meta);
        let inode = read_inode(&sym_meta);

        if size > MAX_BLOB_BYTES {
            let blob_hash = hash_file_streaming(path)?;
            skipped_large_files.push(rel_str.clone());
            entries.push(BlobEntry {
                path: rel_str,
                blob_hash,
                mode,
                size,
                mtime_nanos,
                inode,
                link_target,
                skipped_large: true,
            });
            continue;
        }

        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let blob_hash = hash_bytes_hex(&bytes);
        if store_blobs {
            write_blob_object(&obj_dir, &blob_hash, &bytes)?;
        }

        entries.push(BlobEntry {
            path: rel_str,
            blob_hash,
            mode,
            size,
            mtime_nanos,
            inode,
            link_target,
            skipped_large: false,
        });
    }

    Ok(CheckpointManifest {
        checkpoint_id: checkpoint_id.to_string(),
        workspace_root: workspace_root.to_string(),
        kind: kind.to_string(),
        entries,
        skipped_large_files,
    })
}

pub fn enforce_lru_disk_limit(workspace_root: &str, referenced_hashes: &HashSet<String>) {
    let obj_dir = objects_dir(workspace_root);
    if !obj_dir.is_dir() {
        return;
    }
    let mut items: Vec<(PathBuf, u64, u128, String)> = Vec::new();
    let mut total_size: u64 = 0;

    if let Ok(read_dir) = fs::read_dir(&obj_dir) {
        for ent in read_dir.flatten() {
            let p = ent.path();
            if !p.is_file() {
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if name.starts_with(".tmp.") {
                let _ = fs::remove_file(&p);
                continue;
            }
            if let Ok(meta) = ent.metadata() {
                let sz = meta.len();
                let mtime = read_mtime_nanos(&meta);
                total_size = total_size.saturating_add(sz);
                items.push((p, sz, mtime, name));
            }
        }
    }

    if total_size <= MAX_STORE_BYTES {
        return;
    }

    items.sort_by(|a, b| a.2.cmp(&b.2));

    for (p, sz, _, hash_name) in &items {
        if total_size <= MAX_STORE_BYTES {
            break;
        }
        if !referenced_hashes.contains(hash_name) {
            if fs::remove_file(p).is_ok() {
                total_size = total_size.saturating_sub(*sz);
            }
        }
    }

    let _ = total_size;
}

pub fn atomic_restore_batch(
    manifest: &CheckpointManifest,
    expected_hashes: Option<&HashMap<String, String>>,
) -> Result<RewindResult, String> {
    let root = Path::new(&manifest.workspace_root);
    let obj_dir = objects_dir(&manifest.workspace_root);

    let mut dirty_external_files = Vec::new();
    let mut skipped_large_files = Vec::new();
    let mut staged: Vec<(PathBuf, PathBuf, Option<PathBuf>)> = Vec::new();
    let mut to_restore_paths: HashSet<String> = HashSet::new();

    for entry in &manifest.entries {
        to_restore_paths.insert(entry.path.clone());
        let target = crate::tools::path_norm::validate_path_with_base(
            &entry.path,
            Some(root),
            std::slice::from_ref(&root.to_path_buf()),
        )?;

        if let Some(exp_map) = expected_hashes {
            if let Some(exp_hash) = exp_map.get(&entry.path) {
                if target.exists() {
                    if let Ok(cur_hash) = hash_file_streaming(&target) {
                        if &cur_hash != exp_hash {
                            dirty_external_files.push(entry.path.clone());
                        }
                    }
                }
            }
        }

        if entry.skipped_large {
            skipped_large_files.push(entry.path.clone());
            continue;
        }

        let obj_path = obj_dir.join(&entry.blob_hash);
        if !obj_path.is_file() {
            for (_, tmp, _) in &staged {
                let _ = fs::remove_file(tmp);
            }
            return Err(format!(
                "Missing blob {} for '{}' in .harness/objects",
                entry.blob_hash, entry.path
            ));
        }

        let write_target = if fs::symlink_metadata(&target)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            let resolved = fs::canonicalize(&target).map_err(|e| e.to_string())?;
            if !crate::tools::path_norm::is_within_root(&resolved, root) {
                for (_, tmp, _) in &staged {
                    let _ = fs::remove_file(tmp);
                }
                return Err(format!("Symlink target escapes workspace: {}", entry.path));
            }
            resolved
        } else {
            target.clone()
        };

        if let Some(parent) = write_target.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                for (_, tmp, _) in &staged {
                    let _ = fs::remove_file(tmp);
                }
                return Err(e.to_string());
            }
        }

        let blob_bytes = fs::read(&obj_path).map_err(|e| e.to_string())?;
        let tmp_path = write_target
            .parent()
            .unwrap_or(root)
            .join(format!(".omni.rewind.{}.tmp", uuid::Uuid::new_v4()));

        {
            let mut f = match File::create(&tmp_path) {
                Ok(f) => f,
                Err(e) => {
                    for (_, t, _) in &staged {
                        let _ = fs::remove_file(t);
                    }
                    return Err(e.to_string());
                }
            };
            if let Err(e) = f.write_all(&blob_bytes).and_then(|_| f.sync_all()) {
                let _ = fs::remove_file(&tmp_path);
                for (_, t, _) in &staged {
                    let _ = fs::remove_file(t);
                }
                return Err(e.to_string());
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(entry.mode));
        }
        #[cfg(not(unix))]
        {
            if let Ok(mut perms) = fs::metadata(&tmp_path).map(|m| m.permissions()) {
                perms.set_readonly((entry.mode & 0o200) == 0);
                let _ = fs::set_permissions(&tmp_path, perms);
            }
        }

        let backup_path = if write_target.exists() {
            let bk = write_target
                .parent()
                .unwrap_or(root)
                .join(format!(".omni.backup.{}.tmp", uuid::Uuid::new_v4()));
            if let Ok(existing) = fs::read(&write_target) {
                if fs::write(&bk, existing).is_ok() {
                    Some(bk)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        staged.push((write_target, tmp_path, backup_path));
    }

    let mut committed: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let mut restored_files = Vec::new();

    for (write_target, tmp_path, backup_path) in staged {
        if let Err(err) = fs::rename(&tmp_path, &write_target) {
            let _ = fs::remove_file(&tmp_path);
            for (prev_target, prev_bk) in committed {
                if let Some(bk) = prev_bk {
                    let _ = fs::rename(&bk, &prev_target);
                } else {
                    let _ = fs::remove_file(&prev_target);
                }
            }
            if let Some(bk) = backup_path {
                let _ = fs::remove_file(bk);
            }
            return Err(format!("Batch rewind failed at '{}': {}", write_target.display(), err));
        }
        if let Some(rel) = write_target.strip_prefix(root).ok() {
            restored_files.push(rel.to_string_lossy().replace('\\', "/"));
        } else {
            restored_files.push(write_target.to_string_lossy().to_string());
        }
        committed.push((write_target, backup_path));
    }

    for (_, bk_opt) in committed {
        if let Some(bk) = bk_opt {
            let _ = fs::remove_file(bk);
        }
    }

    Ok(RewindResult {
        checkpoint_id: manifest.checkpoint_id.clone(),
        mode: "code".to_string(),
        restored_files,
        deleted_files: Vec::new(),
        skipped_large_files,
        dirty_external_files,
    })
}
