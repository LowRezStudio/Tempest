//! A bounding volume hierarchy over world-space triangles for shadow and
//! gather rays (Lightmass' `FStaticLightingAggregateMesh` plays this part;
//! any correct ray caster gives the same answers). Binned SAH build,
//! iterative traversal, Möller–Trumbore intersection; `f32` throughout.

pub type V3 = [f32; 3];

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}
pub fn normalize(a: V3) -> V3 {
    let l = length(a);
    if l > 1e-12 {
        scale(a, 1.0 / l)
    } else {
        [0.0; 3]
    }
}

#[derive(Clone, Copy, Debug)]
struct Aabb {
    min: V3,
    max: V3,
}

impl Aabb {
    const EMPTY: Aabb = Aabb { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3] };
    fn grow(&mut self, p: V3) {
        for k in 0..3 {
            self.min[k] = self.min[k].min(p[k]);
            self.max[k] = self.max[k].max(p[k]);
        }
    }
    fn merge(&mut self, o: &Aabb) {
        self.grow(o.min);
        self.grow(o.max);
    }
    fn area(&self) -> f32 {
        let d = sub(self.max, self.min);
        if d[0] < 0.0 {
            return 0.0;
        }
        2.0 * (d[0] * d[1] + d[1] * d[2] + d[2] * d[0])
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Aabb,
    /// Leaf: first triangle; inner: index of the left child (right = left + 1).
    start: u32,
    /// Triangles in a leaf, 0 for an inner node.
    count: u32,
}

/// A hit: distance along the ray, triangle and barycentrics (of v1, v2).
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub tri: u32,
    pub u: f32,
    pub v: f32,
}

pub struct Bvh {
    nodes: Vec<Node>,
    /// Triangle vertices in leaf order.
    tris: Vec<[V3; 3]>,
    /// The caller's triangle id of each entry of `tris`.
    ids: Vec<u32>,
}

const BINS: usize = 16;
const LEAF: usize = 4;

impl Bvh {
    /// Build over `tris`; hits report the index into this slice.
    pub fn build(tris: Vec<[V3; 3]>) -> Bvh {
        let n = tris.len();
        let mut ids: Vec<u32> = (0..n as u32).collect();
        let centroids: Vec<V3> = tris.iter().map(|t| scale(add(add(t[0], t[1]), t[2]), 1.0 / 3.0)).collect();
        let boxes: Vec<Aabb> = tris
            .iter()
            .map(|t| {
                let mut b = Aabb::EMPTY;
                for p in t {
                    b.grow(*p);
                }
                b
            })
            .collect();
        let mut nodes = vec![Node { bounds: Aabb::EMPTY, start: 0, count: n as u32 }];
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            let (start, count) = (nodes[ni].start as usize, nodes[ni].count as usize);
            let mut bounds = Aabb::EMPTY;
            let mut cb = Aabb::EMPTY;
            for &id in &ids[start..start + count] {
                bounds.merge(&boxes[id as usize]);
                cb.grow(centroids[id as usize]);
            }
            nodes[ni].bounds = bounds;
            if count <= LEAF {
                continue;
            }
            // Binned SAH over the widest centroid axis.
            let ext = sub(cb.max, cb.min);
            let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] { 0 } else if ext[1] >= ext[2] { 1 } else { 2 };
            if ext[axis] <= 0.0 {
                continue;
            }
            let k = BINS as f32 / ext[axis];
            let bin = |c: V3| (((c[axis] - cb.min[axis]) * k) as usize).min(BINS - 1);
            let mut bin_box = [Aabb::EMPTY; BINS];
            let mut bin_n = [0usize; BINS];
            for &id in &ids[start..start + count] {
                let b = bin(centroids[id as usize]);
                bin_box[b].merge(&boxes[id as usize]);
                bin_n[b] += 1;
            }
            let mut best = (f32::INFINITY, 0usize);
            for split in 1..BINS {
                let (mut l, mut r) = (Aabb::EMPTY, Aabb::EMPTY);
                let (mut ln, mut rn) = (0, 0);
                for b in 0..split {
                    l.merge(&bin_box[b]);
                    ln += bin_n[b];
                }
                for b in split..BINS {
                    r.merge(&bin_box[b]);
                    rn += bin_n[b];
                }
                if ln == 0 || rn == 0 {
                    continue;
                }
                let cost = l.area() * ln as f32 + r.area() * rn as f32;
                if cost < best.0 {
                    best = (cost, split);
                }
            }
            if !best.0.is_finite() || best.0 >= bounds.area() * count as f32 {
                if count <= 16 {
                    continue;
                }
            }
            let split = if best.0.is_finite() { best.1 } else { BINS / 2 };
            let slice = &mut ids[start..start + count];
            let mut i = 0;
            let mut j = slice.len();
            while i < j {
                if bin(centroids[slice[i] as usize]) < split {
                    i += 1;
                } else {
                    j -= 1;
                    slice.swap(i, j);
                }
            }
            let mid = if i == 0 || i == count { count / 2 } else { i };
            let left = nodes.len();
            nodes.push(Node { bounds: Aabb::EMPTY, start: start as u32, count: mid as u32 });
            nodes.push(Node { bounds: Aabb::EMPTY, start: (start + mid) as u32, count: (count - mid) as u32 });
            nodes[ni].start = left as u32;
            nodes[ni].count = 0;
            stack.push(left);
            stack.push(left + 1);
        }
        let ordered: Vec<[V3; 3]> = ids.iter().map(|&i| tris[i as usize]).collect();
        Bvh { nodes, tris: ordered, ids }
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    fn slab(b: &Aabb, o: V3, inv: V3, tmax: f32) -> Option<f32> {
        let mut t0 = 0f32;
        let mut t1 = tmax;
        for k in 0..3 {
            let a = (b.min[k] - o[k]) * inv[k];
            let c = (b.max[k] - o[k]) * inv[k];
            let (lo, hi) = if a < c { (a, c) } else { (c, a) };
            t0 = t0.max(lo);
            t1 = t1.min(hi);
            if t0 > t1 {
                return None;
            }
        }
        Some(t0)
    }

    fn tri(t: &[V3; 3], o: V3, d: V3, tmax: f32) -> Option<(f32, f32, f32)> {
        let e1 = sub(t[1], t[0]);
        let e2 = sub(t[2], t[0]);
        let p = cross(d, e2);
        let det = dot(e1, p);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let s = sub(o, t[0]);
        let u = dot(s, p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = cross(s, e1);
        let v = dot(d, q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = dot(e2, q) * inv;
        (t > 0.0 && t < tmax).then_some((t, u, v))
    }

    fn walk(&self, o: V3, d: V3, tmax: f32, any: bool) -> Option<Hit> {
        if self.nodes.is_empty() || self.tris.is_empty() {
            return None;
        }
        let inv = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
        let mut best: Option<Hit> = None;
        let mut limit = tmax;
        let mut stack = [0u32; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let n = &self.nodes[stack[sp] as usize];
            if Self::slab(&n.bounds, o, inv, limit).is_none() {
                continue;
            }
            if n.count > 0 {
                for i in n.start..n.start + n.count {
                    if let Some((t, u, v)) = Self::tri(&self.tris[i as usize], o, d, limit) {
                        let h = Hit { t, tri: self.ids[i as usize], u, v };
                        if any {
                            return Some(h);
                        }
                        limit = t;
                        best = Some(h);
                    }
                }
            } else if sp + 2 <= stack.len() {
                // Nearer child last so it is popped first.
                let (l, r) = (n.start, n.start + 1);
                let dl = Self::slab(&self.nodes[l as usize].bounds, o, inv, limit);
                let dr = Self::slab(&self.nodes[r as usize].bounds, o, inv, limit);
                match (dl, dr) {
                    (Some(a), Some(b)) => {
                        if a <= b {
                            stack[sp] = r;
                            stack[sp + 1] = l;
                        } else {
                            stack[sp] = l;
                            stack[sp + 1] = r;
                        }
                        sp += 2;
                    }
                    (Some(_), None) => {
                        stack[sp] = l;
                        sp += 1;
                    }
                    (None, Some(_)) => {
                        stack[sp] = r;
                        sp += 1;
                    }
                    (None, None) => {}
                }
            }
        }
        best
    }

    /// Does anything block the segment `o + t·d`, `0 < t < tmax` (`d` unit)?
    pub fn occluded(&self, o: V3, d: V3, tmax: f32) -> bool {
        self.walk(o, d, tmax, true).is_some()
    }

    /// The nearest hit along the ray.
    pub fn closest(&self, o: V3, d: V3, tmax: f32) -> Option<Hit> {
        self.walk(o, d, tmax, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_the_nearest() {
        let mut tris = Vec::new();
        for z in 0..50 {
            let z = z as f32 * 10.0;
            tris.push([[-1.0, -1.0, z], [1.0, -1.0, z], [0.0, 1.0, z]]);
        }
        let bvh = Bvh::build(tris);
        let h = bvh.closest([0.0, 0.0, 95.0], [0.0, 0.0, 1.0], 1e9).unwrap();
        assert_eq!(h.tri, 10);
        assert!((h.t - 5.0).abs() < 1e-4);
        assert!(bvh.occluded([0.0, 0.0, -5.0], [0.0, 0.0, 1.0], 6.0));
        assert!(!bvh.occluded([0.0, 0.0, -5.0], [0.0, 0.0, 1.0], 4.0));
        assert!(!bvh.occluded([5.0, 0.0, -5.0], [0.0, 0.0, 1.0], 1e9));
    }
}
