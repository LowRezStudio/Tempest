//! Object graph produced by the compiler: classes and their fields, with
//! every serialised attribute, plus references to objects in already-loaded
//! packages.

use tempest_upk::io::PackageIndex;

use crate::context::ObjRef;
use crate::objects::PropertyKind;

pub type NodeId = usize;

/// A field reference: either a node of the package being compiled or an
/// export of a loaded package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldRef {
    Local(NodeId),
    External(ObjRef),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Class,
    State,
    Function,
    ScriptStruct,
    Enum,
    Const,
    Property(PropertyKind),
    /// Class default object / sub-object (defaults carrier).
    Object,
}

#[derive(Clone, Debug, Default)]
pub struct StructInfo {
    pub super_struct: Option<FieldRef>,
    /// Children in final linked-list order.
    pub children: Vec<NodeId>,
    /// Disk-form bytecode and in-memory size (filled by `finalize`).
    pub script: Vec<u8>,
    pub mem_size: u32,
    /// Memory-form bytecode being written (8-byte reference handles).
    pub script_mem: Vec<u8>,
    pub line: i32,
    pub text_pos: i32,
    /// `structcpptext` / class cpptext.
    pub cpp_text: Option<String>,
    /// `UScriptStruct::StructFlags`.
    pub struct_flags: u32,
    /// `UStruct::ConstructorLink != NULL` after linking.
    pub has_ctor_link: bool,
    pub linked: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FunctionInfo {
    pub flags: u32,
    pub native_index: u16,
    pub precedence: u8,
    pub rep_offset: u16,
    pub friendly_name: String,
    pub num_parms: i32,
    /// Source position of the body for pass 2.
    pub body_pos: Option<(usize, u32)>,
    pub defined: bool,
    /// Default-value source positions per optional parameter (node id, pos).
    pub param_defaults: Vec<(NodeId, usize, u32)>,
}

#[derive(Clone, Debug, Default)]
pub struct StateInfo {
    pub probe_mask: u32,
    pub label_table_offset: u16,
    pub state_flags: u32,
    pub func_map: Vec<(String, NodeId)>,
    pub body_pos: Option<(usize, u32)>,
    pub ignores: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ClassInfo {
    pub class_flags: u32,
    pub class_within: Option<FieldRef>,
    pub config_name: String,
    pub component_map: Vec<(String, Option<FieldRef>)>,
    /// `(InterfaceClass, PointerProperty)`.
    pub interfaces: Vec<(FieldRef, Option<NodeId>)>,
    pub hide_categories: Vec<String>,
    pub auto_expand_categories: Vec<String>,
    pub auto_collapse_categories: Vec<String>,
    pub dont_sort_categories: Vec<String>,
    pub class_group_names: Vec<String>,
    pub force_script_order: bool,
    pub header_filename: String,
    pub dll_bind_name: String,
    pub depends_on: Vec<String>,
    pub cdo: Option<NodeId>,
    pub default_props_text: Option<(String, u32)>,
    /// Source position after the class declaration (replication block pos etc.).
    pub replication_pos: Option<(usize, u32)>,
    pub source: Option<String>,
    pub source_file: String,
}

#[derive(Clone, Debug, Default)]
pub struct PropertyInfo {
    pub array_dim: i32,
    pub flags: u64,
    pub category: Option<String>,
    pub array_size_enum: Option<FieldRef>,
    pub rep_offset: u16,
    /// Enum / PropertyClass / InterfaceClass / Struct / Inner / Key / Function
    pub ref1: Option<FieldRef>,
    /// MetaClass / Value / SourceDelegate
    pub ref2: Option<FieldRef>,
    pub delegate_name: Option<String>,
    pub export_text: Option<String>,
    /// Memory layout (`UProperty::Offset`/`ElementSize`), computed by `link`.
    pub offset: i32,
    pub element_size: i32,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub name: String,
    pub outer: Option<NodeId>,
    pub object_flags: u64,
    pub strct: StructInfo,
    pub func: FunctionInfo,
    pub state: StateInfo,
    pub class: ClassInfo,
    pub prop: PropertyInfo,
    pub enum_names: Vec<String>,
    pub const_value: String,
    /// Class of a defaults object (CDO/sub-object).
    pub object_class: Option<FieldRef>,
    pub archetype: Option<FieldRef>,
    /// Tagged default properties (serialised form) of a defaults object.
    pub default_props: Vec<crate::objects::TaggedProperty>,
    /// Full property values of a defaults object or script-struct defaults (set by the defaults pass).
    pub values: Option<Vec<super::values::Slot>>,
    /// Raw payload for component objects: `TemplateOwnerClass`, `TemplateName`.
    pub component_template: Option<(Option<FieldRef>, Option<String>)>,
    /// `PropertiesSize`/`MinAlignment` of struct-like nodes.
    pub properties_size: i32,
    pub min_alignment: i32,
    /// Export index assigned when writing.
    pub export_index: Option<usize>,
    /// Intrinsic (C++-only) class stand-in: emitted as an import, never an export.
    pub intrinsic: bool,
    /// Package an intrinsic/phantom class is imported from (`None` → `Core`).
    pub intrinsic_package: Option<String>,
    /// Native (non-tagged) serialization tail of a sub-object, copied from its archetype.
    pub native_rest: Vec<u8>,
    pub line: u32,
}

impl Node {
    pub fn new(kind: NodeKind, name: &str, outer: Option<NodeId>) -> Node {
        Node {
            kind,
            name: name.to_string(),
            outer,
            object_flags: 0,
            strct: StructInfo::default(),
            func: FunctionInfo::default(),
            state: StateInfo::default(),
            class: ClassInfo::default(),
            prop: PropertyInfo::default(),
            enum_names: Vec::new(),
            const_value: String::new(),
            object_class: None,
            archetype: None,
            default_props: Vec::new(),
            values: None,
            component_template: None,
            properties_size: 0,
            min_alignment: 1,
            export_index: None,
            intrinsic: false,
            intrinsic_package: None,
            native_rest: Vec::new(),
            line: 0,
        }
    }
    pub fn is_struct_like(&self) -> bool {
        matches!(self.kind, NodeKind::Class | NodeKind::State | NodeKind::Function | NodeKind::ScriptStruct)
    }
    pub fn property_kind(&self) -> Option<PropertyKind> {
        match self.kind {
            NodeKind::Property(k) => Some(k),
            _ => None,
        }
    }
}

/// Everything being compiled for one package.
#[derive(Default)]
pub struct Model {
    pub package_name: String,
    pub nodes: Vec<Node>,
    pub classes: Vec<NodeId>,
    /// Stand-ins for intrinsic classes (`Class`, `Package`, property classes...).
    pub intrinsics: Vec<NodeId>,
}

impl Model {
    pub fn add(&mut self, node: Node) -> NodeId {
        self.nodes.push(node);
        self.nodes.len() - 1
    }
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id]
    }
    /// Dotted path inside the package (`Class.Func.Param`).
    pub fn path(&self, id: NodeId) -> String {
        let mut parts = vec![self.nodes[id].name.clone()];
        let mut cur = self.nodes[id].outer;
        while let Some(o) = cur {
            parts.push(self.nodes[o].name.clone());
            cur = self.nodes[o].outer;
        }
        parts.reverse();
        parts.join(".")
    }
    pub fn owner_class(&self, id: NodeId) -> Option<NodeId> {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if self.nodes[c].kind == NodeKind::Class {
                return Some(c);
            }
            cur = self.nodes[c].outer;
        }
        None
    }
    pub fn find_class(&self, name: &str) -> Option<NodeId> {
        self.classes.iter().chain(self.intrinsics.iter()).copied().find(|&c| self.nodes[c].name.eq_ignore_ascii_case(name))
    }
}

/// Serialisation-time helper: a package index assigned to a `FieldRef`.
#[derive(Default)]
pub struct IndexMap {
    pub locals: Vec<Option<PackageIndex>>,
}
