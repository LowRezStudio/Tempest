//! Animations on a skeleton: poses for previews and exports, and imports
//! from glTF / FBX files back into compressed tracks ([`crate::anim`]).
//!
//! Tracks are bound to a skeletal mesh's bones by name through the owning
//! `UAnimSet`'s `TrackBoneNames`; bones without a track keep their reference
//! pose (and with `bAnimRotationOnly`, translations come from the reference
//! pose except for `UseTranslationBoneNames`).
//!
//! Imports work in model space so that a file whose bones were re-oriented
//! (Blender's glTF importer changes bone directions) still maps onto the
//! game's skeleton: for every bone found in the file, the change of its
//! world transform from the file's bind pose is applied to the mesh's bind
//! pose, then the result is turned back into parent-relative transforms.

use std::collections::HashMap;

use tempest_upk::io::{Error, Result};

use crate::anim::{self, Anim, Track};
use crate::mesh_ops::{mat_inverse_rigid, mat_mul, mat_to_quat, quat_translation_matrix, Bone, GltfAnim};

pub type Mat = [[f32; 4]; 4];

/// How an AnimSet's tracks attach to bones.
#[derive(Clone, Debug, Default)]
pub struct Binding {
    pub track_bones: Vec<String>,
    pub rotation_only: bool,
    pub use_translation: Vec<String>,
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// Bone index per track (`None` when the mesh has no bone of that name).
pub fn track_to_bone(binding: &Binding, bones: &[Bone]) -> Vec<Option<usize>> {
    let by_name: HashMap<String, usize> = bones.iter().enumerate().map(|(i, b)| (lower(&b.name), i)).collect();
    binding.track_bones.iter().map(|n| by_name.get(&lower(n)).copied()).collect()
}

/// Every bone's local transform (Unreal space) at frame `f`.
pub fn pose(anim: &Anim, binding: &Binding, bones: &[Bone], map: &[Option<usize>], f: f32) -> Vec<Mat> {
    let mut locals: Vec<Mat> = bones.iter().map(|b| b.local).collect();
    let use_trans: Vec<String> = binding.use_translation.iter().map(|n| lower(n)).collect();
    for (t, track) in anim.tracks.iter().enumerate() {
        let Some(Some(b)) = map.get(t) else { continue };
        let reference = bones[*b].local;
        let (r, p) = track.sample(f);
        let rot = r.map(|q| mesh_quat(q, *b)).unwrap_or_else(|| mat_to_quat(&reference));
        let keep_ref_trans = binding.rotation_only && !use_trans.contains(&lower(&bones[*b].name));
        let trans = match (p, keep_ref_trans) {
            (Some(p), false) => p,
            _ => [reference[3][0], reference[3][1], reference[3][2]],
        };
        locals[*b] = quat_translation_matrix(rot, trans);
    }
    locals
}

/// A sequence's rotation in the mesh's convention, and back (its own inverse):
/// the engine stores every bone's reference rotation but the root's conjugated
/// relative to the animation keys.
fn mesh_quat(q: [f32; 4], bone: usize) -> [f32; 4] {
    if bone == 0 { q } else { [-q[0], -q[1], -q[2], q[3]] }
}

/// The frame times of a sequence (frame `k` at `k · length / (frames - 1)`).
pub fn frame_times(anim: &Anim) -> Vec<f32> {
    let n = anim.num_frames.max(1);
    let step = if n > 1 { anim.length / (n - 1) as f32 } else { 0.0 };
    (0..n).map(|k| k as f32 * step).collect()
}

/// A sequence as a glTF animation of the mesh's joints.
pub fn gltf_anim(name: &str, anim: &Anim, binding: &Binding, bones: &[Bone]) -> GltfAnim {
    let map = track_to_bone(binding, bones);
    let times = frame_times(anim);
    let frames = (0..times.len()).map(|k| pose(anim, binding, bones, &map, k as f32)).collect();
    GltfAnim { name: name.to_string(), times, frames }
}

/// World (model space) bind matrices of a skeleton.
pub fn bind_globals(bones: &[Bone]) -> Vec<Mat> {
    let mut out: Vec<Mat> = Vec::with_capacity(bones.len());
    for (i, b) in bones.iter().enumerate() {
        let g = match b.parent {
            Some(p) if p < i => mat_mul(&out[p], &b.local),
            _ => b.local,
        };
        out.push(g);
    }
    out
}

/// General 4x4 inverse (column-major); identity when singular.
pub fn mat_inverse(m: &Mat) -> Mat {
    let a: Vec<f64> = (0..16).map(|i| m[i / 4][i % 4] as f64).collect();
    // a[c*4 + r]: column-major. Gauss-Jordan on rows of the row-major form.
    let mut r = [[0f64; 8]; 4];
    for (row, rr) in r.iter_mut().enumerate() {
        for col in 0..4 {
            rr[col] = a[col * 4 + row];
        }
        rr[4 + row] = 1.0;
    }
    for c in 0..4 {
        let p = (c..4).max_by(|&x, &y| r[x][c].abs().partial_cmp(&r[y][c].abs()).unwrap()).unwrap();
        if r[p][c].abs() < 1e-12 {
            return std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1.0 } else { 0.0 }));
        }
        r.swap(c, p);
        let d = r[c][c];
        for x in r[c].iter_mut() {
            *x /= d;
        }
        for row in 0..4 {
            if row != c {
                let f = r[row][c];
                if f != 0.0 {
                    let pivot = r[c];
                    for (x, p) in r[row].iter_mut().zip(pivot) {
                        *x -= f * p;
                    }
                }
            }
        }
    }
    let mut out = [[0f32; 4]; 4];
    for (col, oc) in out.iter_mut().enumerate() {
        for (row, x) in oc.iter_mut().enumerate() {
            *x = r[row][4 + col] as f32;
        }
    }
    out
}

/// Re-express a glTF-space transform in Unreal space (inverse of
/// [`crate::mesh_ops::to_gltf_matrix`]: y/z swapped, metres → centimetres).
pub fn from_gltf_matrix(m: &Mat) -> Mat {
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
        out[3][r] = m[3][p(r)] * 100.0;
    }
    out[3][3] = 1.0;
    out
}

/// Remove scale and shear from a transform's rotation part (Gram-Schmidt).
fn orthonormalize(m: &Mat) -> Mat {
    let norm = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let x = norm([m[0][0], m[0][1], m[0][2]]);
    let y0 = [m[1][0], m[1][1], m[1][2]];
    let d = dot(x, y0);
    let y = norm([y0[0] - d * x[0], y0[1] - d * x[1], y0[2] - d * x[2]]);
    let z = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
    [[x[0], x[1], x[2], 0.0], [y[0], y[1], y[2], 0.0], [z[0], z[1], z[2], 0.0], [m[3][0], m[3][1], m[3][2], 1.0]]
}

/// An animation read from a file: the frame times sampled and, per frame,
/// the world transform of every joint found (Unreal space), plus each
/// joint's world bind transform in the file (Unreal space).
pub struct FileAnim {
    pub name: String,
    pub duration: f32,
    /// Joint name (lower case) → index into `bind` / frame columns.
    pub joints: HashMap<String, usize>,
    pub bind: Vec<Mat>,
    /// `frames[k][j]`: world transform of joint `j` at frame `k`.
    pub frames: Vec<Vec<Mat>>,
}

/// The mesh's bone locals per frame from a file animation (see the module
/// notes), and the names of the bones the file did not have.
pub fn retarget(file: &FileAnim, bones: &[Bone]) -> (Vec<Vec<Mat>>, Vec<String>) {
    let bind = bind_globals(bones);
    let found: Vec<Option<usize>> = bones.iter().map(|b| file.joints.get(&lower(&b.name)).copied()).collect();
    let missing = bones.iter().zip(&found).filter(|(_, f)| f.is_none()).map(|(b, _)| b.name.clone()).collect();
    let mut out = Vec::with_capacity(file.frames.len());
    for frame in &file.frames {
        let mut world: Vec<Mat> = Vec::with_capacity(bones.len());
        let mut locals: Vec<Mat> = Vec::with_capacity(bones.len());
        for (i, b) in bones.iter().enumerate() {
            let parent = b.parent.filter(|&p| p < i);
            let w = match found[i] {
                Some(j) => {
                    let delta = mat_mul(&frame[j], &mat_inverse(&file.bind[j]));
                    orthonormalize(&mat_mul(&delta, &bind[i]))
                }
                None => match parent {
                    Some(p) => mat_mul(&world[p], &b.local),
                    None => b.local,
                },
            };
            let local = match parent {
                Some(p) => orthonormalize(&mat_mul(&mat_inverse_rigid(&world[p]), &w)),
                None => w,
            };
            world.push(w);
            locals.push(local);
        }
        out.push(locals);
    }
    (out, missing)
}

/// Tracks (in `binding`'s order) from per-frame bone locals. A track whose
/// bone the mesh lacks keeps `previous`'s track (time-scaled to the new frame
/// count) when there is one.
pub fn tracks_from_locals(frames: &[Vec<Mat>], binding: &Binding, bones: &[Bone], previous: Option<&Anim>) -> Vec<Track> {
    let map = track_to_bone(binding, bones);
    let n = frames.len();
    map.iter()
        .enumerate()
        .map(|(t, b)| match b {
            Some(b) => {
                let rot: Vec<[f32; 4]> = frames.iter().map(|f| anim::normalize(mesh_quat(mat_to_quat(&f[*b]), *b))).collect();
                let trans: Vec<[f32; 3]> = frames.iter().map(|f| [f[*b][3][0], f[*b][3][1], f[*b][3][2]]).collect();
                anim::reduce(&rot, &trans, anim::ROT_TOLERANCE, anim::TRANS_TOLERANCE)
            }
            None => match previous.and_then(|p| p.tracks.get(t).map(|tr| (p.num_frames, tr))) {
                Some((old_n, tr)) => {
                    let s = if old_n > 1 && n > 1 { (n - 1) as f32 / (old_n - 1) as f32 } else { 1.0 };
                    let mut tr = tr.clone();
                    tr.rot_frames.iter_mut().for_each(|f| *f = (*f * s).round());
                    tr.trans_frames.iter_mut().for_each(|f| *f = (*f * s).round());
                    tr
                }
                None => Track { rot: vec![[0.0, 0.0, 0.0, 1.0]], rot_frames: vec![0.0], trans: vec![[0.0; 3]], trans_frames: vec![0.0] },
            },
        })
        .collect()
}

// ----------------------------------------------------------------------------
// glTF
// ----------------------------------------------------------------------------

fn trs_of(node: &serde_json::Value) -> ([f32; 3], [f32; 4], [f32; 3]) {
    let arr = |k: &str, d: &[f32]| -> Vec<f32> { node[k].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_else(|| d.to_vec()) };
    let t = arr("translation", &[0.0, 0.0, 0.0]);
    let r = arr("rotation", &[0.0, 0.0, 0.0, 1.0]);
    let s = arr("scale", &[1.0, 1.0, 1.0]);
    ([t[0], t[1], t[2]], [r[0], r[1], r[2], r[3]], [s[0], s[1], s[2]])
}

fn compose(t: [f32; 3], r: [f32; 4], s: [f32; 3]) -> Mat {
    let mut m = quat_translation_matrix(r, t);
    for (c, col) in m.iter_mut().enumerate().take(3) {
        for x in col.iter_mut().take(3) {
            *x *= s[c];
        }
    }
    m
}

struct Channel {
    node: usize,
    path: String,
    times: Vec<f32>,
    values: Vec<[f32; 4]>,
    interp: String,
}

impl Channel {
    fn sample(&self, t: f32) -> [f32; 4] {
        let cubic = self.interp == "CUBICSPLINE";
        let value = |k: usize| if cubic { self.values[k * 3 + 1] } else { self.values[k] };
        if self.times.is_empty() {
            return [0.0, 0.0, 0.0, 1.0];
        }
        if t <= self.times[0] {
            return value(0);
        }
        let last = self.times.len() - 1;
        if t >= self.times[last] {
            return value(last);
        }
        let i = self.times.partition_point(|&x| x <= t).saturating_sub(1).min(last - 1);
        if self.interp == "STEP" {
            return value(i);
        }
        let s = (t - self.times[i]) / (self.times[i + 1] - self.times[i]).max(1e-9);
        let (a, b) = (value(i), value(i + 1));
        if self.path == "rotation" {
            anim::lerp_quat(a, b, s)
        } else {
            [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, a[2] + (b[2] - a[2]) * s, 0.0]
        }
    }
}

/// The names of a glTF file's animations.
pub fn gltf_animation_names(path: &std::path::Path, bytes: &[u8]) -> Result<Vec<String>> {
    let g = crate::mesh_import::load_gltf(path, bytes)?;
    Ok(g.json["animations"].as_array().map(|a| a.iter().enumerate().map(|(i, x)| x["name"].as_str().map(str::to_string).unwrap_or_else(|| format!("animation {i}"))).collect()).unwrap_or_default())
}

/// Read animation `which` (index, default the first) of a glTF file, sampled at `fps`.
pub fn read_gltf(path: &std::path::Path, bytes: &[u8], which: Option<usize>, fps: f32) -> Result<FileAnim> {
    let g = crate::mesh_import::load_gltf(path, bytes)?;
    let anims = g.json["animations"].as_array().cloned().unwrap_or_default();
    if anims.is_empty() {
        return Err(Error::Format("the file has no animation".into()));
    }
    let ai = which.unwrap_or(0);
    let a = anims.get(ai).ok_or_else(|| Error::Format(format!("the file has no animation {ai}")))?;
    let nodes = g.json["nodes"].as_array().cloned().unwrap_or_default();
    let mut parent: Vec<Option<usize>> = vec![None; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        for c in n["children"].as_array().unwrap_or(&Vec::new()) {
            if let Some(c) = c.as_u64() {
                if (c as usize) < parent.len() {
                    parent[c as usize] = Some(i);
                }
            }
        }
    }
    let samplers = a["samplers"].as_array().cloned().unwrap_or_default();
    let mut channels = Vec::new();
    let mut duration = 0f32;
    let mut start = f32::MAX;
    for c in a["channels"].as_array().unwrap_or(&Vec::new()) {
        let Some(node) = c["target"]["node"].as_u64().map(|v| v as usize) else { continue };
        let path_name = c["target"]["path"].as_str().unwrap_or("").to_string();
        if !["translation", "rotation", "scale"].contains(&path_name.as_str()) {
            continue;
        }
        let Some(s) = c["sampler"].as_u64().and_then(|i| samplers.get(i as usize)) else { continue };
        let times: Vec<f32> = g.accessor(s["input"].as_u64().unwrap_or(0) as usize)?.iter().map(|v| v[0]).collect();
        let values = g.accessor(s["output"].as_u64().unwrap_or(0) as usize)?;
        duration = duration.max(times.last().copied().unwrap_or(0.0));
        start = start.min(times.first().copied().unwrap_or(0.0));
        channels.push(Channel { node, path: path_name, times, values, interp: s["interpolation"].as_str().unwrap_or("LINEAR").to_string() });
    }
    // Joints: the skin's joints, else every named node.
    let skin = g.json["skins"].as_array().and_then(|s| s.first()).cloned();
    let joint_nodes: Vec<usize> = match &skin {
        Some(s) => s["joints"].as_array().map(|a| a.iter().filter_map(|x| x.as_u64().map(|v| v as usize)).collect()).unwrap_or_default(),
        None => (0..nodes.len()).filter(|&i| nodes[i]["name"].is_string()).collect(),
    };
    let rest: Vec<([f32; 3], [f32; 4], [f32; 3])> = nodes.iter().map(trs_of).collect();
    let world_at = |locals: &[Mat]| -> Vec<Mat> {
        let mut memo: Vec<Option<Mat>> = vec![None; nodes.len()];
        fn get(i: usize, parent: &[Option<usize>], locals: &[Mat], memo: &mut Vec<Option<Mat>>, depth: usize) -> Mat {
            if let Some(m) = memo[i] {
                return m;
            }
            let m = match parent[i] {
                Some(p) if depth < 512 => mat_mul(&get(p, parent, locals, memo, depth + 1), &locals[i]),
                _ => locals[i],
            };
            memo[i] = Some(m);
            m
        }
        (0..nodes.len()).map(|i| get(i, &parent, locals, &mut memo, 0)).collect()
    };
    let rest_locals: Vec<Mat> = nodes.iter().enumerate().map(|(i, n)| if n.get("matrix").is_some() { g.node_local(n) } else { compose(rest[i].0, rest[i].1, rest[i].2) }).collect();
    let rest_world = world_at(&rest_locals);
    // Bind pose: the skin's inverse bind matrices (in the space of the node
    // holding the skinned mesh), else the nodes' rest transforms.
    let bind_gltf: Vec<Mat> = match &skin {
        Some(s) if s.get("inverseBindMatrices").is_some() => {
            let ibms = read_mat4(&g, s["inverseBindMatrices"].as_u64().unwrap_or(0) as usize)?;
            let mesh_node = (0..nodes.len()).find(|&i| nodes[i].get("skin").is_some());
            let mesh_world = mesh_node.map(|m| rest_world[m]).unwrap_or(std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1.0 } else { 0.0 })));
            joint_nodes.iter().enumerate().map(|(k, &n)| ibms.get(k).map(|ibm| mat_mul(&mesh_world, &mat_inverse(ibm))).unwrap_or(rest_world[n])).collect()
        }
        _ => joint_nodes.iter().map(|&n| rest_world[n]).collect(),
    };
    // From the first key to the last.
    let start = if start.is_finite() { start } else { 0.0 };
    let duration = (duration - start).max(0.0);
    let fps = if fps > 0.0 { fps } else { 30.0 };
    let n_frames = ((duration * fps).round() as usize + 1).max(2);
    let mut frames = Vec::with_capacity(n_frames);
    for k in 0..n_frames {
        let t = start + (k as f32 / fps).min(duration);
        let mut trs = rest.clone();
        for c in &channels {
            let v = c.sample(t);
            match c.path.as_str() {
                "translation" => trs[c.node].0 = [v[0], v[1], v[2]],
                "rotation" => trs[c.node].1 = v,
                _ => trs[c.node].2 = [v[0], v[1], v[2]],
            }
        }
        let locals: Vec<Mat> = (0..nodes.len()).map(|i| if nodes[i].get("matrix").is_some() && !channels.iter().any(|c| c.node == i) { rest_locals[i] } else { compose(trs[i].0, trs[i].1, trs[i].2) }).collect();
        let world = world_at(&locals);
        frames.push(joint_nodes.iter().map(|&n| crate::anim_ops::from_gltf_matrix(&world[n])).collect());
    }
    let joints = joint_nodes.iter().enumerate().map(|(k, &n)| (lower(nodes[n]["name"].as_str().unwrap_or("")), k)).collect();
    Ok(FileAnim {
        name: a["name"].as_str().unwrap_or("animation").to_string(),
        duration,
        joints,
        bind: bind_gltf.iter().map(from_gltf_matrix).collect(),
        frames,
    })
}

fn read_mat4(g: &crate::mesh_import::Gltf, acc: usize) -> Result<Vec<Mat>> {
    let flat = g.accessor_floats(acc)?;
    Ok(flat.chunks_exact(16).map(|c| std::array::from_fn(|col| std::array::from_fn(|row| c[col * 4 + row]))).collect())
}

// ----------------------------------------------------------------------------
// Objects
// ----------------------------------------------------------------------------

/// An AnimSet's binding and its preview mesh name (`PreviewSkelMeshName`).
pub fn binding_of(pkg: &tempest_upk::Package, set: &crate::object::Object) -> (Binding, Option<String>) {
    use crate::value::Value;
    let names = |p: &str| -> Vec<String> {
        match set.property(p, pkg).map(|p| &p.value) {
            Some(Value::Array(a)) => a.iter().filter_map(|v| if let Value::Name(n) = v { Some(pkg.fname_to_string(*n)) } else { None }).collect(),
            _ => Vec::new(),
        }
    };
    let rotation_only = matches!(set.property("bAnimRotationOnly", pkg).map(|p| &p.value), Some(Value::Bool(true)));
    let preview = match set.property("PreviewSkelMeshName", pkg).map(|p| &p.value) {
        Some(Value::Name(n)) if !pkg.fname_to_string(*n).eq_ignore_ascii_case("None") => Some(pkg.fname_to_string(*n)),
        _ => None,
    };
    (Binding { track_bones: names("TrackBoneNames"), rotation_only, use_translation: names("UseTranslationBoneNames") }, preview)
}

/// Write `anim` into sequence export `i` of an open package: the header
/// properties, `CompressedTrackOffsets` and the compressed stream (as
/// `AKF_VariableKeyLerp` + `ACF_Float96NoW` + `ACF_None`).
pub fn write_sequence(ed: &mut crate::edit::Editor, i: usize, anim: &Anim) -> Result<()> {
    use crate::value::Value;
    let (stream, offsets) = anim::encode(anim);
    ed.set(i, "NumFrames", &anim.num_frames.to_string())?;
    ed.set(i, "SequenceLength", &format!("{}", anim.length))?;
    ed.set(i, "KeyEncodingFormat", "AKF_VariableKeyLerp")?;
    ed.set(i, "RotationCompressionFormat", "ACF_Float96NoW")?;
    let has = |ed: &mut crate::edit::Editor, name: &str| -> Result<bool> {
        let o = ed.object(i)?.clone();
        Ok(o.property(name, &ed.pkg).is_some())
    };
    if has(ed, "TranslationCompressionFormat")? {
        ed.set(i, "TranslationCompressionFormat", "ACF_None")?;
    }
    if !has(ed, "CompressedTrackOffsets")? {
        ed.set(i, "CompressedTrackOffsets[0]", "0")?;
    }
    let pkg = ed.pkg.clone();
    let mut obj = ed.object(i)?.clone();
    if let Some(p) = obj.properties.iter_mut().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case("CompressedTrackOffsets")) {
        p.value = Value::Array(offsets.iter().map(|&o| Value::Int(o)).collect());
    }
    match &mut obj.native {
        crate::object::Native::AnimSequence(a) => {
            a.compressed = stream;
        }
        _ => return Err(Error::Format("not an AnimSequence".into())),
    }
    ed.replace_object(i, obj)
}

/// The skeletal meshes of a package that `binding` can drive, best first:
/// the AnimSet's preview mesh, then by how many track bones they have.
pub fn mesh_candidates(db: &crate::reflect::ClassDb, pkg: &tempest_upk::Package, binding: &Binding, preview: Option<&str>) -> Vec<(usize, usize)> {
    use tempest_upk::io::PackageIndex;
    let layout = crate::object::package_layout(pkg);
    let wanted: std::collections::HashSet<String> = binding.track_bones.iter().map(|n| lower(n)).collect();
    let mut out: Vec<(usize, usize, bool)> = Vec::new();
    for i in 0..pkg.exports.len() {
        if !pkg.class_name_of(PackageIndex::export(i)).eq_ignore_ascii_case("SkeletalMesh") {
            continue;
        }
        let data = pkg.export_data.get(i).map(|d| d.as_slice()).unwrap_or(&[]);
        let crate::object::Parsed::Object(o) = crate::object::parse_export(db, pkg, layout, i, data) else { continue };
        let crate::object::Native::SkeletalMesh(m) = &o.native else { continue };
        let matched = m.ref_skeleton.iter().filter(|b| wanted.contains(&lower(&pkg.fname_to_string(b.name)))).count();
        if matched == 0 {
            continue;
        }
        let is_preview = preview.is_some_and(|p| pkg.object_path(PackageIndex::export(i)).eq_ignore_ascii_case(p));
        out.push((i, matched, is_preview));
    }
    out.sort_by_key(|&(_, m, p)| (std::cmp::Reverse(p), std::cmp::Reverse(m)));
    out.into_iter().map(|(i, m, _)| (i, m)).collect()
}
