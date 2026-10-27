//! Workspace files (`.twrk`): the unsaved state of a Studio session, so work
//! survives even when nothing was saved in place.
//!
//! A `.twrk` is a zip:
//! - `workspace.json`: format, mode, the game folder, the renderer's state
//!   (open package, selection, mode, history, layout…) as an opaque value,
//!   and one entry per stored file;
//! - `deltas/<n>.tdelta`: a file's changes against the file on disk, the
//!   same `.tdelta` the mod system uses (linked mode);
//! - `files/<n>`: a file's whole content, as Save would write it;
//! - `bases/<n>.json`: the Studio's recorded base of a file (portable mode),
//!   so a mod exported on another machine still gets deltas.
//!
//! **Linked** workspaces store deltas and refer to the game folder by path:
//! small, but they need the same game files to open. Script packages and
//! files whose delta cannot be made exactly are stored whole even then.
//! **Portable** workspaces store every changed file whole, plus the packages
//! opened from outside the game folder and the recorded bases: bigger, but
//! they open on another machine or game copy.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, Result};

pub const FORMAT: u64 = 1;

/// One file of a workspace, as listed in `workspace.json`.
pub struct Entry {
    pub meta: J,
    pub bytes: Vec<u8>,
}

/// Write a workspace: the manifest plus every entry's bytes under its
/// `entry` name. Written to a temp file and renamed, so a failed save
/// leaves the previous file intact.
pub fn write(out: &Path, manifest: &J, entries: &[Entry]) -> Result<()> {
    let tmp = out.with_extension("twrk.tmp");
    {
        let f = std::fs::File::create(&tmp)?;
        let mut z = zip::ZipWriter::new(f);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let err = |e: zip::result::ZipError| Error::Format(format!("workspace file: {e}"));
        z.start_file("workspace.json", deflate).map_err(err)?;
        z.write_all(&serde_json::to_vec_pretty(manifest).unwrap_or_default())?;
        for e in entries {
            let name = e.meta["entry"].as_str().unwrap_or_default();
            // Packages and marshals are compressed already: store them as is.
            let method = if name.starts_with("files/") { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated };
            z.start_file(name, zip::write::SimpleFileOptions::default().compression_method(method).large_file(e.bytes.len() as u64 > u32::MAX as u64)).map_err(err)?;
            z.write_all(&e.bytes)?;
        }
        z.finish().map_err(err)?;
    }
    std::fs::rename(&tmp, out)?;
    Ok(())
}

/// An open workspace file.
pub struct Reader {
    zip: zip::ZipArchive<std::fs::File>,
    pub manifest: J,
}

impl Reader {
    pub fn open(path: &Path) -> Result<Reader> {
        let f = std::fs::File::open(path)?;
        let mut zip = zip::ZipArchive::new(f).map_err(|e| Error::Format(format!("{} is not a workspace file: {e}", path.display())))?;
        let mut text = String::new();
        zip.by_name("workspace.json").map_err(|_| Error::Format("workspace.json is missing".into()))?.read_to_string(&mut text)?;
        let manifest: J = serde_json::from_str(&text).map_err(|e| Error::Format(format!("workspace.json: {e}")))?;
        if manifest["format"].as_u64().unwrap_or(0) > FORMAT {
            return Err(Error::Format("this workspace was saved by a newer Studio".into()));
        }
        Ok(Reader { zip, manifest })
    }

    pub fn bytes(&mut self, entry: &str) -> Result<Vec<u8>> {
        let mut f = self.zip.by_name(entry).map_err(|_| Error::Format(format!("{entry} is missing from the workspace")))?;
        let mut out = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut out)?;
        Ok(out)
    }
}

/// The game root of a cooked folder (`<root>/ChaosGame/CookedPCConsole`).
pub fn game_root(cooked: &Path) -> PathBuf {
    let mut p = cooked.to_path_buf();
    for _ in 0..2 {
        if let Some(parent) = p.parent() {
            p = parent.to_path_buf();
        }
    }
    p
}

/// `file` relative to the game root, with `/` separators, when it lies inside it.
pub fn relative(root: &Path, file: &Path) -> Option<String> {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let rel = file.strip_prefix(&root).ok()?;
    Some(rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/"))
}

/// Where a file of a workspace lives now: under the open game root when it
/// was inside the game folder, else at its saved absolute path.
pub fn resolve(root: &Path, meta: &J) -> PathBuf {
    match meta["rel"].as_str() {
        Some(rel) => rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c)),
        None => PathBuf::from(meta["path"].as_str().unwrap_or_default()),
    }
}

pub fn summary(manifest: &J) -> J {
    json!({
        "format": manifest["format"],
        "mode": manifest["mode"],
        "gameDir": manifest["gameDir"],
        "saved": manifest["saved"],
        "files": manifest["files"],
        "state": manifest["state"],
    })
}
