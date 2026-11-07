//! What pawns and shots collide with in a map, and giving volumes a shape
//! (Studio `map.collision`, `map.brush`, `map.setBrush`, `map.addActor
//! {brush}`).
//!
//! A volume (any `Brush` actor with a `BrushComponent`) collides through
//! `BrushComponent.BrushAggGeom` (`UBrushComponent::LineCheck`/`PointCheck`
//! → `FKAggregateGeom`, confirmed in the 8.2 server: nothing else is read,
//! `BuildSimpleBrushCollision` is only called by `csgPrepMovingBrush`).
//! Cooked blocking volumes keep only that (the cooker stripped `Brush` once
//! PhysX data was cached in `CachedPhysBrushData`); `IsValidComponent` wants
//! `Brush` or cached hulls, so a shape written here is a `Brush` model
//! (polys + BSP, `brush::BrushModel`), the hulls built from it, and an empty
//! PhysX cache: what an editor-saved, uncooked volume holds.
//!
//! Static meshes collide through their `StaticMesh`: simple collision
//! (`BodySetup.AggGeom`: boxes, spheres, sphyls, convex hulls) when
//! `UseSimpleBoxCollision` (pawn sweeps) / `UseSimpleLineCollision` (traces)
//! ask for it, else the kDOP tree over the render triangles
//! (`UStaticMeshComponent::LineCheck`, UnStaticMeshCollision.cpp).

use serde_json::{json, Map, Value as J};
use tempest_upk::io::{Error, Guid, PackageIndex, Result, Writer};
use tempest_upk::{ObjectExport, Package};

use crate::brush::{self, BrushModel, ConvexElem, EdPoly, Shape, V};
use crate::edit::{default_value, new_property, Editor};
use crate::level::{BspSurf, LightmassPrimitiveSettings, Model, Poly, Polys};
use crate::mesh::{BoxSphereBounds, BulkArray};
use crate::object::{none_name, write_parsed, Native, Object, Parsed};
use crate::reflect::{CdoValue, ClassDb};
use crate::value::{Property, StructValue, Value};
use tempest_unrealscript::objects::PropertyKind;

// ----- reading values -------------------------------------------------------------

fn member<'a>(v: &'a Value, pkg: &Package, name: &str) -> Option<&'a Value> {
    match v {
        Value::Struct(StructValue::Binary(ms)) => ms.iter().find(|m| m.name.eq_ignore_ascii_case(name)).and_then(|m| m.elems.first()),
        Value::Struct(StructValue::Tagged(ps)) => ps.iter().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(name)).map(|p| &p.value),
        _ => None,
    }
}

fn num(v: Option<&Value>) -> f32 {
    match v {
        Some(Value::Float(f)) => *f,
        Some(Value::Int(i)) => *i as f32,
        Some(Value::Byte(b)) => *b as f32,
        _ => 0.0,
    }
}

fn vec3(v: Option<&Value>, pkg: &Package) -> V {
    match v {
        Some(v) => [num(member(v, pkg, "X")), num(member(v, pkg, "Y")), num(member(v, pkg, "Z"))],
        None => [0.0; 3],
    }
}

fn plane4(v: &Value, pkg: &Package) -> [f32; 4] {
    [num(member(v, pkg, "X")), num(member(v, pkg, "Y")), num(member(v, pkg, "Z")), num(member(v, pkg, "W"))]
}

fn array(v: Option<&Value>) -> &[Value] {
    match v {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

/// The convex elements of an `FKAggregateGeom` value.
pub fn read_convex(agg: Option<&Value>, pkg: &Package) -> Vec<ConvexElem> {
    let Some(agg) = agg else { return Vec::new() };
    array(member(agg, pkg, "ConvexElems"))
        .iter()
        .map(|e| {
            let b = member(e, pkg, "ElemBox");
            ConvexElem {
                vertex_data: array(member(e, pkg, "VertexData")).iter().map(|v| vec3(Some(v), pkg)).collect(),
                permuted_vertex_data: array(member(e, pkg, "PermutedVertexData")).iter().map(|v| plane4(v, pkg)).collect(),
                face_tri_data: array(member(e, pkg, "FaceTriData")).iter().map(|v| num(Some(v)) as i32).collect(),
                edge_directions: array(member(e, pkg, "EdgeDirections")).iter().map(|v| vec3(Some(v), pkg)).collect(),
                face_normal_directions: array(member(e, pkg, "FaceNormalDirections")).iter().map(|v| vec3(Some(v), pkg)).collect(),
                face_plane_data: array(member(e, pkg, "FacePlaneData")).iter().map(|v| plane4(v, pkg)).collect(),
                elem_box: match b {
                    Some(b) => (vec3(member(b, pkg, "Min"), pkg), vec3(member(b, pkg, "Max"), pkg), num(member(b, pkg, "IsValid")) != 0.0 || matches!(member(b, pkg, "IsValid"), Some(Value::Bool(true)))),
                    None => ([0.0; 3], [0.0; 3], false),
                },
            }
        })
        .collect()
}

/// Unreal → the Studio's view space (`(x, z, y) / 100`, as `map`).
fn vp(v: V) -> [f64; 3] {
    [v[0] as f64 / 100.0, v[2] as f64 / 100.0, v[1] as f64 / 100.0]
}

/// Convex elements as one mesh in view space (positions, indices).
fn convex_mesh(elems: &[ConvexElem]) -> (Vec<f64>, Vec<u32>) {
    let mut pos = Vec::new();
    let mut idx = Vec::new();
    for e in elems {
        let base = (pos.len() / 3) as u32;
        for v in &e.vertex_data {
            pos.extend(vp(*v));
        }
        // `map::brush_geometry`'s winding (the y/z swap mirrors it).
        for t in e.face_tri_data.chunks(3).filter(|t| t.len() == 3) {
            idx.extend([base + t[0] as u32, base + t[2] as u32, base + t[1] as u32]);
        }
    }
    (pos, idx)
}

/// An `FMatrix` value (XPlane…WPlane) in view space.
fn matrix_view(v: Option<&Value>, pkg: &Package) -> Vec<f64> {
    let mut m = crate::map::IDENTITY;
    if let Some(v) = v {
        for (r, name) in ["XPlane", "YPlane", "ZPlane", "WPlane"].iter().enumerate() {
            if let Some(p) = member(v, pkg, name) {
                let p = plane4(p, pkg);
                m[r] = [p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64];
            }
        }
    }
    crate::map::to_view(&m)
}

/// Every element of an `FKAggregateGeom`, for drawing: convex hulls as one
/// mesh, boxes (`X/Y/Z` full lengths), spheres, sphyls (capsules along Z:
/// `Length` between the cap centres) with their `TM` in view space.
pub fn agg_json(agg: Option<&Value>, pkg: &Package) -> J {
    let Some(a) = agg else { return json!({}) };
    let convex = read_convex(agg, pkg);
    let (positions, indices) = convex_mesh(&convex);
    let boxes: Vec<J> = array(member(a, pkg, "BoxElems")).iter().map(|e| json!({ "matrix": matrix_view(member(e, pkg, "TM"), pkg), "x": num(member(e, pkg, "X")), "y": num(member(e, pkg, "Y")), "z": num(member(e, pkg, "Z")) })).collect();
    let spheres: Vec<J> = array(member(a, pkg, "SphereElems")).iter().map(|e| json!({ "matrix": matrix_view(member(e, pkg, "TM"), pkg), "radius": num(member(e, pkg, "Radius")) })).collect();
    let sphyls: Vec<J> = array(member(a, pkg, "SphylElems")).iter().map(|e| json!({ "matrix": matrix_view(member(e, pkg, "TM"), pkg), "radius": num(member(e, pkg, "Radius")), "length": num(member(e, pkg, "Length")) })).collect();
    json!({ "convexCount": convex.len(), "positions": positions, "indices": indices, "boxes": boxes, "spheres": spheres, "sphyls": sphyls })
}

// ----- collision flags ---------------------------------------------------------

/// A bool property as the engine sees it: the object's own value, else its
/// archetype's (a component's template in a script package, recursively),
/// else the class defaults.
pub fn bool_of(ed: &mut Editor, i: usize, name: &str) -> Option<bool> {
    let own = ed.with_object(i, |pkg, o| o.property(name, pkg).map(|p| p.value.clone())).ok().flatten();
    if let Some(Value::Bool(b)) = own {
        return Some(b);
    }
    let db = ed.db;
    let arch = ed.pkg.exports.get(i).map(|e| e.archetype).unwrap_or_default();
    if let Some(a) = arch.export_index() {
        if a != i {
            return bool_of(ed, a, name);
        }
    }
    if arch.is_import() {
        let full = ed.pkg.object_path(arch);
        let (root, rest) = full.split_once('.').unwrap_or((full.as_str(), ""));
        if let Some(slot) = db.ctx.slot_of(root) {
            if let Some(b) = script_bool(db, db.ctx.find_export(slot, rest), name) {
                return Some(b);
            }
        }
    }
    let class = db.class(&ed.pkg.class_name_of(PackageIndex::export(i)))?;
    match db.cdo_scalar(class, name) {
        Some(CdoValue::Bool(b)) => Some(b),
        _ => None,
    }
}

/// A bool of an object of the script packages, following its archetypes.
fn script_bool(db: &ClassDb, mut r: Option<tempest_unrealscript::context::ObjRef>, name: &str) -> Option<bool> {
    for _ in 0..16 {
        let o = r?;
        let lp = &db.ctx.packages[o.pkg];
        // Component templates are not parsed with the classes: read their
        // tags from the payload.
        let parsed;
        let props = match db.ctx.object(o) {
            Some(obj) => &obj.header.properties,
            None => {
                use tempest_unrealscript::objects::{read_object_header, HeaderOptions};
                let mut rd = tempest_upk::io::Reader::new(lp.pkg.export_data.get(o.export)?);
                let is_component = db.class_is_a(&lp.pkg.class_name_of(PackageIndex::export(o.export)), "Component");
                parsed = read_object_header(&mut rd, &lp.pkg, HeaderOptions { object_flags: lp.pkg.exports[o.export].object_flags, is_component, component_is_template: is_component, ..Default::default() }).ok()?;
                &parsed.properties
            }
        };
        if let Some(tp) = props.iter().find(|tp| tp.array_index == 0 && lp.name_of(tp.name).eq_ignore_ascii_case(name)) {
            return Some(tp.bool_val.unwrap_or(0) != 0);
        }
        let arch = lp.pkg.exports[o.export].archetype;
        if arch.is_null() {
            return None;
        }
        r = db.ctx.resolve(o.pkg, arch);
    }
    None
}

const COMPONENT_FLAGS: [&str; 6] = ["CollideActors", "BlockActors", "BlockZeroExtent", "BlockNonZeroExtent", "BlockRigidBody", "bDisableAllRigidBody"];
const ACTOR_FLAGS: [&str; 3] = ["bCollideActors", "bBlockActors", "bPathColliding"];

/// The collision switches of a component and of its actor, and what they
/// add up to: `blocks` "all" (pawns and shots), "pawns" (non-zero-extent
/// sweeps only), "shots" (zero-extent traces only) or "none".
pub fn flags_json(ed: &mut Editor, actor: usize, comp: Option<usize>) -> J {
    let mut m = Map::new();
    for f in ACTOR_FLAGS {
        m.insert(f.to_string(), json!(bool_of(ed, actor, f).unwrap_or(false)));
    }
    if let Some(c) = comp {
        for f in COMPONENT_FLAGS {
            m.insert(f.to_string(), json!(bool_of(ed, c, f).unwrap_or(false)));
        }
    }
    let g = |k: &str| m.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let collide = g("bCollideActors") && g("CollideActors");
    let pawns = collide && g("bBlockActors") && g("BlockActors") && g("BlockNonZeroExtent");
    let shots = collide && g("BlockZeroExtent");
    let blocks = match (pawns, shots) {
        (true, true) => "all",
        (true, false) => "pawns",
        (false, true) => "shots",
        _ => "none",
    };
    m.insert("blocks".into(), json!(blocks));
    J::Object(m)
}

/// A static mesh's collision: its `UseSimple*` switches and its
/// `BodySetup.AggGeom` (in the mesh's space).
pub fn mesh_collision(ed: &mut Editor, mesh: usize) -> J {
    let mut m = Map::new();
    for f in ["UseSimpleBoxCollision", "UseSimpleLineCollision", "UseSimpleRigidBodyCollision"] {
        // Intrinsic properties (`UStaticMesh::StaticConstructor`), TRUE by
        // default (`InitializeIntrinsicPropertyValues`); no 8.1 package stores
        // them, so every mesh wants simple collision.
        m.insert(f.to_string(), json!(bool_of(ed, mesh, f).unwrap_or(true)));
    }
    let body = ed
        .with_object(mesh, |pkg, o| match o.property("BodySetup", pkg).map(|p| &p.value) {
            Some(Value::Object(i)) => i.export_index(),
            _ => None,
        })
        .ok()
        .flatten();
    let simple = match body {
        Some(b) => ed.with_object(b, |pkg, o| agg_json(o.property("AggGeom", pkg).map(|p| &p.value), pkg)).unwrap_or(J::Null),
        None => J::Null,
    };
    m.insert("bodySetup".into(), json!(body.is_some()));
    // What sweeps (pawns) and traces (shots) test: `simple` (the body setup;
    // nothing when there is none), or `complex` (the kDOP tree = the render
    // triangles of LOD 0).
    let g = |m: &Map<String, J>, k: &str| m.get(k).and_then(|v| v.as_bool()).unwrap_or(true);
    let pick = |simple: bool| if !simple { "complex" } else if body.is_some() { "simple" } else { "none" };
    let (pawns, shots) = (pick(g(&m, "UseSimpleBoxCollision")), pick(g(&m, "UseSimpleLineCollision")));
    m.insert("pawns".into(), json!(pawns));
    m.insert("shots".into(), json!(shots));
    m.insert("simple".into(), simple);
    J::Object(m)
}

// ----- the shape of a volume ------------------------------------------------------

fn f(p: &serde_json::Value, k: &str, d: f32) -> f32 {
    p.get(k).and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(d)
}
fn b(p: &serde_json::Value, k: &str, d: bool) -> bool {
    p.get(k).and_then(|v| v.as_bool()).unwrap_or(d)
}
fn points(v: Option<&serde_json::Value>) -> Vec<V> {
    v.and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|p| p.as_array()).filter(|p| p.len() == 3).map(|p| [p[0].as_f64().unwrap_or(0.0) as f32, p[1].as_f64().unwrap_or(0.0) as f32, p[2].as_f64().unwrap_or(0.0) as f32]).collect()).unwrap_or_default()
}

/// A builder from the Studio's JSON (`{kind, …}`, Unreal units, the
/// editor's parameter names in camel case and its defaults). `mesh` is
/// resolved by the caller into `hull` points.
pub fn shape_from_json(p: &serde_json::Value) -> std::result::Result<Shape, String> {
    let sides = p.get("sides").and_then(|v| v.as_u64()).unwrap_or(8) as u32;
    Ok(match p.get("kind").and_then(|v| v.as_str()).unwrap_or("box") {
        "box" | "cube" => Shape::Box { x: f(p, "x", 256.0), y: f(p, "y", 256.0), z: f(p, "z", 256.0), hollow: b(p, "hollow", false), wall: f(p, "wallThickness", 16.0), tessellated: b(p, "tessellated", false) },
        "cylinder" => Shape::Cylinder { z: f(p, "z", 256.0), outer: f(p, "outerRadius", 512.0), inner: f(p, "innerRadius", 384.0), sides, align_to_side: b(p, "alignToSide", true), hollow: b(p, "hollow", false) },
        "cone" => Shape::Cone { z: f(p, "z", 256.0), cap_z: f(p, "capZ", 256.0), outer: f(p, "outerRadius", 512.0), inner: f(p, "innerRadius", 384.0), sides, align_to_side: b(p, "alignToSide", true), hollow: b(p, "hollow", false) },
        "sphere" | "tetrahedron" => Shape::Sphere { radius: f(p, "radius", 256.0), extrapolation: p.get("sphereExtrapolation").and_then(|v| v.as_u64()).unwrap_or(2) as u32 },
        "hull" | "points" => Shape::Hull { points: points(p.get("points")) },
        "polys" => Shape::Polys { polys: p.get("polys").and_then(|v| v.as_array()).map(|a| a.iter().map(|q| points(Some(q))).collect()).unwrap_or_default() },
        k => return Err(format!("unknown brush kind {k}")),
    })
}

/// A volume's current shape: its brush polys (the editable form) when it
/// has a `Brush`, else the faces of its convex elements, plus the elements.
pub fn brush_json(ed: &mut Editor, actor: usize) -> Result<J> {
    let comp = component_of(ed, actor)?;
    let elems = ed.with_object(comp, |pkg, o| read_convex(o.property("BrushAggGeom", pkg).map(|p| &p.value), pkg))?;
    let model = ed.with_object(actor, |pkg, o| match o.property("Brush", pkg).map(|p| &p.value) {
        Some(Value::Object(i)) => i.export_index(),
        _ => None,
    })?;
    let polys = model.and_then(|m| ed.with_object(m, |_, o| if let Native::Model(m) = &o.native { m.polys.export_index() } else { None }).ok().flatten());
    let mut out: Vec<Vec<V>> = Vec::new();
    if let Some(p) = polys {
        out = ed.with_object(p, |_, o| if let Native::Polys(p) = &o.native { p.elements.iter().map(|e| e.vertices.clone()).collect() } else { Vec::new() })?;
    }
    let from_brush = !out.is_empty();
    if !from_brush {
        for e in &elems {
            if let Some((verts, faces)) = brush::hull_faces(&e.vertex_data) {
                out.extend(faces.iter().map(|f| f.iter().map(|&k| verts[k]).collect::<Vec<_>>()));
            }
        }
    }
    let hulls: Vec<J> = elems.iter().map(|e| json!({ "vertices": e.vertex_data, "planes": e.face_plane_data.len(), "box": [e.elem_box.0, e.elem_box.1] })).collect();
    Ok(json!({ "component": comp, "model": model, "polys": out, "fromBrush": from_brush, "hulls": hulls }))
}

/// The actor's `BrushComponent`.
fn component_of(ed: &mut Editor, actor: usize) -> Result<usize> {
    let c = ed.with_object(actor, |pkg, o| match o.property("BrushComponent", pkg).map(|p| &p.value) {
        Some(Value::Object(i)) => i.export_index(),
        _ => None,
    })?;
    c.or_else(|| ed.children(actor).into_iter().find(|&c| ed.db.class_is_a(&ed.pkg.class_name_of(PackageIndex::export(c)), "BrushComponent"))).ok_or_else(|| Error::Format("this actor has no brush component".into()))
}

/// A name `<base>_<n>` free under `outer`.
fn fresh(ed: &Editor, outer: PackageIndex, base: &str) -> String {
    let taken = |n: &str| ed.pkg.exports.iter().enumerate().any(|(j, e)| e.outer == outer && ed.pkg.object_name(PackageIndex::export(j)).eq_ignore_ascii_case(n));
    (0..).map(|k| format!("{base}_{k}")).find(|n| !taken(n)).unwrap()
}

fn class_import(pkg: &mut Package, name: &str) -> PackageIndex {
    let engine = pkg.add_import("Core", "Package", PackageIndex::NONE, "Engine");
    pkg.add_import("Core", "Class", engine, name)
}

fn bare(class_name: &str, native: Native, db: &ClassDb) -> Object {
    Object { class: db.class(class_name), class_name: class_name.to_string(), native_prefix: Vec::new(), state_frame: None, component: None, net_index: -1, properties: Vec::new(), native, is_class: false }
}

/// `FLightmassPrimitiveSettings(EC_NativeConstructor)`.
fn lightmass_default() -> LightmassPrimitiveSettings {
    LightmassPrimitiveSettings { fully_occluded_samples_fraction: 1.0, emissive_light_falloff_exponent: 2.0, emissive_boost: 1.0, diffuse_boost: 1.0, specular_boost: 1.0, ..Default::default() }
}

fn bulk<T>(size: usize, items: &[T], put: impl Fn(&mut Writer, &T)) -> BulkArray {
    let mut w = Writer::new();
    for x in items {
        put(&mut w, x);
    }
    BulkArray { elem_size: size as i32, count: items.len() as i32, data: w.into_inner() }
}

fn put_v(w: &mut Writer, v: &V) {
    for x in v {
        w.f32(*x);
    }
}

/// The `UModel` native part of a rebuilt brush (`UModel::Serialize`; FVert
/// shadow coordinates, uninitialised editor memory in shipped maps, zero).
fn model_native(bm: &BrushModel, own: PackageIndex, polys: PackageIndex) -> Model {
    let nodes = bulk(64, &bm.nodes, |w, n| {
        for x in n.plane {
            w.f32(x);
        }
        w.i32(n.i_vert_pool);
        w.i32(n.i_surf);
        w.i32(0); // iVertexIndex
        w.u16(0); // ComponentIndex
        w.u16(0); // ComponentNodeIndex
        w.i32(0); // ComponentElementIndex
        w.i32(n.i_back);
        w.i32(n.i_front);
        w.i32(n.i_plane);
        w.i32(n.i_collision_bound);
        w.u8(n.i_zone[0]);
        w.u8(n.i_zone[1]);
        w.u8(n.num_vertices);
        w.u8(n.node_flags);
        w.i32(n.i_leaf[0]);
        w.i32(n.i_leaf[1]);
    });
    let verts = bulk(24, &bm.verts, |w, v| {
        w.i32(v.0);
        w.i32(v.1);
        for _ in 0..4 {
            w.f32(0.0);
        }
    });
    let surfs = bm
        .surfs
        .iter()
        .map(|s| BspSurf { material: PackageIndex::NONE, poly_flags: s.poly_flags, p_base: s.p_base, v_normal: s.v_normal, v_texture_u: s.v_texture_u, v_texture_v: s.v_texture_v, i_brush_poly: s.i_brush_poly, actor: PackageIndex::NONE, plane: s.plane, shadow_map_scale: 32.0, lighting_channels: 3, i_lightmass_index: 0 })
        .collect();
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let h = |k: u128| ((seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(k * 0x5851_F42D)) >> 32) as u32;
    Model {
        bounds: BoxSphereBounds { origin: bm.bounds.0, box_extent: bm.bounds.1, sphere_radius: bm.bounds.2 },
        vectors: bulk(12, &bm.vectors, put_v),
        points: bulk(12, &bm.points, put_v),
        nodes,
        surfs_owner: own,
        surfs,
        verts,
        num_shared_sides: 4,
        zones: Vec::new(),
        polys,
        leaf_hulls: bulk(4, &bm.leaf_hulls, |w, x| w.i32(*x)),
        leaves: BulkArray { elem_size: 4, count: 0, data: Vec::new() },
        root_outside: bm.root_outside as i32,
        linked: 1,
        portal_nodes: BulkArray { elem_size: 4, count: 0, data: Vec::new() },
        num_unique_vertices: bm.points.len() as i32,
        vertex_buffer: BulkArray { elem_size: 36, count: 0, data: Vec::new() },
        // `appCreateGuid`.
        lighting_guid: Guid { a: h(1), b: h(2), c: h(3), d: h(4) },
        lightmass_settings: vec![lightmass_default()],
    }
}

fn polys_native(pkg: &mut Package, polys: &[EdPoly], own: PackageIndex) -> Polys {
    let none = pkg.intern_fname("None");
    let elements = polys
        .iter()
        .map(|p| Poly {
            base: p.base,
            normal: p.normal,
            texture_u: p.texture_u,
            texture_v: p.texture_v,
            vertices: p.vertices.clone(),
            poly_flags: p.poly_flags,
            actor: PackageIndex::NONE,
            item_name: if p.item_name.is_empty() { none } else { pkg.intern_fname(&p.item_name) },
            material: PackageIndex::NONE,
            i_link: p.i_link,
            // `csgCopyBrush`.
            i_brush_poly: -1,
            shadow_map_scale: 32.0,
            lighting_channels: 3,
            lightmass_settings: lightmass_default(),
            ruleset_variation: none,
        })
        .collect::<Vec<_>>();
    Polys { db_max: elements.len() as i32, owner: own, elements }
}

/// Set a member of a struct value by name: a binary struct's member, or a
/// tagged struct's property (created from the struct's reflection).
fn put_member(db: &ClassDb, pkg: &mut Package, st: &str, v: &mut Value, name: &str, val: Value) -> Result<()> {
    match v {
        Value::Struct(StructValue::Binary(ms)) => {
            let m = ms.iter_mut().find(|m| m.name.eq_ignore_ascii_case(name)).ok_or_else(|| Error::Format(format!("{st} has no {name}")))?;
            m.elems = vec![val];
        }
        Value::Struct(StructValue::Tagged(ps)) => {
            let scope = db.struct_named(st);
            let (mut p, _) = new_property(db, pkg, scope, &name.to_ascii_lowercase(), 0)?;
            p.value = val;
            ps.retain(|q| !pkg.fname_to_string(q.name).eq_ignore_ascii_case(name));
            ps.push(p);
        }
        _ => return Err(Error::Format(format!("{st} is not a struct"))),
    }
    Ok(())
}

fn struct_default(db: &ClassDb, pkg: &mut Package, st: &str) -> Result<Value> {
    default_value(db, pkg, PropertyKind::Struct, db.struct_named(st), 0)
}

fn vector_value(db: &ClassDb, pkg: &mut Package, v: V) -> Result<Value> {
    let mut x = struct_default(db, pkg, "Vector")?;
    for (k, n) in ["X", "Y", "Z"].iter().enumerate() {
        put_member(db, pkg, "Vector", &mut x, n, Value::Float(v[k]))?;
    }
    Ok(x)
}

fn plane_value(db: &ClassDb, pkg: &mut Package, p: [f32; 4]) -> Result<Value> {
    let mut x = struct_default(db, pkg, "Plane")?;
    for (k, n) in ["X", "Y", "Z", "W"].iter().enumerate() {
        put_member(db, pkg, "Plane", &mut x, n, Value::Float(p[k]))?;
    }
    Ok(x)
}

/// `BrushAggGeom` holding `elems` (members in declaration order; empty
/// arrays are left out like the cooker's delta serialisation does).
fn agg_value(db: &ClassDb, pkg: &mut Package, scope: Option<tempest_unrealscript::context::ObjRef>, elems: &[ConvexElem]) -> Result<Property> {
    let (mut prop, _) = new_property(db, pkg, scope, "brushagggeom", 0)?;
    let ks = db.struct_named("KConvexElem");
    let mut list = Vec::new();
    for e in elems {
        let mut props: Vec<Property> = Vec::new();
        let mut push = |pkg: &mut Package, name: &str, value: Value| -> Result<()> {
            if matches!(&value, Value::Array(a) if a.is_empty()) {
                return Ok(());
            }
            let (mut p, _) = new_property(db, pkg, ks, name, 0)?;
            p.value = value;
            props.push(p);
            Ok(())
        };
        let vs = e.vertex_data.iter().map(|v| vector_value(db, pkg, *v)).collect::<Result<Vec<_>>>()?;
        push(pkg, "vertexdata", Value::Array(vs))?;
        let pv = e.permuted_vertex_data.iter().map(|p| plane_value(db, pkg, *p)).collect::<Result<Vec<_>>>()?;
        push(pkg, "permutedvertexdata", Value::Array(pv))?;
        push(pkg, "facetridata", Value::Array(e.face_tri_data.iter().map(|&i| Value::Int(i)).collect()))?;
        let ed_ = e.edge_directions.iter().map(|v| vector_value(db, pkg, *v)).collect::<Result<Vec<_>>>()?;
        push(pkg, "edgedirections", Value::Array(ed_))?;
        let fnd = e.face_normal_directions.iter().map(|v| vector_value(db, pkg, *v)).collect::<Result<Vec<_>>>()?;
        push(pkg, "facenormaldirections", Value::Array(fnd))?;
        let fp = e.face_plane_data.iter().map(|p| plane_value(db, pkg, *p)).collect::<Result<Vec<_>>>()?;
        push(pkg, "faceplanedata", Value::Array(fp))?;
        let mut bx = struct_default(db, pkg, "Box")?;
        let min = vector_value(db, pkg, e.elem_box.0)?;
        let max = vector_value(db, pkg, e.elem_box.1)?;
        put_member(db, pkg, "Box", &mut bx, "Min", min)?;
        put_member(db, pkg, "Box", &mut bx, "Max", max)?;
        put_member(db, pkg, "Box", &mut bx, "IsValid", Value::Byte(e.elem_box.2 as u8))?;
        push(pkg, "elembox", bx)?;
        list.push(Value::Struct(StructValue::Tagged(props)));
    }
    let ka = db.struct_named("KAggregateGeom");
    let (mut convex, _) = new_property(db, pkg, ka, "convexelems", 0)?;
    convex.value = Value::Array(list);
    prop.value = Value::Struct(StructValue::Tagged(if elems.is_empty() { Vec::new() } else { vec![convex] }));
    Ok(prop)
}

/// Move the properties named `first` (lower case) to the front, in that order.
fn lead(pkg: &Package, props: &mut [Property], first: &[&str]) {
    let rank = |p: &Property| first.iter().position(|n| pkg.fname_to_string(p.name).eq_ignore_ascii_case(n)).unwrap_or(first.len());
    props.sort_by_key(rank);
}

/// Give volume `actor` the shape `shape` (in its own space, before
/// `DrawScale3D`): brush polys (`UBrushBuilder::EndBrush`), the brush BSP
/// (`RebuildBrush`) in its `Brush` model (made under the actor when it has
/// none, as `csgCopyBrush` does), `BrushAggGeom` from `KModelToHulls`, and
/// the component's PhysX cache emptied (its hulls were for the old shape;
/// the game cooks the new one from `Brush` when it wants rigid bodies).
pub fn set_brush(ed: &mut Editor, actor: usize, shape: &Shape) -> std::result::Result<J, String> {
    let mut polys = brush::build(shape)?;
    let bm = BrushModel::rebuild(&mut polys);
    let elems = bm.to_hulls();
    if elems.is_empty() {
        return Err("the brush has no solid volume".into());
    }
    let r: Result<J> = (|| {
        let db = ed.db;
        let comp = component_of(ed, actor)?;
        let actor_idx = PackageIndex::export(actor);
        let flags = tempest_upk::flags::rf::TRANSACTIONAL | tempest_upk::flags::rf::LOAD_FOR_CLIENT | tempest_upk::flags::rf::LOAD_FOR_SERVER | tempest_upk::flags::rf::LOAD_FOR_EDIT;
        let existing = ed.with_object(actor, |pkg, o| match o.property("Brush", pkg).map(|p| &p.value) {
            Some(Value::Object(i)) => i.export_index(),
            _ => None,
        })?;
        let existing_polys = existing.and_then(|m| ed.with_object(m, |_, o| if let Native::Model(m) = &o.native { m.polys.export_index() } else { None }).ok().flatten());
        let model = match existing {
            Some(m) => m,
            None => {
                let class = class_import(&mut ed.pkg, "Model");
                let name = fresh(ed, actor_idx, "model");
                let e = ObjectExport { class, outer: actor_idx, object_name: ed.pkg.intern_fname(&name), object_flags: flags, ..Default::default() };
                let data = write_parsed(&Parsed::Object(bare("Model", Native::Model(Box::default()), db)), ed.layout(), none_name(&ed.pkg));
                ed.add_export(e, data)
            }
        };
        let model_idx = PackageIndex::export(model);
        let polys_i = match existing_polys {
            Some(p) => p,
            None => {
                let class = class_import(&mut ed.pkg, "Polys");
                let name = fresh(ed, model_idx, "polys");
                let e = ObjectExport { class, outer: model_idx, object_name: ed.pkg.intern_fname(&name), object_flags: flags, ..Default::default() };
                let data = write_parsed(&Parsed::Object(bare("Polys", Native::Polys(Polys::default()), db)), ed.layout(), none_name(&ed.pkg));
                ed.add_export(e, data)
            }
        };
        let polys_idx = PackageIndex::export(polys_i);
        let pn = polys_native(&mut ed.pkg, &polys, polys_idx);
        let mut po = ed.object(polys_i)?.clone();
        po.native = Native::Polys(pn);
        ed.replace_object(polys_i, po)?;
        let mut mo = ed.object(model)?.clone();
        let keep_guid = match &mo.native {
            Native::Model(m) if m.lighting_guid != Guid::default() => Some(m.lighting_guid),
            _ => None,
        };
        let mut mn = model_native(&bm, model_idx, polys_idx);
        if let Some(g) = keep_guid {
            mn.lighting_guid = g;
        }
        mo.native = Native::Model(Box::new(mn));
        ed.replace_object(model, mo)?;
        let text = crate::kismet::object_text(&ed.pkg, model);
        ed.set(actor, "Brush", &text)?;
        ed.set(comp, "Brush", &text)?;
        let mut ao = ed.object(actor)?.clone();
        lead(&ed.pkg, &mut ao.properties, &["brush"]);
        ed.replace_object(actor, ao)?;
        let class = ed.object(comp)?.class;
        let agg = agg_value(db, &mut ed.pkg, class, &elems)?;
        let mut co = ed.object(comp)?.clone();
        co.properties.retain(|p| !ed.pkg.fname_to_string(p.name).eq_ignore_ascii_case("BrushAggGeom"));
        co.properties.push(agg);
        // `Brush`, `BrushAggGeom` first: `UBrushComponent`'s own properties
        // come before its parents' in cooked components.
        lead(&ed.pkg, &mut co.properties, &["brush", "brushagggeom"]);
        if let Native::BrushComponent(bc) = &mut co.native {
            bc.cached_phys_brush_data.clear();
        }
        ed.replace_object(comp, co)?;
        Ok(json!({ "model": model, "polys": polys.len(), "hulls": elems.len(), "nodes": bm.nodes.len() }))
    })();
    r.map_err(|e| e.to_string())
}
