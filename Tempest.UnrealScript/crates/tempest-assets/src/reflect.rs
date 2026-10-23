//! Class database: the script packages of a game directory loaded into a
//! [`Context`], indexed by class and struct name. Content packages never
//! define classes of their own (their `Class` exports are empty placeholders
//! left by the cooker), so this is the only reflection source needed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tempest_unrealscript::context::{Context, ObjRef};
use tempest_unrealscript::objects::{ObjectBody, PropertyData, PropertyKind};
use tempest_upk::flags::{cpf, pkg as pkgf, strct};
use tempest_upk::{Package, Result};

pub struct ClassDb {
    pub ctx: Context,
    classes: HashMap<String, ObjRef>,
    structs: HashMap<String, ObjRef>,
}

/// Native-only classes (`DECLARE_CLASS_INTRINSIC`, no script export) that
/// cooked content refers to, with their parent class. Hi-Rez's
/// `TgPersistentCookerData` and `TgMenuContentData_Content` have no engine
/// source; `Object` is the safe parent (they carry tagged properties only).
pub const INTRINSIC_CLASSES: &[(&str, &str)] = &[
    ("Package", "Object"),
    ("MetaData", "Object"),
    ("ObjectRedirector", "Object"),
    ("TextBuffer", "Object"),
    ("GuidCache", "Object"),
    ("Linker", "Object"),
    ("LinkerLoad", "Linker"),
    ("LinkerSave", "Linker"),
    ("Polys", "Object"),
    ("Model", "Object"),
    ("Level", "LevelBase"),
    ("World", "Object"),
    ("NavigationMeshBase", "Object"),
    ("ShaderCache", "Object"),
    ("ShadowMap1D", "Object"),
    ("LightMapTexture2D", "Texture2D"),
    ("StaticMesh", "Object"),
    ("FracturedStaticMesh", "StaticMesh"),
    ("PersistentCookerData", "Object"),
    ("TgPersistentCookerData", "PersistentCookerData"),
    ("TgMenuContentData_Content", "Object"),
    ("CodecMovieBink", "CodecMovie"),
    ("PackageMap", "Object"),
    ("System", "Subsystem"),
];

impl Default for ClassDb {
    fn default() -> Self {
        ClassDb::new()
    }
}

impl ClassDb {
    pub fn new() -> ClassDb {
        ClassDb { ctx: Context::new(), classes: HashMap::new(), structs: HashMap::new() }
    }

    /// Packages of `dir` flagged `PKG_ContainsScript`, sorted by name.
    pub fn script_packages_in(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir.as_ref())? {
            let p = e?.path();
            let ext = p.extension().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if ext != "upk" && ext != "u" {
                continue;
            }
            if let Ok(s) = Package::read_summary(&p) {
                if s.package_flags & pkgf::CONTAINS_SCRIPT != 0 {
                    out.push(p);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// Load every script package of a cooked game directory.
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<ClassDb> {
        let files = ClassDb::script_packages_in(&dir)?;
        let mut db = ClassDb::load_files(&files)?;
        db.ctx.add_search_path(dir);
        Ok(db)
    }

    pub fn load_files(files: &[PathBuf]) -> Result<ClassDb> {
        let mut db = ClassDb::new();
        for f in files {
            db.ctx.load_file(f)?;
        }
        db.index();
        Ok(db)
    }

    fn index(&mut self) {
        self.classes.clear();
        self.structs.clear();
        for (slot, lp) in self.ctx.packages.iter().enumerate() {
            for i in 0..lp.pkg.exports.len() {
                let r = ObjRef { pkg: slot, export: i };
                match lp.object(i).map(|o| &o.body) {
                    Some(ObjectBody::Class { .. }) => {
                        self.classes.entry(lp.export_name(i).to_ascii_lowercase()).or_insert(r);
                    }
                    Some(ObjectBody::ScriptStruct { .. }) => {
                        self.structs.entry(lp.export_name(i).to_ascii_lowercase()).or_insert(r);
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn package_names(&self) -> Vec<String> {
        self.ctx.packages.iter().map(|p| p.name.clone()).collect()
    }

    /// A script class by name (case-insensitive; script and content packages
    /// spell names differently). Intrinsic classes have no export: see
    /// [`ClassDb::resolve_class`].
    pub fn class(&self, name: &str) -> Option<ObjRef> {
        self.classes.get(&name.to_ascii_lowercase()).copied()
    }

    /// Parent of an intrinsic (native-only) class.
    pub fn intrinsic_super(name: &str) -> Option<&'static str> {
        INTRINSIC_CLASSES.iter().find(|(c, _)| c.eq_ignore_ascii_case(name)).map(|(_, s)| *s)
    }

    /// The nearest script class of `name`: itself, or the first scripted
    /// ancestor of an intrinsic class (whose properties it inherits).
    pub fn resolve_class(&self, name: &str) -> Option<ObjRef> {
        let mut cur = name.to_string();
        for _ in 0..16 {
            if let Some(c) = self.class(&cur) {
                return Some(c);
            }
            cur = ClassDb::intrinsic_super(&cur)?.to_string();
        }
        None
    }

    /// Does the class called `name` derive from (or equal) `base`, through
    /// intrinsic and script classes alike?
    pub fn class_is_a(&self, name: &str, base: &str) -> bool {
        let mut cur = name.to_string();
        for _ in 0..16 {
            if cur.eq_ignore_ascii_case(base) {
                return true;
            }
            if let Some(c) = self.class(&cur) {
                return self.is_a(c, base);
            }
            match ClassDb::intrinsic_super(&cur) {
                Some(s) => cur = s.to_string(),
                None => return false,
            }
        }
        false
    }

    /// A script struct by bare name. Struct names are not unique across the
    /// engine; prefer the struct referenced by a property definition.
    pub fn struct_named(&self, name: &str) -> Option<ObjRef> {
        self.structs.get(&name.to_ascii_lowercase()).copied()
    }

    pub fn name(&self, r: ObjRef) -> String {
        self.ctx.name(r)
    }

    pub fn full_path(&self, r: ObjRef) -> String {
        self.ctx.full_path(r)
    }

    pub fn is_a(&self, class: ObjRef, name: &str) -> bool {
        self.ctx.is_a(class, name)
    }

    pub fn super_class(&self, class: ObjRef) -> Option<ObjRef> {
        self.ctx.super_of(class)
    }

    /// Class chain from `class` up to `Object`.
    pub fn class_chain(&self, class: ObjRef) -> Vec<ObjRef> {
        let mut out = Vec::new();
        let mut cur = Some(class);
        while let Some(c) = cur {
            out.push(c);
            cur = self.ctx.super_of(c);
            if out.len() > 100 {
                break;
            }
        }
        out
    }

    /// A property by name in a class/struct chain.
    pub fn find_property(&self, scope: ObjRef, name: &str) -> Option<ObjRef> {
        let f = self.ctx.find_field(scope, name)?;
        self.ctx.property_kind(f).map(|_| f)
    }

    pub fn property(&self, p: ObjRef) -> Option<(PropertyKind, &PropertyData)> {
        self.ctx.property(p)
    }

    pub fn property_kind(&self, p: ObjRef) -> Option<PropertyKind> {
        self.ctx.property_kind(p)
    }

    pub fn property_flags(&self, p: ObjRef) -> u64 {
        self.ctx.property(p).map(|(_, d)| d.property_flags).unwrap_or(0)
    }

    pub fn array_dim(&self, p: ObjRef) -> i32 {
        self.ctx.property(p).map(|(_, d)| d.array_dim.max(1)).unwrap_or(1)
    }

    /// The enum of a byte property, the struct of a struct property, the inner
    /// property of an array, the class of an object property.
    pub fn property_ref1(&self, p: ObjRef) -> Option<ObjRef> {
        let (_, d) = self.ctx.property(p)?;
        self.ctx.resolve(p.pkg, d.ref1?)
    }

    pub fn struct_flags(&self, s: ObjRef) -> u32 {
        match self.ctx.object(s).map(|o| &o.body) {
            Some(ObjectBody::ScriptStruct { data, .. }) => data.struct_flags,
            _ => 0,
        }
    }

    /// `UStructProperty::SerializeItem`: binary (`SerializeBin`) layout
    /// instead of tagged properties.
    pub fn struct_is_binary(&self, s: ObjRef, cooked: bool) -> bool {
        let f = self.struct_flags(s);
        f & strct::IMMUTABLE != 0 || (f & strct::IMMUTABLE_WHEN_COOKED != 0 && cooked)
    }

    /// Members in `PropertyLink` order: own properties, then the parents'.
    pub fn struct_members(&self, s: ObjRef) -> Vec<ObjRef> {
        self.ctx.properties_in_serialize_order(s)
    }

    /// All properties of a class, base class first (declaration order).
    pub fn class_properties(&self, class: ObjRef) -> Vec<ObjRef> {
        self.ctx.properties_in_layout_order(class)
    }

    pub fn enum_names(&self, e: ObjRef) -> Vec<String> {
        self.ctx.enum_names(e)
    }

    pub fn class_default_object(&self, class: ObjRef) -> Option<ObjRef> {
        self.ctx.class_default_object(class)
    }

    /// Value of a byte/enum property on the class default object chain
    /// (`Format` of a texture whose export does not override it), as its
    /// tag name.
    pub fn cdo_enum_property(&self, class: ObjRef, name: &str) -> Option<String> {
        let mut cur = Some(class);
        for _ in 0..100 {
            let c = cur?;
            if let Some(cdo) = self.class_default_object(c) {
                if let Some(o) = self.ctx.object(cdo) {
                    let lp = &self.ctx.packages[cdo.pkg];
                    for tp in &o.header.properties {
                        if lp.name_of(tp.name).eq_ignore_ascii_case(name) && tp.array_index == 0 && tp.raw.len() == 8 {
                            let mut r = tempest_upk::Reader::new(&tp.raw);
                            if let Ok(n) = r.fname() {
                                return Some(lp.name_of(n));
                            }
                        }
                    }
                }
            }
            cur = self.ctx.super_of(c);
        }
        None
    }

    /// `UProperty::ShouldSerializeValue` for a persistent archive of a cooked
    /// package (`FilterEditorOnly` set): native never, transient only while
    /// serialising a class default object, deprecated and non-bool
    /// editor-only properties never (the cooker dropped them).
    pub fn should_serialize(&self, p: ObjRef, serializing_defaults: bool, cooked: bool) -> bool {
        let f = self.property_flags(p);
        if f & cpf::NATIVE != 0 {
            return false;
        }
        if f & cpf::TRANSIENT != 0 && !serializing_defaults {
            return false;
        }
        if f & cpf::DEPRECATED != 0 && cooked {
            return false;
        }
        if f & cpf::EDITOR_ONLY != 0 && cooked && self.property_kind(p) != Some(PropertyKind::Bool) {
            return false;
        }
        true
    }
}
