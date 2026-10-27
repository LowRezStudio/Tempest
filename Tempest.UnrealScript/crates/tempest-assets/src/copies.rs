//! The Studio's record of exported copies ("Save as"): which game file each
//! copy was made from, so the mod cooker can offer the copy as a modified
//! version of that file (a delta against its original) across sessions.
//! Kept as JSON under the OS cache folder, next to the base records.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

fn file() -> Option<PathBuf> {
    Some(crate::basestore::cache_dir()?.join("copies.json"))
}

fn load() -> Vec<Value> {
    file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice::<Vec<Value>>(&b).ok()).unwrap_or_default()
}

fn canon(p: &Path) -> String {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_string()
}

/// Remember that `copy` was saved from `original` (replacing an older entry for the same copy).
pub fn add(original: &Path, copy: &Path) {
    let Some(f) = file() else { return };
    let copy_s = canon(copy);
    let mut list: Vec<Value> = load().into_iter().filter(|e| e["copy"].as_str() != Some(copy_s.as_str())).collect();
    let saved = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    list.push(json!({ "original": canon(original), "copy": copy_s, "saved": saved }));
    if let Some(parent) = f.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(f, serde_json::to_vec_pretty(&list).unwrap_or_default());
}

/// Forget a copy (the cooker's "remove from the list").
pub fn remove(copy: &Path) {
    let Some(f) = file() else { return };
    let copy_s = canon(copy);
    let list: Vec<Value> = load().into_iter().filter(|e| e["copy"].as_str() != Some(copy_s.as_str())).collect();
    let _ = std::fs::write(f, serde_json::to_vec_pretty(&list).unwrap_or_default());
}

/// Copies whose file still exists: `(original, copy, saved)`.
pub fn all() -> Vec<(PathBuf, PathBuf, u64)> {
    load()
        .into_iter()
        .filter_map(|e| {
            let copy = PathBuf::from(e["copy"].as_str()?);
            copy.exists().then(|| (PathBuf::from(e["original"].as_str().unwrap_or_default()), copy, e["saved"].as_u64().unwrap_or(0)))
        })
        .collect()
}
