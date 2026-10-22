//! Where a texture's mips actually live: inline in the package, or in a
//! `.tfc` of the cooked directory addressed through
//! `MergedFileIndexCache.bin`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tempest_upk::io::{Error, Result};
use tempest_upk::Tfc;

use crate::bulk::flags as bf;
use crate::mergedindex::MergedFileIndex;
use crate::texture::Mip;

pub struct TextureSource {
    pub dir: PathBuf,
    pub index: Option<MergedFileIndex>,
    tfcs: Mutex<HashMap<String, Arc<Tfc>>>,
}

/// Where one mip's bytes come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MipLocation {
    Inline,
    /// Stripped by the cooker (`BULKDATA_Unused`): no data anywhere.
    Unused,
    /// In `<file>.tfc` at `offset`, `size_on_disk` stored bytes.
    Cache { file: String, offset: i32, size_on_disk: i32, size: i32 },
    /// Flagged as stored elsewhere but not in the index.
    Missing,
}

impl TextureSource {
    /// Open the caches of a cooked directory (the index is optional: a
    /// directory without one only serves inline mips).
    pub fn open(dir: impl AsRef<Path>) -> Result<TextureSource> {
        let dir = dir.as_ref().to_path_buf();
        let idx_path = dir.join("MergedFileIndexCache.bin");
        let index = if idx_path.exists() { Some(MergedFileIndex::load(&idx_path)?) } else { None };
        Ok(TextureSource { dir, index, tfcs: Mutex::new(HashMap::new()) })
    }

    pub fn tfc(&self, name: &str) -> Result<Arc<Tfc>> {
        let key = name.to_ascii_lowercase();
        if let Some(t) = self.tfcs.lock().unwrap().get(&key) {
            return Ok(t.clone());
        }
        let path = self.dir.join(format!("{name}.tfc"));
        let t = Arc::new(Tfc::open(&path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?);
        self.tfcs.lock().unwrap().insert(key, t.clone());
        Ok(t)
    }

    /// `texture_path`: the object's path as the engine's `GetPathName`
    /// returns it (`Package.Group.Name`; for objects copied into seek-free
    /// packages the original package, which is the outermost `Package`
    /// export).
    pub fn locate(&self, texture_path: &str, mip_index: usize, mip: &Mip) -> MipLocation {
        let b = &mip.data;
        if b.is_unused() {
            return MipLocation::Unused;
        }
        if b.inline.is_some() {
            return MipLocation::Inline;
        }
        if b.flags & bf::MERGED_FILE != 0 {
            if let Some(idx) = &self.index {
                if let Some(e) = idx.entry(texture_path, mip_index as i32) {
                    if let Some(file) = idx.file_name(e.file) {
                        return MipLocation::Cache { file: file.to_string(), offset: e.offset, size_on_disk: e.size_on_disk, size: e.size };
                    }
                }
            }
        }
        MipLocation::Missing
    }

    /// The expanded pixel data of one mip, `None` when it is unused.
    pub fn mip_data(&self, texture_path: &str, mip_index: usize, mip: &Mip, obscured: bool) -> Result<Option<Vec<u8>>> {
        match self.locate(texture_path, mip_index, mip) {
            MipLocation::Unused => Ok(None),
            MipLocation::Inline => mip.data.inline_data(obscured),
            MipLocation::Missing => Err(Error::Format(format!("mip {mip_index} of {texture_path} is stored outside the package but not in MergedFileIndexCache.bin"))),
            MipLocation::Cache { file, offset, size_on_disk, size } => {
                let tfc = self.tfc(&file)?;
                let stored = tfc.raw(offset as u64, size_on_disk.max(0) as usize)?;
                if !mip.data.is_compressed() {
                    return Ok(Some(stored.to_vec()));
                }
                // The caches are not XOR-obscured like package chunks, but
                // be lenient: fall back to the obscured reading on failure.
                let data = match mip.data.expand(stored, false) {
                    Ok(d) => d,
                    Err(_) => mip.data.expand(stored, true)?,
                };
                if data.len() != size as usize {
                    return Err(Error::Format(format!("mip {mip_index} of {texture_path}: expanded {} bytes, index says {size}", data.len())));
                }
                Ok(Some(data))
            }
        }
    }
}
