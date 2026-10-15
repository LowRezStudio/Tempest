//! Export ordering as `UObject::SavePackage` does it (UE3 10897, `SavePackage.cpp`):
//! exports are first sorted alphabetically by full name (`FObjectExportSortHelper`),
//! then re-ordered for seek-free loading by `FObjectExportSeekFreeSorter`, which
//! replays object serialization through `FExportReferenceSorter` so that every
//! object's class, archetype, super and fields precede it.
//!
//! External objects (from dependency packages) are walked too: the engine inserts
//! them into its working list and drops them at the end, which never changes the
//! relative order of local objects, so they are not listed here; but walking them
//! marks them processed and resets `CurrentClass`, both of which change how later
//! local fields are placed.

use std::collections::HashSet;

use tempest_upk::flags::{cpf, rf, strct};
use tempest_upk::io::{PackageIndex, Reader};

use super::model::{FieldRef, NodeId, NodeKind};
use super::output::{Emitter, Item};
use crate::bytecode::Disassembler;
use crate::context::ObjRef;
use crate::objects::{ObjectBody, PropertyKind, TaggedProperty};

/// Classes pre-seeded by `FExportReferenceSorter::InitializeCoreClasses`.
const CORE_CLASSES: &[&str] = &[
    "Object", "Field", "Struct", "ScriptStruct", "Function", "State", "Enum", "Class", "Const", "Property",
    "ByteProperty", "IntProperty", "BoolProperty", "FloatProperty", "ObjectProperty", "ComponentProperty",
    "ClassProperty", "InterfaceProperty", "NameProperty", "StrProperty", "ArrayProperty", "MapProperty",
    "StructProperty", "DelegateProperty", "Component", "Interface", "Subsystem",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Obj {
    Local(Item),
    Ext(ObjRef),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Class,
    State,
    Function,
    ScriptStruct,
    Enum,
    Const,
    Property(PropertyKind),
    Object,
}

struct NameShim<'p> {
    pkg: &'p tempest_upk::Package,
    none: u32,
}

impl<'p> crate::objects::NameTable for NameShim<'p> {
    fn name(&self, index: u32) -> &str {
        self.pkg.name_str(index)
    }
    fn is_none(&self, n: tempest_upk::io::FName) -> bool {
        n.index == self.none && n.number == 0
    }
}

pub(super) struct Sorter<'e, 'c, 'a> {
    e: &'e Emitter<'c, 'a>,
    processed: HashSet<Obj>,
    serialized: HashSet<Obj>,
    referenced: Vec<Item>,
    current_insert: Option<usize>,
    core_offset: usize,
    core_classes: Vec<Item>,
    ignore_fields: bool,
    current_class: Option<Obj>,
    /// `Ar.IsSerializingDefaults()` while walking an external object's values.
    bin_defaults: std::cell::Cell<bool>,
}

impl<'e, 'c, 'a> Sorter<'e, 'c, 'a> {
    pub fn new(e: &'e Emitter<'c, 'a>) -> Self {
        let mut s = Sorter {
            e,
            processed: HashSet::new(),
            serialized: HashSet::new(),
            referenced: Vec::new(),
            current_insert: None,
            core_offset: 0,
            core_classes: Vec::new(),
            // The engine never initializes `bIgnoreFieldReferences`; the observed
            // package layouts (fields left over for the second pass) show it is set.
            ignore_fields: true,
            bin_defaults: std::cell::Cell::new(true),
            current_class: None,
        };
        s.initialize_core_classes();
        s
    }

    // ----- object model queries -------------------------------------------------

    fn node(&self, n: NodeId) -> &super::model::Node {
        self.e.c.model.node(n)
    }

    fn ctx(&self) -> &crate::context::Context {
        self.e.c.ctx
    }

    fn of(&self, f: Option<FieldRef>) -> Option<Obj> {
        match f {
            Some(FieldRef::Local(n)) if !self.node(n).intrinsic => Some(Obj::Local(Item::Node(n))),
            Some(FieldRef::Local(n)) => {
                // Intrinsic stand-in: the real class lives in Core if loaded.
                let name = self.node(n).name.clone();
                self.ext_class_named(&name)
            }
            Some(FieldRef::External(r)) => Some(Obj::Ext(r)),
            None => None,
        }
    }

    fn ext_class_named(&self, name: &str) -> Option<Obj> {
        let ctx = self.ctx();
        let core = ctx.slot_of("Core")?;
        ctx.find_export(core, name).map(Obj::Ext)
    }

    fn rx(&self, pkg: usize, idx: PackageIndex) -> Option<Obj> {
        self.ctx().resolve(pkg, idx).map(Obj::Ext)
    }

    fn cdo_item(&self, class: NodeId) -> Item {
        match self.node(class).class.cdo {
            Some(cdo) => Item::Node(cdo),
            None => Item::Cdo(class),
        }
    }

    fn kind(&self, o: Obj) -> Kind {
        match o {
            Obj::Local(Item::Node(n)) => match self.node(n).kind.clone() {
                NodeKind::Class => Kind::Class,
                NodeKind::State => Kind::State,
                NodeKind::Function => Kind::Function,
                NodeKind::ScriptStruct => Kind::ScriptStruct,
                NodeKind::Enum => Kind::Enum,
                NodeKind::Const => Kind::Const,
                NodeKind::Property(k) => Kind::Property(k),
                NodeKind::Object => Kind::Object,
            },
            Obj::Local(_) => Kind::Object,
            Obj::Ext(r) => match self.ctx().class_name(r).as_str() {
                "Class" => Kind::Class,
                "State" => Kind::State,
                "Function" => Kind::Function,
                "ScriptStruct" => Kind::ScriptStruct,
                "Enum" => Kind::Enum,
                "Const" => Kind::Const,
                other => match PropertyKind::from_class_name(other) {
                    Some(k) => Kind::Property(k),
                    None => Kind::Object,
                },
            },
        }
    }

    fn is_cdo(&self, o: Obj) -> bool {
        match o {
            Obj::Local(Item::Cdo(_)) => true,
            Obj::Local(Item::Node(n)) => self.node(n).kind == NodeKind::Object && self.node(n).object_flags & rf::CLASS_DEFAULT_OBJECT != 0,
            Obj::Local(_) => false,
            Obj::Ext(r) => self.ctx().object_flags(r) & rf::CLASS_DEFAULT_OBJECT != 0,
        }
    }

    fn is_field(&self, o: Obj) -> bool {
        self.kind(o) != Kind::Object
    }

    fn is_struct(&self, o: Obj) -> bool {
        matches!(self.kind(o), Kind::Class | Kind::State | Kind::Function | Kind::ScriptStruct)
    }

    fn is_class(&self, o: Obj) -> bool {
        self.kind(o) == Kind::Class
    }

    fn outer(&self, o: Obj) -> Option<Obj> {
        match o {
            Obj::Local(Item::Node(n)) => self.node(n).outer.map(|x| Obj::Local(Item::Node(x))),
            Obj::Local(Item::Cdo(_)) | Obj::Local(Item::Null) => None,
            Obj::Local(Item::ScriptText(c)) | Obj::Local(Item::CppText(c)) => Some(Obj::Local(Item::Node(c))),
            Obj::Ext(r) => self.ctx().outer_of(r).map(Obj::Ext),
        }
    }

    fn class_of(&self, o: Obj) -> Option<Obj> {
        match o {
            Obj::Local(Item::Node(n)) if self.node(n).kind == NodeKind::Object => self.of(self.node(n).object_class),
            Obj::Local(Item::Node(n)) => {
                let name = match &self.node(n).kind {
                    NodeKind::Class => "Class",
                    NodeKind::State => "State",
                    NodeKind::Function => "Function",
                    NodeKind::ScriptStruct => "ScriptStruct",
                    NodeKind::Enum => "Enum",
                    NodeKind::Const => "Const",
                    NodeKind::Property(k) => k.class_name(),
                    NodeKind::Object => "Object",
                };
                self.ext_class_named(name)
            }
            Obj::Local(Item::Cdo(c)) => Some(Obj::Local(Item::Node(c))),
            Obj::Local(_) => self.ext_class_named("TextBuffer"),
            Obj::Ext(r) => self.ctx().class_of(r).map(Obj::Ext),
        }
    }

    fn archetype(&self, o: Obj) -> Option<Obj> {
        match o {
            Obj::Local(Item::Node(n)) if self.node(n).kind == NodeKind::Object => self.of(self.node(n).archetype),
            Obj::Local(Item::Cdo(c)) => match self.node(c).strct.super_struct {
                Some(FieldRef::Local(p)) if !self.node(p).intrinsic => Some(Obj::Local(self.cdo_item(p))),
                Some(FieldRef::External(p)) => self.ctx().class_default_object(p).map(Obj::Ext),
                _ => None,
            },
            Obj::Local(_) => None,
            Obj::Ext(r) => {
                let a = self.ctx().packages[r.pkg].pkg.exports[r.export].archetype;
                self.rx(r.pkg, a)
            }
        }
    }

    fn inheritance_super(&self, o: Obj) -> Option<Obj> {
        match o {
            Obj::Local(Item::Node(n)) => self.of(self.e.c.res().inheritance_super(FieldRef::Local(n))),
            Obj::Local(_) => None,
            Obj::Ext(r) => {
                if self.kind(o) == Kind::Function {
                    None
                } else {
                    self.ctx().super_of(r).map(Obj::Ext)
                }
            }
        }
    }

    /// `UField::Next`.
    fn next(&self, o: Obj) -> Option<Obj> {
        match o {
            Obj::Local(Item::Node(n)) => {
                let outer = self.node(n).outer?;
                let siblings = &self.node(outer).strct.children;
                let i = siblings.iter().position(|&c| c == n)?;
                siblings.get(i + 1).map(|&s| Obj::Local(Item::Node(s)))
            }
            Obj::Local(_) => None,
            Obj::Ext(r) => {
                let nx = self.ctx().object(r)?.next?;
                self.rx(r.pkg, nx)
            }
        }
    }

    fn first_child(&self, o: Obj) -> Option<Obj> {
        match o {
            Obj::Local(Item::Node(n)) => self.node(n).strct.children.first().map(|&c| Obj::Local(Item::Node(c))),
            Obj::Local(_) => None,
            Obj::Ext(r) => {
                let c = self.ctx().struct_data(r)?.children;
                self.rx(r.pkg, c)
            }
        }
    }

    fn owner_class(&self, o: Obj) -> Option<Obj> {
        match o {
            Obj::Local(Item::Node(n)) => self.e.c.model.owner_class(n).map(|c| Obj::Local(Item::Node(c))),
            Obj::Local(_) => None,
            Obj::Ext(r) => self.ctx().owner_class(r).map(Obj::Ext),
        }
    }

    /// Object references in the order `Serialize` emits them (fields included;
    /// the caller filters them as the engine's archive does).
    fn refs_of(&self, o: Obj) -> Vec<Option<Obj>> {
        match o {
            Obj::Local(it) => self.local_refs(it),
            Obj::Ext(r) => self.ext_refs(r),
        }
    }

    fn local_refs(&self, it: Item) -> Vec<Option<Obj>> {
        let mut out: Vec<Option<Obj>> = Vec::new();
        let Item::Node(n) = it else { return out };
        let o = Obj::Local(it);
        let node = self.node(n);
        match node.kind.clone() {
            NodeKind::Object => {
                if let Some(oc) = node.object_class {
                    out.extend(self.local_tagged_refs(&node.default_props, oc));
                }
                if let Some((Some(owner), _)) = node.component_template.clone() {
                    out.push(self.of(Some(owner)));
                }
            }
            NodeKind::Property(k) => {
                out.push(self.next(o));
                out.push(self.of(node.prop.array_size_enum));
                let (r1, r2) = (self.of(node.prop.ref1), self.of(node.prop.ref2));
                match k {
                    PropertyKind::Byte | PropertyKind::Object | PropertyKind::Component | PropertyKind::SafeObject | PropertyKind::Interface | PropertyKind::Struct | PropertyKind::Array => out.push(r1),
                    PropertyKind::Class | PropertyKind::Map | PropertyKind::Delegate => {
                        out.push(r1);
                        out.push(r2);
                    }
                    _ => {}
                }
            }
            NodeKind::Enum | NodeKind::Const => out.push(self.next(o)),
            NodeKind::Class | NodeKind::State | NodeKind::Function | NodeKind::ScriptStruct => {
                out.push(self.next(o));
                out.push(self.of(node.strct.super_struct));
                if node.kind == NodeKind::Class && self.e.opts.keep_source {
                    out.push(Some(Obj::Local(Item::ScriptText(n))));
                }
                out.push(self.first_child(o));
                if node.strct.cpp_text.is_some() && self.e.opts.keep_source {
                    out.push(Some(Obj::Local(Item::CppText(n))));
                }
                out.extend(self.local_script_refs(n));
                match node.kind {
                    NodeKind::ScriptStruct => out.extend(self.local_tagged_refs(&node.default_props, FieldRef::Local(n))),
                    NodeKind::State => {
                        for (_, f) in &node.state.func_map {
                            out.push(Some(Obj::Local(Item::Node(*f))));
                        }
                    }
                    NodeKind::Class => {
                        out.push(self.of(node.class.class_within));
                        for (_, c) in &node.class.component_map {
                            out.push(self.of(*c));
                        }
                        for (i, p) in &node.class.interfaces {
                            out.push(self.of(Some(*i)));
                            out.push(self.of(p.map(FieldRef::Local)));
                        }
                        out.push(Some(Obj::Local(self.cdo_item(n))));
                    }
                    _ => {}
                }
            }
        }
        out
    }

    fn local_script_refs(&self, n: NodeId) -> Vec<Option<Obj>> {
        let node = self.node(n);
        if node.strct.script_mem.is_empty() {
            return Vec::new();
        }
        let none_index = self.e.c.names.iter().position(|x| x == "None").map(|i| i as u32);
        let Ok(mut script) = Disassembler::disassemble_memory(&node.strct.script_mem, none_index) else { return Vec::new() };
        let mut refs = Vec::new();
        for s in &mut script.statements {
            s.token.map_refs(
                &mut |p: PackageIndex| {
                    refs.push(p);
                    p
                },
                &mut |f| f,
            );
        }
        refs.into_iter().map(|p| self.handle_obj(p)).collect()
    }

    fn handle_obj(&self, p: PackageIndex) -> Option<Obj> {
        let h = p.0 as usize;
        if h == 0 {
            return None;
        }
        self.of(self.e.c.ref_table.get(h - 1).copied())
    }

    fn local_tagged_refs(&self, tags: &[TaggedProperty], owner: FieldRef) -> Vec<Option<Obj>> {
        let mut refs: Vec<PackageIndex> = Vec::new();
        self.e.walk_tagged(tags, owner, &mut |p| {
            refs.push(p);
            p
        });
        refs.into_iter().map(|p| self.handle_obj(p)).collect()
    }

    // ----- external objects --------------------------------------------------------

    fn ext_refs(&self, r: ObjRef) -> Vec<Option<Obj>> {
        let ctx = self.ctx();
        let o = Obj::Ext(r);
        let mut out: Vec<Option<Obj>> = Vec::new();
        let Some(obj) = ctx.object(r) else { return out };
        match self.kind(o) {
            Kind::Object => {
                let class = ctx.class_of(r);
                self.bin_defaults.set(ctx.object_flags(r) & tempest_upk::flags::rf::CLASS_DEFAULT_OBJECT != 0);
                out.extend(self.ext_tagged_refs(&obj.header.properties, class, r.pkg));
                if let Some(c) = &obj.header.component {
                    out.push(self.rx(r.pkg, c.template_owner_class));
                }
            }
            Kind::Property(k) => {
                out.push(self.next(o));
                if let ObjectBody::Property { data, .. } = &obj.body {
                    out.push(data.array_size_enum.and_then(|i| self.rx(r.pkg, i)));
                    let r1 = data.ref1.and_then(|i| self.rx(r.pkg, i));
                    let r2 = data.ref2.and_then(|i| self.rx(r.pkg, i));
                    match k {
                        PropertyKind::Byte | PropertyKind::Object | PropertyKind::Component | PropertyKind::SafeObject | PropertyKind::Interface | PropertyKind::Struct | PropertyKind::Array => out.push(r1),
                        PropertyKind::Class | PropertyKind::Map | PropertyKind::Delegate => {
                            out.push(r1);
                            out.push(r2);
                        }
                        _ => {}
                    }
                }
            }
            Kind::Enum | Kind::Const => out.push(self.next(o)),
            Kind::Class | Kind::State | Kind::Function | Kind::ScriptStruct => {
                let Some(s) = ctx.struct_data(r) else { return out };
                out.push(self.next(o));
                out.push(self.rx(r.pkg, s.super_struct));
                out.push(self.rx(r.pkg, s.script_text));
                out.push(self.rx(r.pkg, s.children));
                out.push(self.rx(r.pkg, s.cpp_text));
                if !s.script.is_empty() {
                    let none = ctx.packages[r.pkg].none_name;
                    if let Ok(mut script) = Disassembler::disassemble(&s.script, Some(none)) {
                        let mut refs = Vec::new();
                        for st in &mut script.statements {
                            st.token.map_refs(
                                &mut |p: PackageIndex| {
                                    refs.push(p);
                                    p
                                },
                                &mut |f| f,
                            );
                        }
                        out.extend(refs.into_iter().map(|p| self.rx(r.pkg, p)));
                    }
                }
                match &obj.body {
                    ObjectBody::ScriptStruct { data, .. } => {
                        self.bin_defaults.set(true);
                        out.extend(self.ext_tagged_refs(&data.defaults, Some(r), r.pkg))
                    }
                    ObjectBody::State { state, .. } => {
                        for (_, f) in &state.func_map {
                            out.push(self.rx(r.pkg, *f));
                        }
                    }
                    ObjectBody::Class { class, state, .. } => {
                        for (_, f) in &state.func_map {
                            out.push(self.rx(r.pkg, *f));
                        }
                        out.push(self.rx(r.pkg, class.class_within));
                        for (_, c) in &class.component_name_to_default_object {
                            out.push(self.rx(r.pkg, *c));
                        }
                        for (i, p) in &class.interfaces {
                            out.push(self.rx(r.pkg, *i));
                            out.push(self.rx(r.pkg, *p));
                        }
                        out.push(self.rx(r.pkg, class.class_default_object));
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// Object references inside tagged-property data of an external object.
    fn ext_tagged_refs(&self, tags: &[TaggedProperty], owner: Option<ObjRef>, pkg: usize) -> Vec<Option<Obj>> {
        let mut out = Vec::new();
        let ctx = self.ctx();
        for tp in tags {
            let pname = ctx.packages[pkg].name_of(tp.name);
            let pdef = owner.and_then(|o| ctx.find_field(o, &pname));
            let kind = match pdef.and_then(|p| ctx.property_kind(p)) {
                Some(k) => k,
                None => {
                    let ty = ctx.packages[pkg].name_of(tp.type_name);
                    match PropertyKind::from_class_name(&ty) {
                        Some(k) => k,
                        None => continue,
                    }
                }
            };
            if tp.bool_val.is_some() {
                continue;
            }
            let mut r = Reader::new(&tp.raw);
            self.ext_value_refs(&mut r, kind, pdef, pkg, tp.enum_name.is_some(), &mut out);
        }
        out
    }

    fn ext_value_refs(&self, r: &mut Reader, kind: PropertyKind, p: Option<ObjRef>, pkg: usize, enum_tag: bool, out: &mut Vec<Option<Obj>>) {
        let ctx = self.ctx();
        match kind {
            PropertyKind::Int | PropertyKind::Float => {
                let _ = r.u32();
            }
            PropertyKind::Bool => {
                let _ = r.u8();
            }
            PropertyKind::Byte => {
                let has_enum = p.and_then(|p| ctx.enum_of_property(p)).is_some() || enum_tag;
                if has_enum && r.remaining() >= 8 {
                    let _ = r.fname();
                } else {
                    let _ = r.u8();
                }
            }
            PropertyKind::Name => {
                let _ = r.fname();
            }
            PropertyKind::Str => {
                let _ = r.fstring();
            }
            PropertyKind::Object | PropertyKind::Class | PropertyKind::Component | PropertyKind::SafeObject | PropertyKind::Interface => {
                if let Ok(i) = r.index() {
                    out.push(self.rx(pkg, i));
                }
            }
            PropertyKind::Delegate => {
                if let Ok(i) = r.index() {
                    out.push(self.rx(pkg, i));
                }
                let _ = r.fname();
            }
            PropertyKind::Struct => {
                let sdef = p.and_then(|p| ctx.property(p).and_then(|(_, d)| d.ref1)).and_then(|s| ctx.resolve(p.unwrap().pkg, s));
                let Some(sdef) = sdef else {
                    let _ = r.take(r.remaining());
                    return;
                };
                let flags = match ctx.object(sdef).map(|o| &o.body) {
                    Some(ObjectBody::ScriptStruct { data, .. }) => data.struct_flags,
                    _ => 0,
                };
                if flags & (strct::IMMUTABLE | strct::IMMUTABLE_WHEN_COOKED) != 0 {
                    let cooked = ctx.packages[pkg].pkg.summary.package_flags & tempest_upk::flags::pkg::COOKED != 0;
                    for m in ctx.properties_in_serialize_order(sdef) {
                        let Some((mk, md)) = ctx.property(m) else { continue };
                        let f = md.property_flags;
                        if f & cpf::NATIVE != 0 || (f & cpf::TRANSIENT != 0 && !self.bin_defaults.get()) || (f & cpf::DEPRECATED != 0 && cooked) {
                            continue;
                        }
                        for _ in 0..md.array_dim.max(1) {
                            self.ext_value_refs(r, mk, Some(m), pkg, false, out);
                        }
                    }
                } else {
                    let none = ctx.packages[pkg].none_name;
                    let inner = crate::objects::read_tagged_properties(r, &NameShim { pkg: &ctx.packages[pkg].pkg, none }).unwrap_or_default();
                    out.extend(self.ext_tagged_refs(&inner, Some(sdef), pkg));
                }
            }
            PropertyKind::Array => {
                let inner = p.and_then(|p| ctx.property(p).and_then(|(_, d)| d.ref1)).and_then(|i| ctx.resolve(p.unwrap().pkg, i));
                let n = r.i32().unwrap_or(0);
                let Some(ip) = inner else { return };
                let Some(ik) = ctx.property_kind(ip) else { return };
                for _ in 0..n.max(0) {
                    self.ext_value_refs(r, ik, Some(ip), pkg, false, out);
                }
            }
            PropertyKind::Map => {}
        }
    }

    // ----- FExportReferenceSorter -------------------------------------------------

    fn initialize_core_classes(&mut self) {
        let mut core: Vec<Obj> = Vec::new();
        for name in CORE_CLASSES {
            let local = self.e.c.model.classes.iter().copied().find(|&c| self.node(c).name.eq_ignore_ascii_case(name));
            let o = match local {
                Some(c) => Obj::Local(Item::Node(c)),
                None => match self.ext_class_named(name) {
                    Some(x) => x,
                    None => continue,
                },
            };
            if core.contains(&o) {
                continue;
            }
            core.push(o);
            if let Obj::Local(Item::Node(c)) = o {
                self.core_classes.push(Item::Node(c));
                self.referenced.push(Item::Node(c));
                self.referenced.push(self.cdo_item(c));
            }
        }
        for c in core {
            self.process_struct(Some(c));
        }
        self.core_offset = self.referenced.len();
    }

    fn add_referenced(&mut self, o: Obj, at: usize) {
        if let Obj::Local(it) = o {
            if !self.referenced.contains(&it) {
                let at = at.min(self.referenced.len());
                self.referenced.insert(at, it);
            }
        }
    }

    fn insert_index(&self) -> usize {
        self.current_insert.unwrap_or(self.referenced.len())
    }

    fn handle_dependency(&mut self, required: Option<Obj>, process: bool) {
        let Some(r) = required else { return };
        let prev_count = self.referenced.len() as isize;
        let prev_insert = self.insert_index() as isize;
        if self.is_struct(r) {
            self.process_struct(Some(r));
        } else if process {
            self.process_object(Some(r));
        } else {
            let outer = self.outer(r);
            if outer.map(|o| !self.processed.contains(&o)).unwrap_or(false) {
                self.handle_dependency(outer, false);
            }
            let arch = self.archetype(r);
            self.process_object(arch);
            let class = self.class_of(r);
            self.process_struct(class);
        }
        let insert_offset = self.insert_index() as isize - prev_insert;
        let advance = (self.referenced.len() as isize - prev_count) - insert_offset;
        if advance > 0 {
            if let Some(ci) = self.current_insert.as_mut() {
                *ci += advance as usize;
            }
        }
    }

    /// `FExportReferenceSorter::operator<<`.
    fn visit(&mut self, obj: Option<Obj>) {
        let Some(o) = obj else { return };
        if self.is_cdo(o) || self.processed.contains(&o) {
            return;
        }
        if self.is_field(o) {
            if !self.ignore_fields && !self.is_class(o) {
                let same_class = match self.current_class {
                    Some(cc) => self.outer(o) == Some(cc),
                    None => false,
                };
                if !same_class {
                    if self.is_struct(o) {
                        self.process_struct(Some(o));
                    } else {
                        if let Kind::Property(PropertyKind::Byte) = self.kind(o) {
                            let e = match o {
                                Obj::Local(Item::Node(n)) => self.of(self.node(n).prop.ref1),
                                Obj::Ext(r) => self.ctx().enum_of_property(r).map(Obj::Ext),
                                _ => None,
                            };
                            if e.is_some() {
                                self.handle_dependency(e, true);
                            }
                        }
                        self.processed.insert(o);
                        let at = self.insert_index();
                        self.add_referenced(o, at);
                        if self.serialized.insert(o) {
                            for r in self.refs_of(o) {
                                self.visit(r);
                            }
                        }
                    }
                }
            }
        } else {
            self.handle_dependency(Some(o), false);
        }
    }

    fn process_object(&mut self, obj: Option<Obj>) {
        let Some(o) = obj else { return };
        if self.is_cdo(o) || self.processed.contains(&o) {
            return;
        }
        self.processed.insert(o);
        let recursive = self.current_insert.is_some();
        if !recursive {
            self.current_insert = Some(self.referenced.len());
        }
        let class = self.class_of(o);
        self.handle_dependency(class, true);
        let outer = self.outer(o);
        self.handle_dependency(outer, false);
        let arch = self.archetype(o);
        self.handle_dependency(arch, true);
        let template_owner = match o {
            Obj::Local(Item::Node(n)) => match self.node(n).component_template.clone() {
                Some((Some(owner), _)) => self.of(Some(owner)),
                _ => None,
            },
            Obj::Ext(r) => self.ctx().object(r).and_then(|x| x.header.component.as_ref()).and_then(|c| self.rx(r.pkg, c.template_owner_class)),
            _ => None,
        };
        if template_owner.is_some() {
            self.handle_dependency(template_owner, false);
        }
        let at = self.insert_index();
        self.add_referenced(o, at);
        if self.serialized.insert(o) {
            for r in self.refs_of(o) {
                self.visit(r);
            }
        }
        if !recursive {
            self.current_insert = None;
        }
    }

    fn process_struct(&mut self, obj: Option<Obj>) {
        let Some(s) = obj else { return };
        if self.processed.contains(&s) {
            return;
        }
        self.processed.insert(s);
        let recursive = self.current_insert.is_some();
        if !recursive {
            self.current_insert = Some(self.referenced.len());
        }
        let sup = self.inheritance_super(s);
        self.handle_dependency(sup, false);
        let at = self.insert_index();
        self.add_referenced(s, at);
        if self.serialized.insert(s) {
            let prev_ignore = self.ignore_fields;
            self.ignore_fields = true;
            for r in self.refs_of(s) {
                self.visit(r);
            }
            self.ignore_fields = false;
            if !self.is_class(s) {
                self.current_class = self.owner_class(s);
            }
            let child = self.first_child(s);
            self.visit(child);
            self.current_class = None;
            let next = self.next(s);
            self.visit(next);
            self.ignore_fields = prev_ignore;
        }
        if self.is_class(s) {
            let cdo = match s {
                Obj::Local(Item::Node(n)) => Some(Obj::Local(self.cdo_item(n))),
                Obj::Ext(r) => self.ctx().class_default_object(r).map(Obj::Ext),
                _ => None,
            };
            if let Some(cdo) = cdo {
                if !self.processed.contains(&cdo) {
                    self.processed.insert(cdo);
                    if self.serialized.insert(cdo) {
                        for r in self.refs_of(cdo) {
                            self.visit(r);
                        }
                    }
                    if let (Obj::Local(si), Obj::Local(ci)) = (s, cdo) {
                        if let Some(pos) = self.referenced.iter().position(|&x| x == si) {
                            self.add_referenced(Obj::Local(ci), pos + 1);
                        }
                    }
                }
            }
        }
        if !recursive {
            self.current_insert = None;
        }
    }

    fn clear(&mut self) {
        self.referenced.truncate(self.core_offset);
    }

    fn export_list(&self, include_core: bool) -> Vec<Item> {
        if include_core { self.referenced.clone() } else { self.referenced[self.core_offset..].to_vec() }
    }

    // ----- FObjectExportSeekFreeSorter ---------------------------------------------

    /// `items` must already be sorted alphabetically by full name.
    pub fn sort(mut self, items: &[Item]) -> Vec<Item> {
        let mut sorted: Vec<Item> = Vec::new();
        let mut first = true;
        for &it in items {
            if self.is_class(Obj::Local(it)) {
                self.clear();
                self.process_struct(Some(Obj::Local(it)));
                sorted.extend(self.export_list(first));
                first = false;
            }
        }
        for &it in items {
            self.clear();
            self.process_object(Some(Obj::Local(it)));
            sorted.extend(self.export_list(first));
            first = false;
        }
        let wanted: HashSet<Item> = items.iter().copied().collect();
        let mut seen: HashSet<Item> = HashSet::new();
        let mut out: Vec<Item> = Vec::new();
        for it in sorted {
            if wanted.contains(&it) && seen.insert(it) {
                out.push(it);
            }
        }
        for &it in items {
            if seen.insert(it) {
                eprintln!("warning: export sorter missed {:?}", it);
                out.push(it);
            }
        }
        out
    }
}
