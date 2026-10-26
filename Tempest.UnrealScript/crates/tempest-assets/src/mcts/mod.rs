//! MCTS marshals: the `CMarshal` wire format Hi-Rez's services speak, and
//! the format of `assembly.dat`, `Localization/<EXT>/Lang_<EXT>.dat` and
//! `Localization/WordFilter.dat` (a marshal XOR'd with 0x2A, as
//! `CPackage::ReadFromFile(path, bObscure)` reads it).
//!
//! Stream: `u8 flags`, `u32 function` (FNV-1 of the function name; 0 for
//! the localization files), then a row: `u16 entry count` (0xFFFF → `u32`)
//! and entries. Entry header `u16`: bits 15..12 the kind, bits 5..0 the
//! parameter.
//!
//! | kind | layout |
//! |------|--------|
//! | 1..4 | `param` field tokens (`u16` each) then `param` values of 1/2/4/8 bytes |
//! | 5    | token, `u16` length (bit 15: UTF-16), text (legacy) |
//! | 6    | token, row set: `u16` row count (0xFFFF → `u32`), rows |
//! | 7    | token, 16 GUID bytes |
//! | 8    | token, `u16` size, bytes |
//! | 9    | token, `param` UTF-16 units (≤ 3, legacy) |
//! | 10   | token, `u16` unit count, units; `param` is the encoding (1 UTF-8, 2/3 UTF-32, 4 UTF-16, 5 one byte per character) |
//!
//! Writing follows `CMarshalRow::StoreEntries` exactly, so an unchanged
//! file is rewritten byte for byte: per row, integers go into four width
//! caches (8/16/32/64 bits) flushed when 32 entries are gathered and at the
//! end of the row, in width order; the width comes from the field's type
//! ([`Kind`]) and, for ints and words, from the value. Strings, row sets,
//! GUIDs and blobs are written where they occur. Every field is described
//! by the game's token table ([`Tokens`]): the token is the index of the
//! field in the `MFIELD_TOKEN` enum, and the loader rejects an entry whose
//! kind does not match the field's type class.

pub mod create;
pub mod doc;
pub mod workspace;

use std::collections::HashMap;
use std::io::Read;

use tempest_upk::io::{Error, Result};

use crate::wwise::fnv::fnv1_32;

/// The obscuring byte of the `.dat` files.
pub const OBSCURE: u8 = 0x2A;

/// Field type (`MFIELD_TYPES`), the low byte of the type word of the token table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Unknown = 0,
    Byte = 2,
    Int = 3,
    Word = 4,
    Dword = 5,
    Float = 6,
    Double = 7,
    Qword = 8,
    NetId = 9,
    DateTime = 10,
    String = 12,
    RowSet = 13,
    Guid = 14,
    Blob = 15,
}

impl Kind {
    pub fn from_u8(v: u8) -> Kind {
        match v {
            2 => Kind::Byte,
            3 => Kind::Int,
            4 => Kind::Word,
            5 => Kind::Dword,
            6 => Kind::Float,
            7 => Kind::Double,
            8 => Kind::Qword,
            9 => Kind::NetId,
            10 => Kind::DateTime,
            12 => Kind::String,
            13 => Kind::RowSet,
            14 => Kind::Guid,
            15 => Kind::Blob,
            _ => Kind::Unknown,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Unknown => "unknown",
            Kind::Byte => "byte",
            Kind::Int => "int",
            Kind::Word => "word",
            Kind::Dword => "dword",
            Kind::Float => "float",
            Kind::Double => "double",
            Kind::Qword => "qword",
            Kind::NetId => "netid",
            Kind::DateTime => "datetime",
            Kind::String => "string",
            Kind::RowSet => "rowset",
            Kind::Guid => "guid",
            Kind::Blob => "blob",
        }
    }

    /// A numeric kind (what the loader accepts for entry kinds 1..4).
    pub fn is_numeric(self) -> bool {
        matches!(self, Kind::Byte | Kind::Int | Kind::Word | Kind::Dword | Kind::Float | Kind::Double | Kind::Qword | Kind::NetId | Kind::DateTime | Kind::Unknown)
    }
}

/// One field of the token table.
#[derive(Clone, Debug)]
pub struct Field {
    pub token: u16,
    pub name: String,
    pub kind: Kind,
    /// The type word as extracted (high byte: extra class of `NetId` fields).
    pub raw_type: u16,
}

/// The game's field and function tables.
#[derive(Clone, Debug, Default)]
pub struct Tokens {
    fields: Vec<Option<Field>>,
    by_name: HashMap<String, u16>,
    functions: HashMap<u32, String>,
    /// Where the tables came from, for display.
    pub source: String,
}

impl Tokens {
    /// The tables extracted from the 8.1 client's `MctsInterface.dll`.
    pub fn builtin() -> Tokens {
        let mut t = Tokens::default();
        t.load_fields(include_bytes!("../../data/mcts/fields.dat"));
        t.load_functions(include_bytes!("../../data/mcts/functions.dat"));
        t.source = "built-in (Paladins 8.1)".into();
        t
    }

    /// The tables of a game folder: extracted from its `MctsInterface.dll`
    /// when one is found next to the cooked folder, else the built-in ones.
    pub fn for_dir(cooked: &std::path::Path) -> Tokens {
        for up in [cooked.join("../.."), cooked.join("..")] {
            for sub in ["Binaries/Win64/MctsInterface.dll", "Binaries/Win32/MctsInterface.dll", "Binaries/Linux64/libMctsInterface.so"] {
                let p = up.join(sub);
                if let Ok(bytes) = std::fs::read(&p) {
                    if let Some(mut t) = Tokens::from_binary(&bytes) {
                        t.source = p.canonicalize().unwrap_or(p).to_string_lossy().to_string();
                        return t;
                    }
                }
            }
        }
        Tokens::builtin()
    }

    /// Extract the tables from a game binary: the client DLL embeds them as
    /// gzip blobs (fields with their types after a `VERSION` entry,
    /// functions after `HELLO`). `None` when the binary holds none (the
    /// Linux builds only carry the enum in DWARF).
    pub fn from_binary(bytes: &[u8]) -> Option<Tokens> {
        let mut fields: Option<Vec<u8>> = None;
        let mut functions: Option<Vec<u8>> = None;
        let mut at = 0;
        while at + 3 <= bytes.len() {
            let Some(i) = bytes[at..].windows(3).position(|w| w == [0x1f, 0x8b, 0x08]) else { break };
            let start = at + i;
            at = start + 1;
            let mut out = Vec::new();
            let mut dec = flate2::read::GzDecoder::new(&bytes[start..]);
            if dec.read_to_end(&mut out).is_err() || out.len() < 16 {
                continue;
            }
            if functions.is_none() && out.windows(5).any(|w| w == b"HELLO") {
                functions = Some(out);
            } else if fields.is_none() && out.windows(7).any(|w| w == b"VERSION") {
                fields = Some(out);
            }
            if fields.is_some() && functions.is_some() {
                break;
            }
        }
        let fields = fields?;
        let mut t = Tokens::default();
        // Fields: {u16 wire token, u16 type, name\0}; the position in the
        // table is the enum value the files use.
        let mut pos = 0;
        let mut token = 0u16;
        while pos + 4 < fields.len() {
            let ty = u16::from_be_bytes([fields[pos + 2], fields[pos + 3]]);
            let Some(end) = fields[pos + 4..].iter().position(|&b| b == 0) else { break };
            let name = String::from_utf8_lossy(&fields[pos + 4..pos + 4 + end]).to_string();
            t.insert_field(token, name, ty);
            token = token.wrapping_add(1);
            pos += 4 + end + 1;
        }
        if let Some(blob) = functions {
            // Modern layout: {u16, u16, u32 FNV-1 BE, name\0}; legacy: {u16, u16, name\0}.
            let modern = blob.len() > 12 && {
                let end = blob[8..].iter().position(|&b| b == 0).unwrap_or(0);
                fnv1_32(&blob[8..8 + end]) == u32::from_be_bytes([blob[4], blob[5], blob[6], blob[7]])
            };
            let skip = if modern { 8 } else { 4 };
            let mut pos = 0;
            while pos + skip < blob.len() {
                let Some(end) = blob[pos + skip..].iter().position(|&b| b == 0) else { break };
                let name = String::from_utf8_lossy(&blob[pos + skip..pos + skip + end]).to_string();
                t.functions.insert(fnv1_32(name.as_bytes()), name);
                pos += skip + end + 1;
            }
        }
        if t.fields.len() < 100 {
            return None;
        }
        Some(t)
    }

    /// Parse an extracted `fields.dat` (`{u16 token BE, u16 type BE, name\0}`).
    fn load_fields(&mut self, d: &[u8]) {
        let mut pos = 0;
        while pos + 4 < d.len() {
            let token = u16::from_be_bytes([d[pos], d[pos + 1]]);
            let ty = u16::from_be_bytes([d[pos + 2], d[pos + 3]]);
            let Some(end) = d[pos + 4..].iter().position(|&b| b == 0) else { break };
            let name = String::from_utf8_lossy(&d[pos + 4..pos + 4 + end]).to_string();
            self.insert_field(token, name, ty);
            pos += 4 + end + 1;
        }
    }

    /// Parse an extracted `functions.dat` (`{u16, u16, u32 hash BE, name\0}`).
    fn load_functions(&mut self, d: &[u8]) {
        let mut pos = 0;
        while pos + 8 < d.len() {
            let hash = u32::from_be_bytes([d[pos + 4], d[pos + 5], d[pos + 6], d[pos + 7]]);
            let Some(end) = d[pos + 8..].iter().position(|&b| b == 0) else { break };
            let name = String::from_utf8_lossy(&d[pos + 8..pos + 8 + end]).to_string();
            self.functions.insert(hash, name);
            pos += 8 + end + 1;
        }
    }

    fn insert_field(&mut self, token: u16, name: String, raw_type: u16) {
        let i = token as usize;
        if self.fields.len() <= i {
            self.fields.resize(i + 1, None);
        }
        self.by_name.insert(name.clone(), token);
        self.fields[i] = Some(Field { token, name, kind: Kind::from_u8(raw_type as u8), raw_type });
    }

    pub fn field(&self, token: u16) -> Option<&Field> {
        self.fields.get(token as usize).and_then(|f| f.as_ref())
    }

    pub fn kind(&self, token: u16) -> Kind {
        self.field(token).map(|f| f.kind).unwrap_or(Kind::Unknown)
    }

    /// Name of a token (`#1234` for an unknown one).
    pub fn name(&self, token: u16) -> String {
        self.field(token).map(|f| f.name.clone()).unwrap_or_else(|| format!("#{token}"))
    }

    pub fn token(&self, name: &str) -> Option<u16> {
        self.by_name.get(name).copied().or_else(|| name.strip_prefix('#').and_then(|n| n.parse().ok()))
    }

    pub fn function_name(&self, hash: u32) -> Option<&str> {
        self.functions.get(&hash).map(|s| s.as_str())
    }

    pub fn function(&self, name: &str) -> u32 {
        fnv1_32(name.as_bytes())
    }

    pub fn field_count(&self) -> usize {
        self.fields.iter().filter(|f| f.is_some()).count()
    }

    pub fn function_count(&self) -> usize {
        self.functions.len()
    }
}

// ----------------------------------------------------------------------------
// Values
// ----------------------------------------------------------------------------

/// A field value. Numbers keep their raw bits (`Float` is the `f32` bit
/// pattern, `DateTime` seconds since 1970) so that nothing is lost.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Num(u64),
    Str(String),
    Rows(Vec<Row>),
    Guid([u8; 16]),
    Blob(Vec<u8>),
}

/// One row: its entries in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub entries: Vec<(u16, Value)>,
}

impl Row {
    pub fn get(&self, token: u16) -> Option<&Value> {
        self.entries.iter().find(|(t, _)| *t == token).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, token: u16) -> Option<&mut Value> {
        self.entries.iter_mut().find(|(t, _)| *t == token).map(|(_, v)| v)
    }

    pub fn num(&self, token: u16) -> Option<u64> {
        match self.get(token) {
            Some(Value::Num(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn str(&self, token: u16) -> Option<&str> {
        match self.get(token) {
            Some(Value::Str(s)) => Some(s),
            _ => None,
        }
    }

    pub fn rows(&self, token: u16) -> Option<&Vec<Row>> {
        match self.get(token) {
            Some(Value::Rows(r)) => Some(r),
            _ => None,
        }
    }

    pub fn rows_mut(&mut self, token: u16) -> Option<&mut Vec<Row>> {
        match self.get_mut(token) {
            Some(Value::Rows(r)) => Some(r),
            _ => None,
        }
    }

    /// Set a field, keeping its place when it exists, else appending.
    pub fn set(&mut self, token: u16, value: Value) {
        match self.get_mut(token) {
            Some(v) => *v = value,
            None => self.entries.push((token, value)),
        }
    }

    pub fn remove(&mut self, token: u16) -> Option<Value> {
        let i = self.entries.iter().position(|(t, _)| *t == token)?;
        Some(self.entries.remove(i).1)
    }
}

/// A whole marshal.
#[derive(Clone, Debug, PartialEq)]
pub struct Marshal {
    pub flags: u8,
    pub function: u32,
    pub root: Row,
    /// String encoding written in kind-10 headers (the package's
    /// `m_byEncoding`; the game's files use 4, UTF-16).
    pub encoding: u8,
}

// ----------------------------------------------------------------------------
// Reading
// ----------------------------------------------------------------------------

struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
    tokens: &'a Tokens,
    encoding: Option<u8>,
}

impl<'a> Reader<'a> {
    fn need(&self, n: usize) -> Result<()> {
        if self.pos + n > self.d.len() {
            return Err(Error::Format(format!("marshal truncated at {} (wanted {n} bytes, have {})", self.pos, self.d.len() - self.pos)));
        }
        Ok(())
    }

    fn u8(&mut self) -> Result<u8> {
        self.need(1)?;
        self.pos += 1;
        Ok(self.d[self.pos - 1])
    }

    fn u16(&mut self) -> Result<u16> {
        self.need(2)?;
        let v = u16::from_le_bytes([self.d[self.pos], self.d[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn u32(&mut self) -> Result<u32> {
        self.need(4)?;
        let v = u32::from_le_bytes(self.d[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    fn u64(&mut self) -> Result<u64> {
        self.need(8)?;
        let v = u64::from_le_bytes(self.d[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.need(n)?;
        let s = &self.d[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn count(&mut self) -> Result<usize> {
        let c = self.u16()?;
        Ok(if c == 0xFFFF { self.u32()? as usize } else { c as usize })
    }

    fn row(&mut self) -> Result<Row> {
        let count = self.count()?;
        let mut row = Row::default();
        let mut read = 0;
        while read < count {
            read += self.entry(&mut row)?;
        }
        if read != count {
            return Err(Error::Format(format!("row at {} declares {count} entries but holds {read}", self.pos)));
        }
        Ok(row)
    }

    /// One entry header; returns how many fields it carried.
    fn entry(&mut self, row: &mut Row) -> Result<usize> {
        let at = self.pos;
        let h = self.u16()?;
        let kind = h >> 12;
        let param = (h & 0x3F) as usize;
        match kind {
            1..=4 => {
                if param > 32 {
                    return Err(Error::Format(format!("integer batch of {param} at {at}")));
                }
                let mut tokens = Vec::with_capacity(param);
                for _ in 0..param {
                    tokens.push(self.u16()?);
                }
                for t in tokens {
                    let v = match kind {
                        1 => self.u8()? as u64,
                        2 => self.u16()? as u64,
                        3 => self.u32()? as u64,
                        _ => self.u64()?,
                    };
                    row.entries.push((t, Value::Num(v)));
                }
                Ok(param)
            }
            5 => {
                let t = self.u16()?;
                let len = self.u16()?;
                let s = if len & 0x8000 != 0 {
                    let n = (len & 0x7FFF) as usize;
                    utf16(self.bytes(n * 2)?)
                } else {
                    self.bytes(len as usize)?.iter().map(|&b| b as char).collect()
                };
                self.encoding.get_or_insert(if len & 0x8000 != 0 { 4 } else { 5 });
                row.entries.push((t, Value::Str(s)));
                Ok(1)
            }
            6 => {
                let t = self.u16()?;
                let n = self.count()?;
                let mut rows = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    rows.push(self.row()?);
                }
                row.entries.push((t, Value::Rows(rows)));
                Ok(1)
            }
            7 => {
                let t = self.u16()?;
                let g: [u8; 16] = self.bytes(16)?.try_into().unwrap();
                row.entries.push((t, Value::Guid(g)));
                Ok(1)
            }
            8 => {
                let t = self.u16()?;
                let n = self.u16()? as usize;
                row.entries.push((t, Value::Blob(self.bytes(n)?.to_vec())));
                Ok(1)
            }
            9 => {
                let t = self.u16()?;
                if param > 3 {
                    return Err(Error::Format(format!("inline string of {param} units at {at}")));
                }
                let s = utf16(self.bytes(param * 2)?);
                row.entries.push((t, Value::Str(s)));
                Ok(1)
            }
            10 => {
                let t = self.u16()?;
                let n = self.u16()? as usize;
                let s = match param {
                    1 => String::from_utf8_lossy(self.bytes(n)?).to_string(),
                    2 | 3 => {
                        let b = self.bytes(n * 4)?;
                        b.chunks(4).map(|c| char::from_u32(u32::from_le_bytes([c[0], c[1], c[2], c[3]])).unwrap_or('\u{FFFD}')).collect()
                    }
                    4 => utf16(self.bytes(n * 2)?),
                    5 => self.bytes(n)?.iter().map(|&b| b as char).collect(),
                    _ => return Err(Error::Format(format!("string encoding {param} at {at}"))),
                };
                self.encoding.get_or_insert(param as u8);
                row.entries.push((t, Value::Str(s)));
                Ok(1)
            }
            _ => Err(Error::Format(format!("entry kind {kind} at {at}"))),
        }
    }
}

fn utf16(b: &[u8]) -> String {
    let units: Vec<u16> = b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

/// Parse a marshal (not obscured).
pub fn parse(bytes: &[u8], tokens: &Tokens) -> Result<Marshal> {
    let mut r = Reader { d: bytes, pos: 0, tokens, encoding: None };
    let _ = r.tokens;
    let flags = r.u8()?;
    let function = r.u32()?;
    let root = r.row()?;
    if r.pos != bytes.len() {
        return Err(Error::Format(format!("{} trailing bytes after the marshal", bytes.len() - r.pos)));
    }
    Ok(Marshal { flags, function, root, encoding: r.encoding.unwrap_or(4) })
}

/// Parse an obscured `.dat` file.
pub fn parse_obscured(bytes: &[u8], tokens: &Tokens) -> Result<Marshal> {
    let plain: Vec<u8> = bytes.iter().map(|b| b ^ OBSCURE).collect();
    parse(&plain, tokens)
}

// ----------------------------------------------------------------------------
// Writing
// ----------------------------------------------------------------------------

struct Writer<'a> {
    out: Vec<u8>,
    tokens: &'a Tokens,
    encoding: u8,
}

/// The integer caches of one row (`CMedsCache`).
#[derive(Default)]
struct Caches {
    w: [Vec<(u16, u64)>; 4],
}

impl<'a> Writer<'a> {
    fn u16(&mut self, v: u16) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn count(&mut self, n: usize) {
        if n > 0xFFFE {
            self.u16(0xFFFF);
            self.out.extend_from_slice(&(n as u32).to_le_bytes());
        } else {
            self.u16(n as u16);
        }
    }

    fn flush(&mut self, caches: &mut Caches, width: usize) {
        let c = std::mem::take(&mut caches.w[width]);
        if c.is_empty() {
            return;
        }
        self.u16((((width + 1) as u16) << 12) | c.len() as u16);
        for (t, _) in &c {
            self.u16(*t);
        }
        for (_, v) in &c {
            match width {
                0 => self.out.push(*v as u8),
                1 => self.u16(*v as u16),
                2 => self.out.extend_from_slice(&(*v as u32).to_le_bytes()),
                _ => self.out.extend_from_slice(&v.to_le_bytes()),
            }
        }
    }

    fn push_num(&mut self, caches: &mut Caches, token: u16, v: u64, width: usize) {
        caches.w[width].push((token, v));
        if caches.w[width].len() == 32 {
            self.flush(caches, width);
        }
    }

    fn row(&mut self, row: &Row) {
        self.count(row.entries.len());
        let mut caches = Caches::default();
        for (token, value) in &row.entries {
            match value {
                Value::Num(v) => {
                    let width = match self.tokens.kind(*token) {
                        Kind::Byte => 0,
                        Kind::Word => usize::from(*v > 0xFF),
                        Kind::Double | Kind::Qword | Kind::NetId | Kind::DateTime => 3,
                        // Int, Dword, Float and anything unknown: by value.
                        _ => {
                            if *v <= 0xFF {
                                0
                            } else if *v <= 0xFFFF {
                                1
                            } else if *v <= 0xFFFF_FFFF {
                                2
                            } else {
                                3
                            }
                        }
                    };
                    self.push_num(&mut caches, *token, *v, width);
                }
                Value::Str(s) => {
                    self.u16(0xA000 | self.encoding as u16);
                    self.u16(*token);
                    match self.encoding {
                        1 => {
                            self.u16(s.len() as u16);
                            self.out.extend_from_slice(s.as_bytes());
                        }
                        2 | 3 => {
                            self.u16(s.chars().count() as u16);
                            for c in s.chars() {
                                self.out.extend_from_slice(&(c as u32).to_le_bytes());
                            }
                        }
                        5 => {
                            self.u16(s.chars().count() as u16);
                            for c in s.chars() {
                                self.out.push(c as u32 as u8);
                            }
                        }
                        _ => {
                            let units: Vec<u16> = s.encode_utf16().collect();
                            self.u16(units.len() as u16);
                            for u in units {
                                self.u16(u);
                            }
                        }
                    }
                }
                Value::Rows(rows) => {
                    self.u16(0x6001);
                    self.u16(*token);
                    self.count(rows.len());
                    for r in rows {
                        self.row(r);
                    }
                }
                Value::Guid(g) => {
                    self.u16(0x7001);
                    self.u16(*token);
                    self.out.extend_from_slice(g);
                }
                Value::Blob(b) => {
                    self.u16(0x8001);
                    self.u16(*token);
                    self.u16(b.len() as u16);
                    self.out.extend_from_slice(b);
                }
            }
        }
        for width in 0..4 {
            self.flush(&mut caches, width);
        }
    }
}

/// Serialize a marshal (not obscured).
pub fn write(m: &Marshal, tokens: &Tokens) -> Vec<u8> {
    let mut w = Writer { out: Vec::new(), tokens, encoding: m.encoding };
    w.out.push(m.flags);
    w.out.extend_from_slice(&m.function.to_le_bytes());
    w.row(&m.root);
    w.out
}

/// Serialize an obscured `.dat` file.
pub fn write_obscured(m: &Marshal, tokens: &Tokens) -> Vec<u8> {
    write(m, tokens).into_iter().map(|b| b ^ OBSCURE).collect()
}

// ----------------------------------------------------------------------------
// Display helpers
// ----------------------------------------------------------------------------

/// A number as the game reads it, for display and JSON.
pub fn num_json(kind: Kind, v: u64) -> serde_json::Value {
    use serde_json::json;
    match kind {
        Kind::Int => json!(v as u32 as i32),
        Kind::Float => json!(f32::from_bits(v as u32)),
        Kind::Double => json!(f64::from_bits(v)),
        Kind::Qword | Kind::NetId => json!(v.to_string()),
        Kind::DateTime => json!(v),
        _ => json!(v),
    }
}

/// Parse a number typed by the user, by field kind.
pub fn num_parse(kind: Kind, text: &str) -> Result<u64> {
    let t = text.trim();
    let bad = || Error::Format(format!("{t:?} is not a valid {}", kind.name()));
    Ok(match kind {
        Kind::Float => f32::to_bits(t.parse::<f32>().map_err(|_| bad())?) as u64,
        Kind::Double => f64::to_bits(t.parse::<f64>().map_err(|_| bad())?),
        Kind::Int => {
            if let Ok(i) = t.parse::<i64>() {
                if (i32::MIN as i64..=u32::MAX as i64).contains(&i) {
                    return Ok(i as i32 as u32 as u64);
                }
            }
            return Err(bad());
        }
        Kind::Byte => t.parse::<u8>().map_err(|_| bad())? as u64,
        Kind::Word => t.parse::<u16>().map_err(|_| bad())? as u64,
        Kind::Dword => t.parse::<u32>().map_err(|_| bad())? as u64,
        _ => t.parse::<u64>().map_err(|_| bad())?,
    })
}

/// JSON of a value (nested row sets as arrays of objects keyed by field name).
pub fn value_json(tokens: &Tokens, token: u16, v: &Value) -> serde_json::Value {
    use serde_json::json;
    match v {
        Value::Num(n) => num_json(tokens.kind(token), *n),
        Value::Str(s) => json!(s),
        Value::Rows(rows) => serde_json::Value::Array(rows.iter().map(|r| row_json(tokens, r)).collect()),
        Value::Guid(g) => json!(g.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        Value::Blob(b) => json!(crate::json::base64(b)),
    }
}

/// JSON object of a row (field name → value).
pub fn row_json(tokens: &Tokens, row: &Row) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (t, v) in &row.entries {
        m.insert(tokens.name(*t), value_json(tokens, *t, v));
    }
    serde_json::Value::Object(m)
}

/// A row from JSON (the inverse of [`row_json`]; field order follows the object).
pub fn row_from_json(tokens: &Tokens, v: &serde_json::Value) -> Result<Row> {
    let obj = v.as_object().ok_or_else(|| Error::Format("row is not an object".into()))?;
    let mut row = Row::default();
    for (name, val) in obj {
        let token = tokens.token(name).ok_or_else(|| Error::Format(format!("unknown field {name}")))?;
        row.entries.push((token, value_from_json(tokens, token, val)?));
    }
    Ok(row)
}

pub fn value_from_json(tokens: &Tokens, token: u16, v: &serde_json::Value) -> Result<Value> {
    let kind = tokens.kind(token);
    Ok(match kind {
        Kind::RowSet => {
            let arr = v.as_array().ok_or_else(|| Error::Format(format!("{} must be an array of rows", tokens.name(token))))?;
            Value::Rows(arr.iter().map(|r| row_from_json(tokens, r)).collect::<Result<_>>()?)
        }
        Kind::String => Value::Str(match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        }),
        Kind::Guid => {
            let s = v.as_str().ok_or_else(|| Error::Format("GUID must be a hex string".into()))?;
            let bytes: Vec<u8> = (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect();
            Value::Guid(bytes.try_into().map_err(|_| Error::Format("GUID must be 16 bytes of hex".into()))?)
        }
        Kind::Blob => Value::Blob(crate::serve::base64_decode(v.as_str().unwrap_or(""))),
        _ => Value::Num(match v {
            serde_json::Value::Number(n) => {
                if let Some(u) = n.as_u64() {
                    match kind {
                        Kind::Float => f32::to_bits(u as f32) as u64,
                        Kind::Double => f64::to_bits(u as f64),
                        _ => u,
                    }
                } else if let Some(i) = n.as_i64() {
                    match kind {
                        Kind::Float => f32::to_bits(i as f32) as u64,
                        Kind::Double => f64::to_bits(i as f64),
                        _ => i as i32 as u32 as u64,
                    }
                } else {
                    let f = n.as_f64().unwrap_or(0.0);
                    match kind {
                        Kind::Double => f64::to_bits(f),
                        _ => f32::to_bits(f as f32) as u64,
                    }
                }
            }
            serde_json::Value::String(s) => num_parse(kind, s)?,
            serde_json::Value::Bool(b) => *b as u64,
            other => return Err(Error::Format(format!("{} cannot hold {other}", tokens.name(token)))),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tables() {
        let t = Tokens::builtin();
        assert!(t.field_count() > 3000);
        assert_eq!(t.token("DATA_SET_ITEMS").map(|k| t.kind(k)), Some(Kind::RowSet));
        assert_eq!(t.function("GET_DATA_ASSEMBLY"), 0x2E2EC0E9);
        assert_eq!(t.function_name(0x2E2EC0E9), Some("GET_DATA_ASSEMBLY"));
        assert_eq!(t.kind(t.token("ITEM_ID").unwrap()), Kind::Dword);
    }

    #[test]
    fn round_trip_small() {
        let t = Tokens::builtin();
        let items = t.token("DATA_SET_ITEMS").unwrap();
        let id = t.token("ITEM_ID").unwrap();
        let name = t.token("NAME").unwrap();
        let price = t.token("PRICE").unwrap();
        let mut row = Row::default();
        row.set(id, Value::Num(70000));
        row.set(name, Value::Str("Épée".into()));
        row.set(price, Value::Num(5));
        let m = Marshal { flags: 0, function: t.function("GET_DATA_ASSEMBLY"), root: Row { entries: vec![(items, Value::Rows(vec![row.clone(), row]))] }, encoding: 4 };
        let bytes = write_obscured(&m, &t);
        let back = parse_obscured(&bytes, &t).unwrap();
        // The writer batches numbers after the strings: same fields, and a
        // second write reproduces the bytes.
        let rows = back.root.rows(items).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].str(name), Some("Épée"));
        assert_eq!(rows[0].num(id), Some(70000));
        assert_eq!(rows[1].num(price), Some(5));
        assert_eq!(write_obscured(&back, &t), bytes);
        // Two u8 fields batched, one u32: header 0x1002 then 0x3001.
        let plain: Vec<u8> = bytes.iter().map(|b| b ^ OBSCURE).collect();
        assert_eq!(&plain[..7], &[0, 0xe9, 0xc0, 0x2e, 0x2e, 1, 0]);
    }
}
