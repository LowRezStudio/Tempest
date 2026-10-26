//! Wwise file packages (`AKPK`, version 1): a header with a language map
//! and three tables (soundbanks, streamed media, external media), then the
//! files themselves at block-aligned offsets.
//!
//! Header: `"AKPK", header size, version (1), language map size, bank
//! table size, stream table size, externals table size`, the language map
//! (`count`, `(string offset, language id)` pairs, UTF-16 strings), the
//! bank and stream tables (`count` + `id, block size, size, offset in
//! blocks, language id` entries), the externals table (`count` + entries
//! with a 64-bit id). Sizes count from after the header-size field.

use tempest_upk::io::{Error, Result};

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub block: u32,
    pub language: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Package {
    pub version: u32,
    /// `(language id, name)`.
    pub languages: Vec<(u32, String)>,
    /// The language map bytes verbatim (rewritten as read).
    language_map: Vec<u8>,
    pub banks: Vec<Entry>,
    pub streams: Vec<Entry>,
    pub externals: Vec<Entry>,
}

/// A table row: `(id, block size, size, byte offset, language)`.
type Row = (u64, u32, u32, u64, u32);

fn u32_at(b: &[u8], o: usize) -> Result<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| Error::Format("AKPK: truncated".into()))
}

impl Package {
    pub fn parse(bytes: &[u8]) -> Result<Package> {
        if bytes.len() < 28 || &bytes[0..4] != b"AKPK" {
            return Err(Error::Format("not a Wwise file package (AKPK)".into()));
        }
        let header_size = u32_at(bytes, 4)? as usize;
        let version = u32_at(bytes, 8)?;
        if version != 1 {
            return Err(Error::Format(format!("AKPK version {version} is not supported")));
        }
        let lang_size = u32_at(bytes, 12)? as usize;
        let bank_size = u32_at(bytes, 16)? as usize;
        let stream_size = u32_at(bytes, 20)? as usize;
        let ext_size = u32_at(bytes, 24)? as usize;
        let mut pos = 28;
        let language_map = bytes.get(pos..pos + lang_size).ok_or_else(|| Error::Format("AKPK: truncated language map".into()))?.to_vec();
        let mut languages = Vec::new();
        if lang_size >= 4 {
            let n = u32_at(&language_map, 0)? as usize;
            for i in 0..n {
                let off = u32_at(&language_map, 4 + i * 8)? as usize;
                let id = u32_at(&language_map, 8 + i * 8)?;
                let mut name = String::new();
                let mut p = off;
                while p + 1 < language_map.len() {
                    let c = u16::from_le_bytes([language_map[p], language_map[p + 1]]);
                    if c == 0 {
                        break;
                    }
                    name.push(char::from_u32(c as u32).unwrap_or('?'));
                    p += 2;
                }
                languages.push((id, name));
            }
        }
        pos += lang_size;
        let table = |pos: usize, size: usize, wide: bool| -> Result<Vec<Row>> {
            let t = bytes.get(pos..pos + size).ok_or_else(|| Error::Format("AKPK: truncated table".into()))?;
            if size < 4 {
                return Ok(Vec::new());
            }
            let n = u32_at(t, 0)? as usize;
            let stride = if wide { 24 } else { 20 };
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let e = 4 + i * stride;
                let (id, e) = if wide { (u64::from_le_bytes(t.get(e..e + 8).ok_or_else(|| Error::Format("AKPK: truncated table".into()))?.try_into().unwrap()), e + 8) } else { (u32_at(t, e)? as u64, e + 4) };
                let block = u32_at(t, e)?;
                let size = u32_at(t, e + 4)?;
                let offset = u32_at(t, e + 8)? as u64 * block.max(1) as u64;
                let lang = u32_at(t, e + 12)?;
                out.push((id, block, size, offset, lang));
            }
            Ok(out)
        };
        let banks = table(pos, bank_size, false)?;
        pos += bank_size;
        let streams = table(pos, stream_size, false)?;
        pos += stream_size;
        let externals = table(pos, ext_size, true)?;
        let _ = header_size;
        let load = |rows: Vec<Row>| -> Result<Vec<Entry>> {
            rows.into_iter()
                .map(|(id, block, size, offset, language)| {
                    let data = bytes.get(offset as usize..offset as usize + size as usize).ok_or_else(|| Error::Format(format!("AKPK: entry {id:08x} outside the file")))?.to_vec();
                    Ok(Entry { id, block, language, data })
                })
                .collect()
        };
        Ok(Package { version, languages, language_map, banks: load(banks)?, streams: load(streams)?, externals: load(externals)? })
    }

    /// Serialise with the tables recomputed (entries laid out in table
    /// order, each at a multiple of its block size).
    pub fn to_bytes(&self) -> Vec<u8> {
        let bank_table = 4 + 20 * self.banks.len();
        let stream_table = 4 + 20 * self.streams.len();
        let ext_table = 4 + 24 * self.externals.len();
        let header_size = 20 + self.language_map.len() + bank_table + stream_table + ext_table;
        let mut out = Vec::new();
        out.extend_from_slice(b"AKPK");
        out.extend_from_slice(&(header_size as u32).to_le_bytes());
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&(self.language_map.len() as u32).to_le_bytes());
        out.extend_from_slice(&(bank_table as u32).to_le_bytes());
        out.extend_from_slice(&(stream_table as u32).to_le_bytes());
        out.extend_from_slice(&(ext_table as u32).to_le_bytes());
        out.extend_from_slice(&self.language_map);
        // Data follows the header; compute the offsets first.
        let mut cursor = 8 + header_size;
        let mut place = |entries: &[Entry]| -> Vec<u64> {
            entries
                .iter()
                .map(|e| {
                    let block = e.block.max(1) as usize;
                    cursor = cursor.div_ceil(block) * block;
                    let at = cursor;
                    cursor += e.data.len();
                    at as u64
                })
                .collect()
        };
        let bank_offsets = place(&self.banks);
        let stream_offsets = place(&self.streams);
        let ext_offsets = place(&self.externals);
        let total = cursor;
        let mut write_table = |entries: &[Entry], offsets: &[u64], wide: bool| {
            out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
            for (e, off) in entries.iter().zip(offsets) {
                if wide {
                    out.extend_from_slice(&e.id.to_le_bytes());
                } else {
                    out.extend_from_slice(&(e.id as u32).to_le_bytes());
                }
                out.extend_from_slice(&e.block.to_le_bytes());
                out.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
                out.extend_from_slice(&((off / e.block.max(1) as u64) as u32).to_le_bytes());
                out.extend_from_slice(&e.language.to_le_bytes());
            }
        };
        write_table(&self.banks, &bank_offsets, false);
        write_table(&self.streams, &stream_offsets, false);
        write_table(&self.externals, &ext_offsets, true);
        out.resize(total, 0);
        for (e, off) in self.banks.iter().zip(&bank_offsets).chain(self.streams.iter().zip(&stream_offsets)).chain(self.externals.iter().zip(&ext_offsets)) {
            out[*off as usize..*off as usize + e.data.len()].copy_from_slice(&e.data);
        }
        out
    }

    pub fn language_name(&self, id: u32) -> String {
        self.languages.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone()).unwrap_or_else(|| id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut lang = Vec::new();
        lang.extend_from_slice(&1u32.to_le_bytes());
        lang.extend_from_slice(&12u32.to_le_bytes());
        lang.extend_from_slice(&0u32.to_le_bytes());
        for c in "sfx\0".encode_utf16() {
            lang.extend_from_slice(&c.to_le_bytes());
        }
        let p = Package { version: 1, languages: vec![(0, "sfx".into())], language_map: lang, banks: vec![Entry { id: 7, block: 1, language: 0, data: vec![1, 2, 3] }], streams: vec![Entry { id: 9, block: 16, language: 0, data: vec![4; 20] }], externals: vec![] };
        let bytes = p.to_bytes();
        let back = Package::parse(&bytes).unwrap();
        assert_eq!(back.languages, p.languages);
        assert_eq!(back.banks.len(), 1);
        assert_eq!(back.banks[0].data, vec![1, 2, 3]);
        assert_eq!(back.streams[0].data, vec![4; 20]);
        assert_eq!(back.streams[0].id, 9);
        assert_eq!(back.to_bytes(), bytes);
    }
}
