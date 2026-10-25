//! Resolving the textures a material (instance) samples, for exports that
//! want textured models.
//!
//! A `MaterialInstance` lists overrides in `TextureParameterValues`
//! (`ParameterName`, `ParameterValue`) and points at a `Parent`; the base
//! `Material` holds its defaults in `MaterialExpressionTextureSample*`
//! sub-objects (`ParameterName`, `Texture`). Slots are classified by Hi-Rez's
//! parameter names (`diffuse`, `normal`, `specular`, `emissive …`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tempest_unrealscript::objects::Layout;
use tempest_upk::io::{PackageIndex, Result};
use tempest_upk::{Package, PackageFile};

use crate::edit::Editor;
use crate::object::{package_layout, parse_export, Object, Parsed};
use crate::value::{tag_type_name, Member};
use tempest_unrealscript::objects::PropertyKind;
use crate::reflect::ClassDb;
use crate::texcache::TextureSource;
use crate::texture_ops;
use crate::value::{Property, StructValue, Value};

/// A package opened for cross-references, by file stem.
pub struct LoadedPackage {
    pub name: String,
    pub pkg: Package,
    pub layout: Layout,
}

/// Packages of a cooked directory, opened on demand for import resolution.
pub struct PackageCache<'a> {
    pub db: &'a ClassDb,
    pub dir: PathBuf,
    /// Boxed so a package's address is stable across map growth (callers
    /// hold references across further lookups).
    loaded: HashMap<String, Option<Box<LoadedPackage>>>,
}

impl<'a> PackageCache<'a> {
    pub fn new(db: &'a ClassDb, dir: impl AsRef<Path>) -> PackageCache<'a> {
        PackageCache { db, dir: dir.as_ref().to_path_buf(), loaded: HashMap::new() }
    }

    /// Insert an already loaded package (the one being exported).
    pub fn insert(&mut self, name: &str, pkg: Package) {
        let layout = package_layout(&pkg);
        self.loaded.insert(name.to_ascii_lowercase(), Some(Box::new(LoadedPackage { name: name.to_string(), pkg, layout })));
    }

    pub fn get(&mut self, name: &str) -> Option<&LoadedPackage> {
        let key = name.to_ascii_lowercase();
        if !self.loaded.contains_key(&key) {
            let mut found = None;
            if let Ok(rd) = std::fs::read_dir(&self.dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    let stem = p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                    let ext = p.extension().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                    if stem == key && (ext == "upk" || ext == "u") {
                        if let Ok(pf) = PackageFile::open(&p) {
                            if let Ok(pkg) = pf.load() {
                                let layout = package_layout(&pkg);
                                found = Some(Box::new(LoadedPackage { name: p.file_stem().unwrap().to_string_lossy().to_string(), pkg, layout }));
                            }
                        }
                        break;
                    }
                }
            }
            self.loaded.insert(key.clone(), found);
        }
        self.loaded.get(&key).and_then(|o| o.as_deref())
    }

    /// Resolve a reference from package `from` to (package name, export index).
    pub fn resolve(&mut self, from: &str, idx: PackageIndex) -> Option<(String, usize)> {
        if let Some(e) = idx.export_index() {
            return Some((from.to_string(), e));
        }
        if !idx.is_import() {
            return None;
        }
        let (root, path) = {
            let lp = self.get(from)?;
            let root = lp.pkg.outermost_name(idx);
            let full = lp.pkg.object_path(idx);
            let path = full.split_once('.').map(|x| x.1).unwrap_or("").to_string();
            (root, path)
        };
        if path.is_empty() {
            return None;
        }
        let target = self.get(&root)?;
        let e = target.pkg.find_export(&path, None)?;
        Some((target.name.clone(), e))
    }

    /// Parse an export of a loaded package.
    pub fn object(&mut self, pkg_name: &str, i: usize) -> Option<Object> {
        let db = self.db;
        let lp = self.get(pkg_name)?;
        match parse_export(db, &lp.pkg, lp.layout, i, &lp.pkg.export_data[i]) {
            Parsed::Object(o) => Some(o),
            _ => None,
        }
    }
}

/// Which material slot a texture parameter feeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureSlot {
    BaseColor,
    Normal,
    Specular,
    Emissive,
    Mask,
    Other,
}

/// `texture`: the texture's object name, for the game's naming scheme
/// (`_diff`, `_nrm`, `_spc`, `_msk`, mask templates): Hi-Rez's master
/// material samples "emissive" through a mask template, so a mask-looking
/// texture is a mask whatever its parameter is called.
pub fn classify(param: &str, texture: &str) -> TextureSlot {
    let p = param.to_ascii_lowercase();
    let t = texture.rsplit('.').next().unwrap_or(texture).to_ascii_lowercase();
    // Masks first: "emissive mask", "diffuse mask" and the like are masks.
    if p.contains("mask") || p.contains("msk") || t.contains("_msk") || t.contains("mask") || t.contains("template") {
        TextureSlot::Mask
    } else if p.contains("normal") || p.contains("nrm") || p.contains("bump") {
        TextureSlot::Normal
    } else if p.contains("diffuse") || p.contains("diff") || p.contains("albedo") || p.contains("basecolor") || p.contains("base color") || p == "color" || p == "texture" {
        TextureSlot::BaseColor
    } else if p.contains("spec") || p.contains("rough") || p.contains("gloss") || p.contains("metal") {
        TextureSlot::Specular
    } else if p.contains("emis") || p.contains("glow") {
        TextureSlot::Emissive
    } else if p.contains("mask") {
        TextureSlot::Mask
    } else {
        TextureSlot::Other
    }
}

/// One texture a material samples.
#[derive(Clone, Debug)]
pub struct MaterialTexture {
    pub parameter: String,
    pub slot: TextureSlot,
    pub package: String,
    pub export: usize,
    /// Engine path, unique key for de-duplication.
    pub path: String,
}

fn struct_field<'v>(members: &'v [Property], pkg: &Package, name: &str) -> Option<&'v Value> {
    members.iter().find(|p| pkg.fname_to_string(p.name).eq_ignore_ascii_case(name)).map(|p| &p.value)
}

/// Texture parameters of a material or instance, walking up the parent chain
/// (nearest override wins). Returns textures in slot priority order.
pub fn material_textures(cache: &mut PackageCache, pkg_name: &str, material: PackageIndex) -> Vec<MaterialTexture> {
    let mut out: Vec<MaterialTexture> = Vec::new();
    let mut seen_params: Vec<String> = Vec::new();
    let mut cur = cache.resolve(pkg_name, material);
    let mut guard = 0;
    while let Some((pname, idx)) = cur {
        guard += 1;
        if guard > 16 {
            break;
        }
        let Some(obj) = cache.object(&pname, idx) else { break };
        let (pkg_ptr, class_name) = {
            let lp = cache.get(&pname).unwrap();
            (&lp.pkg as *const Package, obj.class_name.clone())
        };
        // SAFETY: packages are boxed and never removed from the cache, so the
        // allocation outlives this function even when the map grows.
        let pkg: &Package = unsafe { &*pkg_ptr };
        let mut found: Vec<(String, PackageIndex)> = Vec::new();
        if let Some(p) = obj.property("TextureParameterValues", pkg) {
            if let Value::Array(items) = &p.value {
                for it in items {
                    if let Value::Struct(StructValue::Tagged(members)) = it {
                        let name = match struct_field(members, pkg, "ParameterName") {
                            Some(Value::Name(n)) => pkg.fname_to_string(*n),
                            _ => continue,
                        };
                        if let Some(Value::Object(t)) = struct_field(members, pkg, "ParameterValue") {
                            found.push((name, *t));
                        }
                    }
                }
            }
        }
        let is_material = cache.db.class_is_a(&class_name, "Material");
        if is_material {
            // Defaults: texture sample expressions owned by the material.
            for i in 0..pkg.exports.len() {
                let e = &pkg.exports[i];
                if e.outer != PackageIndex::export(idx) {
                    continue;
                }
                let cn = pkg.class_name_of(PackageIndex::export(i));
                if !cache.db.class_is_a(&cn, "MaterialExpressionTextureSample") {
                    continue;
                }
                if let Some(eo) = cache.object(&pname, i) {
                    let name = match eo.property("ParameterName", pkg).map(|p| &p.value) {
                        Some(Value::Name(n)) => pkg.fname_to_string(*n),
                        _ => format!("texture{i}"),
                    };
                    if let Some(Value::Object(t)) = eo.property("Texture", pkg).map(|p| &p.value) {
                        found.push((name, *t));
                    }
                }
            }
        }
        for (name, t) in found {
            if seen_params.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                continue;
            }
            let Some((tp, te)) = cache.resolve(&pname, t) else { continue };
            let path = {
                let lp = cache.get(&tp).unwrap();
                texture_ops::engine_path(&lp.pkg, &lp.name, te)
            };
            seen_params.push(name.clone());
            out.push(MaterialTexture { slot: classify(&name, &path), parameter: name, package: tp, export: te, path });
        }
        if is_material {
            break;
        }
        cur = match obj.property("Parent", pkg).map(|p| &p.value) {
            Some(Value::Object(p)) => cache.resolve(&pname, *p),
            _ => None,
        };
    }
    out.sort_by_key(|t| match t.slot {
        TextureSlot::BaseColor => 0,
        TextureSlot::Normal => 1,
        TextureSlot::Emissive => 2,
        TextureSlot::Specular => 3,
        TextureSlot::Mask => 4,
        TextureSlot::Other => 5,
    });
    out
}

/// PNG of a texture export in any loaded package (largest available mip).
pub fn texture_png(cache: &mut PackageCache, src: &TextureSource, pkg_name: &str, export: usize, max_size: Option<u32>) -> Result<Vec<u8>> {
    let db = cache.db;
    let obj = cache.object(pkg_name, export).ok_or_else(|| tempest_upk::Error::Format("texture could not be parsed".into()))?;
    let lp = cache.get(pkg_name).unwrap();
    match max_size {
        Some(m) => texture_ops::export_png_preview(db, &lp.pkg, &lp.name, src, export, &obj, m),
        None => texture_ops::export_png(db, &lp.pkg, &lp.name, src, export, &obj, 0),
    }
}

/// Everything a material (instance) resolves to, for a preview: the parent
/// chain, textures, scalar / vector / static-switch parameters (nearest
/// override wins; the base material's expressions give the defaults).
#[derive(Clone, Debug, Default)]
pub struct MaterialInfo {
    /// `(package, object path, class)` from the instance up to the base material.
    pub chain: Vec<(String, String, String)>,
    pub textures: Vec<MaterialTexture>,
    pub scalars: Vec<(String, f32)>,
    pub vectors: Vec<(String, [f32; 4])>,
    pub switches: Vec<(String, bool)>,
}

fn linear_color(pkg: &Package, v: &Value) -> Option<[f32; 4]> {
    let get = |name: &str| -> Option<f32> {
        match v {
            Value::Struct(StructValue::Binary(ms)) => ms.iter().find(|m| m.name.eq_ignore_ascii_case(name)).and_then(|m| m.elems.first()).and_then(|e| if let Value::Float(f) = e { Some(*f) } else { None }),
            Value::Struct(StructValue::Tagged(ps)) => struct_field(ps, pkg, name).and_then(|e| if let Value::Float(f) = e { Some(*f) } else { None }),
            _ => None,
        }
    };
    Some([get("R")?, get("G")?, get("B")?, get("A").unwrap_or(1.0)])
}

pub fn material_info(cache: &mut PackageCache, pkg_name: &str, material: PackageIndex) -> MaterialInfo {
    let mut info = MaterialInfo { textures: material_textures(cache, pkg_name, material), ..Default::default() };
    let mut cur = cache.resolve(pkg_name, material);
    let mut guard = 0;
    while let Some((pname, idx)) = cur {
        guard += 1;
        if guard > 16 {
            break;
        }
        let Some(obj) = cache.object(&pname, idx) else { break };
        let (pkg_ptr, class_name) = {
            let lp = cache.get(&pname).unwrap();
            (&lp.pkg as *const Package, obj.class_name.clone())
        };
        // SAFETY: see `material_textures`.
        let pkg: &Package = unsafe { &*pkg_ptr };
        info.chain.push((pname.clone(), pkg.object_path(PackageIndex::export(idx)), class_name.clone()));
        let mut push_scalar = |name: String, v: f32| {
            if !info.scalars.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                info.scalars.push((name, v));
            }
        };
        if let Some(Value::Array(items)) = obj.property("ScalarParameterValues", pkg).map(|p| &p.value) {
            for it in items {
                if let Value::Struct(StructValue::Tagged(ms)) = it {
                    if let (Some(Value::Name(n)), Some(Value::Float(f))) = (struct_field(ms, pkg, "ParameterName"), struct_field(ms, pkg, "ParameterValue")) {
                        push_scalar(pkg.fname_to_string(*n), *f);
                    }
                }
            }
        }
        let mut push_vector = |name: String, v: [f32; 4]| {
            if !info.vectors.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                info.vectors.push((name, v));
            }
        };
        if let Some(Value::Array(items)) = obj.property("VectorParameterValues", pkg).map(|p| &p.value) {
            for it in items {
                if let Value::Struct(StructValue::Tagged(ms)) = it {
                    if let (Some(Value::Name(n)), Some(v)) = (struct_field(ms, pkg, "ParameterName"), struct_field(ms, pkg, "ParameterValue")) {
                        if let Some(c) = linear_color(pkg, v) {
                            push_vector(pkg.fname_to_string(*n), c);
                        }
                    }
                }
            }
        }
        if let Some(Value::Struct(StructValue::Tagged(ps))) = obj.property("StaticParameters", pkg).map(|p| &p.value) {
            if let Some(Value::Array(items)) = struct_field(ps, pkg, "StaticSwitchParameters") {
                for it in items {
                    if let Value::Struct(StructValue::Tagged(ms)) = it {
                        if let (Some(Value::Name(n)), Some(Value::Bool(b))) = (struct_field(ms, pkg, "ParameterName"), struct_field(ms, pkg, "Value")) {
                            let name = pkg.fname_to_string(*n);
                            if !info.switches.iter().any(|(x, _)| x.eq_ignore_ascii_case(&name)) {
                                info.switches.push((name, *b));
                            }
                        }
                    }
                }
            }
        }
        let is_material = cache.db.class_is_a(&class_name, "Material");
        if is_material {
            // Defaults from the base material's parameter expressions.
            for i in 0..pkg.exports.len() {
                if pkg.exports[i].outer != PackageIndex::export(idx) {
                    continue;
                }
                let cn = pkg.class_name_of(PackageIndex::export(i));
                let db = cache.db;
                let is_scalar = db.class_is_a(&cn, "MaterialExpressionScalarParameter");
                let is_vector = db.class_is_a(&cn, "MaterialExpressionVectorParameter");
                let is_switch = db.class_is_a(&cn, "MaterialExpressionStaticSwitchParameter");
                if !(is_scalar || is_vector || is_switch) {
                    continue;
                }
                let Some(eo) = cache.object(&pname, i) else { continue };
                let name = match eo.property("ParameterName", pkg).map(|p| &p.value) {
                    Some(Value::Name(n)) => pkg.fname_to_string(*n),
                    _ => continue,
                };
                let default = eo.property("DefaultValue", pkg).map(|p| &p.value);
                if is_scalar {
                    let v = match default {
                        Some(Value::Float(f)) => *f,
                        _ => 0.0,
                    };
                    if !info.scalars.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                        info.scalars.push((name, v));
                    }
                } else if is_vector {
                    let v = default.and_then(|d| linear_color(pkg, d)).unwrap_or([0.0, 0.0, 0.0, 1.0]);
                    if !info.vectors.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                        info.vectors.push((name, v));
                    }
                } else {
                    let v = matches!(default, Some(Value::Bool(true)));
                    if !info.switches.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                        info.switches.push((name, v));
                    }
                }
            }
            break;
        }
        cur = match obj.property("Parent", pkg).map(|p| &p.value) {
            Some(Value::Object(p)) => cache.resolve(&pname, *p),
            _ => None,
        };
    }
    info.scalars.sort_by_key(|(n, _)| n.to_ascii_lowercase());
    info.vectors.sort_by_key(|(n, _)| n.to_ascii_lowercase());
    info.switches.sort_by_key(|(n, _)| n.to_ascii_lowercase());
    info
}

// ----------------------------------------------------------------------------
// Editing parameters
// ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub enum ParamValue {
    Scalar(f32),
    Vector([f32; 4]),
}

fn fname(pkg: &mut Package, name: &str) -> tempest_upk::io::FName {
    pkg.intern_fname(name)
}

/// `FLinearColor` as the cooked binary struct (R, G, B, A).
fn linear_color_value(c: [f32; 4]) -> Value {
    Value::Struct(StructValue::Binary(["R", "G", "B", "A"].iter().zip(c).map(|(n, x)| Member { name: n.to_string(), kind: PropertyKind::Float, elems: vec![Value::Float(x)] }).collect()))
}

fn guid_value(g: [i32; 4]) -> Value {
    Value::Struct(StructValue::Binary(["A", "B", "C", "D"].iter().zip(g).map(|(n, x)| Member { name: n.to_string(), kind: PropertyKind::Int, elems: vec![Value::Int(x)] }).collect()))
}

fn prop(pkg: &mut Package, name: &str, kind: PropertyKind, struct_name: Option<&str>, value: Value) -> Property {
    let n = fname(pkg, name);
    let type_name = tag_type_name(pkg, kind);
    let struct_name = struct_name.map(|s| fname(pkg, s));
    let enum_name = (kind == PropertyKind::Byte).then(|| fname(pkg, "None"));
    Property { name: n, array_index: 0, kind, type_name, struct_name, enum_name, value }
}

fn set_value_in(pkg: &mut Package, props: &mut Vec<Property>, name: &str, value: ParamValue) {
    let (kind, struct_name, v) = match value {
        ParamValue::Scalar(f) => (PropertyKind::Float, None, Value::Float(f)),
        ParamValue::Vector(c) => (PropertyKind::Struct, Some("LinearColor"), linear_color_value(c)),
    };
    if let Some(p) = props.iter_mut().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(name)) {
        p.value = v;
        return;
    }
    let np = prop(pkg, name, kind, struct_name, v);
    props.push(np);
}

/// The `ExpressionGUID` of parameter `name` in the base material reached
/// through same-package `Parent` links (zero when it is elsewhere).
fn expression_guid(ed: &mut Editor, i: usize, name: &str, vector: bool) -> [i32; 4] {
    let db = ed.db;
    let mut cur = PackageIndex::export(i);
    let mut guard = 0;
    while let Some(e) = cur.export_index() {
        guard += 1;
        if guard > 16 {
            break;
        }
        let class = ed.pkg.class_name_of(cur);
        if db.class_is_a(&class, "Material") {
            let wanted = if vector { "MaterialExpressionVectorParameter" } else { "MaterialExpressionScalarParameter" };
            for j in 0..ed.pkg.exports.len() {
                if ed.pkg.exports[j].outer != cur || !db.class_is_a(&ed.pkg.class_name_of(PackageIndex::export(j)), wanted) {
                    continue;
                }
                let Ok(eo) = ed.object(j) else { continue };
                let eo = eo.clone();
                let pkg = &ed.pkg;
                let matches = matches!(eo.property("ParameterName", pkg).map(|p| &p.value), Some(Value::Name(n)) if pkg.fname_to_string(*n).eq_ignore_ascii_case(name));
                if !matches {
                    continue;
                }
                if let Some(Value::Struct(StructValue::Binary(ms))) = eo.property("ExpressionGUID", pkg).map(|p| &p.value) {
                    let mut g = [0i32; 4];
                    for (k, m) in ms.iter().take(4).enumerate() {
                        if let Some(Value::Int(x)) = m.elems.first() {
                            g[k] = *x;
                        }
                    }
                    return g;
                }
                return [0; 4];
            }
            break;
        }
        let Ok(o) = ed.object(e).map(|o| o.clone()) else { break };
        cur = match o.property("Parent", &ed.pkg).map(|p| &p.value) {
            Some(Value::Object(p)) => *p,
            _ => break,
        };
    }
    [0; 4]
}

/// Set a scalar or vector parameter: on a material instance an override
/// entry (added when missing, with the base material's expression GUID
/// when it is in the same package); on a material the parameter
/// expression's `DefaultValue`. Returns the export that changed.
pub fn set_parameter(ed: &mut Editor, i: usize, name: &str, value: ParamValue) -> Result<usize> {
    let db = ed.db;
    let class = ed.pkg.class_name_of(PackageIndex::export(i));
    let vector = matches!(value, ParamValue::Vector(_));
    if db.class_is_a(&class, "MaterialInstance") {
        let guid = expression_guid(ed, i, name, vector);
        let (array, elem_struct) = if vector { ("VectorParameterValues", "VectorParameterValue") } else { ("ScalarParameterValues", "ScalarParameterValue") };
        let mut obj = ed.object(i)?.clone();
        let pkg = &mut ed.pkg;
        let array_prop = match obj.properties.iter().position(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(array)) {
            Some(k) => k,
            None => {
                let np = prop(pkg, array, PropertyKind::Array, None, Value::Array(Vec::new()));
                obj.properties.push(np);
                obj.properties.len() - 1
            }
        };
        let Value::Array(items) = &mut obj.properties[array_prop].value else {
            return Err(tempest_upk::Error::Format(format!("{array} is not a decoded array")));
        };
        let existing = items.iter().position(|it| match it {
            Value::Struct(StructValue::Tagged(ms)) => matches!(struct_field(ms, pkg, "ParameterName"), Some(Value::Name(n)) if pkg.fname_to_string(*n).eq_ignore_ascii_case(name)),
            _ => false,
        });
        match existing {
            Some(k) => {
                if let Value::Struct(StructValue::Tagged(ms)) = &mut items[k] {
                    set_value_in(pkg, ms, "ParameterValue", value);
                }
            }
            None => {
                let mut ms = Vec::new();
                let pn = fname(pkg, name);
                ms.push(prop(pkg, "ParameterName", PropertyKind::Name, None, Value::Name(pn)));
                set_value_in(pkg, &mut ms, "ParameterValue", value);
                ms.push(prop(pkg, "ExpressionGUID", PropertyKind::Struct, Some("Guid"), guid_value(guid)));
                let _ = elem_struct;
                items.push(Value::Struct(StructValue::Tagged(ms)));
            }
        }
        ed.replace_object(i, obj)?;
        ed.mark_dirty(i);
        return Ok(i);
    }
    if db.class_is_a(&class, "Material") {
        let wanted = if vector { "MaterialExpressionVectorParameter" } else { "MaterialExpressionScalarParameter" };
        for j in 0..ed.pkg.exports.len() {
            if ed.pkg.exports[j].outer != PackageIndex::export(i) || !db.class_is_a(&ed.pkg.class_name_of(PackageIndex::export(j)), wanted) {
                continue;
            }
            let mut eo = ed.object(j)?.clone();
            let is_it = matches!(eo.property("ParameterName", &ed.pkg).map(|p| &p.value), Some(Value::Name(n)) if ed.pkg.fname_to_string(*n).eq_ignore_ascii_case(name));
            if !is_it {
                continue;
            }
            set_value_in(&mut ed.pkg, &mut eo.properties, "DefaultValue", value);
            ed.replace_object(j, eo)?;
            ed.mark_dirty(j);
            return Ok(j);
        }
        return Err(tempest_upk::Error::Format(format!("no {} parameter named {name} in this material", if vector { "vector" } else { "scalar" })));
    }
    Err(tempest_upk::Error::Format(format!("{class} is not a material")))
}
