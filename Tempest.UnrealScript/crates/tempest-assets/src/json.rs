//! JSON views of packages and parsed objects, for `upk serve` (the Studio
//! GUI) and anything else that wants the typed layer without Rust.
//!
//! Object references are always rendered as `{ "ref": <PackageIndex>,
//! "path": ..., "class": ... }` so a client can show them and follow them;
//! names and enums are plain strings; raw bytes are a length plus a hex
//! prefix (the client never needs the bytes, the editor keeps them).

use serde_json::{json, Map, Value as J};
use tempest_unrealscript::objects::NameTable;
use tempest_upk::flags::{describe, OBJECT_FLAG_NAMES};
use tempest_upk::io::PackageIndex;
use tempest_upk::Package;

use crate::dump::Dumper;
use crate::object::{Native, Object, Parsed};
use crate::value::{Member, Property, StructValue, Value};

pub fn reference(pkg: &Package, idx: PackageIndex) -> J {
    if idx.is_null() {
        return json!({ "ref": 0, "path": "None", "class": "" });
    }
    json!({ "ref": idx.0, "path": pkg.object_path(idx), "class": pkg.class_name_of(idx) })
}

pub fn value(pkg: &Package, v: &Value) -> J {
    match v {
        Value::Bool(b) => json!({ "t": "bool", "v": b }),
        Value::Byte(b) => json!({ "t": "byte", "v": b }),
        Value::Enum(n) => json!({ "t": "enum", "v": pkg.fname_to_string(*n) }),
        Value::Int(i) => json!({ "t": "int", "v": i }),
        Value::Float(f) => json!({ "t": "float", "v": f }),
        Value::Name(n) => json!({ "t": "name", "v": pkg.fname_to_string(*n) }),
        Value::Str(s) => json!({ "t": "str", "v": s }),
        Value::Object(i) => json!({ "t": "object", "v": reference(pkg, *i) }),
        Value::Delegate { object, function } => json!({ "t": "delegate", "v": { "object": reference(pkg, *object), "function": pkg.fname_to_string(*function) } }),
        Value::Struct(StructValue::Tagged(ps)) => json!({ "t": "struct", "v": ps.iter().map(|p| property(pkg, p)).collect::<Vec<_>>() }),
        Value::Struct(StructValue::Binary(ms)) => json!({ "t": "binary", "v": ms.iter().map(|m| member(pkg, m)).collect::<Vec<_>>() }),
        Value::Array(items) => json!({ "t": "array", "v": items.iter().map(|i| value(pkg, i)).collect::<Vec<_>>() }),
        Value::Raw(b) => json!({ "t": "raw", "v": { "len": b.len(), "hex": hex_prefix(b, 64) } }),
    }
}

fn member(pkg: &Package, m: &Member) -> J {
    json!({ "name": m.name, "type": m.kind.tag_type(), "elems": m.elems.iter().map(|e| value(pkg, e)).collect::<Vec<_>>() })
}

pub fn property(pkg: &Package, p: &Property) -> J {
    let mut o = Map::new();
    o.insert("name".into(), J::from(pkg.fname_to_string(p.name)));
    o.insert("index".into(), J::from(p.array_index));
    o.insert("type".into(), J::from(p.kind.tag_type()));
    if let Some(s) = p.struct_name {
        o.insert("struct".into(), J::from(pkg.fname_to_string(s)));
    }
    if let Some(e) = p.enum_name.filter(|e| !pkg.is_none(*e)) {
        o.insert("enum".into(), J::from(pkg.fname_to_string(e)));
    }
    o.insert("value".into(), value(pkg, &p.value));
    J::Object(o)
}

pub fn hex_prefix(b: &[u8], n: usize) -> String {
    let n = b.len().min(n);
    b[..n].iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

pub fn native_type(n: &Native) -> &'static str {
    match n {
        Native::Raw(b) if b.is_empty() => "None",
        Native::Raw(_) => "Raw",
        Native::Texture2D(_) => "Texture2D",
        Native::Texture(_) => "Texture",
        Native::TextureMovie(_) => "TextureMovie",
        Native::SoundNodeWave(_) => "SoundNodeWave",
        Native::SoundCue(_) => "SoundCue",
        Native::Font(_) => "Font",
        Native::ObjectRedirector(_) => "ObjectRedirector",
        Native::GuidCache(_) => "GuidCache",
        Native::ShadowMap1D(_) => "ShadowMap1D",
        Native::StaticMesh(_) => "StaticMesh",
        Native::SkeletalMesh(_) => "SkeletalMesh",
        Native::BodySetup(_) => "BodySetup",
        Native::PhysicsAssetInstance(_) => "PhysicsAssetInstance",
        Native::AnimSequence(_) => "AnimSequence",
        Native::StaticMeshComponent(_) => "StaticMeshComponent",
        Native::BrushComponent(_) => "BrushComponent",
        Native::ModelComponent(_) => "ModelComponent",
        Native::Model(_) => "Model",
        Native::Polys(_) => "Polys",
        Native::World(_) => "World",
        Native::Level(_) => "Level",
        Native::Material(_) => "Material",
        Native::MaterialInstance(_) => "MaterialInstance",
        Native::SeqActInterp(_) => "SeqActInterp",
        Native::StaticLightCollectionActor(_) => "StaticLightCollectionActor",
        Native::PrefabInstance(_) => "PrefabInstance",
        Native::DecalComponent(_) => "DecalComponent",
        Native::InstancedFoliageActor(_) => "InstancedFoliageActor",
        Native::FluidSurfaceComponent(_) => "FluidSurfaceComponent",
        Native::SpeedTreeComponent(_) => "SpeedTreeComponent",
        Native::Pylon(_) => "Pylon",
        Native::MetaData(_) => "MetaData",
        Native::ApexGenericAsset(_) => "ApexGenericAsset",
        Native::HavokNavMesh(_) => "HavokNavMesh",
        Native::SoundClass(_) => "SoundClass",
        Native::LandscapeComponent(_) => "LandscapeComponent",
        Native::LandscapeCollision(_) => "LandscapeCollision",
    }
}

/// A parsed export, fully rendered.
pub fn parsed(pkg: &Package, i: usize, p: &Parsed) -> J {
    let d = Dumper::new(pkg);
    let mut o = Map::new();
    o.insert("index".into(), J::from(i));
    o.insert("path".into(), J::from(pkg.object_path(PackageIndex::export(i))));
    o.insert("class".into(), J::from(pkg.class_name_of(PackageIndex::export(i))));
    match p {
        Parsed::Empty => {
            o.insert("kind".into(), J::from("empty"));
        }
        Parsed::Raw { data, error } => {
            o.insert("kind".into(), J::from("raw"));
            o.insert("error".into(), J::from(error.as_str()));
            o.insert("size".into(), J::from(data.len()));
            o.insert("hex".into(), J::from(hex_prefix(data, 64)));
        }
        Parsed::Script(s) => {
            let mut text = String::new();
            d.parsed(p, &mut text);
            o.insert("kind".into(), J::from("script"));
            o.insert("netIndex".into(), J::from(s.header.net_index));
            o.insert("text".into(), J::from(text));
        }
        Parsed::Object(obj) => {
            o.insert("kind".into(), J::from("object"));
            object_into(pkg, obj, &d, &mut o);
        }
    }
    J::Object(o)
}

fn object_into(pkg: &Package, obj: &Object, d: &Dumper, o: &mut Map<String, J>) {
    o.insert("netIndex".into(), J::from(obj.net_index));
    o.insert("isClass".into(), J::from(obj.is_class));
    o.insert("knownClass".into(), J::from(obj.class.is_some()));
    if let Some(sf) = &obj.state_frame {
        o.insert("stateFrame".into(), json!({ "node": reference(pkg, sf.node), "state": reference(pkg, sf.state_node), "probeMask": sf.probe_mask, "latentAction": sf.latent_action, "stack": sf.state_stack.len(), "codeOffset": sf.code_offset }));
    }
    if let Some(c) = &obj.component {
        o.insert("component".into(), json!({ "templateOwnerClass": reference(pkg, c.template_owner_class), "templateName": c.template_name.map(|n| pkg.fname_to_string(n)) }));
    }
    if !obj.native_prefix.is_empty() {
        o.insert("nativePrefix".into(), J::from(obj.native_prefix.len()));
    }
    o.insert("properties".into(), J::Array(obj.properties.iter().map(|p| property(pkg, p)).collect()));
    let mut text = String::new();
    d.native(&obj.native, &mut text);
    o.insert("native".into(), json!({ "type": native_type(&obj.native), "size": obj.native.raw_len(), "text": text.trim_end() }));
    o.insert("hasRawValues".into(), J::from(obj.has_raw_values()));
}

/// One export-table row.
pub fn export_row(pkg: &Package, i: usize) -> J {
    let e = &pkg.exports[i];
    let idx = PackageIndex::export(i);
    json!({
        "i": i,
        "name": pkg.object_name(idx),
        "path": pkg.object_path(idx),
        "class": pkg.class_name_of(idx),
        "classRef": e.class.0,
        "outer": e.outer.0,
        "super": e.super_.0,
        "archetype": e.archetype.0,
        "size": e.serial_size,
        "offset": e.serial_offset,
        "flags": describe(e.object_flags, OBJECT_FLAG_NAMES),
        "flagsHex": format!("{:#x}", e.object_flags),
        "exportFlags": e.export_flags,
    })
}

pub fn import_row(pkg: &Package, i: usize) -> J {
    let im = &pkg.imports[i];
    let idx = PackageIndex::import(i);
    json!({
        "i": i,
        "name": pkg.object_name(idx),
        "path": pkg.object_path(idx),
        "class": pkg.fname_to_string(im.class_name),
        "classPackage": pkg.fname_to_string(im.class_package),
        "outer": im.outer.0,
    })
}

/// Tables of a loaded package (what the tree view needs).
/// The tables as the Studio receives them: the summary, the import rows and
/// the export table as columns (names, class/outer/super/archetype refs,
/// sizes, offsets, flags) that the renderer turns into rows. A script
/// package has 65k exports: columns of numbers parse in a fraction of the
/// time of 65k objects with their paths and flag texts spelled out, which
/// the renderer derives on demand (`package.export`, `package.names`).
pub fn tables_compact(pkg: &Package) -> J {
    let mut v = tables_summary(pkg);
    let n = pkg.exports.len();
    let mut names = Vec::with_capacity(n);
    let mut classes = Vec::with_capacity(n);
    let mut outers = Vec::with_capacity(n);
    let mut supers = Vec::with_capacity(n);
    let mut archetypes = Vec::with_capacity(n);
    let mut sizes = Vec::with_capacity(n);
    let mut offsets = Vec::with_capacity(n);
    let mut export_flags = Vec::with_capacity(n);
    let mut object_flags = Vec::with_capacity(n);
    for (i, e) in pkg.exports.iter().enumerate() {
        names.push(J::from(pkg.object_name(PackageIndex::export(i))));
        classes.push(e.class.0);
        outers.push(e.outer.0);
        supers.push(e.super_.0);
        archetypes.push(e.archetype.0);
        sizes.push(e.serial_size);
        offsets.push(e.serial_offset);
        export_flags.push(e.export_flags);
        object_flags.push(format!("{:x}", e.object_flags));
    }
    v["imports"] = J::Array((0..pkg.imports.len()).map(|i| import_row(pkg, i)).collect());
    v["exports"] = json!({
        "names": names,
        "classes": classes,
        "outers": outers,
        "supers": supers,
        "archetypes": archetypes,
        "sizes": sizes,
        "offsets": offsets,
        "exportFlags": export_flags,
        "objectFlags": object_flags,
    });
    v
}

fn tables_summary(pkg: &Package) -> J {
    let s = &pkg.summary;
    json!({
        "name": pkg.package_name(),
        "summary": {
            "fileVersion": s.file_version,
            "licenseeVersion": s.licensee_version,
            "engineVersion": s.engine_version,
            "cookedContentVersion": s.cooked_content_version,
            "packageFlags": s.package_flags,
            "packageFlagsText": describe(s.package_flags as u64, tempest_upk::flags::PACKAGE_FLAG_NAMES),
            "compressionFlags": s.compression_flags,
            "compressed": pkg.was_compressed,
            "obscured": s.compression_flags & tempest_upk::flags::compression::OBSCURED != 0,
            "cooked": s.is_cooked(),
            "folder": s.folder_name,
            "guid": s.guid.to_string(),
            "chunks": s.compressed_chunks.len(),
            "headerSize": s.total_header_size,
            "names": pkg.names.len(),
            "imports": pkg.imports.len(),
            "exports": pkg.exports.len(),
            "additionalPackages": s.additional_packages_to_cook,
            "textureAllocations": s.texture_allocations.len(),
        },
    })
}

pub fn tables(pkg: &Package) -> J {
    let s = &pkg.summary;
    json!({
        "name": pkg.package_name(),
        "summary": {
            "fileVersion": s.file_version,
            "licenseeVersion": s.licensee_version,
            "engineVersion": s.engine_version,
            "cookedContentVersion": s.cooked_content_version,
            "packageFlags": s.package_flags,
            "packageFlagsText": describe(s.package_flags as u64, tempest_upk::flags::PACKAGE_FLAG_NAMES),
            "compressionFlags": s.compression_flags,
            "compressed": pkg.was_compressed,
            "obscured": s.compression_flags & tempest_upk::flags::compression::OBSCURED != 0,
            "cooked": s.is_cooked(),
            "folder": s.folder_name,
            "guid": s.guid.to_string(),
            "chunks": s.compressed_chunks.len(),
            "headerSize": s.total_header_size,
            "names": pkg.names.len(),
            "imports": pkg.imports.len(),
            "exports": pkg.exports.len(),
            "additionalPackages": s.additional_packages_to_cook,
            "textureAllocations": s.texture_allocations.len(),
        },
        "names": pkg.names.iter().map(|n| J::from(n.name.as_str())).collect::<Vec<_>>(),
        "imports": (0..pkg.imports.len()).map(|i| import_row(pkg, i)).collect::<Vec<_>>(),
        "exports": (0..pkg.exports.len()).map(|i| export_row(pkg, i)).collect::<Vec<_>>(),
    })
}

/// Standard (non-URL) base64, for binary payloads inside responses.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_reference() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
