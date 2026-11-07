//! Havok 2016 binary tagfiles (`TAG0`), as `hkSerialize` writes them and
//! `hkSerializeUtil::loadOnHeap` reads them (SDK version "20160100").
//!
//! A tagfile is a tree of sections. Each section starts with a big-endian
//! `u32`: the low 30 bits are the section size including this 8-byte header,
//! the top two bits are `01` for a leaf and `00` for a section holding
//! sections; then a four-character tag. Layout as the game ships it:
//!
//! ```text
//! TAG0
//!   SDKV  "20160100"
//!   DATA  every object, laid out item after item
//!   TYPE
//!     TSTR  type and template-parameter names (NUL-terminated)
//!     TNAM  per type: name, template parameters (`t…` = type, `v…` = value)
//!     FSTR  member names
//!     TBOD  per type: parent, which optional fields follow, format, sub
//!           type, version, size/alignment, flags, members, interfaces
//!     TPAD  (empty)
//!   INDX
//!     ITEM  12 bytes per item: type | flags << 24, offset in DATA, count
//! ```
//!
//! Integers in `TNAM`/`TBOD` are prefix varints (big-endian payload: `0x`
//! one byte, `10` two, `110` three, `11100` four, `11101` five, `11110`
//! eight bytes). Leaf sections are padded with zeros to 4 bytes, `DATA` to
//! 16. In `DATA` pointers, arrays and strings are `u32` item indices (the
//! item gives the dynamic type and the element count), so every type
//! describes its *tagfile* layout: pointers and `hkArray`s are 4 bytes.
//! Items lie in `DATA` in the order the writer laid them out (not their
//! index order), each at its type's alignment, gaps zero-filled; the bytes
//! inside a record that no member covers are copied from memory (`0xCD`
//! debug fill appears), so items keep their bytes and values are decoded
//! from and encoded into them.

use std::collections::HashMap;

use tempest_upk::io::{Error, Result};

fn err(s: impl Into<String>) -> Error {
    Error::Format(s.into())
}

/// One section of the tree, in file order. Known leaves are regenerated
/// from the parsed model; anything else is kept as stored.
#[derive(Clone, Debug)]
pub enum Section {
    Leaf([u8; 4]),
    Raw([u8; 4], Vec<u8>),
    Node([u8; 4], Vec<Section>),
}

/// `TBOD` optional-field bits (`hkSerialize::TagfileReader`'s type body).
pub mod opt {
    pub const FORMAT: u32 = 0x1;
    pub const SUBTYPE: u32 = 0x2;
    pub const VERSION: u32 = 0x4;
    pub const SIZE_ALIGN: u32 = 0x8;
    pub const FLAGS: u32 = 0x10;
    pub const MEMBERS: u32 = 0x20;
    pub const INTERFACES: u32 = 0x40;
    pub const ATTRIBUTE: u32 = 0x80;
}

/// Kinds in the low nibble of a type's format word.
pub mod kind {
    pub const VOID: u32 = 0;
    pub const OPAQUE: u32 = 1;
    pub const BOOL: u32 = 2;
    pub const STRING: u32 = 3;
    pub const INT: u32 = 4;
    pub const FLOAT: u32 = 5;
    pub const POINTER: u32 = 6;
    pub const RECORD: u32 = 7;
    pub const ARRAY: u32 = 8;
    /// With ARRAY: a fixed-size inline array, `format >> 8` elements.
    pub const INLINE: u32 = 0x20;
    /// With INT: signed.
    pub const SIGNED: u32 = 0x200;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Member {
    /// Index into `FSTR`.
    pub name: u32,
    pub flags: u32,
    pub offset: u32,
    pub ty: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Body {
    pub parent: u32,
    pub opt: u32,
    pub format: u32,
    pub sub: u32,
    pub version: u32,
    pub size: u32,
    pub align: u32,
    pub flags: u32,
    pub members: Vec<Member>,
    pub interfaces: Vec<(u32, u32)>,
    pub attribute: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Type {
    /// Index into `TSTR`.
    pub name: u32,
    /// `(TSTR index, value)`: a type index for `t…` names, a number for `v…`.
    pub params: Vec<(u32, u32)>,
    pub body: Option<Body>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    /// 0x10: a single object (pointer target), 0x20: an array.
    pub flags: u8,
    pub ty: u32,
    pub count: u32,
    /// Offset in `DATA` as read; only orders the layout (new items sort last).
    pub offset: u32,
    /// `count * size` bytes.
    pub bytes: Vec<u8>,
}

pub const ITEM_OBJECT: u8 = 0x10;
pub const ITEM_ARRAY: u8 = 0x20;

#[derive(Clone, Debug, Default)]
pub struct Tagfile {
    pub sections: Vec<Section>,
    pub sdk: Vec<u8>,
    pub tstr: Vec<Vec<u8>>,
    pub fstr: Vec<Vec<u8>>,
    /// Index 0 is the null type.
    pub types: Vec<Type>,
    /// Type indices in `TBOD` order.
    pub body_order: Vec<u32>,
    pub items: Vec<Item>,
}

// --- varints -----------------------------------------------------------------

struct Cur<'a> {
    d: &'a [u8],
    p: usize,
}

impl Cur<'_> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self.d.get(self.p).ok_or_else(|| err("tagfile: varint past the end of its section"))?;
        self.p += 1;
        Ok(b)
    }
    fn vi(&mut self) -> Result<u32> {
        let b0 = self.byte()? as u64;
        let (n, mut v) = if b0 & 0x80 == 0 {
            (0, b0)
        } else if b0 & 0xc0 == 0x80 {
            (1, b0 & 0x3f)
        } else if b0 & 0xe0 == 0xc0 {
            (2, b0 & 0x1f)
        } else if b0 & 0xf8 == 0xe0 {
            (3, b0 & 0x07)
        } else if b0 & 0xf8 == 0xe8 {
            (4, b0 & 0x07)
        } else if b0 & 0xf8 == 0xf0 {
            (7, b0 & 0x07)
        } else {
            return Err(err(format!("tagfile: bad varint prefix {b0:#x}")));
        };
        for _ in 0..n {
            v = v << 8 | self.byte()? as u64;
        }
        u32::try_from(v).map_err(|_| err("tagfile: varint over 32 bits"))
    }
    fn done(&self) -> bool {
        self.p >= self.d.len()
    }
}

fn put_vi(out: &mut Vec<u8>, v: u32) {
    let v = v as u64;
    if v < 0x80 {
        out.push(v as u8);
    } else if v < 0x4000 {
        out.extend_from_slice(&[0x80 | (v >> 8) as u8, v as u8]);
    } else if v < 0x20_0000 {
        out.extend_from_slice(&[0xc0 | (v >> 16) as u8, (v >> 8) as u8, v as u8]);
    } else if v < 0x800_0000 {
        out.extend_from_slice(&[0xe0 | (v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8]);
    } else {
        out.push(0xe8 | (v >> 32) as u8);
        out.extend_from_slice(&(v as u32).to_be_bytes());
    }
}

fn pad(out: &mut Vec<u8>, to: usize) {
    while out.len() % to != 0 {
        out.push(0);
    }
}

fn strings(d: &[u8]) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = d.split(|&b| b == 0).map(|s| s.to_vec()).collect();
    // `split` yields the (empty) remainder after the last NUL.
    v.pop();
    v
}

fn put_strings(list: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for s in list {
        out.extend_from_slice(s);
        out.push(0);
    }
    pad(&mut out, 4);
    out
}

// --- reading -----------------------------------------------------------------

fn read_sections(d: &[u8], mut off: usize, end: usize, leaves: &mut HashMap<[u8; 4], (usize, usize)>) -> Result<Vec<Section>> {
    let mut out = Vec::new();
    while off < end {
        if off + 8 > end {
            return Err(err("tagfile: truncated section header"));
        }
        let h = u32::from_be_bytes(d[off..off + 4].try_into().unwrap());
        let size = (h & 0x3fff_ffff) as usize;
        let tag: [u8; 4] = d[off + 4..off + 8].try_into().unwrap();
        if size < 8 || off + size > end {
            return Err(err(format!("tagfile: section {} has a bad size {size}", String::from_utf8_lossy(&tag))));
        }
        match h >> 30 {
            0 => out.push(Section::Node(tag, read_sections(d, off + 8, off + size, leaves)?)),
            1 => {
                if matches!(&tag, b"SDKV" | b"DATA" | b"TSTR" | b"TNAM" | b"FSTR" | b"TBOD" | b"ITEM") {
                    leaves.insert(tag, (off + 8, off + size));
                    out.push(Section::Leaf(tag));
                } else {
                    out.push(Section::Raw(tag, d[off + 8..off + size].to_vec()));
                }
            }
            f => return Err(err(format!("tagfile: section flags {f}"))),
        }
        off += size;
    }
    Ok(out)
}

impl Tagfile {
    /// Where a tagfile starts in a buffer (the size word before `TAG0`).
    pub fn find(d: &[u8]) -> Option<usize> {
        d.windows(4).position(|w| w == b"TAG0").filter(|&p| p >= 4).map(|p| p - 4)
    }

    /// Size of the tagfile at the start of `d` (its `TAG0` header).
    pub fn stored_len(d: &[u8]) -> Option<usize> {
        if d.len() < 8 || &d[4..8] != b"TAG0" {
            return None;
        }
        Some((u32::from_be_bytes(d[0..4].try_into().unwrap()) & 0x3fff_ffff) as usize)
    }

    pub fn read(d: &[u8]) -> Result<Tagfile> {
        let total = Tagfile::stored_len(d).ok_or_else(|| err("not a Havok tagfile (no TAG0)"))?;
        if total > d.len() {
            return Err(err("tagfile: TAG0 is larger than the data"));
        }
        let mut leaves = HashMap::new();
        let sections = read_sections(d, 0, total, &mut leaves)?;
        let leaf = |t: &[u8; 4]| -> Result<&[u8]> {
            let (a, b) = leaves.get(t).ok_or_else(|| err(format!("tagfile: no {} section", String::from_utf8_lossy(t))))?;
            Ok(&d[*a..*b])
        };
        let mut tf = Tagfile { sections, sdk: leaf(b"SDKV")?.to_vec(), tstr: strings(leaf(b"TSTR")?), fstr: strings(leaf(b"FSTR")?), ..Default::default() };

        let mut c = Cur { d: leaf(b"TNAM")?, p: 0 };
        let n = c.vi()? as usize;
        tf.types.push(Type::default());
        for _ in 1..n {
            let name = c.vi()?;
            let k = c.vi()?;
            let mut params = Vec::with_capacity(k as usize);
            for _ in 0..k {
                params.push((c.vi()?, c.vi()?));
            }
            tf.types.push(Type { name, params, body: None });
        }
        if c.d[c.p..].iter().any(|&b| b != 0) {
            return Err(err("tagfile: TNAM has bytes after its types"));
        }

        let mut c = Cur { d: leaf(b"TBOD")?, p: 0 };
        while !c.done() {
            let ti = c.vi()?;
            if ti == 0 {
                continue;
            }
            let mut b = Body { parent: c.vi()?, opt: c.vi()?, ..Default::default() };
            if b.opt & !0xff != 0 {
                return Err(err(format!("tagfile: unknown type body fields {:#x}", b.opt)));
            }
            if b.opt & opt::FORMAT != 0 {
                b.format = c.vi()?;
            }
            if b.opt & opt::SUBTYPE != 0 {
                b.sub = c.vi()?;
            }
            if b.opt & opt::VERSION != 0 {
                b.version = c.vi()?;
            }
            if b.opt & opt::SIZE_ALIGN != 0 {
                b.size = c.vi()?;
                b.align = c.vi()?;
            }
            if b.opt & opt::FLAGS != 0 {
                b.flags = c.vi()?;
            }
            if b.opt & opt::MEMBERS != 0 {
                let n = c.vi()?;
                for _ in 0..n {
                    b.members.push(Member { name: c.vi()?, flags: c.vi()?, offset: c.vi()?, ty: c.vi()? });
                }
            }
            if b.opt & opt::INTERFACES != 0 {
                let n = c.vi()?;
                for _ in 0..n {
                    b.interfaces.push((c.vi()?, c.vi()?));
                }
            }
            if b.opt & opt::ATTRIBUTE != 0 {
                b.attribute = c.vi()?;
            }
            let t = tf.types.get_mut(ti as usize).ok_or_else(|| err(format!("tagfile: body of unknown type {ti}")))?;
            if t.body.is_some() {
                return Err(err(format!("tagfile: type {ti} has two bodies")));
            }
            t.body = Some(b);
            tf.body_order.push(ti);
        }

        let data = leaf(b"DATA")?;
        let it = leaf(b"ITEM")?;
        for e in it.chunks_exact(12) {
            let w = u32::from_le_bytes(e[0..4].try_into().unwrap());
            let offset = u32::from_le_bytes(e[4..8].try_into().unwrap());
            let count = u32::from_le_bytes(e[8..12].try_into().unwrap());
            let ty = w & 0xff_ffff;
            let flags = (w >> 24) as u8;
            let mut item = Item { flags, ty, count, offset, bytes: Vec::new() };
            if ty != 0 {
                let len = tf.size(ty) as usize * count as usize;
                let o = offset as usize;
                item.bytes = data.get(o..o + len).ok_or_else(|| err(format!("tagfile: item at {o:#x} ({len} bytes) is outside DATA")))?.to_vec();
            }
            tf.items.push(item);
        }
        Ok(tf)
    }

    // --- writing -------------------------------------------------------------

    /// `DATA`: items in their layout order, each aligned, zero gaps, padded to 16.
    pub fn layout(&self) -> (Vec<u8>, Vec<u32>) {
        let mut order: Vec<usize> = (1..self.items.len()).filter(|&i| self.items[i].ty != 0).collect();
        order.sort_by_key(|&i| (self.items[i].offset, i));
        let mut data = Vec::new();
        let mut offsets = vec![0u32; self.items.len()];
        for i in order {
            let it = &self.items[i];
            pad(&mut data, self.align(it.ty).max(1) as usize);
            offsets[i] = data.len() as u32;
            data.extend_from_slice(&it.bytes);
        }
        pad(&mut data, 16);
        (data, offsets)
    }

    fn leaf_bytes(&self, tag: &[u8; 4], data: &(Vec<u8>, Vec<u32>)) -> Vec<u8> {
        match tag {
            b"SDKV" => {
                let mut v = self.sdk.clone();
                pad(&mut v, 4);
                v
            }
            b"DATA" => data.0.clone(),
            b"TSTR" => put_strings(&self.tstr),
            b"FSTR" => put_strings(&self.fstr),
            b"TNAM" => {
                let mut v = Vec::new();
                put_vi(&mut v, self.types.len() as u32);
                for t in &self.types[1..] {
                    put_vi(&mut v, t.name);
                    put_vi(&mut v, t.params.len() as u32);
                    for &(a, b) in &t.params {
                        put_vi(&mut v, a);
                        put_vi(&mut v, b);
                    }
                }
                pad(&mut v, 4);
                v
            }
            b"TBOD" => {
                let mut v = Vec::new();
                for &ti in &self.body_order {
                    let Some(b) = &self.types[ti as usize].body else { continue };
                    put_vi(&mut v, ti);
                    put_vi(&mut v, b.parent);
                    put_vi(&mut v, b.opt);
                    if b.opt & opt::FORMAT != 0 {
                        put_vi(&mut v, b.format);
                    }
                    if b.opt & opt::SUBTYPE != 0 {
                        put_vi(&mut v, b.sub);
                    }
                    if b.opt & opt::VERSION != 0 {
                        put_vi(&mut v, b.version);
                    }
                    if b.opt & opt::SIZE_ALIGN != 0 {
                        put_vi(&mut v, b.size);
                        put_vi(&mut v, b.align);
                    }
                    if b.opt & opt::FLAGS != 0 {
                        put_vi(&mut v, b.flags);
                    }
                    if b.opt & opt::MEMBERS != 0 {
                        put_vi(&mut v, b.members.len() as u32);
                        for m in &b.members {
                            put_vi(&mut v, m.name);
                            put_vi(&mut v, m.flags);
                            put_vi(&mut v, m.offset);
                            put_vi(&mut v, m.ty);
                        }
                    }
                    if b.opt & opt::INTERFACES != 0 {
                        put_vi(&mut v, b.interfaces.len() as u32);
                        for &(a, c) in &b.interfaces {
                            put_vi(&mut v, a);
                            put_vi(&mut v, c);
                        }
                    }
                    if b.opt & opt::ATTRIBUTE != 0 {
                        put_vi(&mut v, b.attribute);
                    }
                }
                pad(&mut v, 4);
                v
            }
            b"ITEM" => {
                let mut v = Vec::with_capacity(self.items.len() * 12);
                for (i, it) in self.items.iter().enumerate() {
                    v.extend_from_slice(&(it.ty | (it.flags as u32) << 24).to_le_bytes());
                    v.extend_from_slice(&data.1[i].to_le_bytes());
                    v.extend_from_slice(&it.count.to_le_bytes());
                }
                v
            }
            _ => Vec::new(),
        }
    }

    fn write_sections(&self, list: &[Section], data: &(Vec<u8>, Vec<u32>), out: &mut Vec<u8>) {
        for s in list {
            let start = out.len();
            out.extend_from_slice(&[0; 8]);
            let (tag, leaf) = match s {
                Section::Leaf(t) => {
                    out.extend_from_slice(&self.leaf_bytes(t, data));
                    (t, true)
                }
                Section::Raw(t, b) => {
                    out.extend_from_slice(b);
                    (t, true)
                }
                Section::Node(t, c) => {
                    self.write_sections(c, data, out);
                    (t, false)
                }
            };
            let size = (out.len() - start) as u32 | if leaf { 0x4000_0000 } else { 0 };
            out[start..start + 4].copy_from_slice(&size.to_be_bytes());
            out[start + 4..start + 8].copy_from_slice(tag);
        }
    }

    pub fn write(&self) -> Vec<u8> {
        let data = self.layout();
        let mut out = Vec::new();
        self.write_sections(&self.sections, &data, &mut out);
        out
    }

    // --- types ---------------------------------------------------------------

    pub fn type_name(&self, ty: u32) -> String {
        if ty == 0 {
            return "-".into();
        }
        let t = &self.types[ty as usize];
        let mut s = String::from_utf8_lossy(self.tstr.get(t.name as usize).map(|v| &v[..]).unwrap_or(b"?")).into_owned();
        if !t.params.is_empty() {
            let ps: Vec<String> = t
                .params
                .iter()
                .map(|&(n, v)| {
                    let n = self.tstr.get(n as usize).map(|v| &v[..]).unwrap_or(b"?");
                    if n.first() == Some(&b't') { self.type_name(v) } else { v.to_string() }
                })
                .collect();
            s = format!("{s}<{}>", ps.join(", "));
        }
        s
    }

    /// The type named `name` (without template parameters).
    pub fn type_by_name(&self, name: &str) -> Option<u32> {
        (1..self.types.len() as u32).find(|&i| self.tstr.get(self.types[i as usize].name as usize).map(|v| &v[..]) == Some(name.as_bytes()))
    }

    pub fn member_name(&self, m: &Member) -> &str {
        self.fstr.get(m.name as usize).and_then(|v| std::str::from_utf8(v).ok()).unwrap_or("?")
    }

    fn walk<T>(&self, mut ty: u32, f: impl Fn(&Body) -> Option<T>) -> Option<T> {
        let mut guard = 0;
        while ty != 0 && guard < 64 {
            let b = self.types.get(ty as usize)?.body.as_ref()?;
            if let Some(v) = f(b) {
                return Some(v);
            }
            ty = b.parent;
            guard += 1;
        }
        None
    }

    pub fn format(&self, ty: u32) -> u32 {
        self.walk(ty, |b| (b.opt & opt::FORMAT != 0).then_some(b.format)).unwrap_or(0)
    }
    pub fn sub(&self, ty: u32) -> u32 {
        self.walk(ty, |b| (b.opt & opt::SUBTYPE != 0).then_some(b.sub)).unwrap_or(0)
    }
    pub fn size(&self, ty: u32) -> u32 {
        self.walk(ty, |b| (b.opt & opt::SIZE_ALIGN != 0).then_some(b.size)).unwrap_or(0)
    }
    pub fn align(&self, ty: u32) -> u32 {
        self.walk(ty, |b| (b.opt & opt::SIZE_ALIGN != 0).then_some(b.align)).unwrap_or(1)
    }

    /// Every member of a record, base classes first.
    pub fn members(&self, ty: u32) -> Vec<&Member> {
        let mut chain = Vec::new();
        let mut t = ty;
        while t != 0 && chain.len() < 64 {
            let Some(b) = self.types.get(t as usize).and_then(|t| t.body.as_ref()) else { break };
            chain.push(b);
            t = b.parent;
        }
        chain.iter().rev().flat_map(|b| b.members.iter()).collect()
    }

    /// Whether `ty` is `base` or derives from it.
    pub fn is_a(&self, ty: u32, base: u32) -> bool {
        let mut t = ty;
        for _ in 0..64 {
            if t == base {
                return true;
            }
            match self.types.get(t as usize).and_then(|t| t.body.as_ref()) {
                Some(b) if b.parent != 0 => t = b.parent,
                _ => return false,
            }
        }
        false
    }

    // --- values --------------------------------------------------------------

    /// Decode element `i` of an item.
    pub fn value(&self, item: u32, i: u32) -> Result<Value> {
        let it = self.items.get(item as usize).ok_or_else(|| err(format!("tagfile: no item {item}")))?;
        let sz = self.size(it.ty) as usize;
        let at = sz * i as usize;
        self.decode(it.ty, it.bytes.get(at..at + sz).ok_or_else(|| err("tagfile: element past the item"))?)
    }

    /// Every element of an item.
    pub fn values(&self, item: u32) -> Result<Vec<Value>> {
        let n = self.items.get(item as usize).map(|i| i.count).unwrap_or(0);
        (0..n).map(|i| self.value(item, i)).collect()
    }

    pub fn decode(&self, ty: u32, b: &[u8]) -> Result<Value> {
        let fmt = self.format(ty);
        let size = self.size(ty) as usize;
        if b.len() < size {
            return Err(err(format!("tagfile: {} needs {size} bytes", self.type_name(ty))));
        }
        let le = |n: usize| -> u64 {
            let mut v = 0u64;
            for k in 0..n {
                v |= (b[k] as u64) << (8 * k);
            }
            v
        };
        Ok(match fmt & 0xf {
            kind::BOOL => Value::Bool(le(size)),
            kind::STRING => Value::Str(le(4) as u32),
            kind::INT => {
                let v = le(size);
                if fmt & kind::SIGNED != 0 {
                    let sh = 64 - 8 * size as u32;
                    Value::Int(((v << sh) as i64) >> sh)
                } else {
                    Value::UInt(v)
                }
            }
            kind::FLOAT => match size {
                2 => Value::Half(le(2) as u16),
                4 => Value::F32(f32::from_bits(le(4) as u32)),
                _ => Value::F64(f64::from_bits(le(8))),
            },
            kind::POINTER => Value::Ptr(le(4) as u32),
            kind::ARRAY if fmt & kind::INLINE != 0 => {
                let n = (fmt >> 8) as usize;
                let et = self.sub(ty);
                let es = self.size(et) as usize;
                Value::Fixed((0..n).map(|k| self.decode(et, &b[k * es..])).collect::<Result<_>>()?)
            }
            kind::ARRAY => Value::Array(le(4) as u32),
            kind::RECORD => Value::Record(ty, self.members(ty).iter().map(|m| self.decode(m.ty, &b[m.offset as usize..])).collect::<Result<_>>()?),
            _ => Value::Opaque(b[..size].to_vec()),
        })
    }

    /// Encode `v` (of type `ty`) over `b`: bytes no member covers are kept.
    pub fn encode(&self, ty: u32, v: &Value, b: &mut [u8]) -> Result<()> {
        let size = self.size(ty) as usize;
        let mut put = |x: u64, n: usize| {
            for k in 0..n {
                b[k] = (x >> (8 * k)) as u8;
            }
        };
        match v {
            Value::Bool(x) | Value::UInt(x) => put(*x, size),
            Value::Int(x) => put(*x as u64, size),
            Value::Half(x) => put(*x as u64, 2),
            Value::F32(x) => put(x.to_bits() as u64, 4),
            Value::F64(x) => put(x.to_bits(), 8),
            Value::Str(x) | Value::Ptr(x) | Value::Array(x) => put(*x as u64, 4),
            Value::Fixed(vs) => {
                let et = self.sub(ty);
                let es = self.size(et) as usize;
                for (k, e) in vs.iter().enumerate() {
                    self.encode(et, e, &mut b[k * es..])?;
                }
            }
            Value::Record(_, vs) => {
                let ms = self.members(ty);
                if ms.len() != vs.len() {
                    return Err(err(format!("tagfile: {} has {} members, value has {}", self.type_name(ty), ms.len(), vs.len())));
                }
                for (m, e) in ms.iter().zip(vs) {
                    self.encode(m.ty, e, &mut b[m.offset as usize..])?;
                }
            }
            Value::Opaque(x) => b[..x.len()].copy_from_slice(x),
        }
        Ok(())
    }

    /// Re-encode every element of every item over a zeroed buffer: the bytes
    /// that differ from the stored ones are those no member covers. Errors
    /// when a re-encode over the stored bytes changes anything.
    pub fn check_values(&self) -> Result<CheckReport> {
        let mut rep = CheckReport::default();
        for (i, it) in self.items.iter().enumerate().skip(1) {
            if it.ty == 0 {
                continue;
            }
            let sz = self.size(it.ty) as usize;
            for e in 0..it.count as usize {
                let orig = &it.bytes[e * sz..(e + 1) * sz];
                let v = self.decode(it.ty, orig)?;
                let mut same = orig.to_vec();
                self.encode(it.ty, &v, &mut same)?;
                if same != orig {
                    return Err(err(format!("tagfile: item {i} element {e} ({}) does not re-encode", self.type_name(it.ty))));
                }
                let mut zero = vec![0u8; sz];
                self.encode(it.ty, &v, &mut zero)?;
                rep.values += 1;
                rep.uncovered_nonzero += zero.iter().zip(orig).filter(|(a, b)| a != b).count();
            }
            rep.items += 1;
        }
        Ok(rep)
    }

    /// Append a new item (laid out after every existing one).
    pub fn add_item(&mut self, flags: u8, ty: u32, count: u32, bytes: Vec<u8>) -> u32 {
        self.items.push(Item { flags, ty, count, offset: u32::MAX, bytes });
        (self.items.len() - 1) as u32
    }

    /// The string an item holds (`char` array, NUL-terminated).
    pub fn string(&self, item: u32) -> Option<String> {
        let it = self.items.get(item as usize)?;
        if item == 0 || it.ty == 0 {
            return None;
        }
        let end = it.bytes.iter().position(|&b| b == 0).unwrap_or(it.bytes.len());
        Some(String::from_utf8_lossy(&it.bytes[..end]).into_owned())
    }

    /// `hkRootLevelContainer.namedVariants`: `(name, class name, item)`.
    pub fn root_variants(&self) -> Result<Vec<(String, String, u32)>> {
        let root = self.items.get(1).ok_or_else(|| err("tagfile: no root item"))?;
        if self.tstr.get(self.types[root.ty as usize].name as usize).map(|v| &v[..]) != Some(b"hkRootLevelContainer") {
            return Err(err(format!("tagfile: root is {}, not hkRootLevelContainer", self.type_name(root.ty))));
        }
        let Value::Record(_, f) = self.value(1, 0)? else { return Err(err("tagfile: root is not a record")) };
        let Some(Value::Array(arr)) = f.first() else { return Err(err("tagfile: root has no namedVariants")) };
        let mut out = Vec::new();
        for v in self.values(*arr)? {
            let Value::Record(_, f) = v else { continue };
            let s = |k: usize| match f.get(k) {
                Some(Value::Str(i)) => self.string(*i).unwrap_or_default(),
                _ => String::new(),
            };
            let item = match f.get(2) {
                Some(Value::Ptr(p)) => *p,
                Some(Value::Record(_, inner)) => match inner.first() {
                    Some(Value::Ptr(p)) => *p,
                    _ => 0,
                },
                _ => 0,
            };
            out.push((s(0), s(1), item));
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, Default)]
pub struct CheckReport {
    pub items: usize,
    pub values: usize,
    /// Non-zero bytes inside items that no member covers (record padding
    /// copied from memory by the writer).
    pub uncovered_nonzero: usize,
}

/// A decoded value. Records list their members' values in
/// [`Tagfile::members`] order; pointers, arrays and strings are item indices.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(u64),
    Int(i64),
    UInt(u64),
    /// `hkHalf16`: the upper 16 bits of an `f32`.
    Half(u16),
    F32(f32),
    F64(f64),
    Str(u32),
    Ptr(u32),
    Array(u32),
    Fixed(Vec<Value>),
    Record(u32, Vec<Value>),
    Opaque(Vec<u8>),
}

impl Value {
    pub fn as_i64(&self) -> i64 {
        match self {
            Value::Int(v) => *v,
            Value::UInt(v) | Value::Bool(v) => *v as i64,
            Value::Str(v) | Value::Ptr(v) | Value::Array(v) => *v as i64,
            Value::Half(h) => half_to_f32(*h) as i64,
            Value::F32(f) => *f as i64,
            Value::F64(f) => *f as i64,
            Value::Record(_, f) if f.len() == 1 => f[0].as_i64(),
            _ => 0,
        }
    }
    pub fn as_f32(&self) -> f32 {
        match self {
            Value::F32(f) => *f,
            Value::F64(f) => *f as f32,
            Value::Half(h) => half_to_f32(*h),
            Value::Record(_, f) if f.len() == 1 => f[0].as_f32(),
            v => v.as_i64() as f32,
        }
    }
    /// The item an array / pointer / string (or a one-member wrapper such
    /// as `hkRefPtr`) refers to.
    pub fn item(&self) -> u32 {
        match self {
            Value::Str(v) | Value::Ptr(v) | Value::Array(v) => *v,
            Value::Record(_, f) if f.len() == 1 => f[0].item(),
            _ => 0,
        }
    }
    pub fn vec4(&self) -> [f32; 4] {
        match self {
            Value::Fixed(v) => {
                let mut o = [0.0; 4];
                for (k, e) in v.iter().take(4).enumerate() {
                    o[k] = e.as_f32();
                }
                o
            }
            _ => [0.0; 4],
        }
    }
}

/// `hkHalf16` → `f32` (Havok stores the top half of the float's bits).
pub fn half_to_f32(h: u16) -> f32 {
    f32::from_bits((h as u32) << 16)
}

/// `f32` → `hkHalf16` (`hkHalf16::setReal<true>`: rounded to nearest).
pub fn f32_to_half(f: f32) -> u16 {
    let b = f.to_bits();
    ((b.wrapping_add(0x8000)) >> 16) as u16
}

/// Record fields by member name.
pub struct Rec<'a> {
    pub tf: &'a Tagfile,
    pub ty: u32,
    pub fields: Vec<(&'a str, Value)>,
}

impl<'a> Rec<'a> {
    pub fn new(tf: &'a Tagfile, v: Value) -> Option<Rec<'a>> {
        let Value::Record(ty, vs) = v else { return None };
        let names: Vec<&str> = tf.members(ty).into_iter().map(|m| tf.member_name(m)).collect();
        Some(Rec { tf, ty, fields: names.into_iter().zip(vs).collect() })
    }
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| v)
    }
    pub fn int(&self, name: &str) -> i64 {
        self.get(name).map(|v| v.as_i64()).unwrap_or(0)
    }
    pub fn real(&self, name: &str) -> f32 {
        self.get(name).map(|v| v.as_f32()).unwrap_or(0.0)
    }
    pub fn item(&self, name: &str) -> u32 {
        self.get(name).map(|v| v.item()).unwrap_or(0)
    }
    pub fn vec4(&self, name: &str) -> [f32; 4] {
        self.get(name).map(|v| v.vec4()).unwrap_or_default()
    }
    pub fn rec(&self, name: &str) -> Option<Rec<'a>> {
        Rec::new(self.tf, self.get(name)?.clone())
    }
    pub fn set(&mut self, name: &str, v: Value) -> bool {
        match self.fields.iter_mut().rev().find(|(n, _)| *n == name) {
            Some(f) => {
                f.1 = v;
                true
            }
            None => false,
        }
    }
    pub fn into_value(self) -> Value {
        Value::Record(self.ty, self.fields.into_iter().map(|(_, v)| v).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_round_trip() {
        for v in [0u32, 1, 0x7f, 0x80, 0x3fff, 0x4000, 0x1f_ffff, 0x20_0000, 0x7ff_ffff, 0x800_0000, u32::MAX] {
            let mut b = Vec::new();
            put_vi(&mut b, v);
            assert_eq!(Cur { d: &b, p: 0 }.vi().unwrap(), v);
        }
    }

    #[test]
    fn half_floats() {
        assert_eq!(half_to_f32(f32_to_half(1.0)), 1.0);
        assert_eq!(half_to_f32(f32_to_half(-2.5)), -2.5);
    }
}
