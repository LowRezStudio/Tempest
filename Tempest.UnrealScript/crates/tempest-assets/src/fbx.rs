//! Binary FBX ("Kaydara FBX Binary", versions 7.1–7.7) as a tree of nodes:
//! parsing and writing of the container only; what the nodes mean is
//! `mesh_fbx.rs`'s business.
//!
//! A node record is `end offset, property count, property list length`
//! (32-bit up to 7.4, 64-bit from 7.5), a one-byte name length, the name,
//! the properties, then the nested nodes closed by an all-zero record.
//! Properties are typed by one byte: `Y` i16, `C` bool, `I` i32, `F` f32,
//! `D` f64, `L` i64, `S` string, `R` raw bytes, and the arrays `f d l i b`
//! as `count, encoding, byte length, data` (zlib-compressed when the
//! encoding is 1).

use std::io::{Read, Write};

use tempest_upk::io::{Error, Result};

const MAGIC: &[u8] = b"Kaydara FBX Binary  \x00\x1a\x00";
const FOOTER_ID: [u8; 16] = [0xfa, 0xbc, 0xab, 0x09, 0xd0, 0xc8, 0xd4, 0x66, 0xb1, 0x76, 0xfb, 0x83, 0x1c, 0xf7, 0x26, 0x7e];
const FOOTER_MAGIC: [u8; 16] = [0xf8, 0x5a, 0x8c, 0x6a, 0xde, 0xf5, 0xd9, 0x7e, 0xec, 0xe9, 0x0c, 0xe3, 0x75, 0x8f, 0x29, 0x0b];

#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    I16(i16),
    Bool(bool),
    I32(i32),
    F32(f32),
    F64(f64),
    I64(i64),
    Str(String),
    Raw(Vec<u8>),
    F32s(Vec<f32>),
    F64s(Vec<f64>),
    I64s(Vec<i64>),
    I32s(Vec<i32>),
    Bools(Vec<bool>),
}

impl Prop {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Prop::I16(v) => Some(*v as i64),
            Prop::Bool(v) => Some(*v as i64),
            Prop::I32(v) => Some(*v as i64),
            Prop::I64(v) => Some(*v),
            Prop::F32(v) => Some(*v as i64),
            Prop::F64(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Prop::F32(v) => Some(*v as f64),
            Prop::F64(v) => Some(*v),
            Prop::I16(v) => Some(*v as f64),
            Prop::I32(v) => Some(*v as f64),
            Prop::I64(v) => Some(*v as f64),
            Prop::Bool(v) => Some(*v as u8 as f64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Prop::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Any numeric array as f64 (a scalar counts as a one-element array).
    pub fn as_f64s(&self) -> Option<Vec<f64>> {
        match self {
            Prop::F32s(v) => Some(v.iter().map(|x| *x as f64).collect()),
            Prop::F64s(v) => Some(v.clone()),
            Prop::I32s(v) => Some(v.iter().map(|x| *x as f64).collect()),
            Prop::I64s(v) => Some(v.iter().map(|x| *x as f64).collect()),
            Prop::Bools(v) => Some(v.iter().map(|x| *x as u8 as f64).collect()),
            p => p.as_f64().map(|x| vec![x]),
        }
    }

    pub fn as_i64s(&self) -> Option<Vec<i64>> {
        match self {
            Prop::I32s(v) => Some(v.iter().map(|x| *x as i64).collect()),
            Prop::I64s(v) => Some(v.clone()),
            Prop::F32s(v) => Some(v.iter().map(|x| *x as i64).collect()),
            Prop::F64s(v) => Some(v.iter().map(|x| *x as i64).collect()),
            Prop::Bools(v) => Some(v.iter().map(|x| *x as i64).collect()),
            p => p.as_i64().map(|x| vec![x]),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new(name: &str) -> Node {
        Node { name: name.to_string(), props: Vec::new(), children: Vec::new() }
    }

    pub fn with(name: &str, props: Vec<Prop>) -> Node {
        Node { name: name.to_string(), props, children: Vec::new() }
    }

    /// Builder: add a child, return the node.
    pub fn with_child(mut self, child: Node) -> Node {
        self.children.push(child);
        self
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    pub fn i64(&self, i: usize) -> Option<i64> {
        self.props.get(i).and_then(|p| p.as_i64())
    }

    pub fn f64(&self, i: usize) -> Option<f64> {
        self.props.get(i).and_then(|p| p.as_f64())
    }

    pub fn str(&self, i: usize) -> Option<&str> {
        self.props.get(i).and_then(|p| p.as_str())
    }

    pub fn f64s(&self, i: usize) -> Option<Vec<f64>> {
        self.props.get(i).and_then(|p| p.as_f64s())
    }

    pub fn i64s(&self, i: usize) -> Option<Vec<i64>> {
        self.props.get(i).and_then(|p| p.as_i64s())
    }

    /// The first child called `name`'s first property as a string.
    pub fn child_str(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|c| c.str(0))
    }

    /// A `Properties70` entry (`P: name, type, label, flags, values…`).
    pub fn property(&self, name: &str) -> Option<&Node> {
        self.child("Properties70")?.children_named("P").find(|p| p.str(0) == Some(name))
    }
}

// ----------------------------------------------------------------------------
// Parsing
// ----------------------------------------------------------------------------

struct Cursor<'a> {
    b: &'a [u8],
    pos: usize,
    wide: bool,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self.b.get(self.pos..self.pos + n).ok_or_else(|| Error::Format("FBX: truncated file".into()))?;
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Result<u64> {
        let s = self.take(8)?;
        Ok(u64::from_le_bytes(s.try_into().unwrap()))
    }
    /// A record header integer: 32 or 64 bits by version.
    fn len(&mut self) -> Result<u64> {
        if self.wide { self.u64() } else { Ok(self.u32()? as u64) }
    }
    fn sentinel_len(&self) -> usize {
        if self.wide { 25 } else { 13 }
    }
}

fn array<T: Copy>(c: &mut Cursor, elem: usize, from: impl Fn(&[u8]) -> T) -> Result<Vec<T>> {
    let count = c.u32()? as usize;
    let encoding = c.u32()?;
    let len = c.u32()? as usize;
    let data = c.take(len)?;
    let raw: Vec<u8>;
    let bytes: &[u8] = if encoding == 1 {
        let mut out = Vec::with_capacity(count * elem);
        flate2::read::ZlibDecoder::new(data).read_to_end(&mut out).map_err(|e| Error::Format(format!("FBX: bad compressed array: {e}")))?;
        raw = out;
        &raw
    } else {
        data
    };
    if bytes.len() < count * elem {
        return Err(Error::Format("FBX: short array".into()));
    }
    Ok(bytes.chunks_exact(elem).take(count).map(from).collect())
}

fn prop(c: &mut Cursor) -> Result<Prop> {
    let t = c.u8()?;
    Ok(match t {
        b'Y' => {
            let s = c.take(2)?;
            Prop::I16(i16::from_le_bytes([s[0], s[1]]))
        }
        b'C' => Prop::Bool(c.u8()? != 0),
        b'I' => Prop::I32(c.u32()? as i32),
        b'F' => Prop::F32(f32::from_bits(c.u32()?)),
        b'D' => Prop::F64(f64::from_bits(c.u64()?)),
        b'L' => Prop::I64(c.u64()? as i64),
        b'S' => {
            let n = c.u32()? as usize;
            Prop::Str(String::from_utf8_lossy(c.take(n)?).into_owned())
        }
        b'R' => {
            let n = c.u32()? as usize;
            Prop::Raw(c.take(n)?.to_vec())
        }
        b'f' => Prop::F32s(array(c, 4, |s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))?),
        b'd' => Prop::F64s(array(c, 8, |s| f64::from_le_bytes(s.try_into().unwrap()))?),
        b'l' => Prop::I64s(array(c, 8, |s| i64::from_le_bytes(s.try_into().unwrap()))?),
        b'i' => Prop::I32s(array(c, 4, |s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))?),
        b'b' => Prop::Bools(array(c, 1, |s| s[0] != 0)?),
        other => return Err(Error::Format(format!("FBX: unknown property type {:?}", other as char))),
    })
}

/// One record; `None` for the all-zero closing record.
fn node(c: &mut Cursor) -> Result<Option<Node>> {
    let start = c.pos;
    let end = c.len()? as usize;
    let nprops = c.len()? as usize;
    let _plen = c.len()?;
    let nlen = c.u8()? as usize;
    if end == 0 {
        c.pos = start + c.sentinel_len();
        return Ok(None);
    }
    let name = String::from_utf8_lossy(c.take(nlen)?).into_owned();
    let mut n = Node { name, props: Vec::with_capacity(nprops), children: Vec::new() };
    for _ in 0..nprops {
        n.props.push(prop(c)?);
    }
    while c.pos < end {
        match node(c)? {
            Some(child) => n.children.push(child),
            None => break,
        }
    }
    if c.pos > end {
        return Err(Error::Format(format!("FBX: record {} overruns its end offset", n.name)));
    }
    c.pos = end;
    Ok(Some(n))
}

/// Parse a binary FBX file into `(version, top-level nodes)`.
pub fn parse(bytes: &[u8]) -> Result<(u32, Vec<Node>)> {
    if !bytes.starts_with(MAGIC) {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
        if head.contains("FBXHeaderExtension") || head.starts_with("; FBX") {
            return Err(Error::Format("ASCII FBX is not supported: export a binary FBX".into()));
        }
        return Err(Error::Format("not an FBX file".into()));
    }
    let mut c = Cursor { b: bytes, pos: MAGIC.len(), wide: false };
    let version = c.u32()?;
    c.wide = version >= 7500;
    let mut nodes = Vec::new();
    while c.pos + c.sentinel_len() <= bytes.len() {
        match node(&mut c)? {
            Some(n) => nodes.push(n),
            None => break,
        }
    }
    Ok((version, nodes))
}

// ----------------------------------------------------------------------------
// Writing
// ----------------------------------------------------------------------------

fn write_array<T>(out: &mut Vec<u8>, items: &[T], elem: usize, enc: impl Fn(&T, &mut Vec<u8>)) {
    let mut raw = Vec::with_capacity(items.len() * elem);
    for it in items {
        enc(it, &mut raw);
    }
    out.extend_from_slice(&(items.len() as u32).to_le_bytes());
    if raw.len() >= 128 {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        let _ = z.write_all(&raw);
        if let Ok(packed) = z.finish() {
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            out.extend_from_slice(&packed);
            return;
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    out.extend_from_slice(&raw);
}

fn write_prop(out: &mut Vec<u8>, p: &Prop) {
    match p {
        Prop::I16(v) => {
            out.push(b'Y');
            out.extend_from_slice(&v.to_le_bytes());
        }
        Prop::Bool(v) => {
            out.push(b'C');
            out.push(*v as u8);
        }
        Prop::I32(v) => {
            out.push(b'I');
            out.extend_from_slice(&v.to_le_bytes());
        }
        Prop::F32(v) => {
            out.push(b'F');
            out.extend_from_slice(&v.to_le_bytes());
        }
        Prop::F64(v) => {
            out.push(b'D');
            out.extend_from_slice(&v.to_le_bytes());
        }
        Prop::I64(v) => {
            out.push(b'L');
            out.extend_from_slice(&v.to_le_bytes());
        }
        Prop::Str(s) => {
            out.push(b'S');
            out.extend_from_slice(&(s.len() as u32).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        Prop::Raw(b) => {
            out.push(b'R');
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(b);
        }
        Prop::F32s(v) => {
            out.push(b'f');
            write_array(out, v, 4, |x, o| o.extend_from_slice(&x.to_le_bytes()));
        }
        Prop::F64s(v) => {
            out.push(b'd');
            write_array(out, v, 8, |x, o| o.extend_from_slice(&x.to_le_bytes()));
        }
        Prop::I64s(v) => {
            out.push(b'l');
            write_array(out, v, 8, |x, o| o.extend_from_slice(&x.to_le_bytes()));
        }
        Prop::I32s(v) => {
            out.push(b'i');
            write_array(out, v, 4, |x, o| o.extend_from_slice(&x.to_le_bytes()));
        }
        Prop::Bools(v) => {
            out.push(b'b');
            write_array(out, v, 1, |x, o| o.push(*x as u8));
        }
    }
}

fn write_node(out: &mut Vec<u8>, n: &Node, wide: bool) {
    let start = out.len();
    let hdr = if wide { 8 } else { 4 };
    out.resize(start + hdr * 3, 0);
    out.push(n.name.len() as u8);
    out.extend_from_slice(n.name.as_bytes());
    let pstart = out.len();
    for p in &n.props {
        write_prop(out, p);
    }
    let plen = out.len() - pstart;
    if !n.children.is_empty() || n.props.is_empty() {
        for c in &n.children {
            write_node(out, c, wide);
        }
        out.resize(out.len() + if wide { 25 } else { 13 }, 0);
    }
    let end = out.len();
    let fields = [end as u64, n.props.len() as u64, plen as u64];
    for (k, v) in fields.iter().enumerate() {
        let at = start + k * hdr;
        if wide {
            out[at..at + 8].copy_from_slice(&v.to_le_bytes());
        } else {
            out[at..at + 4].copy_from_slice(&(*v as u32).to_le_bytes());
        }
    }
}

/// Serialise top-level nodes as a binary FBX file of `version` (7400 is
/// the one every importer reads).
pub fn write(version: u32, nodes: &[Node]) -> Vec<u8> {
    let wide = version >= 7500;
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&version.to_le_bytes());
    for n in nodes {
        write_node(&mut out, n, wide);
    }
    out.resize(out.len() + if wide { 25 } else { 13 }, 0);
    out.extend_from_slice(&FOOTER_ID);
    let pad = (16 - out.len() % 16) % 16;
    out.resize(out.len() + if pad == 0 { 16 } else { pad }, 0);
    out.extend_from_slice(&version.to_le_bytes());
    out.resize(out.len() + 120, 0);
    out.extend_from_slice(&FOOTER_MAGIC);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_round_trips() {
        let tree = vec![
            Node::with("Header", vec![Prop::I32(7400), Prop::Str("x\0\u{1}Model".into())]),
            Node::new("Objects")
                .with_child(Node::with("Geometry", vec![Prop::I64(1), Prop::F64s((0..100).map(|i| i as f64 * 0.5).collect()), Prop::I32s(vec![0, 1, -3]), Prop::Bools(vec![true, false]), Prop::Raw(vec![1, 2, 3])]).with_child(Node::with("Empty", vec![])))
                .with_child(Node::with("Leaf", vec![Prop::Bool(true), Prop::I16(-2), Prop::F32(1.5), Prop::I64s(vec![1 << 40])])),
        ];
        for version in [7400u32, 7500] {
            let bytes = write(version, &tree);
            let (v, back) = parse(&bytes).unwrap();
            assert_eq!(v, version);
            assert_eq!(back, tree);
        }
    }
}
