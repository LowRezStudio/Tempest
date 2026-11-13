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
//!
//! Older builds (0.5x) store the file as an `FString` after each mip, empty
//! when the texture's default file applies, and the second table maps each
//! texture GUID to that default file name instead (`legacy`).

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
    /// Pre-GUID layout: files are named per texture, not by file GUID.
    pub legacy: bool,
    /// Legacy only: texture GUID → GUID of its default file.
    defaults: HashMap<Guid, Guid>,
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
        match MergedFileIndex::parse_modern(bytes) {
            Ok(idx) => Ok(idx),
            Err(e) => MergedFileIndex::parse_legacy(bytes).map_err(|_| e),
        }
    }

    fn parse_modern(bytes: &[u8]) -> Result<MergedFileIndex> {
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
        if r.remaining() != 0 {
            return Err(tempest_upk::io::Error::Format(format!("{} trailing bytes", r.remaining())));
        }
        idx.reindex();
        Ok(idx)
    }

    fn parse_legacy(bytes: &[u8]) -> Result<MergedFileIndex> {
        let mut r = Reader::new(bytes);
        let mut idx = MergedFileIndex { legacy: true, ..Default::default() };
        let n = r.i32()?.max(0);
        // Per mip: (mip, entry, explicit file name).
        let mut raw = Vec::new();
        for _ in 0..n {
            let g = r.guid()?;
            let mips = r.array(|r| {
                let mip = r.i32()?;
                let e = MergedEntry { size: r.i32()?, offset: r.i32()?, size_on_disk: r.i32()?, file: Guid::default() };
                Ok((mip, e, r.fstring()?))
            })?;
            raw.push((g, mips));
        }
        let m = r.i32()?.max(0);
        let mut names = HashMap::new();
        for _ in 0..m {
            let g = r.guid()?;
            names.insert(g, r.fstring()?);
        }
        if r.remaining() != 0 {
            return Err(tempest_upk::io::Error::Format(format!("{} trailing bytes", r.remaining())));
        }
        for (g, mips) in raw {
            let default = names.get(&g).map(|name| idx.add_file(name));
            if let Some(d) = default {
                idx.defaults.insert(g, d);
            }
            let mut out = Vec::with_capacity(mips.len());
            for (mip, mut e, name) in mips {
                e.file = if name.is_empty() { default.unwrap_or_default() } else { idx.add_file(&name) };
                out.push((mip, e));
            }
            idx.textures.push((g, out));
        }
        idx.reindex();
        Ok(idx)
    }

    fn legacy_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        let name = |g: Guid| self.file_name(g).unwrap_or_default();
        w.i32(self.textures.len() as i32);
        for (g, mips) in &self.textures {
            w.guid(g);
            let default = self.defaults.get(g).copied();
            w.array(mips, |w, (mip, e)| {
                w.i32(*mip);
                w.i32(e.size);
                w.i32(e.offset);
                w.i32(e.size_on_disk);
                w.fstring(if Some(e.file) == default { "" } else { name(e.file) });
            });
        }
        let defaults: Vec<_> = self.textures.iter().filter_map(|(g, _)| self.defaults.get(g).map(|d| (*g, *d))).collect();
        w.i32(defaults.len() as i32);
        for (g, d) in defaults {
            w.guid(&g);
            w.fstring(name(d));
        }
        w.into_inner()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        if self.legacy {
            return self.legacy_bytes();
        }
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
        if self.legacy {
            self.defaults.entry(g).or_insert(entry.file);
        }
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

    #[test]
    fn legacy_round_trip() {
        let mut idx = MergedFileIndex { legacy: true, ..Default::default() };
        let a = idx.add_file("Textures_PostLaunch");
        let b = idx.add_file("Textures");
        idx.set_entry("fx.a", 0, MergedEntry { size: 8, offset: 1, size_on_disk: 2, file: a });
        idx.set_entry("fx.a", 1, MergedEntry { size: 4, offset: 3, size_on_disk: 2, file: b });
        let bytes = idx.to_bytes();
        let back = MergedFileIndex::parse(&bytes).unwrap();
        assert!(back.legacy);
        assert_eq!(back.entry("FX.A", 0).unwrap().file, a);
        assert_eq!(back.entry("FX.A", 1).unwrap().file, b);
        assert_eq!(back.to_bytes(), bytes);
    }
}
