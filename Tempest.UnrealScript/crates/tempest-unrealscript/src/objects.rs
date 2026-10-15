//! Serialised forms of the script-related `UObject` subclasses as found in
//! Paladins packages (file version 893).
//!
//! Every reader returns the structure plus the number of consumed bytes; the
//! caller checks it against the export's `SerialSize`. Writers emit the exact
//! inverse.

use tempest_upk::flags::{cpf, func, rf};
use tempest_upk::io::{Error, FName, PackageIndex, Reader, Result, Writer};

/// Per-package serialisation layout choices that the engine decides at load
/// time from platform/cooking state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// `UProperty::Category/ArraySizeEnum` and the `UClass` category lists
    /// are present (not filtered as editor-only).
    pub editor_data: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { editor_data: true }
    }
}

// --------------------------------------------------------------------------
// Tagged properties (default values)
// --------------------------------------------------------------------------

/// One `FPropertyTag` plus the raw value bytes that followed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaggedProperty {
    pub name: FName,
    pub type_name: FName,
    pub array_index: i32,
    /// `StructName` for `StructProperty`.
    pub struct_name: Option<FName>,
    /// `BoolVal` for `BoolProperty` (no payload follows).
    pub bool_val: Option<u8>,
    /// `EnumName` for `ByteProperty`.
    pub enum_name: Option<FName>,
    /// Serialised size as stored in the tag (0 for bools).
    pub size: i32,
    pub raw: Vec<u8>,
}

/// Read tagged properties until the terminating `None` name.
pub fn read_tagged_properties(r: &mut Reader, names: &dyn NameTable) -> Result<Vec<TaggedProperty>> {
    let mut out = Vec::new();
    loop {
        let name = r.fname()?;
        if names.is_none(name) {
            break;
        }
        let type_name = r.fname()?;
        let size = r.i32()?;
        let array_index = r.i32()?;
        let ty = names.name(type_name.index);
        let mut tp = TaggedProperty {
            name,
            type_name,
            array_index,
            struct_name: None,
            bool_val: None,
            enum_name: None,
            size,
            raw: Vec::new(),
        };
        if ty.eq_ignore_ascii_case("StructProperty") {
            tp.struct_name = Some(r.fname()?);
        } else if ty.eq_ignore_ascii_case("BoolProperty") {
            tp.bool_val = Some(r.u8()?);
        } else if ty.eq_ignore_ascii_case("ByteProperty") {
            tp.enum_name = Some(r.fname()?);
        }
        if size < 0 {
            return Err(Error::Format(format!("negative property size {size}")));
        }
        tp.raw = r.take(size as usize)?.to_vec();
        out.push(tp);
    }
    Ok(out)
}

pub fn write_tagged_properties(w: &mut Writer, props: &[TaggedProperty], none: FName) {
    for p in props {
        w.fname(p.name);
        w.fname(p.type_name);
        w.i32(p.raw.len() as i32);
        w.i32(p.array_index);
        if let Some(s) = p.struct_name {
            w.fname(s);
        }
        if let Some(b) = p.bool_val {
            w.u8(b);
        }
        if let Some(e) = p.enum_name {
            w.fname(e);
        }
        w.bytes(&p.raw);
    }
    w.fname(none);
}

/// Name resolution the object readers need (the package name table).
pub trait NameTable {
    fn name(&self, index: u32) -> &str;
    fn is_none(&self, n: FName) -> bool {
        n.number == 0 && self.name(n.index).eq_ignore_ascii_case("None")
    }
}

impl NameTable for tempest_upk::Package {
    fn name(&self, index: u32) -> &str {
        self.name_str(index)
    }
}

// --------------------------------------------------------------------------
// UObject header
// --------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushedState {
    pub state: PackageIndex,
    pub node: PackageIndex,
    pub code_offset: i32,
}

/// `FStateFrame` as serialised for objects with `RF_HasStack`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateFrame {
    pub node: PackageIndex,
    pub state_node: PackageIndex,
    pub probe_mask: u32,
    pub latent_action: u16,
    pub state_stack: Vec<PushedState>,
    /// Only present when `node` is non-null.
    pub code_offset: Option<i32>,
}

/// The `UObject::Serialize` prefix every export starts with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectHeader {
    /// Native data a class serialises before `Super::Serialize` (the dominant light
    /// components' shadow map: an empty array for templates).
    pub native_prefix: Vec<u8>,
    pub state_frame: Option<StateFrame>,
    /// `UComponent::PreSerialize`: `TemplateOwnerClass`, and `TemplateName`
    /// when the component is a template (CDO subobject).
    pub component: Option<ComponentHeader>,
    pub net_index: i32,
    /// Script properties (absent for `UClass` objects, whose class is UClass).
    pub properties: Vec<TaggedProperty>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentHeader {
    pub template_owner_class: PackageIndex,
    pub template_name: Option<FName>,
}

pub fn read_state_frame(r: &mut Reader) -> Result<StateFrame> {
    let node = r.index()?;
    let state_node = r.index()?;
    let probe_mask = r.u32()?;
    let latent_action = r.u16()?;
    let state_stack = r.array(|r| {
        Ok(PushedState { state: r.index()?, node: r.index()?, code_offset: r.i32()? })
    })?;
    let code_offset = if node.is_null() { None } else { Some(r.i32()?) };
    Ok(StateFrame { node, state_node, probe_mask, latent_action, state_stack, code_offset })
}

pub fn write_state_frame(w: &mut Writer, sf: &StateFrame) {
    w.index(sf.node);
    w.index(sf.state_node);
    w.u32(sf.probe_mask);
    w.u16(sf.latent_action);
    w.array(&sf.state_stack, |w, p| {
        w.index(p.state);
        w.index(p.node);
        w.i32(p.code_offset);
    });
    if let Some(c) = sf.code_offset {
        w.i32(c);
    }
}

/// How to read the object header for a given export.
#[derive(Clone, Copy, Debug, Default)]
pub struct HeaderOptions {
    pub object_flags: u64,
    /// The object's class is `Core.Class` (no script properties follow).
    pub is_class: bool,
    /// The object's class derives from `Engine.Component`.
    pub is_component: bool,
    /// `TemplateName` is serialised when the component is a template
    /// (`RF_ClassDefaultObject` on it or an outer) — always true for the
    /// persistent packages we deal with when `is_component`.
    pub component_is_template: bool,
    /// Bytes of native data preceding everything else (see `ObjectHeader::native_prefix`).
    pub native_prefix_len: usize,
}

pub fn read_object_header(r: &mut Reader, names: &dyn NameTable, o: HeaderOptions) -> Result<ObjectHeader> {
    let mut h = ObjectHeader::default();
    if o.native_prefix_len > 0 {
        h.native_prefix = r.take(o.native_prefix_len)?.to_vec();
    }
    if o.object_flags & rf::HAS_STACK != 0 {
        h.state_frame = Some(read_state_frame(r)?);
    }
    if o.is_component {
        let template_owner_class = r.index()?;
        let template_name = if o.component_is_template { Some(r.fname()?) } else { None };
        h.component = Some(ComponentHeader { template_owner_class, template_name });
    }
    h.net_index = r.i32()?;
    if !o.is_class {
        h.properties = read_tagged_properties(r, names)?;
    }
    Ok(h)
}

pub fn write_object_header(w: &mut Writer, h: &ObjectHeader, is_class: bool, none: FName) {
    w.bytes(&h.native_prefix);
    if let Some(sf) = &h.state_frame {
        write_state_frame(w, sf);
    }
    if let Some(c) = &h.component {
        w.index(c.template_owner_class);
        if let Some(n) = c.template_name {
            w.fname(n);
        }
    }
    w.i32(h.net_index);
    if !is_class {
        write_tagged_properties(w, &h.properties, none);
    }
}

// --------------------------------------------------------------------------
// Fields
// --------------------------------------------------------------------------

/// `UStruct` payload (shared by functions, states, classes, script structs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StructData {
    pub super_struct: PackageIndex,
    pub script_text: PackageIndex,
    pub children: PackageIndex,
    pub cpp_text: PackageIndex,
    pub line: i32,
    pub text_pos: i32,
    /// In-memory bytecode size (object references count as 8 bytes).
    pub bytecode_size: i32,
    /// Bytecode as stored on disk (object references are 4-byte indices).
    pub script: Vec<u8>,
}

pub fn read_struct(r: &mut Reader) -> Result<StructData> {
    let super_struct = r.index()?;
    let script_text = r.index()?;
    let children = r.index()?;
    let cpp_text = r.index()?;
    let line = r.i32()?;
    let text_pos = r.i32()?;
    let bytecode_size = r.i32()?;
    let storage_size = r.i32()?;
    if storage_size < 0 {
        return Err(Error::Format(format!("negative script storage size {storage_size}")));
    }
    let script = r.take(storage_size as usize)?.to_vec();
    Ok(StructData { super_struct, script_text, children, cpp_text, line, text_pos, bytecode_size, script })
}

pub fn write_struct(w: &mut Writer, s: &StructData) {
    w.index(s.super_struct);
    w.index(s.script_text);
    w.index(s.children);
    w.index(s.cpp_text);
    w.i32(s.line);
    w.i32(s.text_pos);
    w.i32(s.bytecode_size);
    w.i32(s.script.len() as i32);
    w.bytes(&s.script);
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FunctionData {
    pub native_index: u16,
    pub oper_precedence: u8,
    pub function_flags: u32,
    pub rep_offset: Option<u16>,
    pub friendly_name: FName,
}

pub fn read_function(r: &mut Reader) -> Result<FunctionData> {
    let native_index = r.u16()?;
    let oper_precedence = r.u8()?;
    let function_flags = r.u32()?;
    let rep_offset = if function_flags & func::NET != 0 { Some(r.u16()?) } else { None };
    let friendly_name = r.fname()?;
    Ok(FunctionData { native_index, oper_precedence, function_flags, rep_offset, friendly_name })
}

pub fn write_function(w: &mut Writer, f: &FunctionData) {
    w.u16(f.native_index);
    w.u8(f.oper_precedence);
    w.u32(f.function_flags);
    if f.function_flags & func::NET != 0 {
        w.u16(f.rep_offset.unwrap_or(0));
    }
    w.fname(f.friendly_name);
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateData {
    pub probe_mask: u32,
    pub label_table_offset: u16,
    pub state_flags: u32,
    pub func_map: Vec<(FName, PackageIndex)>,
}

pub fn read_state(r: &mut Reader) -> Result<StateData> {
    let probe_mask = r.u32()?;
    let label_table_offset = r.u16()?;
    let state_flags = r.u32()?;
    let func_map = r.array(|r| Ok((r.fname()?, r.index()?)))?;
    Ok(StateData { probe_mask, label_table_offset, state_flags, func_map })
}

pub fn write_state(w: &mut Writer, s: &StateData) {
    w.u32(s.probe_mask);
    w.u16(s.label_table_offset);
    w.u32(s.state_flags);
    w.array(&s.func_map, |w, (n, i)| {
        w.fname(*n);
        w.index(*i);
    });
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassEditorData {
    pub dont_sort_categories: Vec<FName>,
    pub hide_categories: Vec<FName>,
    pub auto_expand_categories: Vec<FName>,
    pub auto_collapse_categories: Vec<FName>,
    pub force_script_order: u32,
    pub class_group_names: Vec<FName>,
    pub class_header_filename: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassData {
    pub class_flags: u32,
    pub class_within: PackageIndex,
    pub class_config_name: FName,
    pub component_name_to_default_object: Vec<(FName, PackageIndex)>,
    /// `(Class, PointerProperty)`
    pub interfaces: Vec<(PackageIndex, PackageIndex)>,
    pub editor: Option<ClassEditorData>,
    pub dll_bind_name: FName,
    pub class_default_object: PackageIndex,
}

pub fn read_class(r: &mut Reader, layout: Layout) -> Result<ClassData> {
    let class_flags = r.u32()?;
    let class_within = r.index()?;
    let class_config_name = r.fname()?;
    let component_name_to_default_object = r.array(|r| Ok((r.fname()?, r.index()?)))?;
    let interfaces = r.array(|r| Ok((r.index()?, r.index()?)))?;
    let editor = if layout.editor_data {
        Some(ClassEditorData {
            dont_sort_categories: r.array(|r| r.fname())?,
            hide_categories: r.array(|r| r.fname())?,
            auto_expand_categories: r.array(|r| r.fname())?,
            auto_collapse_categories: r.array(|r| r.fname())?,
            force_script_order: r.u32()?,
            class_group_names: r.array(|r| r.fname())?,
            class_header_filename: r.fstring()?,
        })
    } else {
        None
    };
    let dll_bind_name = r.fname()?;
    let class_default_object = r.index()?;
    Ok(ClassData {
        class_flags,
        class_within,
        class_config_name,
        component_name_to_default_object,
        interfaces,
        editor,
        dll_bind_name,
        class_default_object,
    })
}

pub fn write_class(w: &mut Writer, c: &ClassData) {
    w.u32(c.class_flags);
    w.index(c.class_within);
    w.fname(c.class_config_name);
    w.array(&c.component_name_to_default_object, |w, (n, i)| {
        w.fname(*n);
        w.index(*i);
    });
    w.array(&c.interfaces, |w, (a, b)| {
        w.index(*a);
        w.index(*b);
    });
    if let Some(e) = &c.editor {
        w.array(&e.dont_sort_categories, |w, n| w.fname(*n));
        w.array(&e.hide_categories, |w, n| w.fname(*n));
        w.array(&e.auto_expand_categories, |w, n| w.fname(*n));
        w.array(&e.auto_collapse_categories, |w, n| w.fname(*n));
        w.u32(e.force_script_order);
        w.array(&e.class_group_names, |w, n| w.fname(*n));
        w.fstring(&e.class_header_filename);
    }
    w.fname(c.dll_bind_name);
    w.index(c.class_default_object);
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptStructData {
    pub struct_flags: u32,
    pub defaults: Vec<TaggedProperty>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnumData {
    pub names: Vec<FName>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConstData {
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextBufferData {
    pub pos: i32,
    pub top: i32,
    pub text: String,
}

/// Concrete property class, by the class name used in the package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropertyKind {
    Byte,
    Int,
    Bool,
    Float,
    Object,
    Component,
    SafeObject,
    Class,
    Interface,
    Name,
    Str,
    Array,
    Map,
    Struct,
    Delegate,
}

impl PropertyKind {
    pub fn from_class_name(name: &str) -> Option<PropertyKind> {
        Some(match name {
            "ByteProperty" => PropertyKind::Byte,
            "IntProperty" => PropertyKind::Int,
            "BoolProperty" => PropertyKind::Bool,
            "FloatProperty" => PropertyKind::Float,
            "ObjectProperty" => PropertyKind::Object,
            "ComponentProperty" => PropertyKind::Component,
            "SafeObjectProperty" => PropertyKind::SafeObject,
            "ClassProperty" => PropertyKind::Class,
            "InterfaceProperty" => PropertyKind::Interface,
            "NameProperty" => PropertyKind::Name,
            "StrProperty" => PropertyKind::Str,
            "ArrayProperty" => PropertyKind::Array,
            "MapProperty" => PropertyKind::Map,
            "StructProperty" => PropertyKind::Struct,
            "DelegateProperty" => PropertyKind::Delegate,
            _ => return None,
        })
    }
    pub fn class_name(self) -> &'static str {
        match self {
            PropertyKind::Byte => "ByteProperty",
            PropertyKind::Int => "IntProperty",
            PropertyKind::Bool => "BoolProperty",
            PropertyKind::Float => "FloatProperty",
            PropertyKind::Object => "ObjectProperty",
            PropertyKind::Component => "ComponentProperty",
            PropertyKind::SafeObject => "SafeObjectProperty",
            PropertyKind::Class => "ClassProperty",
            PropertyKind::Interface => "InterfaceProperty",
            PropertyKind::Name => "NameProperty",
            PropertyKind::Str => "StrProperty",
            PropertyKind::Array => "ArrayProperty",
            PropertyKind::Map => "MapProperty",
            PropertyKind::Struct => "StructProperty",
            PropertyKind::Delegate => "DelegateProperty",
        }
    }
    /// The `FPropertyTag::Type` name (`UProperty::GetID()`), which is the
    /// class name for every kind.
    pub fn tag_type(self) -> &'static str {
        self.class_name()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PropertyData {
    pub array_dim: i32,
    pub property_flags: u64,
    pub category: Option<FName>,
    pub array_size_enum: Option<PackageIndex>,
    pub rep_offset: Option<u16>,
    /// `Enum` (byte), `PropertyClass` (object/component/safeobject/class),
    /// `InterfaceClass`, `Struct`, `Inner` (array), `Key` (map), `Function` (delegate).
    pub ref1: Option<PackageIndex>,
    /// `MetaClass` (class), `Value` (map), `SourceDelegate` (delegate).
    pub ref2: Option<PackageIndex>,
}

pub fn read_property(r: &mut Reader, kind: PropertyKind, layout: Layout) -> Result<PropertyData> {
    let array_dim = r.i32()?;
    let property_flags = r.u64()?;
    let (category, array_size_enum) =
        if layout.editor_data { (Some(r.fname()?), Some(r.index()?)) } else { (None, None) };
    let rep_offset = if property_flags & cpf::NET != 0 { Some(r.u16()?) } else { None };
    let (ref1, ref2) = match kind {
        PropertyKind::Byte
        | PropertyKind::Object
        | PropertyKind::Component
        | PropertyKind::SafeObject
        | PropertyKind::Interface
        | PropertyKind::Struct
        | PropertyKind::Array => (Some(r.index()?), None),
        PropertyKind::Class | PropertyKind::Map | PropertyKind::Delegate => {
            (Some(r.index()?), Some(r.index()?))
        }
        PropertyKind::Int | PropertyKind::Bool | PropertyKind::Float | PropertyKind::Name | PropertyKind::Str => {
            (None, None)
        }
    };
    Ok(PropertyData { array_dim, property_flags, category, array_size_enum, rep_offset, ref1, ref2 })
}

pub fn write_property(w: &mut Writer, p: &PropertyData, kind: PropertyKind, layout: Layout) {
    w.i32(p.array_dim);
    w.u64(p.property_flags);
    if layout.editor_data {
        w.fname(p.category.unwrap_or_default());
        w.index(p.array_size_enum.unwrap_or_default());
    }
    if p.property_flags & cpf::NET != 0 {
        w.u16(p.rep_offset.unwrap_or(0));
    }
    match kind {
        PropertyKind::Byte
        | PropertyKind::Object
        | PropertyKind::Component
        | PropertyKind::SafeObject
        | PropertyKind::Interface
        | PropertyKind::Struct
        | PropertyKind::Array => w.index(p.ref1.unwrap_or_default()),
        PropertyKind::Class | PropertyKind::Map | PropertyKind::Delegate => {
            w.index(p.ref1.unwrap_or_default());
            w.index(p.ref2.unwrap_or_default());
        }
        _ => {}
    }
}

// --------------------------------------------------------------------------
// Whole exports
// --------------------------------------------------------------------------

/// Everything we know how to interpret in an export payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectBody {
    Class { s: StructData, state: StateData, class: ClassData },
    State { s: StructData, state: StateData },
    Function { s: StructData, function: FunctionData },
    ScriptStruct { s: StructData, data: ScriptStructData },
    Enum(EnumData),
    Const(ConstData),
    Property { kind: PropertyKind, data: PropertyData },
    TextBuffer(TextBufferData),
    /// Any other object: header already parsed, rest kept verbatim.
    Other { rest: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedObject {
    pub header: ObjectHeader,
    /// `UField::Next` for field objects.
    pub next: Option<PackageIndex>,
    pub body: ObjectBody,
}

/// What the parser needs to know about the export being parsed.
#[derive(Clone, Copy, Debug)]
pub struct ExportInfo<'a> {
    pub class_name: &'a str,
    pub object_flags: u64,
    pub is_component: bool,
    pub native_prefix_len: usize,
}

pub fn parse_export(data: &[u8], names: &dyn NameTable, info: ExportInfo, layout: Layout) -> Result<ParsedObject> {
    let mut r = Reader::new(data);
    let cn = info.class_name;
    let is_class = cn == "Class";
    let header = read_object_header(
        &mut r,
        names,
        HeaderOptions {
            object_flags: info.object_flags,
            is_class,
            is_component: info.is_component,
            component_is_template: true,
            native_prefix_len: info.native_prefix_len,
        },
    )?;
    let field_kinds = ["Class", "State", "Function", "ScriptStruct", "Enum", "Const"];
    let prop_kind = PropertyKind::from_class_name(cn);
    let next = if field_kinds.contains(&cn) || prop_kind.is_some() { Some(r.index()?) } else { None };
    let body = match cn {
        "Class" => {
            let s = read_struct(&mut r)?;
            let state = read_state(&mut r)?;
            let class = read_class(&mut r, layout)?;
            ObjectBody::Class { s, state, class }
        }
        "State" => {
            let s = read_struct(&mut r)?;
            let state = read_state(&mut r)?;
            ObjectBody::State { s, state }
        }
        "Function" => {
            let s = read_struct(&mut r)?;
            let function = read_function(&mut r)?;
            ObjectBody::Function { s, function }
        }
        "ScriptStruct" => {
            let s = read_struct(&mut r)?;
            let struct_flags = r.u32()?;
            let defaults = read_tagged_properties(&mut r, names)?;
            ObjectBody::ScriptStruct { s, data: ScriptStructData { struct_flags, defaults } }
        }
        "Enum" => ObjectBody::Enum(EnumData { names: r.array(|r| r.fname())? }),
        "Const" => ObjectBody::Const(ConstData { value: r.fstring()? }),
        "TextBuffer" => {
            ObjectBody::TextBuffer(TextBufferData { pos: r.i32()?, top: r.i32()?, text: r.fstring()? })
        }
        _ => {
            if let Some(kind) = prop_kind {
                ObjectBody::Property { kind, data: read_property(&mut r, kind, layout)? }
            } else {
                let rest = r.take(r.remaining())?.to_vec();
                ObjectBody::Other { rest }
            }
        }
    };
    if r.remaining() != 0 {
        return Err(Error::Format(format!(
            "{} bytes left over after parsing {} ({} of {})",
            r.remaining(),
            cn,
            r.pos(),
            data.len()
        )));
    }
    Ok(ParsedObject { header, next, body })
}

pub fn write_export(obj: &ParsedObject, layout: Layout, none: FName) -> Vec<u8> {
    let mut w = Writer::new();
    let is_class = matches!(obj.body, ObjectBody::Class { .. });
    write_object_header(&mut w, &obj.header, is_class, none);
    if let Some(n) = obj.next {
        w.index(n);
    }
    match &obj.body {
        ObjectBody::Class { s, state, class } => {
            write_struct(&mut w, s);
            write_state(&mut w, state);
            write_class(&mut w, class);
        }
        ObjectBody::State { s, state } => {
            write_struct(&mut w, s);
            write_state(&mut w, state);
        }
        ObjectBody::Function { s, function } => {
            write_struct(&mut w, s);
            write_function(&mut w, function);
        }
        ObjectBody::ScriptStruct { s, data } => {
            write_struct(&mut w, s);
            w.u32(data.struct_flags);
            write_tagged_properties(&mut w, &data.defaults, none);
        }
        ObjectBody::Enum(e) => w.array(&e.names, |w, n| w.fname(*n)),
        ObjectBody::Const(c) => w.fstring(&c.value),
        ObjectBody::Property { kind, data } => write_property(&mut w, data, *kind, layout),
        ObjectBody::TextBuffer(t) => {
            w.i32(t.pos);
            w.i32(t.top);
            w.fstring(&t.text);
        }
        ObjectBody::Other { rest } => w.bytes(rest),
    }
    w.into_inner()
}

/// Decide whether a package carries editor-only field data by trying to
/// parse the first property export both ways.
pub fn probe_layout(pkg: &tempest_upk::Package) -> Layout {
    for (i, e) in pkg.exports.iter().enumerate() {
        let cn = pkg.class_name_of(PackageIndex::export(i));
        if PropertyKind::from_class_name(&cn).is_none() {
            continue;
        }
        let info = ExportInfo { class_name: &cn, object_flags: e.object_flags, is_component: false, native_prefix_len: 0 };
        let with = parse_export(&pkg.export_data[i], pkg, info, Layout { editor_data: true }).is_ok();
        let without = parse_export(&pkg.export_data[i], pkg, info, Layout { editor_data: false }).is_ok();
        if with && !without {
            return Layout { editor_data: true };
        }
        if without && !with {
            return Layout { editor_data: false };
        }
    }
    Layout::default()
}
