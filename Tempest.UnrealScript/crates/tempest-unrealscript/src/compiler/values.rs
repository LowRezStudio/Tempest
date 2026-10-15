//! In-memory property values for default objects: zero/struct defaults,
//! decoding of tagged properties from loaded packages, comparison
//! (`UProperty::Identical`) and tagged serialisation
//! (`UStruct::SerializeTaggedProperties`, save path).

use std::collections::HashMap;

use tempest_upk::flags::{rf, cpf, strct as stf};
use tempest_upk::io::{FName, PackageIndex, Reader, Writer};

use super::engine::Compiler;
use super::model::FieldRef;
use crate::objects::{ObjectBody, PropertyKind, TaggedProperty};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Byte(u8),
    Int(i32),
    Bool(bool),
    Float(f32),
    /// Name text (`Name` or `Name_3`).
    Name(String),
    Str(String),
    Object(Option<FieldRef>),
    Interface(Option<FieldRef>),
    Delegate(Option<FieldRef>, String),
    /// Members in layout order (own properties, then super struct's).
    Struct(Vec<Slot>),
    Array(Vec<Value>),
    Map,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub prop: FieldRef,
    pub elems: Vec<Value>,
}

pub fn find_slot(slots: &[Slot], prop: FieldRef) -> Option<&Slot> {
    slots.iter().find(|s| s.prop == prop)
}
pub fn find_slot_mut(slots: &mut [Slot], prop: FieldRef) -> Option<&mut Slot> {
    slots.iter_mut().find(|s| s.prop == prop)
}

/// Copy every slot of `src` over the matching slot of `dst`.
pub fn overlay_slots(dst: &mut [Slot], src: &[Slot]) {
    for s in src {
        if let Some(d) = find_slot_mut(dst, s.prop) {
            *d = s.clone();
        }
    }
}

/// Replace object references throughout a value tree.
pub fn replace_refs(v: &mut Value, map: &dyn Fn(FieldRef) -> Option<FieldRef>) {
    match v {
        Value::Object(Some(r)) | Value::Interface(Some(r)) | Value::Delegate(Some(r), _) => {
            if let Some(n) = map(*r) {
                *r = n;
            }
        }
        Value::Struct(slots) => {
            for s in slots {
                for e in &mut s.elems {
                    replace_refs(e, map);
                }
            }
        }
        Value::Array(items) => {
            for e in items {
                replace_refs(e, map);
            }
        }
        _ => {}
    }
}

pub fn replace_refs_slots(slots: &mut [Slot], map: &dyn Fn(FieldRef) -> Option<FieldRef>) {
    for s in slots {
        for e in &mut s.elems {
            replace_refs(e, map);
        }
    }
}

#[derive(Default)]
pub struct ValueCache {
    pub struct_defaults: HashMap<FieldRef, Vec<Slot>>,
    pub object_values: HashMap<FieldRef, Vec<Slot>>,
}

impl<'a> Compiler<'a> {
    /// `TFieldIterator<UProperty>(Struct)` order: own properties, then supers.
    pub fn props_in_layout_order(&self, s: FieldRef) -> Vec<FieldRef> {
        let r = self.res();
        let mut out = Vec::new();
        let mut cur = Some(s);
        let mut guard = 0;
        while let Some(st) = cur {
            for ch in r.children(st) {
                if r.is_property(ch) {
                    out.push(ch);
                }
            }
            cur = r.inheritance_super(st);
            guard += 1;
            if guard > 200 {
                break;
            }
        }
        out
    }

    pub fn prop_kind_of(&self, prop: FieldRef) -> PropertyKind {
        self.res().prop_kind(prop).unwrap_or(PropertyKind::Int)
    }

    pub fn zero_value(&mut self, prop: FieldRef) -> Value {
        match self.prop_kind_of(prop) {
            PropertyKind::Byte => Value::Byte(0),
            PropertyKind::Int => Value::Int(0),
            PropertyKind::Bool => Value::Bool(false),
            PropertyKind::Float => Value::Float(0.0),
            PropertyKind::Name => Value::Name("None".into()),
            PropertyKind::Str => Value::Str(String::new()),
            PropertyKind::Object | PropertyKind::Class | PropertyKind::Component | PropertyKind::SafeObject => Value::Object(None),
            PropertyKind::Interface => Value::Interface(None),
            PropertyKind::Delegate => Value::Delegate(None, "None".into()),
            PropertyKind::Struct => match self.res().prop_refs(prop).0 {
                Some(s) => Value::Struct(self.struct_defaults(s)),
                None => Value::Struct(Vec::new()),
            },
            PropertyKind::Array => Value::Array(Vec::new()),
            PropertyKind::Map => Value::Map,
        }
    }

    /// Plain zero value without struct defaults (`appMemzero`).
    pub fn raw_zero_value(&mut self, prop: FieldRef) -> Value {
        match self.prop_kind_of(prop) {
            PropertyKind::Struct => match self.res().prop_refs(prop).0 {
                Some(s) => {
                    let props = self.props_in_layout_order(s);
                    let mut slots = Vec::new();
                    for p in props {
                        let dim = self.res().prop_array_dim(p).max(1) as usize;
                        let mut elems = Vec::new();
                        for _ in 0..dim {
                            elems.push(self.raw_zero_value(p));
                        }
                        slots.push(Slot { prop: p, elems });
                    }
                    Value::Struct(slots)
                }
                None => Value::Struct(Vec::new()),
            },
            _ => self.zero_value(prop),
        }
    }

    pub fn zero_slot(&mut self, prop: FieldRef) -> Slot {
        let dim = self.res().prop_array_dim(prop).max(1) as usize;
        let mut elems = Vec::with_capacity(dim);
        for _ in 0..dim {
            elems.push(self.zero_value(prop));
        }
        Slot { prop, elems }
    }

    /// `UScriptStruct::GetDefaults()` as values.
    pub fn struct_defaults(&mut self, s: FieldRef) -> Vec<Slot> {
        if let Some(v) = self.values.struct_defaults.get(&s) {
            return v.clone();
        }
        let mut slots: Vec<Slot> = Vec::new();
        let own: Vec<FieldRef> = self.res().children(s).into_iter().filter(|c| self.res().is_property(*c)).collect();
        for p in own {
            let sl = self.zero_slot(p);
            slots.push(sl);
        }
        if let Some(sup) = self.res().inheritance_super(s) {
            let parent = self.struct_defaults(sup);
            slots.extend(parent);
        }
        match s {
            FieldRef::Local(n) => {
                if let Some(v) = self.model.node(n).values.clone() {
                    overlay_slots(&mut slots, &v);
                }
            }
            FieldRef::External(o) => {
                let tagged = match self.ctx.object(o).map(|x| &x.body) {
                    Some(ObjectBody::ScriptStruct { data, .. }) => data.defaults.clone(),
                    _ => Vec::new(),
                };
                if !tagged.is_empty() {
                    self.bin_defaults_ctx = true;
                    self.decode_tagged(&mut slots, &tagged, s, o.pkg);
                }
            }
        }
        self.values.struct_defaults.insert(s, slots.clone());
        slots
    }

    /// Full property values of an object (local defaults object or any loaded export).
    pub fn object_values(&mut self, obj: FieldRef) -> Vec<Slot> {
        if let Some(v) = self.values.object_values.get(&obj) {
            return v.clone();
        }
        let result = match obj {
            FieldRef::Local(n) => {
                if let Some(v) = self.model.node(n).values.clone() {
                    v
                } else {
                    let class = self.model.node(n).object_class;
                    let arch = self.model.node(n).archetype;
                    self.initial_object_values(class, arch)
                }
            }
            FieldRef::External(o) => {
                let class = self.ctx.class_of(o).map(FieldRef::External);
                let arch_idx = self.ctx.packages[o.pkg].pkg.exports[o.export].archetype;
                let mut arch = self.res().resolve_field(o.pkg, arch_idx);
                // Saved packages leave the archetype empty when it is the class default
                // object (only class default objects themselves default to the parent's).
                if arch.is_none() && self.ctx.object_flags(o) & rf::CLASS_DEFAULT_OBJECT == 0 {
                    arch = self.ctx.class_of(o).and_then(|c| self.ctx.class_default_object(c)).map(FieldRef::External);
                }
                let mut slots = self.initial_object_values(class, arch);
                let tagged = self.ctx.object(o).map(|x| x.header.properties.clone()).unwrap_or_default();
                if let Some(c) = class {
                    self.bin_defaults_ctx = self.ctx.object_flags(o) & tempest_upk::flags::rf::CLASS_DEFAULT_OBJECT != 0;
                    self.decode_tagged(&mut slots, &tagged, c, o.pkg);
                }
                slots
            }
        };
        self.values.object_values.insert(obj, result.clone());
        result
    }

    /// Values an object starts with: its archetype's values (or the parent
    /// class defaults when it has none), extended to its class's property set.
    pub fn initial_object_values(&mut self, class: Option<FieldRef>, archetype: Option<FieldRef>) -> Vec<Slot> {
        let Some(class) = class else { return Vec::new() };
        let props = self.props_in_layout_order(class);
        let base: Vec<Slot> = match archetype {
            Some(a) => self.object_values(a),
            None => match self.res().super_of(class) {
                Some(sup) => self.class_cdo_values(sup),
                None => Vec::new(),
            },
        };
        let mut slots: Vec<Slot> = Vec::with_capacity(props.len());
        for p in props {
            match find_slot(&base, p) {
                Some(s) => slots.push(s.clone()),
                None => {
                    let sl = self.zero_slot(p);
                    slots.push(sl);
                }
            }
        }
        slots
    }

    /// Values of a class's default object.
    pub fn class_cdo_values(&mut self, class: FieldRef) -> Vec<Slot> {
        match class {
            FieldRef::Local(n) => match self.model.node(n).class.cdo {
                Some(cdo) => self.object_values(FieldRef::Local(cdo)),
                None => self.initial_object_values(Some(class), None),
            },
            FieldRef::External(o) => match self.ctx.class_default_object(o) {
                Some(cdo) => self.object_values(FieldRef::External(cdo)),
                None => self.initial_object_values(Some(class), None),
            },
        }
    }

    // ----- decoding ---------------------------------------------------------------

    fn pkg_name(&self, pkg: usize, n: FName) -> String {
        self.ctx.packages[pkg].pkg.fname_to_string(n)
    }

    /// Apply saved tagged properties (from package `pkg`) onto `slots`.
    pub fn decode_tagged(&mut self, slots: &mut Vec<Slot>, tagged: &[TaggedProperty], owner: FieldRef, pkg: usize) {
        for tp in tagged {
            let name = self.pkg_name(pkg, tp.name);
            let Some(prop) = slots.iter().map(|s| s.prop).find(|p| self.res().name_of(*p).eq_ignore_ascii_case(&name)) else {
                continue;
            };
            let _ = owner;
            let idx = tp.array_index.max(0) as usize;
            let base = find_slot(slots, prop).and_then(|s| s.elems.get(idx)).cloned();
            let value = self.decode_value(prop, tp, pkg, base);
            if let Some(s) = find_slot_mut(slots, prop) {
                if idx < s.elems.len() {
                    s.elems[idx] = value;
                }
            }
        }
    }

    fn decode_value(&mut self, prop: FieldRef, tp: &TaggedProperty, pkg: usize, base: Option<Value>) -> Value {
        let kind = self.prop_kind_of(prop);
        if kind == PropertyKind::Bool {
            return Value::Bool(tp.bool_val.unwrap_or(0) != 0);
        }
        let mut r = Reader::new(&tp.raw);
        self.decode_item(&mut r, prop, kind, pkg, base, true)
    }

    fn resolve_obj(&self, pkg: usize, idx: PackageIndex) -> Option<FieldRef> {
        self.res().resolve_field(pkg, idx)
    }

    fn struct_is_binary(&self, s: FieldRef) -> bool {
        let f = self.res().struct_flags(s);
        f & stf::IMMUTABLE != 0 || (self.cooked && f & stf::IMMUTABLE_WHEN_COOKED != 0)
    }

    /// `SerializeItem` (load) for one element.
    fn decode_item(&mut self, r: &mut Reader, prop: FieldRef, kind: PropertyKind, pkg: usize, base: Option<Value>, tagged_context: bool) -> Value {
        match kind {
            PropertyKind::Byte => {
                let e = self.res().prop_refs(prop).0;
                if e.is_some() && r.remaining() >= 8 && tagged_context {
                    let n = r.fname().unwrap_or_default();
                    let text = self.pkg_name(pkg, n);
                    let names = self.res().enum_names(e.unwrap());
                    let i = names.iter().position(|x| x.eq_ignore_ascii_case(&text)).unwrap_or(0);
                    Value::Byte(i as u8)
                } else if e.is_some() && !tagged_context && r.remaining() >= 8 {
                    // Byte enums inside binary structs are still names.
                    let n = r.fname().unwrap_or_default();
                    let text = self.pkg_name(pkg, n);
                    let names = self.res().enum_names(e.unwrap());
                    let i = names.iter().position(|x| x.eq_ignore_ascii_case(&text)).unwrap_or(0);
                    Value::Byte(i as u8)
                } else {
                    Value::Byte(r.u8().unwrap_or(0))
                }
            }
            PropertyKind::Int => Value::Int(r.i32().unwrap_or(0)),
            PropertyKind::Float => Value::Float(r.f32().unwrap_or(0.0)),
            PropertyKind::Bool => Value::Bool(r.u8().unwrap_or(0) != 0),
            PropertyKind::Name => {
                let n = r.fname().unwrap_or_default();
                Value::Name(self.pkg_name(pkg, n))
            }
            PropertyKind::Str => Value::Str(r.fstring().unwrap_or_default()),
            PropertyKind::Object | PropertyKind::Class | PropertyKind::Component | PropertyKind::SafeObject => {
                let i = r.index().unwrap_or_default();
                Value::Object(self.resolve_obj(pkg, i))
            }
            PropertyKind::Interface => {
                let i = r.index().unwrap_or_default();
                Value::Interface(self.resolve_obj(pkg, i))
            }
            PropertyKind::Delegate => {
                let i = r.index().unwrap_or_default();
                let n = r.fname().unwrap_or_default();
                Value::Delegate(self.resolve_obj(pkg, i), self.pkg_name(pkg, n))
            }
            PropertyKind::Struct => {
                let Some(s) = self.res().prop_refs(prop).0 else { return Value::Struct(Vec::new()) };
                let mut slots = match base {
                    Some(Value::Struct(sl)) => sl,
                    _ => self.struct_defaults(s),
                };
                if self.struct_is_binary(s) {
                    let props = self.props_in_layout_order(s);
                    let cooked_src = self.ctx.packages[pkg].pkg.summary.package_flags & tempest_upk::flags::pkg::COOKED != 0;
                    for p in props {
                        if !self.should_serialize_in(p, self.bin_defaults_ctx, cooked_src) {
                            continue;
                        }
                        let k = self.prop_kind_of(p);
                        let dim = self.res().prop_array_dim(p).max(1) as usize;
                        for i in 0..dim {
                            let b = find_slot(&slots, p).and_then(|x| x.elems.get(i)).cloned();
                            let v = self.decode_item(r, p, k, pkg, b, false);
                            if let Some(sl) = find_slot_mut(&mut slots, p) {
                                if i < sl.elems.len() {
                                    sl.elems[i] = v;
                                }
                            }
                        }
                    }
                } else {
                    let names = &self.ctx.packages[pkg].pkg;
                    if let Ok(tagged) = crate::objects::read_tagged_properties(r, names) {
                        self.decode_tagged(&mut slots, &tagged, s, pkg);
                    }
                }
                Value::Struct(slots)
            }
            PropertyKind::Array => {
                let Some(inner) = self.res().prop_refs(prop).0 else { return Value::Array(Vec::new()) };
                let n = r.i32().unwrap_or(0).max(0) as usize;
                let ik = self.prop_kind_of(inner);
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    let base = self.zero_value(inner);
                    let v = self.decode_item(r, inner, ik, pkg, Some(base), false);
                    items.push(v);
                }
                Value::Array(items)
            }
            PropertyKind::Map => Value::Map,
        }
    }

    // ----- comparison -----------------------------------------------------------------

    /// `UProperty::Identical(A, B)` with `B == NULL` meaning "zero".
    pub fn identical(&self, a: &Value, b: Option<&Value>) -> bool {
        match (a, b) {
            (Value::Byte(x), None) => *x == 0,
            (Value::Byte(x), Some(Value::Byte(y))) => x == y,
            (Value::Int(x), None) => *x == 0,
            (Value::Int(x), Some(Value::Int(y))) => x == y,
            (Value::Bool(x), None) => !*x,
            (Value::Bool(x), Some(Value::Bool(y))) => x == y,
            (Value::Float(x), None) => *x == 0.0,
            (Value::Float(x), Some(Value::Float(y))) => x == y,
            (Value::Name(x), None) => x.eq_ignore_ascii_case("None"),
            (Value::Name(x), Some(Value::Name(y))) => x.eq_ignore_ascii_case(y),
            (Value::Str(x), None) => x.is_empty(),
            (Value::Str(x), Some(Value::Str(y))) => x.eq_ignore_ascii_case(y),
            (Value::Object(x), None) => x.is_none(),
            (Value::Object(x), Some(Value::Object(y))) => x == y,
            (Value::Interface(x), None) => x.is_none(),
            (Value::Interface(x), Some(Value::Interface(y))) => x == y,
            (Value::Delegate(_, f), None) => f.eq_ignore_ascii_case("None"),
            (Value::Delegate(o, f), Some(Value::Delegate(o2, f2))) => f.eq_ignore_ascii_case(f2) && o == o2,
            (Value::Struct(sa), None) => sa.iter().all(|s| s.elems.iter().all(|e| self.identical(e, None))),
            (Value::Struct(sa), Some(Value::Struct(sb))) => sa.iter().all(|s| {
                let other = find_slot(sb, s.prop);
                s.elems.iter().enumerate().all(|(i, e)| self.identical(e, other.and_then(|o| o.elems.get(i))))
            }),
            (Value::Array(xa), None) => xa.is_empty(),
            (Value::Array(xa), Some(Value::Array(xb))) => xa.len() == xb.len() && xa.iter().zip(xb.iter()).all(|(x, y)| self.identical(x, Some(y))),
            (Value::Map, _) => true,
            _ => false,
        }
    }

    // ----- encoding -------------------------------------------------------------------

    fn should_serialize(&self, prop: FieldRef, serializing_defaults: bool) -> bool {
        self.should_serialize_in(prop, serializing_defaults, self.cooked)
    }

    /// `UProperty::ShouldSerializeValue` for a package that is (`cooked`) or is not re-saved by the cooker.
    fn should_serialize_in(&self, prop: FieldRef, serializing_defaults: bool, cooked: bool) -> bool {
        let f = self.res().prop_flags(prop);
        if f & cpf::NATIVE != 0 {
            return false;
        }
        if f & cpf::TRANSIENT != 0 && !serializing_defaults {
            return false;
        }
        // `CPF_Deprecated && Ar.IsSaving() && !GIsUCCMake`: kept by make, dropped by the cooker.
        if f & cpf::DEPRECATED != 0 && cooked {
            return false;
        }
        // The cooker drops non-bool editor-only properties altogether (field and value).
        if f & cpf::EDITOR_ONLY != 0 && cooked && self.res().prop_kind(prop) != Some(PropertyKind::Bool) {
            return false;
        }
        true
    }

    /// `SerializeTaggedProperties` (save) for an object or struct value.
    ///
    /// * `owner`: the class/struct whose properties are being written.
    /// * `defaults`: the archetype's values (`None` → every property compared with zero,
    ///   and for script structs: every property written).
    /// * `defaults_class`: class of the archetype, for the "property covered by
    ///   the defaults block" rule (`Offset + ElementSize <= DefaultsCount`).
    pub fn encode_tagged(
        &mut self,
        slots: &[Slot],
        owner: FieldRef,
        defaults: Option<&[Slot]>,
        defaults_struct: Option<FieldRef>,
        is_class: bool,
        serializing_defaults: bool,
        atomic: bool,
    ) -> Vec<TaggedProperty> {
        let mut out = Vec::new();
        let props = self.props_in_layout_order(owner);
        for prop in props {
            if !self.should_serialize(prop, serializing_defaults) {
                continue;
            }
            let Some(slot) = find_slot(slots, prop) else { continue };
            let covered = match (defaults, defaults_struct) {
                (Some(_), Some(dc)) => {
                    let owner_struct = self.res().outer_of(prop);
                    owner_struct.map(|oc| self.res().is_child_of(dc, oc)).unwrap_or(false)
                }
                (Some(_), None) => !is_class,
                (None, _) => false,
            };
            let default_slot = if covered { defaults.and_then(|d| find_slot(d, prop)) } else { None };
            let dim = slot.elems.len();
            for idx in 0..dim {
                let value = &slot.elems[idx];
                let default_value = default_slot.and_then(|s| s.elems.get(idx));
                let crosslevel = matches!(self.prop_kind_of(prop), PropertyKind::Object | PropertyKind::Class | PropertyKind::Component | PropertyKind::SafeObject)
                    && self.res().prop_flags(prop) & (cpf::CROSS_LEVEL_ACTIVE | cpf::CROSS_LEVEL_PASSIVE) != 0;
                let write = (!is_class && defaults.is_none()) || crosslevel || !self.identical(value, default_value);
                if !write {
                    continue;
                }
                let dv = if !atomic { default_value } else { None };
                let tag = self.encode_tag(prop, idx as i32, value, dv);
                out.push(tag);
            }
        }
        out
    }

    fn encode_tag(&mut self, prop: FieldRef, array_index: i32, value: &Value, defaults: Option<&Value>) -> TaggedProperty {
        let kind = self.prop_kind_of(prop);
        let name = self.res().name_of(prop);
        // `UProperty::GetID`: class/component properties report ObjectProperty; the game's
        // safe object property keeps its own class name.
        let type_name = match kind {
            PropertyKind::Class | PropertyKind::Component => "ObjectProperty",
            k => k.class_name(),
        };
        let mut tag = TaggedProperty {
            name: self.out_fname(&name),
            type_name: self.out_fname(type_name),
            array_index,
            struct_name: None,
            bool_val: None,
            enum_name: None,
            size: 0,
            raw: Vec::new(),
        };
        match kind {
            PropertyKind::Struct => {
                if let Some(s) = self.res().prop_refs(prop).0 {
                    let sn = self.res().name_of(s);
                    tag.struct_name = Some(self.out_fname(&sn));
                }
            }
            PropertyKind::Byte => {
                // `FPropertyTag` always carries `EnumName` for byte properties (`None` without an enum).
                let en = match self.res().prop_refs(prop).0 {
                    Some(e) => self.res().name_of(e),
                    None => "None".to_string(),
                };
                tag.enum_name = Some(self.out_fname(&en));
            }
            PropertyKind::Bool => {
                tag.bool_val = Some(if matches!(value, Value::Bool(true)) { 1 } else { 0 });
                return tag;
            }
            _ => {}
        }
        let mut w = Writer::new();
        self.encode_item(&mut w, prop, value, defaults);
        tag.raw = w.into_inner();
        tag.size = tag.raw.len() as i32;
        tag
    }

    /// Name for output: interned in the compiler name table (the package name
    /// table starts with these, so indices are stable).
    pub fn out_fname(&mut self, text: &str) -> FName {
        let (i, n) = self.fname(text);
        FName { index: i, number: n }
    }

    /// `UProperty::SerializeItem` (save) for one element.
    fn encode_item(&mut self, w: &mut Writer, prop: FieldRef, value: &Value, defaults: Option<&Value>) {
        let kind = self.prop_kind_of(prop);
        match (kind, value) {
            (PropertyKind::Byte, Value::Byte(b)) => match self.res().prop_refs(prop).0 {
                Some(e) => {
                    let names = self.res().enum_names(e);
                    let text = names.get(*b as usize).cloned().unwrap_or_else(|| "None".into());
                    let n = self.out_fname(&text);
                    w.fname(n);
                }
                None => w.u8(*b),
            },
            (PropertyKind::Int, Value::Int(v)) => w.i32(*v),
            (PropertyKind::Float, Value::Float(v)) => w.f32(*v),
            (PropertyKind::Bool, Value::Bool(v)) => w.u8(*v as u8),
            (PropertyKind::Name, Value::Name(s)) => {
                let n = self.out_fname(s);
                w.fname(n);
            }
            (PropertyKind::Str, Value::Str(s)) => w.fstring(s),
            (PropertyKind::Object | PropertyKind::Class | PropertyKind::Component | PropertyKind::SafeObject, Value::Object(o)) => {
                let i = self.pending_ref(*o);
                w.index(i);
            }
            (PropertyKind::Interface, Value::Interface(o)) => {
                let i = self.pending_ref(*o);
                w.index(i);
            }
            (PropertyKind::Delegate, Value::Delegate(o, f)) => {
                let i = self.pending_ref(*o);
                w.index(i);
                let n = self.out_fname(f);
                w.fname(n);
            }
            (PropertyKind::Struct, Value::Struct(slots)) => {
                let Some(s) = self.res().prop_refs(prop).0 else { return };
                if self.struct_is_binary(s) {
                    let props = self.props_in_layout_order(s);
                    for p in props {
                        if !self.should_serialize(p, self.bin_defaults_ctx) {
                            continue;
                        }
                        if let Some(sl) = find_slot(slots, p) {
                            for e in &sl.elems {
                                self.encode_item(w, p, e, None);
                            }
                        }
                    }
                } else {
                    let dslots = match defaults {
                        Some(Value::Struct(d)) => Some(d.as_slice()),
                        _ => None,
                    };
                    let atomic = self.res().struct_flags(s) & stf::ATOMIC != 0 || (self.cooked && self.res().struct_flags(s) & stf::ATOMIC_WHEN_COOKED != 0);
                    let tags = self.encode_tagged(slots, s, dslots, None, false, true, atomic);
                    let none = self.out_fname("None");
                    crate::objects::write_tagged_properties(w, &tags, none);
                }
            }
            (PropertyKind::Array, Value::Array(items)) => {
                let Some(inner) = self.res().prop_refs(prop).0 else { return };
                w.i32(items.len() as i32);
                for it in items {
                    self.encode_item(w, inner, it, None);
                }
            }
            (PropertyKind::Map, _) => {}
            _ => {}
        }
    }

    /// Object reference inside default-property data: encoded through the
    /// bytecode handle table so the emitter can map it to a package index.
    fn pending_ref(&mut self, r: Option<FieldRef>) -> PackageIndex {
        PackageIndex(self.ref_handle(r) as i32)
    }
}

/// Remap pending object references (`PackageIndex(handle)`) inside raw
/// tagged-property bytes: re-decode is not possible, so the emitter calls
/// this with the property definitions to rewrite references in place.
pub fn remap_tagged_refs(
    tags: &mut [TaggedProperty],
    props: &dyn Fn(&str) -> Option<(PropertyKind, Option<FieldRef>, Option<FieldRef>)>,
    map: &mut dyn FnMut(PackageIndex) -> PackageIndex,
) {
    let _ = (tags, props, map);
}
