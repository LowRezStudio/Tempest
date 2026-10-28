//! The display list: `MATRIX` / `CXFORM` records, `PlaceObject` (1, 2, 3)
//! and `RemoveObject` (1, 2) tags, and a timeline simulation that tells,
//! for every frame of a sprite (or the root), which character sits at which
//! depth with which transform, and which tag set it.
//!
//! `PlaceObject2/3` are rewritten from their fields up to `ClipDepth`; what
//! follows (filters, blend mode, bitmap caching, visibility, clip actions)
//! is kept as stored, so editing a matrix never disturbs a filter list.

use serde_json::{json, Value as J};

use super::{cstr, parse_tags, u16_at, BitReader, Tag};

/// Bit writer for SWF's MSB-first bit fields.
#[derive(Default)]
pub(crate) struct BitWriter {
    pub out: Vec<u8>,
    bit: usize,
}

impl BitWriter {
    pub fn bits(&mut self, n: usize, v: u32) {
        for i in (0..n).rev() {
            if self.bit == 0 {
                self.out.push(0);
            }
            if (v >> i) & 1 != 0 {
                let last = self.out.len() - 1;
                self.out[last] |= 0x80 >> self.bit;
            }
            self.bit = (self.bit + 1) % 8;
        }
    }
    pub fn signed(&mut self, n: usize, v: i32) {
        self.bits(n, v as u32 & if n >= 32 { u32::MAX } else { (1u32 << n) - 1 });
    }
    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}

/// Bits a signed value needs (two's complement, at least 1).
fn sbits(v: i32) -> usize {
    if v == 0 {
        return 1;
    }
    let mag = if v < 0 { !v } else { v } as u32;
    (32 - mag.leading_zeros()) as usize + 1
}

impl BitReader<'_> {
    pub(crate) fn align(&mut self) {
        self.pos = self.pos.div_ceil(8) * 8;
    }
    pub(crate) fn byte_pos(&self) -> usize {
        self.pos.div_ceil(8)
    }
}

/// A 2×3 affine transform: `x' = a·x + c·y + tx`, `y' = b·x + d·y + ty`
/// (scale and rotate/skew as 16.16 fixed point, translation in twips).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: i32,
    pub ty: i32,
}

impl Default for Matrix {
    fn default() -> Self {
        Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0, ty: 0 }
    }
}

impl Matrix {
    pub(crate) fn read(br: &mut BitReader) -> Matrix {
        let mut m = Matrix::default();
        if br.bit() == 1 {
            let n = br.bits(5) as usize;
            m.a = br.signed(n) as f64 / 65536.0;
            m.d = br.signed(n) as f64 / 65536.0;
        }
        if br.bit() == 1 {
            let n = br.bits(5) as usize;
            m.b = br.signed(n) as f64 / 65536.0;
            m.c = br.signed(n) as f64 / 65536.0;
        }
        let n = br.bits(5) as usize;
        m.tx = br.signed(n);
        m.ty = br.signed(n);
        br.align();
        m
    }

    pub(crate) fn write(&self, bw: &mut BitWriter) {
        let fx = |v: f64| (v * 65536.0).round() as i32;
        let (a, b, c, d) = (fx(self.a), fx(self.b), fx(self.c), fx(self.d));
        if a != 65536 || d != 65536 {
            bw.bits(1, 1);
            let n = sbits(a).max(sbits(d));
            bw.bits(5, n as u32);
            bw.signed(n, a);
            bw.signed(n, d);
        } else {
            bw.bits(1, 0);
        }
        if b != 0 || c != 0 {
            bw.bits(1, 1);
            let n = sbits(b).max(sbits(c));
            bw.bits(5, n as u32);
            bw.signed(n, b);
            bw.signed(n, c);
        } else {
            bw.bits(1, 0);
        }
        let n = if self.tx == 0 && self.ty == 0 { 0 } else { sbits(self.tx).max(sbits(self.ty)) };
        bw.bits(5, n as u32);
        bw.signed(n, self.tx);
        bw.signed(n, self.ty);
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bw = BitWriter::default();
        self.write(&mut bw);
        bw.finish()
    }

    /// `self · other` (other applied first).
    pub fn concat(&self, o: &Matrix) -> Matrix {
        Matrix {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            tx: (self.a * o.tx as f64 + self.c * o.ty as f64 + self.tx as f64).round() as i32,
            ty: (self.b * o.tx as f64 + self.d * o.ty as f64 + self.ty as f64).round() as i32,
        }
    }

    /// Axis-aligned bounds of a rect (twips) under this matrix.
    pub fn apply_rect(&self, r: [i32; 4]) -> [i32; 4] {
        let corners = [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])];
        let mut out = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for (x, y) in corners {
            let px = (self.a * x as f64 + self.c * y as f64 + self.tx as f64).round() as i32;
            let py = (self.b * x as f64 + self.d * y as f64 + self.ty as f64).round() as i32;
            out[0] = out[0].min(px);
            out[1] = out[1].min(py);
            out[2] = out[2].max(px);
            out[3] = out[3].max(py);
        }
        out
    }

    pub fn json(&self) -> J {
        json!({ "a": self.a, "b": self.b, "c": self.c, "d": self.d, "tx": self.tx as f64 / 20.0, "ty": self.ty as f64 / 20.0 })
    }

    /// From the Studio's `{a, b, c, d, tx, ty}` (translation in pixels).
    pub fn from_json(v: &J, base: Matrix) -> Matrix {
        let f = |k: &str, d: f64| v.get(k).and_then(|x| x.as_f64()).unwrap_or(d);
        Matrix {
            a: f("a", base.a),
            b: f("b", base.b),
            c: f("c", base.c),
            d: f("d", base.d),
            tx: v.get("tx").and_then(|x| x.as_f64()).map(|x| (x * 20.0).round() as i32).unwrap_or(base.tx),
            ty: v.get("ty").and_then(|x| x.as_f64()).map(|x| (x * 20.0).round() as i32).unwrap_or(base.ty),
        }
    }
}

/// A colour transform: multiply terms (8.8 fixed point, 256 = 1) and add
/// terms, red, green, blue, alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cxform {
    pub mul: [i32; 4],
    pub add: [i32; 4],
}

impl Default for Cxform {
    fn default() -> Self {
        Cxform { mul: [256; 4], add: [0; 4] }
    }
}

impl Cxform {
    pub(crate) fn read(br: &mut BitReader, alpha: bool) -> Cxform {
        let mut cx = Cxform::default();
        let has_add = br.bit() == 1;
        let has_mul = br.bit() == 1;
        let n = br.bits(4) as usize;
        let k = if alpha { 4 } else { 3 };
        if has_mul {
            for i in 0..k {
                cx.mul[i] = br.signed(n);
            }
        }
        if has_add {
            for i in 0..k {
                cx.add[i] = br.signed(n);
            }
        }
        br.align();
        cx
    }

    pub(crate) fn write(&self, bw: &mut BitWriter, alpha: bool) {
        let k = if alpha { 4 } else { 3 };
        let has_mul = self.mul[..k].iter().any(|&v| v != 256);
        let has_add = self.add[..k].iter().any(|&v| v != 0);
        bw.bits(1, has_add as u32);
        bw.bits(1, has_mul as u32);
        let mut n = 1;
        if has_mul {
            n = self.mul[..k].iter().map(|&v| sbits(v)).max().unwrap_or(1);
        }
        if has_add {
            n = n.max(self.add[..k].iter().map(|&v| sbits(v)).max().unwrap_or(1));
        }
        let n = n.min(15);
        bw.bits(4, n as u32);
        if has_mul {
            for i in 0..k {
                bw.signed(n, self.mul[i]);
            }
        }
        if has_add {
            for i in 0..k {
                bw.signed(n, self.add[i]);
            }
        }
    }

    pub fn is_identity(&self) -> bool {
        *self == Cxform::default()
    }

    pub fn json(&self) -> J {
        json!({ "mul": self.mul.iter().map(|&v| v as f64 / 256.0).collect::<Vec<_>>(), "add": self.add })
    }

    pub fn from_json(v: &J, base: Cxform) -> Cxform {
        let mut cx = base;
        if let Some(m) = v.get("mul").and_then(|m| m.as_array()) {
            for (i, x) in m.iter().take(4).enumerate() {
                if let Some(x) = x.as_f64() {
                    cx.mul[i] = (x * 256.0).round() as i32;
                }
            }
        }
        if let Some(a) = v.get("add").and_then(|m| m.as_array()) {
            for (i, x) in a.iter().take(4).enumerate() {
                if let Some(x) = x.as_f64() {
                    cx.add[i] = x.round() as i32;
                }
            }
        }
        cx
    }
}

/// A `PlaceObject`, `PlaceObject2` or `PlaceObject3` tag.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// 4, 26 or 70.
    pub code: u16,
    /// `PlaceFlagMove`: modifies the object already at `depth`.
    pub moves: bool,
    pub depth: u16,
    pub class_name: Option<String>,
    pub character: Option<u16>,
    pub matrix: Option<Matrix>,
    pub cxform: Option<Cxform>,
    pub ratio: Option<u16>,
    pub name: Option<String>,
    pub clip_depth: Option<u16>,
    /// PlaceObject3's second flag byte, minus the class-name bit (kept for the tail fields).
    flags2: u8,
    /// Everything after `ClipDepth`, as stored (filters, blend mode, caching, visibility, clip actions).
    tail: Vec<u8>,
    /// Whether the clip-actions flag was set (the tail holds them).
    clip_actions: bool,
}

pub fn is_place(code: u16) -> bool {
    matches!(code, 4 | 26 | 70)
}

impl Place {
    pub fn parse(code: u16, d: &[u8]) -> Option<Place> {
        if code == 4 {
            let character = u16_at(d, 0)?;
            let depth = u16_at(d, 2)?;
            let mut br = BitReader { d: d.get(4..)?, pos: 0 };
            let matrix = Matrix::read(&mut br);
            let at = 4 + br.byte_pos();
            let cxform = if at < d.len() {
                let mut br = BitReader { d: &d[at..], pos: 0 };
                Some(Cxform::read(&mut br, false))
            } else {
                None
            };
            return Some(Place { code, moves: false, depth, class_name: None, character: Some(character), matrix: Some(matrix), cxform, ratio: None, name: None, clip_depth: None, flags2: 0, tail: Vec::new(), clip_actions: false });
        }
        let f1 = *d.first()?;
        let (f2, mut pos) = if code == 70 { (*d.get(1)?, 2) } else { (0, 1) };
        let depth = u16_at(d, pos)?;
        pos += 2;
        let mut class_name = None;
        // HasClassName, or HasImage with HasCharacter (PlaceObject3).
        if code == 70 && (f2 & 0x08 != 0 || (f2 & 0x10 != 0 && f1 & 0x02 != 0)) {
            let (s, next) = cstr(d, pos);
            class_name = Some(s);
            pos = next;
        }
        let mut character = None;
        if f1 & 0x02 != 0 {
            character = Some(u16_at(d, pos)?);
            pos += 2;
        }
        let mut matrix = None;
        if f1 & 0x04 != 0 {
            let mut br = BitReader { d: d.get(pos..)?, pos: 0 };
            matrix = Some(Matrix::read(&mut br));
            pos += br.byte_pos();
        }
        let mut cxform = None;
        if f1 & 0x08 != 0 {
            let mut br = BitReader { d: d.get(pos..)?, pos: 0 };
            cxform = Some(Cxform::read(&mut br, true));
            pos += br.byte_pos();
        }
        let mut ratio = None;
        if f1 & 0x10 != 0 {
            ratio = Some(u16_at(d, pos)?);
            pos += 2;
        }
        let mut name = None;
        if f1 & 0x20 != 0 {
            let (s, next) = cstr(d, pos);
            name = Some(s);
            pos = next;
        }
        let mut clip_depth = None;
        if f1 & 0x40 != 0 {
            clip_depth = Some(u16_at(d, pos)?);
            pos += 2;
        }
        Some(Place {
            code,
            moves: f1 & 0x01 != 0,
            depth,
            class_name,
            character,
            matrix,
            cxform,
            ratio,
            name,
            clip_depth,
            flags2: f2 & !0x08,
            tail: d.get(pos..)?.to_vec(),
            clip_actions: f1 & 0x80 != 0,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        if self.code == 4 {
            let mut out = Vec::new();
            out.extend_from_slice(&self.character.unwrap_or(0).to_le_bytes());
            out.extend_from_slice(&self.depth.to_le_bytes());
            out.extend_from_slice(&self.matrix.unwrap_or_default().to_bytes());
            if let Some(cx) = &self.cxform {
                let mut bw = BitWriter::default();
                cx.write(&mut bw, false);
                out.extend_from_slice(&bw.finish());
            }
            return out;
        }
        let mut f1 = 0u8;
        if self.moves {
            f1 |= 0x01;
        }
        if self.character.is_some() {
            f1 |= 0x02;
        }
        if self.matrix.is_some() {
            f1 |= 0x04;
        }
        if self.cxform.is_some() {
            f1 |= 0x08;
        }
        if self.ratio.is_some() {
            f1 |= 0x10;
        }
        if self.name.is_some() {
            f1 |= 0x20;
        }
        if self.clip_depth.is_some() {
            f1 |= 0x40;
        }
        if self.clip_actions {
            f1 |= 0x80;
        }
        let mut out = vec![f1];
        if self.code == 70 {
            let image = self.flags2 & 0x10 != 0 && self.character.is_some();
            let f2 = self.flags2 | if self.class_name.is_some() && !image { 0x08 } else { 0 };
            out.push(f2);
        }
        out.extend_from_slice(&self.depth.to_le_bytes());
        if self.code == 70 {
            if let Some(c) = &self.class_name {
                out.extend_from_slice(c.as_bytes());
                out.push(0);
            }
        }
        if let Some(c) = self.character {
            out.extend_from_slice(&c.to_le_bytes());
        }
        if let Some(m) = &self.matrix {
            out.extend_from_slice(&m.to_bytes());
        }
        if let Some(cx) = &self.cxform {
            let mut bw = BitWriter::default();
            cx.write(&mut bw, true);
            out.extend_from_slice(&bw.finish());
        }
        if let Some(r) = self.ratio {
            out.extend_from_slice(&r.to_le_bytes());
        }
        if let Some(n) = &self.name {
            out.extend_from_slice(n.as_bytes());
            out.push(0);
        }
        if let Some(c) = self.clip_depth {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&self.tail);
        out
    }

    /// A fresh `PlaceObject2` of a character.
    pub fn new(depth: u16, character: u16, matrix: Matrix) -> Place {
        Place { code: 26, moves: false, depth, class_name: None, character: Some(character), matrix: Some(matrix), cxform: None, ratio: None, name: None, clip_depth: None, flags2: 0, tail: Vec::new(), clip_actions: false }
    }

    /// PlaceObject3's `Visible` field, when the tag sets it.
    pub fn visible(&self) -> Option<bool> {
        if self.flags2 & 0x20 == 0 {
            return None;
        }
        let mut at = 0;
        if self.flags2 & 0x01 != 0 {
            at += super::dict::filter_list_len(&self.tail, 0)?;
        }
        if self.flags2 & 0x02 != 0 {
            at += 1;
        }
        if self.flags2 & 0x04 != 0 {
            at += 1;
        }
        self.tail.get(at).map(|&v| v != 0)
    }

    /// Byte offset of the clip actions (`CLIPACTIONS`) in the tag's data, when it has them.
    pub fn clip_actions_at(&self, data_len: usize) -> Option<usize> {
        if !self.clip_actions {
            return None;
        }
        let mut at = 0;
        if self.flags2 & 0x01 != 0 {
            at += super::dict::filter_list_len(&self.tail, 0)?;
        }
        if self.flags2 & 0x02 != 0 {
            at += 1;
        }
        if self.flags2 & 0x04 != 0 {
            at += 1;
        }
        if self.flags2 & 0x20 != 0 {
            at += 5;
        }
        (at <= self.tail.len()).then(|| data_len - self.tail.len() + at)
    }

    /// The tail holds a filter list, blend mode or other PlaceObject3 extras.
    pub fn has_extras(&self) -> bool {
        !self.tail.is_empty()
    }
}

/// The depth a `RemoveObject` / `RemoveObject2` clears.
pub fn removed_depth(t: &Tag) -> Option<u16> {
    match t.code {
        5 => u16_at(&t.data, 2),
        28 => u16_at(&t.data, 0),
        _ => None,
    }
}

/// One object on the display list at some frame.
#[derive(Clone, Debug)]
pub struct Entry {
    pub depth: u16,
    pub character: u16,
    pub matrix: Matrix,
    pub cxform: Cxform,
    pub name: Option<String>,
    pub ratio: Option<u16>,
    pub clip_depth: Option<u16>,
    pub class_name: Option<String>,
    /// Stream index of the tag that placed the object.
    pub placed_by: usize,
    /// Stream index of the last tag that set its matrix.
    pub matrix_by: usize,
    /// Stream index of the last tag that changed it.
    pub changed_by: usize,
    /// Frame (0-based) the object was placed on.
    pub since: usize,
    /// PlaceObject3's `Visible` (scripts can still show it).
    pub visible: bool,
}

/// What a timeline shows: the display list after each frame, frame labels
/// and the frames' action tags.
pub struct Timeline {
    pub frames: Vec<Vec<Entry>>,
    pub labels: Vec<(usize, String)>,
    pub actions: Vec<(usize, usize)>,
    /// Stream index of every place/remove tag, with the frame it belongs to.
    pub controls: Vec<(usize, usize)>,
}

/// Play a tag stream (root or sprite body) through, `limit` frames at most.
pub fn timeline(tags: &[Tag], limit: usize) -> Timeline {
    let mut list: std::collections::BTreeMap<u16, Entry> = std::collections::BTreeMap::new();
    let mut frames = Vec::new();
    let mut labels = Vec::new();
    let mut actions = Vec::new();
    let mut controls = Vec::new();
    let mut frame = 0usize;
    for (i, t) in tags.iter().enumerate() {
        if frames.len() >= limit {
            break;
        }
        match t.code {
            1 => {
                frames.push(list.values().cloned().collect());
                frame += 1;
            }
            43 => labels.push((frame, cstr(&t.data, 0).0)),
            12 => actions.push((frame, i)),
            5 | 28 => {
                controls.push((i, frame));
                if let Some(d) = removed_depth(t) {
                    list.remove(&d);
                }
            }
            c if is_place(c) => {
                controls.push((i, frame));
                let Some(p) = Place::parse(c, &t.data) else { continue };
                let replace = c == 4 || !p.moves;
                if replace {
                    let Some(ch) = p.character else { continue };
                    list.insert(
                        p.depth,
                        Entry {
                            depth: p.depth,
                            character: ch,
                            matrix: p.matrix.unwrap_or_default(),
                            cxform: p.cxform.unwrap_or_default(),
                            name: p.name.clone(),
                            ratio: p.ratio,
                            clip_depth: p.clip_depth,
                            class_name: p.class_name.clone(),
                            placed_by: i,
                            matrix_by: i,
                            changed_by: i,
                            since: frame,
                            visible: p.visible().unwrap_or(true),
                        },
                    );
                } else if let Some(e) = list.get_mut(&p.depth) {
                    if let Some(ch) = p.character {
                        e.character = ch;
                    }
                    if let Some(m) = p.matrix {
                        e.matrix = m;
                        e.matrix_by = i;
                    }
                    if let Some(cx) = p.cxform {
                        e.cxform = cx;
                    }
                    if p.name.is_some() {
                        e.name = p.name.clone();
                    }
                    if p.ratio.is_some() {
                        e.ratio = p.ratio;
                    }
                    if p.clip_depth.is_some() {
                        e.clip_depth = p.clip_depth;
                    }
                    if let Some(v) = p.visible() {
                        e.visible = v;
                    }
                    e.changed_by = i;
                }
            }
            0 => break,
            _ => {}
        }
    }
    // A stream without a ShowFrame still shows what it placed.
    if frames.is_empty() {
        frames.push(list.values().cloned().collect());
    }
    Timeline { frames, labels, actions, controls }
}

/// The tags of a sprite's body.
pub fn sprite_body(t: &Tag) -> Vec<Tag> {
    parse_tags(t.data.get(4..).unwrap_or(&[])).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_round_trips() {
        for m in [
            Matrix::default(),
            Matrix { a: 1.5, b: 0.25, c: -0.25, d: 0.75, tx: -3980, ty: 20 },
            Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0, ty: -360 },
        ] {
            let bytes = m.to_bytes();
            let back = Matrix::read(&mut BitReader { d: &bytes, pos: 0 });
            assert_eq!(back, m);
        }
    }

    #[test]
    fn place_round_trips_and_edits() {
        let mut p = Place::new(4, 12, Matrix { tx: 200, ty: -40, ..Matrix::default() });
        p.name = Some("TF".into());
        p.cxform = Some(Cxform { mul: [256, 256, 256, 128], add: [0; 4] });
        let bytes = p.write();
        let q = Place::parse(26, &bytes).unwrap();
        assert_eq!(q, p);
        assert_eq!(q.write(), bytes);
        let mut r = q.clone();
        r.matrix = Some(Matrix { tx: -7960, ..r.matrix.unwrap() });
        let back = Place::parse(26, &r.write()).unwrap();
        assert_eq!(back.matrix.unwrap().tx, -7960);
        assert_eq!(back.name.as_deref(), Some("TF"));
    }

    #[test]
    fn timeline_tracks_moves_and_removes() {
        let place = |p: &Place| Tag { code: p.code, data: p.write(), long: false };
        let mut mv = Place::new(1, 0, Matrix { tx: 100, ..Matrix::default() });
        mv.moves = true;
        mv.character = None;
        let tags = vec![
            place(&Place::new(1, 5, Matrix::default())),
            Tag { code: 1, data: vec![], long: false },
            place(&mv),
            Tag { code: 1, data: vec![], long: false },
            Tag { code: 28, data: vec![1, 0], long: false },
            Tag { code: 1, data: vec![], long: false },
            Tag { code: 0, data: vec![], long: false },
        ];
        let tl = timeline(&tags, 100);
        assert_eq!(tl.frames.len(), 3);
        assert_eq!(tl.frames[0][0].matrix.tx, 0);
        assert_eq!(tl.frames[1][0].matrix.tx, 100);
        assert_eq!(tl.frames[1][0].matrix_by, 2);
        assert_eq!(tl.frames[1][0].placed_by, 0);
        assert!(tl.frames[2].is_empty());
    }
}
