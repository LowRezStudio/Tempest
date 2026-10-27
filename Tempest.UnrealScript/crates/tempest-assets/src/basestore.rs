//! The Studio's record of a game file before it was first edited in place:
//! the digest (`delta::digest` / `mcts::delta::digest`) of the file as it
//! was on disk, kept under the OS cache folder, so a mod delta can be made
//! later without the original file. A few megabytes per file instead of a
//! copy of a 140 MB package.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tempest_upk::io::Result;

use crate::mcts::Tokens;
use crate::reflect::ClassDb;

/// `<cache dir>/tempest-studio` (Caches on macOS, LOCALAPPDATA on Windows, XDG cache elsewhere).
pub fn cache_dir() -> Option<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"))
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    }?;
    Some(base.join("tempest-studio"))
}

/// Case-insensitive, slash-normalised form of a path: how files are keyed.
pub fn key(path: &str) -> String {
    let p = path.replace('\\', "/");
    if cfg!(windows) || cfg!(target_os = "macos") { p.to_lowercase() } else { p }
}

fn hash64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h = h.wrapping_mul(0x100000001b3) ^ b as u64;
    }
    h
}

/// Where the digest of `file` lives.
pub fn digest_file(file: &Path) -> Option<PathBuf> {
    let canon = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    Some(cache_dir()?.join("base").join(format!("{:016x}.json", hash64(&key(&canon.to_string_lossy())))))
}

pub fn read(file: &Path) -> Option<Value> {
    let p = digest_file(file)?;
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

pub fn remove(file: &Path) -> bool {
    digest_file(file).map(|p| std::fs::remove_file(p).is_ok()).unwrap_or(false)
}

fn write(file: &Path, mut digest: Value) -> Result<()> {
    let Some(p) = digest_file(file) else { return Ok(()) };
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let meta = std::fs::metadata(file)?;
    digest["file"] = json!(file.to_string_lossy());
    digest["size"] = json!(meta.len());
    digest["recorded"] = json!(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    std::fs::write(p, serde_json::to_vec(&digest).unwrap_or_default())?;
    Ok(())
}

/// Record the package as it is on disk, unless a record exists. Returns
/// whether one was made now.
pub fn record_upk(db: &ClassDb, file: &Path) -> Result<bool> {
    if read(file).is_some() {
        return Ok(false);
    }
    let pkg = tempest_upk::PackageFile::open(file)?.load()?;
    write(file, crate::delta::digest(db, &pkg))?;
    Ok(true)
}

/// Record a marshal file (`assembly.dat`, `Lang_*.dat`) as it is on disk, unless a record exists.
pub fn record_mcts(tokens: &Tokens, file: &Path) -> Result<bool> {
    if read(file).is_some() {
        return Ok(false);
    }
    let m = crate::mcts::parse_obscured(&std::fs::read(file)?, tokens)?;
    write(file, crate::mcts::delta::digest(tokens, &m))?;
    Ok(true)
}

/// What the Studio shows: whether a base is recorded, and when.
pub fn status(file: &Path) -> Value {
    match read(file) {
        Some(d) => json!({ "recorded": true, "when": d["recorded"], "size": d["size"], "objects": d["exports"].as_object().map(|m| m.len()).or_else(|| d["tables"].as_object().map(|t| t.values().map(|v| v["rows"].as_object().map(|r| r.len()).unwrap_or(0)).sum())) }),
        None => json!({ "recorded": false }),
    }
}

/// The game-relative path of a file under a game folder
/// (`ChaosGame/CookedPCConsole/Foo.upk`): from the `ChaosGame` ancestor; a
/// file elsewhere is assumed to belong in the cooked folder.
pub fn game_relative(file: &Path) -> String {
    let comps: Vec<String> = file.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    if let Some(i) = comps.iter().rposition(|c| c.eq_ignore_ascii_case("ChaosGame")) {
        return comps[i..].join("/");
    }
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if name.to_ascii_lowercase().starts_with("lang_") && name.to_ascii_lowercase().ends_with(".dat") {
        let ext = file.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "INT".into());
        return format!("ChaosGame/Localization/{ext}/{name}");
    }
    format!("ChaosGame/CookedPCConsole/{name}")
}
