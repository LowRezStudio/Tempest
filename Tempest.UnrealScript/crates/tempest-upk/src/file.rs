//! Memory-mapped, lazily decompressed view of a package on disk.
//!
//! [`PackageFile::open`] maps the file, reads the summary and expands only the
//! chunk(s) that cover the header tables. Export payloads are read on demand
//! with [`PackageFile::export_data`]; each compressed chunk is expanded at
//! most once and cached, so browsing a 100 MiB package costs a few hundred
//! KiB until an object is actually looked at. The view is immutable and
//! `Sync`: threads can read different exports concurrently. For editing,
//! [`PackageFile::load`] materialises the eager [`Package`].

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use memmap2::Mmap;

use crate::flags::compression;
use crate::io::{Error, Reader, Result};
use crate::package::{read_compressed_chunk, CompressedChunk, Package, Summary};

/// A chunk's expansion, computed at most once.
type ChunkCache = OnceLock<std::result::Result<Vec<u8>, Error>>;

pub struct PackageFile {
    path: PathBuf,
    map: Mmap,
    /// Summary and header tables; `export_data` is empty.
    tables: Package,
    /// Chunks sorted by uncompressed offset, each with its expansion cache.
    chunks: Vec<(CompressedChunk, ChunkCache)>,
    obscured: bool,
}

impl PackageFile {
    pub fn open(path: impl AsRef<Path>) -> Result<PackageFile> {
        let path = path.as_ref().to_path_buf();
        let file = std::fs::File::open(&path)?;
        // SAFETY: the mapping is read-only and the file is not expected to be
        // modified while it is mapped; a concurrent writer would at worst
        // produce a parse error, not memory unsafety for the bytes we read.
        let map = unsafe { Mmap::map(&file)? };
        let summary = Summary::read(&mut Reader::new(&map))?;
        let obscured = summary.compression_flags & compression::OBSCURED != 0;
        let mut chunks: Vec<CompressedChunk> = summary.compressed_chunks.clone();
        chunks.sort_by_key(|c| c.uncompressed_offset);
        let ctype = summary.compression_flags & compression::TYPE_MASK;
        if !chunks.is_empty() && ctype != compression::LZO {
            return Err(Error::Compression(format!(
                "unsupported compression flags {:#x} (only LZO is implemented)",
                summary.compression_flags
            )));
        }
        let mut pf = PackageFile {
            path,
            map,
            tables: Package::default(),
            chunks: chunks.into_iter().map(|c| (c, OnceLock::new())).collect(),
            obscured,
        };
        let header_len = summary.total_header_size.max(0) as usize;
        let header = pf.read_range(0, header_len)?;
        let mut tables = Package::parse_header(&header, summary)?;
        tables.was_compressed = !pf.chunks.is_empty();
        pf.tables = tables;
        Ok(pf)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Summary and tables (no payloads).
    pub fn tables(&self) -> &Package {
        &self.tables
    }

    pub fn summary(&self) -> &Summary {
        &self.tables.summary
    }

    /// Size of the uncompressed file image.
    pub fn image_len(&self) -> usize {
        match self.chunks.last() {
            Some((c, _)) => ((c.uncompressed_offset + c.uncompressed_size) as usize).max(self.map.len()),
            None => self.map.len(),
        }
    }

    /// Payload of export `i`.
    pub fn export_data(&self, i: usize) -> Result<Cow<'_, [u8]>> {
        let e = self.tables.exports.get(i).ok_or_else(|| Error::Format(format!("no export {i}")))?;
        self.read_range(e.serial_offset.max(0) as usize, e.serial_size.max(0) as usize)
    }

    /// `len` bytes at `offset` of the uncompressed file image. Borrowed when
    /// the range lies in the raw file or inside one cached chunk.
    pub fn read_range(&self, offset: usize, len: usize) -> Result<Cow<'_, [u8]>> {
        let end = offset + len;
        if self.chunks.is_empty() {
            return self.map.get(offset..end).map(Cow::Borrowed).ok_or(Error::UnexpectedEof { at: offset, wanted: len, len: self.map.len() });
        }
        // Bytes before the first chunk (the summary) are stored raw.
        let first = self.chunks[0].0.uncompressed_offset as usize;
        if end <= first {
            return self.map.get(offset..end).map(Cow::Borrowed).ok_or(Error::UnexpectedEof { at: offset, wanted: len, len: self.map.len() });
        }
        let mut out: Option<Vec<u8>> = None;
        let mut pos = offset;
        if pos < first {
            let mut v = Vec::with_capacity(len);
            v.extend_from_slice(&self.map[pos..first]);
            out = Some(v);
            pos = first;
        }
        while pos < end {
            let ci = self.chunk_at(pos).ok_or(Error::UnexpectedEof { at: pos, wanted: end - pos, len: self.image_len() })?;
            let (c, _) = &self.chunks[ci];
            let data = self.chunk_data(ci)?;
            let c_start = c.uncompressed_offset as usize;
            let rel = pos - c_start;
            let take = (end - pos).min(data.len() - rel);
            if out.is_none() && take == end - pos {
                return Ok(Cow::Borrowed(&data[rel..rel + take]));
            }
            out.get_or_insert_with(|| Vec::with_capacity(len)).extend_from_slice(&data[rel..rel + take]);
            pos += take;
        }
        Ok(Cow::Owned(out.unwrap_or_default()))
    }

    fn chunk_at(&self, pos: usize) -> Option<usize> {
        let i = self.chunks.partition_point(|(c, _)| (c.uncompressed_offset as usize) <= pos);
        if i == 0 {
            return None;
        }
        let (c, _) = &self.chunks[i - 1];
        let start = c.uncompressed_offset as usize;
        (pos < start + c.uncompressed_size as usize).then_some(i - 1)
    }

    fn chunk_data(&self, i: usize) -> Result<&[u8]> {
        let (c, cell) = &self.chunks[i];
        let r = cell.get_or_init(|| {
            let mut r = Reader::new(&self.map);
            r.seek(c.compressed_offset as usize);
            let dec = read_compressed_chunk(&mut r, self.obscured)?;
            if dec.len() != c.uncompressed_size as usize {
                return Err(Error::Compression(format!("chunk size mismatch: summary {} vs header {}", c.uncompressed_size, dec.len())));
            }
            Ok(dec)
        });
        match r {
            Ok(v) => Ok(v),
            Err(e) => Err(e.clone()),
        }
    }

    /// Materialise the whole package (every payload decompressed).
    pub fn load(&self) -> Result<Package> {
        let mut pkg = self.tables.clone();
        let mut max_end = pkg.summary.total_header_size.max(0) as usize;
        for e in &pkg.exports {
            let start = e.serial_offset.max(0) as usize;
            let size = e.serial_size.max(0) as usize;
            pkg.export_data.push(self.read_range(start, size)?.into_owned());
            max_end = max_end.max(start + size);
        }
        let total = self.image_len();
        if max_end < total {
            pkg.trailing = self.read_range(max_end, total - max_end)?.into_owned();
        }
        Ok(pkg)
    }
}
