//! The character dictionary of a movie: every defined (or imported)
//! character id with its kind, defining tag, the characters it uses,
//! its bounds, linkage names and the tags attached to it (scaling grids,
//! font extras, `DoInitAction` class code).

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value as J};

use super::display::{self, Matrix};
use super::{cstr, tag_name, u16_at, u32_at, BitReader, Movie, Tag};

#[derive(Clone, Debug)]
pub struct Character {
    pub id: u16,
    pub kind: &'static str,
    /// Top-level index of the defining tag (`None` for imports).
    pub tag: Option<usize>,
    pub code: u16,
    pub deps: Vec<u16>,
    /// Own bounds in twips `[xmin, ymin, xmax, ymax]`, when the tag states them.
    pub rect: Option<[i32; 4]>,
    pub frames: u16,
    pub names: Vec<String>,
    /// `ImportAssets2` URL and symbol name for imported characters.
    pub import: Option<(String, String)>,
    /// Top-level indices of tags about this character (DoInitAction, scaling grid, font info…).
    pub attached: Vec<usize>,
}

pub struct Dict {
    pub chars: BTreeMap<u16, Character>,
    /// Bounds of sprites (first frame) and buttons, computed lazily.
    computed: std::cell::RefCell<HashMap<u16, Option<[i32; 4]>>>,
}

pub fn kind_of(code: u16) -> Option<&'static str> {
    Some(match code {
        2 | 22 | 32 | 83 => "shape",
        46 | 84 => "morph",
        39 => "sprite",
        7 | 34 => "button",
        11 | 33 => "text",
        37 => "edittext",
        10 | 48 | 75 | 91 | 1005 => "font",
        6 | 20 | 21 | 35 | 36 => "bitmap",
        1001 | 1009 => "image",
        1003 => "image",
        1008 => "subimage",
        14 | 1006 => "sound",
        60 => "video",
        87 => "binary",
        _ => return None,
    })
}

/// The character id a non-defining tag is about.
fn attached_to(t: &Tag) -> Option<u16> {
    match t.code {
        13 | 17 | 23 | 59 | 62 | 73 | 74 | 78 | 88 => u16_at(&t.data, 0),
        _ => None,
    }
}

pub(crate) fn read_rect(d: &[u8], at: usize) -> Option<([i32; 4], usize)> {
    let mut br = BitReader { d: d.get(at..)?, pos: 0 };
    let n = br.bits(5) as usize;
    let xmin = br.signed(n);
    let xmax = br.signed(n);
    let ymin = br.signed(n);
    let ymax = br.signed(n);
    Some(([xmin, ymin, xmax, ymax], at + (5 + 4 * n).div_ceil(8)))
}

fn union(a: Option<[i32; 4]>, b: [i32; 4]) -> [i32; 4] {
    match a {
        None => b,
        Some(a) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
    }
}

/// Size of a FILTERLIST (count byte included), from `at`.
pub(crate) fn filter_list_len(d: &[u8], at: usize) -> Option<usize> {
    let n = *d.get(at)? as usize;
    let mut pos = at + 1;
    for _ in 0..n {
        let id = *d.get(pos)?;
        pos += 1;
        pos += match id {
            0 => 23,
            1 => 9,
            2 => 15,
            3 => 27,
            4 | 7 => {
                let c = *d.get(pos)? as usize;
                1 + 5 * c + 19
            }
            5 => {
                let x = *d.get(pos)? as usize;
                let y = *d.get(pos + 1)? as usize;
                2 + 8 + 4 * x * y + 5
            }
            6 => 80,
            _ => return None,
        };
    }
    Some(pos - at)
}

/// Button records: `(flags, character, depth, matrix)`.
pub(crate) fn button_records(code: u16, d: &[u8]) -> Vec<(u8, u16, u16, Matrix)> {
    let mut out = Vec::new();
    let mut pos = if code == 34 { 5 } else { 2 };
    while let Some(&flags) = d.get(pos) {
        if flags == 0 {
            break;
        }
        pos += 1;
        let (Some(ch), Some(depth)) = (u16_at(d, pos), u16_at(d, pos + 2)) else { break };
        pos += 4;
        let Some(rest) = d.get(pos..) else { break };
        let mut br = BitReader { d: rest, pos: 0 };
        let m = Matrix::read(&mut br);
        pos += br.byte_pos();
        if code == 34 {
            let Some(rest) = d.get(pos..) else { break };
            let mut br = BitReader { d: rest, pos: 0 };
            display::Cxform::read(&mut br, true);
            pos += br.byte_pos();
            if flags & 0x10 != 0 {
                let Some(n) = filter_list_len(d, pos) else { break };
                pos += n;
            }
            if flags & 0x20 != 0 {
                pos += 1;
            }
        }
        out.push((flags, ch, depth, m));
    }
    out
}

/// Bitmap ids a shape's fill styles use.
fn shape_bitmaps(code: u16, d: &[u8]) -> Vec<u16> {
    let mut out = Vec::new();
    let _ = shape_walk(code, d, &mut |id| {
        if !out.contains(&id) {
            out.push(id);
        }
    });
    out
}

fn read_fill_styles(code: u16, d: &[u8], pos: &mut usize, bitmap: &mut dyn FnMut(u16)) -> Option<()> {
    let mut n = *d.get(*pos)? as usize;
    *pos += 1;
    if n == 0xFF && code != 2 {
        n = u16_at(d, *pos)? as usize;
        *pos += 2;
    }
    for _ in 0..n {
        read_fill_style(code, d, pos, bitmap)?;
    }
    Some(())
}

fn read_fill_style(code: u16, d: &[u8], pos: &mut usize, bitmap: &mut dyn FnMut(u16)) -> Option<()> {
    let ty = *d.get(*pos)?;
    *pos += 1;
    let rgba = code == 32 || code == 83;
    match ty {
        0x00 => *pos += if rgba { 4 } else { 3 },
        0x10 | 0x12 | 0x13 => {
            let mut br = BitReader { d: d.get(*pos..)?, pos: 0 };
            Matrix::read(&mut br);
            *pos += br.byte_pos();
            let n = (*d.get(*pos)? & 0x0F) as usize;
            *pos += 1 + n * (1 + if rgba { 4 } else { 3 });
            if ty == 0x13 {
                *pos += 2;
            }
        }
        0x40..=0x43 => {
            bitmap(u16_at(d, *pos)?);
            *pos += 2;
            let mut br = BitReader { d: d.get(*pos..)?, pos: 0 };
            Matrix::read(&mut br);
            *pos += br.byte_pos();
        }
        _ => return None,
    }
    Some(())
}

fn read_line_styles(code: u16, d: &[u8], pos: &mut usize, bitmap: &mut dyn FnMut(u16)) -> Option<()> {
    let mut n = *d.get(*pos)? as usize;
    *pos += 1;
    if n == 0xFF {
        n = u16_at(d, *pos)? as usize;
        *pos += 2;
    }
    for _ in 0..n {
        *pos += 2;
        if code == 83 {
            let f1 = *d.get(*pos)?;
            let f2 = *d.get(*pos + 1)?;
            *pos += 2;
            if (f1 >> 4) & 3 == 2 {
                *pos += 2;
            }
            if f1 & 0x08 != 0 {
                read_fill_style(code, d, pos, bitmap)?;
            } else {
                *pos += 4;
            }
            let _ = f2;
        } else {
            *pos += if code == 32 { 4 } else { 3 };
        }
    }
    Some(())
}

/// Walk a DefineShape's styles and records, reporting bitmap fills.
fn shape_walk(code: u16, d: &[u8], bitmap: &mut dyn FnMut(u16)) -> Option<()> {
    let (_, mut pos) = read_rect(d, 2)?;
    if code == 83 {
        pos = read_rect(d, pos)?.1 + 1;
    }
    read_fill_styles(code, d, &mut pos, bitmap)?;
    read_line_styles(code, d, &mut pos, bitmap)?;
    let mut br = BitReader { d: d.get(pos..)?, pos: 0 };
    let mut fill_bits = br.bits(4) as usize;
    let mut line_bits = br.bits(4) as usize;
    loop {
        if br.byte_pos() > br.d.len() {
            return None;
        }
        if br.bit() == 0 {
            let flags = br.bits(5);
            if flags == 0 {
                break;
            }
            if flags & 0x01 != 0 {
                let n = br.bits(5) as usize;
                br.signed(n);
                br.signed(n);
            }
            if flags & 0x02 != 0 {
                br.bits(fill_bits);
            }
            if flags & 0x04 != 0 {
                br.bits(fill_bits);
            }
            if flags & 0x08 != 0 {
                br.bits(line_bits);
            }
            if flags & 0x10 != 0 && code != 2 {
                br.align();
                let mut p = pos + br.byte_pos();
                read_fill_styles(code, d, &mut p, bitmap)?;
                read_line_styles(code, d, &mut p, bitmap)?;
                br = BitReader { d: d.get(p..)?, pos: 0 };
                pos = p;
                fill_bits = br.bits(4) as usize;
                line_bits = br.bits(4) as usize;
            }
        } else {
            let straight = br.bit() == 1;
            let n = br.bits(4) as usize + 2;
            if straight {
                if br.bit() == 1 {
                    br.signed(n);
                    br.signed(n);
                } else {
                    br.bit();
                    br.signed(n);
                }
            } else {
                br.signed(n);
                br.signed(n);
                br.signed(n);
                br.signed(n);
            }
        }
    }
    Some(())
}

/// Font ids a DefineText's records use.
fn text_fonts(code: u16, d: &[u8]) -> Vec<u16> {
    let mut out = Vec::new();
    let Some((_, mut pos)) = read_rect(d, 2) else { return out };
    let Some(rest) = d.get(pos..) else { return out };
    let mut br = BitReader { d: rest, pos: 0 };
    Matrix::read(&mut br);
    pos += br.byte_pos();
    let (Some(&gbits), Some(&abits)) = (d.get(pos), d.get(pos + 1)) else { return out };
    pos += 2;
    while let Some(&flags) = d.get(pos) {
        if flags == 0 {
            break;
        }
        pos += 1;
        if flags & 0x08 != 0 {
            let Some(f) = u16_at(d, pos) else { break };
            if !out.contains(&f) {
                out.push(f);
            }
            pos += 2;
        }
        if flags & 0x04 != 0 {
            pos += if code == 33 { 4 } else { 3 };
        }
        if flags & 0x01 != 0 {
            pos += 2;
        }
        if flags & 0x02 != 0 {
            pos += 2;
        }
        if flags & 0x08 != 0 {
            pos += 2;
        }
        let Some(&count) = d.get(pos) else { break };
        pos += 1;
        pos += (count as usize * (gbits as usize + abits as usize)).div_ceil(8);
    }
    out
}

/// Pixel size of an embedded JPEG / PNG / GIF (DefineBits*).
fn image_size(d: &[u8]) -> Option<(u32, u32)> {
    if d.starts_with(b"\x89PNG") {
        let w = u32::from_be_bytes(d.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(d.get(20..24)?.try_into().ok()?);
        return Some((w, h));
    }
    if d.starts_with(b"GIF8") {
        return Some((u16_at(d, 6)? as u32, u16_at(d, 8)? as u32));
    }
    // JPEG: find a start-of-frame marker.
    let mut i = 0;
    while i + 9 < d.len() {
        if d[i] == 0xFF && matches!(d[i + 1], 0xC0..=0xC2) {
            let h = u16::from_be_bytes([d[i + 5], d[i + 6]]) as u32;
            let w = u16::from_be_bytes([d[i + 7], d[i + 8]]) as u32;
            return Some((w, h));
        }
        i += 1;
    }
    None
}

fn px_rect(w: u32, h: u32) -> [i32; 4] {
    [0, 0, w as i32 * 20, h as i32 * 20]
}

fn define(t: &Tag) -> Option<Character> {
    let kind = kind_of(t.code)?;
    let d = &t.data;
    let id = match t.code {
        1001 | 1009 => {
            let raw = if t.code == 1009 { u32_at(d, 0)? } else { u16_at(d, 0)? as u32 };
            (raw & 0xFFFF) as u16
        }
        _ => u16_at(d, 0)?,
    };
    let mut c = Character { id, kind, tag: None, code: t.code, deps: Vec::new(), rect: None, frames: 0, names: Vec::new(), import: None, attached: Vec::new() };
    match t.code {
        2 | 22 | 32 | 83 => {
            c.rect = read_rect(d, 2).map(|r| r.0);
            c.deps = shape_bitmaps(t.code, d);
        }
        46 | 84 => c.rect = read_rect(d, 2).map(|r| r.0),
        11 | 33 => {
            c.rect = read_rect(d, 2).map(|r| r.0);
            c.deps = text_fonts(t.code, d);
        }
        37 => {
            c.rect = read_rect(d, 2).map(|r| r.0);
            if let Some(et) = super::parse_edit_text(d) {
                if let Some(f) = et.font_id {
                    c.deps.push(f);
                }
            }
        }
        39 => {
            c.frames = u16_at(d, 2).unwrap_or(0);
            for it in display::sprite_body(t) {
                if display::is_place(it.code) {
                    if let Some(ch) = display::Place::parse(it.code, &it.data).and_then(|p| p.character) {
                        if !c.deps.contains(&ch) {
                            c.deps.push(ch);
                        }
                    }
                }
            }
        }
        7 | 34 => {
            for (_, ch, _, _) in button_records(t.code, d) {
                if !c.deps.contains(&ch) {
                    c.deps.push(ch);
                }
            }
        }
        20 | 36 => {
            if let (Some(w), Some(h)) = (u16_at(d, 3), u16_at(d, 5)) {
                c.rect = Some(px_rect(w as u32, h as u32));
            }
        }
        6 | 21 => c.rect = image_size(d.get(2..).unwrap_or(&[])).map(|(w, h)| px_rect(w, h)),
        35 => c.rect = image_size(d.get(6..).unwrap_or(&[])).map(|(w, h)| px_rect(w, h)),
        1001 | 1009 => {
            if let Some(img) = super::parse_external_image(t.code, d) {
                c.rect = Some(px_rect(img.width as u32, img.height as u32));
            }
        }
        1008 => {
            if let Some(s) = super::parse_sub_image(d) {
                c.deps.push(s.image_id);
                c.rect = Some(px_rect(s.x2.saturating_sub(s.x1) as u32, s.y2.saturating_sub(s.y1) as u32));
            }
        }
        _ => {}
    }
    Some(c)
}

impl Dict {
    pub fn build(m: &Movie) -> Dict {
        let mut chars: BTreeMap<u16, Character> = BTreeMap::new();
        for (i, t) in m.tags.iter().enumerate() {
            if let Some(mut c) = define(t) {
                c.tag = Some(i);
                chars.insert(c.id, c);
                continue;
            }
            match t.code {
                56 => {
                    let n = u16_at(&t.data, 0).unwrap_or(0) as usize;
                    let mut pos = 2;
                    for _ in 0..n {
                        let id = u16_at(&t.data, pos).unwrap_or(0);
                        let (s, next) = cstr(&t.data, pos + 2);
                        if let Some(c) = chars.get_mut(&id) {
                            c.names.push(s);
                        }
                        pos = next;
                    }
                }
                57 | 71 => {
                    let (url, mut pos) = cstr(&t.data, 0);
                    if t.code == 71 {
                        pos += 2;
                    }
                    let n = u16_at(&t.data, pos).unwrap_or(0) as usize;
                    pos += 2;
                    for _ in 0..n {
                        let id = u16_at(&t.data, pos).unwrap_or(0);
                        let (s, next) = cstr(&t.data, pos + 2);
                        chars.insert(id, Character { id, kind: "import", tag: None, code: t.code, deps: Vec::new(), rect: None, frames: 0, names: vec![s.clone()], import: Some((url.clone(), s)), attached: vec![i] });
                        pos = next;
                    }
                }
                _ => {
                    if let Some(id) = attached_to(t) {
                        if let Some(c) = chars.get_mut(&id) {
                            c.attached.push(i);
                        }
                    }
                }
            }
        }
        Dict { chars, computed: Default::default() }
    }

    /// Bounds of a character (twips), sprites at their first frame.
    pub fn bounds(&self, m: &Movie, id: u16) -> Option<[i32; 4]> {
        self.bounds_depth(m, id, 0)
    }

    fn bounds_depth(&self, m: &Movie, id: u16, depth: usize) -> Option<[i32; 4]> {
        let c = self.chars.get(&id)?;
        if c.rect.is_some() || !matches!(c.kind, "sprite" | "button") {
            return c.rect;
        }
        if let Some(r) = self.computed.borrow().get(&id) {
            return *r;
        }
        if depth > 32 {
            return None;
        }
        let tag = &m.tags[c.tag?];
        let mut r: Option<[i32; 4]> = None;
        if c.kind == "sprite" {
            let body = display::sprite_body(tag);
            let tl = display::timeline(&body, 1);
            for e in tl.frames.first().into_iter().flatten() {
                if e.clip_depth.is_some() {
                    continue;
                }
                if let Some(b) = self.bounds_depth(m, e.character, depth + 1) {
                    r = Some(union(r, e.matrix.apply_rect(b)));
                }
            }
        } else {
            for (flags, ch, _, mx) in button_records(tag.code, &tag.data) {
                if flags & 0x01 == 0 {
                    continue;
                }
                if let Some(b) = self.bounds_depth(m, ch, depth + 1) {
                    r = Some(union(r, mx.apply_rect(b)));
                }
            }
        }
        self.computed.borrow_mut().insert(id, r);
        r
    }

    /// Bounds of a display list (twips).
    pub fn list_bounds(&self, m: &Movie, list: &[display::Entry]) -> Option<[i32; 4]> {
        let mut r: Option<[i32; 4]> = None;
        for e in list {
            if e.clip_depth.is_some() {
                continue;
            }
            if let Some(b) = self.bounds(m, e.character) {
                r = Some(union(r, e.matrix.apply_rect(b)));
            }
        }
        r
    }

    /// Every character `id` needs, itself included.
    pub fn closure(&self, ids: &[u16]) -> std::collections::BTreeSet<u16> {
        let mut seen = std::collections::BTreeSet::new();
        let mut stack: Vec<u16> = ids.to_vec();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(c) = self.chars.get(&id) {
                stack.extend(c.deps.iter().copied());
            }
        }
        seen
    }

    /// Characters that use each character.
    pub fn used_by(&self) -> HashMap<u16, Vec<u16>> {
        let mut out: HashMap<u16, Vec<u16>> = HashMap::new();
        for c in self.chars.values() {
            for d in &c.deps {
                out.entry(*d).or_default().push(c.id);
            }
        }
        out
    }

    pub fn json(&self, m: &Movie) -> J {
        let used = self.used_by();
        let chars: Vec<J> = self
            .chars
            .values()
            .map(|c| {
                let rect = self.bounds(m, c.id).map(rect_json);
                json!({
                    "id": c.id,
                    "kind": c.kind,
                    "tag": c.tag,
                    "tagName": tag_name(c.code),
                    "names": c.names,
                    "rect": rect,
                    "frames": c.frames,
                    "deps": c.deps,
                    "usedBy": used.get(&c.id).cloned().unwrap_or_default(),
                    "import": c.import.as_ref().map(|(u, n)| json!({ "url": u, "name": n })),
                    "attached": c.attached.iter().map(|&i| json!({ "tag": i, "name": tag_name(m.tags[i].code) })).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!(chars)
    }
}

/// `[xmin, ymin, xmax, ymax]` twips → `{x, y, w, h}` pixels.
pub fn rect_json(r: [i32; 4]) -> J {
    json!({ "x": r[0] as f64 / 20.0, "y": r[1] as f64 / 20.0, "w": (r[2] - r[0]) as f64 / 20.0, "h": (r[3] - r[1]) as f64 / 20.0 })
}
