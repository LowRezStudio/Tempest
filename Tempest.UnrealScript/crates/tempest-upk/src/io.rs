//! Little-endian cursor helpers mirroring the subset of `FArchive` that
//! package headers and script objects use.

use std::fmt;

/// Package-relative object reference, as serialised in a UPK (`PACKAGE_INDEX`).
///
/// `0` is `None`, positive values are `exports[i - 1]`, negative values are
/// `imports[-i - 1]`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PackageIndex(pub i32);

impl PackageIndex {
    pub const NONE: PackageIndex = PackageIndex(0);

    pub fn export(i: usize) -> PackageIndex {
        PackageIndex(i as i32 + 1)
    }
    pub fn import(i: usize) -> PackageIndex {
        PackageIndex(-(i as i32) - 1)
    }
    pub fn is_null(self) -> bool {
        self.0 == 0
    }
    pub fn is_export(self) -> bool {
        self.0 > 0
    }
    pub fn is_import(self) -> bool {
        self.0 < 0
    }
    /// Zero-based export table index.
    pub fn export_index(self) -> Option<usize> {
        if self.0 > 0 {
            Some((self.0 - 1) as usize)
        } else {
            None
        }
    }
    /// Zero-based import table index.
    pub fn import_index(self) -> Option<usize> {
        if self.0 < 0 {
            Some((-self.0 - 1) as usize)
        } else {
            None
        }
    }
}

impl fmt::Debug for PackageIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            write!(f, "None")
        } else if self.0 > 0 {
            write!(f, "Export({})", self.0 - 1)
        } else {
            write!(f, "Import({})", -self.0 - 1)
        }
    }
}

/// Serialised `FName`: a name-table index and an instance number.
///
/// `number == 0` means the bare name; `number == n + 1` means `Name_n`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct FName {
    pub index: u32,
    pub number: u32,
}

impl FName {
    pub fn new(index: u32) -> FName {
        FName { index, number: 0 }
    }
}

/// `FGuid`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct Guid {
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub d: u32,
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08X}{:08X}{:08X}{:08X}", self.a, self.b, self.c, self.d)
    }
}

#[derive(Debug, Clone)]
pub enum Error {
    UnexpectedEof { at: usize, wanted: usize, len: usize },
    BadTag(u32),
    BadString(i32),
    Compression(String),
    Format(String),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnexpectedEof { at, wanted, len } => {
                write!(f, "unexpected end of data at {at} (wanted {wanted} bytes, have {len})")
            }
            Error::BadTag(t) => write!(f, "bad package file tag {t:#010x}"),
            Error::BadString(l) => write!(f, "bad FString length {l}"),
            Error::Compression(s) => write!(f, "decompression failed: {s}"),
            Error::Format(s) => write!(f, "{s}"),
            Error::Io(s) => write!(f, "io error: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Read cursor over a byte slice.
#[derive(Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }
    pub fn data(&self) -> &'a [u8] {
        self.data
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    pub fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            return Err(Error::UnexpectedEof { at: self.pos, wanted: n, len: self.data.len() });
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn peek_u8(&self) -> Result<u8> {
        self.data
            .get(self.pos)
            .copied()
            .ok_or(Error::UnexpectedEof { at: self.pos, wanted: 1, len: self.data.len() })
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    pub fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    pub fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    pub fn guid(&mut self) -> Result<Guid> {
        Ok(Guid { a: self.u32()?, b: self.u32()?, c: self.u32()?, d: self.u32()? })
    }
    pub fn fname(&mut self) -> Result<FName> {
        Ok(FName { index: self.u32()?, number: self.u32()? })
    }
    pub fn index(&mut self) -> Result<PackageIndex> {
        Ok(PackageIndex(self.i32()?))
    }

    /// `FString`: `INT` length (negative = UTF-16), characters, terminating NUL.
    pub fn fstring(&mut self) -> Result<String> {
        let len = self.i32()?;
        if len == 0 {
            return Ok(String::new());
        }
        if len > 0 {
            let bytes = self.take(len as usize)?;
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            Ok(bytes[..end].iter().map(|&b| b as char).collect())
        } else {
            let n = (-len) as usize;
            let bytes = self.take(n * 2)?;
            let mut units: Vec<u16> =
                bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            if let Some(end) = units.iter().position(|&u| u == 0) {
                units.truncate(end);
            }
            Ok(String::from_utf16_lossy(&units))
        }
    }

    pub fn array<T>(&mut self, mut f: impl FnMut(&mut Reader<'a>) -> Result<T>) -> Result<Vec<T>> {
        let n = self.i32()?;
        if n < 0 {
            return Err(Error::Format(format!("negative array count {n} at {}", self.pos - 4)));
        }
        let mut v = Vec::with_capacity((n as usize).min(1 << 20));
        for _ in 0..n {
            v.push(f(self)?);
        }
        Ok(v)
    }
}

/// Growable little-endian write buffer.
#[derive(Default, Clone)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer { buf: Vec::new() }
    }
    pub fn tell(&self) -> usize {
        self.buf.len()
    }
    pub fn into_inner(self) -> Vec<u8> {
        self.buf
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i16(&mut self, v: i16) {
        self.u16(v as u16)
    }
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.u32(v as u32)
    }
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits())
    }
    pub fn guid(&mut self, g: &Guid) {
        self.u32(g.a);
        self.u32(g.b);
        self.u32(g.c);
        self.u32(g.d);
    }
    pub fn fname(&mut self, n: FName) {
        self.u32(n.index);
        self.u32(n.number);
    }
    pub fn index(&mut self, i: PackageIndex) {
        self.i32(i.0)
    }
    /// Patch a previously written `u32` in place.
    pub fn patch_u32(&mut self, at: usize, v: u32) {
        self.buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn patch_i32(&mut self, at: usize, v: i32) {
        self.patch_u32(at, v as u32)
    }

    /// `FString`; pure-ASCII text is stored as ANSI, anything else as UTF-16.
    pub fn fstring(&mut self, s: &str) {
        if s.is_empty() {
            self.i32(0);
            return;
        }
        if s.chars().all(|c| (c as u32) < 0x80) {
            self.i32(s.len() as i32 + 1);
            self.buf.extend(s.bytes());
            self.u8(0);
        } else {
            let units: Vec<u16> = s.encode_utf16().collect();
            self.i32(-(units.len() as i32 + 1));
            for u in units {
                self.u16(u);
            }
            self.u16(0);
        }
    }

    pub fn array<T>(&mut self, items: &[T], mut f: impl FnMut(&mut Writer, &T)) {
        self.i32(items.len() as i32);
        for it in items {
            f(self, it);
        }
    }
}
