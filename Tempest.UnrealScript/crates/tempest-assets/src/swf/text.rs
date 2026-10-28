//! Texts and media: every field of a `DefineEditText` (read and rewritten),
//! static texts (`DefineText`/`DefineText2`: records of glyph indices into a
//! font, decoded to characters and re-encoded with the font's advances),
//! and sound (`DefineSound`) and binary data (`DefineBinaryData`) export.

use serde_json::{json, Map, Value as J};
use tempest_upk::io::{Error, Result};

use super::dict::read_rect;
use super::display::{BitWriter, Matrix};
use super::font::Font;
use super::shape::Rgba;
use super::{u16_at, u32_at, BitReader, Movie, Tag};

fn cstr(d: &[u8], at: usize) -> Option<(String, usize)> {
    let end = d.get(at..)?.iter().position(|&b| b == 0)? + at;
    Some((String::from_utf8_lossy(&d[at..end]).to_string(), end + 1))
}

fn sbits(v: i32) -> usize {
    let mag = if v < 0 { !v } else { v } as u32;
    (32 - mag.leading_zeros()) as usize + 1
}

fn write_rect(out: &mut Vec<u8>, r: [i32; 4]) {
    let mut bw = BitWriter::default();
    let n = r.iter().map(|&v| sbits(v)).max().unwrap_or(1);
    bw.bits(5, n as u32);
    for v in [r[0], r[2], r[1], r[3]] {
        bw.signed(n, v);
    }
    out.extend_from_slice(&bw.finish());
}

// ----------------------------------------------------------------------------
// DefineEditText
// ----------------------------------------------------------------------------

/// Flag bits (the two flag bytes read big-endian).
const FLAGS: &[(&str, u16)] = &[
    ("wordWrap", 0x4000),
    ("multiline", 0x2000),
    ("password", 0x1000),
    ("readOnly", 0x0800),
    ("autoSize", 0x0040),
    ("noSelect", 0x0010),
    ("border", 0x0008),
    ("wasStatic", 0x0004),
    ("html", 0x0002),
    ("useOutlines", 0x0001),
];
const HAS_TEXT: u16 = 0x8000;
const HAS_COLOR: u16 = 0x0400;
const HAS_MAX_LENGTH: u16 = 0x0200;
const HAS_FONT: u16 = 0x0100;
const HAS_FONT_CLASS: u16 = 0x0080;
const HAS_LAYOUT: u16 = 0x0020;
const ALIGNS: [&str; 4] = ["left", "right", "center", "justify"];

#[derive(Clone, Debug)]
pub struct EditText {
    pub id: u16,
    pub bounds: [i32; 4],
    pub flags: u16,
    pub font: Option<u16>,
    pub font_class: Option<String>,
    pub height: Option<u16>,
    pub color: Option<Rgba>,
    pub max_length: Option<u16>,
    /// Align, left margin, right margin, indent (twips), leading.
    pub layout: Option<(u8, u16, u16, u16, i16)>,
    pub variable: String,
    pub text: Option<String>,
}

impl EditText {
    pub fn parse(d: &[u8]) -> Option<EditText> {
        let id = u16_at(d, 0)?;
        let (bounds, mut pos) = read_rect(d, 2)?;
        let flags = u16::from_be_bytes([*d.get(pos)?, *d.get(pos + 1)?]);
        pos += 2;
        let mut e = EditText { id, bounds, flags, font: None, font_class: None, height: None, color: None, max_length: None, layout: None, variable: String::new(), text: None };
        if flags & HAS_FONT != 0 {
            e.font = Some(u16_at(d, pos)?);
            pos += 2;
        }
        if flags & HAS_FONT_CLASS != 0 {
            let (s, next) = cstr(d, pos)?;
            e.font_class = Some(s);
            pos = next;
        }
        if flags & (HAS_FONT | HAS_FONT_CLASS) != 0 {
            e.height = Some(u16_at(d, pos)?);
            pos += 2;
        }
        if flags & HAS_COLOR != 0 {
            e.color = Some(Rgba { r: *d.get(pos)?, g: *d.get(pos + 1)?, b: *d.get(pos + 2)?, a: *d.get(pos + 3)? });
            pos += 4;
        }
        if flags & HAS_MAX_LENGTH != 0 {
            e.max_length = Some(u16_at(d, pos)?);
            pos += 2;
        }
        if flags & HAS_LAYOUT != 0 {
            e.layout = Some((*d.get(pos)?, u16_at(d, pos + 1)?, u16_at(d, pos + 3)?, u16_at(d, pos + 5)?, u16_at(d, pos + 7)? as i16));
            pos += 9;
        }
        let (v, next) = cstr(d, pos)?;
        e.variable = v;
        pos = next;
        if flags & HAS_TEXT != 0 {
            e.text = Some(cstr(d, pos)?.0);
        }
        Some(e)
    }

    pub fn write(&self) -> Vec<u8> {
        let mut flags = self.flags & !(HAS_TEXT | HAS_COLOR | HAS_MAX_LENGTH | HAS_FONT | HAS_FONT_CLASS | HAS_LAYOUT);
        flags |= if self.text.is_some() { HAS_TEXT } else { 0 }
            | if self.color.is_some() { HAS_COLOR } else { 0 }
            | if self.max_length.is_some() { HAS_MAX_LENGTH } else { 0 }
            | if self.font.is_some() { HAS_FONT } else { 0 }
            | if self.font_class.is_some() { HAS_FONT_CLASS } else { 0 }
            | if self.layout.is_some() { HAS_LAYOUT } else { 0 };
        let mut d = self.id.to_le_bytes().to_vec();
        write_rect(&mut d, self.bounds);
        d.extend_from_slice(&flags.to_be_bytes());
        if let Some(f) = self.font {
            d.extend_from_slice(&f.to_le_bytes());
        }
        if let Some(c) = &self.font_class {
            d.extend_from_slice(c.as_bytes());
            d.push(0);
        }
        if self.font.is_some() || self.font_class.is_some() {
            d.extend_from_slice(&self.height.unwrap_or(240).to_le_bytes());
        }
        if let Some(c) = self.color {
            d.extend_from_slice(&[c.r, c.g, c.b, c.a]);
        }
        if let Some(m) = self.max_length {
            d.extend_from_slice(&m.to_le_bytes());
        }
        if let Some((a, l, r, i, lead)) = self.layout {
            d.push(a);
            d.extend_from_slice(&l.to_le_bytes());
            d.extend_from_slice(&r.to_le_bytes());
            d.extend_from_slice(&i.to_le_bytes());
            d.extend_from_slice(&lead.to_le_bytes());
        }
        d.extend_from_slice(self.variable.as_bytes());
        d.push(0);
        if let Some(t) = &self.text {
            d.extend_from_slice(t.as_bytes());
            d.push(0);
        }
        d
    }

    /// Every property, pixels for sizes.
    pub fn json(&self) -> J {
        let px = |v: f64| v / 20.0;
        let mut o = Map::new();
        o.insert("bounds".into(), super::dict::rect_json(self.bounds));
        o.insert("font".into(), json!(self.font));
        o.insert("fontClass".into(), json!(self.font_class));
        o.insert("height".into(), json!(self.height.map(|h| px(h as f64))));
        o.insert("color".into(), json!(self.color.map(|c| c.hex())));
        o.insert("maxLength".into(), json!(self.max_length));
        let (align, left, right, indent, leading) = self.layout.unwrap_or((0, 0, 0, 0, 0));
        o.insert("layout".into(), json!(self.layout.is_some()));
        o.insert("align".into(), json!(ALIGNS.get(align as usize).copied().unwrap_or("left")));
        o.insert("leftMargin".into(), json!(px(left as f64)));
        o.insert("rightMargin".into(), json!(px(right as f64)));
        o.insert("indent".into(), json!(px(indent as f64)));
        o.insert("leading".into(), json!(px(leading as f64)));
        o.insert("variable".into(), json!(self.variable));
        for (name, bit) in FLAGS {
            o.insert((*name).into(), json!(self.flags & bit != 0));
        }
        J::Object(o)
    }

    /// Set the properties `p` names (as `json` writes them).
    pub fn set(&mut self, p: &J) -> Result<()> {
        let twips = |v: &J| v.as_f64().map(|x| (x * 20.0).round());
        for (name, bit) in FLAGS {
            if let Some(b) = p.get(*name).and_then(J::as_bool) {
                self.flags = if b { self.flags | bit } else { self.flags & !bit };
            }
        }
        if let Some(v) = p.get("font") {
            self.font = v.as_u64().map(|f| f as u16);
            if self.font.is_some() {
                self.font_class = None;
            }
        }
        if let Some(v) = p.get("fontClass") {
            self.font_class = v.as_str().filter(|s| !s.is_empty()).map(str::to_string);
            if self.font_class.is_some() {
                self.font = None;
            }
        }
        if let Some(h) = p.get("height").and_then(twips) {
            self.height = Some(h.clamp(0.0, 65535.0) as u16);
        }
        if let Some(v) = p.get("color") {
            self.color = match v.as_str() {
                Some(s) => Some(Rgba::parse(s).ok_or_else(|| Error::Format(format!("{s} is not a colour")))?),
                None => None,
            };
        }
        if let Some(v) = p.get("maxLength") {
            self.max_length = v.as_u64().map(|m| m.min(65535) as u16);
        }
        let layout_keys = ["align", "leftMargin", "rightMargin", "indent", "leading"];
        if layout_keys.iter().any(|k| p.get(*k).is_some()) || p.get("layout").and_then(J::as_bool) == Some(true) {
            let (mut a, mut l, mut r, mut i, mut lead) = self.layout.unwrap_or((0, 0, 0, 0, 0));
            if let Some(s) = p.get("align").and_then(J::as_str) {
                a = ALIGNS.iter().position(|x| *x == s).ok_or_else(|| Error::Format(format!("align {s}: left, right, center or justify")))? as u8;
            }
            let u = |v: Option<f64>, cur: u16| v.map(|x| x.clamp(0.0, 65535.0) as u16).unwrap_or(cur);
            l = u(p.get("leftMargin").and_then(twips), l);
            r = u(p.get("rightMargin").and_then(twips), r);
            i = u(p.get("indent").and_then(twips), i);
            lead = p.get("leading").and_then(twips).map(|x| x.clamp(-32768.0, 32767.0) as i16).unwrap_or(lead);
            self.layout = Some((a, l, r, i, lead));
        }
        if p.get("layout").and_then(J::as_bool) == Some(false) {
            self.layout = None;
        }
        if let Some(v) = p.get("variable").and_then(J::as_str) {
            self.variable = v.to_string();
        }
        if let Some(b) = p.get("bounds").filter(|b| b.is_object()) {
            let g = |k: &str, cur: i32| b.get(k).and_then(J::as_f64).map(|v| (v * 20.0).round() as i32).unwrap_or(cur);
            let (x, y) = (g("x", self.bounds[0]), g("y", self.bounds[1]));
            let (w, h) = (g("w", self.bounds[2] - self.bounds[0]), g("h", self.bounds[3] - self.bounds[1]));
            self.bounds = [x, y, x + w.max(0), y + h.max(0)];
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// DefineText / DefineText2
// ----------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct TextRecord {
    pub font: Option<u16>,
    pub color: Option<Rgba>,
    pub x: Option<i16>,
    pub y: Option<i16>,
    pub height: Option<u16>,
    /// Glyph index and advance (twips).
    pub glyphs: Vec<(u32, i32)>,
}

#[derive(Clone, Debug)]
pub struct StaticText {
    pub code: u16,
    pub id: u16,
    pub bounds: [i32; 4],
    pub matrix: Matrix,
    pub records: Vec<TextRecord>,
}

impl StaticText {
    pub fn parse(t: &Tag) -> Option<StaticText> {
        if t.code != 11 && t.code != 33 {
            return None;
        }
        let d = &t.data;
        let id = u16_at(d, 0)?;
        let (bounds, mut pos) = read_rect(d, 2)?;
        let mut br = BitReader { d: d.get(pos..)?, pos: 0 };
        let matrix = Matrix::read(&mut br);
        pos += br.byte_pos();
        let gbits = *d.get(pos)? as usize;
        let abits = *d.get(pos + 1)? as usize;
        pos += 2;
        let mut records = Vec::new();
        while let Some(&flags) = d.get(pos) {
            if flags == 0 {
                break;
            }
            pos += 1;
            let mut r = TextRecord::default();
            if flags & 0x08 != 0 {
                r.font = Some(u16_at(d, pos)?);
                pos += 2;
            }
            if flags & 0x04 != 0 {
                r.color = Some(Rgba { r: *d.get(pos)?, g: *d.get(pos + 1)?, b: *d.get(pos + 2)?, a: if t.code == 33 { *d.get(pos + 3)? } else { 255 } });
                pos += if t.code == 33 { 4 } else { 3 };
            }
            if flags & 0x01 != 0 {
                r.x = Some(u16_at(d, pos)? as i16);
                pos += 2;
            }
            if flags & 0x02 != 0 {
                r.y = Some(u16_at(d, pos)? as i16);
                pos += 2;
            }
            if flags & 0x08 != 0 {
                r.height = Some(u16_at(d, pos)?);
                pos += 2;
            }
            let n = *d.get(pos)? as usize;
            pos += 1;
            let mut br = BitReader { d: d.get(pos..)?, pos: 0 };
            for _ in 0..n {
                let g = br.bits(gbits);
                let a = br.signed(abits);
                r.glyphs.push((g, a));
            }
            pos += br.byte_pos();
            records.push(r);
        }
        Some(StaticText { code: t.code, id, bounds, matrix, records })
    }

    pub fn write(&self) -> Tag {
        let gbits = self.records.iter().flat_map(|r| r.glyphs.iter()).map(|g| 32 - g.0.leading_zeros() as usize).max().unwrap_or(1).max(1);
        let abits = self.records.iter().flat_map(|r| r.glyphs.iter()).map(|g| sbits(g.1)).max().unwrap_or(1).max(1);
        let mut d = self.id.to_le_bytes().to_vec();
        write_rect(&mut d, self.bounds);
        d.extend_from_slice(&self.matrix.to_bytes());
        d.push(gbits as u8);
        d.push(abits as u8);
        for r in &self.records {
            let flags = 0x80 | if r.font.is_some() { 0x08 } else { 0 } | if r.color.is_some() { 0x04 } else { 0 } | if r.y.is_some() { 0x02 } else { 0 } | if r.x.is_some() { 0x01 } else { 0 };
            d.push(flags);
            if let Some(f) = r.font {
                d.extend_from_slice(&f.to_le_bytes());
            }
            if let Some(c) = r.color {
                d.extend_from_slice(&[c.r, c.g, c.b]);
                if self.code == 33 {
                    d.push(c.a);
                }
            }
            if let Some(x) = r.x {
                d.extend_from_slice(&x.to_le_bytes());
            }
            if let Some(y) = r.y {
                d.extend_from_slice(&y.to_le_bytes());
            }
            if r.font.is_some() {
                d.extend_from_slice(&r.height.unwrap_or(240).to_le_bytes());
            }
            d.push(r.glyphs.len() as u8);
            let mut bw = BitWriter::default();
            for (g, a) in &r.glyphs {
                bw.bits(gbits, *g);
                bw.signed(abits, *a);
            }
            d.extend_from_slice(&bw.finish());
        }
        d.push(0);
        Tag { code: self.code, data: d, long: true }
    }

    /// Each record with the font, height, colour and position in effect
    /// (records inherit them from the previous ones).
    pub fn resolved(&self) -> Vec<(u16, u16, Rgba, i32, i32)> {
        let (mut font, mut height, mut color, mut x, mut y) = (0u16, 240u16, Rgba { r: 0, g: 0, b: 0, a: 255 }, 0i32, 0i32);
        let mut out = Vec::new();
        for r in &self.records {
            font = r.font.unwrap_or(font);
            height = r.height.unwrap_or(height);
            color = r.color.unwrap_or(color);
            x = r.x.map(|v| v as i32).unwrap_or(x);
            y = r.y.map(|v| v as i32).unwrap_or(y);
            out.push((font, height, color, x, y));
            x += r.glyphs.iter().map(|g| g.1).sum::<i32>();
        }
        out
    }

    /// Replace a record's characters (glyphs of its font, advances from the
    /// font's layout at the record's height) and/or its colour; the bounds
    /// grow to the new width.
    pub fn edit(&mut self, record: usize, text: Option<&str>, color: Option<Rgba>, fonts: &dyn Fn(u16) -> Option<Font>, known: &std::collections::HashMap<(u16, u32), f64>) -> Result<()> {
        let resolved = self.resolved();
        let &(font_id, height, _, x, _) = resolved.get(record).ok_or_else(|| Error::Format(format!("no record {record}")))?;
        if let Some(text) = text {
            let font = fonts(font_id).ok_or_else(|| Error::Format(format!("font {font_id} is not in this movie")))?;
            let mut glyphs = Vec::new();
            let mut lacking = Vec::new();
            for c in text.chars() {
                match font.index_of(c) {
                    Some(i) => {
                        // Advance in EM units: what the movie's texts already use for
                        // the glyph, else the font's layout, else its outline.
                        let layout = font.layout.as_ref().and_then(|l| l.advances.get(i)).filter(|&&a| a > 0).map(|&a| a as f64);
                        let em_advance = known.get(&(font_id, i as u32)).copied().or(layout).unwrap_or_else(|| outline_advance(&font, i));
                        glyphs.push((i as u32, (em_advance * height as f64 / font.em()).round() as i32));
                    }
                    None => lacking.push(c),
                }
            }
            if !lacking.is_empty() {
                let s: String = lacking.into_iter().collect();
                return Err(Error::Format(format!("{} has no glyph for \"{s}\": add them to the font first", font.name())));
            }
            if glyphs.len() > 255 {
                return Err(Error::Format("a text record holds 255 characters at most".into()));
            }
            self.records[record].glyphs = glyphs;
            let width: i32 = self.records[record].glyphs.iter().map(|g| g.1).sum();
            self.bounds[0] = self.bounds[0].min(x);
            self.bounds[2] = self.bounds[2].max(x + width);
        }
        if let Some(c) = color {
            // The next record inherited this one's colour: it keeps the old one.
            if let Some(next) = self.records.get_mut(record + 1) {
                if next.color.is_none() {
                    next.color = Some(resolved[record].2);
                }
            }
            self.records[record].color = Some(c);
        }
        Ok(())
    }

    /// The records for the Studio: text, font, height (px), colour, position.
    pub fn json(&self, fonts: &dyn Fn(u16) -> Option<Font>) -> J {
        let resolved = self.resolved();
        J::Array(
            self.records
                .iter()
                .zip(resolved)
                .map(|(r, (font, height, color, x, y))| {
                    let f = fonts(font);
                    let indices: Vec<u32> = r.glyphs.iter().map(|g| g.0).collect();
                    json!({
                        "text": f.as_ref().map(|f| f.text(&indices)).unwrap_or_default(),
                        "font": font,
                        "fontName": f.as_ref().map(|f| f.name()),
                        "height": height as f64 / 20.0,
                        "color": color.hex(),
                        "x": x as f64 / 20.0,
                        "y": y as f64 / 20.0,
                    })
                })
                .collect(),
        )
    }
}

/// Glyph advances (EM units) the movie's static texts use, by font and
/// glyph: fonts without a layout table only have these.
pub fn known_advances(m: &Movie, em: &dyn Fn(u16) -> f64) -> std::collections::HashMap<(u16, u32), f64> {
    let mut out = std::collections::HashMap::new();
    for t in &m.tags {
        let Some(s) = StaticText::parse(t) else { continue };
        for (r, (font, height, ..)) in s.records.iter().zip(s.resolved()) {
            if height == 0 {
                continue;
            }
            for &(g, adv) in &r.glyphs {
                out.entry((font, g)).or_insert(adv as f64 * em(font) / height as f64);
            }
        }
    }
    out
}

/// An advance guessed from a glyph's outline: its width plus the left side
/// bearing on both sides (a quarter EM for an empty glyph such as a space).
fn outline_advance(font: &Font, i: usize) -> f64 {
    let pts: Vec<(f64, f64)> = font.contours(i).iter().flat_map(|c| super::shape::flatten(c)).collect();
    if pts.is_empty() {
        return font.em() * 0.25;
    }
    let (x0, x1) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    x1 + x0.max(font.em() * 0.04)
}

/// Rewrite the glyph indices of `font`'s records in every static text (after glyphs were added).
pub fn remap_texts(m: &mut Movie, font: u16, remap: &[usize]) -> usize {
    let mut n = 0;
    for t in m.tags.iter_mut() {
        let Some(mut s) = StaticText::parse(t) else { continue };
        let mut cur = 0u16;
        let mut changed = false;
        for r in &mut s.records {
            cur = r.font.unwrap_or(cur);
            if cur != font {
                continue;
            }
            for g in &mut r.glyphs {
                if let Some(&new) = remap.get(g.0 as usize) {
                    changed |= new as u32 != g.0;
                    g.0 = new as u32;
                }
            }
        }
        if changed {
            *t = s.write();
            n += 1;
        }
    }
    n
}

// ----------------------------------------------------------------------------
// Sounds and binary data
// ----------------------------------------------------------------------------

fn wav(channels: u16, rate: u32, bits: u16, pcm: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(44 + pcm.len());
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    o.extend_from_slice(b"WAVEfmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&channels.to_le_bytes());
    o.extend_from_slice(&rate.to_le_bytes());
    let align = channels * bits / 8;
    o.extend_from_slice(&(rate * align as u32).to_le_bytes());
    o.extend_from_slice(&align.to_le_bytes());
    o.extend_from_slice(&bits.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    o.extend_from_slice(pcm);
    o
}

const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026,
    4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// SWF ADPCM to 16-bit PCM (interleaved).
fn adpcm(d: &[u8], channels: usize) -> Vec<u8> {
    let tables: [&[i32]; 4] = [&[-1, 2], &[-1, -1, 2, 4], &[-1, -1, -1, -1, 2, 4, 6, 8], &[-1, -1, -1, -1, -1, -1, -1, -1, 1, 2, 4, 6, 8, 10, 13, 16]];
    let mut br = BitReader { d, pos: 0 };
    let total = d.len() * 8;
    let bits = br.bits(2) as usize + 2;
    let table = tables[bits - 2];
    let sign = 1u32 << (bits - 1);
    let mut out = Vec::new();
    let mut state = vec![(0i32, 0i32); channels];
    'packets: while br.pos + channels * 22 <= total {
        for s in state.iter_mut() {
            s.0 = br.signed(16);
            s.1 = br.bits(6) as i32;
            out.extend_from_slice(&(s.0 as i16).to_le_bytes());
        }
        for _ in 0..4095 {
            if br.pos + channels * bits > total {
                break 'packets;
            }
            for s in state.iter_mut() {
                let code = br.bits(bits);
                let mut step = STEPS[s.1 as usize];
                let mut diff = 0;
                let mut k = 1u32 << (bits - 2);
                while k != 0 {
                    if code & k != 0 {
                        diff += step;
                    }
                    step >>= 1;
                    k >>= 1;
                }
                diff += step;
                s.0 = if code & sign != 0 { s.0 - diff } else { s.0 + diff }.clamp(-32768, 32767);
                s.1 = (s.1 + table[(code & !sign) as usize]).clamp(0, 88);
                out.extend_from_slice(&(s.0 as i16).to_le_bytes());
            }
        }
    }
    out
}

/// A `DefineSound` as a file: MP3 as stored, PCM and ADPCM as WAV. Returns the extension too.
pub fn sound_file(t: &Tag) -> Result<(&'static str, Vec<u8>)> {
    let d = &t.data;
    let b = *d.get(2).ok_or_else(|| Error::Format("bad DefineSound".into()))?;
    let format = b >> 4;
    let rate = [5512u32, 11025, 22050, 44100][((b >> 2) & 3) as usize];
    let bits16 = b & 2 != 0;
    let channels = if b & 1 != 0 { 2 } else { 1 };
    let data = d.get(7..).unwrap_or(&[]);
    match format {
        2 => Ok(("mp3", data.get(2..).unwrap_or(&[]).to_vec())),
        0 | 3 => Ok(("wav", wav(channels, rate, if bits16 { 16 } else { 8 }, data))),
        1 => Ok(("wav", wav(channels, rate, 16, &adpcm(data, channels as usize)))),
        4..=6 => Err(Error::Format("Nellymoser sounds cannot be exported".into())),
        11 => Err(Error::Format("Speex sounds cannot be exported".into())),
        f => Err(Error::Format(format!("unknown sound format {f}"))),
    }
}

/// The bytes of a `DefineBinaryData`.
pub fn binary_data(t: &Tag) -> Result<Vec<u8>> {
    if t.code != 87 || t.data.len() < 6 || u32_at(&t.data, 2).is_none() {
        return Err(Error::Format("not a DefineBinaryData".into()));
    }
    Ok(t.data[6..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_text_round_trips_and_edits() {
        let e = EditText {
            id: 4,
            bounds: [-40, -40, 2000, 400],
            flags: 0x4000 | 0x0002 | 0x0001,
            font: Some(2),
            font_class: None,
            height: Some(360),
            color: Some(Rgba { r: 1, g: 2, b: 3, a: 255 }),
            max_length: None,
            layout: Some((2, 0, 0, 0, 40)),
            variable: "label".into(),
            text: Some("Hello".into()),
        };
        let d = e.write();
        let p = EditText::parse(&d).unwrap();
        assert_eq!(p.write(), d);
        let mut q = p.clone();
        q.set(&json!({"height": 24, "color": "#ff000080", "align": "right", "multiline": true, "html": false, "fontClass": "$Title"})).unwrap();
        let r = EditText::parse(&q.write()).unwrap();
        assert_eq!((r.height, r.font, r.font_class.as_deref()), (Some(480), None, Some("$Title")));
        assert_eq!(r.color, Some(Rgba { r: 255, g: 0, b: 0, a: 128 }));
        assert_eq!(r.layout.unwrap().0, 1);
        let j = r.json();
        assert_eq!((j["multiline"].as_bool(), j["html"].as_bool(), j["wordWrap"].as_bool()), (Some(true), Some(false), Some(true)));
        assert_eq!(r.text.as_deref(), Some("Hello"));
    }

    #[test]
    fn static_text_round_trips_and_re_encodes() {
        let s = StaticText {
            code: 33,
            id: 9,
            bounds: [0, 0, 100, 100],
            matrix: Matrix::default(),
            records: vec![TextRecord { font: Some(1), color: Some(Rgba { r: 0, g: 0, b: 0, a: 255 }), x: Some(0), y: Some(200), height: Some(400), glyphs: vec![(0, 100), (1, 120)] }],
        };
        let t = s.write();
        let p = StaticText::parse(&t).unwrap();
        assert_eq!(p.write().data, t.data);
        let font = Font { code: 48, id: 1, flags: 0x80 | 0x04, language: 0, name: b"F".to_vec(), glyphs: vec![Vec::new(), Vec::new(), Vec::new()], codes: vec![65, 66, 67], layout: Some(super::super::font::Layout { advances: vec![512, 1024, 256], bounds: vec![[0; 4]; 3], ..Default::default() }) };
        let fonts = |id: u16| (id == 1).then(|| font.clone());
        assert_eq!(p.json(&fonts)[0]["text"], "AB");
        let mut q = p.clone();
        let known = Default::default();
        q.edit(0, Some("CAB"), None, &fonts, &known).unwrap();
        assert_eq!(q.records[0].glyphs, vec![(2, 100), (0, 200), (1, 400)]);
        assert_eq!(q.bounds[2], 700);
        assert!(q.edit(0, Some("Z"), None, &fonts, &known).is_err());
        // A colour stays on its record: the next one keeps the inherited colour.
        q.records.push(TextRecord { glyphs: vec![(0, 100)], ..Default::default() });
        q.edit(0, None, Some(Rgba { r: 255, g: 0, b: 0, a: 255 }), &fonts, &known).unwrap();
        assert_eq!(q.resolved()[1].2, Rgba { r: 0, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn sounds_become_wav() {
        // 16-bit mono 44.1 kHz PCM.
        let mut d = vec![1, 0, (3 << 4) | (3 << 2) | 2, 2, 0, 0, 0];
        d.extend_from_slice(&[1, 0, 2, 0]);
        let (ext, f) = sound_file(&Tag { code: 14, data: d, long: false }).unwrap();
        assert_eq!(ext, "wav");
        assert_eq!(&f[..4], b"RIFF");
        assert_eq!(f.len(), 48);
    }
}
