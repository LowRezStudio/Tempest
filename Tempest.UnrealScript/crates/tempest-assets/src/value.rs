//! Typed property values.
//!
//! A [`Property`] is one `FPropertyTag` with its value decoded. Decoding uses
//! the class database for everything the tag does not say itself (array
//! element types, binary struct layouts, enums inside arrays); whatever
//! cannot be decoded, or does not re-encode to the exact original bytes,
//! stays as [`Value::Raw`] so the object still round-trips. Encoding never
//! needs reflection: every value variant knows its own layout.
//!
//! Engine references: `UStruct::SerializeTaggedProperties`,
//! `UStruct::SerializeBin`, `U*Property::SerializeItem` (UnClass.cpp,
//! UnProp.cpp).

use tempest_unrealscript::context::ObjRef;
use tempest_unrealscript::objects::{read_tagged_properties, write_tagged_properties, NameTable, PropertyKind, TaggedProperty};
use tempest_upk::flags::{pkg as pkgf, rf};
use tempest_upk::io::{Error, FName, PackageIndex, Reader, Result, Writer};
use tempest_upk::Package;

use crate::reflect::ClassDb;

#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub name: FName,
    pub array_index: i32,
    /// Tag type.
    pub kind: PropertyKind,
    /// The tag type's name-table entry, kept so the tag re-encodes verbatim.
    pub type_name: FName,
    /// `StructName` of the tag (struct properties).
    pub struct_name: Option<FName>,
    /// `EnumName` of the tag (byte properties; `None` when not an enum).
    pub enum_name: Option<FName>,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Byte(u8),
    /// Byte property with an enum: the tag name of the value.
    Enum(FName),
    Int(i32),
    Float(f32),
    Name(FName),
    Str(String),
    /// Object, component, safeobject, class and interface properties.
    Object(PackageIndex),
    Delegate { object: PackageIndex, function: FName },
    Struct(StructValue),
    Array(Vec<Value>),
    /// Bytes that could not be decoded (or did not re-encode identically).
    Raw(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum StructValue {
    /// Ordinary struct: a tagged property list.
    Tagged(Vec<Property>),
    /// `STRUCT_Immutable` struct: members in `PropertyLink` order.
    Binary(Vec<Member>),
}

/// One member of a binary struct (static arrays carry `array_dim` elements).
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub name: String,
    pub kind: PropertyKind,
    pub elems: Vec<Value>,
}

impl Value {
    pub fn is_raw(&self) -> bool {
        matches!(self, Value::Raw(_))
    }

    /// Does this value (or anything nested in it) contain raw bytes?
    pub fn has_raw(&self) -> bool {
        match self {
            Value::Raw(_) => true,
            Value::Struct(StructValue::Tagged(ps)) => ps.iter().any(|p| p.value.has_raw()),
            Value::Struct(StructValue::Binary(ms)) => ms.iter().any(|m| m.elems.iter().any(|e| e.has_raw())),
            Value::Array(items) => items.iter().any(|v| v.has_raw()),
            _ => false,
        }
    }

    /// Every object reference in the value, for dependency walks and remaps.
    pub fn for_each_ref(&self, f: &mut dyn FnMut(PackageIndex)) {
        match self {
            Value::Object(i) | Value::Delegate { object: i, .. } => f(*i),
            Value::Struct(StructValue::Tagged(ps)) => ps.iter().for_each(|p| p.value.for_each_ref(f)),
            Value::Struct(StructValue::Binary(ms)) => ms.iter().for_each(|m| m.elems.iter().for_each(|e| e.for_each_ref(f))),
            Value::Array(items) => items.iter().for_each(|v| v.for_each_ref(f)),
            _ => {}
        }
    }

    pub fn map_refs(&mut self, f: &mut dyn FnMut(PackageIndex) -> PackageIndex) {
        match self {
            Value::Object(i) | Value::Delegate { object: i, .. } => *i = f(*i),
            Value::Struct(StructValue::Tagged(ps)) => ps.iter_mut().for_each(|p| p.value.map_refs(f)),
            Value::Struct(StructValue::Binary(ms)) => ms.iter_mut().for_each(|m| m.elems.iter_mut().for_each(|e| e.map_refs(f))),
            Value::Array(items) => items.iter_mut().for_each(|v| v.map_refs(f)),
            _ => {}
        }
    }
}

/// `PropertyKind` from a tag type name, whatever its case.
pub fn kind_from_type_name(name: &str) -> Option<PropertyKind> {
    Some(match name.to_ascii_lowercase().as_str() {
        "byteproperty" => PropertyKind::Byte,
        "intproperty" => PropertyKind::Int,
        "boolproperty" => PropertyKind::Bool,
        "floatproperty" => PropertyKind::Float,
        "objectproperty" => PropertyKind::Object,
        "componentproperty" => PropertyKind::Component,
        "safeobjectproperty" => PropertyKind::SafeObject,
        "classproperty" => PropertyKind::Class,
        "interfaceproperty" => PropertyKind::Interface,
        "nameproperty" => PropertyKind::Name,
        "strproperty" => PropertyKind::Str,
        "arrayproperty" => PropertyKind::Array,
        "mapproperty" => PropertyKind::Map,
        "structproperty" => PropertyKind::Struct,
        "delegateproperty" => PropertyKind::Delegate,
        _ => return None,
    })
}

// ----------------------------------------------------------------------------
// Decoding
// ----------------------------------------------------------------------------

pub struct Decoder<'a> {
    pub db: &'a ClassDb,
    pub pkg: &'a Package,
    /// `PKG_Cooked` on the package.
    pub cooked: bool,
    /// `FArchive::IsSerializingDefaults`: the object is a class default object.
    pub defaults: bool,
}

impl<'a> Decoder<'a> {
    pub fn new(db: &'a ClassDb, pkg: &'a Package, object_flags: u64) -> Decoder<'a> {
        Decoder {
            db,
            pkg,
            cooked: pkg.summary.package_flags & pkgf::COOKED != 0,
            defaults: object_flags & rf::CLASS_DEFAULT_OBJECT != 0,
        }
    }

    fn name_is_none(&self, n: FName) -> bool {
        self.pkg.is_none(n)
    }

    /// Decode a tag list against `owner` (the class or struct that declares
    /// the properties; `None` decodes from the tags alone).
    pub fn decode_tagged(&self, tags: &[TaggedProperty], owner: Option<ObjRef>) -> Vec<Property> {
        tags.iter().map(|tp| self.decode_tag(tp, owner)).collect()
    }

    fn decode_tag(&self, tp: &TaggedProperty, owner: Option<ObjRef>) -> Property {
        let type_name = self.pkg.name_str(tp.type_name.index);
        let kind = kind_from_type_name(type_name).unwrap_or(PropertyKind::Int);
        let pname = self.pkg.fname_to_string(tp.name);
        let pdef = owner.and_then(|o| self.db.find_property(o, &pname));
        // A definition of a different kind than the tag says is a stale match.
        let pdef = pdef.filter(|&p| self.db.property_kind(p) == Some(kind));
        let mut prop = Property { name: tp.name, array_index: tp.array_index, kind, type_name: tp.type_name, struct_name: tp.struct_name, enum_name: tp.enum_name, value: Value::Raw(tp.raw.clone()) };
        if kind == PropertyKind::Bool {
            prop.value = Value::Bool(tp.bool_val.unwrap_or(0) != 0);
            return prop;
        }
        let tag_enum = tp.enum_name.filter(|&e| !self.name_is_none(e));
        let mut r = Reader::new(&tp.raw);
        let decoded = self.decode_value(&mut r, kind, pdef, tag_enum.is_some(), tp.struct_name);
        if let Ok(v) = decoded {
            if r.remaining() == 0 {
                let mut w = Writer::new();
                encode_value(&mut w, &v, self.none());
                if w.into_inner() == tp.raw {
                    prop.value = v;
                }
            }
        }
        prop
    }

    fn none(&self) -> FName {
        FName::new(self.pkg.find_name("None").unwrap_or(0))
    }

    /// `U*Property::SerializeItem`. `tag_enum`: the enclosing tag says the
    /// byte is an enum; `tag_struct`: the tag's struct name.
    fn decode_value(&self, r: &mut Reader, kind: PropertyKind, pdef: Option<ObjRef>, tag_enum: bool, tag_struct: Option<FName>) -> Result<Value> {
        Ok(match kind {
            PropertyKind::Bool => Value::Bool(r.u8()? != 0),
            PropertyKind::Byte => {
                let has_enum = tag_enum || pdef.and_then(|p| self.db.property_ref1(p)).is_some();
                if has_enum {
                    Value::Enum(r.fname()?)
                } else {
                    Value::Byte(r.u8()?)
                }
            }
            PropertyKind::Int => Value::Int(r.i32()?),
            PropertyKind::Float => Value::Float(r.f32()?),
            PropertyKind::Name => Value::Name(r.fname()?),
            PropertyKind::Str => Value::Str(r.fstring()?),
            PropertyKind::Object | PropertyKind::Component | PropertyKind::SafeObject | PropertyKind::Class | PropertyKind::Interface => {
                Value::Object(r.index()?)
            }
            PropertyKind::Delegate => Value::Delegate { object: r.index()?, function: r.fname()? },
            PropertyKind::Struct => {
                let sdef = pdef
                    .and_then(|p| self.db.property_ref1(p))
                    .or_else(|| tag_struct.and_then(|n| self.db.struct_named(self.pkg.name_str(n.index))));
                let Some(sdef) = sdef else {
                    return Err(Error::Format("unknown struct".into()));
                };
                Value::Struct(self.decode_struct(r, sdef)?)
            }
            PropertyKind::Array => {
                let Some(inner) = pdef.and_then(|p| self.db.property_ref1(p)) else {
                    return Err(Error::Format("unknown array element type".into()));
                };
                let Some(ik) = self.db.property_kind(inner) else {
                    return Err(Error::Format("array element is not a property".into()));
                };
                let n = r.i32()?;
                if n < 0 {
                    return Err(Error::Format(format!("negative array length {n}")));
                }
                let mut items = Vec::with_capacity((n as usize).min(1 << 16));
                for _ in 0..n {
                    items.push(self.decode_value(r, ik, Some(inner), false, None)?);
                }
                Value::Array(items)
            }
            PropertyKind::Map => return Err(Error::Format("map property".into())),
        })
    }

    fn decode_struct(&self, r: &mut Reader, sdef: ObjRef) -> Result<StructValue> {
        if self.db.struct_is_binary(sdef, self.cooked) {
            let mut members = Vec::new();
            for m in self.db.struct_members(sdef) {
                if !self.db.should_serialize(m, self.defaults, self.cooked) {
                    continue;
                }
                let Some(mk) = self.db.property_kind(m) else { continue };
                let dim = self.db.array_dim(m) as usize;
                let mut elems = Vec::with_capacity(dim);
                for _ in 0..dim {
                    elems.push(self.decode_value(r, mk, Some(m), false, None)?);
                }
                members.push(Member { name: self.db.name(m), kind: mk, elems });
            }
            Ok(StructValue::Binary(members))
        } else {
            let tags = read_tagged_properties(r, self.pkg)?;
            Ok(StructValue::Tagged(self.decode_tagged(&tags, Some(sdef))))
        }
    }
}

// ----------------------------------------------------------------------------
// Encoding
// ----------------------------------------------------------------------------

/// Serialise one value (`SerializeItem` layout).
pub fn encode_value(w: &mut Writer, v: &Value, none: FName) {
    match v {
        Value::Bool(b) => w.u8(*b as u8),
        Value::Byte(b) => w.u8(*b),
        Value::Enum(n) | Value::Name(n) => w.fname(*n),
        Value::Int(i) => w.i32(*i),
        Value::Float(f) => w.f32(*f),
        Value::Str(s) => w.fstring(s),
        Value::Object(i) => w.index(*i),
        Value::Delegate { object, function } => {
            w.index(*object);
            w.fname(*function);
        }
        Value::Struct(StructValue::Tagged(ps)) => write_tagged_properties(w, &encode_tagged(ps, none), none),
        Value::Struct(StructValue::Binary(ms)) => {
            for m in ms {
                for e in &m.elems {
                    encode_value(w, e, none);
                }
            }
        }
        Value::Array(items) => {
            w.i32(items.len() as i32);
            for it in items {
                encode_value(w, it, none);
            }
        }
        Value::Raw(b) => w.bytes(b),
    }
}

/// Properties back to tags (the inverse of [`Decoder::decode_tagged`]).
pub fn encode_tagged(props: &[Property], none: FName) -> Vec<TaggedProperty> {
    props
        .iter()
        .map(|p| {
            let mut tp = TaggedProperty {
                name: p.name,
                type_name: p.type_name,
                array_index: p.array_index,
                struct_name: p.struct_name,
                bool_val: None,
                enum_name: p.enum_name,
                size: 0,
                raw: Vec::new(),
            };
            match &p.value {
                Value::Bool(b) if p.kind == PropertyKind::Bool => tp.bool_val = Some(*b as u8),
                v => {
                    let mut w = Writer::new();
                    encode_value(&mut w, v, none);
                    tp.raw = w.into_inner();
                    tp.size = tp.raw.len() as i32;
                }
            }
            tp
        })
        .collect()
}

/// The name-table entry for a property kind's tag type, matching the case
/// the package already uses (content packages are lowercase).
pub fn tag_type_name(pkg: &mut Package, kind: PropertyKind) -> FName {
    let canonical = kind.tag_type();
    if let Some(i) = pkg.find_name(canonical) {
        return FName::new(i);
    }
    FName::new(pkg.intern_name(canonical))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempest_upk::ObjectExport;

    fn pkg_with_names(names: &[&str]) -> Package {
        let mut p = Package::default();
        for n in names {
            p.intern_name(n);
        }
        p.exports.push(ObjectExport::default());
        p
    }

    fn tag(pkg: &Package, name: &str, ty: &str, raw: Vec<u8>) -> TaggedProperty {
        TaggedProperty {
            name: FName::new(pkg.find_name(name).unwrap()),
            type_name: FName::new(pkg.find_name(ty).unwrap()),
            array_index: 0,
            struct_name: None,
            bool_val: None,
            enum_name: None,
            size: raw.len() as i32,
            raw,
        }
    }

    #[test]
    fn scalar_tags_round_trip_without_reflection() {
        let pkg = pkg_with_names(&["None", "intproperty", "floatproperty", "strproperty", "nameproperty", "objectproperty", "a", "b", "c", "d", "e", "hello"]);
        let db = ClassDb::new();
        let mut w = Writer::new();
        w.fstring("hi");
        let tags = vec![
            tag(&pkg, "a", "intproperty", 7i32.to_le_bytes().to_vec()),
            tag(&pkg, "b", "floatproperty", 1.5f32.to_le_bytes().to_vec()),
            tag(&pkg, "c", "strproperty", w.into_inner()),
            tag(&pkg, "d", "nameproperty", { let mut w = Writer::new(); w.fname(FName { index: pkg.find_name("hello").unwrap(), number: 3 }); w.into_inner() }),
            tag(&pkg, "e", "objectproperty", (-2i32).to_le_bytes().to_vec()),
        ];
        let dec = Decoder::new(&db, &pkg, 0);
        let props = dec.decode_tagged(&tags, None);
        assert_eq!(props[0].value, Value::Int(7));
        assert_eq!(props[1].value, Value::Float(1.5));
        assert_eq!(props[2].value, Value::Str("hi".into()));
        assert!(matches!(props[3].value, Value::Name(FName { number: 3, .. })));
        assert_eq!(props[4].value, Value::Object(PackageIndex::import(1)));
        assert_eq!(encode_tagged(&props, FName::new(0)), tags);
    }

    #[test]
    fn undecodable_values_stay_raw_and_round_trip() {
        let pkg = pkg_with_names(&["None", "arrayproperty", "x"]);
        let db = ClassDb::new();
        let raw = vec![1, 0, 0, 0, 9, 9, 9];
        let tags = vec![tag(&pkg, "x", "arrayproperty", raw.clone())];
        let props = Decoder::new(&db, &pkg, 0).decode_tagged(&tags, None);
        assert_eq!(props[0].value, Value::Raw(raw));
        assert_eq!(encode_tagged(&props, FName::new(0)), tags);
    }
}
