//! File snapshots taken before edit tools run, used by `/undo`.

use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn checkpoint_dir() -> Result<PathBuf> {
    Ok(crate::config::data_dir()?.join("tagent/checkpoints"))
}

fn session_dir(root: &Path, session_id: &str) -> Result<PathBuf> {
    if session_id.is_empty() || session_id.contains('/') || session_id.contains('\\') {
        anyhow::bail!("invalid session id");
    }
    let dir = root.join(session_id);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn snapshot(root: &Path, session_id: &str, files: &[PathBuf], workspace: &Path) -> Result<u64> {
    let base = session_dir(root, session_id)?;
    let mut next = 0u64;
    while base.join(next.to_string()).exists() {
        next += 1;
    }
    let dir = base.join(next.to_string());
    fs::create_dir_all(&dir)?;
    for file in files {
        if !file.exists() {
            continue;
        }
        let rel = file.strip_prefix(workspace).unwrap_or(file);
        let target = dir.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(file, &target).with_context(|| format!("cannot snapshot {}", file.display()))?;
    }
    Ok(next)
}

pub fn latest(root: &Path, session_id: &str) -> Result<Option<PathBuf>> {
    let base = root.join(session_id);
    if !base.exists() {
        return Ok(None);
    }
    let mut dirs: Vec<u64> = fs::read_dir(&base)?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse().ok()))
        .collect();
    dirs.sort_unstable();
    Ok(dirs.last().map(|n| base.join(n.to_string())))
}

/// Build a unified diff between the latest snapshot of `target` and its
/// current contents. Used by the Web UI diff card after an edit tool runs.
pub fn diff_latest(
    root: &Path,
    session_id: &str,
    files: &[PathBuf],
    workspace: &Path,
    target: &str,
) -> Result<Option<(String, String)>> {
    let Some(dir) = latest(root, session_id)? else {
        return Ok(None);
    };
    let Some(file) = files.iter().find(|p| {
        let text = p.to_string_lossy();
        text.ends_with(target) || target.is_empty() || text.contains(target)
    }) else {
        return Ok(None);
    };
    let rel = file.strip_prefix(workspace).unwrap_or(file);
    let before_path = dir.join(rel);
    if !before_path.exists() || !file.exists() {
        return Ok(None);
    }
    let before = fs::read_to_string(&before_path)?;
    let after = fs::read_to_string(file)?;
    let diff = crate::diff::unified(&rel.to_string_lossy(), &before, &after);
    if diff.is_empty() {
        return Ok(None);
    }
    Ok(Some((rel.to_string_lossy().into_owned(), diff)))
}

pub fn restore(root: &Path, session_id: &str, workspace: &Path) -> Result<usize> {
    let Some(dir) = latest(root, session_id)? else {
        return Ok(0);
    };
    let mut restored = 0;
    for entry in walk(&dir)? {
        let rel = entry.strip_prefix(&dir)?;
        let target = workspace.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&entry, &target)?;
        restored += 1;
    }
    fs::remove_dir_all(dir)?;
    Ok(restored)
}

fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path)?);
        } else {
            out.push(path);
        }
    }
    Ok(out)
}
