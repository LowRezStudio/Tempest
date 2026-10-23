//! Texture file caches (`*.tfc`): flat files holding bulk-data payloads that
//! textures reference by `BulkDataOffsetInFile`. A payload is either raw or
//! one compressed chunk (`BULKDATA_SerializeCompressedLZO`) in the same
//! layout as package chunks; the Hi-Rez cooker XOR-obscures those too when
//! the owning package is obscured.
//!
//! Hi-Rez ships every cache twice: `Name.tfc` and `NamePATCH.tfc`. The
//! texture names the cache; which file a mip lives in is decided by the
//! loader (see the texture layer), so this module only knows files.

use std::borrow::Cow;
use std::io::Write;
use std::path::{Path, PathBuf};

use memmap2::Mmap;

use crate::io::{Error, Reader, Result};
use crate::package::{read_compressed_chunk, write_compressed_chunk};

pub struct Tfc {
    path: PathBuf,
    map: Mmap,
}

impl Tfc {
    pub fn open(path: impl AsRef<Path>) -> Result<Tfc> {
        let path = path.as_ref().to_path_buf();
        let file = std::fs::File::open(&path)?;
        // SAFETY: read-only mapping of a file we do not write through this handle.
        let map = unsafe { Mmap::map(&file)? };
        Ok(Tfc { path, map })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Raw bytes stored at `offset`.
    pub fn raw(&self, offset: u64, size: usize) -> Result<&[u8]> {
        let start = usize::try_from(offset).map_err(|_| Error::Format(format!("bad tfc offset {offset}")))?;
        self.map.get(start..start + size).ok_or(Error::UnexpectedEof { at: start, wanted: size, len: self.map.len() })
    }

    /// Payload at `offset`: expanded when `compressed`, else the raw bytes.
    pub fn payload(&self, offset: u64, size_on_disk: usize, compressed: bool, obscured: bool) -> Result<Cow<'_, [u8]>> {
        let raw = self.raw(offset, size_on_disk)?;
        if !compressed {
            return Ok(Cow::Borrowed(raw));
        }
        let mut r = Reader::new(raw);
        Ok(Cow::Owned(read_compressed_chunk(&mut r, obscured)?))
    }
}

/// Appends payloads to a cache (creating it when missing) and reports where
/// they landed, for textures that are being re-pointed at new mips.
pub struct TfcWriter {
    file: std::fs::File,
    len: u64,
}

impl TfcWriter {
    pub fn open(path: impl AsRef<Path>) -> Result<TfcWriter> {
        let file = std::fs::OpenOptions::new().append(true).create(true).open(path)?;
        let len = file.metadata()?.len();
        Ok(TfcWriter { file, len })
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Append `bytes` as-is; returns the offset they were written at.
    pub fn append_raw(&mut self, bytes: &[u8]) -> Result<u64> {
        let at = self.len;
        self.file.write_all(bytes)?;
        self.len += bytes.len() as u64;
        Ok(at)
    }

    /// Append `payload` as one compressed chunk; returns `(offset, size on disk)`.
    pub fn append_compressed(&mut self, payload: &[u8], obscure: bool) -> Result<(u64, usize)> {
        let chunk = write_compressed_chunk(payload, obscure);
        let at = self.append_raw(&chunk)?;
        Ok((at, chunk.len()))
    }
}
