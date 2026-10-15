//! Core of the `FScriptCompiler` port: tokens, input handling with
//! backtracking, the nest stack with jump fixups, and the script writer.

use std::collections::HashMap;
use std::fmt;

use tempest_upk::flags::{func, state as sf};
use tempest_upk::package::split_name_number;

use super::lexer::{Lexer, Tok};
use super::model::{FieldRef, Model, NodeId, NodeKind};
use super::scope::{GlobalIndex, Resolver};
use super::types::{Cpt, PropType};
use crate::bytecode::op;
use crate::context::Context;

// ----- errors ----------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct CompileError {
    pub class: String,
    pub line: u32,
    pub message: String,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error in {}, Line {}: {}", self.class, self.line, self.message)
    }
}

impl std::error::Error for CompileError {}

pub type CResult<T> = Result<T, CompileError>;

// ----- tokens ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokKind {
    None,
    Identifier,
    Symbol,
    Const,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConstVal {
    None,
    Byte(u8),
    Int(i32),
    Bool(bool),
    Float(f32),
    Object(Option<FieldRef>),
    Name(String),
    Str(String),
    Vector(f32, f32, f32),
    Rotator(i32, i32, i32),
    Delegate,
}

/// `FToken`.
#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokKind,
    pub ident: String,
    pub start: usize,
    pub line: u32,
    pub ty: PropType,
    pub prop: Option<FieldRef>,
    pub func: Option<FieldRef>,
    pub value: ConstVal,
}

impl Token {
    pub fn empty() -> Token {
        Token {
            kind: TokKind::None,
            ident: String::new(),
            start: 0,
            line: 0,
            ty: PropType::none(),
            prop: None,
            func: None,
            value: ConstVal::None,
        }
    }
    pub fn from_type(t: PropType) -> Token {
        let mut tok = Token::empty();
        tok.ty = t;
        tok
    }
    pub fn matches(&self, s: &str) -> bool {
        matches!(self.kind, TokKind::Identifier | TokKind::Symbol) && self.ident.eq_ignore_ascii_case(s)
    }
    pub fn matches_ident(&self, s: &str) -> bool {
        self.kind == TokKind::Identifier && self.ident.eq_ignore_ascii_case(s)
    }
    pub fn is_symbol(&self, s: &str) -> bool {
        self.kind == TokKind::Symbol && self.ident == s
    }
    pub fn set_const(&mut self, ty: PropType, v: ConstVal) {
        self.ty = ty;
        self.value = v;
        self.kind = TokKind::Const;
    }
    pub fn set_const_byte(&mut self, e: Option<FieldRef>, b: u8) {
        self.set_const(PropType::enum_byte(e), ConstVal::Byte(b));
    }
    pub fn set_const_int(&mut self, i: i32) {
        self.set_const(PropType::new(Cpt::Int), ConstVal::Int(i));
    }
    pub fn set_const_bool(&mut self, b: bool) {
        self.set_const(PropType::new(Cpt::Bool), ConstVal::Bool(b));
    }
    pub fn set_const_float(&mut self, f: f32) {
        self.set_const(PropType::new(Cpt::Float), ConstVal::Float(f));
    }
    pub fn set_const_name(&mut self, n: &str) {
        self.set_const(PropType::new(Cpt::Name), ConstVal::Name(n.to_string()));
    }
    pub fn set_const_string(&mut self, s: &str) {
        self.set_const(PropType::new(Cpt::String), ConstVal::Str(s.to_string()));
    }
    pub fn set_const_delegate(&mut self) {
        self.set_const(PropType::new(Cpt::Delegate), ConstVal::Delegate);
    }
    pub fn set_const_vector(&mut self, v: (f32, f32, f32), vector_struct: FieldRef) {
        self.set_const(PropType::strct(vector_struct), ConstVal::Vector(v.0, v.1, v.2));
    }
    pub fn set_const_rotator(&mut self, r: (i32, i32, i32), rotator_struct: FieldRef) {
        self.set_const(PropType::strct(rotator_struct), ConstVal::Rotator(r.0, r.1, r.2));
    }
    pub fn get_const_byte(&self) -> Option<u8> {
        if self.kind != TokKind::Const {
            return None;
        }
        match self.value {
            ConstVal::Byte(b) => Some(b),
            ConstVal::Int(i) if (0..256).contains(&i) => Some(i as u8),
            ConstVal::Float(f) if f >= 0.0 && f < 255.0 && f == f.trunc() => Some(f as u8),
            _ => None,
        }
    }
    pub fn get_const_int(&self) -> Option<i32> {
        if self.kind != TokKind::Const {
            return None;
        }
        match self.value {
            ConstVal::Int(i) => Some(i),
            ConstVal::Byte(b) => Some(b as i32),
            ConstVal::Float(f) if f == f.trunc() => Some(f as i32),
            _ => None,
        }
    }
    pub fn get_const_bool(&self) -> Option<bool> {
        match (self.kind, &self.value) {
            (TokKind::Const, ConstVal::Bool(b)) => Some(*b),
            _ => None,
        }
    }
    pub fn get_const_float(&self) -> Option<f32> {
        if self.kind != TokKind::Const {
            return None;
        }
        match self.value {
            ConstVal::Float(f) => Some(f),
            ConstVal::Int(i) => Some(i as f32),
            ConstVal::Byte(b) => Some(b as f32),
            _ => None,
        }
    }
    pub fn get_const_name(&self) -> Option<String> {
        match (self.kind, &self.value) {
            (TokKind::Const, ConstVal::Name(n)) => Some(n.clone()),
            _ => None,
        }
    }
}

// ----- nests -------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NestType {
    None,
    Class,
    State,
    Function,
    Interface,
    If,
    Loop,
    Switch,
    For,
    ForEach,
    FilterEditorOnly,
}

pub mod allow {
    pub const STATE_CMD: u32 = 0x1;
    pub const CMD: u32 = 0x2;
    pub const FUNCTION: u32 = 0x4;
    pub const STATE: u32 = 0x8;
    pub const ELSE_IF: u32 = 0x10;
    pub const VAR_DECL: u32 = 0x40;
    pub const CLASS: u32 = 0x80;
    pub const CASE: u32 = 0x100;
    pub const DEFAULT: u32 = 0x200;
    pub const RETURN: u32 = 0x400;
    pub const BREAK: u32 = 0x800;
    pub const LABEL: u32 = 0x2000;
    pub const IGNORES: u32 = 0x4000;
    pub const INSTANCE: u32 = 0x8000;
    pub const IN_WHILE: u32 = 0x10000;
    pub const CONTINUE: u32 = 0x20000;
    pub const ITERATOR: u32 = 0x40000;
    pub const TYPE_DECL: u32 = 0x100000;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixup {
    SwitchEnd = 0,
    IfEnd = 1,
    LoopStart = 2,
    LoopEnd = 3,
    LoopPostCond = 4,
    ForStart = 5,
    ForEnd = 6,
    Label = 7,
    IteratorEnd = 8,
    ForInc = 9,
    IteratorSkip = 10,
    FilterEditorOnly = 11,
}

pub const FIXUP_MAX: usize = 12;
pub const MAXWORD: u32 = 0xFFFF;
pub const NESTF_RETURN_VALUE_FOUND: u32 = 1;

#[derive(Clone, Debug)]
pub struct Placeholder {
    pub fixup: Fixup,
    pub icode: u32,
    pub name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NestInfo {
    pub node: Option<NodeId>,
    pub nest_type: NestType,
    pub allow: u32,
    pub nest_flags: u32,
    pub jump_targets: [u32; FIXUP_MAX],
    /// Most recent first (a linked list in UE3).
    pub placeholders: Vec<Placeholder>,
    /// Labels, most recent first.
    pub labels: Vec<(String, u32)>,
    pub struct_mod_bytes: Vec<u32>,
    pub chain_placeholder: Option<u32>,
    pub switch_type: Token,
    pub for_retry: Option<Location>,
}

impl NestInfo {
    pub fn new(nest_type: NestType, node: Option<NodeId>) -> NestInfo {
        NestInfo {
            node,
            nest_type,
            allow: 0,
            nest_flags: 0,
            jump_targets: [MAXWORD; FIXUP_MAX],
            placeholders: Vec::new(),
            labels: Vec::new(),
            struct_mod_bytes: Vec::new(),
            chain_placeholder: None,
            switch_type: Token::empty(),
            for_retry: None,
        }
    }
    pub fn set_jump_target(&mut self, f: Fixup, icode: u32) {
        self.jump_targets[f as usize] = icode;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Location {
    pub pos: usize,
    pub line: u32,
    pub code_top: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Parse,
    Compile,
}

// ----- the compiler ------------------------------------------------------------

pub struct Compiler<'a> {
    pub ctx: &'a mut Context,
    pub global: GlobalIndex,
    pub model: Model,
    /// Script text as bytes: one byte per `TCHAR` so that positions match the engine's.
    pub input: Vec<u8>,
    pub lexer_pos: usize,
    pub lexer_line: u32,
    pub prev_pos: usize,
    pub prev_line: u32,
    pub class: NodeId,
    pub pass: Pass,
    pub nests: Vec<NestInfo>,
    pub top_node: Option<NodeId>,
    pub got_affector: bool,
    pub affector_return_property: Option<FieldRef>,
    pub supress_debug_info: i32,
    pub warnings: Vec<String>,
    /// Object references stored in bytecode as 8-byte handles (index + 1).
    pub ref_table: Vec<FieldRef>,
    ref_lookup: HashMap<FieldRef, u32>,
    /// Compile-time name table (case-insensitive, first spelling wins).
    pub names: Vec<String>,
    name_lookup: HashMap<String, u32>,
    pub pointer_size: u32,
    pub vector_struct: Option<FieldRef>,
    pub rotator_struct: Option<FieldRef>,
    /// Per-class source text for pass 2.
    pub sources: HashMap<NodeId, String>,
    pub in_default_prop_context: bool,
    pub original_variable_index: i32,
    pub func_emit_count: HashMap<FieldRef, i32>,
    /// Set by `pop_nest_compile` when an `else` continues the `if` nest.
    pub else_continued: bool,
    /// Default-property value caches.
    pub values: super::values::ValueCache,
    /// Treat the output as cooked (affects immutable-when-cooked structs).
    pub cooked: bool,
    /// Let defaultproperties set config/localized values (as a cooked package would hold them).
    pub allow_config_defaults: bool,
    /// `RemovedComponents` of the class defaults being imported (`Components.Remove`).
    pub removed_components: Vec<FieldRef>,
    /// `Ar.IsSerializingDefaults()` for the binary struct values being encoded/decoded.
    pub bin_defaults_ctx: bool,
    /// Lower-cased object path → (class name, native payload) of every loaded export with native data.
    pub native_rest_index: Option<std::collections::HashMap<String, (String, Vec<u8>)>>,
}

impl<'a> Compiler<'a> {
    pub fn new(ctx: &'a mut Context, package_name: &str, seed_names: &[String]) -> Compiler<'a> {
        let global = GlobalIndex::build(ctx);
        let mut c = Compiler {
            ctx,
            global,
            model: Model { package_name: package_name.to_string(), ..Default::default() },
            input: Vec::new(),
            lexer_pos: 0,
            lexer_line: 1,
            prev_pos: 0,
            prev_line: 1,
            class: 0,
            pass: Pass::Parse,
            nests: Vec::new(),
            top_node: None,
            got_affector: false,
            affector_return_property: None,
            supress_debug_info: 0,
            warnings: Vec::new(),
            ref_table: Vec::new(),
            ref_lookup: HashMap::new(),
            names: Vec::new(),
            name_lookup: HashMap::new(),
            pointer_size: 8,
            vector_struct: None,
            rotator_struct: None,
            sources: HashMap::new(),
            in_default_prop_context: false,
            original_variable_index: 0,
            func_emit_count: HashMap::new(),
            else_continued: false,
            values: super::values::ValueCache::default(),
            cooked: false,
            allow_config_defaults: false,
            removed_components: Vec::new(),
            bin_defaults_ctx: false,
            native_rest_index: None,
        };
        for n in seed_names {
            c.intern_name_exact(n);
        }
        c.intern_name("None");
        c.add_intrinsic_classes();
        c.add_phantom_classes();
        c.vector_struct = c.res().find_struct_any("Vector");
        c.rotator_struct = c.res().find_struct_any("Rotator");
        c
    }

    /// Stand-in nodes for the intrinsic C++ classes that packages never export.
    fn add_intrinsic_classes(&mut self) {
        let object = self.res().find_class("Object");
        let tree: &[(&str, &str)] = &[
            ("Field", "Object"), ("Struct", "Field"), ("State", "Struct"), ("Class", "State"), ("Function", "Struct"), ("ScriptStruct", "Struct"),
            ("Enum", "Field"), ("Const", "Field"), ("Property", "Field"), ("ByteProperty", "Property"), ("IntProperty", "Property"),
            ("BoolProperty", "Property"), ("FloatProperty", "Property"), ("ObjectProperty", "Property"), ("ComponentProperty", "ObjectProperty"),
            ("ClassProperty", "ObjectProperty"), ("SafeObjectProperty", "ObjectProperty"), ("InterfaceProperty", "Property"), ("NameProperty", "Property"),
            ("StrProperty", "Property"), ("ArrayProperty", "Property"), ("MapProperty", "Property"), ("StructProperty", "Property"),
            ("DelegateProperty", "Property"), ("Package", "Object"), ("TextBuffer", "Object"), ("ObjectRedirector", "Object"), ("MetaData", "Object"),
            ("Linker", "Object"), ("LinkerLoad", "Linker"), ("LinkerSave", "Linker"), ("System", "Subsystem"),
        ];
        for (name, sup) in tree {
            if self.res().find_class(name).is_some() {
                continue;
            }
            let super_ref = self.res().find_class(sup).or(object);
            let mut n = super::model::Node::new(NodeKind::Class, name, None);
            n.object_flags = tempest_upk::flags::rf::PUBLIC | tempest_upk::flags::rf::STANDALONE | tempest_upk::flags::rf::NATIVE;
            n.class.class_flags = tempest_upk::flags::class::NATIVE | tempest_upk::flags::class::INTRINSIC;
            n.class.class_within = object;
            n.strct.super_struct = super_ref;
            n.intrinsic = true;
            let id = self.model.add(n);
            self.model.intrinsics.push(id);
        }
    }

    /// Classes that loaded packages import but that no loaded package exports
    /// (cooked packages drop script-less native classes such as `Engine.StaticMesh`).
    /// They get stand-in nodes so references to them compile to the same imports.
    fn add_phantom_classes(&mut self) {
        let object = self.res().find_class("Object");
        let mut found: Vec<(String, String)> = Vec::new();
        for lp in self.ctx.packages.iter() {
            if lp.shadow {
                continue;
            }
            let p = &lp.pkg;
            for imp in &p.imports {
                if p.fname_to_string(imp.class_name) != "Class" {
                    continue;
                }
                let Some(oi) = imp.outer.import_index() else { continue };
                let Some(outer) = p.imports.get(oi) else { continue };
                if p.fname_to_string(outer.class_name) != "Package" {
                    continue;
                }
                let name = p.fname_to_string(imp.object_name);
                let pkg = p.fname_to_string(outer.object_name);
                if !found.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                    found.push((name, pkg));
                }
            }
        }
        self.add_phantom_classes_from(&found);
    }

    /// Stand-in members (properties, structs) of phantom classes, from an import
    /// table: `(import class name, path segments starting with the package)`.
    /// Object-typed members get the `__Unknown` phantom class, compatible with any class.
    pub fn add_phantom_members(&mut self, imports: &[(String, Vec<String>)]) {
        use crate::objects::PropertyKind;
        let mut sorted: Vec<&(String, Vec<String>)> = imports.iter().filter(|(_, p)| p.len() >= 3).collect();
        sorted.sort_by_key(|(_, p)| p.len());
        let mut unknown: Option<FieldRef> = None;
        let mut created: Vec<NodeId> = Vec::new();
        for (class_name, path) in sorted {
            // Outer: a phantom class (depth 3) or a phantom member (deeper).
            let Some(outer) = self.phantom_by_path(&path[..path.len() - 1]) else { continue };
            let name = path[path.len() - 1].clone();
            if self.model.node(outer).strct.children.iter().any(|&c| self.model.node(c).name.eq_ignore_ascii_case(&name)) {
                continue;
            }
            let kind = match class_name.as_str() {
                "ScriptStruct" => NodeKind::ScriptStruct,
                "Function" => NodeKind::Function,
                "Enum" => NodeKind::Enum,
                other => match PropertyKind::from_class_name(other) {
                    Some(k) => NodeKind::Property(k),
                    None => continue,
                },
            };
            let mut n = super::model::Node::new(kind.clone(), &name, Some(outer));
            n.object_flags = tempest_upk::flags::rf::PUBLIC;
            n.intrinsic = true;
            n.prop.array_dim = 1;
            if let NodeKind::Property(PropertyKind::Object | PropertyKind::Component | PropertyKind::SafeObject | PropertyKind::Interface) = kind {
                if unknown.is_none() {
                    let object = self.res().find_class("Object");
                    let mut u = super::model::Node::new(NodeKind::Class, "__Unknown", None);
                    u.class.class_within = object;
                    u.strct.super_struct = object;
                    u.intrinsic = true;
                    u.intrinsic_package = Some("Core".into());
                    let uid = self.model.add(u);
                    self.model.intrinsics.push(uid);
                    unknown = Some(FieldRef::Local(uid));
                }
                n.prop.ref1 = unknown;
            }
            let id = self.model.add(n);
            created.push(id);
            // Array inners hang off the array property; everything else is a child.
            if let NodeKind::Property(PropertyKind::Array) = self.model.node(outer).kind {
                self.model.node_mut(outer).prop.ref1 = Some(FieldRef::Local(id));
            } else {
                self.model.node_mut(outer).strct.children.push(id);
            }
        }
        // Arrays whose element type was never imported: assume objects of unknown class.
        for id in created {
            let is_array = matches!(self.model.node(id).kind, NodeKind::Property(PropertyKind::Array));
            if is_array && self.model.node(id).prop.ref1.is_none() {
                if unknown.is_none() {
                    let object = self.res().find_class("Object");
                    let mut u = super::model::Node::new(NodeKind::Class, "__Unknown", None);
                    u.class.class_within = object;
                    u.strct.super_struct = object;
                    u.intrinsic = true;
                    u.intrinsic_package = Some("Core".into());
                    let uid = self.model.add(u);
                    self.model.intrinsics.push(uid);
                    unknown = Some(FieldRef::Local(uid));
                }
                let name = self.model.node(id).name.clone();
                let mut inner = super::model::Node::new(NodeKind::Property(PropertyKind::Object), &name, Some(id));
                inner.object_flags = tempest_upk::flags::rf::PUBLIC;
                inner.intrinsic = true;
                inner.prop.array_dim = 1;
                inner.prop.ref1 = unknown;
                let iid = self.model.add(inner);
                self.model.node_mut(id).prop.ref1 = Some(FieldRef::Local(iid));
            }
        }
    }

    fn phantom_by_path(&self, path: &[String]) -> Option<NodeId> {
        // path[0] is the package, path[1] the class.
        let class = self.model.intrinsics.iter().copied().find(|&n| {
            let node = self.model.node(n);
            node.name.eq_ignore_ascii_case(&path[1]) && node.intrinsic_package.as_deref().map(|p| p.eq_ignore_ascii_case(&path[0])).unwrap_or(false)
        })?;
        let mut cur = class;
        for seg in &path[2..] {
            let node = self.model.node(cur);
            let next = if let NodeKind::Property(crate::objects::PropertyKind::Array) = node.kind {
                match node.prop.ref1 {
                    Some(FieldRef::Local(i)) if self.model.node(i).name.eq_ignore_ascii_case(seg) => Some(i),
                    _ => None,
                }
            } else {
                node.strct.children.iter().copied().find(|&c| self.model.node(c).name.eq_ignore_ascii_case(seg))
            };
            cur = next?;
        }
        Some(cur)
    }

    /// Stand-in class nodes for `(name, package)` pairs that resolve to no loaded class.
    pub fn add_phantom_classes_from(&mut self, list: &[(String, String)]) {
        let object = self.res().find_class("Object");
        for (name, pkg) in list {
            if self.res().find_class(name).is_some() {
                continue;
            }
            let mut n = super::model::Node::new(NodeKind::Class, name, None);
            n.object_flags = tempest_upk::flags::rf::PUBLIC | tempest_upk::flags::rf::STANDALONE | tempest_upk::flags::rf::NATIVE;
            n.class.class_flags = tempest_upk::flags::class::NATIVE;
            n.class.class_within = object;
            n.strct.super_struct = object;
            n.intrinsic = true;
            n.intrinsic_package = Some(pkg.clone());
            let id = self.model.add(n);
            self.model.intrinsics.push(id);
        }
    }

    pub fn res(&self) -> Resolver<'_> {
        Resolver::new(self.ctx, &self.model, &self.global)
    }

    pub fn class_name(&self) -> String {
        self.model.nodes.get(self.class).map(|n| n.name.clone()).unwrap_or_default()
    }

    pub fn error<T>(&self, msg: impl Into<String>) -> CResult<T> {
        Err(CompileError { class: self.class_name(), line: self.lexer_line, message: msg.into() })
    }
    pub fn err(&self, msg: impl Into<String>) -> CompileError {
        CompileError { class: self.class_name(), line: self.lexer_line, message: msg.into() }
    }
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(format!("{}({}): {}", self.class_name(), self.lexer_line, msg.into()));
    }

    // ----- names / refs -----------------------------------------------------

    /// Intern a name with exactly this spelling (a reference package's own table).
    pub fn intern_name_exact(&mut self, s: &str) -> u32 {
        let key = s.to_ascii_lowercase();
        if let Some(&i) = self.name_lookup.get(&key) {
            return i;
        }
        let i = self.names.len() as u32;
        self.names.push(s.to_string());
        self.name_lookup.insert(key, i);
        i
    }

    pub fn intern_name(&mut self, s: &str) -> u32 {
        let mapped = cp1252_string(s);
        let s = mapped.as_str();
        let key = s.to_ascii_lowercase();
        if let Some(&i) = self.name_lookup.get(&key) {
            return i;
        }
        // FNames are case-insensitive and keep the first spelling seen; the
        // engine has every loaded package's names registered already.
        let mut spelling = s.to_string();
        for lp in &self.ctx.packages {
            if let Some(idx) = lp.pkg.find_name(s) {
                spelling = lp.pkg.name_str(idx).to_string();
                break;
            }
        }
        let i = self.names.len() as u32;
        self.names.push(spelling);
        self.name_lookup.insert(key, i);
        i
    }

    /// `FName(text)` with the `Name_N` splitting rule → (index, number).
    pub fn fname(&mut self, text: &str) -> (u32, u32) {
        let text = if text.is_empty() { "None" } else { text };
        let (base, number) = split_name_number(text);
        let base = base.to_string();
        (self.intern_name(&base), number)
    }

    pub fn ref_handle(&mut self, r: Option<FieldRef>) -> u64 {
        match r {
            None => 0,
            Some(r) => {
                if let Some(&h) = self.ref_lookup.get(&r) {
                    return h as u64;
                }
                self.ref_table.push(r);
                let h = self.ref_table.len() as u32;
                self.ref_lookup.insert(r, h);
                h as u64
            }
        }
    }

    pub fn handle_ref(&self, h: u64) -> Option<FieldRef> {
        if h == 0 {
            None
        } else {
            self.ref_table.get(h as usize - 1).copied()
        }
    }

    // ----- input ----------------------------------------------------------

    fn lexer(&self) -> Lexer<'_> {
        let mut l = Lexer::new_bytes(&self.input);
        l.pos = self.lexer_pos;
        l.line = self.lexer_line;
        l
    }

    pub fn set_input(&mut self, text: Vec<u8>) {
        self.input = text;
        self.lexer_pos = 0;
        self.lexer_line = 1;
        self.prev_pos = 0;
        self.prev_line = 1;
    }

    pub fn location(&self) -> Location {
        Location { pos: self.lexer_pos, line: self.lexer_line, code_top: self.code_len() }
    }

    pub fn return_to(&mut self, loc: Location, binary: bool, text: bool) {
        if text {
            self.lexer_pos = loc.pos;
            self.lexer_line = loc.line;
        }
        if binary {
            if let Some(n) = self.top_node {
                let s = &mut self.model.nodes[n].strct.script_mem;
                s.truncate(loc.code_top as usize);
            }
        }
    }

    /// Raw `GetToken`: identifiers, symbols and literal constants.
    pub fn get_raw(&mut self, no_consts: bool) -> CResult<Option<Token>> {
        let (t, npos, nline) = {
            let mut lx = self.lexer();
            let t = lx.next(no_consts);
            (t, lx.pos, lx.line)
        };
        let t = t.map_err(|e| CompileError { class: self.class_name(), line: e.line, message: e.message })?;
        self.prev_pos = self.lexer_pos;
        self.prev_line = self.lexer_line;
        self.lexer_pos = npos;
        self.lexer_line = nline;
        let mut tok = Token::empty();
        tok.start = t.start;
        tok.line = t.line;
        match t.tok {
            Tok::Eof => {
                self.lexer_pos = t.start;
                return Ok(None);
            }
            Tok::Ident(s) => {
                tok.kind = TokKind::Identifier;
                tok.ident = s;
            }
            Tok::Symbol(s) => {
                tok.kind = TokKind::Symbol;
                tok.ident = s;
            }
            Tok::Int(v) => {
                tok.ident = v.to_string();
                tok.set_const_int(v);
            }
            Tok::Float(v) => {
                tok.ident = v.to_string();
                tok.set_const_float(v);
            }
            Tok::Str(s) => {
                tok.ident = s.clone();
                tok.set_const_string(&s);
            }
            Tok::Name(s) => {
                tok.ident = s.clone();
                tok.set_const_name(&s);
            }
        }
        Ok(Some(tok))
    }

    pub fn unget(&mut self, t: &Token) {
        self.lexer_pos = t.start;
        self.lexer_line = t.line;
    }

    pub fn peek_raw(&mut self, no_consts: bool) -> CResult<Option<Token>> {
        let save = (self.lexer_pos, self.lexer_line, self.prev_pos, self.prev_line);
        let t = self.get_raw(no_consts)?;
        self.lexer_pos = save.0;
        self.lexer_line = save.1;
        self.prev_pos = save.2;
        self.prev_line = save.3;
        Ok(t)
    }

    pub fn match_symbol(&mut self, s: &str) -> CResult<bool> {
        match self.get_raw(true)? {
            Some(t) if t.is_symbol(s) => Ok(true),
            Some(t) => {
                self.unget(&t);
                Ok(false)
            }
            None => Ok(false),
        }
    }

    pub fn peek_symbol(&mut self, s: &str) -> CResult<bool> {
        Ok(self.peek_raw(true)?.map(|t| t.is_symbol(s)).unwrap_or(false))
    }

    pub fn match_identifier(&mut self, s: &str) -> CResult<bool> {
        match self.get_raw(false)? {
            Some(t) if t.matches_ident(s) => Ok(true),
            Some(t) => {
                self.unget(&t);
                Ok(false)
            }
            None => Ok(false),
        }
    }

    pub fn peek_identifier(&mut self, s: &str) -> CResult<bool> {
        Ok(self.peek_raw(true)?.map(|t| t.matches_ident(s)).unwrap_or(false))
    }

    pub fn require_symbol(&mut self, s: &str, tag: &str) -> CResult<()> {
        if !self.match_symbol(s)? {
            return self.error(format!("Missing '{s}' in {tag}"));
        }
        Ok(())
    }

    pub fn require_identifier(&mut self, s: &str, tag: &str) -> CResult<()> {
        if !self.match_identifier(s)? {
            return self.error(format!("Missing '{s}' in {tag}"));
        }
        Ok(())
    }

    /// `GetIdentifier`.
    pub fn get_identifier(&mut self, no_consts: bool) -> CResult<Option<Token>> {
        match self.get_token_hint(None, no_consts)? {
            Some(t) if t.kind == TokKind::Identifier => Ok(Some(t)),
            Some(t) => {
                self.unget(&t);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    pub fn get_symbol_token(&mut self) -> CResult<Option<Token>> {
        match self.get_token_hint(None, false)? {
            Some(t) if t.kind == TokKind::Symbol => Ok(Some(t)),
            Some(t) => {
                self.unget(&t);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    /// Raw text up to a stop character (`GetRawToken`).
    pub fn get_raw_text(&mut self, stop: u8) -> String {
        let (s, npos, nline) = {
            let mut lx = self.lexer();
            let s = lx.raw_until(stop);
            (s, lx.pos, lx.line)
        };
        self.lexer_pos = npos;
        self.lexer_line = nline;
        s
    }

    pub fn raw_block(&mut self) -> CResult<String> {
        let (s, npos, nline) = {
            let mut lx = self.lexer();
            let s = lx.raw_block();
            (s, lx.pos, lx.line)
        };
        let s = s.map_err(|e| CompileError { class: self.class_name(), line: e.line, message: e.message })?;
        self.lexer_pos = npos;
        self.lexer_line = nline;
        Ok(s)
    }

    /// `GetConstInt` (with optional const lookup in `scope`).
    pub fn get_const_int(&mut self, scope: Option<FieldRef>) -> CResult<Option<i32>> {
        let Some(t) = self.get_token_hint(None, false)? else { return Ok(None) };
        if let Some(v) = t.get_const_int() {
            return Ok(Some(v));
        }
        if let Some(sc) = scope {
            if t.kind == TokKind::Identifier {
                if let Some(f) = self.res().find_field(Some(sc), &t.ident, true, Some(&mut 0)) {
                    if self.res().kind_of(f) == super::scope::FieldKind::Const {
                        let v = self.res().const_value(f);
                        if let Some(ct) = self.parse_const_text(&v)? {
                            if let Some(i) = ct.get_const_int() {
                                return Ok(Some(i));
                            }
                        }
                    }
                }
            }
        }
        self.unget(&t);
        Ok(None)
    }

    pub fn get_const_float(&mut self) -> CResult<Option<f32>> {
        let Some(t) = self.get_token_hint(None, false)? else { return Ok(None) };
        if let Some(v) = t.get_const_float() {
            return Ok(Some(v));
        }
        self.unget(&t);
        Ok(None)
    }

    /// Tokenise a const's value text in isolation.
    pub fn parse_const_text(&mut self, text: &str) -> CResult<Option<Token>> {
        let save_input = std::mem::take(&mut self.input);
        let save = (self.lexer_pos, self.lexer_line, self.prev_pos, self.prev_line);
        self.input = latin1_bytes(text);
        self.lexer_pos = 0;
        self.lexer_line = save.1;
        let r = self.get_token_hint(None, false);
        self.input = save_input;
        self.lexer_pos = save.0;
        self.lexer_line = save.1;
        self.prev_pos = save.2;
        self.prev_line = save.3;
        r
    }

    // ----- code emission ----------------------------------------------------

    pub fn code(&mut self) -> &mut Vec<u8> {
        let n = self.top_node.expect("no top node");
        &mut self.model.nodes[n].strct.script_mem
    }
    pub fn code_len(&self) -> u32 {
        match self.top_node {
            Some(n) => self.model.nodes[n].strct.script_mem.len() as u32,
            None => 0,
        }
    }
    pub fn emit_u8(&mut self, v: u8) {
        self.code().push(v);
    }
    pub fn emit_op(&mut self, v: u8) {
        self.emit_u8(v);
    }
    pub fn emit_u16(&mut self, v: u16) {
        self.code().extend_from_slice(&v.to_le_bytes());
    }
    pub fn emit_i32(&mut self, v: i32) {
        self.code().extend_from_slice(&v.to_le_bytes());
    }
    pub fn emit_f32(&mut self, v: f32) {
        self.code().extend_from_slice(&v.to_le_bytes());
    }
    pub fn emit_name_text(&mut self, text: &str) {
        let (i, n) = self.fname(text);
        self.emit_i32(i as i32);
        self.emit_i32(n as i32);
    }
    pub fn emit_ptr(&mut self, r: Option<FieldRef>) {
        let h = self.ref_handle(r);
        self.code().extend_from_slice(&h.to_le_bytes());
    }
    pub fn write_ptr_at(&mut self, pos: u32, r: Option<FieldRef>) {
        let h = self.ref_handle(r);
        let c = self.code();
        c[pos as usize..pos as usize + 8].copy_from_slice(&h.to_le_bytes());
    }
    pub fn write_u16_at(&mut self, pos: u32, v: u16) {
        let c = self.code();
        c[pos as usize..pos as usize + 2].copy_from_slice(&v.to_le_bytes());
    }
    pub fn write_u8_at(&mut self, pos: u32, v: u8) {
        let c = self.code();
        c[pos as usize] = v;
    }

    pub fn emit_debug_info(&mut self, _opcode: u8) {
        if self.supress_debug_info > 0 {
            self.supress_debug_info -= 1;
        }
        // Debug info emission is disabled (shipped packages carry none).
    }

    /// `MoveCompiledCode(Dest, Source)`: move the code after `source` to `dest`.
    pub fn move_compiled_code(&mut self, dest: Location, source: Location) {
        if dest.code_top == source.code_top {
            return;
        }
        let total = self.code_len();
        let source_size = total - source.code_top;
        let dest_size = source.code_top - dest.code_top;
        // Update placeholder locations in the top nest (UE3 updates only the top nest).
        if let Some(nest) = self.nests.last_mut() {
            for p in &mut nest.placeholders {
                if p.fixup == Fixup::Label {
                    for l in &mut nest.labels {
                        if Some(&l.0) == p.name.as_ref() {
                            if dest.code_top < l.1 && dest.code_top + dest_size >= l.1 {
                                l.1 += source_size;
                            }
                            break;
                        }
                    }
                } else if dest.code_top < p.icode && dest.code_top + dest_size >= p.icode {
                    p.icode += source_size;
                }
            }
        }
        if let Some(nest) = self.nests.last_mut() {
            for b in &mut nest.struct_mod_bytes {
                if dest.code_top < *b && dest.code_top + dest_size >= *b {
                    *b += source_size;
                } else if source.code_top < *b {
                    *b -= dest_size;
                }
            }
        }
        let c = self.code();
        c[dest.code_top as usize..].rotate_right(source_size as usize);
    }

    // ----- nests ------------------------------------------------------------

    pub fn top_nest(&mut self) -> &mut NestInfo {
        self.nests.last_mut().expect("no nest")
    }
    pub fn top_nest_ref(&self) -> &NestInfo {
        self.nests.last().expect("no nest")
    }
    pub fn nest_level(&self) -> usize {
        self.nests.len()
    }

    pub fn check_allow(&mut self, thing: &str, flags: u32) -> CResult<()> {
        let top = self.top_nest_ref();
        if top.allow & flags != flags {
            if top.nest_type == NestType::None {
                return self.error(format!("{thing} is not allowed before the Class definition"));
            }
            return self.error(format!("{thing} is not allowed here"));
        }
        if flags & allow::CMD != 0 {
            self.top_nest().allow &= !(allow::VAR_DECL | allow::FUNCTION | allow::IGNORES | allow::TYPE_DECL);
        }
        Ok(())
    }

    pub fn find_nest(&self, t: NestType) -> Option<usize> {
        (1..self.nests.len()).rev().find(|&i| self.nests[i].nest_type == t)
    }

    pub fn emit_placeholder_for_jump(&mut self, nest_index: usize, fixup: Fixup, name: Option<String>, location: Option<u32>, value: u16) {
        let loc = match location {
            Some(l) => l,
            None => self.code_len(),
        };
        self.nests[nest_index].placeholders.insert(0, Placeholder { fixup, icode: loc, name });
        self.emit_u16(value);
    }

    pub fn emit_placeholder_for_chain(&mut self) {
        let pos = self.code_len();
        self.top_nest().chain_placeholder = Some(pos);
        self.emit_u16(0);
    }

    pub fn emit_chain_address_value(&mut self) {
        if let Some(pos) = self.top_nest().chain_placeholder.take() {
            let here = self.code_len() as u16;
            self.write_u16_at(pos, here);
        }
    }

    pub fn remove_jump_placeholders(&mut self, nest_index: usize, fixup: Fixup) -> usize {
        let before = self.nests[nest_index].placeholders.len();
        self.nests[nest_index].placeholders.retain(|p| p.fixup != fixup);
        before - self.nests[nest_index].placeholders.len()
    }

    /// `ReplacePlaceholderValues`.
    pub fn replace_placeholder_values(&mut self, nest_index: usize) -> CResult<()> {
        let nest = self.nests[nest_index].clone();
        for p in &nest.placeholders {
            if p.fixup == Fixup::Label {
                let name = p.name.clone().unwrap_or_default();
                let Some(l) = nest.labels.iter().find(|l| l.0.eq_ignore_ascii_case(&name)) else {
                    return self.error(format!("Label '{name}' not found in this block of code"));
                };
                self.write_u16_at(p.icode, l.1 as u16);
            } else {
                let target = nest.jump_targets[p.fixup as usize];
                if target == MAXWORD {
                    return self.error(format!("Internal fixup error {}", p.fixup as u8));
                }
                let value = if p.fixup == Fixup::IteratorSkip {
                    target - (p.icode + 2 + self.pointer_size + 1)
                } else {
                    target
                };
                self.write_u16_at(p.icode, value as u16);
            }
        }
        Ok(())
    }

    /// `PushNest` for both passes.
    pub fn push_nest(&mut self, nest_type: NestType, this_name: &str, in_node: Option<NodeId>) -> CResult<()> {
        let prev_top_node = self.top_node;
        let prev_allow = self.nests.last().map(|n| n.allow).unwrap_or(0);
        let is_new_node = matches!(nest_type, NestType::Class | NestType::Interface | NestType::State | NestType::Function);

        let mut nest = NestInfo::new(nest_type, in_node);
        if self.nests.len() >= 16 {
            return self.error("Maximum nesting limit exceeded");
        }
        let mut node = None;
        if !self.nests.is_empty() {
            if self.pass == Pass::Compile {
                node = if is_new_node { in_node } else { self.nests.last().and_then(|n| n.node) };
            } else if is_new_node {
                match nest_type {
                    NestType::Class | NestType::Interface => {
                        node = Some(self.class);
                        let c = self.class;
                        self.model.nodes[c].state.probe_mask = 0;
                        self.model.nodes[c].state.label_table_offset = MAXWORD as u16;
                    }
                    NestType::State => {
                        let outer = prev_top_node.unwrap_or(self.class);
                        let mut n = super::model::Node::new(NodeKind::State, this_name, Some(outer));
                        n.object_flags = tempest_upk::flags::rf::PUBLIC;
                        n.state.probe_mask = 0;
                        n.state.label_table_offset = MAXWORD as u16;
                        n.strct.text_pos = -1;
                        n.strct.line = -1;
                        node = Some(self.model.add(n));
                    }
                    NestType::Function => {
                        let outer = prev_top_node.unwrap_or(self.class);
                        let mut n = super::model::Node::new(NodeKind::Function, this_name, Some(outer));
                        n.object_flags = tempest_upk::flags::rf::PUBLIC;
                        n.func.rep_offset = MAXWORD as u16;
                        n.func.friendly_name = this_name.to_string();
                        n.strct.text_pos = -1;
                        n.strct.line = -1;
                        node = Some(self.model.add(n));
                    }
                    _ => {}
                }
            } else {
                node = self.nests.last().and_then(|n| n.node);
            }
        }
        nest.node = node;
        let prev_node = self.nests.last().and_then(|n| n.node);
        match nest_type {
            NestType::None => nest.allow = allow::CLASS,
            NestType::Class => {
                nest.allow = allow::VAR_DECL | allow::FUNCTION | allow::STATE | allow::IGNORES | allow::INSTANCE | allow::TYPE_DECL;
            }
            NestType::Interface => nest.allow = allow::FUNCTION | allow::TYPE_DECL,
            NestType::State => {
                nest.allow = allow::VAR_DECL | allow::FUNCTION | allow::LABEL | allow::STATE_CMD | allow::IGNORES | allow::INSTANCE;
                if self.pass == Pass::Parse {
                    if let (Some(n), Some(p)) = (node, prev_node) {
                        self.model.nodes[p].strct.children.insert(0, n);
                    }
                }
            }
            NestType::Function => {
                nest.allow = allow::VAR_DECL | allow::RETURN | allow::CMD | allow::LABEL;
                if let Some(n) = node {
                    if self.model.nodes[n].func.flags & func::STATIC == 0 {
                        nest.allow |= allow::INSTANCE;
                    }
                }
                if self.pass == Pass::Parse {
                    if let (Some(n), Some(p)) = (node, prev_node) {
                        self.model.nodes[p].strct.children.insert(0, n);
                    }
                } else if let Some(n) = node {
                    // FuncMap of the owning state/class.
                    let owner = match prev_top_node {
                        Some(p) if self.model.nodes[p].kind == NodeKind::State => p,
                        _ => self.class,
                    };
                    let name = self.model.nodes[n].name.clone();
                    let fm = &mut self.model.nodes[owner].state.func_map;
                    if let Some(e) = fm.iter_mut().find(|e| e.0.eq_ignore_ascii_case(&name)) {
                        e.1 = n;
                    } else {
                        fm.push((name, n));
                    }
                }
            }
            NestType::If => {
                nest.allow = allow::ELSE_IF
                    | (prev_allow & (allow::CMD | allow::LABEL | allow::BREAK | allow::CONTINUE | allow::STATE_CMD | allow::RETURN | allow::INSTANCE));
            }
            NestType::Loop => {
                nest.allow = allow::BREAK | allow::CONTINUE | (prev_allow & (allow::CMD | allow::LABEL | allow::STATE_CMD | allow::RETURN | allow::INSTANCE));
            }
            NestType::Switch => {
                nest.allow = allow::CASE | allow::DEFAULT | (prev_allow & (allow::STATE_CMD | allow::RETURN | allow::INSTANCE));
            }
            NestType::For => {
                nest.allow = allow::BREAK | allow::CONTINUE | (prev_allow & (allow::CMD | allow::LABEL | allow::STATE_CMD | allow::RETURN | allow::INSTANCE));
            }
            NestType::ForEach => {
                nest.allow = allow::ITERATOR | allow::BREAK | allow::CONTINUE | (prev_allow & (allow::CMD | allow::LABEL | allow::RETURN | allow::INSTANCE));
            }
            NestType::FilterEditorOnly => {
                nest.allow = prev_allow & (allow::CMD | allow::LABEL | allow::BREAK | allow::CONTINUE | allow::STATE_CMD | allow::RETURN | allow::INSTANCE);
            }
        }
        self.nests.push(nest);
        self.top_node = node;
        Ok(())
    }

    /// Post-parse: `ProbeMask` from probe function names (EName 300..331).
    pub fn probe_bit(name: &str) -> Option<u32> {
        const PROBES: &[&str] = &[
            "Destroyed", "GainedChild", "LostChild", "HitWall", "Falling", "Landed", "Touch", "UnTouch", "Bump", "BeginState",
            "EndState", "BaseChange", "Attach", "Detach", "EncroachingOn", "EncroachedBy", "MayFall", "Tick", "SeePlayer",
            "EnemyNotVisible", "HearNoise", "UpdateEyeHeight", "SeeMonster", "SpecialHandling", "BotDesireability", "NotifyBump",
            "NotifyLanded", "NotifyHitWall", "PreBeginPlay", "PostBeginPlay", "UnusedProbe", "All",
        ];
        PROBES.iter().position(|p| p.eq_ignore_ascii_case(name)).map(|i| i as u32)
    }

    pub fn state_flag_auto() -> u32 {
        sf::AUTO
    }

    pub fn first_code_op() -> u8 {
        op::NOTHING
    }
}

/// Bytes of a Latin-1 string (every char is < 0x100).
pub fn latin1_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| if (c as u32) < 0x100 { c as u32 as u8 } else { b'?' }).collect()
}

/// Latin-1 string of raw bytes (one char per byte).
pub fn latin1_string(b: &[u8]) -> String {
    b.iter().map(|&x| x as char).collect()
}

/// Windows-1252 interpretation of a Latin-1 string (what the engine does
/// when loading ANSI script files): remaps the 0x80..0x9F range.
pub fn cp1252_string(s: &str) -> String {
    s.chars().map(|c| if (0x80..0xA0).contains(&(c as u32)) { cp1252_char(c as u32 as u8) } else { c }).collect()
}

pub fn cp1252_char(b: u8) -> char {
    const T: [u32; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
        0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    if (0x80..0xA0).contains(&b) {
        char::from_u32(T[(b - 0x80) as usize]).unwrap_or(b as char)
    } else {
        b as char
    }
}
