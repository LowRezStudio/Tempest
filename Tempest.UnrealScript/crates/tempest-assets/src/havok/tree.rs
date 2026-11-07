//! `hkcdStaticTree::Tree<DynamicStorage<Codec3Axis6>>`, the compressed AABB
//! tree of `hkcdStaticAabbTree` (the nav mesh query mediator's face tree).
//!
//! Nodes are 6 bytes in depth-first order: `xyz[3]` (per axis, high nibble
//! = how far the min moves in from the parent's min, low nibble = how far
//! the max moves in from the parent's max, both squared and scaled by
//! `(parentMax − parentMin) / 226`: `hkcdStaticTree::Codec3Axis::unpack`),
//! `hiData`, `loData` (u16). `hiData & 0x80` marks an internal node whose
//! left child follows it and whose right child is `2 × ((hiData & 0x7f) << 16
//! | loData)` nodes further (the left subtree's leaf count, twice); a leaf
//! holds `hiData << 16 | loData`, the face index. The root's box is the
//! tree's `domain`; its own codec bytes are unused (the queries in
//! `hkcdTreeQueries::unary` start from the domain).

/// 1/226 as the game's constant (`data_32bd5a0` = 0x3b90fdbc).
const INV226: f32 = f32::from_bits(0x3b90_fdbc);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StaticTree {
    pub domain: [[f32; 4]; 2],
    pub nodes: Vec<[u8; 6]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// `hkcdStaticTree::Codec3Axis::unpack`.
pub fn unpack(parent: &Aabb, xyz: [u8; 3]) -> Aabb {
    let mut out = *parent;
    for a in 0..3 {
        let scale = (parent.max[a] - parent.min[a]) * INV226;
        let hi = (xyz[a] >> 4) as f32;
        let lo = (xyz[a] & 15) as f32;
        out.min[a] = hi * hi * scale + parent.min[a];
        out.max[a] = parent.max[a] - lo * lo * scale;
    }
    out
}

/// `hkcdStaticTree::Codec3Axis::pack`: per axis, the largest steps that
/// still contain `child`.
pub fn pack(parent: &Aabb, child: &Aabb) -> [u8; 3] {
    let mut xyz = [0u8; 3];
    for a in 0..3 {
        loop {
            if xyz[a] >> 4 == 0xf {
                break;
            }
            let mut t = xyz;
            t[a] += 0x10;
            if unpack(parent, t).min[a] > child.min[a] {
                break;
            }
            xyz = t;
        }
        loop {
            if xyz[a] & 0xf == 0xf {
                break;
            }
            let mut t = xyz;
            t[a] += 1;
            if !(child.max[a] <= unpack(parent, t).max[a]) {
                break;
            }
            xyz = t;
        }
    }
    xyz
}

impl StaticTree {
    pub fn is_leaf(&self, i: usize) -> bool {
        self.nodes[i][3] & 0x80 == 0
    }
    pub fn data(&self, i: usize) -> u32 {
        let n = &self.nodes[i];
        ((n[3] & 0x7f) as u32) << 16 | u16::from_le_bytes([n[4], n[5]]) as u32
    }

    /// Every node's decoded box (`unpack` from its parent's decoded box).
    pub fn boxes(&self) -> Vec<Aabb> {
        let mut out = vec![Aabb { min: [0.0; 3], max: [0.0; 3] }; self.nodes.len()];
        if self.nodes.is_empty() {
            return out;
        }
        let d = &self.domain;
        out[0] = Aabb { min: [d[0][0], d[0][1], d[0][2]], max: [d[1][0], d[1][1], d[1][2]] };
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            if self.is_leaf(i) {
                continue;
            }
            let l = i + 1;
            let r = i + 2 * self.data(i) as usize;
            for c in [l, r] {
                if c < self.nodes.len() {
                    let n = &self.nodes[c];
                    out[c] = unpack(&out[i], [n[0], n[1], n[2]]);
                    stack.push(c);
                }
            }
        }
        out
    }

    /// `(node, face)` of every leaf.
    pub fn leaves(&self) -> Vec<(usize, u32)> {
        (0..self.nodes.len()).filter(|&i| self.is_leaf(i)).map(|i| (i, self.data(i))).collect()
    }
}
