//! Building the mediator's face tree the way Havok does
//! (`hkaiStaticTreeNavMeshQueryMediator::setNavMesh`): every face's box
//! (`hkaiNavMeshUtils::calcFaceAabb`, no extra radius) inserted in an
//! `hkcdDynamicAabbTree`, `rebuild()` (= `rebuildBranchSAH(root, true, 32,
//! 16)`), then `hkcdStaticAabbTree::buildFromDynamicTree`. Ported from the
//! 8.2 server binary; float operations keep the SSE code's order so the
//! bins and costs come out the same.

use super::navmesh::NavMesh;
use super::tree::{pack, unpack, Aabb, StaticTree};

/// A 4-lane box as Havok keeps it (`hkAabb`: the w lanes take part in
/// every min/max, which is why the stored domains carry odd w values).
pub type Box4 = [[f32; 4]; 2];

const EMPTY: Box4 = [[f32::MAX; 4], [-f32::MAX; 4]];

fn union(a: &Box4, b: &Box4) -> Box4 {
    let mut o = *a;
    for k in 0..4 {
        o[0][k] = b[0][k].min(a[0][k]);
        o[1][k] = b[1][k].max(a[1][k]);
    }
    o
}

/// Half the surface area as the SSE code sums it: `t = d.yzx * d`, then
/// `t.z + (t.y + t.x)`.
fn area(b: &Box4) -> f32 {
    let d = [b[1][0] - b[0][0], b[1][1] - b[0][1], b[1][2] - b[0][2]];
    let t = [d[1] * d[0], d[2] * d[1], d[0] * d[2]];
    t[2] + (t[1] + t[0])
}

fn center(b: &Box4, axis: usize) -> f32 {
    (b[0][axis] + b[1][axis]) * 0.5
}

/// `hkaiNavMeshUtils::calcFaceAabb`: the face's vertices (each edge's `a`),
/// all four lanes.
pub fn face_aabb(m: &NavMesh, f: usize) -> Box4 {
    let face = &m.faces[f];
    let mut b = EMPTY;
    for k in 0..face.num_edges.max(0) as usize {
        let Some(e) = m.edges.get(face.start_edge as usize + k) else { continue };
        let Some(v) = m.vertices.get(e.a as usize) else { continue };
        for l in 0..4 {
            b[0][l] = v[l].min(b[0][l]);
            b[1][l] = v[l].max(b[1][l]);
        }
    }
    b
}

const NONE: usize = usize::MAX;

#[derive(Clone, Debug)]
pub struct DynNode {
    pub aabb: Box4,
    /// `[NONE, leaf data]` for a leaf.
    pub children: [usize; 2],
}

/// `hkAlgorithm::explicitStackQuickSort` with `SAHBin::Sorter` (Hoare
/// partition around the middle element, smaller side first on the stack).
fn quick_sort(a: &mut [usize], key: impl Fn(usize) -> f32) {
    if a.len() <= 1 {
        return;
    }
    let mut stack: Vec<(isize, isize)> = Vec::new();
    let (mut lo, mut hi) = (0isize, a.len() as isize - 1);
    loop {
        let pivot = key(a[(lo + (hi - lo) / 2) as usize]);
        let (mut i, mut j) = (lo, hi);
        loop {
            while key(a[i as usize]) < pivot {
                i += 1;
            }
            while pivot < key(a[j as usize]) {
                j -= 1;
            }
            if i > j {
                break;
            }
            if i != j {
                a.swap(i as usize, j as usize);
            }
            j -= 1;
            i += 1;
            if i > j {
                break;
            }
        }
        if lo >= j {
            lo = i;
            if hi <= i {
                match stack.pop() {
                    Some((l, h)) => {
                        lo = l;
                        hi = h;
                    }
                    None => break,
                }
            }
        } else if hi <= i {
            hi = j;
        } else if hi - i <= j - lo {
            stack.push((lo, j));
            lo = i;
        } else {
            stack.push((i, hi));
            hi = j;
        }
    }
}

pub const NUM_BINS: usize = 32;
pub const BRUTE_FORCE: usize = 16;
/// `data_32bd600`, the costs' starting value.
const START_COST: u32 = 0x7f7f_ffee;

/// `rebuildBranchSAH(root, true, 32, 16)` over the leaf boxes in `order`.
///
/// A branch of more than 16 leaves (and the root always) is split by
/// binned SAH: 32 bins per axis over the leaves' bounds, a leaf's bin
/// `trunc(clamp((centre − min) × (1/extent × 32) + 0.5, 0, 31))`; per bin
/// the cost `left count × area(left) + right count × area(right)` with
/// both sides including the bin; the cheapest non-empty bin wins (first axis
/// and bin on ties); bins below go to child 0, above to child 1, and the
/// chosen bin is sorted on its axis and halved (first half to child 0), or
/// given whole to a side left empty. Smaller branches are built bottom-up:
/// merge the pair whose union has the smallest area (first pair on ties),
/// the merged node takes the first's place, the last leaf the second's.
pub fn rebuild_sah(boxes: &[Box4], order: &[usize]) -> (Vec<DynNode>, usize) {
    let mut nodes: Vec<DynNode> = boxes.iter().enumerate().map(|(i, b)| DynNode { aabb: *b, children: [NONE, i] }).collect();
    let mut tasks: Vec<(Vec<usize>, usize, i32)> = vec![(order.to_vec(), NONE, -1)];
    let mut root = NONE;
    let attach = |nodes: &mut Vec<DynNode>, root: &mut usize, parent: usize, slot: i32, child: usize| {
        if slot < 0 {
            *root = child;
        } else {
            nodes[parent].children[slot as usize] = child;
        }
    };
    while let Some((mut list, parent, slot)) = tasks.pop() {
        if list.len() > BRUTE_FORCE || slot < 0 {
            let mut bounds = EMPTY;
            for &l in &list {
                bounds = union(&nodes[l].aabb, &bounds);
            }
            let mut scale = [0f32; 3];
            for k in 0..3 {
                let e = bounds[1][k] - bounds[0][k];
                scale[k] = if e == 0.0 { 0.0 } else { (1.0 / e) * NUM_BINS as f32 };
            }
            let node = nodes.len();
            nodes.push(DynNode { aabb: bounds, children: [NONE, NONE] });
            attach(&mut nodes, &mut root, parent, slot, node);
            let mut bins: Vec<Vec<(Vec<usize>, Box4)>> = vec![vec![(Vec::new(), EMPTY); NUM_BINS]; 3];
            let mut range = [(NUM_BINS, 0usize); 3];
            for &l in &list {
                let b = nodes[l].aabb;
                for a in 0..3 {
                    let c = (center(&b, a) - bounds[0][a]) * scale[a] + 0.5;
                    let bi = c.max(0.0).min((NUM_BINS - 1) as f32) as usize;
                    let bin = &mut bins[a][bi];
                    bin.0.push(l);
                    bin.1 = union(&b, &bin.1);
                    range[a].0 = range[a].0.min(bi);
                    range[a].1 = range[a].1.max(bi);
                }
            }
            let mut best = (f32::from_bits(START_COST), 0usize, NONE);
            for a in 0..3 {
                let (lo, hi) = range[a];
                let mut cost = [0f32; NUM_BINS];
                let (mut lb, mut rb) = (EMPTY, EMPTY);
                let (mut lc, mut rc) = (0u32, 0u32);
                for k in 0..=(hi - lo) {
                    let i = lo + k;
                    lb = union(&bins[a][i].1, &lb);
                    lc += bins[a][i].0.len() as u32;
                    cost[i] += lc as f32 * area(&lb);
                    let j = hi - k;
                    rb = union(&bins[a][j].1, &rb);
                    rc += bins[a][j].0.len() as u32;
                    cost[j] += rc as f32 * area(&rb);
                }
                for i in lo..=hi {
                    if !bins[a][i].0.is_empty() && !(best.0 <= cost[i]) {
                        best = (cost[i], a, i);
                    }
                }
            }
            let (_, axis, bi) = best;
            let (mut left, mut right) = (Vec::new(), Vec::new());
            for (i, bin) in bins[axis].iter().enumerate() {
                if i < bi {
                    left.extend_from_slice(&bin.0);
                } else if i > bi {
                    right.extend_from_slice(&bin.0);
                }
            }
            let mut mid = std::mem::take(&mut bins[axis][bi].0);
            if left.is_empty() == right.is_empty() {
                quick_sort(&mut mid, |l| center(&nodes[l].aabb, axis));
                let half = mid.len() >> 1;
                left.extend_from_slice(&mid[..half]);
                right.extend_from_slice(&mid[half..]);
            } else if right.is_empty() {
                right.extend_from_slice(&mid);
            } else {
                left.extend_from_slice(&mid);
            }
            // Child 0 takes the lower side. Every shipped root agrees (its
            // left leaf count), but deeper nodes sometimes have the sides the
            // other way round (TD_Abyss_P node 1: the same partition, upper
            // side first), which this port does not explain yet.
            tasks.push((left, node, 0));
            tasks.push((right, node, 1));
            continue;
        }
        while list.len() > 1 {
            let mut best = f32::from_bits(START_COST);
            let (mut bi, mut bj) = (NONE, NONE);
            let mut bbox = EMPTY;
            for i in 0..list.len() {
                for j in i + 1..list.len() {
                    let u = union(&nodes[list[i]].aabb, &nodes[list[j]].aabb);
                    let c = area(&u);
                    if c < best {
                        best = c;
                        bi = i;
                        bj = j;
                        bbox = u;
                    }
                }
            }
            let node = nodes.len();
            nodes.push(DynNode { aabb: bbox, children: [list[bi], list[bj]] });
            let last = list.len() - 1;
            if bj != last {
                list[bj] = list[last];
            }
            list.pop();
            list[bi] = node;
        }
        attach(&mut nodes, &mut root, parent, slot, list[0]);
    }
    (nodes, root)
}

/// `hkcdStaticAabbTree::buildFromDynamicTree`: depth first (child 0 first),
/// each box packed against its parent's *decoded* box, the root's box as
/// the domain; an internal node stores half the distance to its child 1.
pub fn to_static(nodes: &[DynNode], root: usize) -> StaticTree {
    let mut order = Vec::with_capacity(nodes.len());
    let mut parent_of = Vec::with_capacity(nodes.len());
    let mut stack = vec![(root, NONE)];
    while let Some((n, p)) = stack.pop() {
        let me = order.len();
        order.push(n);
        parent_of.push(p);
        if nodes[n].children[0] != NONE {
            stack.push((nodes[n].children[1], me));
            stack.push((nodes[n].children[0], me));
        }
    }
    let mut index_of = vec![0usize; nodes.len()];
    for (s, &n) in order.iter().enumerate() {
        index_of[n] = s;
    }
    let to3 = |b: &Box4| Aabb { min: [b[0][0], b[0][1], b[0][2]], max: [b[1][0], b[1][1], b[1][2]] };
    let rb = nodes[root].aabb;
    let domain = to3(&rb);
    let mut decoded = vec![domain; order.len()];
    let mut out = vec![[0u8; 6]; order.len()];
    for (s, &n) in order.iter().enumerate() {
        let pd = if parent_of[s] == NONE { domain } else { decoded[parent_of[s]] };
        let xyz = pack(&pd, &to3(&nodes[n].aabb));
        decoded[s] = unpack(&pd, xyz);
        let node = &nodes[n];
        let (hi, lo) = if node.children[0] != NONE {
            let off = index_of[node.children[1]] - s;
            (((off >> 17) as u8 & 0x7f) | 0x80, (off >> 1) as u16)
        } else {
            let d = node.children[1];
            ((d >> 16) as u8 & 0x7f, d as u16)
        };
        let lo = lo.to_le_bytes();
        out[s] = [xyz[0], xyz[1], xyz[2], hi, lo[0], lo[1]];
    }
    StaticTree { domain: rb, nodes: out }
}

pub fn build(boxes: &[Box4], order: &[usize]) -> StaticTree {
    match boxes.len() {
        0 => StaticTree::default(),
        // `rebuild` leaves trees of two leaves or fewer as inserted.
        1 => to_static(&[DynNode { aabb: boxes[0], children: [NONE, order[0]] }], 0),
        _ => {
            let (nodes, root) = rebuild_sah(boxes, order);
            to_static(&nodes, root)
        }
    }
}

/// The face tree of a mesh, leaves taken in face order.
pub fn build_for_mesh(m: &NavMesh) -> StaticTree {
    let boxes: Vec<Box4> = (0..m.faces.len()).map(|f| face_aabb(m, f)).collect();
    build(&boxes, &(0..boxes.len()).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::havok::navmesh::NavData;
    use crate::havok::tagfile::Tagfile;

    /// `HAVOK_TREE_FILE=<payload holding a TAG0>`: where the stored tree's
    /// second split falls in the bins of node 1's leaves.
    #[test]
    fn debug_split() {
        let Some(path) = std::env::var_os("HAVOK_TREE_FILE") else { return };
        let d = std::fs::read(path).unwrap();
        let at = Tagfile::find(&d).unwrap();
        let nd = NavData::read(&d[at..]).unwrap();
        let t = nd.tree.clone().unwrap();
        let leaves_under = |root: usize| -> Vec<u32> {
            let mut out = Vec::new();
            let mut st = vec![root];
            while let Some(i) = st.pop() {
                if t.is_leaf(i) {
                    out.push(t.data(i));
                } else {
                    st.push(i + 1);
                    st.push(i + 2 * t.data(i) as usize);
                }
            }
            out.sort();
            out
        };
        let boxes: Vec<Box4> = (0..nd.mesh.faces.len()).map(|f| face_aabb(&nd.mesh, f)).collect();
        for (parent, child) in [(0usize, 1usize), (1, 2)] {
            let n1 = leaves_under(parent);
            let n2 = leaves_under(child);
            eprintln!("node {parent}: {} leaves, child {child}: {}", n1.len(), n2.len());
            let mut bounds = EMPTY;
            for &l in &n1 {
                bounds = union(&boxes[l as usize], &bounds);
            }
            for a in 0..3 {
                let e = bounds[1][a] - bounds[0][a];
                let s = (1.0 / e) * 32.0;
                let mut cnt = [0usize; 32];
                let mut inside = [0usize; 32];
                for &l in &n1 {
                    let c = (center(&boxes[l as usize], a) - bounds[0][a]) * s + 0.5;
                    let bi = c.max(0.0).min(31.0) as usize;
                    cnt[bi] += 1;
                    if n2.binary_search(&l).is_ok() {
                        inside[bi] += 1;
                    }
                }
                eprintln!("  axis {a}: {cnt:?}\n          {inside:?}");
            }
        }
    }
}
