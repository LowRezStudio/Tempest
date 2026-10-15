//! Field lookup over the package being compiled and the loaded packages
//! (`FScriptCompiler::FindField` and friends).

use std::collections::HashMap;

use tempest_upk::flags::{class as cf, cpf, func, strct};

use super::model::{FieldRef, Model, NodeId, NodeKind};
use super::types::{Cpt, PropType, RefType};
use crate::context::{Context, ObjRef};
use tempest_upk::io::PackageIndex;
use crate::objects::{ObjectBody, PropertyKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Class,
    State,
    Function,
    ScriptStruct,
    Enum,
    Const,
    Property(PropertyKind),
    Other,
}

/// Name-indexed views of the loaded packages.
#[derive(Default)]
pub struct GlobalIndex {
    pub classes: HashMap<String, ObjRef>,
    pub enums: HashMap<String, ObjRef>,
    pub structs: HashMap<String, ObjRef>,
    /// Every enum tag name → index (`GUCCMakeEnumNameToIndexMap`).
    pub enum_tags: HashMap<String, i32>,
}

impl GlobalIndex {
    pub fn build(ctx: &Context) -> GlobalIndex {
        let mut g = GlobalIndex::default();
        for (p, lp) in ctx.packages.iter().enumerate() {
            if lp.shadow {
                continue;
            }
            for i in 0..lp.pkg.exports.len() {
                let r = ObjRef { pkg: p, export: i };
                let cn = ctx.class_name(r);
                let name = ctx.name(r).to_ascii_lowercase();
                match cn.as_str() {
                    "Class" => {
                        g.classes.entry(name).or_insert(r);
                    }
                    "Enum" => {
                        g.enums.entry(name.clone()).or_insert(r);
                        for (i, tag) in ctx.enum_names(r).iter().enumerate() {
                            g.enum_tags.entry(tag.to_ascii_lowercase()).or_insert(i as i32);
                        }
                    }
                    "ScriptStruct" => {
                        g.structs.entry(name).or_insert(r);
                    }
                    _ => {}
                }
            }
        }
        g
    }
}

pub struct Resolver<'a> {
    pub ctx: &'a Context,
    pub model: &'a Model,
    pub global: &'a GlobalIndex,
}

impl<'a> Resolver<'a> {
    pub fn new(ctx: &'a Context, model: &'a Model, global: &'a GlobalIndex) -> Self {
        Resolver { ctx, model, global }
    }

    /// Resolve a package index to a field, mapping imports of intrinsic
    /// classes (never exported anywhere) to the compiler's stand-in nodes.
    pub fn resolve_field(&self, pkg: usize, idx: PackageIndex) -> Option<FieldRef> {
        if let Some(r) = self.ctx.resolve(pkg, idx) {
            return Some(FieldRef::External(r));
        }
        let i = idx.import_index()?;
        let p = &self.ctx.packages[pkg].pkg;
        let imp = p.imports.get(i)?;
        if p.fname_to_string(imp.class_name) != "Class" {
            return None;
        }
        let name = p.fname_to_string(imp.object_name);
        // A local class of that name first (an unloaded package importing it), then
        // the intrinsic / phantom stand-ins.
        self.model.find_class(&name).map(FieldRef::Local)
    }

    pub fn name_of(&self, r: FieldRef) -> String {
        match r {
            FieldRef::Local(n) => self.model.node(n).name.clone(),
            FieldRef::External(o) => self.ctx.name(o),
        }
    }

    pub fn kind_of(&self, r: FieldRef) -> FieldKind {
        match r {
            FieldRef::Local(n) => match self.model.node(n).kind {
                NodeKind::Class => FieldKind::Class,
                NodeKind::State => FieldKind::State,
                NodeKind::Function => FieldKind::Function,
                NodeKind::ScriptStruct => FieldKind::ScriptStruct,
                NodeKind::Enum => FieldKind::Enum,
                NodeKind::Const => FieldKind::Const,
                NodeKind::Property(k) => FieldKind::Property(k),
                NodeKind::Object => FieldKind::Other,
            },
            FieldRef::External(o) => match self.ctx.class_name(o).as_str() {
                "Class" => FieldKind::Class,
                "State" => FieldKind::State,
                "Function" => FieldKind::Function,
                "ScriptStruct" => FieldKind::ScriptStruct,
                "Enum" => FieldKind::Enum,
                "Const" => FieldKind::Const,
                other => match PropertyKind::from_class_name(other) {
                    Some(k) => FieldKind::Property(k),
                    None => FieldKind::Other,
                },
            },
        }
    }

    pub fn is_class(&self, r: FieldRef) -> bool {
        self.kind_of(r) == FieldKind::Class
    }
    pub fn is_function(&self, r: FieldRef) -> bool {
        self.kind_of(r) == FieldKind::Function
    }
    pub fn is_property(&self, r: FieldRef) -> bool {
        matches!(self.kind_of(r), FieldKind::Property(_))
    }
    pub fn is_struct_like(&self, r: FieldRef) -> bool {
        matches!(self.kind_of(r), FieldKind::Class | FieldKind::State | FieldKind::Function | FieldKind::ScriptStruct)
    }

    pub fn outer_of(&self, r: FieldRef) -> Option<FieldRef> {
        match r {
            FieldRef::Local(n) => self.model.node(n).outer.map(FieldRef::Local),
            FieldRef::External(o) => {
                let out = self.ctx.outer_of(o)?;
                // Package objects are not fields.
                if self.ctx.class_name(out) == "Package" {
                    None
                } else {
                    Some(FieldRef::External(out))
                }
            }
        }
    }

    pub fn owner_class(&self, r: FieldRef) -> Option<FieldRef> {
        let mut cur = Some(r);
        let mut guard = 0;
        while let Some(c) = cur {
            if self.is_class(c) {
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

    pub fn super_of(&self, r: FieldRef) -> Option<FieldRef> {
        match r {
            FieldRef::Local(n) => self.model.node(n).strct.super_struct,
            FieldRef::External(o) => self.ctx.super_of(o).map(FieldRef::External),
        }
    }

    /// `GetInheritanceSuper`: super struct for classes/structs/states/functions.
    pub fn inheritance_super(&self, r: FieldRef) -> Option<FieldRef> {
        if self.is_function(r) {
            return None;
        }
        self.super_of(r)
    }

    pub fn children(&self, r: FieldRef) -> Vec<FieldRef> {
        match r {
            FieldRef::Local(n) => self.model.node(n).strct.children.iter().map(|&c| FieldRef::Local(c)).collect(),
            FieldRef::External(o) => self.ctx.children(o).into_iter().map(FieldRef::External).collect(),
        }
    }

    /// Is `a` the same as or derived from `b` (classes / structs / states)?
    pub fn is_child_of(&self, a: FieldRef, b: FieldRef) -> bool {
        // Phantom classes (imports no loaded package exports) have an unknown
        // hierarchy: treat them as compatible with any plain class.
        if let FieldRef::Local(n) = a {
            let node = self.model.node(n);
            if node.intrinsic && node.intrinsic_package.is_some() && a != b {
                let b_is_component = self.is_component_class(b);
                let b_is_interface = self.class_flags(b) & cf::INTERFACE != 0;
                return !b_is_component && !b_is_interface;
            }
        }
        let mut cur = Some(a);
        let mut guard = 0;
        while let Some(c) = cur {
            if c == b {
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

    pub fn class_flags(&self, c: FieldRef) -> u32 {
        match c {
            FieldRef::Local(n) => self.model.node(n).class.class_flags,
            FieldRef::External(o) => match self.ctx.object(o).map(|x| &x.body) {
                Some(ObjectBody::Class { class, .. }) => class.class_flags,
                _ => 0,
            },
        }
    }

    pub fn class_within(&self, c: FieldRef) -> Option<FieldRef> {
        match c {
            FieldRef::Local(n) => self.model.node(n).class.class_within,
            FieldRef::External(o) => match self.ctx.object(o).map(|x| &x.body) {
                Some(ObjectBody::Class { class, .. }) => self.resolve_field(o.pkg, class.class_within),
                _ => None,
            },
        }
    }

    pub fn class_config_name(&self, c: FieldRef) -> String {
        match c {
            FieldRef::Local(n) => self.model.node(n).class.config_name.clone(),
            FieldRef::External(o) => self.ctx.class_config_name(o),
        }
    }

    pub fn state_flags(&self, s: FieldRef) -> u32 {
        match s {
            FieldRef::Local(n) => self.model.node(n).state.state_flags,
            FieldRef::External(o) => self.ctx.state_flags(o),
        }
    }

    pub fn class_interfaces(&self, c: FieldRef) -> Vec<FieldRef> {
        match c {
            FieldRef::Local(n) => self.model.node(n).class.interfaces.iter().map(|(i, _)| *i).collect(),
            FieldRef::External(o) => match self.ctx.object(o).map(|x| &x.body) {
                Some(ObjectBody::Class { class, .. }) => {
                    class.interfaces.iter().filter_map(|(i, _)| self.resolve_field(o.pkg, *i)).collect()
                }
                _ => Vec::new(),
            },
        }
    }

    /// `UClass::ImplementsInterface`: never true for the `Interface` base class itself.
    pub fn implements_interface(&self, class: FieldRef, iface: FieldRef) -> bool {
        if self.class_flags(iface) & cf::INTERFACE == 0 || self.name_of(iface).eq_ignore_ascii_case("Interface") {
            return false;
        }
        let mut cur = Some(class);
        let mut guard = 0;
        while let Some(c) = cur {
            for i in self.class_interfaces(c) {
                if self.is_child_of(i, iface) {
                    return true;
                }
            }
            cur = self.super_of(c);
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        false
    }

    pub fn struct_flags(&self, s: FieldRef) -> u32 {
        match s {
            FieldRef::Local(n) => self.model.node(n).strct.struct_flags,
            FieldRef::External(o) => match self.ctx.object(o).map(|x| &x.body) {
                Some(ObjectBody::ScriptStruct { data, .. }) => data.struct_flags,
                _ => 0,
            },
        }
    }

    pub fn enum_names(&self, e: FieldRef) -> Vec<String> {
        match e {
            FieldRef::Local(n) => self.model.node(n).enum_names.clone(),
            FieldRef::External(o) => self.ctx.enum_names(o),
        }
    }

    pub fn find_enum_index(&self, e: FieldRef, tag: &str) -> Option<usize> {
        self.enum_names(e).iter().position(|n| n.eq_ignore_ascii_case(tag))
    }

    pub fn const_value(&self, c: FieldRef) -> String {
        match c {
            FieldRef::Local(n) => self.model.node(n).const_value.clone(),
            FieldRef::External(o) => match self.ctx.object(o).map(|x| &x.body) {
                Some(ObjectBody::Const(d)) => d.value.clone(),
                _ => String::new(),
            },
        }
    }

    // ----- properties ------------------------------------------------------

    pub fn prop_flags(&self, p: FieldRef) -> u64 {
        match p {
            FieldRef::Local(n) => self.model.node(n).prop.flags,
            FieldRef::External(o) => self.ctx.property(o).map(|(_, d)| d.property_flags).unwrap_or(0),
        }
    }

    pub fn prop_array_dim(&self, p: FieldRef) -> i32 {
        match p {
            FieldRef::Local(n) => self.model.node(n).prop.array_dim,
            FieldRef::External(o) => self.ctx.property(o).map(|(_, d)| d.array_dim).unwrap_or(1),
        }
    }

    pub fn prop_refs(&self, p: FieldRef) -> (Option<FieldRef>, Option<FieldRef>) {
        match p {
            FieldRef::Local(n) => (self.model.node(n).prop.ref1, self.model.node(n).prop.ref2),
            FieldRef::External(o) => match self.ctx.property(o) {
                Some((_, d)) => (
                    d.ref1.and_then(|r| self.resolve_field(o.pkg, r)),
                    d.ref2.and_then(|r| self.resolve_field(o.pkg, r)),
                ),
                None => (None, None),
            },
        }
    }

    pub fn prop_kind(&self, p: FieldRef) -> Option<PropertyKind> {
        match self.kind_of(p) {
            FieldKind::Property(k) => Some(k),
            _ => None,
        }
    }

    /// `FPropertyBase(UProperty*)`.
    pub fn prop_type(&self, p: FieldRef, ref_type: RefType) -> PropType {
        let mut flags = self.prop_flags(p);
        let mut kind = self.prop_kind(p).unwrap_or(PropertyKind::Int);
        let mut array_dim = self.prop_array_dim(p);
        let (mut r1, mut r2) = self.prop_refs(p);
        let array_size_enum = match p {
            FieldRef::Local(n) => self.model.node(n).prop.array_size_enum,
            FieldRef::External(o) => self
                .ctx
                .property(o)
                .and_then(|(_, d)| d.array_size_enum)
                .and_then(|r| self.ctx.resolve(o.pkg, r))
                .map(FieldRef::External),
        };
        if kind == PropertyKind::Array {
            let propagate = flags & (cpf::PARM_FLAGS | cpf::CONST);
            if let Some(inner) = r1 {
                kind = self.prop_kind(inner).unwrap_or(PropertyKind::Int);
                let (i1, i2) = self.prop_refs(inner);
                r1 = i1;
                r2 = i2;
                flags = self.prop_flags(inner) | propagate;
            }
            array_dim = 0;
        }
        let mut t = match kind {
            PropertyKind::Byte => PropType::enum_byte(r1),
            PropertyKind::Int => PropType::new(Cpt::Int),
            PropertyKind::Bool => PropType::new(Cpt::Bool),
            PropertyKind::Float => PropType::new(Cpt::Float),
            PropertyKind::Object | PropertyKind::Component | PropertyKind::SafeObject => {
                let mut t = PropType::new(Cpt::Object);
                t.inner = r1;
                t.interface = false;
                t
            }
            PropertyKind::Class => {
                let mut t = PropType::new(Cpt::Object);
                t.inner = r1;
                t.meta_class = r2;
                t
            }
            PropertyKind::Interface => {
                let mut t = PropType::new(Cpt::Interface);
                t.inner = r1;
                t
            }
            PropertyKind::Name => PropType::new(Cpt::Name),
            PropertyKind::Str => PropType::new(Cpt::String),
            PropertyKind::Map => PropType::new(Cpt::Map),
            PropertyKind::Struct => {
                let mut t = PropType::new(Cpt::Struct);
                t.inner = r1;
                t
            }
            PropertyKind::Delegate => {
                let mut t = PropType::new(Cpt::Delegate);
                t.inner = r1;
                t
            }
            PropertyKind::Array => PropType::new(Cpt::None),
        };
        t.array_dim = array_dim;
        t.flags = flags;
        t.ref_type = ref_type;
        t.array_index_enum = array_size_enum;
        t.safe = kind == PropertyKind::SafeObject;
        t
    }

    // ----- functions ---------------------------------------------------------

    pub fn func_flags(&self, f: FieldRef) -> u32 {
        match f {
            FieldRef::Local(n) => self.model.node(n).func.flags,
            FieldRef::External(o) => match self.ctx.object(o).map(|x| &x.body) {
                Some(ObjectBody::Function { function, .. }) => function.function_flags,
                _ => 0,
            },
        }
    }

    pub fn func_precedence(&self, f: FieldRef) -> u8 {
        match f {
            FieldRef::Local(n) => self.model.node(n).func.precedence,
            FieldRef::External(o) => match self.ctx.object(o).map(|x| &x.body) {
                Some(ObjectBody::Function { function, .. }) => function.oper_precedence,
                _ => 0,
            },
        }
    }

    pub fn func_native_index(&self, f: FieldRef) -> u16 {
        match f {
            FieldRef::Local(n) => self.model.node(n).func.native_index,
            FieldRef::External(o) => match self.ctx.object(o).map(|x| &x.body) {
                Some(ObjectBody::Function { function, .. }) => function.native_index,
                _ => 0,
            },
        }
    }

    pub fn func_friendly_name(&self, f: FieldRef) -> String {
        match f {
            FieldRef::Local(n) => self.model.node(n).func.friendly_name.clone(),
            FieldRef::External(o) => match self.ctx.object(o).map(|x| &x.body) {
                Some(ObjectBody::Function { function, .. }) => self.ctx.packages[o.pkg].name_of(function.friendly_name),
                _ => self.ctx.name(o),
            },
        }
    }

    /// Parameters including the return value, in order (`TFieldIterator<UProperty>` over `CPF_Parm`).
    pub fn func_parms(&self, f: FieldRef) -> Vec<FieldRef> {
        self.children(f)
            .into_iter()
            .filter(|&c| self.is_property(c) && self.prop_flags(c) & cpf::PARM != 0)
            .collect()
    }

    pub fn func_params(&self, f: FieldRef) -> Vec<FieldRef> {
        self.func_parms(f).into_iter().filter(|&c| self.prop_flags(c) & cpf::RETURN_PARM == 0).collect()
    }

    pub fn func_return(&self, f: FieldRef) -> Option<FieldRef> {
        self.func_parms(f).into_iter().find(|&c| self.prop_flags(c) & cpf::RETURN_PARM != 0)
    }

    pub fn func_num_parms(&self, f: FieldRef) -> i32 {
        match f {
            FieldRef::Local(n) => self.model.node(n).func.num_parms,
            FieldRef::External(_) => self.func_parms(f).len() as i32,
        }
    }

    // ----- lookups -----------------------------------------------------------

    /// `FindClass`: any package (local model first, then loaded packages).
    pub fn find_class(&self, name: &str) -> Option<FieldRef> {
        if let Some(n) = self.model.find_class(name) {
            return Some(FieldRef::Local(n));
        }
        self.global.classes.get(&name.to_ascii_lowercase()).map(|&o| FieldRef::External(o))
    }

    /// `FindObject<UEnum>(ANY_PACKAGE, name)`: the most recently created object
    /// wins in the engine's hash, so local nodes are searched newest first.
    pub fn find_enum_any(&self, name: &str) -> Option<FieldRef> {
        for (i, n) in self.model.nodes.iter().enumerate().rev() {
            if n.kind == NodeKind::Enum && n.name.eq_ignore_ascii_case(name) {
                return Some(FieldRef::Local(i));
            }
        }
        self.global.enums.get(&name.to_ascii_lowercase()).map(|&o| FieldRef::External(o))
    }

    pub fn find_struct_any(&self, name: &str) -> Option<FieldRef> {
        for (i, n) in self.model.nodes.iter().enumerate().rev() {
            if n.kind == NodeKind::ScriptStruct && n.name.eq_ignore_ascii_case(name) {
                return Some(FieldRef::Local(i));
            }
        }
        self.global.structs.get(&name.to_ascii_lowercase()).map(|&o| FieldRef::External(o))
    }

    /// Enum lookup preferring the current scope chain (what the engine finds
    /// first for a class's own types), then any package.
    pub fn find_enum_scoped(&self, scope: FieldRef, name: &str) -> Option<FieldRef> {
        if let Some(f) = self.find_field(Some(scope), name, true, None) {
            if self.kind_of(f) == FieldKind::Enum {
                return Some(f);
            }
        }
        self.find_enum_any(name)
    }

    pub fn find_struct_scoped(&self, scope: FieldRef, name: &str) -> Option<FieldRef> {
        if let Some(f) = self.find_field(Some(scope), name, true, None) {
            if self.kind_of(f) == FieldKind::ScriptStruct {
                return Some(f);
            }
        }
        self.find_struct_any(name)
    }

    pub fn global_enum_tag(&self, tag: &str) -> Option<i32> {
        let key = tag.to_ascii_lowercase();
        for n in &self.model.nodes {
            if n.kind == NodeKind::Enum {
                if let Some(i) = n.enum_names.iter().position(|t| t.eq_ignore_ascii_case(tag)) {
                    return Some(i as i32);
                }
            }
        }
        self.global.enum_tags.get(&key).copied()
    }

    /// `FScriptCompiler::FindField`.
    pub fn find_field(
        &self,
        scope: Option<FieldRef>,
        name: &str,
        include_parents: bool,
        outer_context_count: Option<&mut i32>,
    ) -> Option<FieldRef> {
        if let Some(f) = self.find_field_inner(scope, name, include_parents, outer_context_count) {
            return Some(f);
        }
        // A phantom class has an unknown parent chain: look the member up in any
        // loaded class (its real ancestor among them), preferring its own package.
        if let Some(FieldRef::Local(n)) = scope {
            let node = self.model.node(n);
            if node.intrinsic && node.intrinsic_package.is_some() {
                let pkg = node.intrinsic_package.clone().unwrap_or_default();
                // A reference of unknown (phantom) type most likely points at another
                // native-only class: their imported members win over local classes.
                if node.name == "__Unknown" {
                    let mut phantoms: Vec<NodeId> = self.model.intrinsics.iter().copied().filter(|&c| self.model.node(c).intrinsic_package.is_some() && self.model.node(c).name != "__Unknown").collect();
                    phantoms.sort_by_key(|&c| self.model.node(c).name.to_ascii_lowercase());
                    for c in phantoms {
                        for &ch in &self.model.node(c).strct.children {
                            if self.model.node(ch).name.eq_ignore_ascii_case(name) {
                                return Some(FieldRef::Local(ch));
                            }
                        }
                    }
                }
                // Local classes first (the package being compiled may be the phantom's own).
                let mut locals: Vec<NodeId> = self.model.classes.clone();
                locals.sort_by_key(|&c| self.model.node(c).name.to_ascii_lowercase());
                for c in locals {
                    for &ch in &self.model.node(c).strct.children {
                        if self.model.node(ch).name.eq_ignore_ascii_case(name) {
                            return Some(FieldRef::Local(ch));
                        }
                    }
                }
                let mut candidates: Vec<ObjRef> = self.global.classes.values().copied().collect();
                candidates.sort_by_key(|c| (!self.ctx.packages[c.pkg].name.eq_ignore_ascii_case(&pkg), self.ctx.name(*c).to_ascii_lowercase()));
                for c in candidates {
                    for ch in self.ctx.children(c) {
                        if self.ctx.name(ch).eq_ignore_ascii_case(name) {
                            return Some(FieldRef::External(ch));
                        }
                    }
                }
            }
        }
        None
    }

    fn find_field_inner(
        &self,
        mut scope: Option<FieldRef>,
        name: &str,
        include_parents: bool,
        outer_context_count: Option<&mut i32>,
    ) -> Option<FieldRef> {
        let mut occ = outer_context_count;
        if let Some(c) = occ.as_deref_mut() {
            *c = 0;
        }
        loop {
            let original_scope = scope;
            let mut s = scope;
            while let Some(sc) = s {
                // TFieldIterator<UField>(Scope): walks the struct and its supers.
                let mut cur = Some(sc);
                let mut guard = 0;
                while let Some(st) = cur {
                    if include_parents || Some(st) == original_scope {
                        for ch in self.children(st) {
                            if self.name_of(ch).eq_ignore_ascii_case(name) {
                                return Some(ch);
                            }
                        }
                    }
                    cur = self.inheritance_super(st);
                    guard += 1;
                    if guard > 100 {
                        break;
                    }
                }
                s = self.outer_of(sc).filter(|o| self.is_struct_like(*o));
            }
            match occ.as_deref_mut() {
                Some(c) => {
                    let orig = original_scope?;
                    let owner = self.owner_class(orig)?;
                    let within = self.class_within(owner);
                    match within {
                        Some(w) if !self.name_of(w).eq_ignore_ascii_case("Object") => {
                            *c += 1;
                            scope = Some(w);
                        }
                        _ => return None,
                    }
                }
                None => return None,
            }
        }
    }

    /// Class flag helpers.
    pub fn class_is_interface(&self, c: FieldRef) -> bool {
        self.class_flags(c) & cf::INTERFACE != 0
    }
    pub fn class_is_native(&self, c: FieldRef) -> bool {
        self.class_flags(c) & cf::NATIVE != 0
    }
    pub fn func_is(&self, f: FieldRef, flag: u32) -> bool {
        self.func_flags(f) & flag != 0
    }
    pub fn is_final_func(&self, f: FieldRef) -> bool {
        self.func_is(f, func::FINAL)
    }
    pub fn struct_has(&self, s: FieldRef, flag: u32) -> bool {
        self.struct_flags(s) & flag != 0
    }
    pub fn is_component_class(&self, c: FieldRef) -> bool {
        let mut cur = Some(c);
        let mut guard = 0;
        while let Some(k) = cur {
            if self.name_of(k).eq_ignore_ascii_case("Component") {
                return true;
            }
            cur = self.super_of(k);
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        false
    }
    pub fn is_actor_class(&self, c: FieldRef) -> bool {
        let mut cur = Some(c);
        let mut guard = 0;
        while let Some(k) = cur {
            if self.name_of(k).eq_ignore_ascii_case("Actor") {
                return true;
            }
            cur = self.super_of(k);
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        false
    }
    #[allow(dead_code)]
    fn _unused(&self) -> u32 {
        strct::NATIVE
    }
}
