//! Geometry extraction from mesh payloads and export to glTF (`.glb`) / OBJ.
//!
//! Static meshes: positions from `FPositionVertexBuffer`, tangent-space
//! normals and UVs from `FStaticMeshVertexBuffer` (`FPackedNormal` bytes,
//! half or full float UVs), 16-bit index buffer, one primitive per
//! `FStaticMeshElement`. Skeletal meshes: the rigid/soft vertices of each
//! chunk (full floats) plus the multi-size index buffer; one primitive per
//! section. Bone influences are kept for the GUI but not exported as a skin
//! yet.
//!
//! Unreal is Z-up, left-handed, centimetres; glTF is Y-up, right-handed,
//! metres: `(x, y, z) → (x, z, y) / 100`.

use std::fmt::Write as _;

use tempest_upk::io::{Error, PackageIndex, Result};
use tempest_upk::Package;

use crate::dds::half_to_f32;
use crate::mesh::{SkeletalMesh, StaticMesh};
use crate::object::{Native, Object};

#[derive(Clone, Debug, Default)]
pub struct Section {
    pub first_index: u32,
    pub num_triangles: u32,
    pub material: PackageIndex,
    pub material_name: String,
    /// Material slot of the mesh (what a component's `Materials[]` overrides).
    pub slot: usize,
}

/// Triangle-list geometry of one LOD.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// First UV set.
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub sections: Vec<Section>,
    /// Per-vertex (bone index, weight) × 4 for skeletal meshes (bone indices
    /// are into `bone_names`, weights 0..255).
    pub influences: Vec<[(u16, u8); 4]>,
    pub bone_names: Vec<String>,
    /// Reference skeleton (parent index, local transform in Unreal space,
    /// column-major, `v' = M v`).
    pub bones: Vec<Bone>,
    /// Textured materials aligned with `sections` (empty when not resolved).
    pub materials: Vec<GltfMaterial>,
    /// PNG images referenced by `materials`.
    pub images: Vec<GltfImage>,
}

#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub local: [[f32; 4]; 4],
}

#[derive(Clone, Debug, Default)]
pub struct GltfMaterial {
    pub name: String,
    pub base_color: Option<usize>,
    pub normal: Option<usize>,
    pub emissive: Option<usize>,
    /// Other textures (parameter name, image), stored in `extras`.
    pub extra: Vec<(String, usize)>,
}

#[derive(Clone, Debug)]
pub struct GltfImage {
    pub name: String,
    pub png: Vec<u8>,
}

/// Column-major 4x4 from a UE3 `FQuat` (x, y, z, w) and translation.
pub fn quat_translation_matrix(q: [f32; 4], t: [f32; 3]) -> [[f32; 4]; 4] {
    let [x, y, z, w] = q;
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz) = (x * y, x * z, y * z);
    let (wx, wy, wz) = (w * x, w * y, w * z);
    // Columns.
    [
        [1.0 - 2.0 * (yy + zz), 2.0 * (xy + wz), 2.0 * (xz - wy), 0.0],
        [2.0 * (xy - wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz + wx), 0.0],
        [2.0 * (xz + wy), 2.0 * (yz - wx), 1.0 - 2.0 * (xx + yy), 0.0],
        [t[0], t[1], t[2], 1.0],
    ]
}

pub fn mat_mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut m = [[0f32; 4]; 4];
    for (c, col) in m.iter_mut().enumerate() {
        for (r, cell) in col.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    m
}

/// Inverse of a rigid transform (rotation + translation).
pub fn mat_inverse_rigid(m: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut inv = [[0f32; 4]; 4];
    for c in 0..3 {
        for r in 0..3 {
            inv[c][r] = m[r][c];
        }
    }
    let t = [m[3][0], m[3][1], m[3][2]];
    let last: [f32; 3] = std::array::from_fn(|r| -(inv[0][r] * t[0] + inv[1][r] * t[1] + inv[2][r] * t[2]));
    inv[3] = [last[0], last[1], last[2], 1.0];
    inv
}

/// The rotation (column-major 4x4) taking the +Y axis onto the unit vector `d`.
pub fn rotation_y_to(d: [f32; 3]) -> [[f32; 4]; 4] {
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let mut id = [[0f32; 4]; 4];
    for (i, col) in id.iter_mut().enumerate() {
        col[i] = 1.0;
    }
    if l < 1e-6 {
        return id;
    }
    let d = [d[0] / l, d[1] / l, d[2] / l];
    let c = d[1]; // Y · d
    if c > 1.0 - 1e-6 {
        return id;
    }
    if c < -1.0 + 1e-6 {
        // Opposite: half turn around X.
        return [[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    }
    // Axis = Y × d = (d.z, 0, -d.x); Rodrigues' formula.
    let axis = [d[2], 0.0, -d[0]];
    let s = (axis[0] * axis[0] + axis[2] * axis[2]).sqrt();
    let k = [axis[0] / s, 0.0, axis[2] / s];
    let t = 1.0 - c;
    let r = |i: usize, j: usize| -> f32 {
        let kk = k[i] * k[j] * t;
        let delta = if i == j { c } else { 0.0 };
        // Levi-Civita term s * ε_{ijk} k_k.
        let cross = match (i, j) {
            (0, 1) => -k[2],
            (1, 0) => k[2],
            (0, 2) => k[1],
            (2, 0) => -k[1],
            (1, 2) => -k[0],
            (2, 1) => k[0],
            _ => 0.0,
        } * s;
        kk + delta + cross
    };
    // Column-major: out[col][row] = R[row][col].
    let mut out = [[0f32; 4]; 4];
    for (col, column) in out.iter_mut().enumerate().take(3) {
        for (row, cell) in column.iter_mut().enumerate().take(3) {
            *cell = r(row, col);
        }
    }
    out[3][3] = 1.0;
    out
}

/// Re-express an Unreal-space transform in glTF space (`C M C⁻¹` with the
/// y/z swap and centimetre → metre scale).
pub fn to_gltf_matrix(m: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let p = |i: usize| match i {
        1 => 2,
        2 => 1,
        i => i,
    };
    let mut out = [[0f32; 4]; 4];
    for c in 0..3 {
        for r in 0..3 {
            out[c][r] = m[p(c)][p(r)];
        }
    }
    for r in 0..3 {
        out[3][r] = m[3][p(r)] * 0.01;
    }
    out[3][3] = 1.0;
    out
}

fn packed_normal(b: &[u8]) -> [f32; 3] {
    let f = |x: u8| (x as f32 - 127.5) / 127.5;
    [f(b[0]), f(b[1]), f(b[2])]
}

pub fn static_mesh_geometry(pkg: &Package, m: &StaticMesh, lod: usize) -> Result<Geometry> {
    let l = m.lod_models.get(lod).ok_or_else(|| Error::Format(format!("no LOD {lod}")))?;
    let n = l.num_vertices as usize;
    let pvb = &l.position_vb;
    if pvb.data.elem_size != 12 || pvb.data.count as usize != n {
        return Err(Error::Format(format!("unexpected position buffer ({} x {})", pvb.data.elem_size, pvb.data.count)));
    }
    let mut g = Geometry::default();
    for i in 0..n {
        let e = pvb.data.elem(i);
        let f = |o: usize| f32::from_le_bytes([e[o], e[o + 1], e[o + 2], e[o + 3]]);
        g.positions.push([f(0), f(4), f(8)]);
    }
    let vb = &l.vertex_buffer;
    let Some(data) = &vb.data else {
        return Err(Error::Format("this package was cooked for a server: the GPU vertex buffer is stripped (no normals/UVs)".into()));
    };
    let stride = vb.stride as usize;
    let full = vb.use_full_precision_uvs != 0;
    if data.elem_size as usize != stride || data.count as usize != n || stride < 8 {
        return Err(Error::Format(format!("unexpected vertex buffer ({} x {})", data.elem_size, data.count)));
    }
    for i in 0..n {
        let e = data.elem(i);
        g.normals.push(packed_normal(&e[4..8]));
        let uv = if full {
            [f32::from_le_bytes([e[8], e[9], e[10], e[11]]), f32::from_le_bytes([e[12], e[13], e[14], e[15]])]
        } else {
            [half_to_f32(u16::from_le_bytes([e[8], e[9]])), half_to_f32(u16::from_le_bytes([e[10], e[11]]))]
        };
        g.uvs.push(uv);
    }
    let ib = &l.index_buffer;
    g.indices = match ib.elem_size {
        2 => ib.data.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect(),
        4 => ib.data.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
        s => return Err(Error::Format(format!("index size {s}"))),
    };
    for e in &l.elements {
        g.sections.push(Section { first_index: e.first_index, num_triangles: e.num_triangles, material: e.material, material_name: pkg.object_path(e.material), slot: e.material_index.max(0) as usize });
    }
    Ok(g)
}

pub fn skeletal_mesh_geometry(pkg: &Package, m: &SkeletalMesh, lod: usize) -> Result<Geometry> {
    let l = m.lod_models.get(lod).ok_or_else(|| Error::Format(format!("no LOD {lod}")))?;
    let mut g = Geometry { bone_names: m.ref_skeleton.iter().map(|b| pkg.fname_to_string(b.name)).collect(), ..Default::default() };
    for (i, b) in m.ref_skeleton.iter().enumerate() {
        let parent = if i == 0 { None } else { Some(b.parent_index.max(0) as usize) };
        g.bones.push(Bone { name: pkg.fname_to_string(b.name), parent, local: quat_translation_matrix(b.orientation, b.position) });
    }
    // Cooked packages strip the CPU-side rigid/soft vertex arrays; the GPU
    // skin buffer (`TGPUSkinVertexFloat16Uvs` / `Float32Uvs`) is the source:
    // TangentX, TangentZ, InfluenceBones[4], InfluenceWeights[4], Position, UVs.
    let vb = &l.vertex_buffer;
    let Some(data) = &vb.data else {
        return Err(Error::Format("this package was cooked for a server: the GPU skin vertex buffer is stripped".into()));
    };
    // `bUsePackedPosition` is set on PC meshes too, but the cooker only packs
    // for consoles: the stride tells whether positions are 12-byte floats or
    // a 4-byte `FPackedPosition` (X:11, Y:11, Z:10 signed bits, low bits
    // first; `Position = Unpacked * MeshExtension + MeshOrigin`).
    let stride = data.elem_size as usize;
    let n = data.count as usize;
    let full = vb.use_full_precision_uvs != 0;
    let uv_bytes = vb.num_tex_coords.max(1) as usize * if full { 8 } else { 4 };
    let pos_size = stride.saturating_sub(16 + uv_bytes);
    let packed = match pos_size {
        12 => false,
        4 => true,
        _ => return Err(Error::Format(format!("unexpected GPU skin vertex stride {stride} ({} uv sets, {})", vb.num_tex_coords, if full { "float" } else { "half" }))),
    };
    if n != l.num_vertices as usize {
        return Err(Error::Format(format!("unexpected GPU skin buffer ({} x {})", data.elem_size, data.count)));
    }
    let uv_off = 16 + pos_size;
    // Chunk of a vertex (for its bone map).
    let mut chunk_of = vec![usize::MAX; n];
    for (ci, c) in l.chunks.iter().enumerate() {
        let start = c.base_vertex_index as usize;
        let count = (c.num_rigid_vertices + c.num_soft_vertices).max(0) as usize;
        for slot in chunk_of.iter_mut().skip(start).take(count) {
            *slot = ci;
        }
    }
    for i in 0..n {
        let e = data.elem(i);
        let f = |o: usize| f32::from_le_bytes([e[o], e[o + 1], e[o + 2], e[o + 3]]);
        g.normals.push(packed_normal(&e[4..8]));
        if packed {
            let v = u32::from_le_bytes([e[16], e[17], e[18], e[19]]);
            let sx = (((v & 0x7ff) << 21) as i32 >> 21) as f32 / 1023.0;
            let sy = ((((v >> 11) & 0x7ff) << 21) as i32 >> 21) as f32 / 1023.0;
            let sz = ((((v >> 22) & 0x3ff) << 22) as i32 >> 22) as f32 / 511.0;
            g.positions.push([sx * vb.mesh_extension[0] + vb.mesh_origin[0], sy * vb.mesh_extension[1] + vb.mesh_origin[1], sz * vb.mesh_extension[2] + vb.mesh_origin[2]]);
        } else {
            g.positions.push([f(16), f(20), f(24)]);
        }
        let uv = if full { [f(uv_off), f(uv_off + 4)] } else { [half_to_f32(u16::from_le_bytes([e[uv_off], e[uv_off + 1]])), half_to_f32(u16::from_le_bytes([e[uv_off + 2], e[uv_off + 3]]))] };
        g.uvs.push(uv);
        let mut inf = [(0u16, 0u8); 4];
        let bone_map = chunk_of.get(i).and_then(|&ci| l.chunks.get(ci)).map(|c| c.bone_map.as_slice()).unwrap_or(&[]);
        for k in 0..4 {
            let local = e[8 + k] as usize;
            let bone = bone_map.get(local).copied().unwrap_or(local as u16);
            inf[k] = (bone, e[12 + k]);
        }
        g.influences.push(inf);
    }
    g.indices = l.index_container.to_u32();
    for s in &l.sections {
        let material = m.materials.get(s.material_index as usize).copied().unwrap_or_default();
        g.sections.push(Section { first_index: s.base_index, num_triangles: s.num_triangles, material, material_name: pkg.object_path(material), slot: s.material_index as usize });
    }
    Ok(g)
}

/// `LODInfo[lod].LODMaterialMap`: section material slot → mesh material
/// slot for the lower LODs of a skeletal mesh (empty when identity).
fn lod_material_map(pkg: &Package, obj: &Object, lod: usize) -> Vec<usize> {
    use crate::value::{StructValue, Value};
    let Some(p) = obj.property("LODInfo", pkg) else { return Vec::new() };
    let Value::Array(items) = &p.value else { return Vec::new() };
    let Some(Value::Struct(StructValue::Tagged(members))) = items.get(lod) else { return Vec::new() };
    let Some(map) = members.iter().find(|m| pkg.fname_to_string(m.name).eq_ignore_ascii_case("LODMaterialMap")) else { return Vec::new() };
    let Value::Array(ids) = &map.value else { return Vec::new() };
    ids.iter().map(|v| if let Value::Int(i) = v { (*i).max(0) as usize } else { 0 }).collect()
}

/// Geometry of a parsed mesh object (static or skeletal).
pub fn geometry(pkg: &Package, obj: &Object, lod: usize) -> Result<Geometry> {
    match &obj.native {
        Native::StaticMesh(m) => static_mesh_geometry(pkg, m, lod),
        Native::SkeletalMesh(m) => {
            let mut g = skeletal_mesh_geometry(pkg, m, lod)?;
            let map = lod_material_map(pkg, obj, lod);
            if !map.is_empty() {
                let l = &m.lod_models[lod];
                for (sec, s) in g.sections.iter_mut().zip(&l.sections) {
                    let Some(&slot) = map.get(s.material_index as usize) else { continue };
                    let material = m.materials.get(slot).copied().unwrap_or_default();
                    sec.material = material;
                    sec.material_name = pkg.object_path(material);
                    sec.slot = slot;
                }
            }
            Ok(g)
        }
        _ => Err(Error::Format("not a static or skeletal mesh".into())),
    }
}

pub fn lod_count(obj: &Object) -> usize {
    match &obj.native {
        Native::StaticMesh(m) => m.lod_models.len(),
        Native::SkeletalMesh(m) => m.lod_models.len(),
        _ => 0,
    }
}

fn to_gltf(p: [f32; 3]) -> [f32; 3] {
    [p[0] * 0.01, p[2] * 0.01, p[1] * 0.01]
}

fn to_gltf_dir(p: [f32; 3]) -> [f32; 3] {
    let v = [p[0], p[2], p[1]];
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-6 { [v[0] / len, v[1] / len, v[2] / len] } else { [0.0, 1.0, 0.0] }
}

/// Wavefront OBJ with one group per section.
pub fn write_obj(name: &str, g: &Geometry) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# {name}\no {name}");
    for p in &g.positions {
        let q = to_gltf(*p);
        let _ = writeln!(s, "v {} {} {}", q[0], q[1], q[2]);
    }
    for uv in &g.uvs {
        let _ = writeln!(s, "vt {} {}", uv[0], 1.0 - uv[1]);
    }
    for n in &g.normals {
        let q = to_gltf_dir(*n);
        let _ = writeln!(s, "vn {} {} {}", q[0], q[1], q[2]);
    }
    for (si, sec) in g.sections.iter().enumerate() {
        let _ = writeln!(s, "g section{si}\nusemtl {}", sec.material_name.replace(' ', "_"));
        let start = sec.first_index as usize;
        for t in 0..sec.num_triangles as usize {
            let i = &g.indices[start + t * 3..start + t * 3 + 3];
            let f = |k: u32| format!("{0}/{0}/{0}", k + 1);
            let _ = writeln!(s, "f {} {} {}", f(i[0]), f(i[2]), f(i[1]));
        }
    }
    s
}

/// Binary glTF 2.0: one mesh with a primitive per section, textured
/// materials when `materials`/`images` are filled, and a skin with the
/// reference skeleton when `bones` is filled.
/// An animation for [`write_glb_anims`]: frame times (seconds) and, per
/// frame, every bone's local transform in Unreal space (as `Bone::local`).
#[derive(Clone, Debug)]
pub struct GltfAnim {
    pub name: String,
    pub times: Vec<f32>,
    pub frames: Vec<Vec<[[f32; 4]; 4]>>,
}

/// Rotation of a column-major rigid matrix as a quaternion (x, y, z, w).
pub fn mat_to_quat(m: &[[f32; 4]; 4]) -> [f32; 4] {
    let (m00, m11, m22) = (m[0][0], m[1][1], m[2][2]);
    let tr = m00 + m11 + m22;
    // r(row, col) = m[col][row]
    let r = |row: usize, col: usize| m[col][row];
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(r(2, 1) - r(1, 2)) / s, (r(0, 2) - r(2, 0)) / s, (r(1, 0) - r(0, 1)) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (r(0, 1) + r(1, 0)) / s, (r(0, 2) + r(2, 0)) / s, (r(2, 1) - r(1, 2)) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(r(0, 1) + r(1, 0)) / s, 0.25 * s, (r(1, 2) + r(2, 1)) / s, (r(0, 2) - r(2, 0)) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(r(0, 2) + r(2, 0)) / s, (r(1, 2) + r(2, 1)) / s, 0.25 * s, (r(1, 0) - r(0, 1)) / s]
    };
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-12);
    [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
}

/// The per-bone fix rotations of a skeleton in glTF space (see
/// `write_glb_anims`): what turns each joint so +Y points along its limb.
pub fn joint_fixes(g: &Geometry) -> Vec<[[f32; 4]; 4]> {
    skeleton_fixes(g).1
}

/// The skeleton's children lists, per-bone fix rotations (glTF space) and
/// global bind matrices (glTF space): see `write_glb_anims`.
#[allow(clippy::type_complexity)]
fn skeleton_fixes(g: &Geometry) -> (Vec<Vec<usize>>, Vec<[[f32; 4]; 4]>, Vec<[[f32; 4]; 4]>) {
    let n_bones = g.bones.len();
    let mut globals_ue: Vec<[[f32; 4]; 4]> = Vec::with_capacity(n_bones);
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n_bones];
    for (i, b) in g.bones.iter().enumerate() {
        let global = match b.parent {
            Some(p) if p < i => mat_mul(&globals_ue[p], &b.local),
            _ => b.local,
        };
        globals_ue.push(global);
        if let Some(p) = b.parent {
            if p < i {
                children[p].push(i);
            }
        }
    }
    let globals: Vec<[[f32; 4]; 4]> = globals_ue.iter().map(to_gltf_matrix).collect();
    let pos = |m: &[[f32; 4]; 4]| [m[3][0], m[3][1], m[3][2]];
    // World direction each bone should point at: towards the child that
    // continues the chain (a neck points at the head, not between the
    // clavicles; a pelvis at the spine, not at a prop socket): the child
    // with the biggest subtree, skipping helpers (weighted, prop, socket,
    // ik, twist, roll, attach) and children of another side than the
    // parent (l_/r_ markers). A leaf keeps its parent's direction.
    let mut subtree = vec![1usize; n_bones];
    for i in (0..n_bones).rev() {
        if let Some(p) = g.bones[i].parent.filter(|&p| p < i) {
            subtree[p] += subtree[i];
        }
    }
    let side = |name: &str| -> Option<char> {
        let n = name.to_ascii_lowercase();
        let left = n.starts_with("l_") || n.ends_with("_l") || n.contains("_l_") || n.contains("left");
        let right = n.starts_with("r_") || n.ends_with("_r") || n.contains("_r_") || n.contains("right");
        if left && !right {
            Some('l')
        } else if right && !left {
            Some('r')
        } else {
            None
        }
    };
    let helper = |name: &str| -> bool {
        let n = name.to_ascii_lowercase();
        ["weighted", "prop", "socket", "ik", "twist", "roll", "attach", "fx"].iter().any(|w| n.contains(w))
    };
    let mut dirs: Vec<[f32; 3]> = vec![[0.0, 1.0, 0.0]; n_bones];
    for i in 0..n_bones {
        let p = pos(&globals[i]);
        let parent_side = side(&g.bones[i].name);
        let best = children[i]
            .iter()
            .copied()
            .filter(|&c| {
                let q = pos(&globals[c]);
                (q[0] - p[0]).hypot(q[1] - p[1]).hypot(q[2] - p[2]) > 1e-4
            })
            .max_by_key(|&c| {
                let name = &g.bones[c].name;
                let mut score = subtree[c] as i64;
                if helper(name) {
                    score -= 1_000_000;
                }
                if side(name).is_some() && side(name) != parent_side {
                    score -= 1_000;
                }
                score
            });
        dirs[i] = match best {
            Some(c) => {
                let q = pos(&globals[c]);
                let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
                let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                [d[0] / l, d[1] / l, d[2] / l]
            }
            None => g.bones[i].parent.filter(|&p| p < i).map(|p| dirs[p]).unwrap_or([0.0, 1.0, 0.0]),
        };
    }
    // Per-bone rotation (in the bone's frame) taking +Y to that direction.
    let fixes: Vec<[[f32; 4]; 4]> = (0..n_bones)
        .map(|i| {
            let m = &globals[i];
            let d = dirs[i];
            // Direction in the bone's own frame: Rᵀ d.
            let local = [m[0][0] * d[0] + m[0][1] * d[1] + m[0][2] * d[2], m[1][0] * d[0] + m[1][1] * d[1] + m[1][2] * d[2], m[2][0] * d[0] + m[2][1] * d[1] + m[2][2] * d[2]];
            rotation_y_to(local)
        })
        .collect();
    (children, fixes, globals)
}

pub fn write_glb(name: &str, g: &Geometry) -> Result<Vec<u8>> {
    write_glb_anims(name, g, &[])
}

/// A skinned GLB with animations: each frame's bone locals become the joint
/// nodes' translation and rotation channels (through the same joint fixes as
/// the bind pose, `F_parent⁻¹ · G(local) · F_self`).
pub fn write_glb_anims(name: &str, g: &Geometry, anims: &[GltfAnim]) -> Result<Vec<u8>> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut push_view = |bin: &mut Vec<u8>, bytes: &[u8], target: Option<u32>| -> usize {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let off = bin.len();
        bin.extend_from_slice(bytes);
        let t = target.map(|t| format!(",\"target\":{t}")).unwrap_or_default();
        views.push(format!("{{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{}{t}}}", bytes.len()));
        views.len() - 1
    };
    let n = g.positions.len();
    let mut pos_bytes = Vec::with_capacity(n * 12);
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in &g.positions {
        let q = to_gltf(*p);
        for k in 0..3 {
            min[k] = min[k].min(q[k]);
            max[k] = max[k].max(q[k]);
            pos_bytes.extend_from_slice(&q[k].to_le_bytes());
        }
    }
    let pv = push_view(&mut bin, &pos_bytes, Some(34962));
    accessors.push(format!("{{\"bufferView\":{pv},\"componentType\":5126,\"count\":{n},\"type\":\"VEC3\",\"min\":[{},{},{}],\"max\":[{},{},{}]}}", min[0], min[1], min[2], max[0], max[1], max[2]));
    let pos_acc = accessors.len() - 1;
    let mut nrm_bytes = Vec::with_capacity(n * 12);
    for nm in &g.normals {
        for c in to_gltf_dir(*nm) {
            nrm_bytes.extend_from_slice(&c.to_le_bytes());
        }
    }
    let nv = push_view(&mut bin, &nrm_bytes, Some(34962));
    accessors.push(format!("{{\"bufferView\":{nv},\"componentType\":5126,\"count\":{n},\"type\":\"VEC3\"}}"));
    let nrm_acc = accessors.len() - 1;
    let mut uv_bytes = Vec::with_capacity(n * 8);
    for uv in &g.uvs {
        uv_bytes.extend_from_slice(&uv[0].to_le_bytes());
        uv_bytes.extend_from_slice(&uv[1].to_le_bytes());
    }
    let uvv = push_view(&mut bin, &uv_bytes, Some(34962));
    accessors.push(format!("{{\"bufferView\":{uvv},\"componentType\":5126,\"count\":{n},\"type\":\"VEC2\"}}"));
    let uv_acc = accessors.len() - 1;

    // Skin attributes.
    let skinned = !g.bones.is_empty() && g.influences.len() == n;
    let mut skin_attrs = String::new();
    if skinned {
        let mut joints = Vec::with_capacity(n * 8);
        let mut weights = Vec::with_capacity(n * 16);
        for inf in &g.influences {
            let total: u32 = inf.iter().map(|(_, w)| *w as u32).sum();
            for (b, w) in inf {
                joints.extend_from_slice(&b.to_le_bytes());
                let wf = if total > 0 { *w as f32 / total as f32 } else { 0.0 };
                weights.extend_from_slice(&wf.to_le_bytes());
            }
        }
        let jv = push_view(&mut bin, &joints, Some(34962));
        accessors.push(format!("{{\"bufferView\":{jv},\"componentType\":5123,\"count\":{n},\"type\":\"VEC4\"}}"));
        let ja = accessors.len() - 1;
        let wv = push_view(&mut bin, &weights, Some(34962));
        accessors.push(format!("{{\"bufferView\":{wv},\"componentType\":5126,\"count\":{n},\"type\":\"VEC4\"}}"));
        let wa = accessors.len() - 1;
        skin_attrs = format!(",\"JOINTS_0\":{ja},\"WEIGHTS_0\":{wa}");
    }

    // Images, textures, materials.
    let mut images = Vec::new();
    let mut textures = Vec::new();
    for img in &g.images {
        let iv = push_view(&mut bin, &img.png, None);
        images.push(format!("{{\"name\":\"{}\",\"mimeType\":\"image/png\",\"bufferView\":{iv}}}", img.name.replace('"', "'")));
        textures.push(format!("{{\"sampler\":0,\"source\":{}}}", images.len() - 1));
    }
    let mut materials = Vec::new();
    let tex_ref = |i: usize| format!("{{\"index\":{i},\"texCoord\":0}}");
    for (si, sec) in g.sections.iter().enumerate() {
        let m = g.materials.get(si);
        let mname = m.map(|m| m.name.clone()).unwrap_or_else(|| sec.material_name.clone()).replace('"', "'");
        let mut pbr = String::from("\"metallicFactor\":0.0,\"roughnessFactor\":0.8");
        let mut rest = String::new();
        if let Some(m) = m {
            if let Some(b) = m.base_color {
                pbr.push_str(&format!(",\"baseColorTexture\":{}", tex_ref(b)));
            }
            if let Some(nm) = m.normal {
                rest.push_str(&format!(",\"normalTexture\":{}", tex_ref(nm)));
            }
            if let Some(e) = m.emissive {
                rest.push_str(&format!(",\"emissiveTexture\":{},\"emissiveFactor\":[1,1,1]", tex_ref(e)));
            }
            if !m.extra.is_empty() {
                let extras: Vec<String> = m.extra.iter().map(|(p, i)| format!("\"{}\":{}", p.replace('"', "'"), g.images.get(*i).map(|im| format!("\"{}\"", im.name.replace('"', "'"))).unwrap_or_else(|| "null".into()))).collect();
                rest.push_str(&format!(",\"extras\":{{\"textures\":{{{}}}}}", extras.join(",")));
            }
        }
        materials.push(format!("{{\"name\":\"{mname}\",\"pbrMetallicRoughness\":{{{pbr}}}{rest}}}"));
    }

    let mut prims = Vec::new();
    for sec in &g.sections {
        let start = sec.first_index as usize;
        let count = sec.num_triangles as usize * 3;
        let slice = g.indices.get(start..start + count).ok_or_else(|| Error::Format("section exceeds index buffer".into()))?;
        let mut idx = Vec::with_capacity(count * 4);
        for t in slice.chunks(3) {
            // Flip winding: the axis swap mirrors the mesh.
            for k in [t[0], t[2], t[1]] {
                idx.extend_from_slice(&k.to_le_bytes());
            }
        }
        let iv = push_view(&mut bin, &idx, Some(34963));
        accessors.push(format!("{{\"bufferView\":{iv},\"componentType\":5125,\"count\":{count},\"type\":\"SCALAR\"}}"));
        let ia = accessors.len() - 1;
        let mi = prims.len();
        prims.push(format!("{{\"attributes\":{{\"POSITION\":{pos_acc},\"NORMAL\":{nrm_acc},\"TEXCOORD_0\":{uv_acc}{skin_attrs}}},\"indices\":{ia},\"material\":{mi}}}"));
    }

    // Nodes: mesh node first, then one node per bone (joint i = node i + 1).
    let mut nodes = Vec::new();
    let mut animations = String::new();
    let mut skins = String::new();
    let mesh_node = if skinned { format!("{{\"mesh\":0,\"name\":\"{}\",\"skin\":0}}", name.replace('"', "'")) } else { format!("{{\"mesh\":0,\"name\":\"{}\"}}", name.replace('"', "'")) };
    nodes.push(mesh_node);
    let mut scene_nodes = vec![0usize];
    if skinned {
        // Unreal bones point their +X at the child; glTF viewers and Blender
        // draw a bone along its node's +Y. Each joint gets an extra rotation
        // so that +Y points at its (first) child, leaves keep the direction of
        // their parent, and the inverse bind matrices absorb it, so the skin
        // is unchanged while the skeleton reads as limbs rather than sticks.
        let n_bones = g.bones.len();
        let (children, fixes, globals) = skeleton_fixes(g);
        let mut ibm = Vec::with_capacity(n_bones * 64);
        for i in 0..n_bones {
            let inv = mat_inverse_rigid(&mat_mul(&globals[i], &fixes[i]));
            for col in inv {
                for x in col {
                    ibm.extend_from_slice(&x.to_le_bytes());
                }
            }
        }
        for (i, b) in g.bones.iter().enumerate() {
            let local = to_gltf_matrix(&b.local);
            let m = match b.parent {
                Some(p) if p < i => mat_mul(&mat_mul(&mat_inverse_rigid(&fixes[p]), &local), &fixes[i]),
                _ => mat_mul(&local, &fixes[i]),
            };
            let flat: Vec<String> = m.iter().flatten().map(|x| format!("{x}")).collect();
            let kids: Vec<String> = children[i].iter().map(|c| (c + 1).to_string()).collect();
            let kids = if kids.is_empty() { String::new() } else { format!(",\"children\":[{}]", kids.join(",")) };
            nodes.push(format!("{{\"name\":\"{}\",\"matrix\":[{}]{kids}}}", b.name.replace('"', "'"), flat.join(",")));
        }
        let iv = push_view(&mut bin, &ibm, None);
        accessors.push(format!("{{\"bufferView\":{iv},\"componentType\":5126,\"count\":{},\"type\":\"MAT4\"}}", g.bones.len()));
        let ibm_acc = accessors.len() - 1;
        // Animations: per bone, the node's translation and rotation per frame.
        let mut anim_json = Vec::new();
        for a in anims {
            if a.frames.is_empty() {
                continue;
            }
            let mut tb = Vec::with_capacity(a.times.len() * 4);
            for t in &a.times {
                tb.extend_from_slice(&t.to_le_bytes());
            }
            let tv = push_view(&mut bin, &tb, None);
            let (t0, t1) = (a.times.first().copied().unwrap_or(0.0), a.times.last().copied().unwrap_or(0.0));
            accessors.push(format!("{{\"bufferView\":{tv},\"componentType\":5126,\"count\":{},\"type\":\"SCALAR\",\"min\":[{t0}],\"max\":[{t1}]}}", a.times.len()));
            let time_acc = accessors.len() - 1;
            let mut samplers = Vec::new();
            let mut channels = Vec::new();
            for i in 0..n_bones {
                let (mut pos, mut rot) = (Vec::with_capacity(a.frames.len() * 12), Vec::with_capacity(a.frames.len() * 16));
                let mut prev: Option<[f32; 4]> = None;
                for f in &a.frames {
                    let local = to_gltf_matrix(&f[i]);
                    let m = match g.bones[i].parent {
                        Some(p) if p < i => mat_mul(&mat_mul(&mat_inverse_rigid(&fixes[p]), &local), &fixes[i]),
                        _ => mat_mul(&local, &fixes[i]),
                    };
                    for v in &m[3][..3] {
                        pos.extend_from_slice(&v.to_le_bytes());
                    }
                    let mut q = mat_to_quat(&m);
                    // Keep neighbouring keys on the same side (no spins between frames).
                    if let Some(p) = prev {
                        if p[0] * q[0] + p[1] * q[1] + p[2] * q[2] + p[3] * q[3] < 0.0 {
                            q = [-q[0], -q[1], -q[2], -q[3]];
                        }
                    }
                    prev = Some(q);
                    for c in q {
                        rot.extend_from_slice(&c.to_le_bytes());
                    }
                }
                let pv = push_view(&mut bin, &pos, None);
                accessors.push(format!("{{\"bufferView\":{pv},\"componentType\":5126,\"count\":{},\"type\":\"VEC3\"}}", a.frames.len()));
                samplers.push(format!("{{\"input\":{time_acc},\"output\":{},\"interpolation\":\"LINEAR\"}}", accessors.len() - 1));
                channels.push(format!("{{\"sampler\":{},\"target\":{{\"node\":{},\"path\":\"translation\"}}}}", samplers.len() - 1, i + 1));
                let rv = push_view(&mut bin, &rot, None);
                accessors.push(format!("{{\"bufferView\":{rv},\"componentType\":5126,\"count\":{},\"type\":\"VEC4\"}}", a.frames.len()));
                samplers.push(format!("{{\"input\":{time_acc},\"output\":{},\"interpolation\":\"LINEAR\"}}", accessors.len() - 1));
                channels.push(format!("{{\"sampler\":{},\"target\":{{\"node\":{},\"path\":\"rotation\"}}}}", samplers.len() - 1, i + 1));
            }
            anim_json.push(format!("{{\"name\":\"{}\",\"samplers\":[{}],\"channels\":[{}]}}", a.name.replace('"', "'"), samplers.join(","), channels.join(",")));
        }
        if !anim_json.is_empty() {
            animations = format!(",\"animations\":[{}]", anim_json.join(","));
        }
        let joints: Vec<String> = (0..g.bones.len()).map(|i| (i + 1).to_string()).collect();
        skins = format!(",\"skins\":[{{\"inverseBindMatrices\":{ibm_acc},\"skeleton\":1,\"joints\":[{}]}}]", joints.join(","));
        scene_nodes.push(1);
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let samplers = if images.is_empty() { String::new() } else { ",\"samplers\":[{\"magFilter\":9729,\"minFilter\":9987,\"wrapS\":10497,\"wrapT\":10497}]".to_string() };
    let img_json = if images.is_empty() { String::new() } else { format!(",\"images\":[{}],\"textures\":[{}]", images.join(","), textures.join(",")) };
    let scene_list: Vec<String> = scene_nodes.iter().map(|n| n.to_string()).collect();
    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"tempest-assets\"}},\"scene\":0,\"scenes\":[{{\"nodes\":[{}]}}],\"nodes\":[{}],\"meshes\":[{{\"name\":\"{}\",\"primitives\":[{}]}}],\"materials\":[{}]{}{}{}{},\"accessors\":[{}],\"bufferViews\":[{}],\"buffers\":[{{\"byteLength\":{}}}]}}",
        scene_list.join(","),
        nodes.join(","),
        name.replace('"', "'"),
        prims.join(","),
        materials.join(","),
        img_json,
        samplers,
        skins,
        animations,
        accessors.join(","),
        views.join(","),
        bin.len()
    );
    let mut json_bytes = json.into_bytes();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let mut out = Vec::with_capacity(12 + 8 + json_bytes.len() + 8 + bin.len());
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + 8 + json_bytes.len() + 8 + bin.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

/// Meshes placed in maps often carry no materials of their own: the cooked
/// map sets them on the components. For every section without a material,
/// take the first component of the package that uses `mesh` and overrides
/// that slot.
fn component_materials(g: &mut Geometry, cache: &mut crate::material_ops::PackageCache, pkg_name: &str, mesh: PackageIndex) {
    use crate::value::Value;
    if g.sections.iter().all(|s| !s.material.is_null()) {
        return;
    }
    let n = cache.get(pkg_name).map(|lp| lp.pkg.exports.len()).unwrap_or(0);
    for i in 0..n {
        let class = cache.class_name(pkg_name, i).unwrap_or_default();
        if !cache.db.class_is_a(&class, "MeshComponent") {
            continue;
        }
        let Some(obj) = cache.object(pkg_name, i) else { continue };
        let Some(lp) = cache.get(pkg_name) else { break };
        let pkg = &lp.pkg;
        let uses = ["StaticMesh", "SkeletalMesh"].iter().any(|p| matches!(obj.property(p, pkg).map(|p| &p.value), Some(Value::Object(m)) if *m == mesh));
        if !uses {
            continue;
        }
        let Some(Value::Array(mats)) = obj.property("Materials", pkg).map(|p| &p.value) else { continue };
        let mut done = true;
        for sec in g.sections.iter_mut() {
            if sec.material.is_null() {
                if let Some(Value::Object(m)) = mats.get(sec.slot) {
                    if !m.is_null() {
                        sec.material = *m;
                        sec.material_name = pkg.object_path(*m);
                    }
                }
            }
            done &= !sec.material.is_null();
        }
        if done {
            break;
        }
    }
}

/// Resolve the textures of every section's material into `g.materials` /
/// `g.images` (PNG, largest available mip, de-duplicated by texture path).
/// `max_size`: preview textures (largest mip within the bound, fast PNG);
/// `png_cache`: encoded PNGs by `"<path>@<size>"`, reused across calls.
pub fn attach_materials(
    g: &mut Geometry,
    cache: &mut crate::material_ops::PackageCache,
    src: &crate::texcache::TextureSource,
    pkg_name: &str,
    mesh: PackageIndex,
    max_size: Option<u32>,
    png_cache: &mut std::collections::HashMap<String, Vec<u8>>,
) -> Vec<String> {
    use crate::material_ops::{material_textures_ext, texture_png, TextureSlot};
    component_materials(g, cache, pkg_name, mesh);
    let mut warnings: Vec<String> = Vec::new();
    let mut image_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for sec in &g.sections.clone() {
        let mut m = GltfMaterial { name: sec.material_name.clone(), ..Default::default() };
        let (textures, unresolved) = material_textures_ext(cache, pkg_name, sec.material);
        for u in unresolved {
            let w = format!("{}: {u}: not found in the folder", sec.material_name);
            if !warnings.contains(&w) {
                warnings.push(w);
            }
        }
        for t in textures {
            // Only 2D textures have a place in a model file: cube maps
            // (reflections), render targets and the like are left out.
            if !cache.class_name(&t.package, t.export).is_some_and(|c| cache.db.class_is_a(&c, "Texture2D")) {
                continue;
            }
            let key = format!("{}@{}", t.path, max_size.unwrap_or(0));
            let idx = match image_index.get(&t.path) {
                Some(&i) => Some(i),
                None => match png_cache.get(&key).cloned().map(Ok).unwrap_or_else(|| texture_png(cache, src, &t.package, t.export, max_size)) {
                    Ok(png) => {
                        png_cache.insert(key, png.clone());
                        g.images.push(GltfImage { name: t.path.clone(), png });
                        image_index.insert(t.path.clone(), g.images.len() - 1);
                        Some(g.images.len() - 1)
                    }
                    Err(e) => {
                        let w = format!("{}: {e}", t.path);
                        if !warnings.contains(&w) {
                            warnings.push(w);
                        }
                        None
                    }
                },
            };
            let Some(idx) = idx else { continue };
            match t.slot {
                TextureSlot::BaseColor if m.base_color.is_none() => m.base_color = Some(idx),
                TextureSlot::Normal if m.normal.is_none() => m.normal = Some(idx),
                TextureSlot::Emissive if m.emissive.is_none() => m.emissive = Some(idx),
                _ => m.extra.push((t.parameter.clone(), idx)),
            }
        }
        // Hi-Rez's master material "emits" through the diffuse texture and a
        // mask: as a model file's emissive map that would just glow.
        if m.emissive.is_some() && m.emissive == m.base_color {
            m.emissive = None;
        }
        g.materials.push(m);
    }
    warnings
}
