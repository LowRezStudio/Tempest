//! Unreal Engine 3 package (`.upk` / `.u`) reader and writer tuned for
//! Paladins 8.2 (engine build 10897, package version 893).
//!
//! The crate knows nothing about script objects; it exposes the summary, the
//! name/import/export tables and each export's raw payload. See the
//! `tempest-unrealscript` crate for object serialisation and bytecode.

pub mod file;
pub mod flags;
pub mod io;
pub mod lzo;
pub mod package;
pub mod tfc;

pub use file::PackageFile;
pub use io::{Error, FName, Guid, PackageIndex, Reader, Result, Writer};
pub use tfc::{Tfc, TfcWriter};
pub use package::{
    CompressedChunk, GenerationInfo, LevelGuids, NameEntry, ObjectExport, ObjectImport, Package, Summary,
    WriteOptions, read_compressed_chunk, write_compressed_chunk,
};
