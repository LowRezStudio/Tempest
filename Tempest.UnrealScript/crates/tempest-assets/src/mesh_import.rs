//! Replacing a static or skeletal mesh's geometry from a glTF (`.glb` /
//! `.gltf`) or Wavefront OBJ file: the inverse of `mesh_ops`.
//!
//! What gets rebuilt for the chosen LOD:
//! * static meshes: position / tangent+UV vertex buffers, 16-bit index
//!   buffer, elements (materials matched by name, else by order), bounds and
//!   the compact kDOP collision tree (`TkDOPTreeCompact::Build`, UnkDOP.h);
//!   the simple collision in the `RB_BodySetup` is PhysX cooked data and is
//!   left as it was.
//! * skeletal meshes: the GPU skin vertex buffer (`TGPUSkinVertexFloat16Uvs`
//!   layout), index container, chunks (≤ 75 bones each, like the engine's
//!   chunker), sections, active/required bones and bounds; the skeleton is
//!   kept, so the file's joints must be named like the mesh's bones.
//!
//! Coordinates follow the exporter: glTF `(x, y, z)` metres ↔ engine
//! `(x, z, y)` centimetres, triangle winding flipped.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value as J;
use tempest_upk::io::{Error, Result};
use tempest_upk::Package;

use crate::bulk::BulkData;
use crate::mesh::{BoxSphereBounds, BulkArray, KDopTree, MultiSizeIndexContainer, SkelMeshChunk, SkelMeshSection, SkeletalMesh, StaticMesh, StaticMeshElement};
use crate::object::{Native, Object};

const MAX_GPUSKIN_BONES: usize = 75;
const MAX_TRIS_PER_LEAF_COMPACT: usize = 5;

// ----------------------------------------------------------------------------
// Intermediate form
// ----------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct ImportedVertex {
    /// Engine units and axes.
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uvs: Vec<[f32; 2]>,
    /// Skeleton bone indices (resolved by name) and weights summing to 1.
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct ImportedSection {
    pub material: String,
    pub vertices: Vec<ImportedVertex>,
    /// Engine winding, triangle list.
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct ImportedMesh {
    pub sections: Vec<ImportedSection>,
    /// Joint names in the file's skin (index = joint index used by `joints`).
    pub joint_names: Vec<String>,
    pub skinned: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    pub vertices: usize,
    pub triangles: usize,
    pub sections: usize,
    pub chunks: usize,
    pub warnings: Vec<String>,
}

fn to_engine(p: [f32; 3]) -> [f32; 3] {
    [p[0] * 100.0, p[2] * 100.0, p[1] * 100.0]
}

fn to_engine_dir(p: [f32; 3]) -> [f32; 3] {
    normalize([p[0], p[2], p[1]])
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-8 { [v[0] / len, v[1] / len, v[2] / len] } else { [0.0, 0.0, 1.0] }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Parse a mesh file by extension.
pub fn parse_file(path: &Path, bytes: &[u8]) -> Result<ImportedMesh> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "obj" => parse_obj(std::str::from_utf8(bytes).map_err(|_| Error::Format("OBJ is not UTF-8".into()))?),
        "glb" | "gltf" => parse_gltf(path, bytes),
        "fbx" => crate::mesh_fbx::parse_fbx(bytes),
        _ => Err(Error::Format(format!("unsupported mesh format .{ext} (use .glb, .gltf, .fbx or .obj)"))),
    }
}

// ----------------------------------------------------------------------------
// OBJ
// ----------------------------------------------------------------------------

fn parse_obj(text: &str) -> Result<ImportedMesh> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut mesh = ImportedMesh::default();
    let mut current: Option<ImportedSection> = None;
    let mut dedup: HashMap<(usize, usize, usize), u32> = HashMap::new();
    let parse_f = |s: &str| s.parse::<f32>().map_err(|_| Error::Format(format!("bad number {s} in OBJ")));
    let resolve = |i: i64, n: usize| -> usize {
        if i < 0 { (n as i64 + i) as usize } else { (i - 1) as usize }
    };
    for line in text.lines() {
        let line = line.trim();
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let v: Vec<f32> = it.take(3).map(parse_f).collect::<Result<_>>()?;
                if v.len() < 3 {
                    return Err(Error::Format("short vertex line in OBJ".into()));
                }
                positions.push(to_engine([v[0], v[1], v[2]]));
            }
            Some("vn") => {
                let v: Vec<f32> = it.take(3).map(parse_f).collect::<Result<_>>()?;
                if v.len() < 3 {
                    return Err(Error::Format("short normal line in OBJ".into()));
                }
                normals.push(to_engine_dir([v[0], v[1], v[2]]));
            }
            Some("vt") => {
                let v: Vec<f32> = it.take(2).map(parse_f).collect::<Result<_>>()?;
                if v.len() < 2 {
                    return Err(Error::Format("short uv line in OBJ".into()));
                }
                // OBJ v runs bottom-up; the exporter wrote 1 - v.
                uvs.push([v[0], 1.0 - v[1]]);
            }
            Some("usemtl") | Some("g") | Some("o") => {
                let name = it.collect::<Vec<_>>().join(" ");
                let is_mtl = line.starts_with("usemtl");
                let start_new = match &current {
                    None => true,
                    Some(c) => is_mtl || c.indices.is_empty(),
                };
                if start_new {
                    if let Some(c) = current.take() {
                        if !c.indices.is_empty() {
                            mesh.sections.push(c);
                        }
                    }
                    current = Some(ImportedSection { material: name, ..Default::default() });
                    dedup.clear();
                } else if let Some(c) = &mut current {
                    if c.material.is_empty() {
                        c.material = name;
                    }
                }
            }
            Some("f") => {
                let sec = current.get_or_insert_with(ImportedSection::default);
                let mut corners: Vec<u32> = Vec::new();
                for tok in it {
                    let mut parts = tok.split('/');
                    let pi: i64 = parts.next().unwrap_or("0").parse().map_err(|_| Error::Format(format!("bad face token {tok}")))?;
                    let ti: Option<i64> = parts.next().filter(|s| !s.is_empty()).and_then(|s| s.parse().ok());
                    let ni: Option<i64> = parts.next().filter(|s| !s.is_empty()).and_then(|s| s.parse().ok());
                    let p = resolve(pi, positions.len());
                    let t = ti.map(|t| resolve(t, uvs.len()) + 1).unwrap_or(0);
                    let n = ni.map(|n| resolve(n, normals.len()) + 1).unwrap_or(0);
                    let key = (p, t, n);
                    let idx = match dedup.get(&key) {
                        Some(i) => *i,
                        None => {
                            let v = ImportedVertex {
                                position: *positions.get(p).ok_or_else(|| Error::Format("face references a missing vertex".into()))?,
                                normal: if n > 0 { normals[n - 1] } else { [0.0, 0.0, 0.0] },
                                uvs: vec![if t > 0 { uvs[t - 1] } else { [0.0, 0.0] }],
                                ..Default::default()
                            };
                            sec.vertices.push(v);
                            let i = (sec.vertices.len() - 1) as u32;
                            dedup.insert(key, i);
                            i
                        }
                    };
                    corners.push(idx);
                }
                // Fan-triangulate; flip winding to the engine's.
                for k in 1..corners.len().saturating_sub(1) {
                    sec.indices.extend_from_slice(&[corners[0], corners[k + 1], corners[k]]);
                }
            }
            _ => {}
        }
    }
    if let Some(c) = current.take() {
        if !c.indices.is_empty() {
            mesh.sections.push(c);
        }
    }
    if mesh.sections.is_empty() {
        return Err(Error::Format("no faces in OBJ".into()));
    }
    // Missing normals: face normals accumulated per vertex.
    for sec in &mut mesh.sections {
        if sec.vertices.iter().any(|v| v.normal == [0.0, 0.0, 0.0]) {
            compute_normals(sec);
        }
    }
    Ok(mesh)
}

pub(crate) fn compute_normals(sec: &mut ImportedSection) {
    let mut acc = vec![[0f32; 3]; sec.vertices.len()];
    for t in sec.indices.chunks(3) {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let n = cross(sub(sec.vertices[b].position, sec.vertices[a].position), sub(sec.vertices[c].position, sec.vertices[a].position));
        for i in [a, b, c] {
            for k in 0..3 {
                acc[i][k] += n[k];
            }
        }
    }
    for (v, n) in sec.vertices.iter_mut().zip(acc) {
        if v.normal == [0.0, 0.0, 0.0] {
            v.normal = normalize(n);
        }
    }
}

// ----------------------------------------------------------------------------
// glTF
// ----------------------------------------------------------------------------

struct Gltf {
    json: J,
    buffers: Vec<Vec<u8>>,
}

impl Gltf {
    fn accessor(&self, index: usize) -> Result<Vec<[f32; 4]>> {
        let acc = self.json["accessors"].get(index).ok_or_else(|| Error::Format(format!("glTF: no accessor {index}")))?;
        if acc.get("sparse").is_some() {
            return Err(Error::Format("glTF: sparse accessors are not supported".into()));
        }
        let count = acc["count"].as_u64().unwrap_or(0) as usize;
        let comp = acc["componentType"].as_u64().unwrap_or(0);
        let normalized = acc["normalized"].as_bool().unwrap_or(false);
        let ncomp = match acc["type"].as_str().unwrap_or("") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            t => return Err(Error::Format(format!("glTF: accessor type {t} is not supported"))),
        };
        let comp_size = match comp {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            c => return Err(Error::Format(format!("glTF: component type {c}"))),
        };
        let Some(bv_index) = acc["bufferView"].as_u64() else {
            return Ok(vec![[0.0; 4]; count]);
        };
        let bv = self.json["bufferViews"].get(bv_index as usize).ok_or_else(|| Error::Format("glTF: bad bufferView".into()))?;
        let buffer = self.buffers.get(bv["buffer"].as_u64().unwrap_or(0) as usize).ok_or_else(|| Error::Format("glTF: missing buffer".into()))?;
        let base = bv["byteOffset"].as_u64().unwrap_or(0) as usize + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = bv["byteStride"].as_u64().map(|s| s as usize).unwrap_or(comp_size * ncomp);
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let mut v = [0f32; 4];
            for (c, slot) in v.iter_mut().enumerate().take(ncomp) {
                let off = base + i * stride + c * comp_size;
                let b = buffer.get(off..off + comp_size).ok_or_else(|| Error::Format("glTF: accessor runs past its buffer".into()))?;
                *slot = match comp {
                    5120 => {
                        let x = b[0] as i8 as f32;
                        if normalized { (x / 127.0).max(-1.0) } else { x }
                    }
                    5121 => {
                        let x = b[0] as f32;
                        if normalized { x / 255.0 } else { x }
                    }
                    5122 => {
                        let x = i16::from_le_bytes([b[0], b[1]]) as f32;
                        if normalized { (x / 32767.0).max(-1.0) } else { x }
                    }
                    5123 => {
                        let x = u16::from_le_bytes([b[0], b[1]]) as f32;
                        if normalized { x / 65535.0 } else { x }
                    }
                    5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
                    _ => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                };
            }
            out.push(v);
        }
        Ok(out)
    }

    fn node_local(&self, node: &J) -> [[f32; 4]; 4] {
        if let Some(m) = node["matrix"].as_array() {
            let v: Vec<f32> = m.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
            if v.len() == 16 {
                let mut out = [[0f32; 4]; 4];
                for (c, col) in out.iter_mut().enumerate() {
                    col.copy_from_slice(&v[c * 4..c * 4 + 4]);
                }
                return out;
            }
        }
        let vec = |k: &str, d: [f32; 4]| -> [f32; 4] {
            match node[k].as_array() {
                Some(a) => {
                    let mut o = d;
                    for (i, x) in a.iter().enumerate().take(4) {
                        o[i] = x.as_f64().unwrap_or(0.0) as f32;
                    }
                    o
                }
                None => d,
            }
        };
        let t = vec("translation", [0.0, 0.0, 0.0, 0.0]);
        let q = vec("rotation", [0.0, 0.0, 0.0, 1.0]);
        let s = vec("scale", [1.0, 1.0, 1.0, 1.0]);
        let mut m = crate::mesh_ops::quat_translation_matrix([q[0], q[1], q[2], q[3]], [t[0], t[1], t[2]]);
        for (c, col) in m.iter_mut().enumerate().take(3) {
            for x in col.iter_mut().take(3) {
                *x *= s[c];
            }
        }
        m
    }
}

fn transform_point(m: &[[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    let mut o = [0f32; 3];
    for (r, slot) in o.iter_mut().enumerate() {
        *slot = m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r];
    }
    o
}

fn transform_dir(m: &[[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    let mut o = [0f32; 3];
    for (r, slot) in o.iter_mut().enumerate() {
        *slot = m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2];
    }
    o
}

fn parse_gltf(path: &Path, bytes: &[u8]) -> Result<ImportedMesh> {
    let (json, bin) = if bytes.len() >= 12 && &bytes[0..4] == b"glTF" {
        let mut pos = 12;
        let mut json: Option<J> = None;
        let mut bin: Option<Vec<u8>> = None;
        while pos + 8 <= bytes.len() {
            let len = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
            let kind = u32::from_le_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]]);
            let data = bytes.get(pos + 8..pos + 8 + len).ok_or_else(|| Error::Format("GLB: truncated chunk".into()))?;
            match kind {
                0x4E4F534A => json = Some(serde_json::from_slice(data).map_err(|e| Error::Format(format!("GLB: bad JSON: {e}")))?),
                0x004E4942 => bin = Some(data.to_vec()),
                _ => {}
            }
            pos += 8 + len.div_ceil(4) * 4;
        }
        (json.ok_or_else(|| Error::Format("GLB: no JSON chunk".into()))?, bin)
    } else {
        (serde_json::from_slice(bytes).map_err(|e| Error::Format(format!("glTF: bad JSON: {e}")))?, None)
    };
    // Buffers: the GLB BIN chunk, data URIs, or files next to the .gltf.
    let mut buffers = Vec::new();
    if let Some(list) = json["buffers"].as_array() {
        for (i, b) in list.iter().enumerate() {
            match b["uri"].as_str() {
                None => buffers.push(if i == 0 { bin.clone().unwrap_or_default() } else { Vec::new() }),
                Some(uri) if uri.starts_with("data:") => {
                    let b64 = uri.split_once(',').map(|x| x.1).unwrap_or("");
                    buffers.push(base64_decode(b64));
                }
                Some(uri) => {
                    let p = path.parent().unwrap_or(Path::new(".")).join(uri.replace("%20", " "));
                    buffers.push(std::fs::read(&p).map_err(|e| Error::Format(format!("glTF: cannot read buffer {}: {e}", p.display())))?);
                }
            }
        }
    }
    let g = Gltf { json, buffers };
    let mut mesh = ImportedMesh::default();
    // Joint names of the first skin.
    let mut skin_joint_nodes: Vec<usize> = Vec::new();
    if let Some(skin) = g.json["skins"].as_array().and_then(|s| s.first()) {
        for j in skin["joints"].as_array().unwrap_or(&Vec::new()) {
            skin_joint_nodes.push(j.as_u64().unwrap_or(0) as usize);
        }
        mesh.joint_names = skin_joint_nodes.iter().map(|&n| g.json["nodes"][n]["name"].as_str().unwrap_or("").to_string()).collect();
    }
    // World transform of every node (depth-first from the roots).
    let nodes = g.json["nodes"].as_array().cloned().unwrap_or_default();
    let mut world: Vec<Option<[[f32; 4]; 4]>> = vec![None; nodes.len()];
    let mut is_child = vec![false; nodes.len()];
    for n in &nodes {
        for c in n["children"].as_array().unwrap_or(&Vec::new()) {
            if let Some(ci) = c.as_u64() {
                if (ci as usize) < nodes.len() {
                    is_child[ci as usize] = true;
                }
            }
        }
    }
    let identity = crate::mesh_ops::quat_translation_matrix([0.0, 0.0, 0.0, 1.0], [0.0; 3]);
    let mut stack: Vec<(usize, [[f32; 4]; 4])> = (0..nodes.len()).filter(|&i| !is_child[i]).map(|i| (i, identity)).collect();
    while let Some((i, parent)) = stack.pop() {
        let m = crate::mesh_ops::mat_mul(&parent, &g.node_local(&nodes[i]));
        world[i] = Some(m);
        for c in nodes[i]["children"].as_array().unwrap_or(&Vec::new()) {
            if let Some(ci) = c.as_u64() {
                if (ci as usize) < nodes.len() && world[ci as usize].is_none() {
                    stack.push((ci as usize, m));
                }
            }
        }
    }
    let materials = g.json["materials"].as_array().cloned().unwrap_or_default();
    let mut used_meshes = false;
    for (ni, node) in nodes.iter().enumerate() {
        let Some(mi) = node["mesh"].as_u64() else { continue };
        let skinned = node.get("skin").is_some();
        let m = world[ni].unwrap_or(identity);
        let Some(prims) = g.json["meshes"][mi as usize]["primitives"].as_array() else { continue };
        used_meshes = true;
        for prim in prims {
            if prim["mode"].as_u64().unwrap_or(4) != 4 {
                return Err(Error::Format("glTF: only triangle-list primitives are supported".into()));
            }
            let attrs = &prim["attributes"];
            let pos = g.accessor(attrs["POSITION"].as_u64().ok_or_else(|| Error::Format("glTF: primitive without POSITION".into()))? as usize)?;
            let nrm = match attrs["NORMAL"].as_u64() {
                Some(a) => Some(g.accessor(a as usize)?),
                None => None,
            };
            let mut uv_sets: Vec<Vec<[f32; 4]>> = Vec::new();
            for k in 0..4 {
                match attrs[format!("TEXCOORD_{k}")].as_u64() {
                    Some(a) => uv_sets.push(g.accessor(a as usize)?),
                    None => break,
                }
            }
            let joints = match (skinned, attrs["JOINTS_0"].as_u64()) {
                (true, Some(a)) => Some(g.accessor(a as usize)?),
                _ => None,
            };
            let weights = match (skinned, attrs["WEIGHTS_0"].as_u64()) {
                (true, Some(a)) => Some(g.accessor(a as usize)?),
                _ => None,
            };
            let n = pos.len();
            let mut sec = ImportedSection { material: prim["material"].as_u64().and_then(|mi| materials.get(mi as usize)).and_then(|m| m["name"].as_str()).unwrap_or("").to_string(), ..Default::default() };
            for i in 0..n {
                let mut p = [pos[i][0], pos[i][1], pos[i][2]];
                let mut nv = nrm.as_ref().map(|a| [a[i][0], a[i][1], a[i][2]]).unwrap_or([0.0; 3]);
                if !skinned {
                    p = transform_point(&m, p);
                    nv = transform_dir(&m, nv);
                }
                let mut v = ImportedVertex { position: to_engine(p), normal: if nv == [0.0; 3] { nv } else { to_engine_dir(nv) }, ..Default::default() };
                for set in &uv_sets {
                    v.uvs.push([set[i][0], set[i][1]]);
                }
                if v.uvs.is_empty() {
                    v.uvs.push([0.0, 0.0]);
                }
                if let (Some(j), Some(w)) = (&joints, &weights) {
                    for k in 0..4 {
                        v.joints[k] = j[i][k] as u16;
                        v.weights[k] = w[i][k];
                    }
                }
                sec.vertices.push(v);
            }
            let raw: Vec<u32> = match prim["indices"].as_u64() {
                Some(a) => g.accessor(a as usize)?.iter().map(|x| x[0] as u32).collect(),
                None => (0..n as u32).collect(),
            };
            for t in raw.chunks(3) {
                if t.len() == 3 {
                    sec.indices.extend_from_slice(&[t[0], t[2], t[1]]);
                }
            }
            if sec.indices.iter().any(|&i| i as usize >= n) {
                return Err(Error::Format("glTF: index out of range".into()));
            }
            if nrm.is_none() {
                compute_normals(&mut sec);
            }
            if joints.is_some() {
                mesh.skinned = true;
            }
            mesh.sections.push(sec);
        }
    }
    if !used_meshes || mesh.sections.is_empty() {
        return Err(Error::Format("glTF: no mesh primitives".into()));
    }
    Ok(mesh)
}

fn base64_decode(text: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 26,
            b'0'..=b'9' => (c - b'0') as u32 + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let bytes: Vec<u8> = text.bytes().filter(|b| *b != b'=' && !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (k, c) in chunk.iter().enumerate() {
            n |= val(*c).unwrap_or(0) << (18 - 6 * k);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    out
}

// ----------------------------------------------------------------------------
// Vertex packing
// ----------------------------------------------------------------------------

/// `FPackedNormal`: `Trunc(x * 127.5 + 127.5)` per component.
fn pack_normal(v: [f32; 3], w: u8) -> u32 {
    let c = |x: f32| ((x * 127.5 + 127.5).trunc().clamp(0.0, 255.0)) as u32;
    c(v[0]) | c(v[1]) << 8 | c(v[2]) << 16 | (w as u32) << 24
}

pub fn f32_to_half(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mant != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = (mant | 0x80_0000) >> (1 - e);
        // Round to nearest.
        let rounded = (m + 0x1000) >> 13;
        return sign | rounded as u16;
    }
    let half = ((e as u32) << 10) | (mant >> 13);
    // Round to nearest even on the dropped bits.
    let round = mant & 0x1fff;
    let half = if round > 0x1000 || (round == 0x1000 && (half & 1) == 1) { half + 1 } else { half };
    sign | half as u16
}

/// Tangent frame per vertex from the UVs: (TangentX, TangentZ with the
/// basis sign in W).
fn tangents(vertices: &[ImportedVertex], indices: &[u32]) -> Vec<(u32, u32)> {
    let n = vertices.len();
    let mut tan = vec![[0f32; 3]; n];
    let mut bit = vec![[0f32; 3]; n];
    for t in indices.chunks(3) {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let (p0, p1, p2) = (vertices[a].position, vertices[b].position, vertices[c].position);
        let uv = |i: usize| vertices[i].uvs.first().copied().unwrap_or([0.0, 0.0]);
        let (w0, w1, w2) = (uv(a), uv(b), uv(c));
        let e1 = sub(p1, p0);
        let e2 = sub(p2, p0);
        let (x1, x2) = (w1[0] - w0[0], w2[0] - w0[0]);
        let (y1, y2) = (w1[1] - w0[1], w2[1] - w0[1]);
        let det = x1 * y2 - x2 * y1;
        let r = if det.abs() > 1e-12 { 1.0 / det } else { 0.0 };
        let sdir = [(y2 * e1[0] - y1 * e2[0]) * r, (y2 * e1[1] - y1 * e2[1]) * r, (y2 * e1[2] - y1 * e2[2]) * r];
        let tdir = [(x1 * e2[0] - x2 * e1[0]) * r, (x1 * e2[1] - x2 * e1[1]) * r, (x1 * e2[2] - x2 * e1[2]) * r];
        for i in [a, b, c] {
            for k in 0..3 {
                tan[i][k] += sdir[k];
                bit[i][k] += tdir[k];
            }
        }
    }
    (0..n)
        .map(|i| {
            let nrm = normalize(vertices[i].normal);
            let mut t = tan[i];
            // Gram-Schmidt against the normal; any perpendicular when degenerate.
            let d = dot(nrm, t);
            t = normalize([t[0] - nrm[0] * d, t[1] - nrm[1] * d, t[2] - nrm[2] * d]);
            if t == [0.0, 0.0, 1.0] && nrm[2].abs() > 0.9 {
                t = [1.0, 0.0, 0.0];
            }
            let handed = if dot(cross(nrm, t), bit[i]) < 0.0 { 0u8 } else { 255u8 };
            (pack_normal(t, 128), pack_normal(nrm, handed))
        })
        .collect()
}

fn bounds_of<'a>(points: impl Iterator<Item = &'a [f32; 3]>) -> BoxSphereBounds {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut pts: Vec<[f32; 3]> = Vec::new();
    for p in points {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
        pts.push(*p);
    }
    if pts.is_empty() {
        return BoxSphereBounds::default();
    }
    let origin = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, (min[2] + max[2]) * 0.5];
    let extent = [(max[0] - min[0]) * 0.5, (max[1] - min[1]) * 0.5, (max[2] - min[2]) * 0.5];
    let mut radius = 0f32;
    for p in &pts {
        let d = sub(*p, origin);
        radius = radius.max(dot(d, d).sqrt());
    }
    BoxSphereBounds { origin, box_extent: extent, sphere_radius: radius }
}

/// Skeletal mesh bounds (`UnMeshEd.cpp`): the vertex box doubled around its
/// centre, the bottom tucked up by 10% (the reference pose rarely extends
/// lower than the animations), sphere radius = box extent length.
fn skeletal_bounds<'a>(points: impl Iterator<Item = &'a [f32; 3]>) -> BoxSphereBounds {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut any = false;
    for p in points {
        any = true;
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    if !any {
        return BoxSphereBounds::default();
    }
    let mid = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, (min[2] + max[2]) * 0.5];
    let mut bmin = [0f32; 3];
    let mut bmax = [0f32; 3];
    for k in 0..3 {
        bmin[k] = min[k] + (min[k] - mid[k]);
        bmax[k] = max[k] + (max[k] - mid[k]);
    }
    bmin[2] = min[2] + 0.1 * (min[2] - mid[2]);
    let origin = [(bmin[0] + bmax[0]) * 0.5, (bmin[1] + bmax[1]) * 0.5, (bmin[2] + bmax[2]) * 0.5];
    let extent = [(bmax[0] - bmin[0]) * 0.5, (bmax[1] - bmin[1]) * 0.5, (bmax[2] - bmin[2]) * 0.5];
    BoxSphereBounds { origin, box_extent: extent, sphere_radius: dot(extent, extent).sqrt() }
}

// ----------------------------------------------------------------------------
// Compact kDOP tree (TkDOPTreeCompact::Build, UnkDOP.h)
// ----------------------------------------------------------------------------

struct BuildTri {
    v: [u16; 3],
    material: u16,
    centroid: [f32; 3],
    p: [[f32; 3]; 3],
}

#[derive(Clone, Copy)]
struct Bound {
    min: [f32; 3],
    max: [f32; 3],
}

impl Bound {
    fn empty() -> Bound {
        Bound { min: [f32::MAX; 3], max: [-f32::MAX; 3] }
    }
    fn add_triangles(tris: &[BuildTri]) -> Bound {
        let mut b = Bound::empty();
        for t in tris {
            for p in &t.p {
                for (k, x) in p.iter().enumerate() {
                    b.min[k] = b.min[k].min(*x);
                    b.max[k] = b.max[k].max(*x);
                }
            }
        }
        b
    }
}

fn compress_axis(target: f32, other: f32, left: f32, right: f32) -> u8 {
    let range = other - target;
    if range == 0.0 {
        return 1;
    }
    let lfrac = (left - target) / range;
    let rfrac = (right - target) / range;
    let result: i32 = if lfrac >= rfrac { 128 + ((127.0 * lfrac).floor() as i32 - 1).max(0) } else { 127 - ((127.0 * rfrac).floor() as i32 - 1).max(0) };
    result.clamp(1, 254) as u8
}

fn decompress_axis(c: u8, target: f32, other: f32) -> (f32, f32) {
    let signed = c as f32 - 127.5;
    let lfrac = signed.max(0.5).floor() / 127.0;
    let rfrac = (-signed).max(0.5).floor() / 127.0;
    (target + lfrac * (other - target), target + rfrac * (other - target))
}

fn find_best_plane(tris: &[BuildTri]) -> usize {
    let mut best = 0;
    let mut best_var = 0f32;
    for plane in 0..3 {
        let mut mean = 0f32;
        for t in tris {
            mean += t.centroid[plane];
        }
        mean /= tris.len() as f32;
        let mut var = 0f32;
        for t in tris {
            let d = t.centroid[plane] - mean;
            var += d * d;
        }
        var /= tris.len() as f32;
        if var >= best_var {
            best = plane;
            best_var = var;
        }
    }
    best
}

fn partition(tris: &mut [BuildTri], start: usize, end: usize, pivot: usize, plane: usize) -> usize {
    let pivot_dot = tris[pivot].centroid[plane];
    tris.swap(pivot, end);
    let mut write = start;
    for i in start..end {
        if tris[i].centroid[plane] <= pivot_dot {
            tris.swap(i, write);
            write += 1;
        }
    }
    tris.swap(end, write);
    write
}

fn split_triangle_list(tris: &mut [BuildTri], start: usize, num: usize, bound: Bound, nodes: &mut [[u8; 6]], node_index: usize, real_nodes: usize) {
    let num_right = num / 2;
    let num_left = num - num_right;
    let first_right = start + num_left;
    let plane = find_best_plane(&tris[start..start + num]);
    engine_partial_sort(tris, start, start + num - 1, first_right, plane);
    let left = Bound::add_triangles(&tris[start..first_right]);
    let right = Bound::add_triangles(&tris[first_right..start + num]);
    let mut node = [0u8; 6];
    for k in 0..3 {
        node[k] = compress_axis(bound.min[k], bound.max[k], left.min[k], right.min[k]);
        node[3 + k] = compress_axis(bound.max[k], bound.min[k], left.max[k], right.max[k]);
    }
    nodes[node_index] = node;
    // Decompress so children inherit the same lossy bounds.
    let mut new_left = Bound::empty();
    let mut new_right = Bound::empty();
    for k in 0..3 {
        let (l, r) = decompress_axis(node[k], bound.min[k], bound.max[k]);
        new_left.min[k] = l;
        new_right.min[k] = r;
        let (l, r) = decompress_axis(node[3 + k], bound.max[k], bound.min[k]);
        new_left.max[k] = l;
        new_right.max[k] = r;
    }
    let left_index = 2 * node_index + 1;
    let right_index = left_index + 1;
    if left_index < real_nodes {
        split_triangle_list(tris, start, num_left, new_left, nodes, left_index, real_nodes);
        split_triangle_list(tris, first_right, num_right, new_right, nodes, right_index, real_nodes);
    }
}

/// `TkDOPNodeCompact::PartialSort`, verbatim (including which side the
/// engine keeps narrowing; the node bounds are computed from the real
/// ranges afterwards, so the tree is valid either way).
fn engine_partial_sort(tris: &mut [BuildTri], mut start: usize, mut end: usize, split: usize, plane: usize) {
    while start < end {
        let p = partition(tris, start, end, (start + end) / 2, plane);
        if p < split {
            if p == 0 {
                break;
            }
            end = p - 1;
        } else {
            start = p + 1;
        }
    }
}

/// (positions, indices, element index) per section.
type KdopSection = (Vec<[f32; 3]>, Vec<u32>, u32);

fn build_kdop(sections: &[KdopSection], base_vertex: &[u32]) -> KDopTree {
    let mut tris: Vec<BuildTri> = Vec::new();
    for (si, (positions, indices, material)) in sections.iter().enumerate() {
        for t in indices.chunks(3) {
            let p = [positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]];
            let centroid = [(p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0, (p[0][2] + p[1][2] + p[2][2]) / 3.0];
            tris.push(BuildTri { v: [(base_vertex[si] + t[0]) as u16, (base_vertex[si] + t[1]) as u16, (base_vertex[si] + t[2]) as u16], material: *material as u16, centroid, p });
        }
    }
    let n = tris.len();
    let root = Bound::add_triangles(&tris);
    let mut num_nodes = 0usize;
    if n > MAX_TRIS_PER_LEAF_COMPACT {
        let mut leaves = 1usize;
        while n.div_ceil(leaves) > MAX_TRIS_PER_LEAF_COMPACT * 2 {
            leaves *= 2;
        }
        num_nodes = 2 * leaves - 1;
    }
    let mut nodes = vec![[0u8; 6]; num_nodes + 1];
    if num_nodes > 0 && n > 1 {
        split_triangle_list(&mut tris, 0, n, root, &mut nodes, 0, num_nodes);
    }
    let mut node_bytes = Vec::with_capacity(nodes.len() * 6);
    for nd in &nodes {
        node_bytes.extend_from_slice(nd);
    }
    let mut tri_bytes = Vec::with_capacity(n * 8);
    for t in &tris {
        for v in t.v {
            tri_bytes.extend_from_slice(&v.to_le_bytes());
        }
        tri_bytes.extend_from_slice(&t.material.to_le_bytes());
    }
    if n == 0 {
        return KDopTree { root_min: [f32::MAX; 3], root_max: [-f32::MAX; 3], nodes: BulkArray::from_elems(6, Vec::new()), triangles: BulkArray::from_elems(8, Vec::new()) };
    }
    KDopTree { root_min: root.min, root_max: root.max, nodes: BulkArray::from_elems(6, node_bytes), triangles: BulkArray::from_elems(8, tri_bytes) }
}

// ----------------------------------------------------------------------------
// Static mesh
// ----------------------------------------------------------------------------

/// Match imported section materials to the mesh's existing slots by name
/// (object path or object name), else by position.
fn match_material(name: &str, existing: &[String], fallback: usize) -> usize {
    let n = name.trim();
    if !n.is_empty() {
        if let Some(i) = existing.iter().position(|e| e.eq_ignore_ascii_case(n) || e.rsplit('.').next().map(|leaf| leaf.eq_ignore_ascii_case(n)).unwrap_or(false)) {
            return i;
        }
    }
    fallback.min(existing.len().saturating_sub(1))
}

fn import_static(pkg: &Package, m: &mut StaticMesh, lod: usize, src: &ImportedMesh) -> Result<ImportReport> {
    let Some(l) = m.lod_models.get_mut(lod) else { return Err(Error::Format(format!("no LOD {lod}"))) };
    let mut report = ImportReport::default();
    let total_verts: usize = src.sections.iter().map(|s| s.vertices.len()).sum();
    if total_verts > u16::MAX as usize {
        return Err(Error::Format(format!("{total_verts} vertices: static meshes use 16-bit indices (65535 max); split the mesh or reduce it")));
    }
    let num_uv = l.vertex_buffer.num_tex_coords.max(1) as usize;
    let full_uvs = l.vertex_buffer.use_full_precision_uvs != 0;
    let existing: Vec<String> = l.elements.iter().map(|e| pkg.object_path(e.material)).collect();
    let template = l.elements.clone();
    let mut positions = Vec::with_capacity(total_verts * 12);
    let mut verts = Vec::with_capacity(total_verts * (8 + num_uv * if full_uvs { 8 } else { 4 }));
    let mut indices: Vec<u8> = Vec::new();
    let mut elements = Vec::new();
    let mut kdop_sections = Vec::new();
    let mut base_vertex = Vec::new();
    let mut base = 0u32;
    for (si, sec) in src.sections.iter().enumerate() {
        let slot = match_material(&sec.material, &existing, si);
        let tpl = template.get(slot).or(template.last()).cloned().unwrap_or_default();
        let frames = tangents(&sec.vertices, &sec.indices);
        for (v, (tx, tz)) in sec.vertices.iter().zip(frames) {
            for c in v.position {
                positions.extend_from_slice(&c.to_le_bytes());
            }
            verts.extend_from_slice(&tx.to_le_bytes());
            verts.extend_from_slice(&tz.to_le_bytes());
            for k in 0..num_uv {
                let uv = v.uvs.get(k).or(v.uvs.first()).copied().unwrap_or([0.0, 0.0]);
                if full_uvs {
                    verts.extend_from_slice(&uv[0].to_le_bytes());
                    verts.extend_from_slice(&uv[1].to_le_bytes());
                } else {
                    verts.extend_from_slice(&f32_to_half(uv[0]).to_le_bytes());
                    verts.extend_from_slice(&f32_to_half(uv[1]).to_le_bytes());
                }
            }
        }
        let first_index = (indices.len() / 2) as u32;
        for i in &sec.indices {
            indices.extend_from_slice(&((base + i) as u16).to_le_bytes());
        }
        let ntris = (sec.indices.len() / 3) as u32;
        let (min_v, max_v) = if sec.vertices.is_empty() { (base, base) } else { (base, base + sec.vertices.len() as u32 - 1) };
        elements.push(StaticMeshElement {
            material: tpl.material,
            enable_collision: tpl.enable_collision,
            old_enable_collision: tpl.old_enable_collision,
            enable_shadow_casting: tpl.enable_shadow_casting,
            first_index,
            num_triangles: ntris,
            min_vertex_index: min_v,
            max_vertex_index: max_v,
            material_index: elements.len() as i32,
            fragments: vec![(0, ntris as i32)],
            platform_data: 0,
        });
        if sec.material.trim().is_empty() || !existing.iter().any(|e| e.eq_ignore_ascii_case(sec.material.trim()) || e.rsplit('.').next().map(|leaf| leaf.eq_ignore_ascii_case(sec.material.trim())).unwrap_or(false)) {
            report.warnings.push(format!("section {si} ({}) uses material slot {slot}: {}", if sec.material.is_empty() { "unnamed" } else { sec.material.as_str() }, existing.get(slot).cloned().unwrap_or_default()));
        }
        kdop_sections.push((sec.vertices.iter().map(|v| v.position).collect::<Vec<_>>(), sec.indices.clone(), elements.len() as u32 - 1));
        base_vertex.push(base);
        base += sec.vertices.len() as u32;
        report.triangles += ntris as usize;
    }
    report.vertices = total_verts;
    report.sections = elements.len();
    l.elements = elements;
    l.position_vb.stride = 12;
    l.position_vb.num_vertices = total_verts as i32;
    l.position_vb.data = BulkArray::from_elems(12, positions);
    let stride = 8 + num_uv * if full_uvs { 8 } else { 4 };
    l.vertex_buffer.num_tex_coords = num_uv as u32;
    l.vertex_buffer.stride = stride as u32;
    l.vertex_buffer.num_vertices = total_verts as u32;
    l.vertex_buffer.data = Some(BulkArray::from_elems(stride, verts));
    l.color_vb.num_vertices = 0;
    l.color_vb.data = None;
    l.num_vertices = total_verts as u32;
    l.index_buffer = BulkArray::from_elems(2, indices);
    l.wireframe_index_buffer = BulkArray { elem_size: l.wireframe_index_buffer.elem_size.max(2), count: 0, data: Vec::new() };
    l.adjacency_index_buffer = BulkArray { elem_size: l.adjacency_index_buffer.elem_size.max(2), count: 0, data: Vec::new() };
    l.raw_triangles = BulkData { flags: 0, element_count: 0, size_on_disk: 0, offset_in_file: -1, inline: None };
    if lod == 0 {
        m.bounds = bounds_of(src.sections.iter().flat_map(|s| s.vertices.iter().map(|v| &v.position)));
        m.kdop_tree = build_kdop(&kdop_sections, &base_vertex);
        if !m.body_setup.is_null() {
            report.warnings.push("simple collision (RB_BodySetup) kept from the old mesh; per-triangle collision was rebuilt".into());
        }
    }
    m.source_data = None;
    Ok(report)
}

// ----------------------------------------------------------------------------
// Skeletal mesh
// ----------------------------------------------------------------------------

fn import_skeletal(pkg: &Package, m: &mut SkeletalMesh, lod: usize, src: &ImportedMesh) -> Result<ImportReport> {
    if !src.skinned {
        return Err(Error::Format("the file has no skin (JOINTS_0 / WEIGHTS_0); a skeletal mesh needs weighted vertices".into()));
    }
    let bone_names: Vec<String> = m.ref_skeleton.iter().map(|b| pkg.fname_to_string(b.name)).collect();
    let mut joint_to_bone: Vec<Option<u16>> = Vec::new();
    let mut missing = Vec::new();
    for j in &src.joint_names {
        match bone_names.iter().position(|b| b.eq_ignore_ascii_case(j)) {
            Some(i) => joint_to_bone.push(Some(i as u16)),
            None => {
                joint_to_bone.push(None);
                missing.push(j.clone());
            }
        }
    }
    if m.ref_skeleton.len() > 256 {
        return Err(Error::Format("skeletons with more than 256 bones are not supported".into()));
    }
    let Some(l) = m.lod_models.get_mut(lod) else { return Err(Error::Format(format!("no LOD {lod}"))) };
    let mut report = ImportReport::default();
    if !missing.is_empty() {
        report.warnings.push(format!("{} joints are not bones of this skeleton and were dropped from the weights: {}", missing.len(), missing.iter().take(8).cloned().collect::<Vec<_>>().join(", ")));
    }
    let num_uv = l.vertex_buffer.num_tex_coords.max(1) as usize;
    let full_uvs = l.vertex_buffer.use_full_precision_uvs != 0;
    let existing: Vec<String> = m.materials.iter().map(|i| pkg.object_path(*i)).collect();
    let stride = 16 + 12 + num_uv * if full_uvs { 8 } else { 4 };
    let mut vb: Vec<u8> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut sections = Vec::new();
    let mut chunks = Vec::new();
    let mut used_bones: Vec<bool> = vec![false; m.ref_skeleton.len()];
    let mut all_positions: Vec<[f32; 3]> = Vec::new();
    let mut vertex_count = 0u32;
    for (si, sec) in src.sections.iter().enumerate() {
        let slot = match_material(&sec.material, &existing, si) as u16;
        let frames = tangents(&sec.vertices, &sec.indices);
        // Resolve influences to skeleton bones, normalised to 255.
        let mut infl: Vec<[(u16, u8); 4]> = Vec::with_capacity(sec.vertices.len());
        for v in &sec.vertices {
            let mut pairs: Vec<(u16, f32)> = Vec::new();
            for k in 0..4 {
                if v.weights[k] <= 0.0 {
                    continue;
                }
                if let Some(Some(b)) = joint_to_bone.get(v.joints[k] as usize) {
                    pairs.push((*b, v.weights[k]));
                }
            }
            if pairs.is_empty() {
                pairs.push((0, 1.0));
            }
            pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            pairs.truncate(4);
            let total: f32 = pairs.iter().map(|p| p.1).sum();
            let mut out = [(0u16, 0u8); 4];
            let mut acc = 0u32;
            for (k, (b, w)) in pairs.iter().enumerate() {
                let byte = if k == pairs.len() - 1 { (255 - acc.min(255)) as u8 } else { ((w / total) * 255.0).round().clamp(0.0, 255.0) as u8 };
                acc += byte as u32;
                out[k] = (*b, byte);
                used_bones[*b as usize] = true;
            }
            infl.push(out);
        }
        // Chunk the section's triangles so each chunk maps ≤ 75 bones.
        let mut tri = 0usize;
        let tris = sec.indices.len() / 3;
        while tri < tris {
            let mut bone_map: Vec<u16> = Vec::new();
            let mut local: HashMap<u32, u32> = HashMap::new();
            let mut chunk_indices: Vec<u32> = Vec::new();
            let base_vertex = vertex_count;
            let mut chunk_verts: Vec<u32> = Vec::new();
            let mut max_infl = 1;
            while tri < tris {
                let t = &sec.indices[tri * 3..tri * 3 + 3];
                let mut needed: Vec<u16> = Vec::new();
                for &vi in t {
                    for (b, w) in infl[vi as usize] {
                        if w > 0 && !bone_map.contains(&b) && !needed.contains(&b) {
                            needed.push(b);
                        }
                    }
                }
                if bone_map.len() + needed.len() > MAX_GPUSKIN_BONES {
                    if bone_map.is_empty() {
                        return Err(Error::Format("a single triangle references more than 75 bones".into()));
                    }
                    break;
                }
                bone_map.extend(needed);
                for &vi in t {
                    let li = match local.get(&vi) {
                        Some(x) => *x,
                        None => {
                            let x = chunk_verts.len() as u32;
                            local.insert(vi, x);
                            chunk_verts.push(vi);
                            x
                        }
                    };
                    chunk_indices.push(base_vertex + li);
                }
                tri += 1;
            }
            // Emit the chunk's vertices.
            for &vi in &chunk_verts {
                let v = &sec.vertices[vi as usize];
                let (tx, tz) = frames[vi as usize];
                vb.extend_from_slice(&tx.to_le_bytes());
                vb.extend_from_slice(&tz.to_le_bytes());
                let mut bones = [0u8; 4];
                let mut weights = [0u8; 4];
                let mut n = 0;
                for (k, (b, w)) in infl[vi as usize].iter().enumerate() {
                    bones[k] = bone_map.iter().position(|x| x == b).unwrap_or(0) as u8;
                    weights[k] = *w;
                    if *w > 0 {
                        n = k + 1;
                    }
                }
                max_infl = max_infl.max(n);
                vb.extend_from_slice(&bones);
                vb.extend_from_slice(&weights);
                for c in v.position {
                    vb.extend_from_slice(&c.to_le_bytes());
                }
                for k in 0..num_uv {
                    let uv = v.uvs.get(k).or(v.uvs.first()).copied().unwrap_or([0.0, 0.0]);
                    if full_uvs {
                        vb.extend_from_slice(&uv[0].to_le_bytes());
                        vb.extend_from_slice(&uv[1].to_le_bytes());
                    } else {
                        vb.extend_from_slice(&f32_to_half(uv[0]).to_le_bytes());
                        vb.extend_from_slice(&f32_to_half(uv[1]).to_le_bytes());
                    }
                }
                all_positions.push(v.position);
            }
            vertex_count += chunk_verts.len() as u32;
            let base_index = indices.len() as u32;
            let ntris = (chunk_indices.len() / 3) as u32;
            indices.extend_from_slice(&chunk_indices);
            sections.push(SkelMeshSection { material_index: slot, chunk_index: chunks.len() as u16, base_index, num_triangles: ntris, triangle_sorting: 0 });
            chunks.push(SkelMeshChunk { base_vertex_index: base_vertex, rigid_vertices: Vec::new(), soft_vertices: Vec::new(), bone_map, num_rigid_vertices: 0, num_soft_vertices: chunk_verts.len() as i32, max_bone_influences: max_infl as i32 });
            report.triangles += ntris as usize;
        }
        if sec.material.trim().is_empty() || !existing.iter().any(|e| e.eq_ignore_ascii_case(sec.material.trim()) || e.rsplit('.').next().map(|leaf| leaf.eq_ignore_ascii_case(sec.material.trim())).unwrap_or(false)) {
            report.warnings.push(format!("section {si} ({}) uses material slot {slot}: {}", if sec.material.is_empty() { "unnamed" } else { sec.material.as_str() }, existing.get(slot as usize).cloned().unwrap_or_default()));
        }
    }
    report.vertices = vertex_count as usize;
    report.sections = sections.len();
    report.chunks = chunks.len();
    // Bones: active = used; required = used plus every ancestor, ascending.
    let mut active: Vec<u16> = used_bones.iter().enumerate().filter(|(_, u)| **u).map(|(i, _)| i as u16).collect();
    active.sort_unstable();
    let mut required = vec![false; m.ref_skeleton.len()];
    for &b in &active {
        let mut cur = b as usize;
        loop {
            required[cur] = true;
            let parent = m.ref_skeleton[cur].parent_index;
            if cur == 0 || parent < 0 || parent as usize == cur {
                break;
            }
            cur = parent as usize;
        }
    }
    required[0] = true;
    let wide = vertex_count > u16::MAX as u32;
    let mut index_bytes = Vec::with_capacity(indices.len() * if wide { 4 } else { 2 });
    for i in &indices {
        if wide {
            index_bytes.extend_from_slice(&i.to_le_bytes());
        } else {
            index_bytes.extend_from_slice(&(*i as u16).to_le_bytes());
        }
    }
    l.sections = sections;
    l.index_container = MultiSizeIndexContainer { needs_cpu_access: l.index_container.needs_cpu_access, data_type_size: if wide { 4 } else { 2 }, indices: BulkArray::from_elems(if wide { 4 } else { 2 }, index_bytes) };
    l.active_bone_indices = active;
    l.chunks = chunks;
    l.num_vertices = vertex_count as i32;
    l.required_bones = required.iter().enumerate().filter(|(_, r)| **r).map(|(i, _)| i as u8).collect();
    l.raw_point_indices = BulkData { flags: 0, element_count: 0, size_on_disk: 0, offset_in_file: -1, inline: None };
    l.num_tex_coords = num_uv as i32;
    l.vertex_buffer.num_tex_coords = num_uv as u32;
    l.vertex_buffer.data = Some(BulkArray::from_elems(stride, vb));
    if let Some(c) = &mut l.color_vb {
        // White vertex colours when the mesh declares a colour stream.
        *c = Some(BulkArray::from_elems(4, vec![255u8; vertex_count as usize * 4]));
    }
    l.vertex_influences.clear();
    l.adjacency_index_container = MultiSizeIndexContainer { needs_cpu_access: l.adjacency_index_container.needs_cpu_access, data_type_size: l.index_container.data_type_size, indices: BulkArray::from_elems(l.index_container.data_type_size as usize, Vec::new()) };
    if lod == 0 {
        m.bounds = skeletal_bounds(all_positions.iter());
        if !m.per_poly_bone_kdops.is_empty() {
            m.per_poly_bone_kdops.clear();
            report.warnings.push("per-bone collision (per-poly kDOPs) removed: it referenced the old geometry".into());
        }
    }
    m.source_data = None;
    Ok(report)
}

/// Replace LOD `lod` of a parsed mesh object with the file's geometry.
pub fn import(pkg: &Package, obj: &mut Object, file: &Path, bytes: &[u8], lod: usize) -> Result<ImportReport> {
    let src = parse_file(file, bytes)?;
    match &mut obj.native {
        Native::StaticMesh(m) => import_static(pkg, m, lod, &src),
        Native::SkeletalMesh(m) => import_skeletal(pkg, m, lod, &src),
        _ => Err(Error::Format("not a decoded static or skeletal mesh".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_round_trips() {
        for x in [0.0f32, 1.0, -1.0, 0.5, 0.25, 0.1, 100.0, 1e-5, 65504.0] {
            let h = f32_to_half(x);
            let back = crate::dds::half_to_f32(h);
            assert!((back - x).abs() <= x.abs() * 1e-3 + 1e-6, "{x} -> {h:#x} -> {back}");
        }
    }

    #[test]
    fn kdop_node_count_matches_engine() {
        // 68 triangles → 8 leaves → 15 nodes + 1 pad (the shield mesh in ANDROXUS_ASSETS_SF).
        let positions: Vec<[f32; 3]> = (0..204).map(|i| [i as f32, (i * 7 % 13) as f32, (i * 3 % 5) as f32]).collect();
        let indices: Vec<u32> = (0..204).collect();
        let t = build_kdop(&[(positions, indices, 0)], &[0]);
        assert_eq!(t.nodes.count, 16);
        assert_eq!(t.triangles.count, 68);
        assert_eq!(t.nodes.elem_size, 6);
        assert_eq!(t.triangles.elem_size, 8);
    }

    #[test]
    fn obj_parses_quads_and_groups() {
        let obj = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nusemtl mat_a\nf 1/1 2/2 3/3 4/4\n";
        let m = parse_obj(obj).unwrap();
        assert_eq!(m.sections.len(), 1);
        assert_eq!(m.sections[0].material, "mat_a");
        assert_eq!(m.sections[0].vertices.len(), 4);
        assert_eq!(m.sections[0].indices.len(), 6);
        assert_eq!(m.sections[0].vertices[1].position, [100.0, 0.0, 0.0]);
    }
}
