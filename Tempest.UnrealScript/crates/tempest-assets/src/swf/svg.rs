//! SVG out and in. Export draws a character, a sprite frame or a root frame
//! as vectors: shapes with their fills (solid, gradients, bitmaps as
//! patterns), nested sprites at their first frame, static texts as glyph
//! outlines, masks as clip paths and colour transforms as filters. Edit-text
//! fields, buttons and morph shapes are left out (listed as warnings).
//! Import turns an SVG (any shapes, transforms and styles, resolved by
//! `usvg`) into a `DefineShape3`: each path its own fill or line style,
//! cubic curves split into quadratic ones.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use tempest_upk::io::{Error, Result};

use super::dict::Dict;
use super::display::{self, Cxform, Entry, Matrix};
use super::font::{cubic_to_quads, Font};
use super::shape::{num, svg_d, svg_matrix, write_shape, Bitmap, Defs, Edge, NewFill, NewPath, Pt, Rgba, Shape};
use super::text::StaticText;
use super::{Movie, Tag};

/// What to draw.
#[derive(Clone, Copy, Debug)]
pub enum Target {
    Root { frame: usize },
    Frame { sprite: u16, frame: usize },
    Character { id: u16 },
}

struct Exporter<'a> {
    m: &'a Movie,
    dict: Dict,
    defs: Defs,
    defined: HashSet<u16>,
    fonts: HashMap<u16, Option<Font>>,
    bitmaps: &'a mut dyn FnMut(u16) -> Option<Bitmap>,
    skipped: Vec<String>,
}

impl Exporter<'_> {
    fn font(&mut self, id: u16) -> Option<Font> {
        if !self.fonts.contains_key(&id) {
            let f = self.dict.chars.get(&id).and_then(|c| c.tag).and_then(|t| Font::parse(self.m, t).ok());
            self.fonts.insert(id, f);
        }
        self.fonts[&id].clone()
    }

    fn skip(&mut self, what: String) {
        if !self.skipped.contains(&what) {
            self.skipped.push(what);
        }
    }

    /// The SVG of a character in its own coordinates.
    fn character(&mut self, id: u16, depth: usize) -> String {
        let Some(c) = self.dict.chars.get(&id) else { return String::new() };
        let (kind, tag) = (c.kind, c.tag);
        let Some(tag) = tag else {
            self.skip(format!("imported symbol {id}"));
            return String::new();
        };
        let t = &self.m.tags[tag];
        match kind {
            "shape" => {
                if self.defined.insert(id) {
                    match Shape::parse(t) {
                        Ok(s) => {
                            let body = s.svg(&mut self.defs, &mut *self.bitmaps);
                            let _ = write!(self.defs.out, "<g id=\"c{id}\">{body}</g>");
                        }
                        Err(e) => {
                            self.skip(format!("shape {id}: {e}"));
                            let _ = write!(self.defs.out, "<g id=\"c{id}\"/>");
                        }
                    }
                }
                format!("<use href=\"#c{id}\"/>")
            }
            "sprite" => {
                if depth > 24 {
                    return String::new();
                }
                let body = display::sprite_body(t);
                let list = display::timeline(&body, 1).frames.into_iter().next().unwrap_or_default();
                self.list(&list, depth + 1)
            }
            "text" => match StaticText::parse(t) {
                Some(s) => self.static_text(&s),
                None => String::new(),
            },
            "edittext" => {
                self.skip("edit-text fields (their text is laid out at run time)".into());
                String::new()
            }
            "image" | "subimage" | "bitmap" => {
                // A bitmap placed directly: drawn as an image.
                match (self.bitmaps)(id) {
                    Some((w, h, png)) => format!("<image width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{}\"/>", crate::json::base64(&png)),
                    None => String::new(),
                }
            }
            other => {
                self.skip(format!("{other} characters"));
                String::new()
            }
        }
    }

    fn static_text(&mut self, s: &StaticText) -> String {
        let mut out = format!("<g transform=\"{}\">", svg_matrix(&s.matrix, 1.0));
        for (r, (font, height, color, x, y)) in s.records.iter().zip(s.resolved()) {
            let Some(f) = self.font(font) else {
                self.skip(format!("text with font {font} (not in this movie)"));
                continue;
            };
            let k = height as f64 / f.em();
            let mut pen = x as f64;
            let mut contours: Vec<Vec<Edge>> = Vec::new();
            for &(g, adv) in &r.glyphs {
                for c in f.contours(g as usize) {
                    let mv = |p: Pt| (pen + p.0 * k, y as f64 + p.1 * k);
                    contours.push(c.iter().map(|e| Edge { from: mv(e.from), ctrl: e.ctrl.map(mv), to: mv(e.to) }).collect());
                }
                pen += adv as f64;
            }
            let op = if color.a < 255 { format!(" fill-opacity=\"{}\"", num(color.a as f64 / 255.0)) } else { String::new() };
            let _ = write!(out, "<path fill=\"#{:02x}{:02x}{:02x}\"{op} fill-rule=\"evenodd\" d=\"{}\"/>", color.r, color.g, color.b, svg_d(&contours, true));
        }
        out.push_str("</g>");
        out
    }

    fn cxform(&mut self, cx: &Cxform) -> String {
        if cx.is_identity() {
            return String::new();
        }
        let only_alpha = cx.mul[..3].iter().all(|&v| v == 256) && cx.add.iter().all(|&v| v == 0);
        if only_alpha {
            return format!(" opacity=\"{}\"", num((cx.mul[3] as f64 / 256.0).clamp(0.0, 1.0)));
        }
        let id = self.defs.id("x");
        let m = |i: usize| num(cx.mul[i] as f64 / 256.0);
        let a = |i: usize| num(cx.add[i] as f64 / 255.0);
        let _ = write!(
            self.defs.out,
            "<filter id=\"{id}\" color-interpolation-filters=\"sRGB\"><feColorMatrix type=\"matrix\" values=\"{} 0 0 0 {} 0 {} 0 0 {} 0 0 {} 0 {} 0 0 0 {} {}\"/></filter>",
            m(0),
            a(0),
            m(1),
            a(1),
            m(2),
            a(2),
            m(3),
            a(3)
        );
        format!(" filter=\"url(#{id})\"")
    }

    /// A display list: objects by depth, masks as clip paths over the depths they cover.
    fn list(&mut self, list: &[Entry], depth: usize) -> String {
        let mut sorted: Vec<&Entry> = list.iter().collect();
        sorted.sort_by_key(|e| e.depth);
        let mut masks: Vec<(u16, u16, String)> = Vec::new();
        let mut out = String::new();
        for e in sorted {
            let body = self.character(e.character, depth);
            let tr = svg_matrix(&e.matrix, 1.0);
            if let Some(cd) = e.clip_depth {
                let id = self.defs.id("m");
                let _ = write!(self.defs.out, "<clipPath id=\"{id}\"><g transform=\"{tr}\">{body}</g></clipPath>");
                masks.push((e.depth, cd, id));
                continue;
            }
            if body.is_empty() || !e.visible {
                continue;
            }
            let extra = self.cxform(&e.cxform);
            let mut el = format!("<g transform=\"{tr}\"{extra}>{body}</g>");
            for (from, to, id) in &masks {
                if e.depth > *from && e.depth <= *to {
                    el = format!("<g clip-path=\"url(#{id})\">{el}</g>");
                }
            }
            out.push_str(&el);
        }
        out
    }
}

/// An SVG document of `target` and what it left out.
pub fn export(m: &Movie, target: Target, bitmaps: &mut dyn FnMut(u16) -> Option<Bitmap>) -> Result<(String, Vec<String>)> {
    let dict = Dict::build(m);
    let mut x = Exporter { m, dict, defs: Defs::default(), defined: HashSet::new(), fonts: HashMap::new(), bitmaps, skipped: Vec::new() };
    let (body, view) = match target {
        Target::Character { id } => {
            let c = x.dict.chars.get(&id).ok_or_else(|| Error::Format(format!("no character {id}")))?;
            if c.kind == "sprite" {
                return export(m, Target::Frame { sprite: id, frame: 0 }, x.bitmaps);
            }
            let r = x.dict.bounds(m, id).unwrap_or([0, 0, 20, 20]);
            (x.character(id, 0), r)
        }
        Target::Root { frame } | Target::Frame { frame, .. } => {
            let tags_owned;
            let tags: &[Tag] = match target {
                Target::Frame { sprite, .. } if sprite != 0 => {
                    let c = x.dict.chars.get(&sprite).filter(|c| c.kind == "sprite").ok_or_else(|| Error::Format(format!("no sprite {sprite}")))?;
                    tags_owned = display::sprite_body(&m.tags[c.tag.ok_or_else(|| Error::Format("imported sprite".into()))?]);
                    &tags_owned
                }
                _ => &m.tags,
            };
            let tl = display::timeline(tags, frame + 1);
            let list = tl.frames.get(frame).or(tl.frames.last()).cloned().unwrap_or_default();
            let root = matches!(target, Target::Root { .. }) || matches!(target, Target::Frame { sprite: 0, .. });
            let r = if root {
                let (x0, x1, y0, y1) = m.frame();
                [(x0 * 20.0) as i32, (y0 * 20.0) as i32, (x1 * 20.0) as i32, (y1 * 20.0) as i32]
            } else {
                x.dict.list_bounds(m, &list).unwrap_or([0, 0, 20, 20])
            };
            (x.list(&list, 0), r)
        }
    };
    let (vx, vy, vw, vh) = (view[0] as f64 / 20.0, view[1] as f64 / 20.0, (view[2] - view[0]) as f64 / 20.0, (view[3] - view[1]) as f64 / 20.0);
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\" width=\"{}\" height=\"{}\"><defs>{}</defs>{body}</svg>\n",
        num(vx),
        num(vy),
        num(vw),
        num(vh),
        num(vw),
        num(vh),
        x.defs.out
    );
    let mut warnings: Vec<String> = x.skipped.into_iter().map(|s| format!("left out: {s}")).collect();
    warnings.extend(x.defs.warnings);
    Ok((svg, warnings))
}

// ----------------------------------------------------------------------------
// Import
// ----------------------------------------------------------------------------

struct Importer {
    /// SVG user space → twips.
    to: usvg::Transform,
    paths: Vec<NewPath>,
    warnings: Vec<String>,
}

fn rgba(c: usvg::Color, opacity: f32) -> Rgba {
    Rgba { r: c.red, g: c.green, b: c.blue, a: (opacity.clamp(0.0, 1.0) * 255.0).round() as u8 }
}

fn map(t: &usvg::Transform, x: f32, y: f32) -> Pt {
    ((t.sx * x + t.kx * y + t.tx) as f64, (t.ky * x + t.sy * y + t.ty) as f64)
}

fn swf_matrix(t: &usvg::Transform) -> Matrix {
    Matrix { a: t.sx as f64, b: t.ky as f64, c: t.kx as f64, d: t.sy as f64, tx: t.tx.round() as i32, ty: t.ty.round() as i32 }
}

impl Importer {
    fn paint(&mut self, paint: &usvg::Paint, opacity: f32, abs: usvg::Transform) -> NewFill {
        let full = self.to.pre_concat(abs);
        let stops = |s: &[usvg::Stop]| -> Vec<(u8, Rgba)> { s.iter().map(|st| ((st.offset().get() * 255.0).round() as u8, rgba(st.color(), st.opacity().get() * opacity))).collect() };
        let spread = |s: usvg::SpreadMethod| match s {
            usvg::SpreadMethod::Pad => 0,
            usvg::SpreadMethod::Reflect => 1,
            usvg::SpreadMethod::Repeat => 2,
        };
        match paint {
            usvg::Paint::Color(c) => NewFill::Solid(rgba(*c, opacity)),
            usvg::Paint::LinearGradient(g) => {
                let t = full.pre_concat(g.transform());
                // The ±16384 gradient square's x axis onto (x1,y1)–(x2,y2).
                let (dx, dy) = ((g.x2() - g.x1()) / 32768.0, (g.y2() - g.y1()) / 32768.0);
                let (cx, cy) = ((g.x1() + g.x2()) / 2.0, (g.y1() + g.y2()) / 2.0);
                let local = usvg::Transform::from_row(dx, dy, -dy, dx, cx, cy);
                NewFill::Gradient { kind: 0x10, matrix: swf_matrix(&t.pre_concat(local)), spread: spread(g.spread_method()), stops: stops(g.stops()) }
            }
            usvg::Paint::RadialGradient(g) => {
                let t = full.pre_concat(g.transform());
                let r = g.r().get() / 16384.0;
                let local = usvg::Transform::from_row(r, 0.0, 0.0, r, g.cx(), g.cy());
                if (g.fx() - g.cx()).abs() > 0.01 || (g.fy() - g.cy()).abs() > 0.01 {
                    self.warnings.push("radial gradients with a focal point are drawn centred".into());
                }
                NewFill::Gradient { kind: 0x12, matrix: swf_matrix(&t.pre_concat(local)), spread: spread(g.spread_method()), stops: stops(g.stops()) }
            }
            usvg::Paint::Pattern(_) => {
                self.warnings.push("pattern fills are drawn grey".into());
                NewFill::Solid(Rgba { r: 128, g: 128, b: 128, a: (opacity * 255.0) as u8 })
            }
        }
    }

    fn contours(&self, path: &usvg::Path) -> Vec<Vec<Edge>> {
        let t = self.to.pre_concat(path.abs_transform());
        let mut out: Vec<Vec<Edge>> = Vec::new();
        let mut cur: Vec<Edge> = Vec::new();
        let (mut at, mut start) = ((0.0, 0.0), (0.0, 0.0));
        let close = |cur: &mut Vec<Edge>, out: &mut Vec<Vec<Edge>>, at: Pt, start: Pt| {
            if cur.is_empty() {
                return;
            }
            if (at.0 - start.0).abs() > 0.5 || (at.1 - start.1).abs() > 0.5 {
                cur.push(Edge { from: at, ctrl: None, to: start });
            }
            out.push(std::mem::take(cur));
        };
        use usvg::tiny_skia_path::PathSegment as S;
        for seg in path.data().segments() {
            match seg {
                S::MoveTo(p) => {
                    close(&mut cur, &mut out, at, start);
                    at = map(&t, p.x, p.y);
                    start = at;
                }
                S::LineTo(p) => {
                    let to = map(&t, p.x, p.y);
                    cur.push(Edge { from: at, ctrl: None, to });
                    at = to;
                }
                S::QuadTo(c, p) => {
                    let to = map(&t, p.x, p.y);
                    cur.push(Edge { from: at, ctrl: Some(map(&t, c.x, c.y)), to });
                    at = to;
                }
                S::CubicTo(c1, c2, p) => {
                    let to = map(&t, p.x, p.y);
                    cur.extend(cubic_to_quads(at, map(&t, c1.x, c1.y), map(&t, c2.x, c2.y), to));
                    at = to;
                }
                S::Close => {
                    close(&mut cur, &mut out, at, start);
                    at = start;
                }
            }
        }
        close(&mut cur, &mut out, at, start);
        out
    }

    fn group(&mut self, g: &usvg::Group, opacity: f32) {
        let opacity = opacity * g.opacity().get();
        for node in g.children() {
            match node {
                usvg::Node::Group(c) => self.group(c, opacity),
                usvg::Node::Path(p) => {
                    if !p.is_visible() {
                        continue;
                    }
                    let contours = self.contours(p);
                    if contours.is_empty() {
                        continue;
                    }
                    if let Some(f) = p.fill() {
                        let fill = self.paint(f.paint(), f.opacity().get() * opacity, p.abs_transform());
                        self.paths.push(NewPath { fill: Some(fill), line: None, contours: contours.clone() });
                    }
                    if let Some(s) = p.stroke() {
                        // Lines are solid: a gradient stroke takes its first colour.
                        let c = match s.paint() {
                            usvg::Paint::Color(c) => rgba(*c, s.opacity().get() * opacity),
                            usvg::Paint::LinearGradient(g) => g.stops().first().map(|st| rgba(st.color(), st.opacity().get() * s.opacity().get() * opacity)).unwrap_or(Rgba { r: 0, g: 0, b: 0, a: 255 }),
                            usvg::Paint::RadialGradient(g) => g.stops().first().map(|st| rgba(st.color(), st.opacity().get() * s.opacity().get() * opacity)).unwrap_or(Rgba { r: 0, g: 0, b: 0, a: 255 }),
                            usvg::Paint::Pattern(_) => Rgba { r: 128, g: 128, b: 128, a: 255 },
                        };
                        let t = self.to.pre_concat(p.abs_transform());
                        let scale = ((t.sx * t.sy - t.kx * t.ky).abs() as f64).sqrt();
                        let w = (s.width().get() as f64 * scale).round().clamp(0.0, 65535.0) as u16;
                        self.paths.push(NewPath { fill: None, line: Some((w, c)), contours });
                    }
                }
                usvg::Node::Image(_) => self.warnings.push("images inside the SVG are left out".into()),
                usvg::Node::Text(_) => self.warnings.push("texts inside the SVG are left out (convert them to paths)".into()),
            }
        }
    }
}

/// A `DefineShape3` with character `id` drawing the SVG: its viewport fitted
/// into `fit` (twips, aspect ratio kept, centred) or, without one, one SVG
/// unit per pixel from the shape's origin.
pub fn import(id: u16, svg: &[u8], fit: Option<[i32; 4]>) -> Result<(Tag, Vec<String>)> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).map_err(|e| Error::Format(format!("not an SVG: {e}")))?;
    let size = tree.size();
    let to = match fit {
        Some(r) => {
            let (w, h) = ((r[2] - r[0]) as f32, (r[3] - r[1]) as f32);
            let k = (w / size.width()).min(h / size.height());
            let (ox, oy) = (r[0] as f32 + (w - size.width() * k) / 2.0, r[1] as f32 + (h - size.height() * k) / 2.0);
            usvg::Transform::from_row(k, 0.0, 0.0, k, ox, oy)
        }
        None => usvg::Transform::from_scale(20.0, 20.0),
    };
    let mut im = Importer { to, paths: Vec::new(), warnings: Vec::new() };
    im.group(tree.root(), 1.0);
    if im.paths.is_empty() {
        return Err(Error::Format("the SVG draws nothing".into()));
    }
    im.warnings.dedup();
    Ok((write_shape(id, &im.paths), im.warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_and_exports_an_svg() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50">
            <defs><linearGradient id="g" x1="0" y1="0" x2="100" y2="0" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff" stop-opacity="0.5"/></linearGradient></defs>
            <rect x="0" y="0" width="100" height="50" fill="url(#g)"/>
            <circle cx="25" cy="25" r="20" fill="#00ff00" stroke="#000" stroke-width="2"/>
        </svg>"##;
        let (tag, warnings) = import(5, svg, None).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let s = Shape::parse(&tag).unwrap();
        assert_eq!(s.groups[0].fills.len(), 2);
        assert_eq!(s.groups[0].lines.len(), 1);
        assert_eq!(s.groups[0].lines[0].width, 40);
        assert_eq!(s.bounds, [0, 0, 2000, 1000]);
        let m = Movie { signature: *b"FWS", version: 8, header: vec![0, 0, 24, 1, 0], tags: vec![tag, Tag { code: 0, data: Vec::new(), long: false }] };
        let (out, w) = export(&m, Target::Character { id: 5 }, &mut |_| None).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert!(out.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 50\""), "{out}");
        assert!(out.contains("<linearGradient"), "{out}");
        // Exported SVG imports again, fitted to the same bounds.
        let (again, _) = import(5, out.as_bytes(), Some([0, 0, 2000, 1000])).unwrap();
        let s2 = Shape::parse(&again).unwrap();
        assert_eq!(s2.groups[0].fills.len(), 2);
        assert!((s2.bounds[2] - 2000).abs() <= 20 && (s2.bounds[3] - 1000).abs() <= 20, "{:?}", s2.bounds);
    }
}
