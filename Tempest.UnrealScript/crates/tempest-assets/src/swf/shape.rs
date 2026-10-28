//! Shapes (`DefineShape` 1–4) and glyph outlines: the fill and line styles
//! (with the byte offset of every colour and width, so they can be patched
//! in place), the edges with absolute coordinates, the edges chained into
//! closed contours per style (what SVG export and TrueType export draw), and
//! a writer that turns contours back into a shape or a glyph.
//!
//! Coordinates are twips (1/20 px), y down. Fill sides follow SWF: walking
//! along an edge, `fill0` is on the left and `fill1` on the right.

use std::collections::HashMap;
use std::fmt::Write as _;

use tempest_upk::io::{Error, Result};

use super::dict::read_rect;
use super::display::{BitWriter, Matrix};
use super::{u16_at, BitReader, Tag};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
    }

    /// `#rrggbb` or `#rrggbbaa`.
    pub fn parse(s: &str) -> Option<Rgba> {
        let h = s.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            6 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: 255 }),
            8 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: byte(6)? }),
            _ => None,
        }
    }

    fn rgb(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// A gradient colour stop.
#[derive(Clone, Debug)]
pub struct Stop {
    pub ratio: u8,
    pub color: Rgba,
    /// Byte offset of the colour in the tag.
    pub at: usize,
}

#[derive(Clone, Debug)]
pub enum Fill {
    Solid { color: Rgba, at: usize },
    /// `kind` 0x10 linear, 0x12 radial, 0x13 focal radial.
    Gradient { kind: u8, matrix: Matrix, spread: u8, linear_rgb: bool, stops: Vec<Stop>, focal: f64 },
    /// `kind` 0x40 repeating, 0x41 clipped, 0x42/0x43 the same without smoothing.
    Bitmap { id: u16, matrix: Matrix, kind: u8 },
}

#[derive(Clone, Debug)]
pub struct LineStyle {
    pub width: u16,
    pub width_at: usize,
    /// The colour, or (LineStyle2 with a fill) the fill's first colour.
    pub color: Rgba,
    pub color_at: Option<usize>,
    pub fill: Option<Fill>,
}

/// One style group: the shape's own arrays, then one per `NewStyles` record.
#[derive(Clone, Debug, Default)]
pub struct Styles {
    pub fills: Vec<Fill>,
    pub lines: Vec<LineStyle>,
}

/// A point in twips.
pub type Pt = (f64, f64);

/// A line or quadratic curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub from: Pt,
    pub ctrl: Option<Pt>,
    pub to: Pt,
}

impl Edge {
    pub fn reversed(&self) -> Edge {
        Edge { from: self.to, ctrl: self.ctrl, to: self.from }
    }
}

#[derive(Clone, Debug)]
pub struct ShapeEdge {
    pub group: usize,
    pub fill0: u32,
    pub fill1: u32,
    pub line: u32,
    pub edge: Edge,
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub code: u16,
    pub id: u16,
    pub bounds: [i32; 4],
    pub groups: Vec<Styles>,
    pub edges: Vec<ShapeEdge>,
}

/// What a path paints: fill style or line style `index` (0-based) of `group`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    Fill(usize),
    Line(usize),
}

/// Contours of one style: closed for fills, open polylines for lines.
#[derive(Clone, Debug)]
pub struct StylePath {
    pub group: usize,
    pub paint: Paint,
    pub contours: Vec<Vec<Edge>>,
}

// ----------------------------------------------------------------------------
// Parsing
// ----------------------------------------------------------------------------

fn color(d: &[u8], pos: &mut usize, alpha: bool) -> Option<Rgba> {
    let c = Rgba { r: *d.get(*pos)?, g: *d.get(*pos + 1)?, b: *d.get(*pos + 2)?, a: if alpha { *d.get(*pos + 3)? } else { 255 } };
    *pos += if alpha { 4 } else { 3 };
    Some(c)
}

fn matrix(d: &[u8], pos: &mut usize) -> Option<Matrix> {
    let mut br = BitReader { d: d.get(*pos..)?, pos: 0 };
    let m = Matrix::read(&mut br);
    *pos += br.byte_pos();
    Some(m)
}

fn fill_style(code: u16, d: &[u8], pos: &mut usize) -> Option<Fill> {
    let alpha = code == 32 || code == 83 || code == 46 || code == 84;
    let ty = *d.get(*pos)?;
    *pos += 1;
    Some(match ty {
        0x00 => {
            let at = *pos;
            Fill::Solid { color: color(d, pos, alpha)?, at }
        }
        0x10 | 0x12 | 0x13 => {
            let m = matrix(d, pos)?;
            let b = *d.get(*pos)?;
            *pos += 1;
            let mut stops = Vec::new();
            for _ in 0..(b & 0x0F) {
                let ratio = *d.get(*pos)?;
                *pos += 1;
                let at = *pos;
                stops.push(Stop { ratio, color: color(d, pos, alpha)?, at });
            }
            let mut focal = 0.0;
            if ty == 0x13 {
                focal = u16_at(d, *pos)? as i16 as f64 / 256.0;
                *pos += 2;
            }
            Fill::Gradient { kind: ty, matrix: m, spread: b >> 6, linear_rgb: (b >> 4) & 3 == 1, stops, focal }
        }
        0x40..=0x43 => {
            let id = u16_at(d, *pos)?;
            *pos += 2;
            Fill::Bitmap { id, matrix: matrix(d, pos)?, kind: ty }
        }
        _ => return None,
    })
}

fn count(code: u16, d: &[u8], pos: &mut usize) -> Option<usize> {
    let mut n = *d.get(*pos)? as usize;
    *pos += 1;
    if n == 0xFF && code != 2 {
        n = u16_at(d, *pos)? as usize;
        *pos += 2;
    }
    Some(n)
}

fn styles(code: u16, d: &[u8], pos: &mut usize) -> Option<Styles> {
    let mut s = Styles::default();
    for _ in 0..count(code, d, pos)? {
        s.fills.push(fill_style(code, d, pos)?);
    }
    for _ in 0..count(code, d, pos)? {
        let width_at = *pos;
        let width = u16_at(d, *pos)?;
        *pos += 2;
        if code == 83 {
            let f0 = *d.get(*pos)?;
            *pos += 2;
            if (f0 >> 4) & 3 == 2 {
                *pos += 2; // miter limit
            }
            if f0 & 0x08 != 0 {
                let fill = fill_style(code, d, pos)?;
                let (c, at) = match &fill {
                    Fill::Solid { color, at } => (*color, Some(*at)),
                    Fill::Gradient { stops, .. } => (stops.first().map(|s| s.color).unwrap_or(Rgba { r: 0, g: 0, b: 0, a: 255 }), None),
                    Fill::Bitmap { .. } => (Rgba { r: 0, g: 0, b: 0, a: 255 }, None),
                };
                s.lines.push(LineStyle { width, width_at, color: c, color_at: at, fill: Some(fill) });
                continue;
            }
        }
        let at = *pos;
        let c = color(d, pos, code == 32 || code == 83)?;
        s.lines.push(LineStyle { width, width_at, color: c, color_at: Some(at), fill: None });
    }
    Some(s)
}

/// Shape records from `pos` (NumFillBits/NumLineBits first). `code` decides
/// whether `NewStyles` records can appear (not in DefineShape or glyphs: pass 0).
fn records(code: u16, d: &[u8], mut pos: usize, groups: &mut Vec<Styles>) -> Option<Vec<ShapeEdge>> {
    let mut edges = Vec::new();
    let mut br = BitReader { d: d.get(pos..)?, pos: 0 };
    let mut fill_bits = br.bits(4) as usize;
    let mut line_bits = br.bits(4) as usize;
    let (mut x, mut y) = (0i32, 0i32);
    let (mut f0, mut f1, mut ln) = (0u32, 0u32, 0u32);
    let mut group = groups.len().saturating_sub(1);
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
                x = br.signed(n);
                y = br.signed(n);
            }
            if flags & 0x02 != 0 {
                f0 = br.bits(fill_bits);
            }
            if flags & 0x04 != 0 {
                f1 = br.bits(fill_bits);
            }
            if flags & 0x08 != 0 {
                ln = br.bits(line_bits);
            }
            if flags & 0x10 != 0 && code != 2 && code != 0 {
                br.align();
                let mut p = pos + br.byte_pos();
                groups.push(styles(code, d, &mut p)?);
                group = groups.len() - 1;
                br = BitReader { d: d.get(p..)?, pos: 0 };
                pos = p;
                fill_bits = br.bits(4) as usize;
                line_bits = br.bits(4) as usize;
            }
        } else {
            let straight = br.bit() == 1;
            let n = br.bits(4) as usize + 2;
            let from = (x as f64, y as f64);
            let edge = if straight {
                let (dx, dy) = if br.bit() == 1 {
                    (br.signed(n), br.signed(n))
                } else if br.bit() == 1 {
                    (0, br.signed(n))
                } else {
                    (br.signed(n), 0)
                };
                x += dx;
                y += dy;
                Edge { from, ctrl: None, to: (x as f64, y as f64) }
            } else {
                let (cx, cy) = (x + br.signed(n), y + br.signed(n));
                x = cx + br.signed(n);
                y = cy + br.signed(n);
                Edge { from, ctrl: Some((cx as f64, cy as f64)), to: (x as f64, y as f64) }
            };
            edges.push(ShapeEdge { group, fill0: f0, fill1: f1, line: ln, edge });
        }
    }
    Some(edges)
}

pub fn is_shape(code: u16) -> bool {
    matches!(code, 2 | 22 | 32 | 83)
}

impl Shape {
    pub fn parse(t: &Tag) -> Result<Shape> {
        Self::parse_inner(t).ok_or_else(|| Error::Format(format!("{} does not parse as a shape", super::tag_name(t.code))))
    }

    fn parse_inner(t: &Tag) -> Option<Shape> {
        if !is_shape(t.code) {
            return None;
        }
        let d = &t.data;
        let id = u16_at(d, 0)?;
        let (bounds, mut pos) = read_rect(d, 2)?;
        if t.code == 83 {
            pos = read_rect(d, pos)?.1 + 1;
        }
        let mut groups = vec![styles(t.code, d, &mut pos)?];
        let edges = records(t.code, d, pos, &mut groups)?;
        Some(Shape { code: t.code, id, bounds, groups, edges })
    }

    /// The glyph outline of a font (`SHAPE` without styles: fill style 1).
    pub fn glyph(d: &[u8]) -> Option<Shape> {
        let mut groups = vec![Styles::default()];
        let edges = records(0, d, 0, &mut groups)?;
        Some(Shape { code: 0, id: 0, bounds: [0; 4], groups, edges })
    }

    /// Every style's contours, in drawing order: per group, the fills then the lines.
    pub fn paths(&self) -> Vec<StylePath> {
        let mut out = Vec::new();
        for (g, styles) in self.groups.iter().enumerate() {
            let fills = if self.code == 0 { 1 } else { styles.fills.len() };
            for f in 1..=fills as u32 {
                let mut edges = Vec::new();
                for e in self.edges.iter().filter(|e| e.group == g) {
                    // Both sides the same fill: an inner edge, it draws nothing.
                    if e.fill0 == f && e.fill1 == f {
                        continue;
                    }
                    if e.fill1 == f {
                        edges.push(e.edge);
                    }
                    if e.fill0 == f {
                        edges.push(e.edge.reversed());
                    }
                }
                if !edges.is_empty() {
                    out.push(StylePath { group: g, paint: Paint::Fill(f as usize - 1), contours: chain(edges, true) });
                }
            }
            for l in 1..=styles.lines.len() as u32 {
                let edges: Vec<Edge> = self.edges.iter().filter(|e| e.group == g && e.line == l).map(|e| e.edge).collect();
                if !edges.is_empty() {
                    out.push(StylePath { group: g, paint: Paint::Line(l as usize - 1), contours: chain(edges, false) });
                }
            }
        }
        out
    }
}

fn key(p: Pt) -> (i64, i64) {
    (p.0.round() as i64, p.1.round() as i64)
}

/// Join edges into contours: each edge continues the contour that ends where
/// it starts; with `merge`, contours are then joined end to start until none
/// can be (fills: every contour closes).
fn chain(edges: Vec<Edge>, merge: bool) -> Vec<Vec<Edge>> {
    let mut contours: Vec<Vec<Edge>> = Vec::new();
    for e in edges {
        match contours.last_mut() {
            Some(c) if key(c.last().unwrap().to) == key(e.from) => c.push(e),
            _ => contours.push(vec![e]),
        }
    }
    if !merge {
        return contours;
    }
    let mut done: Vec<Vec<Edge>> = Vec::new();
    let mut open: Vec<Option<Vec<Edge>>> = contours.into_iter().map(Some).collect();
    let mut by_start: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, c) in open.iter().enumerate() {
        by_start.entry(key(c.as_ref().unwrap()[0].from)).or_default().push(i);
    }
    for i in 0..open.len() {
        let Some(mut c) = open[i].take() else { continue };
        if let Some(v) = by_start.get_mut(&key(c[0].from)) {
            v.retain(|&j| j != i);
        }
        loop {
            let end = key(c.last().unwrap().to);
            if end == key(c[0].from) {
                break;
            }
            let next = by_start.get_mut(&end).and_then(|v| v.pop());
            match next.and_then(|j| open[j].take()) {
                Some(more) => c.extend(more),
                None => break,
            }
        }
        done.push(c);
    }
    done
}

// ----------------------------------------------------------------------------
// Geometry
// ----------------------------------------------------------------------------

/// A contour as a polygon (curves flattened into a few segments).
pub fn flatten(c: &[Edge]) -> Vec<Pt> {
    let mut pts = Vec::with_capacity(c.len() * 2);
    for e in c {
        pts.push(e.from);
        if let Some(q) = e.ctrl {
            for k in 1..4 {
                let t = k as f64 / 4.0;
                let u = 1.0 - t;
                pts.push((u * u * e.from.0 + 2.0 * u * t * q.0 + t * t * e.to.0, u * u * e.from.1 + 2.0 * u * t * q.1 + t * t * e.to.1));
            }
        }
    }
    pts
}

/// Shoelace area, y down: positive when the contour turns clockwise on screen.
pub fn area(pts: &[Pt]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i].0 * pts[(i + 1) % n].1 - pts[(i + 1) % n].0 * pts[i].1).sum::<f64>() / 2.0
}

fn inside(p: Pt, poly: &[Pt]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
    }
    c
}

/// For each contour, whether its inside is filled (even-odd: a contour nested
/// in an odd number of others is a hole).
pub fn filled_inside(contours: &[Vec<Edge>]) -> Vec<bool> {
    let polys: Vec<Vec<Pt>> = contours.iter().map(|c| flatten(c)).collect();
    (0..polys.len())
        .map(|i| {
            // A point just inside contour i: the midpoint of its first edge, nudged along the normal.
            let p = &polys[i];
            if p.len() < 3 {
                return true;
            }
            let (a, b) = (p[0], p[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt().max(1e-9);
            let side = if area(p) > 0.0 { 1.0 } else { -1.0 };
            // Clockwise on screen (y down): the inside is on the right, (-dy, dx).
            let probe = ((a.0 + b.0) / 2.0 - dy / len * side * 0.5, (a.1 + b.1) / 2.0 + dx / len * side * 0.5);
            let depth = polys.iter().enumerate().filter(|(j, q)| *j != i && q.len() >= 3 && inside(probe, q)).count();
            depth % 2 == 0
        })
        .collect()
}

// ----------------------------------------------------------------------------
// SVG
// ----------------------------------------------------------------------------

pub fn num(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// SVG path data of contours, in pixels.
pub fn svg_d(contours: &[Vec<Edge>], close: bool) -> String {
    let px = |v: f64| num(v / 20.0);
    let mut d = String::new();
    for c in contours {
        let Some(first) = c.first() else { continue };
        let _ = write!(d, "M{} {}", px(first.from.0), px(first.from.1));
        for (i, e) in c.iter().enumerate() {
            // `Z` draws the closing line.
            if close && i + 1 == c.len() && e.ctrl.is_none() && key(e.to) == key(first.from) {
                break;
            }
            match e.ctrl {
                Some(q) => {
                    let _ = write!(d, "Q{} {} {} {}", px(q.0), px(q.1), px(e.to.0), px(e.to.1));
                }
                None => {
                    let _ = write!(d, "L{} {}", px(e.to.0), px(e.to.1));
                }
            }
        }
        if close {
            d.push('Z');
        }
    }
    d
}

/// `matrix(…)` of a SWF matrix, translation in pixels (`scale`: divide a–d too).
pub fn svg_matrix(m: &Matrix, scale: f64) -> String {
    format!("matrix({} {} {} {} {} {})", num(m.a / scale), num(m.b / scale), num(m.c / scale), num(m.d / scale), num(m.tx as f64 / 20.0), num(m.ty as f64 / 20.0))
}

/// The `<defs>` an SVG document collects (gradients, bitmap patterns, shapes).
#[derive(Default)]
pub struct Defs {
    pub out: String,
    next: usize,
    pub warnings: Vec<String>,
}

impl Defs {
    pub fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }
}

/// A bitmap for a fill: width, height (pixels the movie draws it at) and PNG bytes.
pub type Bitmap = (u32, u32, Vec<u8>);

fn paint_attr(fill: &Fill, defs: &mut Defs, bitmaps: &mut dyn FnMut(u16) -> Option<Bitmap>) -> (String, f64) {
    match fill {
        Fill::Solid { color, .. } => (color.rgb(), color.a as f64 / 255.0),
        Fill::Gradient { kind, matrix, spread, linear_rgb, stops, focal } => {
            let id = defs.id("g");
            let spread = match spread {
                1 => " spreadMethod=\"reflect\"",
                2 => " spreadMethod=\"repeat\"",
                _ => "",
            };
            let interp = if *linear_rgb { " color-interpolation=\"linearRGB\"" } else { "" };
            // The gradient square is ±16384 twips; its matrix maps it to the shape.
            let tr = svg_matrix(matrix, 20.0);
            let open = if *kind == 0x10 {
                format!("<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"-16384\" y1=\"0\" x2=\"16384\" y2=\"0\" gradientTransform=\"{tr}\"{spread}{interp}>")
            } else {
                let fx = if *kind == 0x13 { format!(" fx=\"{}\" fy=\"0\"", num(focal * 16384.0)) } else { String::new() };
                format!("<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"0\" cy=\"0\" r=\"16384\"{fx} gradientTransform=\"{tr}\"{spread}{interp}>")
            };
            defs.out.push_str(&open);
            for s in stops {
                let _ = write!(defs.out, "<stop offset=\"{}\" stop-color=\"{}\"{}/>", num(s.ratio as f64 / 255.0), s.color.rgb(), if s.color.a < 255 { format!(" stop-opacity=\"{}\"", num(s.color.a as f64 / 255.0)) } else { String::new() });
            }
            defs.out.push_str(if *kind == 0x10 { "</linearGradient>" } else { "</radialGradient>" });
            (format!("url(#{id})"), 1.0)
        }
        Fill::Bitmap { id: bitmap, matrix, .. } => match bitmaps(*bitmap) {
            Some((w, h, png)) => {
                let id = defs.id("p");
                let _ = write!(
                    defs.out,
                    "<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{w}\" height=\"{h}\" patternTransform=\"{}\"><image width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{}\"/></pattern>",
                    svg_matrix(matrix, 20.0),
                    crate::json::base64(&png)
                );
                (format!("url(#{id})"), 1.0)
            }
            None => {
                defs.warnings.push(format!("bitmap {bitmap} is not available: drawn grey"));
                ("#808080".into(), 1.0)
            }
        },
    }
}

impl Shape {
    /// The shape's SVG elements (its own coordinates, pixels).
    pub fn svg(&self, defs: &mut Defs, bitmaps: &mut dyn FnMut(u16) -> Option<Bitmap>) -> String {
        let mut out = String::new();
        for p in self.paths() {
            let styles = &self.groups[p.group];
            match p.paint {
                Paint::Fill(i) => {
                    let Some(fill) = styles.fills.get(i) else { continue };
                    let (paint, opacity) = paint_attr(fill, defs, bitmaps);
                    let op = if opacity < 1.0 { format!(" fill-opacity=\"{}\"", num(opacity)) } else { String::new() };
                    let _ = write!(out, "<path fill=\"{paint}\"{op} fill-rule=\"evenodd\" d=\"{}\"/>", svg_d(&p.contours, true));
                }
                Paint::Line(i) => {
                    let Some(l) = styles.lines.get(i) else { continue };
                    let (paint, opacity) = match &l.fill {
                        Some(f) => paint_attr(f, defs, bitmaps),
                        None => (l.color.rgb(), l.color.a as f64 / 255.0),
                    };
                    let op = if opacity < 1.0 { format!(" stroke-opacity=\"{}\"", num(opacity)) } else { String::new() };
                    // Width 0 is a hairline: one pixel whatever the scale.
                    let width = if l.width == 0 { "1\" vector-effect=\"non-scaling-stroke".to_string() } else { num(l.width as f64 / 20.0) };
                    let _ = write!(out, "<path fill=\"none\" stroke=\"{paint}\"{op} stroke-width=\"{width}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" d=\"{}\"/>", svg_d(&p.contours, false));
                }
            }
        }
        out
    }

    /// Every editable colour: `fill:<group>:<index>`, `line:<group>:<index>`,
    /// `stop:<group>:<index>:<stop>`, with the value and whether alpha is stored.
    pub fn colors(&self) -> Vec<(String, Rgba)> {
        let mut v = Vec::new();
        for (g, s) in self.groups.iter().enumerate() {
            for (i, f) in s.fills.iter().enumerate() {
                match f {
                    Fill::Solid { color, .. } => v.push((format!("fill:{g}:{i}"), *color)),
                    Fill::Gradient { stops, .. } => {
                        for (k, st) in stops.iter().enumerate() {
                            v.push((format!("stop:{g}:{i}:{k}"), st.color));
                        }
                    }
                    Fill::Bitmap { .. } => {}
                }
            }
            for (i, l) in s.lines.iter().enumerate() {
                if l.color_at.is_some() {
                    v.push((format!("line:{g}:{i}"), l.color));
                }
            }
        }
        v
    }

    pub fn has_alpha(&self) -> bool {
        self.code == 32 || self.code == 83
    }

    /// Patch colours (`key` as `colors` names them) and line widths
    /// (`width:<group>:<index>`, pixels) in the tag bytes, in place.
    pub fn patch(&self, data: &mut [u8], key: &str, value: &str) -> Result<()> {
        let bad = || Error::Format(format!("no style {key} in shape {}", self.id));
        let parts: Vec<&str> = key.split(':').collect();
        let n = |i: usize| parts.get(i).and_then(|s| s.parse::<usize>().ok()).ok_or_else(bad);
        let styles = self.groups.get(n(1)?).ok_or_else(bad)?;
        if parts[0] == "width" {
            let l = styles.lines.get(n(2)?).ok_or_else(bad)?;
            let px: f64 = value.trim().parse().map_err(|_| Error::Format(format!("{value} is not a width")))?;
            let w = (px * 20.0).round().clamp(0.0, 65535.0) as u16;
            data[l.width_at..l.width_at + 2].copy_from_slice(&w.to_le_bytes());
            return Ok(());
        }
        let at = match parts[0] {
            "fill" => match styles.fills.get(n(2)?).ok_or_else(bad)? {
                Fill::Solid { at, .. } => *at,
                _ => return Err(bad()),
            },
            "stop" => match styles.fills.get(n(2)?).ok_or_else(bad)? {
                Fill::Gradient { stops, .. } => stops.get(n(3)?).ok_or_else(bad)?.at,
                _ => return Err(bad()),
            },
            "line" => styles.lines.get(n(2)?).and_then(|l| l.color_at).ok_or_else(bad)?,
            _ => return Err(bad()),
        };
        let c = Rgba::parse(value).ok_or_else(|| Error::Format(format!("{value} is not a colour (#rrggbb or #rrggbbaa)")))?;
        data[at] = c.r;
        data[at + 1] = c.g;
        data[at + 2] = c.b;
        if self.has_alpha() {
            data[at + 3] = c.a;
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// Writing
// ----------------------------------------------------------------------------

/// A fill for a new shape (DefineShape3: colours with alpha).
#[derive(Clone, Debug)]
pub enum NewFill {
    Solid(Rgba),
    /// `kind` 0x10 / 0x12; `matrix` maps the ±16384 gradient square to twips.
    Gradient { kind: u8, matrix: Matrix, spread: u8, stops: Vec<(u8, Rgba)> },
}

/// One painted path of a new shape: a fill or a line (width in twips, colour).
#[derive(Clone, Debug)]
pub struct NewPath {
    pub fill: Option<NewFill>,
    pub line: Option<(u16, Rgba)>,
    pub contours: Vec<Vec<Edge>>,
}

fn sbits(v: i32) -> usize {
    let mag = if v < 0 { !v } else { v } as u32;
    (32 - mag.leading_zeros()) as usize + 1
}

fn ubits(v: u32) -> usize {
    (32 - v.leading_zeros()) as usize
}

fn rect(bw: &mut BitWriter, r: [i32; 4]) {
    let n = r.iter().map(|&v| sbits(v)).max().unwrap_or(1);
    bw.bits(5, n as u32);
    for v in [r[0], r[2], r[1], r[3]] {
        bw.signed(n, v);
    }
}

fn rgba(out: &mut Vec<u8>, c: Rgba) {
    out.extend_from_slice(&[c.r, c.g, c.b, c.a]);
}

/// Edge records of contours with the given fill sides (`f0`/`f1` per
/// contour) and line style; coordinates rounded to whole twips, deltas
/// taken between rounded points so nothing drifts.
fn write_contours(bw: &mut BitWriter, contours: &[Vec<Edge>], sides: &[(u32, u32)], line: u32, fill_bits: usize, line_bits: usize, first_style: &mut bool) {
    let r = |p: Pt| (p.0.round() as i32, p.1.round() as i32);
    for (ci, c) in contours.iter().enumerate() {
        let Some(first) = c.first() else { continue };
        let (f0, f1) = sides[ci];
        let (mx, my) = r(first.from);
        // StyleChange: move, and the styles of this contour.
        bw.bits(1, 0);
        let set_line = *first_style && line_bits > 0;
        let flags = 0x01 | if fill_bits > 0 { 0x02 | 0x04 } else { 0 } | if set_line { 0x08 } else { 0 };
        bw.bits(5, flags);
        let n = sbits(mx).max(sbits(my));
        bw.bits(5, n as u32);
        bw.signed(n, mx);
        bw.signed(n, my);
        if fill_bits > 0 {
            bw.bits(fill_bits, f0);
            bw.bits(fill_bits, f1);
        }
        if set_line {
            bw.bits(line_bits, line);
        }
        *first_style = false;
        let (mut x, mut y) = (mx, my);
        for e in c {
            let (tx, ty) = r(e.to);
            match e.ctrl {
                Some(q) => {
                    let (qx, qy) = r(q);
                    let v = [qx - x, qy - y, tx - qx, ty - qy];
                    let n = v.iter().map(|&d| sbits(d)).max().unwrap().max(2);
                    bw.bits(1, 1);
                    bw.bits(1, 0);
                    bw.bits(4, (n - 2) as u32);
                    for d in v {
                        bw.signed(n, d);
                    }
                }
                None => {
                    let (dx, dy) = (tx - x, ty - y);
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    bw.bits(1, 1);
                    bw.bits(1, 1);
                    if dx != 0 && dy != 0 {
                        let n = sbits(dx).max(sbits(dy)).max(2);
                        bw.bits(4, (n - 2) as u32);
                        bw.bits(1, 1);
                        bw.signed(n, dx);
                        bw.signed(n, dy);
                    } else {
                        let v = if dx != 0 { dx } else { dy };
                        let n = sbits(v).max(2);
                        bw.bits(4, (n - 2) as u32);
                        bw.bits(1, 0);
                        bw.bits(1, (dx == 0) as u32);
                        bw.signed(n, v);
                    }
                }
            }
            (x, y) = (tx, ty);
        }
    }
}

/// Fill sides of each contour for fill style `f`: the filled region (inside,
/// or outside for a hole) on the right is `fill1`, on the left `fill0`.
fn sides(contours: &[Vec<Edge>], f: u32) -> Vec<(u32, u32)> {
    let inside = filled_inside(contours);
    contours
        .iter()
        .zip(inside)
        .map(|(c, ins)| {
            let clockwise = area(&flatten(c)) > 0.0;
            // Clockwise on screen: the inside is on the right.
            if clockwise == ins { (0, f) } else { (f, 0) }
        })
        .collect()
}

fn bounds_of(paths: &[NewPath]) -> [i32; 4] {
    let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for p in paths {
        let pad = p.line.map(|(w, _)| w as i32 / 2).unwrap_or(0);
        for c in &p.contours {
            for e in c {
                for q in [Some(e.from), e.ctrl, Some(e.to)].into_iter().flatten() {
                    b[0] = b[0].min(q.0.round() as i32 - pad);
                    b[1] = b[1].min(q.1.round() as i32 - pad);
                    b[2] = b[2].max(q.0.round() as i32 + pad);
                    b[3] = b[3].max(q.1.round() as i32 + pad);
                }
            }
        }
    }
    if b[0] > b[2] { [0; 4] } else { b }
}

/// A `DefineShape3` of the paths (each its own style, drawn in order).
pub fn write_shape(id: u16, paths: &[NewPath]) -> Tag {
    let fills: Vec<&NewFill> = paths.iter().filter_map(|p| p.fill.as_ref()).collect();
    let lines: Vec<(u16, Rgba)> = paths.iter().filter_map(|p| p.line).collect();
    let mut d = id.to_le_bytes().to_vec();
    let bounds = bounds_of(paths);
    let mut bw = BitWriter::default();
    rect(&mut bw, bounds);
    d.extend_from_slice(&bw.finish());
    let count = |d: &mut Vec<u8>, n: usize| {
        if n >= 0xFF {
            d.push(0xFF);
            d.extend_from_slice(&(n as u16).to_le_bytes());
        } else {
            d.push(n as u8);
        }
    };
    count(&mut d, fills.len());
    for f in &fills {
        match f {
            NewFill::Solid(c) => {
                d.push(0);
                rgba(&mut d, *c);
            }
            NewFill::Gradient { kind, matrix, spread, stops } => {
                d.push(*kind);
                d.extend_from_slice(&matrix.to_bytes());
                let stops = &stops[..stops.len().min(15)];
                d.push((spread << 6) | stops.len() as u8);
                for (ratio, c) in stops {
                    d.push(*ratio);
                    rgba(&mut d, *c);
                }
            }
        }
    }
    count(&mut d, lines.len());
    for (w, c) in &lines {
        d.extend_from_slice(&w.to_le_bytes());
        rgba(&mut d, *c);
    }
    let fill_bits = ubits(fills.len() as u32);
    let line_bits = ubits(lines.len() as u32);
    let mut bw = BitWriter::default();
    bw.bits(4, fill_bits as u32);
    bw.bits(4, line_bits as u32);
    let (mut fi, mut li) = (0u32, 0u32);
    for p in paths {
        if p.fill.is_some() {
            fi += 1;
            let s = sides(&p.contours, fi);
            let mut first = true;
            // Lines off while filling (line style 0), set once per path.
            write_contours(&mut bw, &p.contours, &s, 0, fill_bits, line_bits, &mut first);
        }
        if p.line.is_some() {
            li += 1;
            let s = vec![(0, 0); p.contours.len()];
            let mut first = true;
            write_contours(&mut bw, &p.contours, &s, li, fill_bits, line_bits, &mut first);
        }
    }
    bw.bits(1, 0);
    bw.bits(5, 0);
    d.extend_from_slice(&bw.finish());
    Tag { code: 32, data: d, long: true }
}

/// A glyph `SHAPE` (fill style 1, no lines) of contours in font units.
pub fn write_glyph(contours: &[Vec<Edge>]) -> Vec<u8> {
    let s = sides(contours, 1);
    let mut bw = BitWriter::default();
    bw.bits(4, 1);
    bw.bits(4, 0);
    let mut first = true;
    write_contours(&mut bw, contours, &s, 0, 1, 0, &mut first);
    bw.bits(1, 0);
    bw.bits(5, 0);
    bw.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64, clockwise: bool) -> Vec<Edge> {
        let mut p = vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s)];
        if !clockwise {
            p.reverse();
        }
        (0..4).map(|i| Edge { from: p[i], ctrl: None, to: p[(i + 1) % 4] }).collect()
    }

    #[test]
    fn writes_and_reads_back_a_shape_with_a_hole() {
        let red = Rgba { r: 255, g: 0, b: 0, a: 128 };
        let paths = vec![
            NewPath { fill: Some(NewFill::Solid(red)), line: None, contours: vec![square(0.0, 0.0, 200.0, true), square(50.0, 50.0, 100.0, true)] },
            NewPath { fill: None, line: Some((40, Rgba { r: 0, g: 0, b: 255, a: 255 })), contours: vec![square(0.0, 0.0, 200.0, false)] },
        ];
        let tag = write_shape(7, &paths);
        let s = Shape::parse(&tag).unwrap();
        assert_eq!(s.id, 7);
        assert_eq!(s.bounds, [-20, -20, 220, 220]);
        let ps = s.paths();
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].paint, Paint::Fill(0));
        assert_eq!(ps[0].contours.len(), 2);
        // The outer square has its inside filled, the hole its outside.
        assert_eq!(filled_inside(&ps[0].contours), vec![true, false]);
        assert_eq!(ps[1].paint, Paint::Line(0));
        assert_eq!(s.colors(), vec![("fill:0:0".to_string(), red), ("line:0:0".to_string(), Rgba { r: 0, g: 0, b: 255, a: 255 })]);
        let svg = s.svg(&mut Defs::default(), &mut |_| None);
        assert!(svg.contains("fill=\"#ff0000\" fill-opacity=\"0.5\""), "{svg}");
        assert!(svg.contains("M0 0L10 0L10 10L0 10Z"), "{svg}");

        let mut data = tag.data.clone();
        s.patch(&mut data, "fill:0:0", "#00ff00ff").unwrap();
        s.patch(&mut data, "width:0:0", "4").unwrap();
        let s2 = Shape::parse(&Tag { code: 32, data, long: true }).unwrap();
        assert_eq!(s2.colors()[0].1, Rgba { r: 0, g: 255, b: 0, a: 255 });
        assert_eq!(s2.groups[0].lines[0].width, 80);
    }

    #[test]
    fn glyphs_round_trip() {
        let g = write_glyph(&[square(0.0, -700.0, 600.0, false)]);
        let s = Shape::glyph(&g).unwrap();
        let p = s.paths();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].contours[0].len(), 4);
        assert!(filled_inside(&p[0].contours)[0]);
    }
}
