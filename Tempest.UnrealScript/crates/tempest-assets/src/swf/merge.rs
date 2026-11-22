//! Three-way merge of two edits of one movie (two mods changing the same
//! `SwfMovie`): the tags each side changed against the movie both started
//! from are combined, a sprite both changed is merged tag by tag.
//!
//! Tags are matched by what they define (code and character id), other tags
//! by their bytes: a definition edited in place stays matched and only its
//! content is compared. A tag both sides change differently takes the later
//! side's version and is listed ([`Merged::overridden`]); the edits do not
//! combine at all when they replace overlapping stretches of tags, define the
//! same new character id differently, or both change the movie's header.

use std::collections::HashMap;

use super::{parse_tags, write_tags, Movie, Tag};

/// Why two edits do not combine.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict(pub String);

/// Both edits in one movie.
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    pub bytes: Vec<u8>,
    /// Tags both sides changed, where the later side's version was kept.
    pub overridden: Vec<String>,
}

/// The movie with both edits, or why they do not combine. `ours` is the
/// movie as an earlier mod left it, `theirs` the one a later mod wants.
pub fn merge3(base: &[u8], ours: &[u8], theirs: &[u8]) -> Result<Merged, Conflict> {
    let parse = |b: &[u8], what: &str| Movie::parse(b).map_err(|e| Conflict(format!("the {what} movie could not be read: {e}")));
    let (b, o, t) = (parse(base, "original")?, parse(ours, "installed")?, parse(theirs, "new")?);
    let header = pick(&b.header, &o.header, &t.header).ok_or_else(|| Conflict("both change the movie's size, frame rate or frame count".into()))?.clone();
    let mut overridden = Vec::new();
    let tags = merge_tags(&b.tags, &o.tags, &t.tags, "", &mut overridden)?;
    // Character ids both sides define anew must be the same definition.
    let base_ids: HashMap<u16, &Tag> = b.tags.iter().filter_map(|t| Some((defined_id(t)?, t))).collect();
    let new_ids = |tags: &[Tag]| -> HashMap<u16, Vec<u8>> { tags.iter().filter_map(|t| defined_id(t).filter(|id| !base_ids.contains_key(id)).map(|id| (id, t.data.clone()))).collect() };
    let (ours_new, theirs_new) = (new_ids(&o.tags), new_ids(&t.tags));
    if let Some(id) = ours_new.iter().find(|(id, d)| theirs_new.get(id).is_some_and(|x| x != *d)).map(|(id, _)| id) {
        return Err(Conflict(format!("both add a different character {id}")));
    }
    let signature = if o.signature == b.signature { t.signature } else { o.signature };
    let version = o.version.max(t.version);
    Ok(Merged { bytes: Movie { signature, version, header, tags }.write(), overridden })
}

/// The side that changed a value, or either when both made the same change.
fn pick<'a, T: PartialEq>(base: &'a T, ours: &'a T, theirs: &'a T) -> Option<&'a T> {
    if ours == base || ours == theirs {
        Some(theirs)
    } else if theirs == base {
        Some(ours)
    } else {
        None
    }
}

/// The character a definition tag defines (its first two bytes; the low
/// half of `DefineExternalImage2`'s u32).
fn defined_id(t: &Tag) -> Option<u16> {
    const DEFINES: &[u16] = &[2, 6, 7, 10, 11, 14, 20, 21, 22, 32, 33, 34, 35, 36, 37, 39, 46, 48, 60, 75, 83, 84, 87, 90, 1001, 1009];
    DEFINES.contains(&t.code).then(|| super::u16_at_pub(&t.data, 0)).flatten()
}

/// The character a tag describes without defining it (font info and names,
/// alignment zones, text settings, scaling grids): one such tag per character.
fn described_id(t: &Tag) -> Option<u16> {
    const DESCRIBES: &[u16] = &[13, 62, 73, 74, 78, 88];
    DESCRIBES.contains(&t.code).then(|| super::u16_at_pub(&t.data, 0)).flatten()
}

/// What matches a tag across versions: what it defines, else its bytes.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Def(u16, u16),
    Bytes(u16, u64),
}

fn key(t: &Tag) -> Key {
    match defined_id(t).or_else(|| described_id(t)) {
        Some(id) => Key::Def(t.code, id),
        None => {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            t.data.hash(&mut h);
            Key::Bytes(t.code, h.finish())
        }
    }
}

/// A stretch of the base a side replaced: `base[start..end]` became `with`.
struct Hunk<'a> {
    start: usize,
    end: usize,
    with: &'a [Tag],
}

/// One side's changes against the base: the replaced stretches and, for
/// each base tag it kept, where that tag is on its side.
fn changes<'a>(base: &[Tag], side: &'a [Tag]) -> (Vec<Hunk<'a>>, Vec<Option<usize>>) {
    let (kb, ks): (Vec<Key>, Vec<Key>) = (base.iter().map(key).collect(), side.iter().map(key).collect());
    let pairs = matches(&kb, &ks);
    let mut kept = vec![None; base.len()];
    let mut hunks = Vec::new();
    let (mut i, mut j) = (0, 0);
    for &(bi, sj) in pairs.iter().chain(std::iter::once(&(base.len(), side.len()))) {
        if bi > i || sj > j {
            hunks.push(Hunk { start: i, end: bi, with: &side[j..sj] });
        }
        if bi < base.len() {
            kept[bi] = Some(sj);
        }
        (i, j) = (bi + 1, sj + 1);
    }
    (hunks, kept)
}

/// The matched (base, side) positions of a longest common subsequence of the
/// keys (Myers' algorithm, after the common ends: edits are few).
fn matches(a: &[Key], b: &[Key]) -> Vec<(usize, usize)> {
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (ma, mb) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut out: Vec<(usize, usize)> = (0..pre).map(|i| (i, i)).collect();
    out.extend(myers(ma, mb).into_iter().map(|(x, y)| (x + pre, y + pre)));
    out.extend((0..suf).map(|k| (a.len() - suf + k, b.len() - suf + k)));
    out
}

fn myers(a: &[Key], b: &[Key]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    let off = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    'search: for d in 0..=max as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && v[(off + k - 1) as usize] < v[(off + k + 1) as usize]) { v[(off + k + 1) as usize] } else { v[(off + k - 1) as usize] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(off + k) as usize] = x;
            if x >= n && y >= m {
                break 'search;
            }
            k += 2;
        }
    }
    // Walk the trace back from the end, collecting the diagonal moves.
    let mut out = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..trace.len() as isize).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && v[(off + k - 1) as usize] < v[(off + k + 1) as usize]) { k + 1 } else { k - 1 };
        let prev_x = if d == 0 { 0 } else { v[(off + prev_k) as usize] };
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            out.push((x as usize, y as usize));
        }
        if d > 0 {
            (x, y) = (prev_x, prev_y);
        }
    }
    out.reverse();
    out
}

/// Merges two edits of a tag list (a movie's, or a sprite's) of `base`.
fn merge_tags(base: &[Tag], ours: &[Tag], theirs: &[Tag], within: &str, overridden: &mut Vec<String>) -> Result<Vec<Tag>, Conflict> {
    let (oh, okept) = changes(base, ours);
    let (th, tkept) = changes(base, theirs);
    let mut out = Vec::with_capacity(ours.len().max(theirs.len()));
    let (mut oi, mut ti) = (0, 0);
    let mut i = 0;
    loop {
        // Stretches replaced at this point of the base (insertions first).
        let o = oh.get(oi).filter(|h| h.start == i);
        let t = th.get(ti).filter(|h| h.start == i);
        match (o, t) {
            (Some(o), Some(t)) => {
                let same = o.end == t.end && o.with.len() == t.with.len() && o.with.iter().zip(t.with).all(|(x, y)| x.code == y.code && x.data == y.data);
                let (o_adds, t_adds) = (o.start == o.end, t.start == t.end);
                if same {
                    out.extend_from_slice(o.with);
                } else if o_adds != t_adds {
                    // Tags added just before what the other side replaces.
                    out.extend_from_slice(if o_adds { o.with } else { t.with });
                    if o_adds {
                        oi += 1;
                    } else {
                        ti += 1;
                    }
                    continue;
                } else if o_adds {
                    // Both add tags here: the earlier mod's first.
                    out.extend_from_slice(o.with);
                    out.extend_from_slice(t.with);
                } else {
                    return Err(Conflict(format!("both change the tags around {}", describe(base.get(i), within))));
                }
                i = o.end.max(t.end);
                (oi, ti) = (oi + 1, ti + 1);
                continue;
            }
            (Some(h), None) | (None, Some(h)) => {
                let other = if o.is_some() { &th[ti..] } else { &oh[oi..] };
                if other.first().is_some_and(|x| x.start < h.end) {
                    return Err(Conflict(format!("both change the tags around {}", describe(base.get(i), within))));
                }
                out.extend_from_slice(h.with);
                i = h.end;
                if o.is_some() {
                    oi += 1;
                } else {
                    ti += 1;
                }
                continue;
            }
            (None, None) => {}
        }
        if i >= base.len() {
            break;
        }
        // A tag both kept: whichever side changed it.
        let (b, ot, tt) = (&base[i], &ours[okept[i].expect("kept by ours")], &theirs[tkept[i].expect("kept by theirs")]);
        if let Some(data) = pick(&b.data, &ot.data, &tt.data) {
            // The length form too follows the side that changed it: the same in either order.
            let long = if ot.long != b.long { ot.long } else { tt.long };
            out.push(Tag { code: b.code, data: data.clone(), long });
        } else if let Some(sprite) = (b.code == 39).then(|| merge_sprite(b, ot, tt, overridden)).and_then(Result::ok) {
            out.push(sprite);
        } else {
            overridden.push(format!("both change {}", describe(Some(b), within)));
            out.push(tt.clone());
        }
        i += 1;
    }
    Ok(out)
}

/// A sprite both sides changed: its own tags merged.
fn merge_sprite(b: &Tag, o: &Tag, t: &Tag, overridden: &mut Vec<String>) -> Result<Tag, Conflict> {
    let id = defined_id(b).unwrap_or(0);
    let parts = |t: &Tag| -> Option<(Vec<u8>, Vec<Tag>)> { Some((t.data.get(..4)?.to_vec(), parse_tags(t.data.get(4..)?).ok()?)) };
    let (Some((bh, bt)), Some((oh, ot)), Some((th, tt))) = (parts(b), parts(o), parts(t)) else {
        return Err(Conflict(format!("both change sprite {id}")));
    };
    let head = pick(&bh, &oh, &th).ok_or_else(|| Conflict(format!("both change the frame count of sprite {id}")))?;
    let mut inner = Vec::new();
    let tags = merge_tags(&bt, &ot, &tt, &format!(" in sprite {id}"), &mut inner)?;
    overridden.append(&mut inner);
    let mut data = head.clone();
    write_tags(&tags, &mut data);
    Ok(Tag { code: 39, data, long: o.long || t.long })
}

fn describe(t: Option<&Tag>, within: &str) -> String {
    match t {
        Some(t) => match defined_id(t) {
            Some(id) => format!("{} {id}{within}", super::tag_name(t.code)),
            None => format!("a {} tag{within}", super::tag_name(t.code)),
        },
        None => format!("the end{within}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(code: u16, id: Option<u16>, body: &[u8]) -> Tag {
        let mut data = id.map(|i| i.to_le_bytes().to_vec()).unwrap_or_default();
        data.extend_from_slice(body);
        Tag { code, data, long: false }
    }

    fn sprite(id: u16, inner: &[Tag]) -> Tag {
        let mut data = id.to_le_bytes().to_vec();
        data.extend_from_slice(&1u16.to_le_bytes());
        write_tags(inner, &mut data);
        Tag { code: 39, data, long: true }
    }

    fn movie(tags: Vec<Tag>) -> Vec<u8> {
        // A 1-bit-field rect (5 bits + 4 × 1), frame rate, frame count.
        Movie { signature: *b"GFX", version: 15, header: vec![0x08, 0x00, 0x00, 0x1e, 0x01, 0x00], tags }.write()
    }

    fn tags_of(bytes: &[u8]) -> Vec<(u16, Vec<u8>)> {
        Movie::parse(bytes).unwrap().tags.into_iter().map(|t| (t.code, t.data)).collect()
    }

    fn base_tags() -> Vec<Tag> {
        vec![
            tag(69, None, &[1, 0, 0, 0]),
            tag(32, Some(1), b"shape one"),
            tag(37, Some(2), b"text two"),
            sprite(3, &[tag(26, None, b"place a"), tag(1, None, b""), tag(0, None, b"")]),
            tag(32, Some(4), b"shape four"),
            tag(1, None, b""),
            tag(0, None, b""),
        ]
    }

    #[test]
    fn edits_of_different_tags_combine() {
        let base = base_tags();
        let mut ours = base.clone();
        ours[1] = tag(32, Some(1), b"shape one, recoloured");
        ours[2] = tag(37, Some(2), b"text two, restyled");
        let mut theirs = base.clone();
        // The later mod replaces sprite 3 and adds two characters before it.
        theirs.splice(3..4, [tag(83, Some(30000), b"scope"), sprite(30001, &[tag(0, None, b"")]), sprite(3, &[tag(26, None, b"place scope"), tag(1, None, b""), tag(0, None, b"")])]);
        let merged = tags_of(&merge3(&movie(base.clone()), &movie(ours.clone()), &movie(theirs.clone())).unwrap().bytes);
        let mut want = ours.clone();
        want.splice(3..4, theirs[3..6].iter().cloned());
        assert_eq!(merged, want.into_iter().map(|t| (t.code, t.data)).collect::<Vec<_>>());
    }

    #[test]
    fn a_sprite_both_change_merges_inside() {
        let base = base_tags();
        let mut ours = base.clone();
        ours[3] = sprite(3, &[tag(26, None, b"place a"), tag(43, None, b"label"), tag(1, None, b""), tag(0, None, b"")]);
        let mut theirs = base.clone();
        theirs[3] = sprite(3, &[tag(26, None, b"place b"), tag(1, None, b""), tag(0, None, b"")]);
        let merged = tags_of(&merge3(&movie(base), &movie(ours), &movie(theirs)).unwrap().bytes);
        assert_eq!(merged[3].1, sprite(3, &[tag(26, None, b"place b"), tag(43, None, b"label"), tag(1, None, b""), tag(0, None, b"")]).data);
    }

    #[test]
    fn the_same_tag_changed_twice_takes_the_later_one() {
        let base = base_tags();
        let mut ours = base.clone();
        ours[1] = tag(32, Some(1), b"shape one, ours");
        ours[4] = tag(32, Some(4), b"shape four, red");
        let mut theirs = base.clone();
        theirs[4] = tag(32, Some(4), b"shape four, blue");
        let m = merge3(&movie(base), &movie(ours), &movie(theirs)).unwrap();
        assert_eq!(m.overridden, vec!["both change DefineShape3 4".to_string()]);
        let tags = tags_of(&m.bytes);
        assert_eq!((tags[1].1.as_slice(), tags[4].1.as_slice()), (tag(32, Some(1), b"shape one, ours").data.as_slice(), tag(32, Some(4), b"shape four, blue").data.as_slice()));
    }

    #[test]
    fn the_same_new_id_conflicts() {
        let base = base_tags();
        let mut ours = base.clone();
        ours.insert(5, tag(32, Some(9), b"ours"));
        let mut theirs = base.clone();
        theirs.insert(1, tag(32, Some(9), b"theirs"));
        assert!(merge3(&movie(base), &movie(ours), &movie(theirs)).is_err());
    }

    #[test]
    fn either_order_gives_the_same_movie() {
        let base = base_tags();
        // One side rewrote every tag in the long length form and edited one.
        let mut ours: Vec<Tag> = base.iter().cloned().map(|t| Tag { long: true, ..t }).collect();
        ours[2] = Tag { long: true, ..tag(37, Some(2), b"text two, restyled") };
        let mut theirs = base.clone();
        theirs[4] = tag(32, Some(4), b"shape four, scoped");
        let (b, o, t) = (movie(base), movie(ours), movie(theirs));
        assert_eq!(merge3(&b, &o, &t).unwrap().bytes, merge3(&b, &t, &o).unwrap().bytes);
    }

    #[test]
    fn identical_changes_are_taken_once() {
        let base = base_tags();
        let mut ours = base.clone();
        ours[1] = tag(32, Some(1), b"same edit");
        let theirs = ours.clone();
        assert_eq!(tags_of(&merge3(&movie(base), &movie(ours.clone()), &movie(theirs)).unwrap().bytes), tags_of(&movie(ours)));
    }
}
