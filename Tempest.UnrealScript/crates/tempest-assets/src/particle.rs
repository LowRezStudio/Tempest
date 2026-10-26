//! Cooked particle systems: the tree the Studio previews (system →
//! emitters → LOD levels → modules) and the baked distribution tables the
//! game samples at runtime.
//!
//! Cooking strips every `Distribution*` curve object: a module property such
//! as `StartSize` is an `FRawDistributionVector` whose `Distribution` is
//! `None` and whose `LookupTable` holds what `UDistribution::GetValue` would
//! return. Table layout: `[min, max]` (over every value, for the engine's
//! range queries) then entries of `LookupTableChunkSize` floats, one every
//! `1 / LookupTableTimeScale` seconds from `LookupTableStartTime`; a chunk
//! is `LookupTableNumElements` values of `ChunkSize / NumElements`
//! components each (two values when `Op` is random or extreme). Editing the
//! table edits what the game plays.

use tempest_unrealscript::objects::PropertyKind;
use tempest_upk::io::{Error, PackageIndex, Result};
use tempest_upk::{FName, Package};

use crate::object::Object;
use crate::reflect::ClassDb;
use crate::value::{tag_type_name, Property, StructValue, Value};

/// New contents of a raw distribution: the table and the optional header
/// fields (`None` keeps what the module has).
#[derive(Clone, Debug, Default)]
pub struct TableEdit {
    pub table: Vec<f32>,
    pub time_scale: Option<f32>,
    pub start_time: Option<f32>,
    pub op: Option<u8>,
    pub num_elements: Option<u8>,
    pub chunk_size: Option<u8>,
}

/// Set (or add) a member of a tagged struct.
pub fn set_member(pkg: &mut Package, members: &mut Vec<Property>, name: &str, kind: PropertyKind, value: Value) {
    // Content packages carry lower-case names: match the existing member
    // whatever its case, so no duplicate is added.
    let existing = members.iter().position(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(name));
    let type_name = tag_type_name(pkg, kind);
    if let Some(p) = existing.map(|i| &mut members[i]) {
        p.value = value;
        p.kind = kind;
        p.type_name = type_name;
        return;
    }
    let fname = pkg.intern_fname(name);
    let enum_name = (kind == PropertyKind::Byte).then(|| FName::new(pkg.intern_name("None")));
    members.push(Property { name: fname, array_index: 0, kind, type_name, struct_name: None, enum_name, value });
}

/// Replace the lookup table (and header fields) of the raw distribution
/// property `prop` of a module.
pub fn set_distribution(pkg: &mut Package, obj: &mut Object, prop: &str, edit: &TableEdit) -> Result<()> {
    let at = obj.properties.iter().position(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(prop)).ok_or_else(|| Error::Format(format!("no property {prop} on this module")))?;
    let p = &mut obj.properties[at];
    let Value::Struct(StructValue::Tagged(members)) = &mut p.value else {
        return Err(Error::Format(format!("{prop} is not a distribution struct")));
    };
    set_member(pkg, members, "LookupTable", PropertyKind::Array, Value::Array(edit.table.iter().map(|f| Value::Float(*f)).collect()));
    if let Some(v) = edit.time_scale {
        set_member(pkg, members, "LookupTableTimeScale", PropertyKind::Float, Value::Float(v));
    }
    if let Some(v) = edit.start_time {
        set_member(pkg, members, "LookupTableStartTime", PropertyKind::Float, Value::Float(v));
    }
    if let Some(v) = edit.op {
        set_member(pkg, members, "Op", PropertyKind::Byte, Value::Byte(v));
    }
    if let Some(v) = edit.num_elements {
        set_member(pkg, members, "LookupTableNumElements", PropertyKind::Byte, Value::Byte(v));
    }
    if let Some(v) = edit.chunk_size {
        set_member(pkg, members, "LookupTableChunkSize", PropertyKind::Byte, Value::Byte(v));
    }
    Ok(())
}

/// The ParticleSystem export owning `i` (itself, or an outer).
pub fn system_root(db: &ClassDb, pkg: &Package, mut i: usize) -> Option<usize> {
    for _ in 0..16 {
        let class = pkg.class_name_of(PackageIndex::export(i));
        if db.class_is_a(&class, "ParticleSystem") {
            return Some(i);
        }
        i = pkg.exports.get(i)?.outer.export_index()?;
    }
    None
}

/// Export indices referenced by an object-array property.
pub fn object_array(pkg: &Package, obj: &Object, name: &str) -> Vec<usize> {
    match obj.property(name, pkg).map(|p| &p.value) {
        Some(Value::Array(items)) => items.iter().filter_map(|v| if let Value::Object(r) = v { r.export_index() } else { None }).collect(),
        _ => Vec::new(),
    }
}

/// The export an object property references.
pub fn object_ref(pkg: &Package, obj: &Object, name: &str) -> Option<PackageIndex> {
    match obj.property(name, pkg).map(|p| &p.value) {
        Some(Value::Object(r)) if !r.is_null() => Some(*r),
        _ => None,
    }
}
