//! Maps as a scene: what a level package places and where, for the Studio's
//! map view, and moving what it places.
//!
//! A map is a persistent level package (`TD_Abyss_P`) whose `WorldInfo`
//! streams sublevels (`StreamingLevels` → `LevelStreaming*.PackageName`,
//! one package each: art, collision, lighting, AI…). Every level package has
//! one `Level` export (`TheWorld.PersistentLevel`) listing its actors.
//! Transforms are ordinary properties: an actor's `Location`, `Rotation`,
//! `DrawScale`, `DrawScale3D`, `PrePivot`; a component's `Translation`,
//! `Rotation`, `Scale`, `Scale3D` relative to its owner (the cooker bakes
//! the placement of the meshes it gathers into `StaticMeshCollectionActor`s
//! into their components, the actor sitting at the origin). Engine:
//! `AActor::LocalToWorld`, `UActorComponent::SetParentToWorld`,
//! `FRotationMatrix` (UnMath.h), `FMatrix::Rotator`.
//!
//! Coordinates sent to the Studio are in the mesh previews' glTF space
//! (`(x, z, y) / 100`, see `mesh_ops`): a matrix `M` (Unreal, row vectors)
//! becomes `P⁻¹ M P`, written row-major, which is three.js' column-major
//! `Matrix4.fromArray` order for column vectors.

use std::collections::HashMap;

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, PackageIndex, Result};
use tempest_upk::Package;

use crate::edit::Editor;
use crate::object::{Native, Object};
use crate::reflect::ClassDb;
use crate::value::{StructValue, Value};

pub type Mat = [[f64; 4]; 4];

pub const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// Rotator units (65536 per turn) to radians.
fn rad(u: f64) -> f64 {
    u * std::f64::consts::PI / 32768.0
}

/// `FRotationMatrix`.
pub fn rotation(pitch: f64, yaw: f64, roll: f64) -> Mat {
    let (sp, cp) = rad(pitch).sin_cos();
    let (sy, cy) = rad(yaw).sin_cos();
    let (sr, cr) = rad(roll).sin_cos();
    [
        [cp * cy, cp * sy, sp, 0.0],
        [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp, 0.0],
        [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// `FScaleRotationTranslationMatrix`.
pub fn srt(scale: [f64; 3], rot: [f64; 3], trans: [f64; 3]) -> Mat {
    let mut m = rotation(rot[0], rot[1], rot[2]);
    for (row, s) in m.iter_mut().zip(scale) {
        for x in row.iter_mut().take(3) {
            *x *= s;
        }
    }
    m[3] = [trans[0], trans[1], trans[2], 1.0];
    m
}

pub fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut out = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// Inverse of an affine matrix (row vectors: the last row is the translation).
pub fn inverse(m: &Mat) -> Mat {
    let a = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0]) + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    let d = if det.abs() < 1e-12 { 1e-12 } else { det };
    let inv = [
        [(a[1][1] * a[2][2] - a[1][2] * a[2][1]) / d, (a[0][2] * a[2][1] - a[0][1] * a[2][2]) / d, (a[0][1] * a[1][2] - a[0][2] * a[1][1]) / d],
        [(a[1][2] * a[2][0] - a[1][0] * a[2][2]) / d, (a[0][0] * a[2][2] - a[0][2] * a[2][0]) / d, (a[0][2] * a[1][0] - a[0][0] * a[1][2]) / d],
        [(a[1][0] * a[2][1] - a[1][1] * a[2][0]) / d, (a[0][1] * a[2][0] - a[0][0] * a[2][1]) / d, (a[0][0] * a[1][1] - a[0][1] * a[1][0]) / d],
    ];
    let t = m[3];
    let mut out = IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = inv[i][j];
        }
    }
    for j in 0..3 {
        out[3][j] = -(t[0] * inv[0][j] + t[1] * inv[1][j] + t[2] * inv[2][j]);
    }
    out
}

/// Unreal → glTF (`(x, z, y) / 100`) as a row-vector matrix, and back.
const P: Mat = [[0.01, 0.0, 0.0, 0.0], [0.0, 0.0, 0.01, 0.0], [0.0, 0.01, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
const P_INV: Mat = [[100.0, 0.0, 0.0, 0.0], [0.0, 0.0, 100.0, 0.0], [0.0, 100.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// A matrix in the Studio's space, as three.js `fromArray` takes it.
pub fn to_view(m: &Mat) -> Vec<f64> {
    let g = mul(&mul(&P_INV, m), &P);
    g.iter().flatten().map(|v| (v * 1e6).round() / 1e6).collect()
}

pub fn from_view(a: &[f64]) -> Result<Mat> {
    if a.len() != 16 {
        return Err(Error::Format("a matrix has 16 numbers".into()));
    }
    let mut g = [[0.0; 4]; 4];
    for (i, v) in a.iter().enumerate() {
        g[i / 4][i % 4] = *v;
    }
    Ok(mul(&mul(&P, &g), &P_INV))
}

/// A point in the Studio's space.
fn view_point(v: [f64; 3]) -> [f64; 3] {
    [v[0] / 100.0, v[2] / 100.0, v[1] / 100.0]
}

/// `FMatrix::Rotator` and the scale of each axis.
pub fn decompose(m: &Mat) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let len = |r: &[f64; 4]| (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
    let scale = [len(&m[0]), len(&m[1]), len(&m[2])];
    let axis = |i: usize| {
        let s = if scale[i] > 1e-9 { scale[i] } else { 1.0 };
        [m[i][0] / s, m[i][1] / s, m[i][2] / s]
    };
    let (x, y, z) = (axis(0), axis(1), axis(2));
    let k = 32768.0 / std::f64::consts::PI;
    let pitch = x[2].atan2((x[0] * x[0] + x[1] * x[1]).sqrt()) * k;
    let yaw = x[1].atan2(x[0]) * k;
    let sy = rotation(pitch, yaw, 0.0)[1];
    let dot = |a: [f64; 3]| a[0] * sy[0] + a[1] * sy[1] + a[2] * sy[2];
    let roll = dot(z).atan2(dot(y)) * k;
    (scale, [pitch, yaw, roll], [m[3][0], m[3][1], m[3][2]])
}

// ----- property access -------------------------------------------------------

pub fn prop<'a>(obj: &'a Object, pkg: &Package, name: &str) -> Option<&'a Value> {
    obj.property(name, pkg).map(|p| &p.value)
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f as f64),
        Value::Int(i) => Some(*i as f64),
        Value::Byte(b) => Some(*b as f64),
        _ => None,
    }
}

/// Members of a struct value by name (binary or tagged).
fn member<'a>(v: &'a Value, pkg: &Package, name: &str) -> Option<&'a Value> {
    match v {
        Value::Struct(StructValue::Binary(ms)) => ms.iter().find(|m| m.name.eq_ignore_ascii_case(name)).and_then(|m| m.elems.first()),
        Value::Struct(StructValue::Tagged(ps)) => ps.iter().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(name)).map(|p| &p.value),
        _ => None,
    }
}

fn members(v: Option<&Value>, pkg: &Package, names: [&str; 3], default: [f64; 3]) -> [f64; 3] {
    let Some(v) = v else { return default };
    let mut out = default;
    for (o, n) in out.iter_mut().zip(names) {
        if let Some(x) = member(v, pkg, n).and_then(num) {
            *o = x;
        }
    }
    out
}

fn vector(obj: &Object, pkg: &Package, name: &str, default: [f64; 3]) -> [f64; 3] {
    members(prop(obj, pkg, name), pkg, ["X", "Y", "Z"], default)
}

fn rotator(obj: &Object, pkg: &Package, name: &str) -> [f64; 3] {
    members(prop(obj, pkg, name), pkg, ["Pitch", "Yaw", "Roll"], [0.0; 3])
}

fn scalar(obj: &Object, pkg: &Package, name: &str, default: f64) -> f64 {
    prop(obj, pkg, name).and_then(num).unwrap_or(default)
}

fn object_ref(obj: &Object, pkg: &Package, name: &str) -> PackageIndex {
    match prop(obj, pkg, name) {
        Some(Value::Object(i)) => *i,
        _ => PackageIndex::default(),
    }
}

fn flag(obj: &Object, pkg: &Package, name: &str) -> Option<bool> {
    match prop(obj, pkg, name) {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}

/// `AActor::LocalToWorld`: `T(-PrePivot) · S · R · T(Location)`.
pub fn actor_matrix(obj: &Object, pkg: &Package) -> Mat {
    let ds = scalar(obj, pkg, "DrawScale", 1.0);
    let ds3 = vector(obj, pkg, "DrawScale3D", [1.0; 3]);
    let pivot = vector(obj, pkg, "PrePivot", [0.0; 3]);
    let m = srt([ds * ds3[0], ds * ds3[1], ds * ds3[2]], rotator(obj, pkg, "Rotation"), vector(obj, pkg, "Location", [0.0; 3]));
    let mut pre = IDENTITY;
    pre[3] = [-pivot[0], -pivot[1], -pivot[2], 1.0];
    mul(&pre, &m)
}

/// A component relative to its owner.
pub fn component_matrix(obj: &Object, pkg: &Package) -> Mat {
    let s = scalar(obj, pkg, "Scale", 1.0);
    let s3 = vector(obj, pkg, "Scale3D", [1.0; 3]);
    srt([s * s3[0], s * s3[1], s * s3[2]], rotator(obj, pkg, "Rotation"), vector(obj, pkg, "Translation", [0.0; 3]))
}

// ----- the scene ---------------------------------------------------------------

/// What an actor is, for the view's filters and icons.
pub fn category(db: &ClassDb, class: &str) -> &'static str {
    let is = |base: &str| db.class_is_a(class, base);
    let lower = class.to_ascii_lowercase();
    if is("PlayerStart") {
        "playerstart"
    } else if lower.contains("factory") {
        "botfactory"
    } else if lower.contains("spawnpoint") || lower.contains("spawner") {
        "spawn"
    } else if is("StaticMeshActorBase") || is("StaticMeshActor") || is("InterpActor") || is("KActor") || is("SkeletalMeshActor") {
        "mesh"
    } else if is("Trigger") || lower.contains("trigger") {
        "trigger"
    } else if is("BlockingVolume") {
        "blocking"
    } else if is("Volume") {
        "volume"
    } else if is("Light") || is("StaticLightCollectionActor") {
        "light"
    } else if is("AkAmbientSound") || is("AmbientSound") || lower.contains("sound") {
        "sound"
    } else if is("Emitter") {
        "emitter"
    } else if lower.contains("jumppad") || lower.contains("objective") || lower.contains("capturepoint") || lower.contains("payload") || lower.contains("cart") || lower.contains("teleport") || lower.contains("spline") || lower.contains("goal") {
        "gameplay"
    } else if is("NavigationPoint") || lower.starts_with("havok") {
        "navigation"
    } else if is("Note") || is("WorldInfo") || lower.contains("performancecapture") || lower.contains("gameinfo") {
        "info"
    } else {
        "other"
    }
}

/// Exports whose outer is `owner`.
fn children_of(pkg: &Package, owner: usize) -> Vec<usize> {
    let o = PackageIndex::export(owner);
    pkg.exports.iter().enumerate().filter(|(_, e)| e.outer == o).map(|(i, _)| i).collect()
}

/// The triangles of a brush component (`BrushAggGeom.ConvexElems`, in the
/// owner's space), else of its brush's polys (`Brush` → Model → Polys),
/// as view-space positions and indices.
fn brush_geometry(ed: &mut Editor, actor: &Object, comp: Option<usize>) -> Option<(Vec<f64>, Vec<u32>)> {
    let mut pos: Vec<f64> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    if let Some(c) = comp {
        if let Ok(geom) = ed.with_object(c, |pkg, o| {
            let mut pos = Vec::new();
            let mut idx = Vec::new();
            let agg = prop(o, pkg, "BrushAggGeom");
            let elems = agg.and_then(|a| member(a, pkg, "ConvexElems"));
            if let Some(Value::Array(elems)) = elems {
                for e in elems {
                    let base = (pos.len() / 3) as u32;
                    if let Some(Value::Array(vs)) = member(e, pkg, "VertexData") {
                        for v in vs {
                            let p = members(Some(v), pkg, ["X", "Y", "Z"], [0.0; 3]);
                            pos.extend(view_point(p));
                        }
                    }
                    if let Some(Value::Array(tris)) = member(e, pkg, "FaceTriData") {
                        for t in tris.chunks(3) {
                            if t.len() == 3 {
                                // Reversed: the swap of y and z mirrors the winding.
                                for v in [&t[0], &t[2], &t[1]] {
                                    idx.push(base + num(v).unwrap_or(0.0) as u32);
                                }
                            }
                        }
                    }
                }
            }
            (pos, idx)
        }) {
            pos = geom.0;
            idx = geom.1;
        }
    }
    if !idx.is_empty() {
        return Some((pos, idx));
    }
    // Polys of the brush model.
    let brush = object_ref(actor, &ed.pkg, "Brush").export_index()?;
    let polys = ed.with_object(brush, |_, o| match &o.native {
        Native::Model(m) => m.polys.export_index(),
        _ => None,
    });
    let polys = polys.ok().flatten()?;
    let elems = ed.with_object(polys, |_, o| match &o.native {
        Native::Polys(p) => p.elements.iter().map(|e| e.vertices.clone()).collect::<Vec<_>>(),
        _ => Vec::new(),
    });
    for poly in elems.ok()? {
        let base = (pos.len() / 3) as u32;
        for v in &poly {
            pos.extend(view_point([v[0] as f64, v[1] as f64, v[2] as f64]));
        }
        for k in 1..poly.len().saturating_sub(1) {
            idx.extend([base, base + k as u32 + 1, base + k as u32]);
        }
    }
    (!idx.is_empty()).then_some((pos, idx))
}

/// A mesh a component draws: an export of the package, or an object path
/// (an import) the caller resolves.
pub enum MeshRef {
    Export(usize),
    Path(String),
}

fn mesh_ref(pkg: &Package, idx: PackageIndex) -> Option<MeshRef> {
    if let Some(i) = idx.export_index() {
        return Some(MeshRef::Export(i));
    }
    idx.is_import().then(|| MeshRef::Path(pkg.object_path(idx)))
}

fn mesh_json(r: &MeshRef, path: &str) -> J {
    match r {
        MeshRef::Export(i) => json!({ "path": path, "index": i }),
        MeshRef::Path(p) => json!({ "object": p }),
    }
}

/// Everything a component shows: a mesh, a light, a collision cylinder, a
/// particle system, a sprite.
fn component_json(ed: &mut Editor, db: &ClassDb, path: &str, c: usize) -> Option<J> {
    let class = ed.pkg.class_name_of(PackageIndex::export(c));
    let is = |b: &str| db.class_is_a(&class, b);
    let pkgpath = path.to_string();
    ed.with_object(c, |pkg, o| {
        let local = to_view(&component_matrix(o, pkg));
        let hidden = flag(o, pkg, "HiddenGame").unwrap_or(false);
        if is("StaticMeshComponent") || is("SkeletalMeshComponent") {
            let mesh = object_ref(o, pkg, if is("StaticMeshComponent") { "StaticMesh" } else { "SkeletalMesh" });
            let r = mesh_ref(pkg, mesh)?;
            let materials: Vec<J> = match prop(o, pkg, "Materials") {
                Some(Value::Array(ms)) => ms
                    .iter()
                    .map(|m| match m {
                        Value::Object(i) if !i.is_null() => json!(pkg.object_path(*i)),
                        _ => J::Null,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Some(json!({ "kind": "mesh", "export": c, "class": class, "mesh": mesh_json(&r, &pkgpath), "meshName": pkg.object_path(mesh), "materials": materials, "matrix": local, "hidden": hidden }))
        } else if is("LightComponent") {
            let color = prop(o, pkg, "LightColor").map(|v| members(Some(v), pkg, ["R", "G", "B"], [255.0; 3])).unwrap_or([255.0; 3]);
            let kind = if is("SpotLightComponent") {
                "spot"
            } else if is("PointLightComponent") {
                "point"
            } else if is("DirectionalLightComponent") {
                "directional"
            } else {
                "sky"
            };
            Some(json!({ "kind": "light", "light": kind, "export": c, "class": class, "radius": scalar(o, pkg, "Radius", 1024.0), "brightness": scalar(o, pkg, "Brightness", 1.0), "color": color, "matrix": local, "outerCone": scalar(o, pkg, "OuterConeAngle", 44.0) }))
        } else if is("CylinderComponent") {
            Some(json!({ "kind": "cylinder", "export": c, "radius": scalar(o, pkg, "CollisionRadius", 22.0), "height": scalar(o, pkg, "CollisionHeight", 22.0), "matrix": local }))
        } else if is("ParticleSystemComponent") {
            let t = object_ref(o, pkg, "Template");
            Some(json!({ "kind": "particles", "export": c, "template": (!t.is_null()).then(|| pkg.object_path(t)), "matrix": local }))
        } else {
            None
        }
    })
    .ok()
    .flatten()
}

/// The level export of a package.
pub fn level_export(pkg: &Package) -> Option<usize> {
    (0..pkg.exports.len()).find(|&i| pkg.class_name_of(PackageIndex::export(i)).eq_ignore_ascii_case("Level"))
}

/// The packages a persistent level streams (`WorldInfo.StreamingLevels`),
/// by package name.
pub fn streamed_levels(ed: &mut Editor) -> Vec<String> {
    let n = ed.pkg.exports.len();
    let mut out = Vec::new();
    for i in 0..n {
        if !ed.pkg.class_name_of(PackageIndex::export(i)).eq_ignore_ascii_case("WorldInfo") {
            continue;
        }
        let refs: Vec<usize> = ed
            .with_object(i, |pkg, o| match prop(o, pkg, "StreamingLevels") {
                Some(Value::Array(a)) => a.iter().filter_map(|v| if let Value::Object(x) = v { x.export_index() } else { None }).collect(),
                _ => Vec::new(),
            })
            .unwrap_or_default();
        for r in refs {
            if let Ok(Some(name)) = ed.with_object(r, |pkg, o| match prop(o, pkg, "PackageName") {
                Some(Value::Name(n)) => Some(pkg.fname_to_string(*n)),
                _ => None,
            }) {
                out.push(name);
            }
        }
    }
    out
}

/// `Actor.m_bCanBaseOn` (Hi-Rez): False makes the surfaces "rooftops" pawns
/// slide off; on walls that is what players call a heavy wall. Absent means
/// the class default (True on `Actor`).
fn can_base_on(db: &ClassDb, obj: &Object, pkg: &Package) -> bool {
    if let Some(b) = flag(obj, pkg, "m_bCanBaseOn") {
        return b;
    }
    match obj.class.and_then(|c| db.cdo_scalar(c, "m_bCanBaseOn")) {
        Some(crate::reflect::CdoValue::Bool(b)) => b,
        _ => true,
    }
}

/// Properties worth showing next to an actor in the outliner.
fn info(obj: &Object, pkg: &Package, cat: &str) -> J {
    let mut m = serde_json::Map::new();
    for name in ["TeamIndex", "m_nTaskForce", "Tag", "bPrimaryStart", "bEnabled"] {
        if let Some(v) = prop(obj, pkg, name) {
            let text = match v {
                Value::Int(i) => json!(i),
                Value::Byte(b) => json!(b),
                Value::Bool(b) => json!(b),
                Value::Name(n) => json!(pkg.fname_to_string(*n)),
                Value::Float(f) => json!(f),
                _ => continue,
            };
            m.insert(name.to_string(), text);
        }
    }
    let _ = cat;
    J::Object(m)
}

/// `map.scene` for one level package: every actor with its world matrix,
/// what its components draw, and how it can be moved (`edit`: the actor's
/// Location/Rotation/DrawScale3D, or for a mesh of a collection actor the
/// component's Translation/Rotation/Scale3D).
pub fn level_scene(ed: &mut Editor, db: &ClassDb, path: &str) -> Result<J> {
    let level = level_export(&ed.pkg).ok_or_else(|| Error::Format("this package has no level".into()))?;
    let actors: Vec<usize> = ed.with_object(level, |_, o| match &o.native {
        Native::Level(l) => l.actors.iter().filter_map(|a| a.export_index()).collect(),
        _ => Vec::new(),
    })?;
    let mut out = Vec::new();
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    for a in actors {
        let class = ed.pkg.class_name_of(PackageIndex::export(a));
        let cat = category(db, &class);
        let name = ed.pkg.object_name(PackageIndex::export(a));
        let Ok((m, loc, rot, ds, ds3, information, hidden, base_on)) = ed.with_object(a, |pkg, o| {
            (
                actor_matrix(o, pkg),
                vector(o, pkg, "Location", [0.0; 3]),
                rotator(o, pkg, "Rotation"),
                scalar(o, pkg, "DrawScale", 1.0),
                vector(o, pkg, "DrawScale3D", [1.0; 3]),
                info(o, pkg, cat),
                flag(o, pkg, "bHidden").unwrap_or(false),
                can_base_on(db, o, pkg),
            )
        }) else {
            continue;
        };
        let comps = children_of(&ed.pkg, a);
        let collection = db.class_is_a(&class, "StaticMeshCollectionActor");
        if collection {
            // Each mesh is placed by its component: one item per component.
            for c in comps {
                let Some(mut cj) = component_json(ed, db, path, c) else { continue };
                let cm = ed.with_object(c, |pkg, o| component_matrix(o, pkg))?;
                cj["matrix"] = json!(to_view(&IDENTITY));
                *counts.entry("mesh").or_default() += 1;
                out.push(json!({
                    "export": c,
                    "name": ed.pkg.object_name(PackageIndex::export(c)),
                    "class": ed.pkg.class_name_of(PackageIndex::export(c)),
                    "category": "mesh",
                    "matrix": to_view(&mul(&cm, &m)),
                    "edit": "component",
                    "parent": a,
                    "parts": [cj],
                    "info": {},
                    "canBaseOn": base_on,
                }));
            }
            continue;
        }
        let mut parts = Vec::new();
        let mut brush_comp = None;
        for c in &comps {
            let cclass = ed.pkg.class_name_of(PackageIndex::export(*c));
            if db.class_is_a(&cclass, "BrushComponent") {
                brush_comp = Some(*c);
            }
            if let Some(cj) = component_json(ed, db, path, *c) {
                parts.push(cj);
            }
        }
        // Light collections place their lights with stored matrices.
        if db.class_is_a(&class, "StaticLightCollectionActor") {
            let mats = ed.with_object(a, |_, o| match &o.native {
                Native::StaticLightCollectionActor(l) => l.light_to_world.clone(),
                _ => Vec::new(),
            })?;
            let order: Vec<usize> = ed.with_object(a, |pkg, o| match prop(o, pkg, "LightComponents") {
                Some(Value::Array(v)) => v.iter().filter_map(|x| if let Value::Object(i) = x { i.export_index() } else { None }).collect(),
                _ => Vec::new(),
            })?;
            for (k, c) in order.iter().enumerate() {
                let Some(mat) = mats.get(k) else { continue };
                let Some(mut cj) = component_json(ed, db, path, *c) else { continue };
                let mut mm = IDENTITY;
                for (i, row) in mm.iter_mut().enumerate() {
                    for (j, x) in row.iter_mut().enumerate() {
                        *x = mat[i * 4 + j] as f64;
                    }
                }
                cj["matrix"] = json!(to_view(&mm));
                cj["world"] = json!(true);
                parts.retain(|p| p["export"].as_u64() != Some(*c as u64));
                parts.push(cj);
            }
        }
        let actor_obj = ed.with_object(a, |_, o| o.clone())?;
        if let Some((pos, idx)) = brush_geometry(ed, &actor_obj, brush_comp) {
            parts.push(json!({ "kind": "brush", "export": brush_comp, "positions": pos, "indices": idx }));
        }
        *counts.entry(cat).or_default() += 1;
        out.push(json!({
            "export": a,
            "name": name,
            "class": class,
            "category": cat,
            "matrix": to_view(&m),
            "location": loc,
            "rotation": rot,
            "drawScale": ds,
            "drawScale3D": ds3,
            "edit": "actor",
            "hidden": hidden,
            "parts": parts,
            "info": information,
            "canBaseOn": base_on,
        }));
    }
    Ok(json!({ "path": path, "level": level, "actors": out, "counts": counts }))
}

/// Move an actor (`Location`, `Rotation`, `DrawScale3D`) or a component
/// (`Translation`, `Rotation`, `Scale3D`) to a world matrix given in the
/// Studio's space; the component's owner (`parent`) matrix is taken out.
pub fn set_transform(ed: &mut Editor, i: usize, view: &[f64], parent: Option<usize>) -> Result<J> {
    let world = from_view(view)?;
    let component = parent.is_some();
    let local = match parent {
        Some(p) => {
            let pm = ed.with_object(p, |pkg, o| actor_matrix(o, pkg))?;
            mul(&world, &inverse(&pm))
        }
        None => {
            // PrePivot comes off before the actor's own transform.
            let pivot = ed.with_object(i, |pkg, o| vector(o, pkg, "PrePivot", [0.0; 3]))?;
            let mut post = IDENTITY;
            post[3] = [pivot[0], pivot[1], pivot[2], 1.0];
            mul(&post, &world)
        }
    };
    let (scale, rot, trans) = decompose(&local);
    let base = ed.with_object(i, |pkg, o| scalar(o, pkg, if component { "Scale" } else { "DrawScale" }, 1.0))?;
    let base = if base.abs() < 1e-9 { 1.0 } else { base };
    let (t_name, s_name) = if component { ("Translation", "Scale3D") } else { ("Location", "DrawScale3D") };
    let fmt = |v: f64| {
        let r = (v * 10000.0).round() / 10000.0;
        if r == r.trunc() {
            format!("{r:.1}")
        } else {
            format!("{r}")
        }
    };
    let old_scale = ed.with_object(i, |pkg, o| vector(o, pkg, s_name, [1.0; 3]))?;
    for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
        ed.set(i, &format!("{t_name}.{axis}"), &fmt(trans[k]))?;
    }
    for (k, axis) in ["Pitch", "Yaw", "Roll"].iter().enumerate() {
        ed.set(i, &format!("Rotation.{axis}"), &format!("{}", rot[k].round() as i64))?;
    }
    let new_scale = [scale[0] / base, scale[1] / base, scale[2] / base];
    if (0..3).any(|k| (new_scale[k] - old_scale[k]).abs() > 1e-4) {
        for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
            ed.set(i, &format!("{s_name}.{axis}"), &fmt(new_scale[k]))?;
        }
    }
    Ok(json!({ "translation": trans, "rotation": rot.map(|r| r.round()), "scale": new_scale }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotator_round_trip() {
        for (p, y, r) in [(0.0, 16384.0, 0.0), (2048.0, 0.0, 2048.0), (-4000.0, 12000.0, 300.0)] {
            let m = srt([2.0, 1.0, 0.5], [p, y, r], [10.0, -20.0, 30.0]);
            let (s, rot, t) = decompose(&m);
            assert!((s[0] - 2.0).abs() < 1e-6 && (s[2] - 0.5).abs() < 1e-6);
            assert!((rot[0] - p).abs() < 1e-3 && (rot[1] - y).abs() < 1e-3 && (rot[2] - r).abs() < 1e-3, "{rot:?}");
            assert_eq!(t, [10.0, -20.0, 30.0]);
            let back = from_view(&to_view(&m)).unwrap();
            for i in 0..4 {
                for j in 0..4 {
                    assert!((back[i][j] - m[i][j]).abs() < 1e-4);
                }
            }
            let inv = mul(&m, &inverse(&m));
            for i in 0..4 {
                for j in 0..4 {
                    assert!((inv[i][j] - IDENTITY[i][j]).abs() < 1e-9);
                }
            }
        }
    }
}
