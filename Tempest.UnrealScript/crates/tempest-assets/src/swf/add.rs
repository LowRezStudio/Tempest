//! New characters in a movie: a `DefineExternalImage2` (1009) naming a
//! texture of the package, a shape that draws it (a rectangle with a
//! clipped bitmap fill, as the GFx exporter writes them), and a
//! `PlaceObject2` putting a character on a timeline frame.

use tempest_upk::io::{Error, Result};

use super::display::{is_place, BitWriter, Matrix, Place};
use super::{parse_external_image, with_stream, Movie, Tag};

/// Tags that open a movie before any definition; new definitions go after them.
const HEADER_TAGS: &[u16] = &[1000, 69, 77, 9, 65, 64, 58, 86];

/// Twips per pixel.
const TWIPS: i32 = 20;

/// The external image every new one copies its id namespace, format code and
/// file extension from (the movie's first), if any.
pub struct ImageStyle {
    /// High half of the 32-bit GFx image id (`0x0009_0000` in some movies).
    pub namespace: u32,
    pub format: u16,
    /// `.tga`, `.dds`…
    pub extension: String,
}

impl ImageStyle {
    pub fn of(m: &Movie) -> ImageStyle {
        let first = m.tags.iter().find_map(|t| match t.code {
            1001 | 1009 => parse_external_image(t.code, &t.data),
            _ => None,
        });
        match first {
            Some(img) => ImageStyle {
                namespace: img.id & 0xFFFF_0000,
                format: img.format,
                extension: img.file_name.rfind('.').map(|i| img.file_name[i..].to_string()).unwrap_or_else(|| ".tga".into()),
            },
            // 13 is what the game's exporter writes for its TGA references.
            None => ImageStyle { namespace: 0, format: 13, extension: ".tga".into() },
        }
    }
}

/// The lowest character id above every one in use (definitions and imports).
pub fn next_id(m: &Movie) -> Result<u16> {
    let dict = super::dict::Dict::build(m);
    let max = dict.chars.keys().copied().filter(|&id| id != 0xFFFF).max().unwrap_or(0);
    // 0xFFFF is the "no bitmap" id of fill styles.
    if max >= 0xFFFE {
        return Err(Error::Format("the movie has no free character id".into()));
    }
    Ok(max + 1)
}

fn pstr(out: &mut Vec<u8>, s: &str) -> Result<()> {
    if s.len() > 255 {
        return Err(Error::Format(format!("{s}: names are limited to 255 bytes")));
    }
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

/// `DefineExternalImage2`: character `id`, drawn at `width`×`height` pixels,
/// loaded from `file_name` (the engine resolves it to a Texture2D).
pub fn external_image(style: &ImageStyle, id: u16, width: u32, height: u32, file_name: &str) -> Result<Tag> {
    if width == 0 || height == 0 || width > 0xFFFF || height > 0xFFFF {
        return Err(Error::Format(format!("an image of {width}×{height} cannot be referenced")));
    }
    let mut d = Vec::new();
    d.extend_from_slice(&(style.namespace | id as u32).to_le_bytes());
    d.extend_from_slice(&style.format.to_le_bytes());
    d.extend_from_slice(&(width as u16).to_le_bytes());
    d.extend_from_slice(&(height as u16).to_le_bytes());
    pstr(&mut d, "")?;
    pstr(&mut d, file_name)?;
    Ok(Tag { code: 1009, data: d, long: true })
}

/// Bits a signed value needs.
fn sbits(v: i32) -> usize {
    let mag = if v < 0 { !v } else { v } as u32;
    (32 - mag.leading_zeros()) as usize + 1
}

fn rect(bw: &mut BitWriter, x0: i32, x1: i32, y0: i32, y1: i32) {
    let n = [x0, x1, y0, y1].into_iter().map(sbits).max().unwrap_or(1);
    bw.bits(5, n as u32);
    for v in [x0, x1, y0, y1] {
        bw.signed(n, v);
    }
}

/// `DefineShape` `id`: a `width`×`height` pixel rectangle with its top-left
/// corner at the origin, filled with `bitmap` (clipped, smoothed) at one
/// image pixel per stage pixel.
pub fn bitmap_shape(id: u16, bitmap: u16, width: u32, height: u32) -> Tag {
    let (w, h) = (width as i32 * TWIPS, height as i32 * TWIPS);
    let mut d = id.to_le_bytes().to_vec();
    let mut bw = BitWriter::default();
    rect(&mut bw, 0, w, 0, h);
    d.extend_from_slice(&bw.finish());
    // One fill style: clipped bitmap, image pixels scaled to twips.
    d.push(1);
    d.push(0x41);
    d.extend_from_slice(&bitmap.to_le_bytes());
    d.extend_from_slice(&Matrix { a: TWIPS as f64, d: TWIPS as f64, ..Matrix::default() }.to_bytes());
    // No line styles.
    d.push(0);
    let mut bw = BitWriter::default();
    bw.bits(4, 1); // NumFillBits
    bw.bits(4, 0); // NumLineBits
    // StyleChangeRecord: fill style 1, move to the origin.
    bw.bits(1, 0);
    bw.bits(5, 0b0_0101); // FillStyle1, MoveTo
    bw.bits(5, 1);
    bw.signed(1, 0);
    bw.signed(1, 0);
    bw.bits(1, 1);
    // The four edges, clockwise.
    for (dx, dy) in [(w, 0), (0, h), (-w, 0), (0, -h)] {
        let v = if dx != 0 { dx } else { dy };
        let n = sbits(v).max(2);
        bw.bits(1, 1); // edge
        bw.bits(1, 1); // straight
        bw.bits(4, (n - 2) as u32);
        bw.bits(1, 0); // not a general line
        bw.bits(1, (dx == 0) as u32); // vertical
        bw.signed(n, v);
    }
    // EndShapeRecord.
    bw.bits(6, 0);
    d.extend_from_slice(&bw.finish());
    Tag { code: 2, data: d, long: true }
}

/// Insert definition tags after the movie's opening tags (exporter info,
/// file attributes, metadata, background colour), before anything uses them.
pub fn insert_definitions(m: &mut Movie, tags: Vec<Tag>) {
    let at = m.tags.iter().position(|t| !HEADER_TAGS.contains(&t.code)).unwrap_or(m.tags.len());
    m.tags.splice(at..at, tags);
}

/// Place `character` on frame `frame` of `sprite` (0: the root) above every
/// object and mask of that timeline, its top-left at `(x, y)` pixels, with an
/// optional instance name. Returns the depth.
pub fn place(m: &mut Movie, sprite: u16, frame: usize, character: u16, x: f64, y: f64, name: Option<&str>) -> Result<u16> {
    with_stream(m, sprite, |tags| {
        let mut top = 0u16;
        for t in tags.iter().filter(|t| is_place(t.code)) {
            if let Some(p) = Place::parse(t.code, &t.data) {
                top = top.max(p.depth).max(p.clip_depth.unwrap_or(0));
            }
        }
        if top == 0xFFFF {
            return Err(Error::Format("the timeline has no free depth".into()));
        }
        let depth = top + 1;
        // Before the frame's ShowFrame, so the object appears on that frame
        // and stays on the following ones.
        let mut shown = 0;
        let mut at = None;
        for (i, t) in tags.iter().enumerate() {
            if t.code == 1 {
                if shown == frame {
                    at = Some(i);
                    break;
                }
                shown += 1;
            }
        }
        let at = at.ok_or_else(|| Error::Format(format!("the timeline has no frame {frame}")))?;
        let matrix = Matrix { tx: (x * TWIPS as f64).round() as i32, ty: (y * TWIPS as f64).round() as i32, ..Matrix::default() };
        let mut p = Place::new(depth, character, matrix);
        p.name = name.filter(|n| !n.is_empty()).map(str::to_string);
        let data = p.write();
        tags.insert(at, Tag { code: 26, long: data.len() >= 0x3F, data });
        Ok(depth)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swf::dict::Dict;

    fn movie() -> Movie {
        // FileAttributes, SetBackgroundColor, one shape placed on frame 0, ShowFrame, End.
        let mut m = Movie { signature: *b"FWS", version: 8, header: vec![0, 0, 24, 1, 0], tags: Vec::new() };
        m.tags.push(Tag { code: 69, data: vec![0, 0, 0, 0], long: false });
        m.tags.push(Tag { code: 9, data: vec![0, 0, 0], long: false });
        m.tags.push(bitmap_shape(1, 0xFFFF, 4, 4));
        m.tags.push(Tag { code: 26, data: Place::new(3, 1, Matrix::default()).write(), long: false });
        m.tags.push(Tag { code: 1, data: Vec::new(), long: false });
        m.tags.push(Tag { code: 0, data: Vec::new(), long: false });
        m
    }

    #[test]
    fn adds_an_image_a_shape_and_a_placement() {
        let mut m = movie();
        let style = ImageStyle::of(&m);
        let image = next_id(&m).unwrap();
        assert_eq!(image, 2);
        insert_definitions(&mut m, vec![external_image(&style, image, 361, 87, "Home_New.tga").unwrap(), bitmap_shape(image + 1, image, 361, 87)]);
        assert_eq!(m.tags[2].code, 1009);
        let depth = place(&mut m, 0, 0, image + 1, 10.0, 20.5, Some("newImage")).unwrap();
        assert_eq!(depth, 4);

        let img = parse_external_image(1009, &m.tags[2].data).unwrap();
        assert_eq!((img.id, img.format, img.width, img.height, img.file_name.as_str()), (2, 13, 361, 87, "Home_New.tga"));
        let dict = Dict::build(&m);
        let shape = &dict.chars[&3];
        assert_eq!(shape.kind, "shape");
        assert_eq!(shape.deps, vec![2]);
        assert_eq!(shape.rect, Some([0, 0, 361 * 20, 87 * 20]));

        let tl = super::super::display::timeline(&m.tags, 10);
        let e = tl.frames[0].iter().find(|e| e.depth == 4).unwrap();
        assert_eq!((e.character, e.matrix.tx, e.matrix.ty, e.name.as_deref()), (3, 200, 410, Some("newImage")));
        // The stream still parses after writing.
        let again = Movie::parse(&m.write()).unwrap();
        assert_eq!(again.tags.len(), m.tags.len());
    }
}
