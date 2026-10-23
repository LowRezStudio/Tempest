//! `MergedFileIndexCache.bin`: Hi-Rez's replacement for per-texture file
//! cache names. Every streamed mip of every texture is keyed by the MD5 of
//! the texture's upper-cased path name (`UObject::GetGuidFromPathName` →
//! `GetGuidFromString(path, bIgnoreCase=TRUE)`) and the mip index, and maps
//! to (uncompressed size, offset, size on disk, file GUID); a second table
//! names the `.tfc` file for each file GUID (`FMergedFileIndexCache`,
//! `FMergedFileIndexData`, read from the 8.2 server binary).
//!
//! On disk: `TMap<FGuid, TMap<INT, FMergedFileIndexData>>` then
//! `TMap<FGuid, FString>`, both as count + pairs.

use std::collections::HashMap;
use std::path::Path;

use md5::{Digest, Md5};
use tempest_upk::io::{Guid, Reader, Result, Writer};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MergedEntry {
    /// Uncompressed size of the mip.
    pub size: i32,
    /// Offset of the stored payload in the cache file.
    pub offset: i32,
    /// Stored (compressed) size.
    pub size_on_disk: i32,
    pub file: Guid,
}

#[derive(Clone, Debug, Default)]
pub struct MergedFileIndex {
    /// Texture path GUID → mip index → entry (insertion order kept).
    pub textures: Vec<(Guid, Vec<(i32, MergedEntry)>)>,
    pub files: Vec<(Guid, String)>,
    by_texture: HashMap<Guid, usize>,
    by_file: HashMap<Guid, usize>,
}

/// `UObject::GetGuidFromString(path, TRUE)`: MD5 of the ANSI upper-case path.
pub fn path_guid(path: &str) -> Guid {
    let digest = Md5::digest(path.to_ascii_uppercase().as_bytes());
    let w = |i: usize| u32::from_le_bytes([digest[i], digest[i + 1], digest[i + 2], digest[i + 3]]);
    Guid { a: w(0), b: w(4), c: w(8), d: w(12) }
}

impl MergedFileIndex {
    pub fn load(path: impl AsRef<Path>) -> Result<MergedFileIndex> {
        let bytes = std::fs::read(path)?;
        MergedFileIndex::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<MergedFileIndex> {
        let mut r = Reader::new(bytes);
        let mut idx = MergedFileIndex::default();
        let n = r.i32()?.max(0);
        for _ in 0..n {
            let g = r.guid()?;
            let mips = r.array(|r| {
                let mip = r.i32()?;
                let e = MergedEntry { size: r.i32()?, offset: r.i32()?, size_on_disk: r.i32()?, file: r.guid()? };
                Ok((mip, e))
            })?;
            idx.textures.push((g, mips));
        }
        let n = r.i32()?.max(0);
        for _ in 0..n {
            let g = r.guid()?;
            // Stored with its terminating NUL, as `FString` serialises.
            let s = r.fstring()?;
            idx.files.push((g, s));
        }
        idx.reindex();
        Ok(idx)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.i32(self.textures.len() as i32);
        for (g, mips) in &self.textures {
            w.guid(g);
            w.array(mips, |w, (mip, e)| {
                w.i32(*mip);
                w.i32(e.size);
                w.i32(e.offset);
                w.i32(e.size_on_disk);
                w.guid(&e.file);
            });
        }
        w.i32(self.files.len() as i32);
        for (g, s) in &self.files {
            w.guid(g);
            w.fstring(s);
        }
        w.into_inner()
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        std::fs::write(path, self.to_bytes())?;
        Ok(())
    }

    fn reindex(&mut self) {
        self.by_texture = self.textures.iter().enumerate().map(|(i, (g, _))| (*g, i)).collect();
        self.by_file = self.files.iter().enumerate().map(|(i, (g, _))| (*g, i)).collect();
    }

    /// Entries of a texture by its path (`Package.Group.Name`, the forced-
    /// export path for objects copied into seek-free packages).
    pub fn texture(&self, path: &str) -> Option<&[(i32, MergedEntry)]> {
        self.texture_by_guid(path_guid(path))
    }

    pub fn texture_by_guid(&self, g: Guid) -> Option<&[(i32, MergedEntry)]> {
        self.by_texture.get(&g).map(|&i| self.textures[i].1.as_slice())
    }

    pub fn entry(&self, path: &str, mip: i32) -> Option<&MergedEntry> {
        self.texture(path)?.iter().find(|(m, _)| *m == mip).map(|(_, e)| e)
    }

    /// Cache file name (without extension) for a file GUID.
    pub fn file_name(&self, g: Guid) -> Option<&str> {
        self.by_file.get(&g).map(|&i| self.files[i].1.as_str())
    }

    pub fn file_guid(&self, name: &str) -> Option<Guid> {
        self.files.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)).map(|(g, _)| *g)
    }

    /// Register a cache file name (returns its GUID, creating one when new).
    pub fn add_file(&mut self, name: &str) -> Guid {
        if let Some(g) = self.file_guid(name) {
            return g;
        }
        let g = path_guid(name);
        self.files.push((g, name.to_string()));
        self.reindex();
        g
    }

    /// Set (or replace) the entry of one mip of a texture.
    pub fn set_entry(&mut self, path: &str, mip: i32, entry: MergedEntry) {
        let g = path_guid(path);
        let i = match self.by_texture.get(&g) {
            Some(&i) => i,
            None => {
                self.textures.push((g, Vec::new()));
                self.reindex();
                self.textures.len() - 1
            }
        };
        let mips = &mut self.textures[i].1;
        match mips.iter_mut().find(|(m, _)| *m == mip) {
            Some((_, e)) => *e = entry,
            None => mips.push((mip, entry)),
        }
    }

    pub fn remove_texture(&mut self, path: &str) {
        let g = path_guid(path);
        if let Some(&i) = self.by_texture.get(&g) {
            self.textures.remove(i);
            self.reindex();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut idx = MergedFileIndex::default();
        let f = idx.add_file("Textures4");
        idx.set_entry("fx_atlas.textures.t_atlasshield_a", 1, MergedEntry { size: 32768, offset: 47913624, size_on_disk: 22659, file: f });
        let bytes = idx.to_bytes();
        let back = MergedFileIndex::parse(&bytes).unwrap();
        assert_eq!(back.entry("FX_ATLAS.TEXTURES.T_ATLASSHIELD_A", 1).unwrap().offset, 47913624);
        assert_eq!(back.file_name(f), Some("Textures4"));
        assert_eq!(back.to_bytes(), bytes);
    }
}
