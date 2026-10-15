//! Turn a compiled `Model` into a `Package`: name/import/export tables,
//! export payloads, and bytecode conversion from memory form to disk form.

use std::collections::{HashMap, HashSet};

use tempest_upk::flags::{cpf, pkg as pkgf, rf, ENGINE_VERSION, PACKAGE_FILE_TAG, PACKAGE_FILE_VERSION};
use tempest_upk::io::{FName, Guid, PackageIndex};
use tempest_upk::{NameEntry, ObjectExport, ObjectImport, Package};

use super::engine::Compiler;
use super::model::{FieldRef, NodeId, NodeKind};
use crate::bytecode::{Assembler, Disassembler, Statement, Token};
use crate::context::ObjRef;
use crate::objects::NameTable;
use crate::objects::{
    write_export, ClassData, ClassEditorData, ConstData, EnumData, FunctionData, Layout, ObjectBody, ObjectHeader, ParsedObject, PropertyData,
    ScriptStructData, StateData, StructData, TextBufferData,
};

#[derive(Clone, Debug)]
pub struct OutputOptions {
    pub file_version: u16,
    pub licensee_version: u16,
    /// Serialize editor-only field data (UDK / client layout) or the server layout.
    pub editor_data: bool,
    /// Emit `ScriptText` / `CppText` text buffers.
    pub keep_source: bool,
    /// Write a cooked package like the game's own: `PKG_Cooked` flags, cooked
    /// content version, and `NetIndex` = export index.
    pub cooked: bool,
    /// `NetIndex` per object path (lower-cased) taken from an existing package;
    /// objects not listed get fresh indices after the highest one (or none when
    /// the source package has none at all).
    pub net_index_map: Option<std::collections::HashMap<String, i32>>,
    /// Per class (lower-cased name): the reference package's component map entries
    /// `(name, is_null)` in order; null entries (cooker-stripped components) are kept.
    pub conform_components: Option<std::collections::HashMap<String, Vec<(String, bool)>>>,
    /// Export order of the reference package (lower-cased object paths, `""` for a
    /// null export): exports keep their index, missing ones become null placeholders,
    /// new ones are appended (`SortExports` with a linker to conform to).
    pub conform_exports: Option<Vec<String>>,
    /// Reference tag lists per object path: (name, type, array index, is a null object
    /// reference). The cooker writes a tag for a reference it nulled on save (the object
    /// was cooked out), which no source can express: those tags are copied.
    pub conform_tags: Option<std::collections::HashMap<String, Vec<(String, String, i32, bool)>>>,
    /// Reference import table `(class package, class, outer index, name)`, seeded first.
    pub conform_imports: Option<Vec<(String, String, i32, String)>>,
    /// Reference exports (entry + payload) per `conform_exports` position: copied verbatim
    /// for every export this compile does not produce (content the cooker pulled in).
    /// Valid because the name/import/export tables start as the reference's.
    pub conform_raw: Option<Vec<Option<(ObjectExport, Vec<u8>)>>>,
    /// Reference summary fields (GUID, generations, versions, flags).
    pub conform_summary: Option<tempest_upk::package::Summary>,
    /// Reference `(Line, TextPos)` per struct path (source positions a decompiled text cannot reproduce).
    pub conform_pos: Option<std::collections::HashMap<String, (i32, i32)>>,
    /// Reference name table flags, by index (the seeded names come first in the same order).
    pub conform_name_flags: Option<Vec<u64>>,
}

impl Default for OutputOptions {
    fn default() -> Self {
        OutputOptions { file_version: PACKAGE_FILE_VERSION, licensee_version: 0, editor_data: true, keep_source: true, cooked: false, net_index_map: None, conform_components: None, conform_exports: None, conform_tags: None, conform_imports: None, conform_raw: None, conform_summary: None, conform_pos: None, conform_name_flags: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Item {
    Node(NodeId),
    Cdo(NodeId),
    ScriptText(NodeId),
    CppText(NodeId),
    /// Empty placeholder export (conforming to a package whose export no longer exists).
    Null,
}

pub struct Emitter<'c, 'a> {
    pub c: &'c Compiler<'a>,
    pub(super) opts: OutputOptions,
    pub pkg: Package,
    items: Vec<Item>,
    export_of: HashMap<NodeId, usize>,
    cdo_of: HashMap<NodeId, usize>,
    script_text_of: HashMap<NodeId, usize>,
    cpp_text_of: HashMap<NodeId, usize>,
    import_of: HashMap<ObjRef, PackageIndex>,
    named_imports: HashMap<(String, String, i32, String), PackageIndex>,
    /// Export position → reference export position for raw copies.
    raw_of: HashMap<usize, usize>,
    /// Lower-cased object path → export position of a raw-copied reference export.
    raw_by_path: HashMap<String, usize>,
    /// Sub-objects reachable from the class default objects (computed on first use).
    referenced_subobjects: std::cell::RefCell<Option<HashSet<NodeId>>>,
    layout: Layout,
    /// Export currently being serialized (its `NetIndex` in cooked output).
    current_export: usize,
    /// Next fresh `NetIndex` for objects missing from `net_index_map`.
    next_net_index: i32,
}

impl<'c, 'a> Emitter<'c, 'a> {
    pub fn new(c: &'c Compiler<'a>, opts: OutputOptions) -> Self {
        Emitter {
            c,
            pkg: Package::default(),
            layout: Layout { editor_data: opts.editor_data },
            current_export: 0,
            next_net_index: opts.net_index_map.as_ref().map(|m| m.values().copied().max().unwrap_or(-1) + 1).unwrap_or(0),
            opts,
            items: Vec::new(),
            export_of: HashMap::new(),
            cdo_of: HashMap::new(),
            script_text_of: HashMap::new(),
            cpp_text_of: HashMap::new(),
            import_of: HashMap::new(),
            named_imports: HashMap::new(),
            raw_of: HashMap::new(),
            raw_by_path: HashMap::new(),
            referenced_subobjects: std::cell::RefCell::new(None),
        }
    }

    fn fname(&mut self, text: &str) -> FName {
        self.pkg.intern_fname(text)
    }

    fn none(&mut self) -> FName {
        FName { index: self.pkg.intern_name("None"), number: 0 }
    }

    /// Import of a top-level package by name.
    fn import_package(&mut self, name: &str) -> PackageIndex {
        self.import_named("Core", "Package", PackageIndex(0), name)
    }

    fn import_named(&mut self, class_package: &str, class_name: &str, outer: PackageIndex, name: &str) -> PackageIndex {
        let key = (class_package.to_ascii_lowercase(), class_name.to_ascii_lowercase(), outer.0, name.to_ascii_lowercase());
        if let Some(&i) = self.named_imports.get(&key) {
            return i;
        }
        let imp = ObjectImport {
            class_package: self.fname(class_package),
            class_name: self.fname(class_name),
            outer,
            object_name: self.fname(name),
        };
        self.pkg.imports.push(imp);
        let idx = PackageIndex::import(self.pkg.imports.len() - 1);
        self.named_imports.insert(key, idx);
        idx
    }

    /// Import of an intrinsic class (`Core.IntProperty`, `Core.Function`, ...).
    fn import_core_class(&mut self, name: &str) -> PackageIndex {
        let core = self.import_package("Core");
        self.import_named("Core", "Class", core, name)
    }

    /// Import of an external object (with its outer chain).
    fn import_ref(&mut self, r: ObjRef) -> PackageIndex {
        if let Some(&i) = self.import_of.get(&r) {
            return i;
        }
        // An object the reference package carries as an export (seek-free content copied
        // in verbatim): that export, whichever loaded package resolved the reference.
        if !self.raw_by_path.is_empty() {
            if let Some(&pos) = self.raw_by_path.get(&self.c.ctx.path(r).to_ascii_lowercase()) {
                let idx = PackageIndex::export(pos);
                self.import_of.insert(r, idx);
                return idx;
            }
        }
        let ctx = &self.c.ctx;
        let name = ctx.name(r);
        let class_name = ctx.class_name(r);
        // A top-level `Package` export is a forced export's original package (seek-free
        // cooking): references to it are imports of that package, not of the file.
        if class_name == "Package" && ctx.outer_of(r).is_none() {
            let idx = self.import_package(&name);
            self.import_of.insert(r, idx);
            return idx;
        }
        let class_pkg = ctx.class_of(r).map(|c| ctx.packages[c.pkg].name.clone()).unwrap_or_else(|| "Core".into());
        let outer = match ctx.outer_of(r) {
            Some(o) => self.import_ref(o),
            None => {
                let pn = self.c.ctx.packages[r.pkg].name.clone();
                self.import_package(&pn)
            }
        };
        let idx = self.import_named(&class_pkg, &class_name, outer, &name);
        self.import_of.insert(r, idx);
        idx
    }

    pub fn index_of(&mut self, f: Option<FieldRef>) -> PackageIndex {
        match f {
            None => PackageIndex(0),
            Some(FieldRef::Local(n)) => {
                // A property the cooker strips: references to it become null.
                if self.opts.cooked && self.stripped_property(n) {
                    return PackageIndex(0);
                }
                if self.c.model.node(n).intrinsic {
                    let node = self.c.model.node(n);
                    let name = node.name.clone();
                    if node.kind != NodeKind::Class {
                        // Member of a phantom class: import under its outer's import.
                        let class_name = match &node.kind {
                            NodeKind::Property(k) => k.class_name().to_string(),
                            NodeKind::ScriptStruct => "ScriptStruct".into(),
                            NodeKind::Function => "Function".into(),
                            NodeKind::Enum => "Enum".into(),
                            _ => "Object".into(),
                        };
                        let outer = self.index_of(node.outer.map(FieldRef::Local));
                        return self.import_named("Core", &class_name, outer, &name);
                    }
                    let pkg = node.intrinsic_package.clone().unwrap_or_else(|| "Core".into());
                    let outer = self.import_package(&pkg);
                    return self.import_named("Core", "Class", outer, &name);
                }
                match self.export_of.get(&n) {
                    Some(&e) => PackageIndex::export(e),
                    None => PackageIndex(0),
                }
            }
            Some(FieldRef::External(r)) => self.import_ref(r),
        }
    }

    fn assign_exports(&mut self) {
        // Collect every export, then order them the way SavePackage does.
        let classes = self.c.model.classes.clone();
        for class in classes {
            self.assign_node(class);
            match self.c.model.node(class).class.cdo {
                Some(cdo) => self.assign_node(cdo),
                None => self.items.push(Item::Cdo(class)),
            }
        }
        if self.opts.keep_source {
            let classes = self.c.model.classes.clone();
            for class in classes {
                self.items.push(Item::ScriptText(class));
                if self.c.model.node(class).strct.cpp_text.is_some() {
                    self.items.push(Item::CppText(class));
                }
                self.assign_struct_cpp_texts(class);
            }
        }
        let mut items = std::mem::take(&mut self.items);
        let mut keyed: Vec<(Vec<u8>, Item)> = items.drain(..).map(|it| (self.full_name(it).to_ascii_lowercase().into_bytes(), it)).collect();
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        let sorted: Vec<Item> = keyed.into_iter().map(|(_, it)| it).collect();
        self.items = super::exportsort::Sorter::new(self).sort(&sorted);
        if let Some(reference) = self.opts.conform_exports.clone() {
            let mut by_path: std::collections::HashMap<String, Item> = std::collections::HashMap::new();
            for &it in &self.items {
                by_path.insert(self.item_path(it).to_ascii_lowercase(), it);
            }
            let mut used: HashSet<Item> = HashSet::new();
            let mut conformed: Vec<Item> = Vec::new();
            self.raw_of.clear();
            self.raw_by_path.clear();
            for (pos, path) in reference.iter().enumerate() {
                match by_path.get(path) {
                    Some(&it) if !path.is_empty() && used.insert(it) => conformed.push(it),
                    _ => {
                        self.raw_of.insert(conformed.len(), pos);
                        if !path.is_empty() {
                            self.raw_by_path.insert(path.clone(), conformed.len());
                        }
                        conformed.push(Item::Null)
                    }
                }
            }
            for &it in &self.items {
                if !used.contains(&it) {
                    conformed.push(it);
                }
            }
            self.items = conformed;
        }
        self.export_of.clear();
        self.cdo_of.clear();
        self.script_text_of.clear();
        self.cpp_text_of.clear();
        for (i, it) in self.items.iter().enumerate() {
            match *it {
                Item::Node(n) => {
                    self.export_of.insert(n, i);
                    let node = self.c.model.node(n);
                    if node.kind == NodeKind::Object && node.object_flags & rf::CLASS_DEFAULT_OBJECT != 0 {
                        if let Some(oc) = node.object_class {
                            if let FieldRef::Local(c) = oc {
                                self.cdo_of.insert(c, i);
                            }
                        }
                    }
                }
                Item::Cdo(c) => {
                    self.cdo_of.insert(c, i);
                }
                Item::ScriptText(c) => {
                    self.script_text_of.insert(c, i);
                }
                Item::CppText(c) => {
                    self.cpp_text_of.insert(c, i);
                }
                Item::Null => {}
            }
        }
    }

    /// `UObject::GetFullName()`: `"ClassName Package.Outer.Name"`.
    fn full_name(&self, it: Item) -> String {
        let pkg = &self.c.model.package_name;
        match it {
            Item::Node(n) => {
                let node = self.c.model.node(n);
                let class_name = match &node.kind {
                    NodeKind::Class => "Class".to_string(),
                    NodeKind::State => "State".into(),
                    NodeKind::Function => "Function".into(),
                    NodeKind::ScriptStruct => "ScriptStruct".into(),
                    NodeKind::Enum => "Enum".into(),
                    NodeKind::Const => "Const".into(),
                    NodeKind::Property(k) => k.class_name().to_string(),
                    NodeKind::Object => node.object_class.map(|c| self.c.res().name_of(c)).unwrap_or_else(|| "Object".into()),
                };
                format!("{class_name} {pkg}.{}", self.c.model.path(n))
            }
            Item::Cdo(c) => {
                let cn = self.c.model.node(c).name.clone();
                format!("{cn} {pkg}.Default__{cn}")
            }
            Item::ScriptText(c) => format!("TextBuffer {pkg}.{}.ScriptText", self.c.model.path(c)),
            Item::CppText(c) => format!("TextBuffer {pkg}.{}.CppText", self.c.model.path(c)),
            Item::Null => String::new(),
        }
    }

    /// Whether this sub-object is reachable from a class default object or a class
    /// component map through property values (`SavePackage` only tags what is referenced).
    fn subobject_referenced(&self, s: NodeId) -> bool {
        let mut cache = self.referenced_subobjects.borrow_mut();
        if cache.is_none() {
            let mut set: HashSet<NodeId> = HashSet::new();
            let mut stack: Vec<NodeId> = Vec::new();
            for (i, node) in self.c.model.nodes.iter().enumerate() {
                match node.kind {
                    NodeKind::Class => {
                        for (_, r) in &node.class.component_map {
                            if let Some(FieldRef::Local(x)) = r {
                                stack.push(*x);
                            }
                        }
                    }
                    NodeKind::Object if node.object_flags & rf::CLASS_DEFAULT_OBJECT != 0 => stack.push(i),
                    _ => {}
                }
            }
            while let Some(n) = stack.pop() {
                if !set.insert(n) {
                    continue;
                }
                let node = self.c.model.node(n);
                let values = self.c.values.object_values.get(&FieldRef::Local(n)).or(node.values.as_ref());
                if let Some(v) = values {
                    for r in super::defaults::collect_object_refs(v) {
                        if let FieldRef::Local(x) = r {
                            if self.c.model.node(x).kind == NodeKind::Object {
                                stack.push(x);
                            }
                        }
                    }
                }
            }
            *cache = Some(set);
        }
        cache.as_ref().map(|set| set.contains(&s)).unwrap_or(true)
    }

    /// A bool property value of a defaults object (full layout, so inherited defaults count).
    fn object_bool(&self, n: NodeId, name: &str) -> bool {
        let node = self.c.model.node(n);
        let Some(values) = self.c.values.object_values.get(&FieldRef::Local(n)).or(node.values.as_ref()) else { return false };
        for s in values {
            if self.c.res().name_of(s.prop).eq_ignore_ascii_case(name) {
                if let Some(super::values::Value::Bool(b)) = s.elems.first() {
                    return *b;
                }
            }
        }
        false
    }

    fn object_is_a(&self, n: NodeId, class_name: &str) -> bool {
        match (self.c.model.node(n).object_class, self.c.res().find_class(class_name)) {
            (Some(c), Some(d)) => self.c.res().is_child_of(c, d),
            _ => false,
        }
    }

    /// `UObject::NeedsLoadForClient` and the overrides that matter for templates.
    fn needs_load_for_client(&self, n: NodeId) -> bool {
        if self.c.model.node(n).object_flags & rf::NOT_FOR_CLIENT != 0 {
            return false;
        }
        // `UPrimitiveComponent`: hidden in game, no collision, not always loaded.
        if self.object_is_a(n, "PrimitiveComponent") && self.object_bool(n, "HiddenGame") && !self.object_bool(n, "CollideActors") && !self.object_bool(n, "AlwaysLoadOnClient") {
            return false;
        }
        // `UAnimNode`: editor-only nodes.
        if self.object_is_a(n, "AnimNode") && self.object_bool(n, "bEditorOnly") {
            return false;
        }
        true
    }

    fn needs_load_for_server(&self, n: NodeId) -> bool {
        if self.c.model.node(n).object_flags & rf::NOT_FOR_SERVER != 0 {
            return false;
        }
        if self.object_is_a(n, "PrimitiveComponent") && !self.object_bool(n, "CollideActors") && !self.object_bool(n, "AlwaysLoadOnServer") {
            return false;
        }
        if self.object_is_a(n, "AnimNode") && self.object_bool(n, "bEditorOnly") {
            return false;
        }
        true
    }

    /// Children that reach the package: the cooker strips editor-only properties.
    fn exported_children(&self, node: NodeId) -> Vec<NodeId> {
        let cooked = self.opts.cooked;
        self.c.model.node(node).strct.children.iter().copied().filter(|&ch| !(cooked && self.stripped_property(ch))).collect()
    }

    /// The cooker drops editor-only properties from classes and structs, except
    /// bools (packed bitfields keep their slot).
    fn stripped_property(&self, n: NodeId) -> bool {
        let node = self.c.model.node(n);
        matches!(node.kind, NodeKind::Property(k) if k != crate::objects::PropertyKind::Bool) && node.prop.flags & cpf::EDITOR_ONLY != 0
    }

    fn assign_struct_cpp_texts(&mut self, node: NodeId) {
        let children = self.c.model.node(node).strct.children.clone();
        for ch in children {
            if self.c.model.node(ch).kind == NodeKind::ScriptStruct {
                if self.c.model.node(ch).strct.cpp_text.is_some() {
                    self.items.push(Item::CppText(ch));
                }
                self.assign_struct_cpp_texts(ch);
            }
        }
    }

    fn assign_node(&mut self, n: NodeId) {
        self.items.push(Item::Node(n));
        self.export_of.insert(n, usize::MAX);
        let node = self.c.model.node(n);
        if let NodeKind::Property(crate::objects::PropertyKind::Array) = node.kind {
            if let Some(FieldRef::Local(inner)) = node.prop.ref1 {
                self.assign_node(inner);
            }
        }
        let children = self.exported_children(n);
        for ch in children {
            self.assign_node(ch);
        }
        // Sub-objects of a defaults object (components, nested objects).
        if node.kind == NodeKind::Object {
            let subs: Vec<NodeId> = self.c.model.nodes.iter().enumerate().filter(|(_, x)| x.kind == NodeKind::Object && x.outer == Some(n)).map(|(i, _)| i).collect();
            for s in subs {
                // `SavePackage` only tags what is referenced: a sub-object nothing points at
                // any more (instanced, then replaced by the defaults text) is left out.
                if !self.export_of.contains_key(&s) && self.subobject_referenced(s) {
                    self.assign_node(s);
                }
            }
        }
    }

    fn class_import_for_kind(&mut self, n: NodeId) -> PackageIndex {
        let kind = self.c.model.node(n).kind.clone();
        let name = match kind {
            NodeKind::Class => "Class",
            NodeKind::State => "State",
            NodeKind::Function => "Function",
            NodeKind::ScriptStruct => "ScriptStruct",
            NodeKind::Enum => "Enum",
            NodeKind::Const => "Const",
            NodeKind::Property(k) => k.class_name(),
            NodeKind::Object => {
                let oc = self.c.model.node(n).object_class;
                return self.index_of(oc);
            }
        };
        self.import_core_class(name)
    }

    /// Finalize memory-form bytecode into disk form.
    fn finalize_script(&mut self, n: NodeId) -> (Vec<u8>, i32) {
        let node = self.c.model.node(n);
        if node.strct.script_mem.is_empty() {
            return (Vec::new(), 0);
        }
        let none_index = self.c.names.iter().position(|x| x == "None").map(|i| i as u32);
        let mut script = match Disassembler::disassemble_memory(&node.strct.script_mem, none_index) {
            Ok(s) => s,
            Err(e) => panic!("bytecode finalize failed for {}: {e}", self.c.model.path(n)),
        };
        let table: Vec<PackageIndex> = (0..self.c.ref_table.len()).map(|i| self.index_of(Some(self.c.ref_table[i]))).collect();
        let mut map_ref = |p: PackageIndex| -> PackageIndex {
            let h = p.0 as usize;
            if h == 0 {
                PackageIndex(0)
            } else {
                table.get(h - 1).copied().unwrap_or_default()
            }
        };
        // The package name table starts with the compiler's names, so name indices are already valid.
        let mut map_name = |fname: FName| -> FName { fname };
        for s in &mut script.statements {
            s.token.map_refs(&mut map_ref, &mut map_name);
        }
        script.statements.push(Statement { mem_offset: script.mem_size, disk_offset: 0, token: Token::EndOfScript });
        let (bytes, mem) = Assembler::assemble(&script);
        (bytes, mem as i32)
    }

    /// Rewrite pending object references (`PackageIndex(handle)`) inside
    /// tagged property data into package indices.
    fn remap_tagged(&mut self, tags: &[crate::objects::TaggedProperty], owner: FieldRef) -> Vec<crate::objects::TaggedProperty> {
        let table: Vec<PackageIndex> = (0..self.c.ref_table.len()).map(|i| self.index_of(Some(self.c.ref_table[i]))).collect();
        let mut map = |i: PackageIndex| -> PackageIndex {
            let h = i.0 as usize;
            if h == 0 { PackageIndex(0) } else { table.get(h - 1).copied().unwrap_or_default() }
        };
        self.walk_tagged(tags, owner, &mut map)
    }

    /// Walk raw tagged-property bytes with the property definitions, passing every
    /// object reference (`PackageIndex(handle)`) through `map`; returns the rewritten tags.
    pub(super) fn walk_tagged(&self, tags: &[crate::objects::TaggedProperty], owner: FieldRef, map: &mut dyn FnMut(PackageIndex) -> PackageIndex) -> Vec<crate::objects::TaggedProperty> {
        let names = CompilerNames(&self.c.names);
        let mut out = Vec::new();
        for t in tags {
            let mut t = t.clone();
            let name = names.name(t.name.index).to_string();
            let prop = self.c.props_in_layout_order(owner).into_iter().find(|p| self.c.res().name_of(*p).eq_ignore_ascii_case(&name));
            if let Some(p) = prop {
                if t.bool_val.is_none() {
                    let mut r = tempest_upk::io::Reader::new(&t.raw);
                    let mut w = tempest_upk::io::Writer::new();
                    self.remap_item(&mut r, &mut w, p, map, &names, true);
                    t.raw = w.into_inner();
                    t.size = t.raw.len() as i32;
                }
            }
            out.push(t);
        }
        out
    }

    fn remap_item(&self, r: &mut tempest_upk::io::Reader, w: &mut tempest_upk::io::Writer, prop: FieldRef, map: &mut dyn FnMut(PackageIndex) -> PackageIndex, names: &CompilerNames, tagged_ctx: bool) {
        use crate::objects::PropertyKind as K;
        let res = self.c.res();
        let kind = res.prop_kind(prop).unwrap_or(K::Int);
        match kind {
            K::Byte => {
                if res.prop_refs(prop).0.is_some() {
                    if let Ok(n) = r.fname() { w.fname(n); }
                } else if let Ok(b) = r.u8() { w.u8(b); }
            }
            K::Int | K::Float => { if let Ok(v) = r.u32() { w.u32(v); } }
            K::Bool => { if let Ok(b) = r.u8() { w.u8(b); } }
            K::Name => { if let Ok(n) = r.fname() { w.fname(n); } }
            K::Str => { if let Ok(s) = r.fstring() { w.fstring(&s); } }
            K::Object | K::Class | K::Component | K::SafeObject | K::Interface => { if let Ok(i) = r.index() { w.index(map(i)); } }
            K::Delegate => {
                if let Ok(i) = r.index() { w.index(map(i)); }
                if let Ok(n) = r.fname() { w.fname(n); }
            }
            K::Struct => {
                let Some(s) = res.prop_refs(prop).0 else { return };
                let f = res.struct_flags(s);
                let binary = f & tempest_upk::flags::strct::IMMUTABLE != 0 || (self.c.cooked && f & tempest_upk::flags::strct::IMMUTABLE_WHEN_COOKED != 0);
                if binary {
                    for p in self.c.props_in_layout_order(s) {
                        let dim = res.prop_array_dim(p).max(1);
                        for _ in 0..dim {
                            self.remap_item(r, w, p, map, names, false);
                        }
                    }
                } else if let Ok(tags) = crate::objects::read_tagged_properties(r, names) {
                    let mut out = Vec::new();
                    for t in tags {
                        let mut t = t.clone();
                        let name = names.name(t.name.index).to_string();
                        if t.bool_val.is_none() {
                            if let Some(p) = self.c.props_in_layout_order(s).into_iter().find(|p| res.name_of(*p).eq_ignore_ascii_case(&name)) {
                                let mut rr = tempest_upk::io::Reader::new(&t.raw);
                                let mut ww = tempest_upk::io::Writer::new();
                                self.remap_item(&mut rr, &mut ww, p, map, names, true);
                                t.raw = ww.into_inner();
                                t.size = t.raw.len() as i32;
                            }
                        }
                        out.push(t);
                    }
                    let none = FName { index: self.c.names.iter().position(|n| n == "None").unwrap_or(0) as u32, number: 0 };
                    crate::objects::write_tagged_properties(w, &out, none);
                }
            }
            K::Array => {
                let Some(inner) = res.prop_refs(prop).0 else { return };
                let n = r.i32().unwrap_or(0);
                w.i32(n);
                for _ in 0..n.max(0) {
                    self.remap_item(r, w, inner, map, names, false);
                }
            }
            K::Map => {}
        }
        let _ = tagged_ctx;
    }

    fn object_flags_for(&self, n: NodeId) -> u64 {
        let node = self.c.model.node(n);
        let mut f = node.object_flags & rf::LOAD;
        f |= rf::LOAD_FOR_CLIENT | rf::LOAD_FOR_SERVER | rf::LOAD_FOR_EDIT;
        if node.kind == NodeKind::Class {
            f |= rf::STANDALONE;
        }
        f
    }

    fn struct_data(&mut self, n: NodeId) -> StructData {
        let (script, mem) = self.finalize_script(n);
        let node = self.c.model.node(n);
        let super_struct = self.index_of(node.strct.super_struct);
        let first_child = self.exported_children(n).first().copied();
        let node = self.c.model.node(n);
        let children = self.index_of(first_child.map(FieldRef::Local));
        let script_text = if node.kind == NodeKind::Class { self.script_text_of.get(&n).map(|&i| PackageIndex::export(i)).unwrap_or_default() } else { PackageIndex(0) };
        let cpp_text = self.cpp_text_of.get(&n).map(|&i| PackageIndex::export(i)).unwrap_or_default();
        let (mut line, mut text_pos) = (node.strct.line, node.strct.text_pos);
        if let Some(map) = &self.opts.conform_pos {
            if let Some(&(l, t)) = map.get(&self.item_path(self.items[self.current_export]).to_ascii_lowercase()) {
                line = l;
                text_pos = t;
            }
        }
        StructData { super_struct, script_text, children, cpp_text, line, text_pos, bytecode_size: mem, script }
    }

    fn state_data(&mut self, n: NodeId) -> StateData {
        let node = self.c.model.node(n);
        let fm = node.state.func_map.clone();
        let func_map = fm.into_iter().map(|(name, id)| (self.fname(&name), self.index_of(Some(FieldRef::Local(id))))).collect();
        StateData { probe_mask: node.state.probe_mask, label_table_offset: node.state.label_table_offset, state_flags: node.state.state_flags, func_map }
    }

    fn class_data(&mut self, n: NodeId) -> ClassData {
        let node = self.c.model.node(n);
        let k = node.class.clone();
        let class_within = self.index_of(k.class_within);
        let class_config_name = self.fname(if k.config_name.is_empty() { "None" } else { &k.config_name });
        let mut entries: Vec<(String, Option<FieldRef>)> = k.component_map.clone();
        if let Some(reference) = self.opts.conform_components.as_ref().and_then(|m| m.get(&node.name.to_ascii_lowercase())).cloned() {
            let mut merged: Vec<(String, Option<FieldRef>)> = Vec::new();
            for (name, is_null) in &reference {
                match entries.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                    Some(e) => merged.push(e.clone()),
                    None if *is_null => merged.push((name.clone(), None)),
                    None => {}
                }
            }
            // Entries the reference lacks were dropped by the cooker (inherited components
            // of another package that no longer exist there).
            entries = merged;
        }
        let component_name_to_default_object = entries.iter().map(|(name, id)| (self.fname(name), self.index_of(*id))).collect();
        let interfaces = k.interfaces.iter().map(|(i, p)| (self.index_of(Some(*i)), self.index_of(p.map(FieldRef::Local)))).collect();
        let editor = if self.layout.editor_data {
            Some(ClassEditorData {
                dont_sort_categories: k.dont_sort_categories.iter().map(|s| self.fname(s)).collect(),
                hide_categories: k.hide_categories.iter().map(|s| self.fname(s)).collect(),
                auto_expand_categories: k.auto_expand_categories.iter().map(|s| self.fname(s)).collect(),
                auto_collapse_categories: k.auto_collapse_categories.iter().map(|s| self.fname(s)).collect(),
                force_script_order: k.force_script_order as u32,
                class_group_names: k.class_group_names.iter().map(|s| self.fname(s)).collect(),
                class_header_filename: k.header_filename.clone(),
            })
        } else {
            None
        };
        let dll_bind_name = self.fname(if k.dll_bind_name.is_empty() { "None" } else { &k.dll_bind_name });
        let class_default_object = self.cdo_of.get(&n).map(|&i| PackageIndex::export(i)).unwrap_or_default();
        ClassData { class_flags: k.class_flags, class_within, class_config_name, component_name_to_default_object, interfaces, editor, dll_bind_name, class_default_object }
    }

    fn property_data(&mut self, n: NodeId) -> PropertyData {
        let node = self.c.model.node(n);
        let p = node.prop.clone();
        let category = Some(self.fname(p.category.as_deref().unwrap_or("None")));
        let array_size_enum = Some(self.index_of(p.array_size_enum));
        let rep_offset = if p.flags & tempest_upk::flags::cpf::NET != 0 { Some(p.rep_offset) } else { None };
        let ref1 = Some(self.index_of(p.ref1));
        let ref2 = Some(self.index_of(p.ref2));
        PropertyData { array_dim: p.array_dim, property_flags: p.flags, category, array_size_enum, rep_offset, ref1, ref2 }
    }

    fn header(&mut self, props: Vec<crate::objects::TaggedProperty>) -> ObjectHeader {
        // The cooker saves objects loaded from the compiled package, whose NetIndex is
        // their linker index; freshly compiled objects have none.
        let net_index = if let Some(map) = &self.opts.net_index_map {
            let path = self.item_path(self.items[self.current_export]).to_ascii_lowercase();
            match map.get(&path) {
                Some(&i) => i,
                None if self.next_net_index > 0 => {
                    let i = self.next_net_index;
                    self.next_net_index += 1;
                    i
                }
                None => -1,
            }
        } else if self.opts.cooked {
            self.current_export as i32
        } else {
            -1
        };
        let item = self.items[self.current_export];
        let mut props = props;
        if let Some(map) = &self.opts.conform_tags {
            let path = self.item_path(item).to_ascii_lowercase();
            if let Some(reference) = map.get(&path).cloned() {
                let ours: Vec<(String, i32)> = props.iter().map(|t| (self.pkg.fname_to_string(t.name).to_ascii_lowercase(), t.array_index)).collect();
                let mut merged: Vec<crate::objects::TaggedProperty> = Vec::new();
                let mut used = vec![false; props.len()];
                for (name, ty, idx, is_null) in &reference {
                    let lname = name.to_ascii_lowercase();
                    if let Some(pos) = ours.iter().position(|(n, i)| *n == lname && *i == *idx) {
                        if !used[pos] {
                            used[pos] = true;
                            merged.push(props[pos].clone());
                        }
                    } else if *is_null {
                        let name = self.fname(name);
                        let type_name = self.fname(ty);
                        merged.push(crate::objects::TaggedProperty { name, type_name, array_index: *idx, struct_name: None, bool_val: None, enum_name: None, size: 4, raw: vec![0; 4] });
                    }
                }
                for (i, t) in props.into_iter().enumerate() {
                    if !used[i] {
                        merged.push(t);
                    }
                }
                props = merged;
            }
        }
        // `UDominant*LightComponent::Serialize`: shadow map before everything (empty on templates).
        let native_prefix = match item {
            Item::Node(n) if self.c.model.node(n).kind == NodeKind::Object && self.c.model.node(n).object_flags & rf::CLASS_DEFAULT_OBJECT == 0 => {
                let class = self.c.model.node(n).object_class;
                let dominant = ["DominantDirectionalLightComponent", "DominantSpotLightComponent"]
                    .iter()
                    .any(|name| match (self.c.res().find_class(name), class) {
                        (Some(d), Some(c)) => self.c.res().is_child_of(c, d),
                        _ => false,
                    });
                if dominant { vec![0u8; 4] } else { Vec::new() }
            }
            _ => Vec::new(),
        };
        ObjectHeader { native_prefix, state_frame: None, component: None, net_index, properties: props }
    }

    /// `Outer.Name` path of an export within the package.
    fn item_path(&self, it: Item) -> String {
        match it {
            Item::Node(n) => self.c.model.path(n),
            Item::Cdo(c) => format!("Default__{}", self.c.model.node(c).name),
            Item::ScriptText(c) => format!("{}.ScriptText", self.c.model.path(c)),
            Item::CppText(c) => format!("{}.CppText", self.c.model.path(c)),
            Item::Null => String::new(),
        }
    }

    fn build_node(&mut self, n: NodeId) -> ParsedObject {
        let kind = self.c.model.node(n).kind.clone();
        let header = self.header(Vec::new());
        let next = {
            // Next sibling in the outer's children list (array inners have no siblings).
            let node = self.c.model.node(n);
            match node.outer {
                Some(o) => {
                    let siblings = self.exported_children(o);
                    match siblings.iter().position(|&c| c == n) {
                        Some(i) => Some(siblings.get(i + 1).map(|&s| PackageIndex::export(self.export_of[&s])).unwrap_or_default()),
                        None => Some(PackageIndex(0)),
                    }
                }
                None => Some(PackageIndex(0)),
            }
        };
        let body = match kind {
            NodeKind::Class => {
                let s = self.struct_data(n);
                let state = self.state_data(n);
                let class = self.class_data(n);
                ObjectBody::Class { s, state, class }
            }
            NodeKind::State => {
                let s = self.struct_data(n);
                let state = self.state_data(n);
                ObjectBody::State { s, state }
            }
            NodeKind::Function => {
                let s = self.struct_data(n);
                let f = self.c.model.node(n).func.clone();
                let function = FunctionData {
                    native_index: f.native_index,
                    oper_precedence: f.precedence,
                    function_flags: f.flags,
                    rep_offset: if f.flags & tempest_upk::flags::func::NET != 0 { Some(f.rep_offset) } else { None },
                    friendly_name: self.fname(&f.friendly_name),
                };
                ObjectBody::Function { s, function }
            }
            NodeKind::ScriptStruct => {
                let s = self.struct_data(n);
                let tags = self.c.model.node(n).default_props.clone();
                let defaults = self.remap_tagged(&tags, FieldRef::Local(n));
                let node = self.c.model.node(n);
                let data = ScriptStructData { struct_flags: node.strct.struct_flags, defaults };
                ObjectBody::ScriptStruct { s, data }
            }
            NodeKind::Enum => {
                let names = self.c.model.node(n).enum_names.clone();
                ObjectBody::Enum(EnumData { names: names.iter().map(|x| self.fname(x)).collect() })
            }
            NodeKind::Const => ObjectBody::Const(ConstData { value: self.c.model.node(n).const_value.clone() }),
            NodeKind::Property(k) => {
                let data = self.property_data(n);
                ObjectBody::Property { kind: k, data }
            }
            NodeKind::Object => {
                let tags = self.c.model.node(n).default_props.clone();
                let props = match self.c.model.node(n).object_class {
                    Some(oc) => self.remap_tagged(&tags, oc),
                    None => tags,
                };
                let node = self.c.model.node(n);
                let component = node.component_template.clone().map(|(owner, name)| crate::objects::ComponentHeader {
                    template_owner_class: self.index_of(owner),
                    template_name: name.map(|s| self.fname(&s)),
                });
                let mut h = self.header(props);
                h.component = component;
                let rest = self.c.model.node(n).native_rest.clone();
                return ParsedObject { header: h, next: None, body: ObjectBody::Other { rest } };
            }
        };
        ParsedObject { header, next, body }
    }

    pub fn build(mut self, package_name: &str) -> Package {
        // Name table: the compiler's names first so bytecode name indices stay valid.
        for n in self.c.names.clone() {
            self.pkg.names.push(NameEntry { name: n, flags: 0x0007_0010_0000_0000 });
        }
        if let Some(flags) = &self.opts.conform_name_flags {
            for (i, f) in flags.iter().enumerate() {
                if let Some(e) = self.pkg.names.get_mut(i) {
                    e.flags = *f;
                }
            }
        }
        self.pkg.rebuild_name_lookup();
        if let Some(imports) = self.opts.conform_imports.clone() {
            for (cp, cn, outer, name) in imports {
                // Every entry is kept (the reference may hold duplicates) so indices line up.
                let idx = PackageIndex::import(self.pkg.imports.len());
                let entry = ObjectImport { class_package: self.fname(&cp), class_name: self.fname(&cn), outer: PackageIndex(outer), object_name: self.fname(&name) };
                self.pkg.imports.push(entry);
                self.named_imports.entry((cp.to_ascii_lowercase(), cn.to_ascii_lowercase(), outer, name.to_ascii_lowercase())).or_insert(idx);
            }
        }
        self.assign_exports();
        let n = self.items.len();
        let mut exports: Vec<ObjectExport> = Vec::with_capacity(n);
        let mut data: Vec<Vec<u8>> = Vec::with_capacity(n);
        let none = self.none();
        for i in 0..n {
            self.current_export = i;
            let item = self.items[i].clone();
            let (export, payload) = match item {
                Item::Null => match self.raw_of.get(&i).and_then(|&rp| self.opts.conform_raw.as_ref().and_then(|v| v.get(rp)).cloned().flatten()) {
                    Some((export, payload)) => (export, payload),
                    None => (ObjectExport { object_name: none, ..Default::default() }, Vec::new()),
                },
                Item::Node(id) => {
                    let obj = self.build_node(id);
                    let node = self.c.model.node(id);
                    let class = self.class_import_for_kind(id);
                    let super_ = match node.kind {
                        NodeKind::Class | NodeKind::State | NodeKind::Function | NodeKind::ScriptStruct => self.index_of(node.strct.super_struct),
                        _ => PackageIndex(0),
                    };
                    let outer = match node.outer {
                        Some(o) => PackageIndex::export(self.export_of[&o]),
                        None => PackageIndex(0),
                    };
                    let archetype = match node.kind {
                        // SavePackage: an archetype that is the class default object is implied (0).
                        NodeKind::Object => {
                            let is_cdo = match (node.archetype, node.object_class) {
                                (Some(FieldRef::External(a)), Some(FieldRef::External(c))) => self.c.ctx.class_default_object(c) == Some(a),
                                (Some(FieldRef::Local(a)), Some(FieldRef::Local(c))) => self.c.model.node(c).class.cdo == Some(a),
                                _ => false,
                            };
                            if is_cdo { PackageIndex(0) } else { self.index_of(node.archetype) }
                        }
                        _ => PackageIndex(0),
                    };
                    let flags = if node.kind == NodeKind::Object {
                        let mut f = node.object_flags | rf::LOAD_FOR_CLIENT | rf::LOAD_FOR_SERVER | rf::LOAD_FOR_EDIT;
                        // Class default objects are always loaded.
                        let is_cdo = node.object_flags & rf::CLASS_DEFAULT_OBJECT != 0;
                        if !is_cdo && !self.needs_load_for_client(id) {
                            f &= !rf::LOAD_FOR_CLIENT;
                        }
                        if !is_cdo && !self.needs_load_for_server(id) {
                            f &= !rf::LOAD_FOR_SERVER;
                        }
                        f
                    } else {
                        self.object_flags_for(id)
                    };
                    let name = self.fname(&node.name.clone());
                    let payload = write_export(&obj, self.layout, none);
                    (ObjectExport { class, super_: super_, outer, object_name: name, archetype, object_flags: flags, ..Default::default() }, payload)
                }
                Item::Cdo(class) => {
                    let class_idx = PackageIndex::export(self.export_of[&class]);
                    let node = self.c.model.node(class);
                    let archetype = match node.strct.super_struct {
                        Some(FieldRef::Local(p)) => self.cdo_of.get(&p).map(|&i| PackageIndex::export(i)).unwrap_or_default(),
                        Some(FieldRef::External(p)) => match self.c.ctx.class_default_object(p) {
                            Some(cdo) => self.import_ref(cdo),
                            None => PackageIndex(0),
                        },
                        None => PackageIndex(0),
                    };
                    let name = self.fname(&format!("Default__{}", node.name));
                    let obj = ParsedObject { header: self.header(Vec::new()), next: None, body: ObjectBody::Other { rest: Vec::new() } };
                    let payload = write_export(&obj, self.layout, none);
                    (
                        ObjectExport {
                            class: class_idx,
                            super_: PackageIndex(0),
                            outer: PackageIndex(0),
                            object_name: name,
                            archetype,
                            object_flags: rf::CLASS_DEFAULT_OBJECT | rf::PUBLIC | rf::LOAD_FOR_CLIENT | rf::LOAD_FOR_SERVER | rf::LOAD_FOR_EDIT,
                            ..Default::default()
                        },
                        payload,
                    )
                }
                Item::ScriptText(class) | Item::CppText(class) => {
                    let is_script = matches!(item, Item::ScriptText(_));
                    let class_import = self.import_core_class("TextBuffer");
                    let outer = PackageIndex::export(self.export_of[&class]);
                    let text = super::engine::cp1252_string(&if is_script {
                        self.c.sources.get(&class).cloned().unwrap_or_default()
                    } else {
                        self.c.model.node(class).strct.cpp_text.clone().unwrap_or_default()
                    });
                    let name = self.fname(if is_script { "ScriptText" } else { "CppText" });
                    let obj = ParsedObject { header: self.header(Vec::new()), next: None, body: ObjectBody::TextBuffer(TextBufferData { pos: 0, top: 0, text }) };
                    let payload = write_export(&obj, self.layout, none);
                    (
                        ObjectExport {
                            class: class_import,
                            super_: PackageIndex(0),
                            outer,
                            object_name: name,
                            archetype: PackageIndex(0),
                            object_flags: rf::LOAD_FOR_EDIT | rf::NOT_FOR_CLIENT | rf::NOT_FOR_SERVER,
                            ..Default::default()
                        },
                        payload,
                    )
                }
            };
            exports.push(export);
            data.push(payload);
        }
        // `FArchiveSaveTagExports`: a template in this package gets the load flags of
        // every object instanced from it, so those objects can always load.
        let load_mask = rf::LOAD_FOR_CLIENT | rf::LOAD_FOR_SERVER | rf::LOAD_FOR_EDIT;
        loop {
            let mut changed = false;
            for i in 0..exports.len() {
                if let Some(a) = exports[i].archetype.export_index() {
                    let add = exports[i].object_flags & load_mask;
                    if a < exports.len() && exports[a].object_flags & add != add {
                        exports[a].object_flags |= add;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        self.pkg.exports = exports;
        self.pkg.export_data = data;
        self.pkg.depends = vec![Vec::new(); n];
        let s = &mut self.pkg.summary;
        s.tag = PACKAGE_FILE_TAG;
        s.file_version = self.opts.file_version;
        s.licensee_version = self.opts.licensee_version;
        s.folder_name = "None".into();
        s.package_flags = if self.opts.cooked {
            pkgf::ALLOW_DOWNLOAD | pkgf::COOKED | pkgf::DISALLOW_LAZY_LOADING | pkgf::CONTAINS_SCRIPT | pkgf::NO_EXPORT_ALLOWED | pkgf::STRIPPED_SOURCE | pkgf::FILTER_EDITOR_ONLY
        } else {
            pkgf::ALLOW_DOWNLOAD | pkgf::CONTAINS_SCRIPT
        };
        s.guid = make_guid(package_name);
        s.generations = vec![tempest_upk::GenerationInfo { export_count: n as i32, name_count: 0, net_object_count: n as i32 }];
        s.engine_version = ENGINE_VERSION;
        s.cooked_content_version = if self.opts.cooked { tempest_upk::flags::COOKED_CONTENT_VERSION } else { 0 };
        s.package_source = 0;
        let names = self.pkg.names.len() as i32;
        self.pkg.summary.generations[0].name_count = names;
        if let Some(r) = &self.opts.conform_summary {
            // Identity of the package being replaced.
            let s = &mut self.pkg.summary;
            s.guid = r.guid;
            s.generations = r.generations.clone();
            s.folder_name = r.folder_name.clone();
            s.engine_version = r.engine_version;
            s.cooked_content_version = r.cooked_content_version;
            s.package_flags = r.package_flags;
            s.package_source = r.package_source;
            s.additional_packages_to_cook = r.additional_packages_to_cook.clone();
            s.texture_allocations = r.texture_allocations.clone();
        }
        self.pkg
    }
}

fn make_guid(seed: &str) -> Guid {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes().chain(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0).to_le_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let h2 = h.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(31);
    Guid { a: h as u32, b: (h >> 32) as u32, c: h2 as u32, d: (h2 >> 32) as u32 }
}

/// `NameTable` over the compiler's interned names.
struct CompilerNames<'a>(&'a [String]);

impl<'a> crate::objects::NameTable for CompilerNames<'a> {
    fn name(&self, index: u32) -> &str {
        self.0.get(index as usize).map(|s| s.as_str()).unwrap_or("None")
    }
}
