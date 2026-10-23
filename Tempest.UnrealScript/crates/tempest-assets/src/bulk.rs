//! `FUntypedBulkData` as stored in cooked packages
//! (`FUntypedBulkData::Serialize`, UnBulkData.cpp, with the Hi-Rez changes
//! read from the 8.2 server binary).
//!
//! Layout: `BulkDataFlags`, `ElementCount`, `BulkDataSizeOnDisk`,
//! `BulkDataOffsetInFile`, then — only when the data is neither stored in a
//! separate file nor flagged unused — `BulkDataSizeOnDisk` bytes of payload
//! (one compressed chunk when a `SerializeCompressed*` flag is set, raw
//! otherwise). The cooker leaves `BulkDataOffsetInFile` at -1 for inline
//! payloads (`if (!GIsCooking)` guards the write-back) and the loader
//! replaces it with the current position. Hi-Rez's `BULKDATA_MergedFile`
//! marks payloads that live in a texture file cache addressed through
//! `MergedFileIndexCache.bin` (see [`crate::mergedindex`]).

use tempest_upk::io::{Reader, Result, Writer};
use tempest_upk::{read_compressed_chunk, write_compressed_chunk};

pub mod flags {
    pub const STORE_IN_SEPARATE_FILE: u32 = 1 << 0;
    pub const SERIALIZE_COMPRESSED_ZLIB: u32 = 1 << 1;
    pub const FORCE_SINGLE_ELEMENT_SERIALIZATION: u32 = 1 << 2;
    pub const SINGLE_USE: u32 = 1 << 3;
    pub const SERIALIZE_COMPRESSED_LZO: u32 = 1 << 4;
    pub const UNUSED: u32 = 1 << 5;
    pub const STORE_ONLY_PAYLOAD: u32 = 1 << 6;
    pub const SERIALIZE_COMPRESSED_LZX: u32 = 1 << 7;
    /// Hi-Rez: the payload is in a merged texture file cache; the file and
    /// offset come from `MergedFileIndexCache.bin`, not from this header.
    pub const MERGED_FILE: u32 = 1 << 8;
    pub const SERIALIZE_COMPRESSED: u32 = SERIALIZE_COMPRESSED_ZLIB | SERIALIZE_COMPRESSED_LZO | SERIALIZE_COMPRESSED_LZX;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BulkData {
    pub flags: u32,
    pub element_count: i32,
    pub size_on_disk: i32,
    pub offset_in_file: i32,
    /// Payload bytes exactly as stored in the package (compressed chunk or
    /// raw), present only for inline data.
    pub inline: Option<Vec<u8>>,
}

impl BulkData {
    pub fn is_inline(&self) -> bool {
        self.flags & (flags::STORE_IN_SEPARATE_FILE | flags::UNUSED) == 0
    }

    pub fn is_separate(&self) -> bool {
        self.flags & flags::STORE_IN_SEPARATE_FILE != 0
    }

    pub fn is_unused(&self) -> bool {
        self.flags & flags::UNUSED != 0
    }

    pub fn is_compressed(&self) -> bool {
        self.flags & flags::SERIALIZE_COMPRESSED != 0
    }

    pub fn read(r: &mut Reader) -> Result<BulkData> {
        let flags = r.u32()?;
        let element_count = r.i32()?;
        let size_on_disk = r.i32()?;
        let offset_in_file = r.i32()?;
        let mut b = BulkData { flags, element_count, size_on_disk, offset_in_file, inline: None };
        if b.is_inline() {
            b.inline = Some(r.take(size_on_disk.max(0) as usize)?.to_vec());
        }
        Ok(b)
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(self.flags);
        w.i32(self.element_count);
        w.i32(self.size_on_disk);
        w.i32(self.offset_in_file);
        if let Some(d) = &self.inline {
            w.bytes(d);
        }
    }

    /// Expand a stored payload (inline or fetched from a cache) according to
    /// the compression flags.
    pub fn expand(&self, stored: &[u8], obscured: bool) -> Result<Vec<u8>> {
        if self.is_compressed() {
            read_compressed_chunk(&mut Reader::new(stored), obscured)
        } else {
            Ok(stored.to_vec())
        }
    }

    /// Inline payload, expanded.
    pub fn inline_data(&self, obscured: bool) -> Result<Option<Vec<u8>>> {
        match &self.inline {
            Some(d) => Ok(Some(self.expand(d, obscured)?)),
            None => Ok(None),
        }
    }

    /// Replace the payload with `data` stored inline (compressed with LZO
    /// when `compress`, like the cooker does for textures), element size 1.
    pub fn set_inline(&mut self, data: &[u8], compress: bool, obscure: bool) {
        self.flags &= !(flags::STORE_IN_SEPARATE_FILE | flags::UNUSED | flags::MERGED_FILE | flags::SERIALIZE_COMPRESSED);
        let stored = if compress {
            self.flags |= flags::SERIALIZE_COMPRESSED_LZO;
            write_compressed_chunk(data, obscure)
        } else {
            data.to_vec()
        };
        self.element_count = data.len() as i32;
        self.size_on_disk = stored.len() as i32;
        self.offset_in_file = -1;
        self.inline = Some(stored);
    }
}
