//! Preview movies: one character, or one frame of a timeline, alone on a
//! stage cut to its bounds, for the Studio's Ruffle player.
//!
//! The preview keeps the definitions the target needs (its dependency
//! closure), the imports among them (Ruffle fetches the shared libraries;
//! all of them when playing) and places
//! the target on the root frame. In the static mode it shows the layout the
//! tags describe: the target's display list at the chosen frame, nested
//! sprites cut to their first frame and no ActionScript, so what is drawn is
//! exactly what the placements say. In the play mode the target runs with
//! its scripts and timelines, like in the movie.

use super::dict::Dict;
use super::display::{self, BitWriter, Entry, Matrix, Place};
use super::{u16_at, u32_at, write_tags, Movie, Tag};

pub struct Preview {
    pub movie: Movie,
    /// What the stage shows, in the target's own coordinates (twips).
    pub rect: [i32; 4],
}

fn write_rect(r: [i32; 4]) -> Vec<u8> {
    let bits = |v: i32| if v == 0 { 1 } else { (32 - (if v < 0 { !v } else { v } as u32).leading_zeros()) as usize + 1 };
    let n = [r[0], r[2], r[1], r[3]].iter().map(|&v| bits(v)).max().unwrap_or(1);
    let mut bw = BitWriter::default();
    bw.bits(5, n as u32);
    bw.signed(n, r[0]);
    bw.signed(n, r[2]);
    bw.signed(n, r[1]);
    bw.signed(n, r[3]);
    bw.finish()
}

/// The first frame of a sprite, without scripts or sounds.
fn first_frame(t: &Tag) -> Tag {
    let mut body = Vec::new();
    for it in display::sprite_body(t) {
        match it.code {
            12 | 15 | 18 | 19 | 45 => continue,
            0 => break,
            1 => {
                body.push(it);
                break;
            }
            _ => body.push(it),
        }
    }
    body.push(Tag { code: 0, data: Vec::new(), long: false });
    let mut data = t.data[..2].to_vec();
    data.extend_from_slice(&1u16.to_le_bytes());
    write_tags(&body, &mut data);
    Tag { code: 39, data, long: true }
}

/// The characters a layout draws: like `Dict::closure`, but a sprite only
/// brings what its first frame places, since a layout cuts nested sprites to
/// that frame (`first_frame`). Everything their later frames use (hover
/// states, animations, other tabs) would otherwise be defined, converted and
/// decoded for nothing: 11.7 M pixels of bitmaps for endofmatchlobby's
/// `UIEndOfMatchLobby`.
fn first_frame_closure(m: &Movie, dict: &Dict, roots: &[u16]) -> std::collections::BTreeSet<u16> {
    let mut seen = std::collections::BTreeSet::new();
    let mut stack = roots.to_vec();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(c) = dict.chars.get(&id) else { continue };
        match (c.kind, c.tag) {
            ("sprite", Some(i)) => {
                let tl = display::timeline(&display::sprite_body(&m.tags[i]), 1);
                stack.extend(tl.frames.first().into_iter().flatten().map(|e| e.character));
            }
            _ => stack.extend(c.deps.iter().copied()),
        }
    }
    seen
}

/// What to show.
pub enum Target {
    /// The root timeline at a frame (the stage).
    Root(usize),
    /// A sprite's timeline at a frame, as a layout.
    Frame(u16, usize),
    /// One character, placed once; `play` runs its scripts and timeline.
    Character(u16, bool),
}

/// What the Studio's stage asks for besides the target.
#[derive(Default)]
pub struct Options {
    /// Depths left out of a timeline target: the object being edited, which
    /// the Studio draws on a layer of its own and moves without re-rendering.
    pub exclude: Vec<u16>,
    /// The stage area (twips), kept fixed while editing so the view does not
    /// re-fit when an object moves; the content bounds otherwise.
    pub view: Option<[i32; 4]>,
}

/// Build the preview from the movie (external images are embedded afterwards).
pub fn build(m: &Movie, dict: &Dict, target: Target, opts: &Options) -> Preview {
    let play = matches!(target, Target::Character(_, true));
    let (mut list, rect): (Vec<Entry>, Option<[i32; 4]>) = match target {
        Target::Root(frame) => {
            let tl = display::timeline(&m.tags, frame + 1);
            let list = tl.frames.get(frame).or(tl.frames.last()).cloned().unwrap_or_default();
            let (x0, x1, y0, y1) = m.frame();
            let stage = [(x0 * 20.0) as i32, (y0 * 20.0) as i32, (x1 * 20.0) as i32, (y1 * 20.0) as i32];
            (list, Some(stage))
        }
        Target::Frame(sprite, frame) => {
            let list = dict
                .chars
                .get(&sprite)
                .and_then(|c| c.tag)
                .map(|i| {
                    let tl = display::timeline(&display::sprite_body(&m.tags[i]), frame + 1);
                    tl.frames.get(frame).or(tl.frames.last()).cloned().unwrap_or_default()
                })
                .unwrap_or_default();
            let rect = dict.list_bounds(m, &list);
            (list, rect)
        }
        Target::Character(id, _) => {
            let e = Entry { depth: 1, character: id, matrix: Matrix::default(), cxform: Default::default(), name: Some("__preview".into()), ratio: None, clip_depth: None, class_name: None, placed_by: 0, matrix_by: 0, changed_by: 0, since: 0, visible: true };
            (vec![e], dict.bounds(m, id))
        }
    };
    if !matches!(target, Target::Character(..)) {
        list.retain(|e| !opts.exclude.contains(&e.depth));
    }
    let mut rect = opts.view.or(rect).unwrap_or([0, 0, 200 * 20, 200 * 20]);
    if rect[2] - rect[0] < 20 {
        rect[2] = rect[0] + 20;
    }
    if rect[3] - rect[1] < 20 {
        rect[3] = rect[1] + 20;
    }
    let roots: Vec<u16> = list.iter().map(|e| e.character).collect();
    let needed = if play { dict.closure(&roots) } else { first_frame_closure(m, dict, &roots) };
    let mut tags = definitions(m, &needed, play, matches!(target, Target::Root(_)));
    // The root frame: the list, moved so the bounds start at the stage origin.
    let shift = Matrix { tx: -rect[0], ty: -rect[1], ..Matrix::default() };
    for e in &list {
        let mut p = Place::new(e.depth, e.character, shift.concat(&e.matrix));
        if !e.cxform.is_identity() {
            p.cxform = Some(e.cxform);
        }
        p.name = e.name.clone();
        p.ratio = e.ratio;
        p.clip_depth = e.clip_depth;
        tags.push(Tag { code: 26, data: p.write(), long: false });
    }
    if !play {
        // A transparent pixel above everything: the Studio knows the layout
        // has been drawn once Ruffle draws anything at all. Ruffle clears
        // empty frames while it is still parsing a large movie (an 8 MB one
        // takes 0.7 s), and an empty layout draws nothing otherwise.
        if let Some(id) = (1..=u16::MAX).rev().find(|id| !dict.chars.contains_key(id)) {
            let pixel = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
            let edges = (0..4).map(|i| super::shape::Edge { from: pixel[i], ctrl: None, to: pixel[(i + 1) % 4] }).collect();
            let path = super::shape::NewPath { fill: Some(super::shape::NewFill::Solid(super::shape::Rgba { r: 0, g: 0, b: 0, a: 0 })), line: None, contours: vec![edges] };
            let at = tags.iter().position(|t| t.code == 26).unwrap_or(tags.len());
            tags.insert(at, super::shape::write_shape(id, &[path]));
            let top = list.iter().map(|e| e.depth.max(e.clip_depth.unwrap_or(0))).max().unwrap_or(0);
            tags.push(Tag { code: 26, data: Place::new(top.saturating_add(1), id, Matrix::default()).write(), long: false });
        }
    }
    tags.push(Tag { code: 1, data: Vec::new(), long: false });
    tags.push(Tag { code: 0, data: Vec::new(), long: false });
    // A layout only loads the libraries its characters come from; a playing
    // target keeps every import (scripts can attach any imported symbol).
    super::merge_imports(&mut tags, |id| play || needed.contains(&id));

    let mut header = write_rect([0, 0, rect[2] - rect[0], rect[3] - rect[1]]);
    let n = m.header.len();
    if play {
        header.extend_from_slice(&m.header[n - 4..n - 2]);
    } else {
        // Ruffle draws a movie's first picture about ten frames after loading
        // it (400 ms at the UI's 24 fps); a layout has one frame and no
        // scripts, so the fastest rate only makes it appear sooner.
        header.extend_from_slice(&[0, 255]);
    }
    header.extend_from_slice(&1u16.to_le_bytes());
    Preview { movie: Movie { signature: *b"FWS", version: m.version.max(8), header, tags }, rect }
}

/// A shared library cut to the symbols a layout imports from it (`names`):
/// what they draw on their first frame (`first_frame_closure`, sprites cut
/// like the layout's own), their exports and the library's imports among
/// them. Loading the whole library made Ruffle parse 1.4 MB of fonts for one
/// font, and fetch libraries of libraries, for every layer. None when the
/// library exports none of the names.
pub fn library_subset(lib: &Movie, names: &[String]) -> Option<Movie> {
    let dict = Dict::build(lib);
    let mut roots = Vec::new();
    for t in lib.tags.iter().filter(|t| t.code == 56) {
        let n = u16_at(&t.data, 0).unwrap_or(0) as usize;
        let mut pos = 2;
        for _ in 0..n {
            let Some(id) = u16_at(&t.data, pos) else { break };
            let start = (pos + 2).min(t.data.len());
            let end = t.data[start..].iter().position(|&b| b == 0).map_or(t.data.len(), |e| start + e);
            let name = String::from_utf8_lossy(&t.data[start..end]);
            if names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                roots.push(id);
            }
            pos = end + 1;
        }
    }
    if roots.is_empty() {
        return None;
    }
    let needed = first_frame_closure(lib, &dict, &roots);
    let mut tags = definitions(lib, &needed, false, false);
    tags.push(Tag { code: 1, data: Vec::new(), long: false });
    tags.push(Tag { code: 0, data: Vec::new(), long: false });
    super::merge_imports(&mut tags, |id| needed.contains(&id));
    let mut header = lib.header.clone();
    let n = header.len();
    header[n - 2..].copy_from_slice(&1u16.to_le_bytes());
    Some(Movie { signature: *b"FWS", version: lib.version.max(8), header, tags })
}

/// The tags defining `needed` (and what they attach, export and import), in
/// stream order; sprites cut to their first frame unless `play`.
fn definitions(m: &Movie, needed: &std::collections::BTreeSet<u16>, play: bool, background: bool) -> Vec<Tag> {
    let mut tags: Vec<Tag> = Vec::new();
    for t in &m.tags {
        match t.code {
            69 | 1000 => tags.push(t.clone()),
            9 if background => tags.push(t.clone()),
            57 | 71 => tags.push(t.clone()),
            56 => {
                let n = u16_at(&t.data, 0).unwrap_or(0) as usize;
                let mut pos = 2;
                let mut kept: Vec<(u16, Vec<u8>)> = Vec::new();
                for _ in 0..n {
                    let id = u16_at(&t.data, pos).unwrap_or(0);
                    let end = t.data[(pos + 2).min(t.data.len())..].iter().position(|&b| b == 0).map(|e| pos + 2 + e + 1).unwrap_or(t.data.len());
                    if needed.contains(&id) {
                        kept.push((id, t.data[pos + 2..end].to_vec()));
                    }
                    pos = end;
                }
                if !kept.is_empty() {
                    let mut d = (kept.len() as u16).to_le_bytes().to_vec();
                    for (id, name) in kept {
                        d.extend_from_slice(&id.to_le_bytes());
                        d.extend_from_slice(&name);
                    }
                    tags.push(Tag { code: 56, data: d, long: false });
                }
            }
            59 => {
                if play && u16_at(&t.data, 0).is_some_and(|id| needed.contains(&id)) {
                    tags.push(t.clone());
                }
            }
            13 | 17 | 23 | 62 | 73 | 74 | 78 | 88 => {
                if u16_at(&t.data, 0).is_some_and(|id| needed.contains(&id)) {
                    tags.push(t.clone());
                }
            }
            code => {
                if super::dict::kind_of(code).is_none() {
                    continue;
                }
                let id = match code {
                    1009 => u32_at(&t.data, 0).map(|v| (v & 0xFFFF) as u16),
                    _ => u16_at(&t.data, 0),
                };
                if !id.is_some_and(|id| needed.contains(&id)) {
                    continue;
                }
                if code == 39 && !play {
                    tags.push(first_frame(t));
                } else {
                    tags.push(t.clone());
                }
            }
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swf::shape::{write_shape, Edge, NewFill, NewPath, Rgba};

    fn square(id: u16) -> Tag {
        let p = [(0.0, 0.0), (200.0, 0.0), (200.0, 200.0), (0.0, 200.0)];
        let edges = (0..4).map(|i| Edge { from: p[i], ctrl: None, to: p[(i + 1) % 4] }).collect();
        write_shape(id, &[NewPath { fill: Some(NewFill::Solid(Rgba { r: 255, g: 0, b: 0, a: 255 })), line: None, contours: vec![edges] }])
    }

    #[test]
    fn library_subset_keeps_what_the_first_frame_draws() {
        // Sprite 3 shows shape 1 on its first frame and shape 2 on its second.
        let mut body = 3u16.to_le_bytes().to_vec();
        body.extend_from_slice(&2u16.to_le_bytes());
        let show = Tag { code: 1, data: Vec::new(), long: false };
        let inner = [
            Tag { code: 26, data: Place::new(1, 1, Matrix::default()).write(), long: false },
            show.clone(),
            Tag { code: 26, data: Place::new(2, 2, Matrix::default()).write(), long: false },
            show.clone(),
            Tag { code: 0, data: Vec::new(), long: false },
        ];
        write_tags(&inner, &mut body);
        let mut exports = 2u16.to_le_bytes().to_vec();
        exports.extend_from_slice(&3u16.to_le_bytes());
        exports.extend_from_slice(b"Clip\0");
        exports.extend_from_slice(&2u16.to_le_bytes());
        exports.extend_from_slice(b"Other\0");
        let lib = Movie {
            signature: *b"FWS",
            version: 8,
            header: { let mut h = write_rect([0, 0, 2000, 2000]); h.extend_from_slice(&[0, 24, 1, 0]); h },
            tags: vec![square(1), square(2), Tag { code: 39, data: body, long: true }, Tag { code: 56, data: exports, long: false }, show, Tag { code: 0, data: Vec::new(), long: false }],
        };
        let sub = library_subset(&lib, &["clip".to_string()]).expect("Clip is exported");
        let defined: Vec<u16> = sub.tags.iter().filter(|t| matches!(t.code, 32 | 39)).filter_map(|t| u16_at(&t.data, 0)).collect();
        assert_eq!(defined, vec![1, 3]);
        let export = sub.tags.iter().find(|t| t.code == 56).unwrap();
        assert_eq!(u16_at(&export.data, 0), Some(1));
        assert!(library_subset(&lib, &["missing".to_string()]).is_none());
        // The subset parses back as a movie.
        assert!(Movie::parse(&sub.write()).is_ok());
    }
}
