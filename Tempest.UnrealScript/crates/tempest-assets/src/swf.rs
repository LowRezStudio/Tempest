//! Scaleform GFx / Flash movies (`SwfMovie.RawData`): the tag stream, what
//! a modder edits in it, and a writer that keeps every untouched byte.
//!
//! The game's movies are `GFX` files (uncompressed SWF with GFx tags, AS2):
//! shapes, sprites, `DefineEditText` fields, `DoInitAction` class
//! definitions, `ExportAssets` linkage names, `ImportAssets2` font and
//! shared-asset imports, and `DefineExternalImage2` (1009) references to
//! textures by file name (the engine resolves `Foo.tga` to a `Texture2D`
//! of the package, so pictures are replaced through the texture; `add`
//! inserts new image references, shapes and placements).
//! Editable here: the initial text of edit-text fields, the strings of the
//! top-level actions of a script (constant pools and pushes: the rest of
//! the bytecode is position-independent relative branches, so only the
//! enclosing `DefineFunction` sizes would move, and those are left alone
//! by editing outside function bodies), and external image file names.
//! The submodules hold the rest of the editor: the character dictionary
//! (`dict`), the display list and timelines (`display`, whose placements
//! `apply` edits) and per-character preview movies (`preview`).

pub mod add;
pub mod as2;
pub mod dict;
pub mod display;
pub mod font;
pub mod preview;
pub mod shape;
pub mod svg;
pub mod text;

use std::io::{Read, Write};

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, Result};

/// One tag as stored: code, payload, and whether the long length form was used.
#[derive(Clone, Debug)]
pub struct Tag {
    pub code: u16,
    pub data: Vec<u8>,
    pub long: bool,
}

#[derive(Clone, Debug)]
pub struct Movie {
    pub signature: [u8; 3],
    pub version: u8,
    /// Frame rect, frame rate and frame count, as stored.
    pub header: Vec<u8>,
    pub tags: Vec<Tag>,
}

pub fn tag_name(code: u16) -> &'static str {
    match code {
        0 => "End",
        1 => "ShowFrame",
        2 => "DefineShape",
        4 => "PlaceObject",
        5 => "RemoveObject",
        6 => "DefineBits",
        7 => "DefineButton",
        8 => "JPEGTables",
        9 => "SetBackgroundColor",
        10 => "DefineFont",
        11 => "DefineText",
        12 => "DoAction",
        13 => "DefineFontInfo",
        14 => "DefineSound",
        15 => "StartSound",
        17 => "DefineButtonSound",
        18 => "SoundStreamHead",
        19 => "SoundStreamBlock",
        20 => "DefineBitsLossless",
        21 => "DefineBitsJPEG2",
        22 => "DefineShape2",
        23 => "DefineButtonCxform",
        24 => "Protect",
        26 => "PlaceObject2",
        28 => "RemoveObject2",
        32 => "DefineShape3",
        33 => "DefineText2",
        34 => "DefineButton2",
        35 => "DefineBitsJPEG3",
        36 => "DefineBitsLossless2",
        37 => "DefineEditText",
        39 => "DefineSprite",
        43 => "FrameLabel",
        45 => "SoundStreamHead2",
        46 => "DefineMorphShape",
        48 => "DefineFont2",
        56 => "ExportAssets",
        57 => "ImportAssets",
        58 => "EnableDebugger",
        59 => "DoInitAction",
        60 => "DefineVideoStream",
        61 => "VideoFrame",
        62 => "DefineFontInfo2",
        64 => "EnableDebugger2",
        65 => "ScriptLimits",
        66 => "SetTabIndex",
        69 => "FileAttributes",
        70 => "PlaceObject3",
        71 => "ImportAssets2",
        73 => "DefineFontAlignZones",
        74 => "CSMTextSettings",
        75 => "DefineFont3",
        76 => "SymbolClass",
        77 => "Metadata",
        78 => "DefineScalingGrid",
        82 => "DoABC",
        83 => "DefineShape4",
        84 => "DefineMorphShape2",
        86 => "DefineSceneAndFrameLabelData",
        87 => "DefineBinaryData",
        88 => "DefineFontName",
        91 => "DefineFont4",
        1000 => "GFxExporterInfo",
        1001 => "GFxDefineExternalImage",
        1002 => "GFxFontTextureInfo",
        1003 => "GFxDefineExternalGradientImage",
        1004 => "GFxDefineGradientMap",
        1005 => "GFxDefineCompactedFont",
        1006 => "GFxDefineExternalSound",
        1007 => "GFxDefineExternalStreamSound",
        1008 => "GFxDefineSubImage",
        1009 => "GFxDefineExternalImage2",
        _ => "?",
    }
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(at)?, *d.get(at + 1)?]))
}

pub fn u16_at_pub(d: &[u8], at: usize) -> Option<u16> {
    u16_at(d, at)
}

pub fn u32_at_pub(d: &[u8], at: usize) -> Option<u32> {
    u32_at(d, at)
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// Tags of a stream (a movie body after its header, or a sprite's body).
fn parse_tags(d: &[u8]) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();
    let mut pos = 0;
    while pos + 2 <= d.len() {
        let h = u16_at(d, pos).unwrap();
        pos += 2;
        let code = h >> 6;
        let mut len = (h & 0x3F) as usize;
        let long = len == 0x3F;
        if long {
            len = u32_at(d, pos).ok_or_else(|| Error::Format("truncated tag length".into()))? as usize;
            pos += 4;
        }
        let data = d.get(pos..pos + len).ok_or_else(|| Error::Format(format!("tag {code} at {pos} runs past the end")))?.to_vec();
        pos += len;
        tags.push(Tag { code, data, long });
        if code == 0 {
            break;
        }
    }
    Ok(tags)
}

fn write_tags(tags: &[Tag], out: &mut Vec<u8>) {
    for t in tags {
        let long = t.long || t.data.len() >= 0x3F;
        if long {
            out.extend_from_slice(&((t.code << 6) | 0x3F).to_le_bytes());
            out.extend_from_slice(&(t.data.len() as u32).to_le_bytes());
        } else {
            out.extend_from_slice(&((t.code << 6) | t.data.len() as u16).to_le_bytes());
        }
        out.extend_from_slice(&t.data);
    }
}

impl Movie {
    pub fn parse(bytes: &[u8]) -> Result<Movie> {
        if bytes.len() < 8 {
            return Err(Error::Format("too short for a movie".into()));
        }
        let signature: [u8; 3] = bytes[..3].try_into().unwrap();
        let version = bytes[3];
        let body: Vec<u8> = match &signature {
            b"GFX" | b"FWS" => bytes[8..].to_vec(),
            b"CFX" | b"CWS" => {
                let mut out = Vec::new();
                flate2::read::ZlibDecoder::new(&bytes[8..]).read_to_end(&mut out).map_err(|e| Error::Format(format!("zlib: {e}")))?;
                out
            }
            b"ZFX" | b"ZWS" => return Err(Error::Format("LZMA-compressed movies are not supported".into())),
            _ => return Err(Error::Format("not a GFx or SWF movie (bad signature)".into())),
        };
        let nbits = (body.first().copied().unwrap_or(0) >> 3) as usize;
        let rect_bytes = (5 + 4 * nbits).div_ceil(8);
        let header_len = rect_bytes + 4;
        if body.len() < header_len {
            return Err(Error::Format("truncated movie header".into()));
        }
        let tags = parse_tags(&body[header_len..])?;
        Ok(Movie { signature, version, header: body[..header_len].to_vec(), tags })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut body = self.header.clone();
        write_tags(&self.tags, &mut body);
        let mut out = Vec::new();
        out.extend_from_slice(&self.signature);
        out.push(self.version);
        out.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
        match &self.signature {
            b"CFX" | b"CWS" => {
                let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                let _ = enc.write_all(&body);
                out.extend_from_slice(&enc.finish().unwrap_or_default());
            }
            _ => out.extend_from_slice(&body),
        }
        out
    }

    /// Frame rect in pixels (twips / 20): (x min, x max, y min, y max).
    pub fn frame(&self) -> (f32, f32, f32, f32) {
        let mut br = BitReader { d: &self.header, pos: 0 };
        let nbits = br.bits(5) as usize;
        let v = |br: &mut BitReader| br.signed(nbits) as f32 / 20.0;
        let xmin = v(&mut br);
        let xmax = v(&mut br);
        let ymin = v(&mut br);
        let ymax = v(&mut br);
        (xmin, xmax, ymin, ymax)
    }

    pub fn frame_rate(&self) -> f32 {
        let n = self.header.len();
        u16_at(&self.header, n - 4).unwrap_or(0) as f32 / 256.0
    }

    pub fn frame_count(&self) -> u16 {
        let n = self.header.len();
        u16_at(&self.header, n - 2).unwrap_or(0)
    }
}

pub(crate) struct BitReader<'a> {
    d: &'a [u8],
    pos: usize,
}

impl BitReader<'_> {
    fn bit(&mut self) -> u32 {
        let b = self.d.get(self.pos / 8).copied().unwrap_or(0);
        let v = (b >> (7 - self.pos % 8)) & 1;
        self.pos += 1;
        v as u32
    }
    fn bits(&mut self, n: usize) -> u32 {
        (0..n).fold(0, |acc, _| (acc << 1) | self.bit())
    }
    fn signed(&mut self, n: usize) -> i32 {
        if n == 0 {
            return 0;
        }
        let v = self.bits(n);
        if v & (1 << (n - 1)) != 0 {
            v as i32 - (1 << n)
        } else {
            v as i32
        }
    }
}

fn cstr(d: &[u8], at: usize) -> (String, usize) {
    let end = d[at.min(d.len())..].iter().position(|&b| b == 0).map(|e| at + e).unwrap_or(d.len());
    (String::from_utf8_lossy(&d[at.min(d.len())..end]).to_string(), (end + 1).min(d.len()))
}

// ----------------------------------------------------------------------------
// DefineEditText
// ----------------------------------------------------------------------------

/// The parts of a `DefineEditText` tag around its text.
pub struct EditText {
    pub id: u16,
    pub flags: u16,
    pub font_id: Option<u16>,
    pub font_class: Option<String>,
    pub font_height: Option<u16>,
    pub variable: String,
    pub text: Option<String>,
    /// Byte offset where the initial text (or the end) starts.
    text_at: usize,
}

pub fn parse_edit_text(d: &[u8]) -> Option<EditText> {
    let id = u16_at(d, 0)?;
    let nbits = (*d.get(2)? >> 3) as usize;
    let mut pos = 2 + (5 + 4 * nbits).div_ceil(8);
    // The flag bytes are a big-endian bit field: first byte HasText, WordWrap,
    // Multiline, Password, ReadOnly, HasTextColor, HasMaxLength, HasFont;
    // second byte HasFontClass, AutoSize, HasLayout, NoSelect, Border,
    // WasStatic, HTML, UseOutlines.
    let flags = ((*d.get(pos)? as u16) << 8) | *d.get(pos + 1)? as u16;
    pos += 2;
    let has_text = flags & 0x8000 != 0;
    let has_font = flags & 0x0100 != 0;
    let has_max_length = flags & 0x0200 != 0;
    let has_text_color = flags & 0x0400 != 0;
    let has_layout = flags & 0x0020 != 0;
    let has_font_class = flags & 0x0080 != 0;
    let mut font_id = None;
    let mut font_class = None;
    let mut font_height = None;
    if has_font {
        font_id = u16_at(d, pos);
        pos += 2;
    }
    if has_font_class {
        let (s, next) = cstr(d, pos);
        font_class = Some(s);
        pos = next;
    }
    if has_font {
        font_height = u16_at(d, pos);
        pos += 2;
    }
    if has_text_color {
        pos += 4;
    }
    if has_max_length {
        pos += 2;
    }
    if has_layout {
        pos += 9;
    }
    let (variable, next) = cstr(d, pos);
    pos = next;
    let text = if has_text { Some(cstr(d, pos).0) } else { None };
    Some(EditText { id, flags, font_id, font_class, font_height, variable, text, text_at: pos })
}

/// The tag with another initial text (the HasText flag set).
pub fn set_edit_text(d: &[u8], text: &str) -> Option<Vec<u8>> {
    let et = parse_edit_text(d)?;
    let nbits = (*d.get(2)? >> 3) as usize;
    let flags_at = 2 + (5 + 4 * nbits).div_ceil(8);
    let mut out = d[..et.text_at].to_vec();
    let flags = et.flags | 0x8000;
    out[flags_at..flags_at + 2].copy_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(text.as_bytes());
    out.push(0);
    Some(out)
}

// ----------------------------------------------------------------------------
// DefineExternalImage2
// ----------------------------------------------------------------------------

pub struct ExternalImage {
    pub id: u32,
    pub format: u16,
    pub width: u16,
    pub height: u16,
    pub export_name: String,
    pub file_name: String,
}

fn pstr(d: &[u8], at: usize) -> Option<(String, usize)> {
    let n = *d.get(at)? as usize;
    let s = d.get(at + 1..at + 1 + n)?;
    Some((String::from_utf8_lossy(s).to_string(), at + 1 + n))
}

pub fn parse_external_image(code: u16, d: &[u8]) -> Option<ExternalImage> {
    let (id, mut pos) = if code == 1009 { (u32_at(d, 0)?, 4) } else { (u16_at(d, 0)? as u32, 2) };
    let format = u16_at(d, pos)?;
    let width = u16_at(d, pos + 2)?;
    let height = u16_at(d, pos + 4)?;
    pos += 6;
    let (export_name, next) = pstr(d, pos)?;
    let (file_name, _) = pstr(d, next)?;
    Some(ExternalImage { id, format, width, height, export_name, file_name })
}

pub fn set_external_image_file(code: u16, d: &[u8], file_name: &str) -> Option<Vec<u8>> {
    let head = if code == 1009 { 4 } else { 2 } + 6;
    let (_, next) = pstr(d, head)?;
    let mut out = d[..next].to_vec();
    out.push(file_name.len().min(255) as u8);
    out.extend_from_slice(&file_name.as_bytes()[..file_name.len().min(255)]);
    Some(out)
}

// ----------------------------------------------------------------------------
// ActionScript 2
// ----------------------------------------------------------------------------

/// A string of the top-level actions of a script: in a constant pool or a push.
pub struct ScriptString {
    pub index: usize,
    pub text: String,
    pub action: &'static str,
    /// Byte range of the action record (including its header) in the action stream.
    action_at: usize,
    /// Position inside the pool / push data.
    item: usize,
}

/// The top-level action records of a script body: `(offset, op, payload)`.
fn actions(a: &[u8]) -> Vec<(usize, u8, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < a.len() {
        let op = a[pos];
        let start = pos;
        pos += 1;
        let mut payload = Vec::new();
        if op >= 0x80 {
            let Some(len) = u16_at(a, pos) else { break };
            pos += 2;
            payload = a.get(pos..pos + len as usize).unwrap_or(&[]).to_vec();
            pos += len as usize;
        }
        out.push((start, op, payload));
        if op == 0 {
            break;
        }
    }
    out
}

fn push_items(p: &[u8]) -> Vec<(usize, u8, Option<String>)> {
    // (offset in payload, type, string for type 0)
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < p.len() {
        let ty = p[pos];
        let at = pos;
        pos += 1;
        match ty {
            0 => {
                let (s, next) = cstr(p, pos);
                out.push((at, ty, Some(s)));
                pos = next;
            }
            1 => pos += 4,
            2 | 3 => {}
            4 | 5 => pos += 1,
            6 => pos += 8,
            7 => pos += 4,
            8 => pos += 1,
            9 => pos += 2,
            _ => break,
        }
        if ty != 0 {
            out.push((at, ty, None));
        }
    }
    out
}

/// The editable strings of a script body (`DoInitAction` after its sprite id, or `DoAction`).
pub fn script_strings(a: &[u8]) -> Vec<ScriptString> {
    let mut out = Vec::new();
    for (at, op, payload) in actions(a) {
        match op {
            0x88 => {
                let count = u16_at(&payload, 0).unwrap_or(0) as usize;
                let mut pos = 2;
                for i in 0..count {
                    let (s, next) = cstr(&payload, pos);
                    out.push(ScriptString { index: out.len(), text: s, action: "ConstantPool", action_at: at, item: i });
                    pos = next;
                }
            }
            0x96 => {
                for (i, (_, ty, s)) in push_items(&payload).into_iter().enumerate() {
                    if ty == 0 {
                        out.push(ScriptString { index: out.len(), text: s.unwrap_or_default(), action: "Push", action_at: at, item: i });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The script body with one top-level string replaced.
pub fn set_script_string(a: &[u8], index: usize, text: &str) -> Option<Vec<u8>> {
    let strings = script_strings(a);
    let target = strings.get(index)?;
    let mut out = Vec::new();
    for (at, op, payload) in actions(a) {
        if at != target.action_at {
            let end = at + 1 + if op >= 0x80 { 2 + payload.len() } else { 0 };
            out.extend_from_slice(&a[at..end.min(a.len())]);
            continue;
        }
        let new_payload = match op {
            0x88 => {
                let count = u16_at(&payload, 0).unwrap_or(0) as usize;
                let mut items = Vec::new();
                let mut pos = 2;
                for _ in 0..count {
                    let (s, next) = cstr(&payload, pos);
                    items.push(s);
                    pos = next;
                }
                *items.get_mut(target.item)? = text.to_string();
                let mut p = (items.len() as u16).to_le_bytes().to_vec();
                for s in items {
                    p.extend_from_slice(s.as_bytes());
                    p.push(0);
                }
                p
            }
            0x96 => {
                let items = push_items(&payload);
                let (item_at, _, _) = items.get(target.item)?;
                let next_at = items.get(target.item + 1).map(|(n, _, _)| *n).unwrap_or(payload.len());
                let mut p = payload[..*item_at + 1].to_vec();
                p.extend_from_slice(text.as_bytes());
                p.push(0);
                p.extend_from_slice(&payload[next_at..]);
                p
            }
            _ => return None,
        };
        out.push(op);
        out.extend_from_slice(&(new_payload.len() as u16).to_le_bytes());
        out.extend_from_slice(&new_payload);
    }
    Some(out)
}

// ----------------------------------------------------------------------------
// Studio view
// ----------------------------------------------------------------------------

fn define_id(code: u16, d: &[u8]) -> Option<u16> {
    match code {
        2 | 6 | 7 | 10 | 11 | 14 | 20 | 21 | 22 | 32 | 33 | 34 | 35 | 36 | 37 | 39 | 46 | 48 | 60 | 75 | 83 | 84 | 87 | 91 | 1001 | 1003 | 1005 => u16_at(d, 0),
        _ => None,
    }
}

/// One `GFxDefineSubImage` (1008): a crop of an external image under its own
/// character id, which is what the shapes reference.
#[derive(Clone, Copy, Debug)]
pub struct SubImage {
    pub id: u16,
    pub image_id: u16,
    pub x1: u16,
    pub y1: u16,
    pub x2: u16,
    pub y2: u16,
}

pub fn parse_sub_image(d: &[u8]) -> Option<SubImage> {
    if d.len() < 12 {
        return None;
    }
    Some(SubImage { id: u16_at(d, 0)?, image_id: u16_at(d, 2)?, x1: u16_at(d, 4)?, y1: u16_at(d, 6)?, x2: u16_at(d, 8)?, y2: u16_at(d, 10)? })
}

/// The pixels of a `DefineBitsLossless`/`DefineBitsLossless2` (straight
/// RGBA): 32-bit (premultiplied in version 2) and colour-mapped formats.
pub fn decode_lossless(t: &Tag) -> Option<(u16, u32, u32, Vec<u8>)> {
    if t.code != 20 && t.code != 36 {
        return None;
    }
    let d = &t.data;
    let id = u16_at(d, 0)?;
    let format = *d.get(2)?;
    let (w, h) = (u16_at(d, 3)? as usize, u16_at(d, 5)? as usize);
    let alpha = t.code == 36;
    let (table, start) = if format == 3 { (*d.get(7)? as usize + 1, 8) } else { (0, 7) };
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(d.get(start..)?).read_to_end(&mut raw).ok()?;
    let mut rgba = Vec::with_capacity(w * h * 4);
    match format {
        5 => {
            for px in raw.get(..w * h * 4)?.as_chunks::<4>().0 {
                let a = if alpha { px[0] } else { 255 };
                let un = |c: u8| if alpha && a > 0 && a < 255 { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 } else { c };
                rgba.extend_from_slice(&[un(px[1]), un(px[2]), un(px[3]), a]);
            }
        }
        3 => {
            let entry = if alpha { 4 } else { 3 };
            let colors = raw.get(..table * entry)?;
            let stride = w.div_ceil(4) * 4;
            for y in 0..h {
                for x in 0..w {
                    let i = *raw.get(table * entry + y * stride + x)? as usize;
                    let c = colors.get(i * entry..i * entry + entry)?;
                    rgba.extend_from_slice(&[c[0], c[1], c[2], if alpha { c[3] } else { 255 }]);
                }
            }
        }
        _ => return None,
    }
    Some((id, w as u32, h as u32, rgba))
}

/// URLs of the movies this one imports (`ImportAssets`/`ImportAssets2`), in
/// file order and without duplicates. Scaleform splits its UI into shared
/// libraries (`../SharedAssets/SharedAssets.swf`, the font library, …) that
/// the engine loads from the workspace; Ruffle needs them too.
pub fn import_urls(m: &Movie) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    for t in &m.tags {
        if t.code != 71 && t.code != 57 {
            continue;
        }
        let (url, _) = cstr(&t.data, 0);
        if !url.is_empty() && !urls.iter().any(|u| u.eq_ignore_ascii_case(&url)) {
            urls.push(url);
        }
    }
    urls
}

/// The URL and symbol names of an `ImportAssets`/`ImportAssets2` tag.
pub fn import_entries(t: &Tag) -> Option<(String, Vec<String>)> {
    let d = &t.data;
    let url_len = d.iter().position(|&b| b == 0)?;
    let head = url_len + 1 + if t.code == 71 { 2 } else { 0 };
    let n = u16_at(d, head)?;
    let mut pos = head + 2;
    let mut names = Vec::with_capacity(n as usize);
    for _ in 0..n {
        u16_at(d, pos)?;
        let (name, next) = cstr(d, pos + 2);
        names.push(name);
        pos = next;
    }
    Some((String::from_utf8_lossy(&d[..url_len]).into_owned(), names))
}

/// Point an import tag at another URL.
pub fn set_import_url(t: &mut Tag, url: &str) {
    if let Some(url_len) = t.data.iter().position(|&b| b == 0) {
        let mut d = url.as_bytes().to_vec();
        d.extend_from_slice(&t.data[url_len..]);
        t.data = d;
    }
}

/// Merge the `ImportAssets`/`ImportAssets2` tags of one URL into the first of
/// them, keeping only the symbols `keep` accepts (a tag left empty is
/// dropped). Flash writes one tag per imported symbol and Ruffle loads and
/// parses the library again for every tag: the Home movie loaded the 1.4 MB
/// font library eight times. Returns whether anything changed.
pub fn merge_imports(tags: &mut Vec<Tag>, keep: impl Fn(u16) -> bool) -> bool {
    struct Group {
        at: usize,
        code: u16,
        url_len: usize,
        head: Vec<u8>,
        entries: Vec<u8>,
        count: u16,
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut dropped = vec![false; tags.len()];
    let mut changed = false;
    for (i, t) in tags.iter().enumerate() {
        if t.code != 57 && t.code != 71 {
            continue;
        }
        let d = &t.data;
        let Some(url_len) = d.iter().position(|&b| b == 0) else { continue };
        let head_len = url_len + 1 + if t.code == 71 { 2 } else { 0 };
        let Some(n) = u16_at(d, head_len) else { continue };
        let mut pos = head_len + 2;
        let mut entries = Vec::new();
        let mut count = 0u16;
        for _ in 0..n {
            let Some(id) = u16_at(d, pos) else { break };
            let end = d[pos + 2..].iter().position(|&b| b == 0).map_or(d.len(), |e| pos + 2 + e + 1);
            if keep(id) {
                entries.extend_from_slice(&d[pos..end]);
                count += 1;
            } else {
                changed = true;
            }
            pos = end;
        }
        let url = &d[..url_len];
        match groups.iter_mut().find(|g| g.code == t.code && g.head[..g.url_len].eq_ignore_ascii_case(url)) {
            Some(g) => {
                g.entries.extend_from_slice(&entries);
                g.count += count;
                dropped[i] = true;
                changed = true;
            }
            None => groups.push(Group { at: i, code: t.code, url_len, head: d[..head_len].to_vec(), entries, count }),
        }
    }
    if !changed {
        return false;
    }
    for g in groups {
        if g.count == 0 {
            dropped[g.at] = true;
            continue;
        }
        let mut data = g.head;
        data.extend_from_slice(&g.count.to_le_bytes());
        data.extend_from_slice(&g.entries);
        tags[g.at].data = data;
    }
    let mut i = 0;
    tags.retain(|_| {
        i += 1;
        !dropped[i - 1]
    });
    true
}

/// Everything the Studio shows about a movie.
pub fn info(m: &Movie) -> J {
    let (xmin, xmax, ymin, ymax) = m.frame();
    let mut tags = Vec::new();
    let mut images = Vec::new();
    let mut texts = Vec::new();
    let mut exports = Vec::new();
    let mut imports = Vec::new();
    for (i, t) in m.tags.iter().enumerate() {
        let mut detail = String::new();
        match t.code {
            1001 | 1009 => {
                if let Some(img) = parse_external_image(t.code, &t.data) {
                    detail = format!("{} {}×{} {}", img.id, img.width, img.height, img.file_name);
                    images.push(json!({ "tag": i, "id": img.id, "format": img.format, "width": img.width, "height": img.height, "exportName": img.export_name, "fileName": img.file_name }));
                }
            }
            37 => {
                if let Some(et) = parse_edit_text(&t.data) {
                    detail = format!("{} {}", et.id, et.variable);
                    texts.push(json!({ "tag": i, "id": et.id, "variable": et.variable, "text": et.text, "fontId": et.font_id, "fontClass": et.font_class, "fontHeight": et.font_height.map(|h| h as f32 / 20.0), "html": et.flags & 0x0002 != 0, "multiline": et.flags & 0x2000 != 0 }));
                }
            }
            59 | 12 => {
                let (sprite, body) = if t.code == 59 { (u16_at(&t.data, 0), t.data.get(2..).unwrap_or(&[])) } else { (None, &t.data[..]) };
                detail = sprite.map(|s| format!("sprite {s}, {} bytes", body.len())).unwrap_or_else(|| format!("{} bytes", body.len()));
            }
            56 => {
                let n = u16_at(&t.data, 0).unwrap_or(0) as usize;
                let mut pos = 2;
                let mut names = Vec::new();
                for _ in 0..n {
                    let id = u16_at(&t.data, pos).unwrap_or(0);
                    let (s, next) = cstr(&t.data, pos + 2);
                    names.push(format!("{id}={s}"));
                    exports.push(json!({ "tag": i, "id": id, "name": s }));
                    pos = next;
                }
                detail = names.join(", ");
            }
            71 | 57 => {
                let (url, mut pos) = cstr(&t.data, 0);
                if t.code == 71 {
                    pos += 2;
                }
                let n = u16_at(&t.data, pos).unwrap_or(0) as usize;
                pos += 2;
                let mut names = Vec::new();
                for _ in 0..n {
                    let id = u16_at(&t.data, pos).unwrap_or(0);
                    let (s, next) = cstr(&t.data, pos + 2);
                    names.push(json!({ "id": id, "name": s }));
                    pos = next;
                }
                detail = format!("{url} ({} symbols)", names.len());
                imports.push(json!({ "tag": i, "url": url, "symbols": names }));
            }
            39 => {
                let id = u16_at(&t.data, 0).unwrap_or(0);
                let frames = u16_at(&t.data, 2).unwrap_or(0);
                let inner = parse_tags(t.data.get(4..).unwrap_or(&[])).map(|v| v.len()).unwrap_or(0);
                detail = format!("{id}: {frames} frames, {inner} tags");
            }
            43 => detail = cstr(&t.data, 0).0,
            9 => detail = format!("#{}", t.data.iter().map(|b| format!("{b:02x}")).collect::<String>()),
            _ => {}
        }
        tags.push(json!({ "index": i, "code": t.code, "name": tag_name(t.code), "len": t.data.len(), "id": define_id(t.code, &t.data), "detail": detail }));
    }
    let scripts: Vec<J> = as2::scripts(m)
        .into_iter()
        .map(|s| {
            let (owner, tag) = match s.loc {
                as2::Loc::Tag { sprite, tag } | as2::Loc::Clip { sprite, tag, .. } => (sprite, tag),
                as2::Loc::Button { tag, .. } => (0, tag),
            };
            // The class a DoInitAction defines is its sprite's; other scripts belong to the timeline they are on.
            let sprite = if s.kind == "DoInitAction" { s.label.parse::<u16>().ok() } else { (owner != 0).then_some(owner) };
            let strings: Vec<J> = if owner == 0 && matches!(s.loc, as2::Loc::Tag { .. }) { script_strings(&s.bytes).iter().map(|s| json!({ "index": s.index, "text": s.text, "action": s.action })).collect() } else { Vec::new() };
            let placed = s.placed.as_ref().map(|(depth, character, name)| json!({ "depth": depth, "character": character, "name": name }));
            json!({ "key": s.loc.key(), "loc": s.loc.to_json(), "kind": s.kind, "label": s.label, "tag": tag, "owner": owner, "sprite": sprite, "placed": placed, "frame": s.frame, "bytes": s.bytes.len(), "strings": strings })
        })
        .collect();
    json!({
        "signature": String::from_utf8_lossy(&m.signature),
        "version": m.version,
        "compressed": matches!(&m.signature, b"CFX" | b"CWS"),
        "width": xmax - xmin,
        "height": ymax - ymin,
        "frameRate": m.frame_rate(),
        "frames": m.frame_count(),
        "tags": tags,
        "images": images,
        "texts": texts,
        "scripts": scripts,
        "exports": exports,
        "imports": imports,
    })
}

/// The tag stream of a sprite (0: the root timeline), handed to `f` and
/// written back into the `DefineSprite` afterwards.
fn with_stream<T>(m: &mut Movie, sprite: u16, f: impl FnOnce(&mut Vec<Tag>) -> Result<T>) -> Result<T> {
    if sprite == 0 {
        return f(&mut m.tags);
    }
    let t = m.tags.iter_mut().find(|t| t.code == 39 && u16_at(&t.data, 0) == Some(sprite)).ok_or_else(|| Error::Format(format!("no sprite {sprite}")))?;
    let mut body = parse_tags(t.data.get(4..).unwrap_or(&[]))?;
    let r = f(&mut body)?;
    let mut data = t.data[..4].to_vec();
    write_tags(&body, &mut data);
    t.data = data;
    Ok(r)
}

/// A placement edit: `{kind: "place", sprite, tag, matrix?, name?, cxform?, character?, depth?}`
/// rewrites the place tag at stream index `tag`. `name: null` / `cxform: null`
/// remove the field; omitted fields stay.
fn edit_place(m: &mut Movie, e: &J) -> Result<()> {
    let sprite = e.get("sprite").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
    let tag = e.get("tag").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("place edit without tag".into()))? as usize;
    with_stream(m, sprite, |tags| {
        let t = tags.get_mut(tag).ok_or_else(|| Error::Format(format!("no tag {tag} in sprite {sprite}")))?;
        let mut p = display::Place::parse(t.code, &t.data).ok_or_else(|| Error::Format(format!("tag {tag} is not a placement")))?;
        if let Some(mx) = e.get("matrix").filter(|v| v.is_object()) {
            p.matrix = Some(display::Matrix::from_json(mx, p.matrix.unwrap_or_default()));
        }
        if let Some(v) = e.get("name") {
            if p.code == 4 {
                return Err(Error::Format("a PlaceObject (version 1) has no name".into()));
            }
            p.name = v.as_str().filter(|s| !s.is_empty()).map(str::to_string);
        }
        if let Some(v) = e.get("cxform") {
            p.cxform = if v.is_null() { None } else { Some(display::Cxform::from_json(v, p.cxform.unwrap_or_default())) };
        }
        if let Some(c) = e.get("character").and_then(|v| v.as_u64()) {
            p.character = Some(c as u16);
        }
        if let Some(d) = e.get("depth").and_then(|v| v.as_u64()) {
            p.depth = d as u16;
        }
        t.data = p.write();
        t.long = t.long || t.data.len() >= 0x3F;
        Ok(())
    })
}

/// The font a character id names, parsed (for static texts).
fn font_of(m: &Movie, id: u16) -> Option<font::Font> {
    let tag = m.tags.iter().position(|t| font::is_font(t.code) && u16_at(&t.data, 0) == Some(id))?;
    font::Font::parse(m, tag).ok()
}

/// `{kind: "staticText", tag, record, text?, color?}`.
fn edit_static_text(m: &mut Movie, e: &J) -> Result<()> {
    let tag = e.get("tag").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("staticText without tag".into()))? as usize;
    let t = m.tags.get(tag).ok_or_else(|| Error::Format(format!("no tag {tag}")))?;
    let mut st = text::StaticText::parse(t).ok_or_else(|| Error::Format(format!("tag {tag} is not a static text")))?;
    let record = e.get("record").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let color = match e.get("color").and_then(|v| v.as_str()) {
        Some(c) => Some(shape::Rgba::parse(c).ok_or_else(|| Error::Format(format!("{c} is not a colour")))?),
        None => None,
    };
    let fonts = |id: u16| font_of(m, id);
    let known = text::known_advances(m, &|id| font_of(m, id).map(|f| f.em()).unwrap_or(1024.0));
    st.edit(record, e.get("text").and_then(|v| v.as_str()), color, &fonts, &known)?;
    m.tags[tag] = st.write();
    Ok(())
}

/// `{kind: "fontGlyphs", tag, file, chars}`: characters from a TrueType/OpenType
/// file added to a font; static texts and alignment zones follow the new glyph order.
fn add_font_glyphs(m: &mut Movie, e: &J) -> Result<()> {
    let tag = e.get("tag").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("fontGlyphs without tag".into()))? as usize;
    let file = e.get("file").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("fontGlyphs without file".into()))?;
    let chars = e.get("chars").and_then(|v| v.as_str()).unwrap_or("");
    let bytes = std::fs::read(file).map_err(|err| Error::Format(format!("{file}: {err}")))?;
    let mut f = font::Font::parse(m, tag)?;
    let (_, _, remap) = f.add_glyphs(&bytes, chars)?;
    let total = f.glyphs.len();
    m.tags[tag] = f.write()?;
    text::remap_texts(m, f.id, &remap);
    for t in m.tags.iter_mut().filter(|t| t.code == 73 && u16_at(&t.data, 0) == Some(f.id)) {
        font::remap_align_zones(t, &remap, total)?;
    }
    Ok(())
}

/// Apply edits and return how many were applied. Kinds: `text` (edit-text
/// initial text), `string` (script string), `script` / `pcode` (`{loc,
/// source}` / `{loc, pcode}`: a script replaced by compiled ActionScript or
/// assembled P-code, `loc` as `as2::Loc::from_json` reads it), `image`
/// (external image file name), `place` (see `edit_place`) and `deletePlace` (`{sprite, tag}`:
/// drop a place/remove tag; deletions run last, highest index first, so
/// the other edits' indices stay valid).
pub fn apply(m: &mut Movie, edits: &[J]) -> Result<usize> {
    let mut n = 0;
    let mut deletes: Vec<(u16, usize)> = Vec::new();
    for e in edits {
        match e.get("kind").and_then(|v| v.as_str()) {
            Some("place") => {
                edit_place(m, e)?;
                n += 1;
                continue;
            }
            Some("deletePlace") => {
                let sprite = e.get("sprite").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                let tag = e.get("tag").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("deletePlace without tag".into()))? as usize;
                deletes.push((sprite, tag));
                continue;
            }
            Some("staticText") => {
                edit_static_text(m, e)?;
                n += 1;
                continue;
            }
            Some("fontGlyphs") => {
                add_font_glyphs(m, e)?;
                n += 1;
                continue;
            }
            Some(kind @ ("script" | "pcode")) => {
                let loc = as2::Loc::from_json(e.get("loc").unwrap_or(&J::Null))?;
                let text = |k: &str| e.get(k).and_then(|v| v.as_str()).ok_or_else(|| Error::Format(format!("{kind} edit without {k}")));
                let script = if kind == "script" { as2::compile::compile(text("source")?)? } else { as2::pcode::parse(text("pcode")?)? };
                as2::replace(m, &loc, &script.encode()?)?;
                n += 1;
                continue;
            }
            _ => {}
        }
        let tag = e.get("tag").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("edit without tag".into()))? as usize;
        let t = m.tags.get_mut(tag).ok_or_else(|| Error::Format(format!("no tag {tag}")))?;
        let kind = e.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let text = e.get("text").and_then(|v| v.as_str()).unwrap_or("");
        match kind {
            "text" if t.code == 37 => {
                t.data = set_edit_text(&t.data, text).ok_or_else(|| Error::Format("bad DefineEditText".into()))?;
            }
            "string" if t.code == 59 || t.code == 12 => {
                let index = e.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let (head, body) = if t.code == 59 { (t.data[..2].to_vec(), &t.data[2..]) } else { (Vec::new(), &t.data[..]) };
                let new_body = set_script_string(body, index, text).ok_or_else(|| Error::Format(format!("string {index} cannot be edited")))?;
                let mut d = head;
                d.extend_from_slice(&new_body);
                t.data = d;
            }
            "image" if t.code == 1009 || t.code == 1001 => {
                t.data = set_external_image_file(t.code, &t.data, text).ok_or_else(|| Error::Format("bad external image tag".into()))?;
            }
            "shapeColor" if shape::is_shape(t.code) => {
                let s = shape::Shape::parse(t)?;
                let key = e.get("key").and_then(|v| v.as_str()).unwrap_or("");
                let value = e.get("value").and_then(|v| v.as_str()).unwrap_or("");
                s.patch(&mut t.data, key, value)?;
            }
            "shapeSvg" if shape::is_shape(t.code) => {
                let id = u16_at(&t.data, 0).unwrap_or(0);
                let fit = if e.get("fit").and_then(|v| v.as_bool()).unwrap_or(true) { dict::read_rect(&t.data, 2).map(|r| r.0) } else { None };
                let svg = e.get("svg").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("shapeSvg without svg".into()))?;
                *t = svg::import(id, svg.as_bytes(), fit)?.0;
            }
            "editText" if t.code == 37 => {
                let mut et = text::EditText::parse(&t.data).ok_or_else(|| Error::Format("bad DefineEditText".into()))?;
                et.set(e.get("props").unwrap_or(&J::Null))?;
                t.data = et.write();
                t.long = true;
            }
            _ => return Err(Error::Format(format!("edit {kind} does not apply to {}", tag_name(t.code)))),
        }
        n += 1;
    }
    deletes.sort_by(|a, b| b.cmp(a));
    deletes.dedup();
    for (sprite, tag) in deletes {
        with_stream(m, sprite, |tags| {
            let t = tags.get(tag).ok_or_else(|| Error::Format(format!("no tag {tag} in sprite {sprite}")))?;
            if !display::is_place(t.code) && !matches!(t.code, 5 | 28) {
                return Err(Error::Format(format!("tag {tag} is a {}, not a placement", tag_name(t.code))));
            }
            tags.remove(tag);
            Ok(())
        })?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn import2(url: &str, symbols: &[(u16, &str)]) -> Tag {
        let mut d = url.as_bytes().to_vec();
        d.extend_from_slice(&[0, 1, 0]);
        d.extend_from_slice(&(symbols.len() as u16).to_le_bytes());
        for (id, name) in symbols {
            d.extend_from_slice(&id.to_le_bytes());
            d.extend_from_slice(name.as_bytes());
            d.push(0);
        }
        Tag { code: 71, data: d, long: false }
    }

    #[test]
    fn merges_imports_per_url() {
        let shape = Tag { code: 2, data: vec![9, 0], long: false };
        let mut tags = vec![import2("../A/a.swf", &[(1, "one")]), shape.clone(), import2("../B/b.swf", &[(2, "two")]), import2("../a/A.swf", &[(3, "three"), (4, "four")])];
        assert!(merge_imports(&mut tags, |_| true));
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0].data, import2("../A/a.swf", &[(1, "one"), (3, "three"), (4, "four")]).data);
        assert_eq!(tags[1].data, shape.data);
        assert_eq!(tags[2].data, import2("../B/b.swf", &[(2, "two")]).data);
        assert!(!merge_imports(&mut tags, |_| true));

        // Pruned to the symbols kept; a library left without symbols is not imported.
        assert!(merge_imports(&mut tags, |id| id == 4));
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].data, import2("../A/a.swf", &[(4, "four")]).data);
        assert_eq!(tags[1].code, 2);
    }

    #[test]
    fn round_trips_a_small_movie() {
        // Header: GFX v8, rect of 5 zero bits (1 byte), rate, 1 frame; tags: SetBackgroundColor, DefineEditText, DoInitAction, ShowFrame, End.
        let mut body = vec![0u8, 0, 24, 1, 0];
        let mut tags = Vec::new();
        tags.push(Tag { code: 9, data: vec![1, 2, 3], long: false });
        // DefineEditText: id 7, empty rect (1 byte), flags HasText, variable "v", text "hi".
        let mut et = vec![7, 0, 0];
        et.extend_from_slice(&0x8000u16.to_be_bytes());
        et.extend_from_slice(b"v\0hi\0");
        tags.push(Tag { code: 37, data: et, long: false });
        // DoInitAction sprite 1: ConstantPool ["a","bb"], Push "x", End.
        let mut sc = vec![1, 0, 0x88, 7, 0, 2, 0];
        sc.extend_from_slice(b"a\0bb\0");
        sc.extend_from_slice(&[0x96, 3, 0, 0, b'x', 0, 0]);
        tags.push(Tag { code: 59, data: sc, long: false });
        tags.push(Tag { code: 1, data: vec![], long: false });
        tags.push(Tag { code: 0, data: vec![], long: false });
        write_tags(&tags, &mut body);
        let mut file = b"GFX".to_vec();
        file.push(8);
        file.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
        file.extend_from_slice(&body);
        let m = Movie::parse(&file).unwrap();
        assert_eq!(m.write(), file);
        let info = info(&m);
        assert_eq!(info["texts"][0]["text"], "hi");
        assert_eq!(info["scripts"][0]["strings"].as_array().unwrap().len(), 3);
        let mut m2 = m.clone();
        apply(&mut m2, &[json!({ "kind": "text", "tag": 1, "text": "hello" }), json!({ "kind": "string", "tag": 2, "index": 1, "text": "ccc" }), json!({ "kind": "string", "tag": 2, "index": 2, "text": "yy" })]).unwrap();
        let m3 = Movie::parse(&m2.write()).unwrap();
        let info3 = super::info(&m3);
        assert_eq!(info3["texts"][0]["text"], "hello");
        assert_eq!(info3["scripts"][0]["strings"][1]["text"], "ccc");
        assert_eq!(info3["scripts"][0]["strings"][2]["text"], "yy");
        let script = as2::action::Script::decode(&m3.tags[2].data[2..]).unwrap();
        assert!(as2::pcode::print(&script).contains("ConstantPool"));
    }
}
