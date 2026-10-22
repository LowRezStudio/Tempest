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

use crate::object::{package_layout, parse_export, Object, Parsed};
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
pub fn texture_png(cache: &mut PackageCache, src: &TextureSource, pkg_name: &str, export: usize) -> Result<Vec<u8>> {
    let db = cache.db;
    let obj = cache.object(pkg_name, export).ok_or_else(|| tempest_upk::Error::Format("texture could not be parsed".into()))?;
    let lp = cache.get(pkg_name).unwrap();
    texture_ops::export_png(db, &lp.pkg, &lp.name, src, export, &obj, 0)
}
