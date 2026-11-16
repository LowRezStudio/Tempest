//! Nav mesh generation from level geometry: the walkable surface of the
//! level's collision triangles (Recast: voxelise, filter, erode by the
//! agent radius, regions, contours, convex polygons) turned into an
//! `hkaiNavMesh` (faces counter-clockwise seen from +Z, edges chained a→b,
//! opposite edges paired by their vertices), with a cluster graph
//! (`hkaiDirectedGraphExplicitCost`) grown over it.
//!
//! This is not Havok's generator (`hkaiNavMeshGenerationUtils`, linked in
//! the 8.2 server): the faces differ from what Havok would make for the same
//! level, but follow the same rules the runtime relies on (convex faces,
//! symmetric opposites, every face in one cluster, a static AABB tree over
//! the faces). Everything here is in Havok units (Unreal / 16, Z up).

use std::collections::HashMap;

use glam::{UVec3, Vec3A};
use rerecast::{AreaType, BuildContoursFlags, ConfigBuilder, HeightfieldBuilder, PolygonNavmesh, TriMesh};

use super::navmesh::{edge_flags, Edge, Face, Graph, GraphEdge, NavMesh, INVALID};
use super::tagfile::f32_to_half;

/// What the agent can walk on, in Havok units.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The voxel size across and up.
    pub cell_size: f32,
    pub cell_height: f32,
    /// How far the mesh keeps from walls (the shipped meshes' `erosionRadius`: 2.0625 = 33 Unreal units).
    pub agent_radius: f32,
    /// The headroom a floor needs.
    pub agent_height: f32,
    /// The highest step walked up.
    pub max_climb: f32,
    /// The steepest walkable slope, in degrees.
    pub max_slope: f32,
    /// The largest face (the shipped meshes go up to 16 vertices).
    pub max_face_vertices: u16,
    /// Islands smaller than this (in cells) are dropped.
    pub min_region_area: u16,
    /// Regions smaller than this (in cells) merge into a neighbour.
    pub merge_region_area: u16,
    /// How far a simplified border may stray from the voxels (in cells).
    pub max_error: f32,
    /// The longest border edge before it is split (0: no limit).
    pub max_edge_length: f32,
    /// The area a cluster of faces grows to.
    pub cluster_area: f32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            // 12 Unreal units: finer cells cost minutes on the largest maps (watershed regions) and cover no more.
            cell_size: 0.75,
            cell_height: 0.25,
            agent_radius: 2.0625,
            // A champion stands about 180 Unreal units tall; UE3 steps up 35 and walks floors up to Z 0.7.
            agent_height: 11.0,
            max_climb: 2.1875,
            max_slope: 45.0,
            max_face_vertices: 12,
            min_region_area: 64,
            merge_region_area: 400,
            max_error: 1.3,
            max_edge_length: 0.0,
            cluster_area: 400.0,
        }
    }
}

/// Convex walkable polygons: their vertices and each face's vertex indices (counter-clockwise from +Z).
#[derive(Clone, Debug, Default)]
pub struct Polygons {
    pub vertices: Vec<[f32; 3]>,
    pub faces: Vec<Vec<u32>>,
    /// Region outlines Recast could not cut into polygons (left out).
    pub skipped: usize,
}

fn err(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

/// Havok (x, y, z up) → Recast (x, y up, z).
fn to_recast(p: [f32; 3]) -> Vec3A {
    Vec3A::new(p[0], p[2], p[1])
}

/// The walkable polygons of `triangles` (a triangle list, Havok units)
/// within `bounds` (min, max; default: the walkable triangles' extent).
/// Triangles wholly outside are left out, so far-off geometry (sky domes,
/// kill volumes) does not blow up the voxel grid.
pub fn polygons(triangles: &[[f32; 3]], bounds: Option<[[f32; 3]; 2]>, s: &Settings) -> Result<Polygons, String> {
    let cos = s.max_slope.to_radians().cos();
    let walkable = |t: &[[f32; 3]]| {
        let (a, b, c) = (to_recast(t[0]), to_recast(t[1]), to_recast(t[2]));
        // Either winding: collision hulls and render triangles do not agree on one.
        (b - a).cross(c - a).normalize_or_zero().y.abs() >= cos
    };
    let bounds = match bounds {
        Some(b) => b,
        None => {
            let mut b = [[f32::MAX; 3], [f32::MIN; 3]];
            for t in triangles.chunks_exact(3).filter(|t| walkable(t)) {
                for v in t {
                    for k in 0..3 {
                        b[0][k] = b[0][k].min(v[k]);
                        b[1][k] = b[1][k].max(v[k]);
                    }
                }
            }
            // Headroom above the highest floor.
            b[1][2] += s.agent_height * 2.0;
            b
        }
    };
    if bounds[0][0] > bounds[1][0] {
        return Err("there is no level geometry to walk on".into());
    }
    let outside = |t: &[[f32; 3]]| (0..3).any(|k| t.iter().all(|v| v[k] < bounds[0][k]) || t.iter().all(|v| v[k] > bounds[1][k]));
    let mut mesh = TriMesh::default();
    for t in triangles.chunks_exact(3).filter(|t| !outside(t)) {
        let k = mesh.vertices.len() as u32;
        // Clamped up and down: a wall rising out of the box still blocks, up to its top.
        mesh.vertices.extend(t.iter().map(|v| to_recast([v[0], v[1], v[2].clamp(bounds[0][2], bounds[1][2])])));
        mesh.indices.push(UVec3::new(k, k + 1, k + 2));
        mesh.area_types.push(if walkable(t) { AreaType::DEFAULT_WALKABLE } else { AreaType::NOT_WALKABLE });
    }
    if mesh.indices.is_empty() {
        return Err("no level geometry inside the nav mesh's bounds".into());
    }
    let aabb = rerecast::Aabb3d { min: to_recast(bounds[0]).into(), max: to_recast(bounds[1]).into() };
    if (aabb.max.y - aabb.min.y) / s.cell_height > 60000.0 {
        return Err(format!("the bounds are {} units tall: too tall for the voxel grid", aabb.max.y - aabb.min.y));
    }
    let cells = |v: f32| (v / s.cell_size).ceil().max(0.0) as u16;
    let voxels = |v: f32| (v / s.cell_height).ceil().max(0.0) as u16;
    let (walkable_height, walkable_climb) = (voxels(s.agent_height).max(3), (s.max_climb / s.cell_height).floor() as u16);
    let _ = ConfigBuilder::default();
    let mut hf = HeightfieldBuilder { aabb, cell_size: s.cell_size, cell_height: s.cell_height }.build().map_err(err)?;
    hf.populate_from_trimesh(mesh, walkable_height, walkable_climb).map_err(err)?;
    hf.filter_low_hanging_walkable_obstacles(walkable_climb);
    hf.filter_ledge_spans(walkable_height, walkable_climb);
    hf.filter_walkable_low_height_spans(walkable_height);
    let mut chf = hf.into_compact(walkable_height, walkable_climb).map_err(err)?;
    chf.erode_walkable_area(cells(s.agent_radius));
    chf.build_distance_field();
    chf.build_regions(0, s.min_region_area, s.merge_region_area).map_err(err)?;
    let contours = chf.build_contours(s.max_error, (s.max_edge_length / s.cell_size) as u16, BuildContoursFlags::DEFAULT);
    // Each contour on its own (as `rcBuildPolyMesh` does), so one Recast cannot
    // triangulate is left out (C++ Recast warns and skips it) instead of failing all.
    let mut out = Polygons::default();
    let mut shared: HashMap<[u16; 3], u32> = HashMap::new();
    for c in &contours.contours {
        let one = rerecast::ContourSet { contours: vec![c.clone()], ..contours.clone_empty() };
        match one.into_polygon_mesh(s.max_face_vertices) {
            Ok(pm) => add_recast(&pm, &mut out, &mut shared),
            Err(_) => out.skipped += 1,
        }
    }
    if out.faces.is_empty() {
        return Err("nothing in the level is walkable with these settings".into());
    }
    Ok(out)
}

trait CloneEmpty {
    fn clone_empty(&self) -> Self;
}

impl CloneEmpty for rerecast::ContourSet {
    fn clone_empty(&self) -> Self {
        rerecast::ContourSet { contours: Vec::new(), aabb: self.aabb, cell_size: self.cell_size, cell_height: self.cell_height, width: self.width, height: self.height, border_size: self.border_size, max_error: self.max_error }
    }
}

/// Add a Recast polygon mesh's faces, vertices shared by their cell coordinates.
fn add_recast(pm: &PolygonNavmesh, out: &mut Polygons, shared: &mut HashMap<[u16; 3], u32>) {
    let index: Vec<u32> = pm
        .vertices
        .iter()
        .map(|v| {
            *shared.entry([v.x, v.y, v.z]).or_insert_with(|| {
                let x = pm.aabb.min.x + v.x as f32 * pm.cell_size;
                let up = pm.aabb.min.y + v.y as f32 * pm.cell_height;
                let z = pm.aabb.min.z + v.z as f32 * pm.cell_size;
                out.vertices.push([x, z, up]);
                out.vertices.len() as u32 - 1
            })
        })
        .collect();
    let nvp = pm.max_vertices_per_polygon as usize;
    for p in 0..pm.polygon_count() {
        let mut f: Vec<u32> = pm.polygons[p * nvp..(p + 1) * nvp].iter().take_while(|&&i| i != PolygonNavmesh::NO_INDEX).map(|&i| index[i as usize]).collect();
        if f.len() < 3 {
            continue;
        }
        // Counter-clockwise seen from +Z, as the shipped faces.
        if signed_area(&out.vertices, &f) < 0.0 {
            f.reverse();
        }
        out.faces.push(f);
    }
}

/// Keep only the connected parts (faces sharing an edge) that hold a seed:
/// a point above a face, within `below` of it (a player start stands about
/// half a capsule over the floor) and not more than `above` under it.
/// Unreachable floors (roof tops, wall tops, closed interiors) go. Returns
/// the faces left out; nothing is dropped when no seed lands anywhere.
pub fn prune(p: &mut Polygons, seeds: &[[f32; 3]], below: f32, above: f32) -> usize {
    let n = p.faces.len();
    // Connected parts by shared edges.
    let mut part: Vec<usize> = (0..n).collect();
    fn root(part: &mut [usize], mut i: usize) -> usize {
        while part[i] != i {
            part[i] = part[part[i]];
            i = part[i];
        }
        i
    }
    let mut by_edge: HashMap<(u32, u32), usize> = HashMap::new();
    for (f, vs) in p.faces.iter().enumerate() {
        for k in 0..vs.len() {
            let (a, b) = (vs[k], vs[(k + 1) % vs.len()]);
            if let Some(&g) = by_edge.get(&(b, a)) {
                let (ra, rb) = (root(&mut part, f), root(&mut part, g));
                part[ra] = rb;
            }
            by_edge.insert((a, b), f);
        }
    }
    // Faces by the grid cells their boxes touch, so a seed tests only the faces around it.
    const CELL: f32 = 16.0;
    let key = |x: f32, y: f32| ((x / CELL).floor() as i32, (y / CELL).floor() as i32);
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (f, vs) in p.faces.iter().enumerate() {
        let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for &v in vs {
            let q = p.vertices[v as usize];
            lo = (lo.0.min(q[0]), lo.1.min(q[1]));
            hi = (hi.0.max(q[0]), hi.1.max(q[1]));
        }
        let (k0, k1) = (key(lo.0, lo.1), key(hi.0, hi.1));
        for x in k0.0..=k1.0 {
            for y in k0.1..=k1.1 {
                grid.entry((x, y)).or_default().push(f);
            }
        }
    }
    let mut keep = std::collections::HashSet::new();
    for seed in seeds {
        for &f in grid.get(&key(seed[0], seed[1])).map(Vec::as_slice).unwrap_or_default() {
            let pts: Vec<[f32; 3]> = p.faces[f].iter().map(|&v| p.vertices[v as usize]).collect();
            let inside = (0..pts.len()).all(|k| {
                let (u, v) = (pts[k], pts[(k + 1) % pts.len()]);
                (v[0] - u[0]) * (seed[1] - u[1]) - (v[1] - u[1]) * (seed[0] - u[0]) >= 0.0
            });
            let z = pts.iter().map(|q| q[2]).sum::<f32>() / pts.len() as f32;
            if inside && seed[2] >= z - above && seed[2] <= z + below {
                keep.insert(root(&mut part, f));
            }
        }
    }
    if keep.is_empty() {
        return 0;
    }
    let before = p.faces.len();
    let faces = std::mem::take(&mut p.faces);
    p.faces = faces.into_iter().enumerate().filter(|(f, _)| keep.contains(&root(&mut part, *f))).map(|(_, vs)| vs).collect();
    // The vertices only dropped faces used go too.
    let mut map = vec![u32::MAX; p.vertices.len()];
    let mut vertices = Vec::new();
    for f in &mut p.faces {
        for v in f.iter_mut() {
            if map[*v as usize] == u32::MAX {
                map[*v as usize] = vertices.len() as u32;
                vertices.push(p.vertices[*v as usize]);
            }
            *v = map[*v as usize];
        }
    }
    p.vertices = vertices;
    before - p.faces.len()
}

/// Twice the signed area of a face projected on the XY plane.
fn signed_area(vertices: &[[f32; 3]], f: &[u32]) -> f32 {
    let mut a = 0.0;
    for k in 0..f.len() {
        let (p, q) = (vertices[f[k] as usize], vertices[f[(k + 1) % f.len()] as usize]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a
}

/// An `hkaiNavMesh` of the polygons: one edge per face side (`ORIGINAL`), opposite edges paired.
pub fn nav_mesh(p: &Polygons, erosion_radius: f32) -> NavMesh {
    let mut m = NavMesh { erosion_radius, face_data_striding: 1, edge_data_striding: 1, ..Default::default() };
    m.vertices = p.vertices.iter().map(|v| [v[0], v[1], v[2], 0.0]).collect();
    for f in &p.faces {
        m.faces.push(Face { start_edge: m.edges.len() as i32, start_user_edge: -1, num_edges: f.len() as i16, num_user_edges: 0, cluster: 0, padding: 0xcdcd });
        for k in 0..f.len() {
            m.edges.push(Edge { a: f[k] as i32, b: f[(k + 1) % f.len()] as i32, opposite_edge: INVALID, opposite_face: INVALID, flags: edge_flags::ORIGINAL, user_edge_cost: 0 });
        }
    }
    let mut by_ends: HashMap<(i32, i32), (usize, usize)> = HashMap::new();
    for (fi, f) in m.faces.iter().enumerate() {
        for e in f.start_edge as usize..(f.start_edge + f.num_edges as i32) as usize {
            by_ends.insert((m.edges[e].a, m.edges[e].b), (fi, e));
        }
    }
    for fi in 0..m.faces.len() {
        let f = &m.faces[fi];
        for e in f.start_edge as usize..(f.start_edge + f.num_edges as i32) as usize {
            if let Some(&(of, oe)) = by_ends.get(&(m.edges[e].b, m.edges[e].a)) {
                if of != fi {
                    m.edges[e].opposite_edge = oe as u32;
                    m.edges[e].opposite_face = of as u32;
                }
            }
        }
    }
    m.face_data = vec![0; m.faces.len()];
    m.edge_data = vec![0; m.edges.len()];
    let mut lo = [f32::MAX; 4];
    let mut hi = [f32::MIN; 4];
    for v in &m.vertices {
        for k in 0..4 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    // The shipped boxes are grown by the erosion radius in every direction.
    for k in 0..3 {
        lo[k] -= erosion_radius;
        hi[k] += erosion_radius;
    }
    m.aabb = [lo, hi];
    m
}

fn face_centre(m: &NavMesh, f: usize) -> ([f32; 3], f32) {
    let face = &m.faces[f];
    let vs: Vec<[f32; 4]> = (0..face.num_edges as usize).map(|k| m.vertices[m.edges[face.start_edge as usize + k].a as usize]).collect();
    let mut c = [0.0; 3];
    for v in &vs {
        for k in 0..3 {
            c[k] += v[k] / vs.len() as f32;
        }
    }
    let mut area = 0.0;
    for k in 1..vs.len().saturating_sub(1) {
        let (a, b, d) = (vs[0], vs[k], vs[k + 1]);
        area += ((b[0] - a[0]) * (d[1] - a[1]) - (d[0] - a[0]) * (b[1] - a[1])).abs() * 0.5;
    }
    (c, area)
}

fn distance(a: [f32; 4], b: [f32; 4]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Group the faces into clusters (grown breadth first over opposite edges up
/// to `cluster_area`) and build the cluster graph: a node per cluster at the
/// centre of its face nearest to its centroid, an edge each way between
/// clusters that share a face edge, costing their distance.
pub fn cluster(m: &mut NavMesh, cluster_area: f32) -> Graph {
    let n = m.faces.len();
    let centres: Vec<([f32; 3], f32)> = (0..n).map(|f| face_centre(m, f)).collect();
    let mut cluster = vec![usize::MAX; n];
    let mut members: Vec<Vec<usize>> = Vec::new();
    for seed in 0..n {
        if cluster[seed] != usize::MAX {
            continue;
        }
        let c = members.len();
        let mut queue = std::collections::VecDeque::from([seed]);
        cluster[seed] = c;
        let mut faces = Vec::new();
        let mut area = 0.0;
        while let Some(f) = queue.pop_front() {
            faces.push(f);
            area += centres[f].1;
            if area >= cluster_area {
                continue;
            }
            let face = &m.faces[f];
            for e in face.start_edge as usize..(face.start_edge + face.num_edges as i32) as usize {
                let o = m.edges[e].opposite_face;
                if o != INVALID && cluster[o as usize] == usize::MAX {
                    cluster[o as usize] = c;
                    queue.push_back(o as usize);
                }
            }
        }
        members.push(faces);
    }
    for (f, face) in m.faces.iter_mut().enumerate() {
        face.cluster = cluster[f].min(i16::MAX as usize) as i16;
    }
    let positions: Vec<[f32; 4]> = members
        .iter()
        .map(|fs| {
            let total: f32 = fs.iter().map(|&f| centres[f].1).sum::<f32>().max(1e-6);
            let mut c = [0.0f32; 3];
            for &f in fs {
                for (ck, fk) in c.iter_mut().zip(centres[f].0) {
                    *ck += fk * centres[f].1 / total;
                }
            }
            let near = fs.iter().copied().min_by(|&a, &b| {
                let d = |f: usize| (0..3).map(|k| (centres[f].0[k] - c[k]).powi(2)).sum::<f32>();
                d(a).total_cmp(&d(b))
            });
            let p = near.map(|f| centres[f].0).unwrap_or(c);
            [p[0], p[1], p[2], 0.0]
        })
        .collect();
    let mut links: Vec<Vec<u32>> = vec![Vec::new(); members.len()];
    for (f, face) in m.faces.iter().enumerate() {
        for e in face.start_edge as usize..(face.start_edge + face.num_edges as i32) as usize {
            let o = m.edges[e].opposite_face;
            if o == INVALID {
                continue;
            }
            let (a, b) = (cluster[f], cluster[o as usize]);
            if a != b && !links[a].contains(&(b as u32)) {
                links[a].push(b as u32);
            }
        }
    }
    let mut g = Graph { positions, ..Default::default() };
    for (c, targets) in links.iter_mut().enumerate() {
        targets.sort_unstable();
        g.nodes.push((g.edges.len() as i32, targets.len() as i32));
        for &t in targets.iter() {
            let cost = distance(g.positions[c], g.positions[t as usize]);
            g.edges.push(GraphEdge { cost: f32_to_half(cost), flags: 0, target: t });
        }
    }
    g
}

/// How much of `a` lies on `b`: the area-weighted share of `a`'s faces
/// whose centre is inside a face of `b` (seen from above, within 2 units of
/// its height). Compares a generated mesh with the one it replaces.
pub fn coverage(a: &NavMesh, b: &NavMesh) -> f32 {
    const CELL: f32 = 8.0;
    let key = |x: f32, y: f32| ((x / CELL).floor() as i32, (y / CELL).floor() as i32);
    // `b`'s faces by the grid cells their boxes touch.
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let polys: Vec<Vec<[f32; 4]>> = (0..b.faces.len())
        .map(|f| {
            let face = &b.faces[f];
            (0..face.num_edges as usize).filter_map(|k| b.edges.get(face.start_edge as usize + k)).filter_map(|e| b.vertices.get(e.a as usize).copied()).collect()
        })
        .collect();
    for (f, vs) in polys.iter().enumerate() {
        if vs.is_empty() {
            continue;
        }
        let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for v in vs {
            lo = (lo.0.min(v[0]), lo.1.min(v[1]));
            hi = (hi.0.max(v[0]), hi.1.max(v[1]));
        }
        let (k0, k1) = (key(lo.0, lo.1), key(hi.0, hi.1));
        for x in k0.0..=k1.0 {
            for y in k0.1..=k1.1 {
                grid.entry((x, y)).or_default().push(f);
            }
        }
    }
    let inside = |vs: &[[f32; 4]], p: [f32; 3]| -> bool {
        let mut z = 0.0;
        for k in 0..vs.len() {
            let (u, v) = (vs[k], vs[(k + 1) % vs.len()]);
            if (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0]) < 0.0 {
                return false;
            }
            z += u[2] / vs.len() as f32;
        }
        (z - p[2]).abs() < 2.0
    };
    let (mut covered, mut total) = (0.0f64, 0.0f64);
    for f in 0..a.faces.len() {
        let (c, area) = face_centre(a, f);
        total += area as f64;
        let hit = grid.get(&key(c[0], c[1])).is_some_and(|fs| fs.iter().any(|&g| inside(&polys[g], c)));
        if hit {
            covered += area as f64;
        }
    }
    if total > 0.0 { (covered / total) as f32 } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 128×64 floor with a 16×16 block 8 tall (lower than the agent, too high to climb) in its middle.
    fn floor() -> Vec<[f32; 3]> {
        let quad = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| vec![a, b, c, a, c, d];
        let mut t = quad([0.0, 0.0, 0.0], [128.0, 0.0, 0.0], [128.0, 64.0, 0.0], [0.0, 64.0, 0.0]);
        let (x0, x1, y0, y1, h) = (56.0, 72.0, 24.0, 40.0, 8.0);
        t.extend(quad([x0, y0, h], [x1, y0, h], [x1, y1, h], [x0, y1, h]));
        t.extend(quad([x0, y0, 0.0], [x1, y0, 0.0], [x1, y0, h], [x0, y0, h]));
        t.extend(quad([x1, y0, 0.0], [x1, y1, 0.0], [x1, y1, h], [x1, y0, h]));
        t.extend(quad([x1, y1, 0.0], [x0, y1, 0.0], [x0, y1, h], [x1, y1, h]));
        t.extend(quad([x0, y1, 0.0], [x0, y0, 0.0], [x0, y0, h], [x0, y1, h]));
        t
    }

    #[test]
    fn a_floor_becomes_a_valid_nav_mesh() {
        let s = Settings { min_region_area: 4, merge_region_area: 20, ..Default::default() };
        let p = polygons(&floor(), None, &s).unwrap();
        assert!(!p.faces.is_empty());
        for f in &p.faces {
            assert!(signed_area(&p.vertices, f) > 0.0, "faces wind counter-clockwise from +Z");
        }
        let mut m = nav_mesh(&p, s.agent_radius);
        assert!(super::super::navmesh::validate(&m).is_empty(), "{:?}", super::super::navmesh::validate(&m));
        // Eroded by the radius: nothing within it of the outer border, and the floor goes round the pillar.
        let on_floor: Vec<&[f32; 4]> = m.vertices.iter().filter(|v| v[2].abs() < 0.5).collect();
        for v in &on_floor {
            assert!(v[0] >= s.agent_radius - s.cell_size && v[0] <= 128.0 - s.agent_radius + s.cell_size, "{v:?}");
            assert!(!(v[0] > 57.0 && v[0] < 71.0 && v[1] > 25.0 && v[1] < 39.0), "a floor vertex under the pillar: {v:?}");
        }
        assert!(m.faces.len() >= 4, "{} faces", m.faces.len());
        let g = cluster(&mut m, 400.0);
        assert!(g.nodes.len() >= 2, "a 128×64 floor makes more than one 400-unit cluster");
        assert!(m.faces.iter().all(|f| (f.cluster as usize) < g.nodes.len()));
        assert!((coverage(&m, &m) - 1.0).abs() < 1e-4, "a mesh covers itself");
        // A seed on the floor keeps the floor and drops the block's top.
        let mut q = p.clone();
        let tops = q.faces.iter().filter(|f| f.iter().all(|&v| q.vertices[v as usize][2] > 4.0)).count();
        assert!(tops > 0, "the block's top is walkable before pruning");
        assert_eq!(prune(&mut q, &[[10.0, 10.0, 5.0]], 8.0, 2.0), tops);
        assert!(q.faces.iter().all(|f| f.iter().all(|&v| q.vertices[v as usize][2] < 4.0)));
        // Every graph edge has its way back.
        for (a, (start, count)) in g.nodes.iter().enumerate() {
            for e in &g.edges[*start as usize..(*start + *count) as usize] {
                let (s2, c2) = g.nodes[e.target as usize];
                assert!(g.edges[s2 as usize..(s2 + c2) as usize].iter().any(|b| b.target == a as u32));
            }
        }
    }
}
