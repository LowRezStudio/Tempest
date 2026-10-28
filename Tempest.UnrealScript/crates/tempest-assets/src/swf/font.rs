//! Fonts: `DefineFont2`/`DefineFont3` parsed and rewritten byte for byte
//! (`DefineFont` with its `DefineFontInfo` read for the code table), export
//! as a TrueType file, and new glyphs taken from a TrueType/OpenType font.
//!
//! Glyphs are `SHAPE`s in an EM square of 1024 units (`DefineFont3`: 20480,
//! twips), y down. Text tags refer to glyphs by index, so adding glyphs in
//! code order renumbers them and the caller remaps the texts (`remap`).

use tempest_upk::io::{Error, Result};

use super::dict::read_rect;
use super::display::BitWriter;
use super::shape::{write_glyph, Edge, Pt, Shape};
use super::{u16_at, u32_at, Movie, Tag};

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub ascent: u16,
    pub descent: u16,
    pub leading: i16,
    pub advances: Vec<i16>,
    pub bounds: Vec<[i32; 4]>,
    pub kerning: Vec<(u16, u16, i16)>,
}

#[derive(Clone, Debug)]
pub struct Font {
    pub code: u16,
    pub id: u16,
    /// DefineFont2/3 flags (layout, Shift-JIS, small text, ANSI, wide offsets, wide codes, italic, bold).
    pub flags: u8,
    pub language: u8,
    pub name: Vec<u8>,
    /// Each glyph's `SHAPE` bytes.
    pub glyphs: Vec<Vec<u8>>,
    /// Character code of each glyph (UCS-2).
    pub codes: Vec<u16>,
    pub layout: Option<Layout>,
}

const HAS_LAYOUT: u8 = 0x80;
const WIDE_OFFSETS: u8 = 0x08;
const WIDE_CODES: u8 = 0x04;
const ITALIC: u8 = 0x02;
const BOLD: u8 = 0x01;

pub fn is_font(code: u16) -> bool {
    matches!(code, 10 | 48 | 75)
}

impl Font {
    pub fn name(&self) -> String {
        String::from_utf8_lossy(&self.name).trim_end_matches('\0').to_string()
    }

    /// Units per EM of the glyph coordinates.
    pub fn em(&self) -> f64 {
        if self.code == 75 { 20480.0 } else { 1024.0 }
    }

    pub fn bold(&self) -> bool {
        self.flags & BOLD != 0
    }

    pub fn italic(&self) -> bool {
        self.flags & ITALIC != 0
    }

    /// Glyph index of a character.
    pub fn index_of(&self, c: char) -> Option<usize> {
        let code = u16::try_from(c as u32).ok()?;
        self.codes.iter().position(|&k| k == code)
    }

    pub fn text(&self, indices: &[u32]) -> String {
        indices.iter().map(|&i| self.codes.get(i as usize).and_then(|&c| char::from_u32(c as u32)).unwrap_or('\u{fffd}')).collect()
    }

    /// The font defined by `tag`, with `DefineFontInfo` codes for a `DefineFont`.
    pub fn parse(m: &Movie, tag: usize) -> Result<Font> {
        let t = &m.tags[tag];
        let bad = || Error::Format(format!("{} does not parse as a font", super::tag_name(t.code)));
        match t.code {
            48 | 75 => Self::parse2(t).ok_or_else(bad),
            10 => Self::parse1(m, t).ok_or_else(bad),
            _ => Err(bad()),
        }
    }

    fn parse1(m: &Movie, t: &Tag) -> Option<Font> {
        let d = &t.data;
        let id = u16_at(d, 0)?;
        let n = if d.len() > 2 { u16_at(d, 2)? as usize / 2 } else { 0 };
        let offsets: Vec<usize> = (0..n).map(|i| u16_at(d, 2 + i * 2).map(|o| o as usize + 2)).collect::<Option<_>>()?;
        let glyphs = (0..n).map(|i| d.get(offsets[i]..*offsets.get(i + 1).unwrap_or(&d.len())).map(|s| s.to_vec())).collect::<Option<Vec<_>>>()?;
        let mut f = Font { code: 10, id, flags: 0, language: 0, name: Vec::new(), glyphs, codes: (0..n as u16).collect(), layout: None };
        // DefineFontInfo / DefineFontInfo2: name, flags, codes.
        if let Some(info) = m.tags.iter().find(|x| matches!(x.code, 13 | 62) && u16_at(&x.data, 0) == Some(id)) {
            let d = &info.data;
            let len = *d.get(2)? as usize;
            f.name = d.get(3..3 + len)?.to_vec();
            let flags = *d.get(3 + len)?;
            f.flags = flags & (ITALIC | BOLD);
            let mut pos = 4 + len + if info.code == 62 { 1 } else { 0 };
            let wide = flags & 0x01 != 0;
            f.codes = (0..n)
                .map(|_| {
                    let c = if wide { u16_at(d, pos) } else { d.get(pos).map(|&b| b as u16) };
                    pos += if wide { 2 } else { 1 };
                    c
                })
                .collect::<Option<_>>()?;
        }
        Some(f)
    }

    fn parse2(t: &Tag) -> Option<Font> {
        let d = &t.data;
        let id = u16_at(d, 0)?;
        let flags = *d.get(2)?;
        let language = *d.get(3)?;
        let len = *d.get(4)? as usize;
        let name = d.get(5..5 + len)?.to_vec();
        let mut pos = 5 + len;
        let n = u16_at(d, pos)? as usize;
        pos += 2;
        let wide = flags & WIDE_OFFSETS != 0;
        let table = pos;
        let off = |i: usize| if wide { u32_at(d, table + i * 4).map(|v| v as usize) } else { u16_at(d, table + i * 2).map(|v| v as usize) };
        let offsets: Vec<usize> = (0..n).map(off).collect::<Option<_>>()?;
        let code_table = if n > 0 || d.len() > table { off(n).unwrap_or(0) } else { 0 };
        let mut glyphs = Vec::with_capacity(n);
        for i in 0..n {
            let end = if i + 1 < n { offsets[i + 1] } else { code_table };
            glyphs.push(d.get(table + offsets[i]..table + end)?.to_vec());
        }
        let mut f = Font { code: t.code, id, flags, language, name, glyphs, codes: Vec::with_capacity(n), layout: None };
        if n == 0 && code_table == 0 {
            return Some(f);
        }
        pos = table + code_table;
        let wide_codes = flags & WIDE_CODES != 0;
        for _ in 0..n {
            f.codes.push(if wide_codes { u16_at(d, pos)? } else { *d.get(pos)? as u16 });
            pos += if wide_codes { 2 } else { 1 };
        }
        if flags & HAS_LAYOUT != 0 {
            let mut l = Layout { ascent: u16_at(d, pos)?, descent: u16_at(d, pos + 2)?, leading: u16_at(d, pos + 4)? as i16, ..Layout::default() };
            pos += 6;
            for _ in 0..n {
                l.advances.push(u16_at(d, pos)? as i16);
                pos += 2;
            }
            for _ in 0..n {
                let (r, next) = read_rect(d, pos)?;
                l.bounds.push(r);
                pos = next;
            }
            let k = u16_at(d, pos)? as usize;
            pos += 2;
            for _ in 0..k {
                let (a, b) = if wide_codes { (u16_at(d, pos)?, u16_at(d, pos + 2)?) } else { (*d.get(pos)? as u16, *d.get(pos + 1)? as u16) };
                pos += if wide_codes { 4 } else { 2 };
                l.kerning.push((a, b, u16_at(d, pos)? as i16));
                pos += 2;
            }
            f.layout = Some(l);
        }
        Some(f)
    }

    /// The `DefineFont2`/`DefineFont3` tag. Offsets widen when the glyphs need it.
    pub fn write(&self) -> Result<Tag> {
        if self.code == 10 {
            return Err(Error::Format("DefineFont (version 1) fonts cannot be rewritten; only DefineFont2/3".into()));
        }
        let n = self.glyphs.len();
        let shapes: usize = self.glyphs.iter().map(Vec::len).sum();
        let mut flags = self.flags;
        let narrow_size = (n + 1) * 2 + shapes;
        if narrow_size > 0xFFFF {
            flags |= WIDE_OFFSETS;
        }
        if self.codes.iter().any(|&c| c > 0xFF) || self.code == 75 {
            flags |= WIDE_CODES;
        }
        let wide = flags & WIDE_OFFSETS != 0;
        let wide_codes = flags & WIDE_CODES != 0;
        let mut d = self.id.to_le_bytes().to_vec();
        d.push(flags);
        d.push(self.language);
        d.push(self.name.len() as u8);
        d.extend_from_slice(&self.name);
        d.extend_from_slice(&(n as u16).to_le_bytes());
        let entry = if wide { 4 } else { 2 };
        let mut at = (n + 1) * entry;
        // A font without glyphs keeps no offset table (device fonts).
        if n > 0 || self.layout.is_some() {
            for g in &self.glyphs {
                if wide { d.extend_from_slice(&(at as u32).to_le_bytes()) } else { d.extend_from_slice(&(at as u16).to_le_bytes()) }
                at += g.len();
            }
            if wide { d.extend_from_slice(&(at as u32).to_le_bytes()) } else { d.extend_from_slice(&(at as u16).to_le_bytes()) }
            for g in &self.glyphs {
                d.extend_from_slice(g);
            }
            for &c in &self.codes {
                if wide_codes { d.extend_from_slice(&c.to_le_bytes()) } else { d.push(c as u8) }
            }
        }
        if let Some(l) = &self.layout {
            d.extend_from_slice(&l.ascent.to_le_bytes());
            d.extend_from_slice(&l.descent.to_le_bytes());
            d.extend_from_slice(&l.leading.to_le_bytes());
            for a in &l.advances {
                d.extend_from_slice(&a.to_le_bytes());
            }
            for r in &l.bounds {
                let mut bw = BitWriter::default();
                let nb = r.iter().map(|&v| sbits(v)).max().unwrap_or(1);
                bw.bits(5, nb as u32);
                for v in [r[0], r[2], r[1], r[3]] {
                    bw.signed(nb, v);
                }
                d.extend_from_slice(&bw.finish());
            }
            d.extend_from_slice(&(l.kerning.len() as u16).to_le_bytes());
            for (a, b, k) in &l.kerning {
                if wide_codes {
                    d.extend_from_slice(&a.to_le_bytes());
                    d.extend_from_slice(&b.to_le_bytes());
                } else {
                    d.push(*a as u8);
                    d.push(*b as u8);
                }
                d.extend_from_slice(&k.to_le_bytes());
            }
        }
        Ok(Tag { code: self.code, data: d, long: true })
    }

    /// A glyph's filled contours (fill side on the right), font units.
    pub fn contours(&self, i: usize) -> Vec<Vec<Edge>> {
        let Some(s) = self.glyphs.get(i).and_then(|g| Shape::glyph(g)) else { return Vec::new() };
        s.paths().into_iter().flat_map(|p| p.contours).collect()
    }

    /// Add the characters of `chars` that the font lacks, from a TrueType or
    /// OpenType font file, in code order. Returns the characters added, those
    /// the file lacks too, and the old → new glyph index map.
    pub fn add_glyphs(&mut self, file: &[u8], chars: &str) -> Result<(Vec<char>, Vec<char>, Vec<usize>)> {
        let face = ttf_parser::Face::parse(file, 0).map_err(|e| Error::Format(format!("not a TrueType/OpenType font: {e}")))?;
        let scale = self.em() / face.units_per_em() as f64;
        let mut added = Vec::new();
        let mut missing = Vec::new();
        let mut new: Vec<(u16, Vec<u8>, i16, [i32; 4])> = Vec::new();
        for c in chars.chars() {
            let Ok(code) = u16::try_from(c as u32) else {
                missing.push(c);
                continue;
            };
            if self.codes.contains(&code) || new.iter().any(|n| n.0 == code) || added.contains(&c) {
                continue;
            }
            let Some(g) = face.glyph_index(c) else {
                missing.push(c);
                continue;
            };
            let mut b = Outline { scale, contours: Vec::new(), cur: Vec::new(), at: (0.0, 0.0) };
            let rect = face.outline_glyph(g, &mut b);
            b.close();
            let advance = (face.glyph_hor_advance(g).unwrap_or(0) as f64 * scale).round() as i16;
            let bounds = rect.map(|r| [(r.x_min as f64 * scale).round() as i32, (-(r.y_max as f64) * scale).round() as i32, (r.x_max as f64 * scale).round() as i32, (-(r.y_min as f64) * scale).round() as i32]).unwrap_or([0; 4]);
            new.push((code, write_glyph(&b.contours), advance, bounds));
            added.push(c);
        }
        // Merge in code order; the old glyphs keep their relative order.
        let n = self.glyphs.len();
        // (code, old index, shape, advance, bounds)
        type Glyph = (u16, Option<usize>, Vec<u8>, i16, [i32; 4]);
        let mut all: Vec<Glyph> = Vec::with_capacity(n + new.len());
        for i in 0..n {
            let (adv, b) = self.layout.as_ref().map(|l| (l.advances.get(i).copied().unwrap_or(0), l.bounds.get(i).copied().unwrap_or([0; 4]))).unwrap_or((0, [0; 4]));
            all.push((self.codes[i], Some(i), std::mem::take(&mut self.glyphs[i]), adv, b));
        }
        for (code, shape, adv, b) in new {
            all.push((code, None, shape, adv, b));
        }
        all.sort_by_key(|g| g.0);
        let mut remap = vec![0usize; n];
        self.glyphs.clear();
        self.codes.clear();
        let mut advances = Vec::new();
        let mut bounds = Vec::new();
        for (k, (code, old, shape, adv, b)) in all.into_iter().enumerate() {
            if let Some(o) = old {
                remap[o] = k;
            }
            self.codes.push(code);
            self.glyphs.push(shape);
            advances.push(adv);
            bounds.push(b);
        }
        if let Some(l) = &mut self.layout {
            l.advances = advances;
            l.bounds = bounds;
        } else if !added.is_empty() && self.code != 10 {
            // Texts need advances: a font that had none gets a layout, its
            // old glyphs measured from their outlines.
            for &new in &remap {
                let pts: Vec<Pt> = self.contours(new).iter().flat_map(|c| super::shape::flatten(c)).collect();
                advances[new] = if pts.is_empty() {
                    (self.em() * 0.25) as i16
                } else {
                    let (x0, x1) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
                    let (y0, y1) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
                    bounds[new] = [x0 as i32, y0 as i32, x1 as i32, y1 as i32];
                    (x1 + x0.max(self.em() * 0.04)).round().clamp(0.0, 32767.0) as i16
                };
            }
            let asc = face.ascender() as f64 * scale;
            let desc = -(face.descender() as f64) * scale;
            self.layout = Some(Layout { ascent: asc.round() as u16, descent: desc.round() as u16, leading: (face.line_gap() as f64 * scale).round() as i16, advances, bounds, kerning: Vec::new() });
            self.flags |= HAS_LAYOUT;
        }
        Ok((added, missing, remap))
    }
}

fn sbits(v: i32) -> usize {
    let mag = if v < 0 { !v } else { v } as u32;
    (32 - mag.leading_zeros()) as usize + 1
}

/// Collects a glyph outline as SWF contours: scaled to the font's EM, y down,
/// cubic curves split into quadratic ones.
struct Outline {
    scale: f64,
    contours: Vec<Vec<Edge>>,
    cur: Vec<Edge>,
    at: Pt,
}

impl Outline {
    fn pt(&self, x: f32, y: f32) -> Pt {
        (x as f64 * self.scale, -(y as f64) * self.scale)
    }

    fn close(&mut self) {
        if let (Some(first), Some(last)) = (self.cur.first().map(|e| e.from), self.cur.last().map(|e| e.to)) {
            if (first.0 - last.0).abs() > 0.5 || (first.1 - last.1).abs() > 0.5 {
                self.cur.push(Edge { from: last, ctrl: None, to: first });
            }
        }
        if !self.cur.is_empty() {
            self.contours.push(std::mem::take(&mut self.cur));
        }
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.at = self.pt(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.pt(x, y);
        self.cur.push(Edge { from: self.at, ctrl: None, to });
        self.at = to;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let to = self.pt(x, y);
        self.cur.push(Edge { from: self.at, ctrl: Some(self.pt(x1, y1)), to });
        self.at = to;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, to) = (self.pt(x1, y1), self.pt(x2, y2), self.pt(x, y));
        let from = self.at;
        self.cur.extend(cubic_to_quads(from, c1, c2, to));
        self.at = to;
    }
    fn close(&mut self) {
        Outline::close(self);
    }
}

/// Quadratic curves close to a cubic one (split in halves until the
/// midpoint-control approximation is within a twip or two).
pub fn cubic_to_quads(p0: Pt, p1: Pt, p2: Pt, p3: Pt) -> Vec<Edge> {
    fn go(p0: Pt, p1: Pt, p2: Pt, p3: Pt, depth: usize, out: &mut Vec<Edge>) {
        // Control point of the quadratic through the cubic's ends.
        let q = ((3.0 * (p1.0 + p2.0) - p0.0 - p3.0) / 4.0, (3.0 * (p1.1 + p2.1) - p0.1 - p3.1) / 4.0);
        // Distance between the two curves is bounded by √3/36 · |p3 - 3p2 + 3p1 - p0|.
        let ex = p3.0 - 3.0 * p2.0 + 3.0 * p1.0 - p0.0;
        let ey = p3.1 - 3.0 * p2.1 + 3.0 * p1.1 - p0.1;
        if depth >= 6 || (ex * ex + ey * ey).sqrt() * 0.048 < 2.0 {
            out.push(Edge { from: p0, ctrl: Some(q), to: p3 });
            return;
        }
        let mid = |a: Pt, b: Pt| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let (a, b, c) = (mid(p0, p1), mid(p1, p2), mid(p2, p3));
        let (d, e) = (mid(a, b), mid(b, c));
        let m = mid(d, e);
        go(p0, a, d, m, depth + 1, out);
        go(m, e, c, p3, depth + 1, out);
    }
    let mut out = Vec::new();
    go(p0, p1, p2, p3, 0, &mut out);
    out
}

// ----------------------------------------------------------------------------
// TrueType export
// ----------------------------------------------------------------------------

struct Writer(Vec<u8>);

impl Writer {
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
}

fn checksum(d: &[u8]) -> u32 {
    d.chunks(4).fold(0u32, |acc, c| {
        let mut b = [0u8; 4];
        b[..c.len()].copy_from_slice(c);
        acc.wrapping_add(u32::from_be_bytes(b))
    })
}

impl Font {
    /// The font as a TrueType file (1024 units per EM, glyph 0 `.notdef`).
    pub fn to_ttf(&self) -> Result<Vec<u8>> {
        if self.glyphs.is_empty() {
            return Err(Error::Format(format!("{} has no glyphs (a device font)", self.name())));
        }
        let k = self.em() / 1024.0;
        // Glyph outlines: y up, contours reversed (TrueType fills the right side going clockwise).
        struct G {
            contours: Vec<Vec<(i16, i16, bool)>>,
            bbox: [i16; 4],
            advance: u16,
        }
        let mut glyphs = vec![G { contours: Vec::new(), bbox: [0; 4], advance: 512 }];
        for i in 0..self.glyphs.len() {
            let mut contours = Vec::new();
            let mut bbox = [i16::MAX, i16::MAX, i16::MIN, i16::MIN];
            for c in self.contours(i) {
                let mut pts: Vec<(i16, i16, bool)> = Vec::new();
                for e in c.iter().rev() {
                    let e = e.reversed();
                    let p = |q: Pt| ((q.0 / k).round() as i16, (-q.1 / k).round() as i16);
                    let (x, y) = p(e.from);
                    pts.push((x, y, true));
                    if let Some(q) = e.ctrl {
                        let (x, y) = p(q);
                        pts.push((x, y, false));
                    }
                }
                for &(x, y, _) in &pts {
                    bbox = [bbox[0].min(x), bbox[1].min(y), bbox[2].max(x), bbox[3].max(y)];
                }
                if pts.len() >= 2 {
                    contours.push(pts);
                }
            }
            if contours.is_empty() {
                bbox = [0; 4];
            }
            let advance = self.layout.as_ref().and_then(|l| l.advances.get(i)).map(|&a| (a as f64 / k).round().max(0.0) as u16).unwrap_or((bbox[2].max(0) as u16).saturating_add(50));
            glyphs.push(G { contours, bbox, advance });
        }
        let n = glyphs.len();

        // glyf + loca
        let mut glyf = Writer(Vec::new());
        let mut loca = Writer(Vec::new());
        for g in &glyphs {
            loca.u32(glyf.0.len() as u32);
            if g.contours.is_empty() {
                continue;
            }
            glyf.i16(g.contours.len() as i16);
            for v in g.bbox {
                glyf.i16(v);
            }
            let mut end = 0u16;
            for c in &g.contours {
                end += c.len() as u16;
                glyf.u16(end - 1);
            }
            glyf.u16(0); // no instructions
            let pts: Vec<&(i16, i16, bool)> = g.contours.iter().flatten().collect();
            for p in &pts {
                glyf.0.push(if p.2 { 1 } else { 0 });
            }
            let (mut px, mut py) = (0i16, 0i16);
            for p in &pts {
                glyf.i16(p.0.wrapping_sub(px));
                px = p.0;
            }
            for p in &pts {
                glyf.i16(p.1.wrapping_sub(py));
                py = p.1;
            }
            while !glyf.0.len().is_multiple_of(4) {
                glyf.0.push(0);
            }
        }
        loca.u32(glyf.0.len() as u32);

        let bbox = glyphs.iter().filter(|g| !g.contours.is_empty()).fold([i16::MAX, i16::MAX, i16::MIN, i16::MIN], |b, g| [b[0].min(g.bbox[0]), b[1].min(g.bbox[1]), b[2].max(g.bbox[2]), b[3].max(g.bbox[3])]);
        let (ascent, descent, gap) = match &self.layout {
            Some(l) => ((l.ascent as f64 / k).round() as i16, -((l.descent as f64 / k).round() as i16), (l.leading as f64 / k).round() as i16),
            None => (bbox[3], bbox[1], 0),
        };
        let max_advance = glyphs.iter().map(|g| g.advance).max().unwrap_or(0);
        let max_points = glyphs.iter().map(|g| g.contours.iter().map(Vec::len).sum::<usize>()).max().unwrap_or(0);
        let max_contours = glyphs.iter().map(|g| g.contours.len()).max().unwrap_or(0);

        let mut head = Writer(Vec::new());
        head.u32(0x0001_0000);
        head.u32(0x0001_0000);
        head.u32(0); // checksum adjustment, set at the end
        head.u32(0x5F0F_3CF5);
        head.u16(0x000B);
        head.u16(1024);
        head.0.extend_from_slice(&[0; 16]); // created, modified
        for v in bbox {
            head.i16(v);
        }
        head.u16(if self.bold() { 1 } else { 0 } | if self.italic() { 2 } else { 0 });
        head.u16(8);
        head.i16(2);
        head.i16(1); // long loca
        head.i16(0);

        let mut hhea = Writer(Vec::new());
        hhea.u32(0x0001_0000);
        hhea.i16(ascent);
        hhea.i16(descent);
        hhea.i16(gap);
        hhea.u16(max_advance);
        hhea.i16(glyphs.iter().map(|g| g.bbox[0]).min().unwrap_or(0));
        hhea.i16(glyphs.iter().map(|g| g.advance as i16 - g.bbox[2]).min().unwrap_or(0));
        hhea.i16(bbox[2]);
        hhea.i16(1);
        hhea.i16(0);
        hhea.i16(0);
        hhea.0.extend_from_slice(&[0; 8]);
        hhea.i16(0);
        hhea.u16(n as u16);

        let mut maxp = Writer(Vec::new());
        maxp.u32(0x0001_0000);
        maxp.u16(n as u16);
        maxp.u16(max_points as u16);
        maxp.u16(max_contours as u16);
        maxp.u16(0);
        maxp.u16(0);
        maxp.u16(2);
        for _ in 0..8 {
            maxp.u16(0);
        }

        let mut hmtx = Writer(Vec::new());
        for g in &glyphs {
            hmtx.u16(g.advance);
            hmtx.i16(g.bbox[0]);
        }

        // cmap: format 4, one segment per code (glyph = code + delta), then 0xFFFF.
        let mut map: Vec<(u16, u16)> = self.codes.iter().enumerate().map(|(i, &c)| (c, i as u16 + 1)).filter(|(c, _)| *c != 0xFFFF).collect();
        map.sort();
        map.dedup_by_key(|m| m.0);
        let seg = map.len() + 1;
        let mut sub = Writer(Vec::new());
        sub.u16(4);
        sub.u16((16 + seg * 8) as u16);
        sub.u16(0);
        sub.u16((seg * 2) as u16);
        let p = 1u16 << (15 - (seg as u16).leading_zeros());
        sub.u16(p * 2);
        sub.u16(p.trailing_zeros() as u16);
        sub.u16((seg as u16 * 2).wrapping_sub(p * 2));
        for (c, _) in &map {
            sub.u16(*c);
        }
        sub.u16(0xFFFF);
        sub.u16(0);
        for (c, _) in &map {
            sub.u16(*c);
        }
        sub.u16(0xFFFF);
        for (c, g) in &map {
            sub.u16(g.wrapping_sub(*c));
        }
        sub.u16(1);
        for _ in 0..seg {
            sub.u16(0);
        }
        let mut cmap = Writer(Vec::new());
        cmap.u16(0);
        cmap.u16(2);
        for (platform, encoding) in [(0u16, 3u16), (3, 1)] {
            cmap.u16(platform);
            cmap.u16(encoding);
            cmap.u32(20);
        }
        cmap.0.extend_from_slice(&sub.0);

        let family = self.name();
        let family = if family.is_empty() { format!("Font{}", self.id) } else { family };
        let style = match (self.bold(), self.italic()) {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            _ => "Regular",
        };
        let ps: String = format!("{family}-{style}").chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        let strings = [(1u16, family.clone()), (2, style.to_string()), (4, format!("{family} {style}")), (6, ps)];
        let mut name = Writer(Vec::new());
        name.u16(0);
        name.u16(strings.len() as u16);
        name.u16((6 + strings.len() * 12) as u16);
        let mut storage = Vec::new();
        for (id, s) in &strings {
            let bytes: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
            name.u16(3);
            name.u16(1);
            name.u16(0x0409);
            name.u16(*id);
            name.u16(bytes.len() as u16);
            name.u16(storage.len() as u16);
            storage.extend_from_slice(&bytes);
        }
        name.0.extend_from_slice(&storage);

        let mut os2 = Writer(Vec::new());
        let avg = (glyphs.iter().map(|g| g.advance as u32).sum::<u32>() / n as u32) as i16;
        os2.u16(4);
        os2.i16(avg);
        os2.u16(if self.bold() { 700 } else { 400 });
        os2.u16(5);
        os2.u16(0);
        for v in [650i16, 600, 0, 75, 650, 600, 0, 350, 50, 250] {
            os2.i16(v);
        }
        os2.i16(0);
        os2.0.extend_from_slice(&[0; 10]); // panose
        os2.0.extend_from_slice(&[0; 16]); // unicode ranges
        os2.0.extend_from_slice(b"TMPS");
        os2.u16(if self.bold() { 0x20 } else { 0 } | if self.italic() { 0x01 } else { 0 } | if !self.bold() && !self.italic() { 0x40 } else { 0 });
        os2.u16(map.first().map(|m| m.0).unwrap_or(0));
        os2.u16(map.last().map(|m| m.0).unwrap_or(0));
        os2.i16(ascent);
        os2.i16(descent);
        os2.i16(gap);
        os2.u16(bbox[3].max(ascent).max(0) as u16);
        os2.u16((-bbox[1].min(descent)).max(0) as u16);
        os2.u32(1);
        os2.u32(0);
        os2.i16((ascent as f64 * 0.5) as i16);
        os2.i16((ascent as f64 * 0.7) as i16);
        os2.u16(0);
        os2.u16(32);
        os2.u16(0);

        let mut post = Writer(Vec::new());
        post.u32(0x0003_0000);
        post.u32(if self.italic() { (-12i32 << 16) as u32 } else { 0 });
        post.i16(-100);
        post.i16(50);
        for _ in 0..5 {
            post.u32(0);
        }

        let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
            (b"OS/2", os2.0),
            (b"cmap", cmap.0),
            (b"glyf", glyf.0),
            (b"head", head.0),
            (b"hhea", hhea.0),
            (b"hmtx", hmtx.0),
            (b"loca", loca.0),
            (b"maxp", maxp.0),
            (b"name", name.0),
            (b"post", post.0),
        ];
        tables.sort_by_key(|t| *t.0);
        let count = tables.len() as u16;
        let mut out = Writer(Vec::new());
        out.u32(0x0001_0000);
        out.u16(count);
        let p = 1u16 << (15 - count.leading_zeros());
        out.u16(p * 16);
        out.u16(p.trailing_zeros() as u16);
        out.u16(count * 16 - p * 16);
        let mut offset = 12 + 16 * tables.len();
        let mut head_at = 0;
        for (tag, data) in &tables {
            out.0.extend_from_slice(*tag);
            out.u32(checksum(data));
            out.u32(offset as u32);
            out.u32(data.len() as u32);
            if *tag == b"head" {
                head_at = offset;
            }
            offset += data.len().div_ceil(4) * 4;
        }
        for (_, data) in &tables {
            out.0.extend_from_slice(data);
            while !out.0.len().is_multiple_of(4) {
                out.0.push(0);
            }
        }
        let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out.0));
        out.0[head_at + 8..head_at + 12].copy_from_slice(&adjust.to_be_bytes());
        Ok(out.0)
    }
}

/// `DefineFontAlignZones` of a font, with zone records for added glyphs
/// (no zones: no hinting) at their new indices.
pub fn remap_align_zones(t: &mut Tag, remap: &[usize], total: usize) -> Result<()> {
    let head = t.data.get(..3).ok_or_else(|| Error::Format("bad DefineFontAlignZones".into()))?.to_vec();
    let mut records: Vec<Vec<u8>> = Vec::new();
    let mut pos = 3;
    while pos < t.data.len() {
        let n = t.data[pos] as usize;
        let len = 1 + n * 4 + 1;
        records.push(t.data.get(pos..pos + len).ok_or_else(|| Error::Format("bad zone record".into()))?.to_vec());
        pos += len;
    }
    let mut out: Vec<Vec<u8>> = vec![vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0]; total];
    for (old, rec) in records.into_iter().enumerate() {
        if let Some(&new) = remap.get(old) {
            out[new] = rec;
        }
    }
    t.data = head;
    for r in out {
        t.data.extend_from_slice(&r);
    }
    t.long = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_what_it_reads_and_exports_truetype() {
        let square = |x: f64| -> Vec<u8> {
            let p = [(x, -700.0), (x + 500.0, -700.0), (x + 500.0, 0.0), (x, 0.0)];
            write_glyph(&[(0..4).map(|i| Edge { from: p[i], ctrl: None, to: p[(i + 1) % 4] }).collect()])
        };
        let f = Font {
            code: 48,
            id: 3,
            flags: HAS_LAYOUT | WIDE_CODES,
            language: 1,
            name: b"Test\0".to_vec(),
            glyphs: vec![square(0.0), square(100.0)],
            codes: vec![65, 67],
            layout: Some(Layout { ascent: 800, descent: 200, leading: 10, advances: vec![600, 650], bounds: vec![[0, -700, 500, 0], [100, -700, 600, 0]], kerning: vec![(65, 67, -20)] }),
        };
        let t = f.write().unwrap();
        let m = Movie { signature: *b"FWS", version: 8, header: vec![0, 0, 24, 1, 0], tags: vec![t.clone()] };
        let g = Font::parse(&m, 0).unwrap();
        assert_eq!(g.codes, vec![65, 67]);
        assert_eq!(g.layout.as_ref().unwrap().kerning, vec![(65, 67, -20)]);
        assert_eq!(g.write().unwrap().data, t.data);
        assert_eq!(g.contours(1).len(), 1);
        let ttf = g.to_ttf().unwrap();
        let face = ttf_parser::Face::parse(&ttf, 0).unwrap();
        assert_eq!(face.number_of_glyphs(), 3);
        assert_eq!(face.glyph_index('C').map(|g| g.0), Some(2));
        assert_eq!(face.glyph_hor_advance(ttf_parser::GlyphId(2)), Some(650));
        let r = face.glyph_bounding_box(ttf_parser::GlyphId(1)).unwrap();
        assert_eq!((r.x_min, r.y_min, r.x_max, r.y_max), (0, 0, 500, 700));

        // Glyphs from the exported file: 'B' lands between 'A' and 'C'.
        let (added, missing, remap) = g.clone().add_glyphs(&ttf, "AB").unwrap();
        assert!(added.is_empty());
        assert_eq!(missing, vec!['B']);
        assert_eq!(remap, vec![0, 1]);
        let mut k = g.clone();
        k.codes = vec![66, 68];
        let ttf2 = k.to_ttf().unwrap();
        let mut j = g.clone();
        let (added, _, remap) = j.add_glyphs(&ttf2, "B").unwrap();
        assert_eq!(added, vec!['B']);
        assert_eq!(j.codes, vec![65, 66, 67]);
        assert_eq!(remap, vec![0, 2]);
        assert_eq!(j.layout.as_ref().unwrap().advances, vec![600, 600, 650]);
        assert_eq!(j.contours(1).len(), 1);
    }
}
