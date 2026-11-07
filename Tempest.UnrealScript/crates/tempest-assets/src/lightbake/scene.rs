//! What static lighting needs from a level package: its static lights, its
//! static mesh components (placement, mesh, light map settings) and the
//! world's Lightmass settings, read with the engine's defaults (a property
//! an object does not store comes from its archetype, then from the class
//! default objects).
//!
//! Light transforms follow `ULightComponent::SetParentToWorld`,
//! `UPointLightComponent::SetTransformedToWorld` and
//! `USpotLightComponent::SetTransformedToWorld`: the parent matrix is the
//! owner's `LocalToWorld`, or for a `StaticLightCollectionActor` the matrix
//! stored per light; a light points along the parent's X axis.

use std::collections::HashMap;
use std::sync::Arc;

use tempest_upk::io::{Guid, PackageIndex};
use tempest_upk::Package;

use super::bvh::{cross, normalize, V3};
use crate::edit::Editor;
use crate::map::{actor_matrix, component_matrix, level_export, mul, Mat};
use crate::mesh::StaticMesh;
use crate::object::{package_layout, parse_export, Native, Object, Parsed};
use crate::reflect::ClassDb;
use crate::value::{StructValue, Value};

// ----- property values with defaults ---------------------------------------

/// A property value detached from its package (names as text).
#[derive(Clone, Debug, PartialEq)]
pub enum Pv {
    Num(f64),
    Bool(bool),
    Name(String),
    Struct(Vec<(String, Pv)>),
    Other,
}

fn pv(pkg: &Package, v: &Value) -> Pv {
    match v {
        Value::Bool(b) => Pv::Bool(*b),
        Value::Byte(b) => Pv::Num(*b as f64),
        Value::Int(i) => Pv::Num(*i as f64),
        Value::Float(f) => Pv::Num(*f as f64),
        Value::Enum(n) | Value::Name(n) => Pv::Name(pkg.fname_to_string(*n)),
        Value::Struct(StructValue::Binary(ms)) => Pv::Struct(ms.iter().filter_map(|m| m.elems.first().map(|e| (m.name.to_ascii_lowercase(), pv(pkg, e)))).collect()),
        Value::Struct(StructValue::Tagged(ps)) => Pv::Struct(ps.iter().filter(|p| p.array_index == 0).map(|p| (pkg.fname_to_string(p.name).to_ascii_lowercase(), pv(pkg, &p.value))).collect()),
        _ => Pv::Other,
    }
}

impl Pv {
    pub fn num(&self) -> Option<f64> {
        match self {
            Pv::Num(n) => Some(*n),
            Pv::Bool(b) => Some(*b as u8 as f64),
            _ => None,
        }
    }
    pub fn member(&self, name: &str) -> Option<&Pv> {
        match self {
            Pv::Struct(ms) => ms.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// An object's properties (index 0 of each), lower-case names.
pub type Props = HashMap<String, Pv>;

pub fn props_of(pkg: &Package, o: &Object) -> Props {
    o.properties.iter().filter(|p| p.array_index == 0).map(|p| (pkg.fname_to_string(p.name).to_ascii_lowercase(), pv(pkg, &p.value))).collect()
}

/// The properties of an object, then of its archetypes, then of its class's
/// default objects: the first that stores a property (or struct member) wins.
#[derive(Clone, Debug, Default)]
pub struct Chain(pub Vec<Props>);

impl Chain {
    pub fn get(&self, name: &str) -> Option<&Pv> {
        let n = name.to_ascii_lowercase();
        self.0.iter().find_map(|p| p.get(&n))
    }
    pub fn num(&self, name: &str, default: f64) -> f64 {
        self.get(name).and_then(|v| v.num()).unwrap_or(default)
    }
    pub fn flag(&self, name: &str, default: bool) -> bool {
        match self.get(name) {
            Some(Pv::Bool(b)) => *b,
            _ => default,
        }
    }
    /// A struct member: tagged structs only store what differs from their
    /// archetype's value, so each level of the chain is asked in turn.
    pub fn member(&self, name: &str, member: &str) -> Option<&Pv> {
        let n = name.to_ascii_lowercase();
        self.0.iter().find_map(|p| p.get(&n).and_then(|v| v.member(member)))
    }
    pub fn member_num(&self, name: &str, member: &str, default: f64) -> f64 {
        self.member(name, member).and_then(|v| v.num()).unwrap_or(default)
    }
    pub fn member_flag(&self, name: &str, member: &str, default: bool) -> bool {
        match self.member(name, member) {
            Some(Pv::Bool(b)) => *b,
            _ => default,
        }
    }
}

/// Parse a script-package object of the class database.
fn db_object(db: &ClassDb, r: tempest_unrealscript::context::ObjRef) -> Option<(Props, PackageIndex, usize)> {
    let lp = db.ctx.packages.get(r.pkg)?;
    let data = lp.pkg.export_data.get(r.export)?;
    match parse_export(db, &lp.pkg, package_layout(&lp.pkg), r.export, data) {
        Parsed::Object(o) => Some((props_of(&lp.pkg, &o), lp.pkg.exports[r.export].archetype, r.pkg)),
        _ => None,
    }
}

/// An object path (`Engine.Default__PointLight.PointLightComponent0`) in
/// the class database.
fn db_find(db: &ClassDb, path: &str) -> Option<tempest_unrealscript::context::ObjRef> {
    let (root, rest) = path.split_once('.')?;
    let slot = db.ctx.slot_of(root)?;
    db.ctx.find_export(slot, rest)
}

/// The defaults chain of export `i` of the editor's package.
pub fn chain(ed: &mut Editor, db: &ClassDb, i: usize) -> Chain {
    let mut out = Vec::new();
    let Ok((props, class)) = ed.with_object(i, |pkg, o| (props_of(pkg, o), o.class)) else { return Chain::default() };
    out.push(props);
    // Archetypes: exports of this package first, then script objects.
    let mut arch = ed.pkg.exports[i].archetype;
    let mut db_ref = None;
    for _ in 0..16 {
        if let Some(e) = arch.export_index() {
            let Ok(props) = ed.with_object(e, |pkg, o| props_of(pkg, o)) else { break };
            out.push(props);
            arch = ed.pkg.exports[e].archetype;
            continue;
        }
        if arch.is_import() {
            db_ref = db_find(db, &ed.pkg.object_path(arch));
        }
        break;
    }
    let mut seen_cdo = false;
    for _ in 0..16 {
        let Some(r) = db_ref else { break };
        let Some((props, next, pkg)) = db_object(db, r) else { break };
        if db.ctx.name(r).to_ascii_lowercase().starts_with("default__") {
            seen_cdo = true;
        }
        out.push(props);
        db_ref = if next.is_null() { None } else { db.ctx.resolve(pkg, next) };
    }
    if !seen_cdo {
        // The class default objects, child first.
        let mut c = class;
        for _ in 0..32 {
            let Some(cl) = c else { break };
            if let Some(cdo) = db.class_default_object(cl) {
                if let Some((props, _, _)) = db_object(db, cdo) {
                    out.push(props);
                }
            }
            c = db.super_class(cl);
        }
    }
    Chain(out)
}

// ----- lighting channels -----------------------------------------------------

/// `FLightingChannelContainer` as the set of channels that are on
/// (`bInitialized` aside).
pub fn channels(c: &Chain, defaults: &[&str]) -> Vec<String> {
    let mut on: Vec<String> = defaults.iter().map(|s| s.to_string()).collect();
    // Each level of the chain overrides the members it stores, from the base up.
    for p in c.0.iter().rev() {
        if let Some(Pv::Struct(ms)) = p.get("lightingchannels") {
            for (n, v) in ms {
                if n == "binitialized" {
                    continue;
                }
                on.retain(|x| x != n);
                if matches!(v, Pv::Bool(true)) {
                    on.push(n.clone());
                }
            }
        }
    }
    on
}

/// `FLightingChannelContainer::OverlapsWith`.
pub fn overlaps(a: &[String], b: &[String]) -> bool {
    a.iter().any(|x| b.contains(x))
}

/// `LightComponent.uc`: `LightingChannels=(BSP,Static,Dynamic,CompositeDynamic)`.
pub const LIGHT_CHANNELS: [&str; 4] = ["bsp", "static", "dynamic", "compositedynamic"];
/// A primitive's channels: all off by default; `UPrimitiveComponent::Attach`
/// sets `Static` (static shadowing) or `Dynamic` when they were never
/// initialised or none is on.
pub fn primitive_channels(c: &Chain, static_shadowing: bool) -> Vec<String> {
    let on = channels(c, &[]);
    if !c.member_flag("lightingchannels", "binitialized", false) || on.is_empty() {
        return vec![if static_shadowing { "static" } else { "dynamic" }.to_string()];
    }
    on
}

// ----- lights ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum LightKind {
    Point,
    Spot { inner: f32, outer: f32 },
    Directional,
    Sky { lower: [f32; 3] },
}

#[derive(Clone, Debug)]
pub struct Light {
    pub level: usize,
    pub export: usize,
    pub name: String,
    pub kind: LightKind,
    /// World position (point, spot).
    pub position: V3,
    /// The direction the light shines along (spot, directional).
    pub direction: V3,
    pub radius: f32,
    pub falloff: f32,
    /// `FLinearColor(LightColor) · Brightness` (sky: the upper hemisphere).
    pub intensity: [f32; 3],
    pub casts_static_shadows: bool,
    /// `LightmassSettings.LightSourceRadius` / `LightSourceAngle` (radians).
    pub source_radius: f32,
    pub source_angle: f32,
    pub indirect_scale: f32,
    pub light_guid: Guid,
    pub lightmap_guid: Guid,
    pub channels: Vec<String>,
}

fn guid(c: &Chain, name: &str) -> Guid {
    let m = |k: &str| c.member_num(name, k, 0.0) as i64 as i32 as u32;
    Guid { a: m("a"), b: m("b"), c: m("c"), d: m("d") }
}

/// `FLinearColor(FColor)`: `pow(c / 255, 2.2)`.
pub fn srgb_color(c: &Chain, name: &str, default: [f64; 3]) -> [f32; 3] {
    let ch = |k: &str, d: f64| ((c.member_num(name, k, d) / 255.0).powf(2.2)) as f32;
    [ch("r", default[0]), ch("g", default[1]), ch("b", default[2])]
}

fn row(m: &Mat, r: usize) -> V3 {
    [m[r][0] as f32, m[r][1] as f32, m[r][2] as f32]
}

/// A light component placed by `parent` (its owner's `LocalToWorld` or the
/// collection's stored matrix). `None` for lights that are not baked.
fn light(db: &ClassDb, class: &str, c: &Chain, parent: &Mat) -> Option<(LightKind, V3, V3, f32, f32)> {
    let is = |b: &str| db.class_is_a(class, b);
    if is("DominantDirectionalLightComponent") || is("DominantSpotLightComponent") || is("DominantPointLightComponent") {
        // Dominant lights stay dynamic (distance field shadow maps only).
        return None;
    }
    let translation = [c.member_num("translation", "x", 0.0), c.member_num("translation", "y", 0.0), c.member_num("translation", "z", 0.0)];
    let at = |local: &Mat| -> V3 { row(&mul(local, parent), 3) };
    let mut t = crate::map::IDENTITY;
    t[3] = [translation[0], translation[1], translation[2], 1.0];
    if is("SpotLightComponent") {
        let rot = [c.member_num("rotation", "pitch", 0.0), c.member_num("rotation", "yaw", 0.0), c.member_num("rotation", "roll", 0.0)];
        let local = crate::map::srt([1.0; 3], rot, translation);
        let m = mul(&local, parent);
        let inner = c.num("innerconeangle", 0.0) as f32;
        let outer = c.num("outerconeangle", 44.0) as f32;
        Some((LightKind::Spot { inner, outer }, row(&m, 3), normalize(row(&m, 0)), c.num("radius", 1024.0) as f32, c.num("falloffexponent", 2.0) as f32))
    } else if is("PointLightComponent") {
        Some((LightKind::Point, at(&t), [0.0; 3], c.num("radius", 1024.0) as f32, c.num("falloffexponent", 2.0) as f32))
    } else if is("DirectionalLightComponent") {
        Some((LightKind::Directional, [0.0; 3], normalize(row(parent, 0)), 0.0, 0.0))
    } else if is("SkyLightComponent") {
        let lower = srgb_color(c, "lowercolor", [0.0; 3]);
        let lb = c.num("lowerbrightness", 0.0) as f32;
        Some((LightKind::Sky { lower: [lower[0] * lb, lower[1] * lb, lower[2] * lb] }, [0.0; 3], [0.0, 0.0, 1.0], 0.0, 0.0))
    } else {
        None
    }
}

/// `FPointLight::AffectsBounds` / `FSpotLight::AffectsBounds` (Lightmass
/// `Scene.cpp`): can the light reach a bounding sphere at all.
pub fn affects_bounds(l: &Light, origin: V3, radius: f32) -> bool {
    use super::bvh::{dot, scale, sub};
    match l.kind {
        LightKind::Point | LightKind::Spot { .. } => {
            let d = sub(origin, l.position);
            if dot(d, d) > (l.radius + radius).powi(2) {
                return false;
            }
            let LightKind::Spot { inner, outer } = l.kind else { return true };
            let ci = inner.clamp(0.0, 89.0).to_radians();
            let co = outer.to_radians().clamp(ci + 0.001, 89f32.to_radians() + 0.001);
            let (sin, cos) = (co.sin(), co.cos());
            let u = sub(l.position, scale(l.direction, radius / sin));
            let d = sub(origin, u);
            let (dsqr, e) = (dot(d, d), dot(l.direction, d));
            if e > 0.0 && e * e >= dsqr * cos * cos {
                let d = sub(origin, l.position);
                let dsqr = dot(d, d);
                let e = -dot(l.direction, d);
                if e > 0.0 && e * e >= dsqr * sin * sin {
                    return dsqr <= radius * radius;
                }
                return true;
            }
            false
        }
        _ => true,
    }
}

// ----- meshes ----------------------------------------------------------------

/// Level of detail 0 of a static mesh as static lighting sees it.
#[derive(Debug, Default)]
pub struct MeshData {
    pub positions: Vec<V3>,
    pub tangent_x: Vec<V3>,
    pub tangent_y: Vec<V3>,
    pub tangent_z: Vec<V3>,
    /// The light map texture coordinates (`LightMapCoordinateIndex`).
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Triangles of elements that cast shadows.
    pub shadow: Vec<bool>,
    pub resolution: i32,
    pub coordinate_index: i32,
    pub has_lightmap_uvs: bool,
    /// LODs 1 and up, each baked into its own light map like Lightmass
    /// does (shadows are cast by LOD 0 only).
    pub lower: Vec<Arc<MeshData>>,
}

fn packed_normal(b: &[u8]) -> (V3, f32) {
    // FPackedNormal: x / 127.5 - 1.
    let f = |x: u8| x as f32 / 127.5 - 1.0;
    ([f(b[0]), f(b[1]), f(b[2])], f(b[3]))
}

fn half(h: u16) -> f32 {
    crate::dds::half_to_f32(h)
}

/// `FStaticMeshRenderData` of every LOD with the light map channel (LOD 0,
/// the others in `lower`).
pub fn mesh_data(m: &StaticMesh, c: &Chain) -> Option<MeshData> {
    let mut md = lod_data(m, c, 0)?;
    md.lower = (1..m.lod_models.len()).map_while(|l| lod_data(m, c, l)).map(Arc::new).collect();
    Some(md)
}

fn lod_data(m: &StaticMesh, c: &Chain, lod: usize) -> Option<MeshData> {
    let l = m.lod_models.get(lod)?;
    let n = l.num_vertices as usize;
    let pvb = &l.position_vb;
    if pvb.data.elem_size != 12 || pvb.data.count as usize != n {
        return None;
    }
    let vb = &l.vertex_buffer;
    let data = vb.data.as_ref()?;
    let stride = vb.stride as usize;
    let full = vb.use_full_precision_uvs != 0;
    let coord = c.num("lightmapcoordinateindex", 0.0) as i32;
    let has_uvs = coord >= 0 && (coord as u32) < vb.num_tex_coords;
    let uv_size = if full { 8 } else { 4 };
    let uv_off = 8 + coord.max(0) as usize * uv_size;
    if data.count as usize != n || stride < 8 {
        return None;
    }
    let mut md = MeshData { resolution: c.num("lightmapresolution", 0.0) as i32, coordinate_index: coord, has_lightmap_uvs: has_uvs, ..Default::default() };
    for i in 0..n {
        let e = pvb.data.elem(i);
        let f = |o: usize| f32::from_le_bytes([e[o], e[o + 1], e[o + 2], e[o + 3]]);
        md.positions.push([f(0), f(4), f(8)]);
        let v = data.elem(i);
        let (tx, _) = packed_normal(&v[0..4]);
        let (tz, w) = packed_normal(&v[4..8]);
        // FStaticMeshVertexBuffer::VertexTangentY.
        let ty = cross(tz, tx);
        md.tangent_x.push(tx);
        md.tangent_y.push([ty[0] * w, ty[1] * w, ty[2] * w]);
        md.tangent_z.push(tz);
        let uv = if has_uvs && uv_off + uv_size <= stride {
            if full {
                [f32::from_le_bytes(v[uv_off..uv_off + 4].try_into().unwrap()), f32::from_le_bytes(v[uv_off + 4..uv_off + 8].try_into().unwrap())]
            } else {
                [half(u16::from_le_bytes([v[uv_off], v[uv_off + 1]])), half(u16::from_le_bytes([v[uv_off + 2], v[uv_off + 3]]))]
            }
        } else {
            [0.0, 0.0]
        };
        md.uvs.push(uv);
    }
    let ib = &l.index_buffer;
    md.indices = match ib.elem_size {
        2 => ib.data.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect(),
        4 => ib.data.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
        _ => return None,
    };
    let tris = md.indices.len() / 3;
    md.shadow = vec![true; tris];
    for e in &l.elements {
        if e.enable_shadow_casting == 0 {
            let first = e.first_index as usize / 3;
            for t in first..(first + e.num_triangles as usize).min(tris) {
                md.shadow[t] = false;
            }
        }
    }
    Some(md)
}

// ----- level contents ----------------------------------------------------------

/// Where a component's mesh is: an export of its level, or an object path
/// the caller resolves in another package.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MeshKey {
    Export(String, usize),
    Path(String),
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub level: usize,
    /// The component export.
    pub export: usize,
    pub owner: usize,
    pub name: String,
    pub mesh: MeshKey,
    /// Component to world (Unreal row vectors).
    pub world: Mat,
    pub casts_shadow: bool,
    /// Has static lighting (an owner that does not move) and accepts lights.
    pub static_lighting: bool,
    /// `bOverrideLightMapRes` / `OverriddenLightMapRes`.
    pub override_resolution: Option<i32>,
    pub channels: Vec<String>,
    pub two_sided: bool,
}

#[derive(Clone, Debug, Default)]
pub struct WorldSettings {
    /// `FLinearColor(EnvironmentColor) · EnvironmentIntensity`.
    pub environment: [f32; 3],
    pub diffuse_boost: f32,
    pub bounces: i32,
    /// `bUseAmbientOcclusion` and its settings (`FLightmassWorldInfoSettings`).
    pub ambient_occlusion: bool,
    pub direct_occlusion: f32,
    pub indirect_occlusion: f32,
    pub occlusion_exponent: f32,
    pub fully_occluded_fraction: f32,
    pub max_occlusion_distance: f32,
}

#[derive(Debug, Default)]
pub struct LevelData {
    pub path: String,
    pub lights: Vec<Light>,
    pub instances: Vec<Instance>,
    pub meshes: HashMap<MeshKey, Arc<MeshData>>,
    /// Object paths of meshes in other packages.
    pub missing: Vec<String>,
    pub world: Option<WorldSettings>,
}

fn actor_static(c: &Chain) -> bool {
    // AActor::HasStaticShadowing.
    c.flag("bstatic", false) || (c.flag("bnodelete", false) && !c.flag("bmovable", true))
}

/// Read a level package: lights, static mesh components, world settings.
pub fn gather(ed: &mut Editor, db: &ClassDb, level_index: usize, path: &str) -> LevelData {
    let mut out = LevelData { path: path.to_string(), ..Default::default() };
    let Some(level) = level_export(&ed.pkg) else { return out };
    let actors: Vec<usize> = ed
        .with_object(level, |_, o| match &o.native {
            Native::Level(l) => l.actors.iter().filter_map(|a| a.export_index()).collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default();
    let children = |pkg: &Package, owner: usize| -> Vec<usize> {
        let o = PackageIndex::export(owner);
        pkg.exports.iter().enumerate().filter(|(_, e)| e.outer == o).map(|(i, _)| i).collect()
    };
    for a in actors {
        let class = ed.pkg.class_name_of(PackageIndex::export(a));
        let Ok(am) = ed.with_object(a, |pkg, o| actor_matrix(o, pkg)) else { continue };
        let ac = chain(ed, db, a);
        let collection_lights = db.class_is_a(&class, "StaticLightCollectionActor");
        if db.class_is_a(&class, "WorldInfo") {
            let col = |k: &str| ((ac.0.iter().find_map(|p| p.get("lightmasssettings").and_then(|s| s.member("environmentcolor")).and_then(|c| c.member(k)).and_then(|v| v.num())).unwrap_or(0.0)) / 255.0).powf(2.2) as f32;
            let intensity = ac.member_num("lightmasssettings", "environmentintensity", 1.0) as f32;
            out.world = Some(WorldSettings {
                environment: [col("r") * intensity, col("g") * intensity, col("b") * intensity],
                // WorldInfo.uc defaults.
                diffuse_boost: ac.member_num("lightmasssettings", "diffuseboost", 5.0) as f32,
                bounces: ac.member_num("lightmasssettings", "numindirectlightingbounces", 3.0) as i32,
                ambient_occlusion: ac.member_flag("lightmasssettings", "buseambientocclusion", false),
                direct_occlusion: ac.member_num("lightmasssettings", "directilluminationocclusionfraction", 0.5) as f32,
                indirect_occlusion: ac.member_num("lightmasssettings", "indirectilluminationocclusionfraction", 1.0) as f32,
                occlusion_exponent: ac.member_num("lightmasssettings", "occlusionexponent", 1.0) as f32,
                fully_occluded_fraction: ac.member_num("lightmasssettings", "fullyoccludedsamplesfraction", 1.0) as f32,
                max_occlusion_distance: ac.member_num("lightmasssettings", "maxocclusiondistance", 200.0) as f32,
            });
        }
        let static_owner = collection_lights || db.class_is_a(&class, "StaticMeshCollectionActor") || actor_static(&ac);
        // Lights: the collection's stored matrices, else the owner's placement.
        let mut light_parents: Vec<(usize, Mat)> = Vec::new();
        if collection_lights {
            let mats = ed
                .with_object(a, |_, o| match &o.native {
                    Native::StaticLightCollectionActor(l) => l.light_to_world.clone(),
                    _ => Vec::new(),
                })
                .unwrap_or_default();
            let order: Vec<usize> = ed
                .with_object(a, |pkg, o| match o.property("LightComponents", pkg).map(|p| &p.value) {
                    Some(Value::Array(v)) => v.iter().filter_map(|x| if let Value::Object(i) = x { i.export_index() } else { None }).collect(),
                    _ => Vec::new(),
                })
                .unwrap_or_default();
            for (k, c) in order.iter().enumerate() {
                if let Some(m) = mats.get(k) {
                    let mut mm = crate::map::IDENTITY;
                    for (r, rowv) in mm.iter_mut().enumerate() {
                        for (cc, x) in rowv.iter_mut().enumerate() {
                            *x = m[r * 4 + cc] as f64;
                        }
                    }
                    light_parents.push((*c, mm));
                }
            }
        }
        for c in children(&ed.pkg, a) {
            let cclass = ed.pkg.class_name_of(PackageIndex::export(c));
            if db.class_is_a(&cclass, "LightComponent") {
                if !static_owner {
                    continue;
                }
                let parent = light_parents.iter().find(|(i, _)| *i == c).map(|(_, m)| *m).unwrap_or(am);
                let lc = chain(ed, db, c);
                if !lc.flag("benabled", true) || lc.flag("bforcedynamiclight", false) {
                    continue;
                }
                let Some((kind, position, direction, radius, falloff)) = light(db, &cclass, &lc, &parent) else { continue };
                let color = srgb_color(&lc, "lightcolor", [255.0; 3]);
                let b = lc.num("brightness", 1.0) as f32;
                let point_like = matches!(kind, LightKind::Point | LightKind::Spot { .. });
                out.lights.push(Light {
                    level: level_index,
                    export: c,
                    name: ed.pkg.object_name(PackageIndex::export(c)),
                    kind,
                    position,
                    direction,
                    radius,
                    falloff,
                    intensity: [color[0] * b, color[1] * b, color[2] * b],
                    casts_static_shadows: lc.flag("castshadows", true) && lc.flag("caststaticshadows", true),
                    source_radius: if point_like { lc.member_num("lightmasssettings", "lightsourceradius", 100.0) as f32 } else { 0.0 },
                    source_angle: (lc.member_num("lightmasssettings", "lightsourceangle", 3.0) as f32).to_radians(),
                    indirect_scale: lc.member_num("lightmasssettings", "indirectlightingscale", 1.0) as f32,
                    light_guid: guid(&lc, "lightguid"),
                    lightmap_guid: guid(&lc, "lightmapguid"),
                    channels: channels(&lc, &LIGHT_CHANNELS),
                });
                continue;
            }
            if !db.class_is_a(&cclass, "StaticMeshComponent") {
                continue;
            }
            let cc = chain(ed, db, c);
            let Ok((cm, mesh)) = ed.with_object(c, |pkg, o| (component_matrix(o, pkg), o.property("StaticMesh", pkg).map(|p| p.value.clone()))) else { continue };
            let Some(Value::Object(mi)) = mesh else { continue };
            if mi.is_null() {
                continue;
            }
            let key = match mi.export_index() {
                Some(e) => MeshKey::Export(path.to_string(), e),
                None => MeshKey::Path(ed.pkg.object_path(mi)),
            };
            if let MeshKey::Export(_, e) = &key {
                if !out.meshes.contains_key(&key) {
                    let mc = chain(ed, db, *e);
                    if let Ok(Some(md)) = ed.with_object(*e, |_, o| match &o.native {
                        Native::StaticMesh(m) => mesh_data(m, &mc),
                        _ => None,
                    }) {
                        out.meshes.insert(key.clone(), Arc::new(md));
                    }
                }
            } else if let MeshKey::Path(p) = &key {
                if !out.missing.contains(p) {
                    out.missing.push(p.clone());
                }
            }
            let hidden = cc.flag("hiddengame", false);
            let casts = cc.flag("castshadow", true) && (!hidden || cc.flag("bcasthiddenshadow", false));
            let accepts = cc.flag("bacceptslights", true);
            let ov = cc.flag("boverridelightmapres", false).then(|| cc.num("overriddenlightmapres", 64.0) as i32);
            out.instances.push(Instance {
                level: level_index,
                export: c,
                owner: a,
                name: ed.pkg.object_name(PackageIndex::export(c)),
                mesh: key,
                world: mul(&cm, &am),
                casts_shadow: casts,
                static_lighting: static_owner && accepts,
                override_resolution: ov,
                channels: primitive_channels(&cc, static_owner),
                two_sided: cc.member_flag("lightmasssettings", "busetwosidedlighting", false),
            });
        }
    }
    out
}

/// A mesh of another package (found by the caller): parse it with its defaults.
pub fn mesh_from(ed: &mut Editor, db: &ClassDb, i: usize) -> Option<MeshData> {
    let mc = chain(ed, db, i);
    ed.with_object(i, |_, o| match &o.native {
        Native::StaticMesh(m) => mesh_data(m, &mc),
        _ => None,
    })
    .ok()
    .flatten()
}
