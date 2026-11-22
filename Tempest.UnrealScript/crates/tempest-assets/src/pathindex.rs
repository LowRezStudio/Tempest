//! The searchable object paths of a workspace package (`workspace.search`)
//! and which package holds an object path (`Owners`).
//!
//! A cooked folder holds about three million export paths (200 MB of text)
//! that mostly share their prefixes: the index keeps every distinct
//! dot-separated part once (20 MB) and one node per distinct prefix.

use std::collections::HashMap;

use memchr::memmem::Finder;

const NO_PARENT: u32 = u32::MAX;

#[derive(Clone, Default)]
pub struct SearchIndex {
    pub classes: Vec<String>,
    /// The distinct parts of the paths, end to end, and where each ends.
    parts: String,
    part_ends: Vec<u32>,
    /// (parent node, part) per distinct path prefix; a parent comes before
    /// its children.
    nodes: Vec<(u32, u32)>,
    /// Class id and path node per export.
    class: Vec<u16>,
    node: Vec<u32>,
    /// Some part has upper-case letters (script packages keep case, content
    /// packages are lower-case): searching must fold them.
    mixed_case: bool,
}

/// Buffers reused between the packages of one search.
#[derive(Default)]
pub struct Scratch {
    hits: Vec<bool>,
    part_hits: Vec<bool>,
    lower: Vec<u8>,
}

impl SearchIndex {
    pub fn len(&self) -> usize {
        self.node.len()
    }

    pub fn is_empty(&self) -> bool {
        self.node.is_empty()
    }

    pub fn class_of(&self, i: usize) -> u16 {
        self.class[i]
    }

    fn part(&self, p: u32) -> &str {
        let start = if p == 0 { 0 } else { self.part_ends[p as usize - 1] as usize };
        &self.parts[start..self.part_ends[p as usize] as usize]
    }

    /// The parts of an export's path, last first.
    pub fn parts_up(&self, i: usize) -> impl Iterator<Item = &str> {
        let mut cur = self.node[i];
        std::iter::from_fn(move || {
            let (parent, part) = *self.nodes.get(cur as usize)?;
            cur = parent;
            Some(self.part(part))
        })
    }

    /// The object name of an export (the last part of its path).
    pub fn leaf(&self, i: usize) -> &str {
        self.part(self.nodes[self.node[i] as usize].1)
    }

    pub fn path(&self, i: usize) -> String {
        let parts: Vec<&str> = self.parts_up(i).collect();
        let mut out = String::with_capacity(parts.iter().map(|p| p.len() + 1).sum());
        for (k, p) in parts.iter().rev().enumerate() {
            if k > 0 {
                out.push('.');
            }
            out.push_str(p);
        }
        out
    }

    /// Whether an export's path is `path`, ignoring ASCII case.
    pub fn path_is(&self, i: usize, path: &str) -> bool {
        let mut want = path.rsplit('.');
        self.parts_up(i).all(|p| want.next().is_some_and(|w| w.eq_ignore_ascii_case(p))) && want.next().is_none()
    }

    /// Calls `f` with every export whose path contains `query` (lower-case;
    /// every export when empty), in export order, until it returns true.
    /// Returns whether it did.
    pub fn find(&self, query: &str, finder: &Finder, s: &mut Scratch, mut f: impl FnMut(usize, u16) -> bool) -> bool {
        if !query.is_empty() {
            self.mark(query, finder, s);
        }
        (0..self.len()).any(|i| (query.is_empty() || s.hits[self.node[i] as usize]) && f(i, self.class[i]))
    }

    /// `s.hits`: the nodes whose path contains `query`. A path contains it
    /// when its parent's does or the query ends in its last part.
    fn mark(&self, query: &str, finder: &Finder, s: &mut Scratch) {
        let Scratch { hits, part_hits, lower } = s;
        hits.clear();
        hits.resize(self.nodes.len(), false);
        if !query.contains('.') {
            // Within one part: one search over all of them, a match running
            // into the next part is not one.
            part_hits.clear();
            part_hits.resize(self.part_ends.len(), false);
            let hay = if self.mixed_case {
                lower.clear();
                lower.extend(self.parts.bytes().map(|b| b.to_ascii_lowercase()));
                &lower[..]
            } else {
                self.parts.as_bytes()
            };
            let mut pos = 0;
            while let Some(m) = finder.find(&hay[pos..]) {
                let at = pos + m;
                let p = self.part_ends.partition_point(|&end| end as usize <= at);
                let end = self.part_ends[p] as usize;
                part_hits[p] |= at + query.len() <= end;
                pos = end;
            }
            for (n, &(parent, part)) in self.nodes.iter().enumerate() {
                hits[n] = part_hits[part as usize] || (parent != NO_PARENT && hits[parent as usize]);
            }
        } else {
            // Across parts: the first piece ends a part, the middle ones are
            // whole parts and the last one starts this node's part.
            let pieces: Vec<&[u8]> = query.split('.').map(str::as_bytes).collect();
            let last = pieces.len() - 1;
            for (n, &(parent, _)) in self.nodes.iter().enumerate() {
                if parent != NO_PARENT && hits[parent as usize] {
                    hits[n] = true;
                    continue;
                }
                let mut cur = n as u32;
                hits[n] = pieces.iter().enumerate().rev().all(|(k, piece)| {
                    let Some(&(parent, part)) = self.nodes.get(cur as usize) else { return false };
                    cur = parent;
                    let text = self.part(part).as_bytes();
                    let at = if k == last { 0 } else { text.len().wrapping_sub(piece.len()) };
                    (k == 0 || k == last || text.len() == piece.len()) && text.get(at..at + piece.len()).is_some_and(|t| t.eq_ignore_ascii_case(piece))
                });
            }
        }
    }

    /// `path_hash` of every export's path, in export order.
    pub fn path_hashes(&self) -> impl Iterator<Item = u64> + '_ {
        let mut of_node = Vec::with_capacity(self.nodes.len());
        for &(parent, part) in &self.nodes {
            let h = if parent == NO_PARENT { FNV_OFFSET } else { fnv_extend(of_node[parent as usize], b".") };
            of_node.push(fnv_extend(h, self.part(part).as_bytes()));
        }
        self.node.iter().map(move |&n| of_node[n as usize])
    }

    /// Appends the index to a cache file (see [`SearchIndex::read`]).
    pub fn write(&self, v: &mut Vec<u8>) {
        let put_u32 = |v: &mut Vec<u8>, n: usize| v.extend_from_slice(&(n as u32).to_le_bytes());
        put_u32(v, self.classes.len());
        for c in &self.classes {
            put_u32(v, c.len());
            v.extend_from_slice(c.as_bytes());
        }
        put_u32(v, self.parts.len());
        v.extend_from_slice(self.parts.as_bytes());
        put_u32(v, self.part_ends.len());
        for e in &self.part_ends {
            v.extend_from_slice(&e.to_le_bytes());
        }
        put_u32(v, self.nodes.len());
        for (parent, part) in &self.nodes {
            v.extend_from_slice(&parent.to_le_bytes());
            v.extend_from_slice(&part.to_le_bytes());
        }
        put_u32(v, self.node.len());
        for (c, n) in self.class.iter().zip(&self.node) {
            v.extend_from_slice(&c.to_le_bytes());
            v.extend_from_slice(&n.to_le_bytes());
        }
    }

    /// An index written by [`SearchIndex::write`] at `b[*pos..]`; `None` when
    /// it is truncated or inconsistent.
    pub fn read(b: &[u8], pos: &mut usize) -> Option<SearchIndex> {
        let mut take = |n: usize| -> Option<&[u8]> {
            let s = b.get(*pos..pos.checked_add(n)?)?;
            *pos += n;
            Some(s)
        };
        macro_rules! count {
            () => {
                u32::from_le_bytes(take(4)?.try_into().ok()?) as usize
            };
        }
        let nc = count!();
        let mut classes = Vec::with_capacity(nc.min(1 << 16));
        for _ in 0..nc {
            let n = count!();
            classes.push(std::str::from_utf8(take(n)?).ok()?.to_string());
        }
        let n = count!();
        let parts = std::str::from_utf8(take(n)?).ok()?.to_string();
        let n = count!();
        let part_ends: Vec<u32> = take(n.checked_mul(4)?)?.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let n = count!();
        let nodes: Vec<(u32, u32)> = take(n.checked_mul(8)?)?.chunks_exact(8).map(|c| (u32::from_le_bytes(c[..4].try_into().unwrap()), u32::from_le_bytes(c[4..].try_into().unwrap()))).collect();
        let n = count!();
        let (class, node): (Vec<u16>, Vec<u32>) = take(n.checked_mul(6)?)?.chunks_exact(6).map(|c| (u16::from_le_bytes(c[..2].try_into().unwrap()), u32::from_le_bytes(c[2..].try_into().unwrap()))).unzip();
        let mut start = 0;
        for &end in &part_ends {
            if (end as usize) < start || !parts.is_char_boundary(end as usize) {
                return None;
            }
            start = end as usize;
        }
        let nodes_ok = nodes.iter().enumerate().all(|(k, &(parent, part))| (parent == NO_PARENT || (parent as usize) < k) && (part as usize) < part_ends.len());
        let exports_ok = class.iter().zip(&node).all(|(&c, &n)| (c as usize) < classes.len() && (n as usize) < nodes.len());
        if !nodes_ok || !exports_ok {
            return None;
        }
        let mixed_case = parts.bytes().any(|b| b.is_ascii_uppercase());
        Some(SearchIndex { classes, parts, part_ends, nodes, class, node, mixed_case })
    }
}

/// Builds a [`SearchIndex`] one export path at a time.
#[derive(Default)]
pub struct Builder {
    idx: SearchIndex,
    class_ids: HashMap<String, u16>,
    part_ids: HashMap<Box<str>, u32>,
    node_ids: HashMap<(u32, u32), u32>,
}

impl Builder {
    pub fn push(&mut self, class: &str, path: &str) {
        let idx = &mut self.idx;
        let class = match self.class_ids.get(class) {
            Some(&id) => id,
            None => {
                let id = idx.classes.len() as u16;
                idx.classes.push(class.to_string());
                self.class_ids.insert(class.to_string(), id);
                id
            }
        };
        let mut node = NO_PARENT;
        for text in path.split('.') {
            let part = match self.part_ids.get(text) {
                Some(&p) => p,
                None => {
                    let p = idx.part_ends.len() as u32;
                    idx.mixed_case |= text.bytes().any(|b| b.is_ascii_uppercase());
                    idx.parts.push_str(text);
                    idx.part_ends.push(idx.parts.len() as u32);
                    self.part_ids.insert(text.into(), p);
                    p
                }
            };
            node = *self.node_ids.entry((node, part)).or_insert_with(|| {
                idx.nodes.push((node, part));
                idx.nodes.len() as u32 - 1
            });
        }
        idx.class.push(class);
        idx.node.push(node);
    }

    pub fn finish(self) -> SearchIndex {
        let mut idx = self.idx;
        idx.parts.shrink_to_fit();
        idx.part_ends.shrink_to_fit();
        idx.nodes.shrink_to_fit();
        idx.class.shrink_to_fit();
        idx.node.shrink_to_fit();
        idx
    }
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;

fn fnv_extend(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h = (h ^ b.to_ascii_lowercase() as u64).wrapping_mul(0x100000001b3);
    }
    h
}

/// Case-insensitive hash of an object path ([`Owners`] keys).
pub fn path_hash(path: &str) -> u64 {
    fnv_extend(FNV_OFFSET, path.as_bytes())
}

/// Object path hash → index of the first workspace package holding the path,
/// as two sorted arrays (12 bytes a path; a hash map takes twice that).
#[derive(Default)]
pub struct Owners {
    hashes: Vec<u64>,
    packages: Vec<u32>,
}

impl Owners {
    /// From (hash, package) pairs in package order.
    pub fn new(mut pairs: Vec<(u64, u32)>) -> Owners {
        // Stable: the first package of a path stays first.
        pairs.sort_by_key(|&(h, _)| h);
        pairs.dedup_by_key(|&mut (h, _)| h);
        let (hashes, packages) = pairs.into_iter().unzip();
        Owners { hashes, packages }
    }

    pub fn get(&self, hash: u64) -> Option<u32> {
        self.hashes.binary_search(&hash).ok().map(|i| self.packages[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(paths: &[(&str, &str)]) -> SearchIndex {
        let mut b = Builder::default();
        for (class, path) in paths {
            b.push(class, path);
        }
        b.finish()
    }

    /// Every export whose path contains the query, the way a plain scan of the
    /// full paths finds them.
    fn scan(paths: &[(&str, &str)], query: &str) -> Vec<usize> {
        paths.iter().enumerate().filter(|(_, (_, p))| p.to_ascii_lowercase().contains(query)).map(|(i, _)| i).collect()
    }

    const PATHS: &[(&str, &str)] = &[
        ("Package", "chaosgame"),
        ("Class", "chaosgame.TgPawn"),
        ("Function", "chaosgame.TgPawn.Tick"),
        ("Function", "chaosgame.TgPawn.Tick.DeltaTime"),
        ("Level", "theworld.persistentlevel"),
        ("StaticMeshActor", "theworld.persistentlevel.staticmeshactor_12"),
        ("StaticMeshActor", "theworld.persistentlevel.staticmeshactor_12"),
        ("Texture2D", "a..b"),
        ("Texture2D", ".lead"),
        ("Texture2D", ""),
        ("Material", "pawn.pawn.mat"),
    ];

    #[test]
    fn paths_come_back() {
        let idx = index(PATHS);
        for (i, (class, path)) in PATHS.iter().enumerate() {
            assert_eq!(idx.path(i), *path);
            assert_eq!(idx.classes[idx.class_of(i) as usize], *class);
            assert!(idx.path_is(i, &path.to_ascii_uppercase()));
            assert_eq!(idx.leaf(i), path.rsplit('.').next().unwrap());
        }
        assert!(!idx.path_is(2, "chaosgame.TgPawn"));
        assert!(!idx.path_is(1, "x.chaosgame.TgPawn"));
        assert_eq!(idx.path_hashes().collect::<Vec<_>>(), PATHS.iter().map(|(_, p)| path_hash(p)).collect::<Vec<_>>());
    }

    #[test]
    fn search_matches_a_scan_of_the_paths() {
        let idx = index(PATHS);
        let mut s = Scratch::default();
        for q in ["tick", "pawn", "pawn.", ".tick", "n.t", "game.tgpawn.tick.d", "persistentlevel.static", "l.s", ".", "..", "a.", "a..b", ".lead", "ead", "awn.pawn.m", "zz", "chaosgame.tgpawn", "_12", "c.t"] {
            let mut got = Vec::new();
            idx.find(q, &Finder::new(q.as_bytes()), &mut s, |i, _| {
                got.push(i);
                false
            });
            assert_eq!(got, scan(PATHS, q), "query {q:?}");
        }
        let mut all = 0;
        idx.find("", &Finder::new(b""), &mut s, |_, _| {
            all += 1;
            false
        });
        assert_eq!(all, PATHS.len());
    }

    #[test]
    fn cache_round_trip() {
        let idx = index(PATHS);
        let mut v = Vec::new();
        idx.write(&mut v);
        let mut pos = 0;
        let back = SearchIndex::read(&v, &mut pos).unwrap();
        assert_eq!(pos, v.len());
        assert!(back.mixed_case);
        assert_eq!((0..back.len()).map(|i| back.path(i)).collect::<Vec<_>>(), PATHS.iter().map(|(_, p)| p.to_string()).collect::<Vec<_>>());
        assert!(SearchIndex::read(&v[..v.len() - 1], &mut 0).is_none());
    }

    #[test]
    fn owners_keep_the_first_package() {
        let o = Owners::new(vec![(5, 0), (3, 0), (5, 1), (9, 2), (3, 2)]);
        assert_eq!((o.get(5), o.get(3), o.get(9), o.get(4)), (Some(0), Some(0), Some(2), None));
    }
}
