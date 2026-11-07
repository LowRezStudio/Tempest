//! Nav mesh buffers for the Studio's map view (`map.navmesh`), in the
//! view space of `map.rs` (glTF metres, Y up: Unreal `(x, z, y) / 100`,
//! Unreal = Havok × 16).

use super::navmesh::{edge_flags, NavData, HAVOK_TO_UNREAL, INVALID};

/// Havok → view space.
pub fn to_view(p: [f32; 4]) -> [f32; 3] {
    let s = HAVOK_TO_UNREAL / 100.0;
    [p[0] * s, p[2] * s, p[1] * s]
}

/// View space → Havok.
pub fn from_view(v: [f32; 3]) -> [f32; 3] {
    let s = 100.0 / HAVOK_TO_UNREAL;
    [v[0] * s, v[2] * s, v[1] * s]
}

#[derive(Default)]
pub struct Buffers {
    /// xyz per vertex.
    pub positions: Vec<f32>,
    /// Three vertex indices per triangle (faces fanned from their first vertex).
    pub triangles: Vec<u32>,
    /// Face of each triangle.
    pub tri_face: Vec<u32>,
    pub face_cluster: Vec<i32>,
    pub face_region: Vec<i32>,
    /// Vertex pairs: every edge once (the lower index of an opposite pair).
    pub lines: Vec<u32>,
    /// Per line: 0 inner edge, 1 boundary, 2 blocked/user.
    pub line_kind: Vec<u8>,
    /// Per user edge pair: four points (the A segment's ends, then B's).
    pub links: Vec<f32>,
    pub regions: usize,
}

/// Connected faces through opposite edges (user edges, i.e. jump links,
/// do not join regions), numbered in face order.
pub fn regions(nd: &NavData) -> (Vec<i32>, usize) {
    let m = &nd.mesh;
    let mut region = vec![-1i32; m.faces.len()];
    let mut n = 0;
    for start in 0..m.faces.len() {
        if region[start] >= 0 {
            continue;
        }
        let mut stack = vec![start];
        region[start] = n as i32;
        while let Some(f) = stack.pop() {
            let face = &m.faces[f];
            for k in 0..face.num_edges.max(0) as usize {
                let Some(e) = m.edges.get(face.start_edge as usize + k) else { continue };
                if e.opposite_face == INVALID || e.flags & edge_flags::EXTERNAL_OPPOSITE != 0 {
                    continue;
                }
                let o = e.opposite_face as usize;
                if o < region.len() && region[o] < 0 {
                    region[o] = n as i32;
                    stack.push(o);
                }
            }
        }
        n += 1;
    }
    (region, n)
}

pub fn buffers(nd: &NavData) -> Buffers {
    let m = &nd.mesh;
    let mut b = Buffers::default();
    for v in &m.vertices {
        b.positions.extend_from_slice(&to_view(*v));
    }
    for (fi, f) in m.faces.iter().enumerate() {
        let vs = nd.face_vertices(fi);
        for k in 1..vs.len().saturating_sub(1) {
            b.triangles.extend_from_slice(&[vs[0], vs[k], vs[k + 1]]);
            b.tri_face.push(fi as u32);
        }
        b.face_cluster.push(f.cluster as i32);
        for k in 0..f.num_edges.max(0) as usize {
            let ei = f.start_edge as usize + k;
            let Some(e) = m.edges.get(ei) else { continue };
            let boundary = e.opposite_edge == INVALID;
            if !boundary && (e.opposite_edge as usize) < ei {
                continue;
            }
            b.lines.extend_from_slice(&[e.a as u32, e.b as u32]);
            b.line_kind.push(if e.flags & (edge_flags::BLOCKED | edge_flags::USER) != 0 { 2 } else if boundary { 1 } else { 0 });
        }
    }
    let (r, n) = regions(nd);
    b.face_region = r;
    b.regions = n;
    if let Some(l) = &nd.library {
        for p in &l.user_edge_pairs {
            for k in 0..4 {
                b.links.extend_from_slice(&to_view([p.x[k], p.y[k], p.z[k], 0.0]));
            }
        }
    }
    b
}

pub fn f32_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn u32_bytes(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn i32_bytes(v: &[i32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
