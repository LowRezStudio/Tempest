//! Jump links of a generated nav mesh: the game's `hkaiUserEdgeUtils` user
//! edge pairs and their traversal annotations (`docs/navmesh.md`). A pair
//! joins a segment of face A (the take-off: `x`/`y`/`z` points 0 and 1) to a
//! segment of face B (the landing: points 2 and 3). The mesh holds it as a
//! user edge on A (`USER | ORIGINAL`, cost `costAtoB`, edge data `userDataA`)
//! and one on B pointing back (`BLOCKED` too when the link is one way), each
//! with two vertices of its own after the faces' vertices; user edges come
//! after every face's own edges, grouped by face. Annotation `k` describes
//! pair `k` (`userDataA` = `0x7F900000 + k`), its `tEquation` mapping the
//! take-off segment to 0..1.
//!
//! Generation keeps the links the replaced mesh had, each end moved onto the
//! new face under it, and finds new drop-downs laid out like the shipped
//! ones: from a ledge, `DROP_FORWARD` out, onto a face lower by more than a
//! step, merged along the ledge while they land on the same face.

use std::collections::HashMap;

use super::navmesh::{edge_flags, Annotation, Edge, Graph, GraphEdge, Library, NavMesh, UserEdgePair, INVALID};
use super::tagfile::f32_to_half;
use crate::lightbake::bvh::Bvh;

type V3 = [f32; 3];

/// `hkaiTraversalType` of a drop-down.
pub const DROP_DOWN: u8 = 4;
/// What a pair's `userDataA` holds: this plus the annotation's index.
pub const USER_DATA_BASE: u32 = 0x7F90_0000;
/// How far out from the ledge a drop-down lands: 11.85 for nine in ten of
/// the 5467 shipped ones (median and quartiles alike).
pub const DROP_FORWARD: f32 = 11.85;
/// The deepest drop (the shipped ones go to 50.2).
pub const MAX_DROP: f32 = 52.0;
/// Ledges are tried every `STEP` along their edges.
const STEP: f32 = 0.5;
/// Narrower take-offs are left out; wider ones are split.
const MIN_WIDTH: f32 = 0.5;
const MAX_WIDTH: f32 = 50.0;

/// A link: the pair and its annotation.
#[derive(Clone, Debug)]
pub struct Link {
    pub pair: UserEdgePair,
    pub annotation: Annotation,
}

/// What became of the links: kept (moved onto the new faces), not needed
/// (both ends on one face: the new mesh walks across, as Recast's often does
/// over the small gaps the shipped jumps cross), lost (an end with no face
/// under it), found (new drop-downs).
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub kept: usize,
    pub not_needed: usize,
    pub lost: usize,
    pub found: usize,
}

fn xy_dist2(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// The faces of a mesh by where they are, seen from above.
struct Locator {
    polys: Vec<Vec<V3>>,
    boxes: Vec<[f32; 4]>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

const CELL: f32 = 16.0;

fn cell(x: f32, y: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (y / CELL).floor() as i32)
}

impl Locator {
    fn new(m: &NavMesh) -> Locator {
        let mut polys = Vec::with_capacity(m.faces.len());
        let mut boxes = Vec::with_capacity(m.faces.len());
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (fi, f) in m.faces.iter().enumerate() {
            let p: Vec<V3> = (0..f.num_edges.max(0) as usize).map(|k| m.edges[f.start_edge as usize + k].a).map(|v| m.vertices[v as usize]).map(|v| [v[0], v[1], v[2]]).collect();
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for v in &p {
                b = [b[0].min(v[0]), b[1].min(v[1]), b[2].max(v[0]), b[3].max(v[1])];
            }
            let (lo, hi) = (cell(b[0], b[1]), cell(b[2], b[3]));
            for cx in lo.0..=hi.0 {
                for cy in lo.1..=hi.1 {
                    grid.entry((cx, cy)).or_default().push(fi as u32);
                }
            }
            polys.push(p);
            boxes.push(b);
        }
        Locator { polys, boxes, grid }
    }

    /// Faces whose box comes within `r` of (x, y).
    fn near(&self, x: f32, y: f32, r: f32) -> Vec<usize> {
        let (lo, hi) = (cell(x - r, y - r), cell(x + r, y + r));
        let mut out = Vec::new();
        for cx in lo.0..=hi.0 {
            for cy in lo.1..=hi.1 {
                for &f in self.grid.get(&(cx, cy)).into_iter().flatten() {
                    let b = self.boxes[f as usize];
                    if x >= b[0] - r && x <= b[2] + r && y >= b[1] - r && y <= b[3] + r && !out.contains(&(f as usize)) {
                        out.push(f as usize);
                    }
                }
            }
        }
        out
    }

    /// The point of face `f` closest to `p` seen from above, at the face's height there.
    fn closest(&self, f: usize, p: V3) -> V3 {
        let poly = &self.polys[f];
        // Inside: the height of the fan triangle holding it.
        for k in 1..poly.len().saturating_sub(1) {
            if let Some(z) = height_in(poly[0], poly[k], poly[k + 1], p) {
                return [p[0], p[1], z];
            }
        }
        let mut best = (f32::MAX, poly[0]);
        for k in 0..poly.len() {
            let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let len2 = d[0] * d[0] + d[1] * d[1];
            let t = if len2 > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let q = [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + (b[2] - a[2]) * t];
            let d2 = xy_dist2([p[0], p[1]], [q[0], q[1]]);
            if d2 < best.0 {
                best = (d2, q);
            }
        }
        best.1
    }

    /// The face nearest `p` within `across` seen from above and `up_down`
    /// of its height, and the point of it closest to `p`.
    fn find(&self, p: V3, across: f32, up_down: f32) -> Option<(usize, V3)> {
        let mut best: Option<(f32, usize, V3)> = None;
        for f in self.near(p[0], p[1], across) {
            let q = self.closest(f, p);
            let (h, v) = (xy_dist2([p[0], p[1]], [q[0], q[1]]).sqrt(), (q[2] - p[2]).abs());
            if h <= across && v <= up_down && best.is_none_or(|b| h + v < b.0) {
                best = Some((h + v, f, q));
            }
        }
        best.map(|(_, f, q)| (f, q))
    }

    /// The highest face under (x, y) between `below.0` and `below.1` lower than `z`.
    fn landing(&self, x: f32, y: f32, z: f32, below: (f32, f32)) -> Option<(usize, V3)> {
        let mut best: Option<(usize, V3)> = None;
        for f in self.near(x, y, 0.0) {
            let poly = &self.polys[f];
            let Some(h) = (1..poly.len().saturating_sub(1)).find_map(|k| height_in(poly[0], poly[k], poly[k + 1], [x, y, z])) else { continue };
            let drop = z - h;
            if drop >= below.0 && drop <= below.1 && best.is_none_or(|b| h > b.1[2]) {
                best = Some((f, [x, y, h]));
            }
        }
        best
    }
}

/// The height of triangle (a, b, c) above (p.x, p.y), when it lies over it.
fn height_in(a: V3, b: V3, c: V3, p: V3) -> Option<f32> {
    let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if det.abs() < 1e-9 {
        return None;
    }
    let l1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / det;
    let l2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / det;
    let l3 = 1.0 - l1 - l2;
    const E: f32 = -1e-4;
    (l1 >= E && l2 >= E && l3 >= E).then(|| l1 * a[2] + l2 * b[2] + l3 * c[2])
}

/// `tEquation`: the take-off segment `a0`→`a1` mapped to 0..1 (in the plane).
pub fn t_equation(a0: V3, a1: V3) -> Option<[f32; 4]> {
    let d = [a1[0] - a0[0], a1[1] - a0[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    (len2 > 1e-8).then(|| {
        let (x, y) = (d[0] / len2, d[1] / len2);
        [x, y, 0.0, -(x * a0[0] + y * a0[1])]
    })
}

fn points(p: &UserEdgePair) -> [V3; 4] {
    [0, 1, 2, 3].map(|k| [p.x[k], p.y[k], p.z[k]])
}

fn set_points(p: &mut UserEdgePair, pts: [V3; 4]) {
    for (k, v) in pts.iter().enumerate() {
        p.x[k] = v[0];
        p.y[k] = v[1];
        p.z[k] = v[2];
    }
}

fn mid(a: V3, b: V3) -> V3 {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5]
}

/// The links of `old` moved onto `m`: each segment onto the face under its
/// middle (within `across` seen from above and `up_down` of its height),
/// its ends onto that face. Links with an end on no face (lost) or both ends
/// on one face (not needed) are left out; see [`Report`].
pub fn reattach(old: &Library, m: &NavMesh, across: f32, up_down: f32, report: &mut Report) -> Vec<Link> {
    let loc = Locator::new(m);
    let mut out = Vec::new();
    for (k, p) in old.user_edge_pairs.iter().enumerate() {
        let pts = points(p);
        let (Some((fa, _)), Some((fb, _))) = (loc.find(mid(pts[0], pts[1]), across, up_down), loc.find(mid(pts[2], pts[3]), across, up_down)) else {
            report.lost += 1;
            continue;
        };
        if fa == fb {
            report.not_needed += 1;
            continue;
        }
        let moved = [loc.closest(fa, pts[0]), loc.closest(fa, pts[1]), loc.closest(fb, pts[2]), loc.closest(fb, pts[3])];
        let mut pair = p.clone();
        set_points(&mut pair, moved);
        pair.face_a = fa as i32;
        pair.face_b = fb as i32;
        let mut annotation = old.annotations.get(k).cloned().unwrap_or_else(|| drop_annotation([0.0; 4]));
        if let Some(t) = t_equation(moved[0], moved[1]) {
            annotation.t_equation = t;
        }
        out.push(Link { pair, annotation });
    }
    report.kept = out.len();
    out
}

fn drop_annotation(t_equation: [f32; 4]) -> Annotation {
    Annotation { t_equation, user_data: 0, first_partition: INVALID, first_vector: INVALID, num_partitions: 0, num_vectors: 0, traversal_type: DROP_DOWN }
}

/// Whether a pawn can drop from `top` (on the ledge) to `bottom`: rays at
/// knee and chest height out over the edge, then down, hit nothing.
fn clear(blockers: &Bvh, top: V3, bottom: V3, agent_height: f32) -> bool {
    for h in [1.0, agent_height * 0.5] {
        let o = [top[0], top[1], top[2] + h];
        let d = [bottom[0] - top[0], bottom[1] - top[1], 0.0];
        let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if len > 0.0 && blockers.occluded(o, [d[0] / len, d[1] / len, 0.0], len) {
            return false;
        }
    }
    let o = [bottom[0], bottom[1], top[2] + 1.0];
    let fall = o[2] - (bottom[2] + 0.5);
    !(fall > 0.0 && blockers.occluded(o, [0.0, 0.0, -1.0], fall))
}

/// New drop-downs from every ledge (an edge with no face beyond it) of `m`:
/// tried every `STEP` along it, `DROP_FORWARD` out, onto the highest face
/// lower by `min_drop`..`MAX_DROP` there, with nothing in the way
/// (`blockers`: the level's collision); consecutive tries landing on the
/// same face make one link. Ledge stretches within `near` of a kept link's
/// take-off are skipped.
pub fn detect(m: &NavMesh, keep: &[Link], blockers: Option<&Bvh>, min_drop: f32, agent_height: f32, near: f32) -> Vec<Link> {
    let loc = Locator::new(m);
    let taken: Vec<(usize, V3, V3)> = keep.iter().map(|l| (l.pair.face_a as usize, points(&l.pair)[0], points(&l.pair)[1])).collect();
    let close_to_kept = |f: usize, p: V3| {
        taken.iter().any(|(tf, a, b)| {
            if *tf != f {
                return false;
            }
            let d = [b[0] - a[0], b[1] - a[1]];
            let len2 = d[0] * d[0] + d[1] * d[1];
            let t = if len2 > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
            xy_dist2([p[0], p[1]], [a[0] + d[0] * t, a[1] + d[1] * t]) < near * near
        })
    };
    let mut out = Vec::new();
    for (fa, f) in m.faces.iter().enumerate() {
        for e in f.start_edge as usize..(f.start_edge + f.num_edges as i32) as usize {
            let edge = &m.edges[e];
            if edge.opposite_face != INVALID {
                continue;
            }
            let (a, b) = (m.vertices[edge.a as usize], m.vertices[edge.b as usize]);
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
            if len < MIN_WIDTH {
                continue;
            }
            // Faces wind counter-clockwise seen from above: outward is to the right.
            let n = [d[1] / len, -d[0] / len];
            let at = |t: f32| [a[0] + d[0] * t / len, a[1] + d[1] * t / len, a[2] + d[2] * t / len];
            // (t, landing face) of each try that lands.
            let mut hits: Vec<(f32, usize)> = Vec::new();
            let mut t = STEP * 0.5;
            while t < len {
                let p = at(t);
                if !close_to_kept(fa, p) {
                    let (x, y) = (p[0] + n[0] * DROP_FORWARD, p[1] + n[1] * DROP_FORWARD);
                    if let Some((fb, q)) = loc.landing(x, y, p[2], (min_drop, MAX_DROP)) {
                        if fb != fa && blockers.is_none_or(|bv| clear(bv, p, q, agent_height)) {
                            hits.push((t, fb));
                        }
                    }
                }
                t += STEP;
            }
            // Runs of consecutive tries on one landing face.
            let mut k = 0;
            while k < hits.len() {
                let mut j = k;
                while j + 1 < hits.len() && hits[j + 1].1 == hits[k].1 && hits[j + 1].0 - hits[j].0 < STEP * 1.5 && hits[j + 1].0 - hits[k].0 < MAX_WIDTH {
                    j += 1;
                }
                let (t0, t1) = ((hits[k].0 - STEP * 0.5).max(0.0), (hits[j].0 + STEP * 0.5).min(len));
                if t1 - t0 >= MIN_WIDTH {
                    let fb = hits[k].1;
                    let (a0, a1) = (at(t0), at(t1));
                    let out_of = |p: V3| loc.closest(fb, [p[0] + n[0] * DROP_FORWARD, p[1] + n[1] * DROP_FORWARD, p[2]]);
                    let pts = [a0, a1, out_of(a0), out_of(a1)];
                    let mut pair = UserEdgePair { face_a: fa as i32, face_b: fb as i32, cost_a_to_b: 1.0, cost_b_to_a: 0.0, direction: 1, ..Default::default() };
                    set_points(&mut pair, pts);
                    out.push(Link { pair, annotation: drop_annotation(t_equation(a0, a1).unwrap_or([0.0; 4])) });
                }
                k = j + 1;
            }
        }
    }
    out
}

/// Put `links` into `m` as user edges (after every face's own edges and
/// vertices, see the module comment) and number them: `userDataA` of link
/// `k` becomes `USER_DATA_BASE + k`. Any user edges `m` had are replaced.
pub fn add_user_edges(m: &mut NavMesh, links: &mut [Link]) {
    // A generated mesh has none; an edited one keeps its own edges first.
    let own: usize = m.faces.iter().map(|f| f.num_edges.max(0) as usize).sum();
    m.edges.truncate(own);
    m.edge_data.truncate(own);
    for f in &mut m.faces {
        f.start_user_edge = -1;
        f.num_user_edges = 0;
    }
    let base = m.vertices.len();
    for (k, l) in links.iter_mut().enumerate() {
        l.pair.user_data_a = (USER_DATA_BASE + k as u32) as i32;
        for v in points(&l.pair) {
            m.vertices.push([v[0], v[1], v[2], 0.0]);
        }
    }
    // (face, link, side) in storage order: by face, then link, A before B.
    let mut order: Vec<(usize, usize, usize)> = Vec::new();
    for (k, l) in links.iter().enumerate() {
        order.push((l.pair.face_a as usize, k, 0));
        order.push((l.pair.face_b as usize, k, 1));
    }
    order.sort_unstable();
    let mut at: HashMap<(usize, usize), usize> = HashMap::new();
    for &(f, k, side) in &order {
        let e = m.edges.len();
        let face = &mut m.faces[f];
        if face.num_user_edges == 0 {
            face.start_user_edge = e as i32;
        }
        face.num_user_edges += 1;
        at.insert((k, side), e);
        let p = &links[k].pair;
        let v = (base + 4 * k + 2 * side) as i32;
        let (opposite_face, cost, data) = if side == 0 { (p.face_b, p.cost_a_to_b, p.user_data_a) } else { (p.face_a, p.cost_b_to_a, p.user_data_b) };
        let mut flags = edge_flags::USER | edge_flags::ORIGINAL;
        if cost <= 0.0 {
            flags |= edge_flags::BLOCKED;
        }
        m.edges.push(Edge { a: v, b: v + 1, opposite_edge: INVALID, opposite_face: opposite_face as u32, flags, user_edge_cost: if cost > 0.0 { f32_to_half(cost) } else { 0 } });
        m.edge_data.push(data);
    }
    for (k, _) in links.iter().enumerate() {
        let (ea, eb) = (at[&(k, 0)], at[&(k, 1)]);
        m.edges[ea].opposite_edge = eb as u32;
        m.edges[eb].opposite_edge = ea as u32;
    }
}

/// Cluster graph edges for the links: one from the take-off face's cluster to
/// the landing face's (and back for a two-way link), costing the distance
/// between the nodes, when the graph has none yet.
pub fn link_clusters(g: &mut Graph, m: &NavMesh, links: &[Link]) {
    let mut targets: Vec<Vec<GraphEdge>> = g.nodes.iter().map(|&(s, n)| g.edges[s as usize..(s + n) as usize].to_vec()).collect();
    let mut add = |from: usize, to: usize| {
        if from == to || from >= targets.len() || to >= targets.len() || targets[from].iter().any(|e| e.target as usize == to) {
            return;
        }
        let (p, q) = (g.positions[from], g.positions[to]);
        let cost = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
        targets[from].push(GraphEdge { cost: f32_to_half(cost), flags: 0, target: to as u32 });
    };
    for l in links {
        let (ca, cb) = (m.faces[l.pair.face_a as usize].cluster as usize, m.faces[l.pair.face_b as usize].cluster as usize);
        add(ca, cb);
        if l.pair.cost_b_to_a > 0.0 {
            add(cb, ca);
        }
    }
    g.nodes.clear();
    g.edges.clear();
    for t in targets.iter_mut() {
        t.sort_by_key(|e| e.target);
        g.nodes.push((g.edges.len() as i32, t.len() as i32));
        g.edges.extend(t.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::havok::generate::{nav_mesh, Polygons};
    use crate::havok::navmesh::validate;

    /// A 10×10 ledge at height 20 and, its +X edge at x = 10, a floor below
    /// from x = 15 to 40 at height 0.
    fn ledge_and_floor() -> NavMesh {
        let p = Polygons {
            vertices: vec![[0.0, 0.0, 20.0], [10.0, 0.0, 20.0], [10.0, 10.0, 20.0], [0.0, 10.0, 20.0], [15.0, -5.0, 0.0], [40.0, -5.0, 0.0], [40.0, 15.0, 0.0], [15.0, 15.0, 0.0]],
            faces: vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7]],
            ..Default::default()
        };
        nav_mesh(&p, 2.0625)
    }

    #[test]
    fn finds_a_drop_down_and_stores_it_valid() {
        let mut m = ledge_and_floor();
        let mut links = detect(&m, &[], None, 2.1875, 11.0, 4.0);
        assert_eq!(links.len(), 1, "one drop-down off the ledge's +X edge");
        let l = &links[0];
        assert_eq!((l.pair.face_a, l.pair.face_b), (0, 1));
        // The take-off runs along the edge, the landing 11.85 out at the floor's height.
        assert!((l.pair.x[0] - 10.0).abs() < 1e-4 && (l.pair.x[2] - (10.0 + DROP_FORWARD)).abs() < 1e-3);
        assert!(l.pair.z[2].abs() < 1e-4 && (l.pair.z[0] - 20.0).abs() < 1e-4);
        add_user_edges(&mut m, &mut links);
        assert!(validate(&m).is_empty(), "{:?}", validate(&m));
        assert_eq!(m.faces[0].num_user_edges, 1);
        let e = &m.edges[m.faces[0].start_user_edge as usize];
        assert_eq!(e.flags, edge_flags::USER | edge_flags::ORIGINAL);
        assert_eq!(e.user_edge_cost, 0x3f80);
        let back = &m.edges[e.opposite_edge as usize];
        assert_eq!(back.flags, edge_flags::USER | edge_flags::ORIGINAL | edge_flags::BLOCKED);
        assert_eq!(links[0].pair.user_data_a as u32, USER_DATA_BASE);
        // tEquation maps the take-off segment to 0..1.
        let t = links[0].annotation.t_equation;
        let at = |k: usize| t[0] * links[0].pair.x[k] + t[1] * links[0].pair.y[k] + t[3];
        assert!(at(0).abs() < 1e-4 && (at(1) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn kept_links_move_onto_the_new_faces() {
        let mut old = ledge_and_floor();
        let mut links = detect(&old, &[], None, 2.1875, 11.0, 4.0);
        add_user_edges(&mut old, &mut links);
        let library = Library { user_edge_pairs: links.iter().map(|l| l.pair.clone()).collect(), annotations: links.iter().map(|l| l.annotation.clone()).collect(), ..Default::default() };
        let mut report = Report::default();
        let kept = reattach(&library, &ledge_and_floor(), 4.125, 3.0, &mut report);
        assert_eq!((report.kept, report.not_needed, report.lost), (1, 0, 0));
        assert_eq!((kept[0].pair.face_a, kept[0].pair.face_b), (0, 1));
    }
}
