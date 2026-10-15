//! Multi-package object database: loads packages from search paths, parses
//! their exports, resolves imports across packages and answers class
//! hierarchy / native-function questions for the decompiler and compiler.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tempest_upk::flags::{cpf, func};
use tempest_upk::io::{FName, PackageIndex, Result};
use tempest_upk::Package;

use crate::objects::{ClassData, self, ExportInfo, Layout, ObjectBody, ParsedObject, PropertyData, PropertyKind, StructData};

/// Global reference to an export in a loaded package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjRef {
    pub pkg: usize,
    pub export: usize,
}

pub struct LoadedPackage {
    pub name: String,
    pub path: PathBuf,
    pub pkg: Package,
    pub layout: Layout,
    /// Parsed exports (`None` when the payload could not be interpreted).
    pub objects: Vec<Option<ParsedObject>>,
    pub none_name: u32,
    /// Lower-cased object path → export index, for import resolution.
    path_index: HashMap<String, usize>,
    /// Loaded only for object-literal and native-data lookups (a package being
    /// recompiled): its classes, enums, structs and natives stay out of the global index.
    pub shadow: bool,
}

impl LoadedPackage {
    pub fn object(&self, i: usize) -> Option<&ParsedObject> {
        self.objects.get(i).and_then(|o| o.as_ref())
    }
    pub fn name_of(&self, n: FName) -> String {
        self.pkg.fname_to_string(n)
    }
    pub fn export_name(&self, i: usize) -> String {
        self.pkg.fname_to_string(self.pkg.exports[i].object_name)
    }
}

#[derive(Default)]
pub struct Context {
    pub packages: Vec<LoadedPackage>,
    by_name: HashMap<String, usize>,
    pub search_paths: Vec<PathBuf>,
    /// native index → function
    natives: HashMap<u16, ObjRef>,
    missing: HashSet<String>,
}

impl Context {
    pub fn new() -> Context {
        Context::default()
    }

    pub fn add_search_path(&mut self, p: impl AsRef<Path>) {
        self.search_paths.push(p.as_ref().to_path_buf());
    }

    /// Load a package file and index it. Returns its slot.
    pub fn load_file(&mut self, path: impl AsRef<Path>) -> Result<usize> {
        let path = path.as_ref().to_path_buf();
        let pkg = Package::load(&path)?;
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        Ok(self.add_package(name, path, pkg))
    }

    pub fn add_package(&mut self, name: String, path: PathBuf, pkg: Package) -> usize {
        let layout = objects::probe_layout(&pkg);
        let none_name = pkg.find_name("None").unwrap_or(0);
        let slot = self.packages.len();
        let mut lp = LoadedPackage {
            name: name.clone(),
            path,
            pkg,
            layout,
            objects: Vec::new(),
            none_name,
            path_index: HashMap::new(),
            shadow: false,
        };
        for i in 0..lp.pkg.exports.len() {
            let p = lp.pkg.object_path(PackageIndex::export(i)).to_ascii_lowercase();
            lp.path_index.entry(p).or_insert(i);
        }
        self.by_name.insert(name.to_ascii_lowercase(), slot);
        self.packages.push(lp);
        self.parse_all(slot);
        slot
    }

    /// Load a package as a shadow (see `LoadedPackage::shadow`).
    pub fn add_package_shadow(&mut self, name: String, path: PathBuf, pkg: Package) -> usize {
        let slot = self.add_package(name, path, pkg);
        self.packages[slot].shadow = true;
        // Natives were registered while parsing; a shadow must not claim indices.
        self.natives.retain(|_, r| r.pkg != slot);
        slot
    }

    pub fn slot_of(&self, name: &str) -> Option<usize> {
        self.by_name.get(&name.to_ascii_lowercase()).copied()
    }

    /// Find (and lazily load from the search paths) a package by name.
    pub fn package_by_name(&mut self, name: &str) -> Option<usize> {
        let key = name.to_ascii_lowercase();
        if let Some(&i) = self.by_name.get(&key) {
            return Some(i);
        }
        if self.missing.contains(&key) {
            return None;
        }
        for dir in self.search_paths.clone() {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    let stem = p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                    let ext = p.extension().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                    if stem == key && (ext == "upk" || ext == "u") {
                        if let Ok(slot) = self.load_file(&p) {
                            return Some(slot);
                        }
                    }
                }
            }
        }
        self.missing.insert(key);
        None
    }

    /// Load every package referenced by `slot`'s imports, transitively, then
    /// re-parse component objects now that class hierarchies resolve.
    pub fn preload_imports(&mut self, slot: usize) {
        let mut todo = vec![slot];
        let mut seen = HashSet::new();
        while let Some(s) = todo.pop() {
            if !seen.insert(s) {
                continue;
            }
            let roots: Vec<String> = (0..self.packages[s].pkg.imports.len())
                .map(|i| self.packages[s].pkg.outermost_name(PackageIndex::import(i)))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            for r in roots {
                if let Some(t) = self.package_by_name(&r) {
                    todo.push(t);
                }
            }
        }
        for s in seen {
            self.reparse_components(s);
        }
    }

    fn parse_all(&mut self, slot: usize) {
        let n = self.packages[slot].pkg.exports.len();
        let mut objs: Vec<Option<ParsedObject>> = Vec::with_capacity(n);
        for i in 0..n {
            let lp = &self.packages[slot];
            let e = &lp.pkg.exports[i];
            let cn = lp.pkg.class_name_of(PackageIndex::export(i));
            if e.serial_size == 0 && e.class.is_null() {
                objs.push(None);
                continue;
            }
            let native_prefix_len = self.native_prefix_len(ObjRef { pkg: slot, export: i });
            let lp = &self.packages[slot];
            let e = &lp.pkg.exports[i];
            let info = ExportInfo { class_name: &cn, object_flags: e.object_flags, is_component: false, native_prefix_len };
            let r = objects::parse_export(&lp.pkg.export_data[i], &lp.pkg, info, lp.layout);
            objs.push(r.ok());
        }
        self.packages[slot].objects = objs;
        for i in 0..n {
            if let Some(ParsedObject { body: ObjectBody::Function { function, .. }, .. }) = self.packages[slot].object(i) {
                if function.function_flags & func::NATIVE != 0 && function.native_index != 0 && !self.packages[slot].shadow {
                    self.natives.insert(function.native_index, ObjRef { pkg: slot, export: i });
                }
            }
        }
    }

    /// Re-parse exports whose class derives from `Component`.
    /// `UDominant*LightComponent::Serialize` writes its shadow map before
    /// `Super::Serialize` (an empty array on templates); class default objects of
    /// those classes carry no such prefix.
    fn native_prefix_len(&self, r: ObjRef) -> usize {
        const DOMINANT: [&str; 2] = ["DominantDirectionalLightComponent", "DominantSpotLightComponent"];
        if self.object_flags(r) & tempest_upk::flags::rf::CLASS_DEFAULT_OBJECT != 0 {
            return 0;
        }
        let cn = self.class_name(r);
        if DOMINANT.iter().any(|d| cn.eq_ignore_ascii_case(d)) {
            return 4;
        }
        match self.class_of(r) {
            Some(c) if DOMINANT.iter().any(|d| self.is_a(c, d)) => 4,
            _ => 0,
        }
    }

    pub fn reparse_components(&mut self, slot: usize) {
        let n = self.packages[slot].pkg.exports.len();
        for i in 0..n {
            let r = ObjRef { pkg: slot, export: i };
            let is_component = self.class_of(r).map(|c| self.is_a(c, "Component")).unwrap_or(false);
            // Class default objects of component classes carry no component header.
            if !is_component || self.object_flags(r) & tempest_upk::flags::rf::CLASS_DEFAULT_OBJECT != 0 {
                continue;
            }
            let native_prefix_len = self.native_prefix_len(r);
            let lp = &self.packages[slot];
            let e = &lp.pkg.exports[i];
            let cn = lp.pkg.class_name_of(PackageIndex::export(i));
            let info = ExportInfo { class_name: &cn, object_flags: e.object_flags, is_component: true, native_prefix_len };
            let parsed = objects::parse_export(&lp.pkg.export_data[i], &lp.pkg, info, lp.layout).ok();
            if parsed.is_some() {
                self.packages[slot].objects[i] = parsed;
            }
        }
    }

    // ----- resolution ----------------------------------------------------

    /// Resolve a package-local index to a global export reference. Import
    /// targets must already be loaded (see [`Context::preload_imports`]).
    pub fn resolve(&self, pkg: usize, idx: PackageIndex) -> Option<ObjRef> {
        if let Some(e) = idx.export_index() {
            return Some(ObjRef { pkg, export: e });
        }
        let i = idx.import_index()?;
        let lp = &self.packages[pkg];
        lp.pkg.imports.get(i)?;
        let root = lp.pkg.outermost_name(idx);
        let full = lp.pkg.object_path(idx);
        let path = full.splitn(2, '.').nth(1).unwrap_or("");
        if path.is_empty() {
            return None;
        }
        let target = *self.by_name.get(&root.to_ascii_lowercase())?;
        let e = *self.packages[target].path_index.get(&path.to_ascii_lowercase())?;
        Some(ObjRef { pkg: target, export: e })
    }

    pub fn find_export(&self, pkg: usize, path: &str) -> Option<ObjRef> {
        let e = *self.packages[pkg].path_index.get(&path.to_ascii_lowercase())?;
        Some(ObjRef { pkg, export: e })
    }

    pub fn object(&self, r: ObjRef) -> Option<&ParsedObject> {
        self.packages[r.pkg].object(r.export)
    }

    pub fn name(&self, r: ObjRef) -> String {
        self.packages[r.pkg].export_name(r.export)
    }

    pub fn path(&self, r: ObjRef) -> String {
        self.packages[r.pkg].pkg.object_path(PackageIndex::export(r.export))
    }

    /// `Package.Outer.Name`
    pub fn full_path(&self, r: ObjRef) -> String {
        if self.is_forced_export(r) {
            return self.path(r);
        }
        format!("{}.{}", self.packages[r.pkg].name, self.path(r))
    }

    /// Whether the export's outermost outer is a `Package` export (a forced export
    /// copied in by seek-free cooking), so its path already starts with a package name.
    pub fn is_forced_export(&self, r: ObjRef) -> bool {
        let mut cur = r;
        let mut guard = 0;
        while let Some(o) = self.outer_of(cur) {
            cur = o;
            guard += 1;
            if guard > 64 {
                return false;
            }
        }
        self.class_name(cur) == "Package"
    }

    pub fn class_name(&self, r: ObjRef) -> String {
        self.packages[r.pkg].pkg.class_name_of(PackageIndex::export(r.export))
    }

    pub fn object_flags(&self, r: ObjRef) -> u64 {
        self.packages[r.pkg].pkg.exports[r.export].object_flags
    }

    pub fn class_of(&self, r: ObjRef) -> Option<ObjRef> {
        let c = self.packages[r.pkg].pkg.exports[r.export].class;
        if c.is_null() {
            return None;
        }
        self.resolve(r.pkg, c)
    }

    pub fn outer_of(&self, r: ObjRef) -> Option<ObjRef> {
        let o = self.packages[r.pkg].pkg.exports[r.export].outer;
        self.resolve(r.pkg, o)
    }

    /// The class that (transitively) contains this field.
    pub fn owner_class(&self, r: ObjRef) -> Option<ObjRef> {
        let mut cur = Some(r);
        let mut guard = 0;
        while let Some(c) = cur {
            if self.class_name(c) == "Class" {
                return Some(c);
            }
            cur = self.outer_of(c);
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        None
    }

    pub fn struct_data(&self, r: ObjRef) -> Option<&StructData> {
        match &self.object(r)?.body {
            ObjectBody::Class { s, .. }
            | ObjectBody::State { s, .. }
            | ObjectBody::Function { s, .. }
            | ObjectBody::ScriptStruct { s, .. } => Some(s),
            _ => None,
        }
    }

    /// Super struct/class of a struct-like export.
    pub fn super_of(&self, r: ObjRef) -> Option<ObjRef> {
        let s = self.struct_data(r)?.super_struct;
        if s.is_null() {
            return None;
        }
        self.resolve(r.pkg, s)
    }

    /// Does the class `class` derive from (or equal) a class named `name`?
    pub fn is_a(&self, class: ObjRef, name: &str) -> bool {
        let mut cur = Some(class);
        let mut guard = 0;
        while let Some(c) = cur {
            if self.name(c).eq_ignore_ascii_case(name) {
                return true;
            }
            cur = self.super_of(c);
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        false
    }

    /// Is `class` equal to or derived from `base`?
    pub fn derives_from(&self, class: ObjRef, base: ObjRef) -> bool {
        let mut cur = Some(class);
        let mut guard = 0;
        while let Some(c) = cur {
            if c == base {
                return true;
            }
            cur = self.super_of(c);
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        false
    }

    pub fn native_function(&self, index: u16) -> Option<ObjRef> {
        self.natives.get(&index).copied()
    }

    /// Children (linked through `UField::Next`) of a struct-like export.
    pub fn children(&self, r: ObjRef) -> Vec<ObjRef> {
        let mut out = Vec::new();
        let Some(s) = self.struct_data(r) else { return out };
        let mut cur = s.children;
        let mut guard = 0;
        while !cur.is_null() && guard < 100_000 {
            let Some(cr) = self.resolve(r.pkg, cur) else { break };
            out.push(cr);
            cur = match self.object(cr) {
                Some(o) => o.next.unwrap_or_default(),
                None => break,
            };
            guard += 1;
        }
        out
    }

    pub fn property(&self, r: ObjRef) -> Option<(PropertyKind, &PropertyData)> {
        match &self.object(r)?.body {
            ObjectBody::Property { kind, data } => Some((*kind, data)),
            _ => None,
        }
    }

    pub fn property_kind(&self, r: ObjRef) -> Option<PropertyKind> {
        self.property(r).map(|p| p.0)
    }

    /// Parameters of a function in declaration order (excluding the return value).
    pub fn function_params(&self, f: ObjRef) -> Vec<ObjRef> {
        self.children(f)
            .into_iter()
            .filter(|&c| {
                self.property(c)
                    .map(|(_, d)| d.property_flags & cpf::PARM != 0 && d.property_flags & cpf::RETURN_PARM == 0)
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn function_return(&self, f: ObjRef) -> Option<ObjRef> {
        self.children(f)
            .into_iter()
            .find(|&c| self.property(c).map(|(_, d)| d.property_flags & cpf::RETURN_PARM != 0).unwrap_or(false))
    }

    pub fn function_locals(&self, f: ObjRef) -> Vec<ObjRef> {
        self.children(f)
            .into_iter()
            .filter(|&c| self.property(c).map(|(_, d)| d.property_flags & cpf::PARM == 0).unwrap_or(false))
            .collect()
    }

    /// Find a function by name in a class (walking super classes) and, when
    /// `state` is given, that state chain first.
    pub fn find_function(&self, class: ObjRef, state: Option<ObjRef>, name: &str) -> Option<ObjRef> {
        let mut st = state;
        let mut guard = 0;
        while let Some(s) = st {
            for c in self.children(s) {
                if self.class_name(c) == "Function" && self.name(c).eq_ignore_ascii_case(name) {
                    return Some(c);
                }
            }
            st = self.super_of(s);
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        let mut cur = Some(class);
        guard = 0;
        while let Some(c) = cur {
            for ch in self.children(c) {
                if self.class_name(ch) == "Function" && self.name(ch).eq_ignore_ascii_case(name) {
                    return Some(ch);
                }
            }
            cur = self.super_of(c);
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        None
    }

    /// Find a field (property/struct/enum/const) by name in a struct chain.
    pub fn find_field(&self, scope: ObjRef, name: &str) -> Option<ObjRef> {
        let mut cur = Some(scope);
        let mut guard = 0;
        while let Some(c) = cur {
            for ch in self.children(c) {
                if self.name(ch).eq_ignore_ascii_case(name) {
                    return Some(ch);
                }
            }
            cur = self.super_of(c);
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        None
    }

    /// Properties of a struct/class in `PropertyLink` order: super first,
    /// then own children in declaration order.
    /// `PropertyLink` order (`SerializeBin`): the struct's own properties, then its parents'.
    pub fn properties_in_serialize_order(&self, s: ObjRef) -> Vec<ObjRef> {
        let mut out = Vec::new();
        let mut cur = Some(s);
        let mut guard = 0;
        while let Some(c) = cur {
            for ch in self.children(c) {
                if self.property_kind(ch).is_some() {
                    out.push(ch);
                }
            }
            cur = self.super_of(c);
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        out
    }

    pub fn properties_in_layout_order(&self, s: ObjRef) -> Vec<ObjRef> {
        let mut chain = Vec::new();
        let mut cur = Some(s);
        let mut guard = 0;
        while let Some(c) = cur {
            chain.push(c);
            cur = self.super_of(c);
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        let mut out = Vec::new();
        for c in chain.into_iter().rev() {
            for ch in self.children(c) {
                if self.property_kind(ch).is_some() {
                    out.push(ch);
                }
            }
        }
        out
    }

    pub fn enum_names(&self, e: ObjRef) -> Vec<String> {
        match &self.object(e).map(|o| &o.body) {
            Some(ObjectBody::Enum(d)) => d.names.iter().map(|n| self.packages[e.pkg].name_of(*n)).collect(),
            _ => Vec::new(),
        }
    }

    /// The enum behind a byte property, or the enum of an array's inner byte.
    pub fn enum_of_property(&self, p: ObjRef) -> Option<ObjRef> {
        let (kind, data) = self.property(p)?;
        match kind {
            PropertyKind::Byte => self.resolve(p.pkg, data.ref1?),
            _ => None,
        }
    }

    /// `ClassConfigName` of a loaded class.
    pub fn class_config_name(&self, r: ObjRef) -> String {
        match self.object(r).map(|x| &x.body) {
            Some(ObjectBody::Class { class, .. }) => self.packages[r.pkg].pkg.fname_to_string(class.class_config_name),
            _ => "None".into(),
        }
    }

    /// `ClassHeaderFilename` of a loaded class (empty for server layouts).
    pub fn class_header_filename(&self, r: ObjRef) -> String {
        match self.object(r).map(|x| &x.body) {
            Some(ObjectBody::Class { class, .. }) => class.editor.as_ref().map(|e| e.class_header_filename.clone()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// `(HideCategories, AutoExpandCategories, AutoCollapseCategories, DontSortCategories, bForceScriptOrder)`.
    pub fn class_category_lists(&self, r: ObjRef) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>, bool) {
        match self.object(r).map(|x| &x.body) {
            Some(ObjectBody::Class { class: ClassData { editor: Some(e), .. }, .. }) => {
                let pkg = &self.packages[r.pkg].pkg;
                let conv = |v: &Vec<FName>| v.iter().map(|n| pkg.fname_to_string(*n)).collect::<Vec<_>>();
                (conv(&e.hide_categories), conv(&e.auto_expand_categories), conv(&e.auto_collapse_categories), conv(&e.dont_sort_categories), e.force_script_order != 0)
            }
            _ => (Vec::new(), Vec::new(), Vec::new(), Vec::new(), false),
        }
    }

    /// `StateFlags` of a state or class.
    pub fn state_flags(&self, r: ObjRef) -> u32 {
        match self.object(r).map(|x| &x.body) {
            Some(ObjectBody::Class { state, .. }) | Some(ObjectBody::State { state, .. }) => state.state_flags,
            _ => 0,
        }
    }

    /// The class default object of a loaded class.
    pub fn class_default_object(&self, class: ObjRef) -> Option<ObjRef> {
        match self.object(class).map(|x| &x.body) {
            Some(ObjectBody::Class { class: c, .. }) => self.resolve(class.pkg, c.class_default_object),
            _ => None,
        }
    }

    pub fn none_fname(&self, pkg: usize) -> FName {
        FName::new(self.packages[pkg].none_name)
    }
}
