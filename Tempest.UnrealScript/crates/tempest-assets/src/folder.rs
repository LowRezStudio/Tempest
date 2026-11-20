//! The files of a workspace folder: its own and those of its subfolders, as
//! the game keeps localized packages and Wwise banks in `English(US)/`.
//! Hidden folders (`.tempest/` backups, `.git`) are left out.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How deep subfolders are read.
const DEPTH: usize = 6;

/// Every file under `dir` whose extension is one of `exts` (lower case): the
/// folder's own first, then each subfolder's, level by level, each sorted.
/// A name found twice is first found at the shallowest level.
pub fn files(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut level = vec![dir.to_path_buf()];
    for _ in 0..=DEPTH {
        let mut next = Vec::new();
        for d in &level {
            let mut found = Vec::new();
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let path = e.path();
                let Ok(kind) = e.file_type() else { continue };
                if kind.is_dir() {
                    if !e.file_name().to_string_lossy().starts_with('.') {
                        next.push(path);
                    }
                } else if path.extension().is_some_and(|x| exts.iter().any(|w| x.eq_ignore_ascii_case(w))) {
                    found.push(path);
                }
            }
            found.sort();
            out.extend(found);
        }
        if next.is_empty() {
            break;
        }
        next.sort();
        level = next;
    }
    out
}

/// The subfolder of `dir` that holds `file` (`English(US)`, `a/b`), or
/// `None` for the folder's own files.
pub fn subfolder(dir: &Path, file: &Path) -> Option<String> {
    let rel = file.parent()?.strip_prefix(dir).ok()?;
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// When `dir` and its subfolders last changed: a listing of them stays valid
/// while this does not change.
pub fn stamp(dir: &Path) -> Vec<Option<SystemTime>> {
    let mut out = Vec::new();
    let mut level = vec![dir.to_path_buf()];
    for _ in 0..=DEPTH {
        let mut next = Vec::new();
        for d in &level {
            out.push(std::fs::metadata(d).ok().and_then(|m| m.modified().ok()));
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.') {
                    next.push(e.path());
                }
            }
        }
        if next.is_empty() {
            break;
        }
        next.sort();
        level = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_files_first_then_subfolders() {
        let dir = std::env::temp_dir().join(format!("tempest-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["English(US)", ".tempest/v2/backup"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in ["B.upk", "a.PCK", "notes.txt", "English(US)/WW_VOX_A.upk", "English(US)/WWB_VOX_A.pck", ".tempest/v2/backup/B.upk"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let names = |exts: &[&str]| files(&dir, exts).iter().map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/")).collect::<Vec<_>>();
        assert_eq!(names(&["upk"]), ["B.upk", "English(US)/WW_VOX_A.upk"]);
        assert_eq!(names(&["pck", "upk"]), ["B.upk", "a.PCK", "English(US)/WWB_VOX_A.pck", "English(US)/WW_VOX_A.upk"]);
        assert_eq!(subfolder(&dir, &dir.join("English(US)/WW_VOX_A.upk")).as_deref(), Some("English(US)"));
        assert_eq!(subfolder(&dir, &dir.join("B.upk")), None);
        assert_eq!(stamp(&dir).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
