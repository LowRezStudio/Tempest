//! Preview movies: one character, or one frame of a timeline, alone on a
//! stage cut to its bounds, for the Studio's Ruffle player.
//!
//! The preview keeps the definitions the target needs (its dependency
//! closure), every import (Ruffle fetches the shared libraries) and places
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
    let needed = dict.closure(&list.iter().map(|e| e.character).collect::<Vec<_>>());

    let mut tags: Vec<Tag> = Vec::new();
    for t in &m.tags {
        match t.code {
            69 | 1000 => tags.push(t.clone()),
            9 if matches!(target, Target::Root(_)) => tags.push(t.clone()),
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
    tags.push(Tag { code: 1, data: Vec::new(), long: false });
    tags.push(Tag { code: 0, data: Vec::new(), long: false });

    let mut header = write_rect([0, 0, rect[2] - rect[0], rect[3] - rect[1]]);
    let n = m.header.len();
    header.extend_from_slice(&m.header[n - 4..n - 2]);
    header.extend_from_slice(&1u16.to_le_bytes());
    Preview { movie: Movie { signature: *b"FWS", version: m.version.max(8), header, tags }, rect }
}
