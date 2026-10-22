//! One export as the engine sees it: the `UObject::Serialize` prefix (state
//! frame, component template info, `NetIndex`), the tagged script
//! properties, and the class's native payload after them.
//!
//! The native payload is kept verbatim until a class-specific serializer
//! exists for it (see the per-asset modules as they land); the object still
//! writes back byte-for-byte either way.

use tempest_unrealscript::context::ObjRef;
use tempest_unrealscript::objects::{self, read_object_header, write_object_header, ComponentHeader, ExportInfo, HeaderOptions, Layout, ObjectHeader, ParsedObject, StateFrame};
use tempest_upk::flags::rf;
use tempest_upk::io::{FName, PackageIndex, Reader, Writer};
use tempest_upk::Package;

use crate::actors::{ApexGenericAsset, DecalComponent, FluidSurfaceComponent, HavokNavMesh, InstancedFoliageActor, LandscapeCollision, LandscapeComponent, SoundClass, MetaData, PrefabInstance, Pylon, SeqActInterp, SpeedTreeComponent, StaticLightCollectionActor};
use crate::level::{AnimSequence, BrushComponent, Level, Model, ModelComponent, Polys, StaticMeshComponent, World};
use crate::material::{Material, MaterialInstance};
use crate::mesh::{self, BodySetup, PhysicsAssetInstance, SkeletalMesh, StaticMesh};
use crate::misc::{Font, GuidCache, ObjectRedirector, ShadowMap1D, SoundCue, SoundNodeWave};
use crate::value::Value;
use crate::reflect::ClassDb;
use crate::texture::{Texture2D, TextureBase, TextureMovie};
use crate::value::{encode_tagged, Decoder, Property};

/// The class-specific payload after the tagged properties.
#[derive(Clone, Debug, PartialEq)]
pub enum Native {
    /// Not decoded (no serializer for the class yet, or it did not consume
    /// the payload exactly); kept verbatim.
    Raw(Vec<u8>),
    Texture2D(Texture2D),
    Texture(TextureBase),
    TextureMovie(TextureMovie),
    SoundNodeWave(SoundNodeWave),
    SoundCue(SoundCue),
    Font(Font),
    ObjectRedirector(ObjectRedirector),
    GuidCache(GuidCache),
    ShadowMap1D(ShadowMap1D),
    StaticMesh(Box<StaticMesh>),
    SkeletalMesh(Box<SkeletalMesh>),
    BodySetup(BodySetup),
    PhysicsAssetInstance(PhysicsAssetInstance),
    AnimSequence(AnimSequence),
    StaticMeshComponent(StaticMeshComponent),
    BrushComponent(BrushComponent),
    ModelComponent(ModelComponent),
    Model(Box<Model>),
    Polys(Polys),
    World(World),
    Level(Box<Level>),
    Material(Material),
    MaterialInstance(MaterialInstance),
    SeqActInterp(SeqActInterp),
    StaticLightCollectionActor(StaticLightCollectionActor),
    PrefabInstance(PrefabInstance),
    DecalComponent(DecalComponent),
    InstancedFoliageActor(InstancedFoliageActor),
    FluidSurfaceComponent(FluidSurfaceComponent),
    SpeedTreeComponent(SpeedTreeComponent),
    Pylon(Pylon),
    MetaData(MetaData),
    ApexGenericAsset(ApexGenericAsset),
    HavokNavMesh(HavokNavMesh),
    SoundClass(SoundClass),
    LandscapeComponent(LandscapeComponent),
    LandscapeCollision(LandscapeCollision),
}

impl Native {
    pub fn is_raw(&self) -> bool {
        matches!(self, Native::Raw(b) if !b.is_empty())
    }

    pub fn raw_len(&self) -> usize {
        match self {
            Native::Raw(b) => b.len(),
            _ => 0,
        }
    }

    pub fn write(&self, w: &mut Writer) {
        match self {
            Native::Raw(b) => w.bytes(b),
            Native::Texture2D(t) => t.write(w),
            Native::Texture(t) => t.write(w),
            Native::TextureMovie(t) => t.write(w),
            Native::SoundNodeWave(t) => t.write(w),
            Native::SoundCue(t) => t.write(w),
            Native::Font(t) => t.write(w),
            Native::ObjectRedirector(t) => t.write(w),
            Native::GuidCache(t) => t.write(w),
            Native::ShadowMap1D(t) => t.write(w),
            Native::StaticMesh(t) => t.write(w),
            Native::SkeletalMesh(t) => t.write(w),
            Native::BodySetup(t) => t.write(w),
            Native::PhysicsAssetInstance(t) => t.write(w),
            Native::AnimSequence(t) => t.write(w),
            Native::StaticMeshComponent(t) => t.write(w),
            Native::BrushComponent(t) => t.write(w),
            Native::ModelComponent(t) => t.write(w),
            Native::Model(t) => t.write(w),
            Native::Polys(t) => t.write(w),
            Native::World(t) => t.write(w),
            Native::Level(t) => t.write(w),
            Native::Material(t) => t.write(w),
            Native::MaterialInstance(t) => t.write(w),
            Native::SeqActInterp(t) => t.write(w),
            Native::StaticLightCollectionActor(t) => t.write(w),
            Native::PrefabInstance(t) => t.write(w),
            Native::DecalComponent(t) => t.write(w),
            Native::InstancedFoliageActor(t) => t.write(w),
            Native::FluidSurfaceComponent(t) => t.write(w),
            Native::SpeedTreeComponent(t) => t.write(w),
            Native::Pylon(t) => t.write(w),
            Native::MetaData(t) => t.write(w),
            Native::ApexGenericAsset(t) => t.write(w),
            Native::HavokNavMesh(t) => t.write(w),
            Native::SoundClass(t) => t.write(w),
            Native::LandscapeComponent(t) => t.write(w),
            Native::LandscapeCollision(t) => t.write(w),
        }
    }
}

/// Diagnostic: run the native reader of `class_name` on `data` and report
/// how far it got (for `upk native-debug`).
pub fn native_debug(db: &ClassDb, class_name: &str, props: &[Property], pkg: &Package, data: &[u8]) -> String {
    let is_a = |n: &str| db.class_is_a(class_name, n);
    let has_vertex_colors = props.iter().any(|p| matches!(p.value, Value::Bool(true)) && pkg.fname_to_string(p.name).eq_ignore_ascii_case("bHasVertexColors"));
    let mut out = String::new();
    for server in [false, true] {
        let layout = mesh::Layout { server };
        let mut r = Reader::new(data);
        let res: std::result::Result<Native, tempest_upk::Error> = if is_a("StaticMesh") {
            StaticMesh::read(&mut r, layout).map(|m| Native::StaticMesh(Box::new(m)))
        } else if is_a("StaticMeshComponent") {
            StaticMeshComponent::read(&mut r, layout).map(Native::StaticMeshComponent)
        } else if is_a("SkeletalMesh") {
            SkeletalMesh::read(&mut r, layout, has_vertex_colors).map(|m| Native::SkeletalMesh(Box::new(m)))
        } else if is_a("Model") {
            Model::read(&mut r).map(|m| Native::Model(Box::new(m)))
        } else if is_a("Level") {
            Level::read(&mut r).map(|l| Native::Level(Box::new(l)))
        } else if is_a("Polys") {
            Polys::read(&mut r).map(Native::Polys)
        } else if is_a("AnimSequence") {
            AnimSequence::read(&mut r).map(Native::AnimSequence)
        } else if is_a("Material") {
            Material::read(&mut r).map(Native::Material)
        } else if is_a("MaterialInstance") {
            let has_perm = props.iter().any(|p| matches!(p.value, Value::Bool(true)) && pkg.fname_to_string(p.name).eq_ignore_ascii_case("bHasStaticPermutationResource"));
            MaterialInstance::read(&mut r, has_perm).map(Native::MaterialInstance)
        } else {
            out.push_str("no debug reader for this class\n");
            break;
        };
        match res {
            Ok(n) => {
                let mut w = Writer::new();
                n.write(&mut w);
                let back = w.into_inner();
                let same = back == data;
                let first_diff = back.iter().zip(data.iter()).position(|(a, b)| a != b);
                out.push_str(&format!("layout server={server}: parsed, consumed {} of {} bytes, re-encode {} bytes, identical={same}{}\n", r.pos(), data.len(), back.len(), first_diff.map(|d| format!(", first diff at {d}")).unwrap_or_default()));
            }
            Err(e) => out.push_str(&format!("layout server={server}: error at {}: {e}\n", r.pos())),
        }
    }
    out
}

/// Decode the native payload of an object of class `class_name`; `Raw` when
/// there is no serializer or the bytes do not round-trip. Mesh payloads are
/// tried with the client layout first, then the server one (GPU buffers
/// stripped), since nothing in the package says which cooker produced it.
fn parse_native(db: &ClassDb, class_name: &str, props: &[Property], pkg: &Package, data: &[u8]) -> Native {
    let is_a = |n: &str| db.class_is_a(class_name, n);
    if is_a("StaticMesh") || is_a("SkeletalMesh") || is_a("StaticMeshComponent") {
        let has_vertex_colors = props.iter().any(|p| matches!(p.value, Value::Bool(true)) && pkg.fname_to_string(p.name).eq_ignore_ascii_case("bHasVertexColors"));
        for server in [false, true] {
            let layout = mesh::Layout { server };
            let mut r = Reader::new(data);
            let parsed = if is_a("StaticMesh") {
                StaticMesh::read(&mut r, layout).map(|m| Native::StaticMesh(Box::new(m)))
            } else if is_a("StaticMeshComponent") {
                StaticMeshComponent::read(&mut r, layout).map(Native::StaticMeshComponent)
            } else {
                SkeletalMesh::read(&mut r, layout, has_vertex_colors).map(|m| Native::SkeletalMesh(Box::new(m)))
            };
            if let Ok(n) = parsed {
                if r.remaining() == 0 {
                    let mut w = Writer::new();
                    n.write(&mut w);
                    if w.into_inner() == data {
                        return n;
                    }
                }
            }
        }
        return Native::Raw(data.to_vec());
    }
    let mut r = Reader::new(data);
    let parsed = if is_a("LightMapTexture2D") {
        Texture2D::read(&mut r, true).map(Native::Texture2D)
    } else if is_a("Texture2D") {
        Texture2D::read(&mut r, false).map(Native::Texture2D)
    } else if is_a("TextureMovie") {
        TextureMovie::read(&mut r).map(Native::TextureMovie)
    } else if is_a("Texture") {
        TextureBase::read(&mut r).map(Native::Texture)
    } else if is_a("SoundNodeWave") {
        SoundNodeWave::read(&mut r).map(Native::SoundNodeWave)
    } else if is_a("SoundCue") {
        SoundCue::read(&mut r).map(Native::SoundCue)
    } else if is_a("Font") {
        Font::read(&mut r).map(Native::Font)
    } else if is_a("ObjectRedirector") {
        ObjectRedirector::read(&mut r).map(Native::ObjectRedirector)
    } else if is_a("GuidCache") {
        GuidCache::read(&mut r).map(Native::GuidCache)
    } else if is_a("ShadowMap1D") {
        ShadowMap1D::read(&mut r).map(Native::ShadowMap1D)
    } else if is_a("RB_BodySetup") {
        BodySetup::read(&mut r).map(Native::BodySetup)
    } else if is_a("PhysicsAssetInstance") {
        PhysicsAssetInstance::read(&mut r).map(Native::PhysicsAssetInstance)
    } else if is_a("AnimSequence") {
        AnimSequence::read(&mut r).map(Native::AnimSequence)
    } else if is_a("BrushComponent") {
        BrushComponent::read(&mut r).map(Native::BrushComponent)
    } else if is_a("ModelComponent") {
        ModelComponent::read(&mut r).map(Native::ModelComponent)
    } else if is_a("Model") {
        Model::read(&mut r).map(|m| Native::Model(Box::new(m)))
    } else if is_a("Polys") {
        Polys::read(&mut r).map(Native::Polys)
    } else if is_a("World") {
        World::read(&mut r).map(Native::World)
    } else if is_a("Level") {
        Level::read(&mut r).map(|l| Native::Level(Box::new(l)))
    } else if is_a("Material") {
        Material::read(&mut r).map(Native::Material)
    } else if is_a("MaterialInstance") {
        let has_perm = props.iter().any(|p| matches!(p.value, Value::Bool(true)) && pkg.fname_to_string(p.name).eq_ignore_ascii_case("bHasStaticPermutationResource"));
        MaterialInstance::read(&mut r, has_perm).map(Native::MaterialInstance)
    } else if is_a("SeqAct_Interp") {
        SeqActInterp::read(&mut r).map(Native::SeqActInterp)
    } else if is_a("StaticLightCollectionActor") {
        StaticLightCollectionActor::read(&mut r).map(Native::StaticLightCollectionActor)
    } else if is_a("PrefabInstance") {
        PrefabInstance::read(&mut r).map(Native::PrefabInstance)
    } else if is_a("DecalComponent") {
        DecalComponent::read(&mut r).map(Native::DecalComponent)
    } else if is_a("InstancedFoliageActor") {
        InstancedFoliageActor::read(&mut r).map(Native::InstancedFoliageActor)
    } else if is_a("FluidSurfaceComponent") {
        FluidSurfaceComponent::read(&mut r).map(Native::FluidSurfaceComponent)
    } else if is_a("SpeedTreeComponent") {
        SpeedTreeComponent::read(&mut r).map(Native::SpeedTreeComponent)
    } else if is_a("Pylon") {
        Pylon::read(&mut r).map(Native::Pylon)
    } else if is_a("MetaData") {
        MetaData::read(&mut r).map(Native::MetaData)
    } else if is_a("ApexGenericAsset") {
        ApexGenericAsset::read(&mut r).map(Native::ApexGenericAsset)
    } else if is_a("HavokNavMeshActor") || is_a("HavokNavMeshGrid") {
        HavokNavMesh::read(&mut r).map(Native::HavokNavMesh)
    } else if is_a("SoundClass") {
        SoundClass::read(&mut r).map(Native::SoundClass)
    } else if is_a("LandscapeComponent") {
        LandscapeComponent::read(&mut r).map(Native::LandscapeComponent)
    } else if is_a("LandscapeHeightfieldCollisionComponent") {
        LandscapeCollision::read(&mut r).map(Native::LandscapeCollision)
    } else {
        return Native::Raw(data.to_vec());
    };
    match parsed {
        Ok(n) if r.remaining() == 0 => {
            let mut w = Writer::new();
            n.write(&mut w);
            if w.into_inner() == data { n } else { Native::Raw(data.to_vec()) }
        }
        _ => Native::Raw(data.to_vec()),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    /// The class (resolved in the class database) — `None` for `Class`
    /// exports and classes the database does not know.
    pub class: Option<ObjRef>,
    pub class_name: String,
    /// Native data a class writes before `Super::Serialize` (dominant light
    /// components' shadow map array).
    pub native_prefix: Vec<u8>,
    pub state_frame: Option<StateFrame>,
    pub component: Option<ComponentHeader>,
    pub net_index: i32,
    pub properties: Vec<Property>,
    /// Everything after the tagged properties (`None` terminator included in
    /// the header): the class's own `Serialize` body.
    pub native: Native,
    /// The export is a `UClass` (no tagged properties in the header).
    pub is_class: bool,
}

/// Result of parsing one export.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Parsed {
    Object(Object),
    /// A script object (class, function, property, enum, …) in the exact
    /// form `tempest_unrealscript::objects` reads and writes.
    Script(ParsedObject),
    /// Zero-size placeholder with no class (left by the cooker).
    Empty,
    /// The header could not be read; payload kept as-is.
    Raw { data: Vec<u8>, error: String },
}

impl Object {
    pub fn has_raw_values(&self) -> bool {
        self.properties.iter().any(|p| p.value.has_raw())
    }

    pub fn property(&self, name: &str, pkg: &Package) -> Option<&Property> {
        self.properties.iter().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(name))
    }
}

/// What the header reader needs to know about an export, derived from the
/// tables and the class database.
pub fn header_options(db: &ClassDb, pkg: &Package, i: usize) -> (HeaderOptions, Option<ObjRef>, String) {
    let e = &pkg.exports[i];
    let idx = PackageIndex::export(i);
    let class_name = pkg.class_name_of(idx);
    let is_class = e.class.is_null();
    let class = if is_class { None } else { db.resolve_class(&class_name) };
    let cdo = e.object_flags & rf::CLASS_DEFAULT_OBJECT != 0;
    let is_component = !cdo && !is_class && db.class_is_a(&class_name, "Component");
    // `UComponent::PreSerialize`: `TemplateName` follows when
    // `IsTemplate(RF_ClassDefaultObject)` — the object or one of its outers
    // is a class default object.
    let mut component_is_template = cdo;
    let mut cur = e.outer;
    let mut guard = 0;
    while let Some(o) = cur.export_index() {
        if pkg.exports[o].object_flags & rf::CLASS_DEFAULT_OBJECT != 0 {
            component_is_template = true;
            break;
        }
        cur = pkg.exports[o].outer;
        guard += 1;
        if guard > 64 {
            break;
        }
    }
    let opts = HeaderOptions { object_flags: e.object_flags, is_class, is_component, component_is_template, native_prefix_len: 0 };
    (opts, class, class_name)
}

/// Bytes a class serialises before `Super::Serialize`, which therefore
/// precede the `UObject` header in the payload:
/// `UDominant{Directional,Spot}LightComponent::Serialize` writes
/// `TArray<WORD> DominantLightShadowMap` first (class default objects of
/// those classes are not serialised through that path).
fn native_prefix_len(db: &ClassDb, class: Option<ObjRef>, object_flags: u64, data: &[u8]) -> usize {
    if object_flags & rf::CLASS_DEFAULT_OBJECT != 0 {
        return 0;
    }
    let dominant = ["DominantDirectionalLightComponent", "DominantSpotLightComponent"];
    if !class.map(|c| dominant.iter().any(|d| db.is_a(c, d))).unwrap_or(false) {
        return 0;
    }
    let count = Reader::new(data).i32().unwrap_or(0).max(0) as usize;
    4 + 2 * count
}

/// Class names whose payloads `tempest_unrealscript::objects` serialises exactly.
const SCRIPT_CLASSES: [&str; 22] = [
    "Class", "State", "Function", "ScriptStruct", "Enum", "Const", "TextBuffer",
    "ByteProperty", "IntProperty", "BoolProperty", "FloatProperty", "ObjectProperty", "ComponentProperty",
    "SafeObjectProperty", "ClassProperty", "InterfaceProperty", "NameProperty", "StrProperty", "ArrayProperty",
    "MapProperty", "StructProperty", "DelegateProperty",
];

/// Canonical spelling of a script class name (content packages lowercase it).
fn script_class(name: &str) -> Option<&'static str> {
    SCRIPT_CLASSES.iter().copied().find(|c| c.eq_ignore_ascii_case(name))
}

/// Per-package serialisation layout (`UProperty` editor data present or not).
pub fn package_layout(pkg: &Package) -> Layout {
    objects::probe_layout(pkg)
}

/// Parse export `i` of `pkg` from `data` (its payload).
pub fn parse_export(db: &ClassDb, pkg: &Package, layout: Layout, i: usize, data: &[u8]) -> Parsed {
    let e = &pkg.exports[i];
    if data.is_empty() && e.class.is_null() {
        return Parsed::Empty;
    }
    let (mut opts, class, class_name) = header_options(db, pkg, i);
    if let Some(cn) = script_class(&class_name) {
        let info = ExportInfo { class_name: cn, object_flags: e.object_flags, is_component: false, native_prefix_len: 0 };
        if let Ok(o) = objects::parse_export(data, pkg, info, layout) {
            return Parsed::Script(o);
        }
    }
    opts.native_prefix_len = native_prefix_len(db, class, e.object_flags, data);
    let mut r = Reader::new(data);
    let header: ObjectHeader = match read_object_header(&mut r, pkg, opts) {
        Ok(h) => h,
        Err(err) => return Parsed::Raw { data: data.to_vec(), error: err.to_string() },
    };
    let dec = Decoder::new(db, pkg, e.object_flags);
    let properties = dec.decode_tagged(&header.properties, class);
    let native = parse_native(db, &class_name, &properties, pkg, r.take(r.remaining()).unwrap_or(&[]));
    Parsed::Object(Object {
        class,
        class_name,
        native_prefix: header.native_prefix,
        state_frame: header.state_frame,
        component: header.component,
        net_index: header.net_index,
        properties,
        native,
        is_class: opts.is_class,
    })
}

/// Serialise an object back to an export payload.
pub fn write_object(obj: &Object, none: FName) -> Vec<u8> {
    let header = ObjectHeader {
        native_prefix: obj.native_prefix.clone(),
        state_frame: obj.state_frame.clone(),
        component: obj.component.clone(),
        net_index: obj.net_index,
        properties: encode_tagged(&obj.properties, none),
    };
    let mut w = Writer::new();
    write_object_header(&mut w, &header, obj.is_class, none);
    obj.native.write(&mut w);
    w.into_inner()
}

pub fn write_parsed(p: &Parsed, layout: Layout, none: FName) -> Vec<u8> {
    match p {
        Parsed::Object(o) => write_object(o, none),
        Parsed::Script(o) => objects::write_export(o, layout, none),
        Parsed::Empty => Vec::new(),
        Parsed::Raw { data, .. } => data.clone(),
    }
}

/// The `None` name of a package (every package that has objects has one).
pub fn none_name(pkg: &Package) -> FName {
    FName::new(pkg.find_name("None").unwrap_or(0))
}
