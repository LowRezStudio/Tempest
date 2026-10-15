//! First pass (`PASS_Parse`): class header, declarations, type parsing and
//! object-model construction. Port of the declaration half of `UnScrCom.cpp`.

use tempest_upk::flags::{class as cf, cpf, func, rf, state as sf, strct as stf};

use super::engine::{allow, CResult, Compiler, NestType, Pass, TokKind, Token, MAXWORD};
use super::model::{FieldRef, Node, NodeId, NodeKind};
use super::scope::FieldKind;
use super::types::{Cpt, PropType, RefType};
use crate::objects::PropertyKind;

const MAX_ARRAY_SIZE: i32 = 2048;

/// `CppTags`: operator characters → identifier fragments for signature names.
const CPP_TAGS: [&str; 96] = [
    "Spc", "Not", "DoubleQuote", "Pound", "Concat", "Percent", "And", "SingleQuote", "OpenParen", "CloseParen", "Multiply", "Add", "Comma",
    "Subtract", "Dot", "Divide", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "Colon", "Semicolon", "Less", "Equal", "Greater",
    "Question", "At", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W",
    "X", "Y", "Z", "OpenBracket", "Backslash", "CloseBracket", "Xor", "_", "Not", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k",
    "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "OpenBrace", "Or", "CloseBrace", "Complement", "Or",
];

/// `GetPropertyName(CPT)` for operator signatures.
/// Operator signature part for a primitive type: the property class name with
/// `Property` removed (`GetPropertyName(Type)` minus the suffix).
fn cpt_signature_name(c: Cpt) -> &'static str {
    match c {
        Cpt::Byte => "Byte",
        Cpt::Int => "Int",
        Cpt::Bool => "Bool",
        Cpt::Float => "Float",
        Cpt::Name => "Name",
        Cpt::String => "Str",
        Cpt::Delegate => "Delegate",
        Cpt::Map => "Map",
        _ => "None",
    }
}

impl<'a> Compiler<'a> {
    // ----- small helpers -------------------------------------------------------

    pub fn class_ref(&self) -> FieldRef {
        FieldRef::Local(self.class)
    }
    pub fn class_node(&self) -> &Node {
        self.model.node(self.class)
    }
    pub fn class_node_mut(&mut self) -> &mut Node {
        self.model.node_mut(self.class)
    }
    pub fn class_has(&self, flag: u32) -> bool {
        self.class_node().class.class_flags & flag != 0
    }
    pub fn super_class(&self) -> Option<FieldRef> {
        self.class_node().strct.super_struct
    }
    fn top_node_id(&self) -> CResult<NodeId> {
        self.top_node.ok_or_else(|| self.err("Internal error: no top node"))
    }
    pub fn node_kind_is(&self, n: NodeId, k: NodeKind) -> bool {
        self.model.node(n).kind == k
    }

    /// Own property children of a local scope, in field order.
    fn own_properties(&self, scope: NodeId) -> Vec<NodeId> {
        self.model.node(scope).strct.children.iter().copied().filter(|&c| matches!(self.model.node(c).kind, NodeKind::Property(_))).collect()
    }

    /// Insert `child` after `prev` in `scope`'s children (or at the front).
    fn insert_child_after(&mut self, scope: NodeId, prev: Option<NodeId>, child: NodeId) {
        let children = &mut self.model.node_mut(scope).strct.children;
        match prev.and_then(|p| children.iter().position(|&c| c == p)) {
            Some(i) => children.insert(i + 1, child),
            None => children.insert(0, child),
        }
    }

    fn prepend_child(&mut self, scope: NodeId, child: NodeId) {
        self.model.node_mut(scope).strct.children.insert(0, child);
    }

    /// `FindField` returning only fields whose outer is exactly `scope` (`Existing->GetOuter()==Scope`).
    fn find_own_field(&self, scope: NodeId, name: &str) -> Option<NodeId> {
        self.model.node(scope).strct.children.iter().copied().find(|&c| self.model.node(c).name.eq_ignore_ascii_case(name))
    }

    /// `GetQualifiedClass`.
    pub fn get_qualified_class(&mut self, thing: Option<&str>) -> CResult<Option<FieldRef>> {
        let mut class_name = String::new();
        loop {
            let Some(t) = self.get_identifier(false)? else { break };
            class_name.push_str(&t.ident);
            if !self.match_symbol(".")? {
                break;
            }
            class_name.push('.');
        }
        if !class_name.is_empty() {
            let short = class_name.rsplit('.').next().unwrap_or(&class_name).to_string();
            let found = self.res().find_class(&short);
            if found.is_none() {
                return self.error(format!("Class '{class_name}' not found"));
            }
            Ok(found)
        } else if let Some(t) = thing {
            self.error(format!("{t}: Missing class name"))
        } else {
            Ok(None)
        }
    }

    /// `SkipStatements`.
    pub fn skip_statements(&mut self, mut nest_count: i32, tag: &str) -> CResult<()> {
        let original = nest_count;
        while let Some(t) = self.get_token_hint(None, true)? {
            if t.is_symbol("{") {
                nest_count += 1;
            } else if t.is_symbol("}") {
                nest_count -= 1;
            } else if t.is_symbol(";") && original == 0 {
                break;
            }
            if nest_count < original || nest_count < 0 {
                break;
            }
        }
        if nest_count > 0 {
            return self.error(format!("Unexpected end of file at end of {tag}"));
        } else if nest_count < 0 {
            return self.error(format!("Extraneous closing brace found in {tag}"));
        }
        Ok(())
    }

    /// `UProperty::IsLocalized`, which `UStructProperty`/`UArrayProperty` extend to their members.
    fn property_is_localized(&self, p: FieldRef, depth: usize) -> bool {
        if self.res().prop_flags(p) & cpf::LOCALIZED != 0 {
            return true;
        }
        if depth > 16 {
            return false;
        }
        match self.res().prop_kind(p) {
            Some(PropertyKind::Array) => self.res().prop_refs(p).0.map(|i| self.property_is_localized(i, depth + 1)).unwrap_or(false),
            Some(PropertyKind::Struct) => {
                let Some(s) = self.res().prop_refs(p).0 else { return false };
                let mut cur = Some(s);
                let mut guard = 0;
                while let Some(st) = cur {
                    if self.res().children(st).into_iter().any(|c| self.res().is_property(c) && self.property_is_localized(c, depth + 1)) {
                        return true;
                    }
                    cur = self.res().inheritance_super(st);
                    guard += 1;
                    if guard > 64 {
                        break;
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// `CompileDirective` (after the `#`).
    pub(super) fn compile_directive(&mut self) -> CResult<()> {
        self.compile_directive_named(None)
    }

    /// `CompileDirective` with the directive name already read (`#name` lexes as one identifier).
    pub(super) fn compile_directive_named(&mut self, name: Option<&str>) -> CResult<()> {
        let d = match name {
            Some(n) => {
                let mut t = Token::empty();
                t.kind = super::engine::TokKind::Identifier;
                t.ident = n.to_string();
                t
            }
            None => {
                let Some(d) = self.get_identifier(false)? else { return self.error("Missing compiler directive after '#'") };
                d
            }
        };
        if d.matches_ident("Error") {
            return self.error("#Error directive encountered");
        } else if d.matches_ident("linenumber") {
            let Some(n) = self.get_token_hint(None, false)? else { return self.error("Missing line number in line number directive") };
            if let Some(v) = n.get_const_int() {
                self.lexer_line = v as u32;
            }
        } else {
            return self.error(format!("Unrecognized compiler directive {}", d.ident));
        }
        let _ = self.get_raw_text(b'\n');
        Ok(())
    }

    /// `ParsePropertyExportText({...})`: returns the raw text if present.
    fn parse_property_export_text(&mut self, what: &str) -> CResult<Option<String>> {
        if self.match_symbol("{")? {
            let text = self.get_raw_text(b'}');
            if text.is_empty() {
                return self.error(format!("'{what}': No {what} type specified"));
            }
            self.require_symbol("}", &format!("' {what} type'"))?;
            return Ok(Some(text));
        }
        Ok(None)
    }

    /// `ParsePropertyExportText(ExportFlags)` for access specifiers with `{...}` header text.
    fn parse_access_export_text(&mut self, hint: &str) -> CResult<()> {
        if self.match_symbol("{")? {
            let text = self.get_raw_text(b'}');
            if text.is_empty() {
                return self.error(format!("No header text specified for modifier '{hint}'"));
            }
            if !["private", "protected", "public"].iter().any(|s| text.eq_ignore_ascii_case(s)) {
                return self.error(format!("Invalid header text specified for modifier '{hint}'"));
            }
            self.require_symbol("}", &format!("' {hint} modifier'"))?;
        }
        Ok(())
    }

    /// `ParsePropertyMetaData(<...>)`.
    fn parse_property_meta_data(&mut self, what: &str) -> CResult<()> {
        if self.match_symbol("<")? {
            let text = self.get_raw_text(b'>');
            if text.is_empty() {
                return self.error(format!("'{what}': No metadata specified"));
            }
            self.require_symbol(">", &format!("' {what} metadata'"))?;
            for pair in text.split('|') {
                let key = pair.split('=').next().unwrap_or("").trim();
                if key.is_empty() {
                    return self.error("Metadata was not formatted properly");
                }
            }
        }
        Ok(())
    }

    // ----- statements ---------------------------------------------------------

    /// `CompileStatement`.
    pub fn compile_statement(&mut self) -> CResult<bool> {
        let mut need_semicolon = true;
        let Some(mut token) = self.get_token_hint(None, true)? else { return Ok(false) };
        if !self.compile_declaration(&mut token, &mut need_semicolon)? {
            if self.pass == Pass::Parse {
                if self.nests.len() < 3 {
                    return self.error(format!("Unexpected '{}'", token.ident));
                }
                self.unget(&token);
                let nt = self.top_nest_ref().nest_type;
                self.pop_nest(nt, nest_type_name(nt))?;
                self.skip_statements(1, nest_type_name(nt))?;
                need_semicolon = false;
            } else {
                self.compile_command(&token, &mut need_semicolon)?;
            }
        }
        if need_semicolon && !self.match_symbol(";")? {
            if let Some(t) = self.get_token_hint(None, false)? {
                return self.error(format!("Missing ';' before '{}'", t.ident));
            }
            return self.error("Missing ';'");
        }
        Ok(true)
    }

    pub fn compile_statements(&mut self) -> CResult<()> {
        let original = self.nests.len();
        loop {
            self.compile_statement()?;
            if self.nests.len() <= original {
                break;
            }
        }
        Ok(())
    }

    fn is_valid_function_specifier(&mut self, t: &Token) -> CResult<bool> {
        if t.kind != TokKind::Identifier {
            return Ok(false);
        }
        let l = t.ident.to_ascii_lowercase();
        Ok(match l.as_str() {
            "function" | "delegate" | "operator" | "preoperator" | "postoperator" | "virtual" | "native" | "final" | "private" | "protected"
            | "public" | "latent" | "iterator" | "singular" | "static" | "noexport" | "noexportheader" | "exec" | "server" | "client"
            | "reliable" | "unreliable" | "dllimport" | "pcomnetapproved" | "pcomservercmd" => true,
            "event" => self.top_nest_ref().allow & allow::FUNCTION != 0,
            "simulated" => !self.peek_identifier("state")?,
            _ => false,
        })
    }

    /// `CompileDeclaration`.
    pub fn compile_declaration(&mut self, token: &mut Token, need_semicolon: &mut bool) -> CResult<bool> {
        let allow_class = self.top_nest_ref().allow & allow::CLASS != 0;
        if token.matches_ident("class") && allow_class {
            self.compile_class_declaration(token, need_semicolon)?;
        } else if token.matches_ident("interface") && allow_class {
            self.compile_interface_declaration(token, need_semicolon)?;
        } else if self.is_valid_function_specifier(token)? && !self.peek_symbol("=")? && !self.peek_symbol(".")? {
            self.compile_function_declaration(token, need_semicolon)?;
        } else if token.matches_ident("const") {
            let c = self.class;
            self.compile_const(c, need_semicolon)?;
        } else if token.matches_ident("var") || token.matches_ident("local") {
            self.compile_variable_declaration(token, need_semicolon)?;
        } else if token.matches_ident("enum") {
            self.check_allow("'Enum'", allow::TYPE_DECL)?;
            let c = self.class;
            self.compile_enum(c, need_semicolon)?;
        } else if token.matches_ident("struct") {
            self.check_allow("'struct'", allow::TYPE_DECL)?;
            let c = self.class;
            self.compile_struct(c, need_semicolon)?;
        } else if token.matches_ident("state") || token.matches_ident("auto") || token.matches_ident("simulated") {
            self.compile_state_declaration(token, need_semicolon)?;
        } else if token.matches_ident("ignores") {
            self.compile_ignore_declaration(token, need_semicolon)?;
        } else if token.matches_ident("replication") {
            if self.top_nest_ref().nest_type != NestType::Class {
                return self.error("'Replication' is not allowed here");
            }
            self.require_symbol("{", "'Replication'")?;
            let top = self.top_node_id()?;
            let pos = self.lexer_pos as i32;
            let line = self.lexer_line as i32;
            let n = self.model.node_mut(top);
            n.strct.text_pos = pos;
            n.strct.line = line;
            n.class.replication_pos = Some((pos as usize, line as u32));
            self.skip_statements(1, "'Replication'")?;
            *need_semicolon = false;
        } else if token.is_symbol("#") {
            self.compile_directive()?;
            *need_semicolon = false;
        } else if token.is_symbol(";") {
            *need_semicolon = false;
            return Ok(true);
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    // ----- class / interface ---------------------------------------------------

    fn parse_category_list(&mut self, tag: &str) -> CResult<Vec<String>> {
        self.require_symbol("(", &format!("'{tag}'"))?;
        let mut out = Vec::new();
        loop {
            let Some(c) = self.get_identifier(true)? else { return self.error(format!("{tag}: Expected category name")) };
            out.push(c.ident);
            if !self.match_symbol(",")? {
                break;
            }
        }
        self.require_symbol(")", &format!("'{tag}'"))?;
        Ok(out)
    }

    fn remove_name(list: &mut Vec<String>, name: &str) {
        list.retain(|n| !n.eq_ignore_ascii_case(name));
    }
    fn add_unique_name(list: &mut Vec<String>, name: &str) {
        if !list.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            list.push(name.to_string());
        }
    }

    /// `CompileClassDeclaration`.
    fn compile_class_declaration(&mut self, token: &mut Token, need_semicolon: &mut bool) -> CResult<()> {
        self.check_allow("'class'", allow::CLASS)?;
        let Some(name_tok) = self.get_token_hint(None, false)? else { return self.error("Missing class name") };
        *token = name_tok.clone();
        let class_name = self.class_node().name.clone();
        if !name_tok.matches(&class_name) {
            return self.error(format!("Class name '{}' doesn't match name of source file '{}'", name_tok.ident, class_name));
        }
        let object_class = self.object_class();
        if self.match_identifier("extends")? {
            let temp = self.get_qualified_class(Some("'extends'"))?.expect("class");
            if self.res().class_flags(temp) & cf::INTERFACE != 0 {
                return self.error(format!("Class '{}' cannot extend interface '{}', use 'implements'", class_name, self.res().name_of(temp)));
            }
            match self.super_class() {
                None => self.class_node_mut().strct.super_struct = Some(temp),
                Some(s) if s != temp => {
                    return self.error(format!("{}'s superclass must be {}, not {}", class_name, self.res().name_of(s), self.res().name_of(temp)));
                }
                _ => {}
            }
        } else if let Some(s) = self.super_class() {
            return self.error(format!("class: missing 'Extends {}'", self.res().name_of(s)));
        }
        if self.match_identifier("within")? {
            let temp = self.get_qualified_class(Some("'within'"))?.expect("class");
            let iface = self.res().find_class("Interface");
            if iface.map(|i| self.res().is_child_of(temp, i)).unwrap_or(false) {
                return self.error("Classes cannot be 'within' interfaces");
            }
            let within = self.class_node().class.class_within;
            if within.is_none() || within == object_class || within.map(|w| self.res().is_child_of(temp, w)).unwrap_or(false) {
                self.class_node_mut().class.class_within = Some(temp);
            } else if within != Some(temp) {
                return self.error(format!(
                    "{} must be within {}, not {}",
                    class_name,
                    within.map(|w| self.res().name_of(w)).unwrap_or_default(),
                    self.res().name_of(temp)
                ));
            }
        } else {
            let w = self.super_class().and_then(|s| self.res().class_within(s)).or(object_class);
            self.class_node_mut().class.class_within = w;
        }
        let expected_within = self.super_class().and_then(|s| self.res().class_within(s)).or(object_class);
        if let (Some(w), Some(e)) = (self.class_node().class.class_within, expected_within) {
            if !self.res().is_child_of(w, e) {
                return self.error(format!(
                    "Parent class declared within '{}'.  Cannot override within class with '{}' since it isn't a child",
                    self.res().name_of(e),
                    self.res().name_of(w)
                ));
            }
        }
        let mut declares_config_file = false;
        let actor = self.actor_class();
        let mut inheritance_parents: Vec<(String, Option<FieldRef>)> = Vec::new();
        loop {
            let Some(t) = self.get_token_hint(None, false)? else { break };
            let l = t.ident.to_ascii_lowercase();
            if t.kind != TokKind::Identifier {
                self.unget(&t);
                break;
            }
            match l.as_str() {
                "native" | "nativeonly" => {
                    if let Some(s) = self.super_class() {
                        if self.res().class_flags(s) & cf::NATIVE == 0 {
                            return self.error("Native classes cannot expand non-native classes");
                        }
                    }
                    self.class_node_mut().object_flags |= rf::NATIVE;
                    self.class_node_mut().class.class_flags |= cf::NATIVE;
                    if l == "nativeonly" {
                        self.class_node_mut().class.class_flags |= cf::NATIVE_ONLY;
                    }
                    if self.match_symbol("(")? {
                        let Some(h) = self.get_identifier(false)? else { return self.error("native: Missing native header filename") };
                        if h.ident.eq_ignore_ascii_case("inherit") {
                            let Some(s) = self.super_class() else {
                                return self.error(format!("Cannot inherit native header filename: {class_name} has no super class"));
                            };
                            let hf = match s {
                                FieldRef::Local(n) => self.model.node(n).class.header_filename.clone(),
                                FieldRef::External(o) => self.ctx.class_header_filename(o),
                            };
                            self.class_node_mut().class.header_filename = hf;
                        } else {
                            self.class_node_mut().class.header_filename = h.ident;
                        }
                        self.require_symbol(")", "native")?;
                    }
                }
                "inherits" => {
                    self.require_symbol("(", "inherits")?;
                    loop {
                        let Some(t) = self.get_identifier(false)? else { return self.error("inherits: Missing C++ base class name") };
                        inheritance_parents.push((t.ident, None));
                        if self.peek_symbol(",")? {
                            self.get_symbol_token()?;
                        } else {
                            break;
                        }
                    }
                    self.require_symbol(")", "inherits")?;
                }
                "noexport" => self.class_node_mut().class.class_flags |= cf::NO_EXPORT,
                "editinlinenew" => {
                    if actor.map(|a| self.res().is_child_of(self.class_ref(), a)).unwrap_or(false) {
                        return self.error("Invalid class attribute: Creating actor instances via the property window is not allowed");
                    }
                    self.class_node_mut().class.class_flags |= cf::EDIT_INLINE_NEW;
                }
                "noteditinlinenew" => self.class_node_mut().class.class_flags &= !cf::EDIT_INLINE_NEW,
                "placeable" => self.class_node_mut().class.class_flags |= cf::PLACEABLE,
                "notplaceable" => self.class_node_mut().class.class_flags &= !cf::PLACEABLE,
                "hidedropdown" => self.class_node_mut().class.class_flags |= cf::HIDE_DROP_DOWN,
                // Extension: `hidden` sets CLASS_Hidden (the engine sets it natively).
                "hidden" => self.class_node_mut().class.class_flags |= cf::HIDDEN,
                "nativereplication" => self.class_node_mut().class.class_flags |= cf::NATIVE_REPLICATION,
                "dependson" => {
                    self.require_symbol("(", "dependsOn")?;
                    loop {
                        let Some(t) = self.get_identifier(false)? else { return self.error("dependsOn: Missing dependent name") };
                        self.class_node_mut().class.depends_on.push(t.ident);
                        if self.peek_symbol(",")? {
                            self.get_symbol_token()?;
                        } else {
                            break;
                        }
                    }
                    self.require_symbol(")", "dependsOn")?;
                }
                "perobjectconfig" => self.class_node_mut().class.class_flags |= cf::PER_OBJECT_CONFIG,
                "perobjectlocalized" => self.class_node_mut().class.class_flags |= cf::PER_OBJECT_LOCALIZED,
                "abstract" => self.class_node_mut().class.class_flags |= cf::ABSTRACT,
                "deprecated" => self.class_node_mut().class.class_flags |= cf::DEPRECATED,
                "transient" => self.class_node_mut().class.class_flags |= cf::TRANSIENT,
                "nontransient" => self.class_node_mut().class.class_flags &= !cf::TRANSIENT,
                "config" => {
                    if self.match_symbol("(")? {
                        let Some(c) = self.get_identifier(false)? else { return self.error("config: Missing name of configuration file.") };
                        if c.ident.eq_ignore_ascii_case("inherit") {
                            let Some(s) = self.super_class() else {
                                return self.error(format!("Cannot inherit config filename: {class_name} has no super class"));
                            };
                            let cn = self.res().class_config_name(s);
                            if cn.eq_ignore_ascii_case("None") || cn.is_empty() {
                                return self.error(format!("Cannot inherit config filename: parent class {} is not marked config.", self.res().name_of(s)));
                            }
                        } else {
                            self.class_node_mut().class.config_name = c.ident;
                        }
                        self.require_symbol(")", "config")?;
                        declares_config_file = true;
                    } else {
                        return self.error("config: Missing configuration name");
                    }
                }
                "safereplace" => self.warn("SafeReplace: keyword is deprecated"),
                "showcategories" => {
                    let list = self.parse_category_list("ShowCategories")?;
                    for c in list {
                        Self::remove_name(&mut self.class_node_mut().class.hide_categories, &c);
                    }
                }
                "forcescriptorder" => {
                    self.require_symbol("(", "'ForceScriptOrder'")?;
                    let Some(v) = self.get_identifier(true)? else { return self.error("ForceScriptOrder: Expected boolean value") };
                    self.class_node_mut().class.force_script_order = v.matches_ident("true");
                    self.require_symbol(")", "'ForceScriptOrder'")?;
                }
                "hidecategories" => {
                    let list = self.parse_category_list("HideCategories")?;
                    for c in list {
                        self.class_node_mut().class.hide_categories.push(c);
                    }
                }
                "classgroup" => {
                    let list = self.parse_category_list("ClassGroup")?;
                    for c in list {
                        self.class_node_mut().class.class_group_names.push(c);
                    }
                }
                "autoexpandcategories" => {
                    let list = self.parse_category_list("AutoExpandCategories")?;
                    for c in list {
                        let cn = self.class_node_mut();
                        Self::remove_name(&mut cn.class.auto_collapse_categories, &c);
                        Self::add_unique_name(&mut cn.class.auto_expand_categories, &c);
                    }
                }
                "autocollapsecategories" => {
                    let list = self.parse_category_list("AutoCollapseCategories")?;
                    for c in list {
                        let cn = self.class_node_mut();
                        Self::remove_name(&mut cn.class.auto_expand_categories, &c);
                        Self::add_unique_name(&mut cn.class.auto_collapse_categories, &c);
                    }
                }
                "dontautocollapsecategories" => {
                    let list = self.parse_category_list("DontAutoCollapseCategories")?;
                    for c in list {
                        Self::remove_name(&mut self.class_node_mut().class.auto_collapse_categories, &c);
                    }
                }
                "dontsortcategories" => {
                    let list = self.parse_category_list("DontSortCategories")?;
                    for c in list {
                        Self::add_unique_name(&mut self.class_node_mut().class.dont_sort_categories, &c);
                    }
                }
                "collapsecategories" => self.class_node_mut().class.class_flags |= cf::COLLAPSE_CATEGORIES,
                "dontcollapsecategories" => self.class_node_mut().class.class_flags &= !cf::COLLAPSE_CATEGORIES,
                "implements" => {
                    self.require_symbol("(", "'Implements'")?;
                    loop {
                        let Some(iface) = self.get_qualified_class(Some("'Implements'"))? else {
                            return self.error("Implements: Interface class not found");
                        };
                        if self.res().class_flags(iface) & cf::INTERFACE == 0 {
                            return self.error(format!("Implements: Class {} is not an interface", self.res().name_of(iface)));
                        }
                        let mut test = Some(self.class_ref());
                        while let Some(tc) = test {
                            if self.res().class_interfaces(tc).contains(&iface) {
                                return self.error(format!(
                                    "Implements: Interface '{}' is already implemented by '{}'",
                                    self.res().name_of(iface),
                                    self.res().name_of(tc)
                                ));
                            }
                            test = self.res().super_of(tc);
                        }
                        let iflags = self.res().class_flags(iface);
                        self.class_node_mut().class.class_flags |= iflags & cf::SCRIPT_INHERIT;
                        self.class_node_mut().class.interfaces.push((iface, None));
                        if iflags & cf::NATIVE != 0 {
                            inheritance_parents.push((format!("I{}", self.res().name_of(iface)), Some(iface)));
                        }
                        if !self.match_symbol(",")? {
                            break;
                        }
                    }
                    self.require_symbol(")", "'Implements'")?;
                }
                "classredirect" => {
                    self.require_symbol("(", "ClassRedirect")?;
                    loop {
                        if self.get_identifier(false)?.is_none() {
                            return self.error("ClassRedirect: Missing previous class name");
                        }
                        if self.peek_symbol(",")? {
                            self.get_symbol_token()?;
                        } else {
                            break;
                        }
                    }
                    self.require_symbol(")", "ClassRedirect")?;
                }
                "dllbind" => {
                    self.require_symbol("(", "DLLBind")?;
                    let Some(t) = self.get_identifier(false)? else { return self.error("DLLBind: Missing DLL name") };
                    self.class_node_mut().class.dll_bind_name = t.ident;
                    self.require_symbol(")", "DLLBind")?;
                }
                _ => {
                    self.unget(&t);
                    break;
                }
            }
        }
        if self.class_has(cf::NO_EXPORT) && !self.class_has(cf::NATIVE) {
            return self.error("'noexport': Only valid for native classes");
        }
        if !self.class_has(cf::NATIVE) {
            for (i, _) in self.class_node().class.interfaces.clone() {
                if self.res().class_flags(i) & cf::NATIVE != 0 {
                    return self.error("Non-native classes not allowed to implement native interfaces");
                }
            }
            if !inheritance_parents.is_empty() {
                return self.error("Inherits: only valid for native classes");
            }
        }
        if !declares_config_file {
            self.class_node_mut().class.config_name = "None".into();
        }
        if self.class_node().strct.cpp_text.is_some() && !self.class_has(cf::NATIVE) {
            return self.error("Cannot use cpptext blocks in non-native classes");
        }
        self.require_symbol(";", "'Class'")?;
        *need_semicolon = false;
        {
            let c = self.class_node_mut();
            c.strct.script_mem.clear();
            c.strct.children.clear();
            c.state.probe_mask = 0;
            c.state.state_flags = 0;
            c.state.label_table_offset = 0;
            c.object_flags &= !rf::TRANSIENT;
        }
        if let Some(s) = self.super_class() {
            let size = self.struct_layout(s).0;
            self.class_node_mut().properties_size = size;
        }
        let cname = self.class_node().name.clone();
        self.push_nest(NestType::Class, &cname, None)?;
        if !inheritance_parents.is_empty() {
            let Some(pointer) = self.res().find_struct_any("Pointer") else { return self.error("Pointer struct not found") };
            for (parent, iface) in inheritance_parents.iter().rev() {
                let mut n = Node::new(NodeKind::Property(PropertyKind::Struct), &format!("VfTable_{parent}"), Some(self.class));
                n.object_flags = 0;
                n.prop.array_dim = 1;
                n.prop.flags = cpf::NATIVE | cpf::CONST | cpf::NO_EXPORT;
                n.prop.ref1 = Some(pointer);
                let id = self.model.add(n);
                let c = self.class;
                self.prepend_child(c, id);
                if let Some(iface) = iface {
                    if let Some(e) = self.class_node_mut().class.interfaces.iter_mut().find(|(i, p)| i == iface && p.is_none()) {
                        e.1 = Some(id);
                    }
                }
            }
        }
        Ok(())
    }

    /// `CompileInterfaceDeclaration`.
    fn compile_interface_declaration(&mut self, token: &mut Token, need_semicolon: &mut bool) -> CResult<()> {
        self.check_allow("'interface'", allow::CLASS)?;
        let Some(name_tok) = self.get_token_hint(None, false)? else { return self.error("Missing interface name") };
        *token = name_tok.clone();
        let class_name = self.class_node().name.clone();
        if !name_tok.matches(&class_name) {
            return self.error(format!("Interface name '{}' doesn't match name of source file '{}'", name_tok.ident, class_name));
        }
        if self.match_identifier("extends")? {
            let temp = self.get_qualified_class(Some("'extends'"))?.expect("class");
            if self.res().class_flags(temp) & cf::INTERFACE == 0 {
                return self.error(format!("Interface class '{}' cannot inherit from non-interface class '{}'", name_tok.ident, self.res().name_of(temp)));
            }
            match self.super_class() {
                None => self.class_node_mut().strct.super_struct = Some(temp),
                Some(s) if s != temp => {
                    return self.error(format!("{}'s superclass must be {}, not {}", class_name, self.res().name_of(s), self.res().name_of(temp)));
                }
                _ => {}
            }
        }
        self.class_node_mut().class.class_flags |= cf::INTERFACE | cf::ABSTRACT;
        let interface_class = self.res().find_class("Interface");
        loop {
            let Some(t) = self.get_token_hint(None, false)? else { break };
            if t.kind != TokKind::Identifier {
                self.unget(&t);
                break;
            }
            let l = t.ident.to_ascii_lowercase();
            match l.as_str() {
                "native" | "nativeonly" => {
                    let sc = self.super_class();
                    if sc != interface_class {
                        if let Some(s) = sc {
                            if self.res().class_flags(s) & cf::NATIVE == 0 {
                                return self.error(format!("Native interface '{}' cannot extend non-native interface '{}'", class_name, self.res().name_of(s)));
                            }
                        }
                    }
                    self.class_node_mut().object_flags |= rf::NATIVE;
                    self.class_node_mut().class.class_flags |= cf::NATIVE;
                    if l == "nativeonly" {
                        self.class_node_mut().class.class_flags |= cf::NATIVE_ONLY;
                    }
                    if self.match_symbol("(")? {
                        let Some(h) = self.get_identifier(false)? else { return self.error("native: Missing native header filename") };
                        if h.ident.eq_ignore_ascii_case("inherit") {
                            let Some(s) = self.super_class() else {
                                return self.error(format!("Cannot inherit native header filename: {class_name} has no super class"));
                            };
                            let hf = match s {
                                FieldRef::Local(n) => self.model.node(n).class.header_filename.clone(),
                                FieldRef::External(o) => self.ctx.class_header_filename(o),
                            };
                            self.class_node_mut().class.header_filename = hf;
                        } else {
                            self.class_node_mut().class.header_filename = h.ident;
                        }
                        self.require_symbol(")", "native")?;
                    }
                }
                "dependson" => {
                    self.require_symbol("(", "dependsOn")?;
                    loop {
                        let Some(t) = self.get_identifier(false)? else { return self.error("dependsOn: Missing dependent name") };
                        self.class_node_mut().class.depends_on.push(t.ident);
                        if self.peek_symbol(",")? {
                            self.get_symbol_token()?;
                        } else {
                            break;
                        }
                    }
                    self.require_symbol(")", "dependsOn")?;
                }
                "editinlinenew" => self.class_node_mut().class.class_flags |= cf::EDIT_INLINE_NEW,
                _ => {
                    self.unget(&t);
                    break;
                }
            }
        }
        self.require_symbol(";", "'interface'")?;
        *need_semicolon = false;
        {
            let c = self.class_node_mut();
            c.strct.script_mem.clear();
            c.strct.children.clear();
            c.state.probe_mask = 0;
            c.state.state_flags = 0;
            c.state.label_table_offset = 0;
            c.object_flags &= !rf::TRANSIENT;
        }
        let cname = self.class_node().name.clone();
        self.push_nest(NestType::Interface, &cname, None)?;
        Ok(())
    }

    // ----- types ---------------------------------------------------------------

    /// `GetVarType`. Returns `Ok(false)` when no type was found and `thing` is `None`.
    #[allow(clippy::too_many_arguments)]
    pub fn get_var_type(
        &mut self,
        scope: NodeId,
        var: &mut PropType,
        object_flags: &mut u64,
        disallow: u64,
        thing: Option<&str>,
        outer_property_type: Option<&PropType>,
    ) -> CResult<bool> {
        *object_flags = rf::PUBLIC;
        let mut flags: u64 = 0;
        loop {
            let Some(spec) = self.get_token_hint(None, false)? else { break };
            if spec.kind != TokKind::Identifier {
                self.unget(&spec);
                break;
            }
            let l = spec.ident.to_ascii_lowercase();
            match l.as_str() {
                "const" => flags |= cpf::CONST,
                "config" => flags |= cpf::CONFIG,
                "globalconfig" => flags |= cpf::GLOBAL_CONFIG | cpf::CONFIG,
                "localized" => flags |= cpf::LOCALIZED | cpf::CONST,
                "private" => {
                    self.parse_access_export_text("private")?;
                    if *object_flags & rf::PROTECTED != 0 {
                        return self.error("Property cannot be marked both 'private' and 'protected'");
                    }
                    *object_flags &= !(rf::PUBLIC | rf::PROTECTED);
                }
                "protected" => {
                    self.parse_access_export_text("protected")?;
                    if *object_flags & rf::PUBLIC == 0 {
                        return self.error("Property cannot be marked both 'private' and 'protected'");
                    }
                    *object_flags |= rf::PUBLIC | rf::PROTECTED;
                }
                "privatewrite" => {
                    self.parse_access_export_text("privatewrite")?;
                    if *object_flags & rf::PUBLIC == 0 {
                        return self.error("Property cannot be marked both 'private' and 'privatewrite'");
                    } else if *object_flags & rf::PROTECTED != 0 {
                        return self.error("Property cannot be marked both 'protected' and 'privatewrite'");
                    } else if flags & cpf::PROTECTED_WRITE != 0 {
                        return self.error("Property cannot be marked both 'privatewrite' and 'protectedwrite'");
                    }
                    flags |= cpf::PRIVATE_WRITE;
                }
                "protectedwrite" => {
                    self.parse_access_export_text("protectedwrite")?;
                    if *object_flags & rf::PROTECTED != 0 {
                        return self.error("Property cannot be marked both 'protected' and 'protectedwrite'");
                    } else if flags & cpf::PRIVATE_WRITE != 0 {
                        return self.error("Property cannot be marked both 'privatewrite' and 'protectedwrite'");
                    }
                    flags |= cpf::PROTECTED_WRITE;
                }
                "public" => {
                    self.parse_access_export_text("public")?;
                    *object_flags |= rf::PUBLIC;
                }
                "editconst" => flags |= cpf::EDIT_CONST,
                "edithide" => flags |= cpf::EDIT_HIDE,
                "edittextbox" => flags |= cpf::EDIT_TEXT_BOX,
                "input" => flags |= cpf::INPUT,
                "transient" => flags |= cpf::TRANSIENT,
                "native" => flags |= cpf::NATIVE,
                "noexport" => flags |= cpf::NO_EXPORT,
                "duplicatetransient" => flags |= cpf::DUPLICATE_TRANSIENT,
                "noimport" => flags |= cpf::NO_IMPORT,
                "out" => flags |= cpf::OUT_PARM,
                "export" => flags |= cpf::EXPORT_OBJECT,
                "editinline" => flags |= cpf::EDIT_INLINE,
                "editinlineuse" => flags |= cpf::EDIT_INLINE | cpf::EDIT_INLINE_USE,
                "noclear" => flags |= cpf::NO_CLEAR,
                "editfixedsize" => flags |= cpf::EDIT_FIXED_SIZE,
                "repnotify" => flags |= cpf::REP_NOTIFY,
                "repretry" => flags |= cpf::REP_RETRY,
                "interp" => flags |= cpf::EDIT | cpf::INTERP,
                "nontransactional" => flags |= cpf::NON_TRANSACTIONAL,
                "deprecated" => flags |= cpf::DEPRECATED,
                "skip" => flags |= cpf::SKIP_PARM,
                "coerce" => flags |= cpf::COERCE_PARM,
                "optional" => flags |= cpf::OPTIONAL_PARM,
                "init" => flags |= cpf::ALWAYS_INIT,
                "instanced" => flags |= cpf::EDIT_INLINE | cpf::EXPORT_OBJECT,
                "databinding" => flags |= cpf::DATA_BINDING,
                "editoronly" => flags |= cpf::EDITOR_ONLY,
                "notforconsole" => flags |= cpf::NOT_FOR_CONSOLE,
                "archetype" => flags |= cpf::ARCHETYPE_PROPERTY,
                "serializetext" => flags |= cpf::SERIALIZE_TEXT,
                "crosslevel" => return self.error("CrossLevel properties must be either 'crosslevelpassive' or 'crosslevelactive'"),
                "crosslevelactive" => flags |= cpf::CROSS_LEVEL_ACTIVE,
                "crosslevelpassive" => flags |= cpf::CROSS_LEVEL_PASSIVE,
                _ => {
                    self.unget(&spec);
                    break;
                }
            }
        }
        let Some(var_type) = self.get_identifier(true)? else {
            let Some(t) = thing else { return Ok(false) };
            return self.error(format!("{t}: Missing variable type"));
        };
        let l = var_type.ident.to_ascii_lowercase();
        let class_class = self.class_class();
        let object_class = self.object_class();
        match l.as_str() {
            "enum" => {
                let mut ns = true;
                let e = self.compile_enum(scope, &mut ns)?;
                *var = PropType::enum_byte(Some(e));
            }
            "struct" => {
                let mut ns = true;
                let s = self.compile_struct(scope, &mut ns)?;
                *var = self.struct_prop_type(s);
            }
            "byte" => *var = PropType::new(Cpt::Byte),
            "int" => *var = PropType::new(Cpt::Int),
            "bool" => *var = PropType::new(Cpt::Bool),
            "float" => *var = PropType::new(Cpt::Float),
            "name" => *var = PropType::new(Cpt::Name),
            "array" => {
                self.require_symbol("<", "'array'")?;
                let mut inner_flags = 0u64;
                let mut outer = PropType::none();
                outer.flags = flags;
                self.get_var_type(scope, var, &mut inner_flags, disallow, Some("'array'"), Some(&outer))?;
                if var.array_dim == 0 {
                    return self.error("Arrays within arrays not supported.");
                } else if var.array_dim < 0 {
                    return self.error("Maps within arrays not supported.");
                }
                var.array_dim = 0;
                self.require_symbol(">", "'array'")?;
            }
            "map" => {
                if flags & cpf::NATIVE == 0 && !self.class_has(cf::TRANSIENT) {
                    return self.error("You must mark this property 'native'.  Non-native maps are not currently supported!");
                }
                *var = PropType::new(Cpt::Map);
                if self.parse_property_export_text("map")?.is_none() {
                    return self.error("'map': No types specified");
                } else if !self.class_has(cf::NATIVE) {
                    return self.error("Export text should only be used in native classes");
                }
                var.array_dim = -1;
            }
            "string" => {
                if self.match_symbol("[")? {
                    if self.get_const_int(None)?.is_none() {
                        return self.error(format!("{}: Missing string size", thing.unwrap_or("Declaration")));
                    }
                    if !self.match_symbol("]")? {
                        return self.error(format!("{}: Missing ']'", thing.unwrap_or("Declaration")));
                    }
                    self.warn("String sizes are now obsolete; all strings are dynamically sized");
                }
                *var = PropType::new(Cpt::String);
            }
            "delegate" => {
                self.require_symbol("<", "'delegate'")?;
                let mut name = String::new();
                loop {
                    let Some(t) = self.get_identifier(true)? else {
                        return self.error(format!("{}: Failed to read delegate name", thing.unwrap_or("Declaration")));
                    };
                    if !name.is_empty() {
                        name.push('.');
                    }
                    name.push_str(&t.ident);
                    if !self.match_symbol(".")? {
                        break;
                    }
                }
                self.require_symbol(">", "'delegate'")?;
                *var = PropType::new(Cpt::Delegate);
                var.delegate_name = Some(name);
            }
            "safeobject" => {
                // Hi-Rez extension: `safeobject<Class>`.
                self.require_symbol("<", "'safeobject'")?;
                let Some(ct) = self.get_identifier(false)? else { return self.error("'safeobject': Missing class name") };
                let Some(c) = self.res().find_class(&ct.ident) else { return self.error(format!("'safeobject': Class '{}' not found", ct.ident)) };
                self.require_symbol(">", "'safeobject'")?;
                *var = PropType::object(c, false);
                var.safe = true;
                // Marked as a safe reference; one to a component gets the component flags.
                flags |= cpf::SAFE_OBJECT;
                if self.res().is_component_class(c) {
                    flags |= (cpf::COMPONENT | cpf::EXPORT_OBJECT) & !disallow;
                }
            }
            _ => {
                if let Some(temp_class) = self.res().find_class(&var_type.ident) {
                    let is_iface = self.res().class_flags(temp_class) & cf::INTERFACE != 0;
                    *var = PropType::object(temp_class, is_iface);
                    if Some(temp_class) == class_class {
                        if self.match_symbol("<")? {
                            let Some(lim) = self.get_identifier(false)? else { return self.error("'class': Missing class limitor") };
                            var.meta_class = self.res().find_class(&lim.ident);
                            if var.meta_class.is_none() {
                                return self.error(format!("'class': Limitor '{}' is not a class name", lim.ident));
                            }
                            self.require_symbol(">", "'class limitor'")?;
                        } else {
                            var.meta_class = object_class;
                        }
                    }
                    if self.match_symbol(".")? {
                        let Some(elem) = self.get_identifier(false)? else {
                            return self.error(format!("'{}': Missing class member type after '.'", var_type.ident));
                        };
                        match self.res().find_field(Some(temp_class), &elem.ident, true, None) {
                            Some(f) if self.res().kind_of(f) == FieldKind::Enum => *var = PropType::enum_byte(Some(f)),
                            Some(f) if self.res().kind_of(f) == FieldKind::ScriptStruct => {
                                *var = self.struct_prop_type(f);
                                if self.res().struct_flags(f) & stf::HAS_COMPONENTS != 0 && disallow & cpf::COMPONENT == 0 {
                                    var.flags |= cpf::COMPONENT;
                                }
                            }
                            _ => return self.error(format!("Unrecognized type '{}' within '{}'", elem.ident, var_type.ident)),
                        }
                    }
                    if self.res().is_component_class(temp_class) {
                        flags |= (cpf::COMPONENT | cpf::EXPORT_OBJECT) & !disallow;
                    } else if flags & cpf::COMPONENT != 0 {
                        return self.error("Component modifier not allowed for non-UComponent object references");
                    }
                } else if let Some(e) = self.res().find_enum_scoped(FieldRef::Local(scope), &var_type.ident) {
                    *var = PropType::enum_byte(Some(e));
                } else if let Some(s) = self.res().find_struct_scoped(FieldRef::Local(scope), &var_type.ident) {
                    *var = self.struct_prop_type(s);
                    if self.res().struct_flags(s) & stf::HAS_COMPONENTS != 0 && disallow & cpf::COMPONENT == 0 {
                        var.flags |= cpf::COMPONENT;
                    }
                } else if thing.is_none() {
                    self.unget(&var_type);
                    return Ok(false);
                } else {
                    return self.error(format!("Unrecognized type '{}'", var_type.ident));
                }
            }
        }
        var.flags |= flags;
        if var.is_object() && var.flags & cpf::CONFIG != 0 {
            return self.error("Not allowed to use 'config' with object variables");
        }
        if var.flags & cpf::NATIVE != 0 && !self.class_has(cf::NATIVE) {
            return self.error("Not allowed to declare variables 'native' in non-native classes");
        }
        if var.flags & cpf::REP_RETRY != 0 && var.cpt != Cpt::Struct {
            return self.error("'RepRetry' is only allowed on struct properties");
        }
        if let Some(mc) = var.meta_class {
            if self.res().class_flags(mc) & cf::DEPRECATED != 0
                && var.flags & cpf::DEPRECATED == 0
                && outer_property_type.map(|o| o.flags & cpf::DEPRECATED == 0).unwrap_or(true)
            {
                self.warn(format!("Property is using a deprecated class: {}.  Property should be marked deprecated as well.", self.res().name_of(mc)));
            }
        }
        if var.cpt == Cpt::Object {
            if let Some(pc) = var.inner {
                if self.res().class_flags(pc) & cf::DEPRECATED != 0
                    && var.flags & cpf::DEPRECATED == 0
                    && outer_property_type.map(|o| o.flags & cpf::DEPRECATED == 0).unwrap_or(true)
                {
                    self.warn(format!("Property is using a deprecated class: {}.  Property should be marked deprecated as well.", self.res().name_of(pc)));
                }
            }
        }
        if !self.node_kind_is(scope, NodeKind::Class) {
            if *object_flags & rf::PROTECTED != 0 {
                return self.error("'Protected' is only allowed on class member variables");
            } else if var.flags & cpf::PRIVATE_WRITE != 0 {
                return self.error("'PrivateWrite' is only allowed on class member variables");
            } else if var.flags & cpf::PROTECTED_WRITE != 0 {
                return self.error("'ProtectedWrite' is only allowed on class member variables");
            } else if var.flags & cpf::DUPLICATE_TRANSIENT != 0 {
                return self.error("'DuplicateTransient' is only allowed on class member variables");
            }
        }
        if var.flags & cpf::SERIALIZE_TEXT != 0 && var.flags & cpf::NATIVE == 0 {
            self.warn("Variable is declared with 'SerializeText' but this keyword only has meaning for Native variables.");
        }
        if var.flags & disallow != 0 {
            return self.error("Specified type modifiers not allowed here");
        }
        Ok(true)
    }

    /// `FPropertyBase(UScriptStruct*)`: struct type with `CPF_PropagateFromStruct`-style flag seeding.
    /// `FPropertyBase(UScriptStruct*)`: the `CPF_Component` flag for structs with
    /// components is added by the callers, subject to `Disallow`.
    fn struct_prop_type(&self, s: FieldRef) -> PropType {
        PropType::strct(s)
    }

    /// `GroupProperty`.
    fn group_property(&self, scope: NodeId, previous: Option<NodeId>, property_flags: u64, kind: PropertyKind) -> Option<NodeId> {
        let mut movable = true;
        let n = self.model.node(scope);
        if n.kind != NodeKind::Class || n.class.class_flags & cf::NO_EXPORT != 0 {
            movable = false;
        }
        if property_flags & cpf::NO_EXPORT != 0 {
            movable = false;
        }
        let mut prev = previous;
        if movable {
            for p in self.own_properties(scope) {
                let pn = self.model.node(p);
                if pn.prop.flags & cpf::NO_EXPORT != 0 && !pn.name.starts_with("Vftable_") && !pn.name.starts_with("VfTable_") {
                    break;
                }
                if pn.kind == NodeKind::Property(kind) {
                    prev = Some(p);
                }
            }
        }
        prev
    }

    /// `GetVarNameAndDim`. Returns the new property node (`None` when `skip`).
    #[allow(clippy::too_many_arguments)]
    pub fn get_var_name_and_dim(
        &mut self,
        scope: NodeId,
        var: &mut PropType,
        mut object_flags: u64,
        no_arrays: bool,
        is_function: bool,
        hardcoded_name: Option<&str>,
        hint_text: &str,
        category: Option<String>,
        skip: bool,
    ) -> CResult<Option<NodeId>> {
        let scope_kind = self.model.node(scope).kind.clone();
        let var_name = match hardcoded_name {
            Some(n) => n.to_string(),
            None => {
                let Some(t) = self.get_identifier(false)? else { return self.error("Missing variable name") };
                t.ident
            }
        };
        if !skip {
            let mut occ = 0;
            let existing = self.res().find_field(Some(FieldRef::Local(scope)), &var_name, true, Some(&mut occ));
            if let Some(e) = existing {
                if self.res().outer_of(e) == Some(FieldRef::Local(scope)) {
                    return self.error(format!("{hint_text}: '{var_name}' already defined"));
                }
                let scope_ok = matches!(scope_kind, NodeKind::Function | NodeKind::Class | NodeKind::State);
                let e_outer_kind = self.res().outer_of(e).map(|o| self.res().kind_of(o));
                if scope_ok && !self.res().is_function(e) && matches!(e_outer_kind, Some(FieldKind::Class) | Some(FieldKind::State)) {
                    let on = self.res().outer_of(e).map(|o| self.res().name_of(o)).unwrap_or_default();
                    let w = if occ > 0 { "'within' class " } else { "" };
                    if self.res().kind_of(e) == FieldKind::ScriptStruct {
                        self.warn(format!("{hint_text}: '{var_name}' conflicts with struct defined in {w}'{on}'"));
                    } else if !self.res().is_property(e) || (self.res().prop_flags(e) & cpf::DEPRECATED == 0 && var.flags & cpf::DEPRECATED == 0) {
                        self.warn(format!("{hint_text}: '{var_name}' conflicts with previously defined field in {w}'{on}'"));
                    }
                }
            }
        }
        if self.match_symbol("[")? {
            if no_arrays {
                return self.error("Arrays aren't allowed in this context");
            }
            if var.array_dim == 0 {
                return self.error("Static arrays of dynamic arrays are not allowed");
            }
            if var.cpt == Cpt::Bool {
                return self.error("Bool arrays are not allowed");
            }
            if !self.peek_symbol("]")? {
                match self.get_const_int(Some(FieldRef::Local(scope)))? {
                    Some(v) => var.array_dim = v,
                    None => {
                        let Some(my) = self.get_identifier(false)? else {
                            return self.error(format!("{hint_text} {var_name}: Bad or missing array size"));
                        };
                        let mut array_index: i32 = -1;
                        let existing = self.res().find_field(Some(FieldRef::Local(scope)), &my.ident, true, Some(&mut 0));
                        if let Some(c) = existing.filter(|f| self.res().kind_of(*f) == FieldKind::Const) {
                            let v = self.res().const_value(c);
                            array_index = v.trim().parse::<i32>().unwrap_or(0);
                            var.array_dim = array_index;
                        } else {
                            let e = existing.filter(|f| self.res().kind_of(*f) == FieldKind::Enum).or_else(|| self.res().find_enum_any(&my.ident));
                            if let Some(e) = e {
                                let names = self.res().enum_names(e);
                                if self.match_symbol(".")? {
                                    let Some(tag) = self.get_token_hint(None, false)? else {
                                        return self.error(format!("Missing enum tag after '{}'", self.res().name_of(e)));
                                    };
                                    if tag.ident.eq_ignore_ascii_case("EnumCount") {
                                        array_index = names.len() as i32 - 1;
                                    } else {
                                        match names.iter().position(|n| n.eq_ignore_ascii_case(&tag.ident)) {
                                            Some(i) if !tag.ident.eq_ignore_ascii_case("None") => array_index = i as i32,
                                            _ => return self.error(format!("Missing enum tag after '{}'", self.res().name_of(e))),
                                        }
                                    }
                                    var.array_dim = array_index;
                                } else {
                                    array_index = names.len() as i32 - 1;
                                    var.array_dim = array_index;
                                }
                                var.array_index_enum = Some(e);
                            }
                        }
                        if array_index == -1 {
                            return self.error(format!("{hint_text} {var_name}: Bad or missing array size"));
                        }
                    }
                }
                if var.array_dim <= 1 || var.array_dim > MAX_ARRAY_SIZE {
                    return self.error(format!("{hint_text} {var_name}: Illegal array size {}", var.array_dim));
                }
            }
            if !self.match_symbol("]")? {
                return self.error(format!("{hint_text} {var_name}: Missing ']'"));
            }
        } else if self.match_symbol("(")? {
            return self.error("Use [] for arrays, not ()");
        }
        let is_function_parm = is_function && !no_arrays;
        let is_function_local = !is_function_parm && scope_kind == NodeKind::Function;
        let is_operator_parm = is_function_parm && skip;
        let is_state_local = !is_function_parm && scope_kind == NodeKind::State;
        let mut export_text: Option<String> = None;
        if !is_operator_parm && !is_function_local && !is_state_local {
            let decl = self.location();
            if let Some(text) = self.parse_property_export_text(&var_name)? {
                if !is_function_parm && !self.class_has(cf::NATIVE) {
                    self.return_to(decl, false, true);
                    return self.error("Export-text may only be used in native classes");
                }
                let fflags = if scope_kind == NodeKind::Function { self.model.node(scope).func.flags } else { 0 };
                let owner_native = self.class_has(cf::NATIVE);
                if var.flags & cpf::NATIVE == 0
                    && !self.class_has(cf::TRANSIENT)
                    && (!is_function_parm
                        || (fflags & (func::NATIVE | func::DELEGATE | func::EVENT)) == 0
                        || ((fflags & (func::NATIVE | func::DELEGATE | func::EVENT)) == func::EVENT && !owner_native))
                {
                    self.return_to(decl, false, true);
                    if is_function_parm {
                        return self.error("Only native functions, delegates, and events may specify export-text.");
                    }
                    return self.error("You must mark this property 'native' in order to specify export-text");
                }
                if var.cpt != Cpt::Struct {
                    self.return_to(decl, false, true);
                    return self.error("Currently only struct properties support export-text on a per-property basis");
                }
                export_text = Some(text);
            }
            if !is_function_parm {
                self.parse_property_meta_data(&var_name)?;
            }
        }
        if !skip && is_state_local {
            self.model.node_mut(scope).state.state_flags |= sf::HAS_LOCALS;
        }
        if scope_kind != NodeKind::Function && scope_kind != NodeKind::State {
            self.original_variable_index += 1;
        }
        if skip {
            return Ok(None);
        }
        let mut prev = self.own_properties(scope).last().copied();
        let mut array_node: Option<NodeId> = None;
        let mut new_scope = scope;
        if var.array_dim == 0 {
            let mut a = Node::new(NodeKind::Property(PropertyKind::Array), &var_name, Some(scope));
            a.object_flags = object_flags;
            let id = self.model.add(a);
            array_node = Some(id);
            new_scope = id;
            var.array_dim = 1;
            object_flags = rf::PUBLIC;
        }
        let class_class = self.class_class();
        let (kind, ref1, ref2) = match var.cpt {
            Cpt::Map => {
                var.array_dim = 1;
                (PropertyKind::Map, None, None)
            }
            Cpt::Byte => {
                if array_node.is_none() {
                    prev = self.group_property(scope, prev, var.flags, PropertyKind::Byte);
                }
                (PropertyKind::Byte, var.inner, None)
            }
            Cpt::Int => (PropertyKind::Int, None, None),
            Cpt::Bool => {
                if array_node.is_none() {
                    prev = self.group_property(scope, prev, var.flags, PropertyKind::Bool);
                }
                (PropertyKind::Bool, None, None)
            }
            Cpt::Float => (PropertyKind::Float, None, None),
            Cpt::Object => {
                let pc = var.inner.expect("object class");
                if Some(pc) == class_class {
                    (PropertyKind::Class, Some(pc), var.meta_class)
                } else if var.safe {
                    (PropertyKind::SafeObject, Some(pc), None)
                } else if self.res().is_component_class(pc) {
                    var.flags |= cpf::EDIT_INLINE;
                    (PropertyKind::Component, Some(pc), None)
                } else {
                    (PropertyKind::Object, Some(pc), None)
                }
            }
            Cpt::Interface => (PropertyKind::Interface, var.inner, None),
            Cpt::Name => (PropertyKind::Name, None, None),
            Cpt::String => (PropertyKind::Str, None, None),
            Cpt::Struct => (PropertyKind::Struct, var.inner, None),
            Cpt::Delegate => (PropertyKind::Delegate, var.inner, None),
            other => return self.error(format!("Unknown property type {}", other as u8)),
        };
        let mut n = Node::new(NodeKind::Property(kind), &var_name, Some(new_scope));
        n.object_flags = object_flags;
        n.prop.ref1 = ref1;
        n.prop.ref2 = ref2;
        n.prop.delegate_name = var.delegate_name.clone();
        n.prop.export_text = export_text.clone();
        n.prop.array_dim = var.array_dim;
        n.prop.flags = var.flags;
        // Only the outer (array) property carries the category.
        n.prop.category = if array_node.is_some() { None } else { category.clone() };
        n.prop.array_size_enum = var.array_index_enum;
        let mut new_property = self.model.add(n);
        if let Some(a) = array_node {
            self.model.node_mut(new_property).prop.flags = var.flags & cpf::PROPAGATE_TO_ARRAY_INNER;
            let an = self.model.node_mut(a);
            an.prop.ref1 = Some(FieldRef::Local(new_property));
            an.prop.array_dim = var.array_dim;
            an.prop.flags = var.flags;
            an.prop.category = category;
            an.prop.array_size_enum = var.array_index_enum;
            an.prop.delegate_name = var.delegate_name.clone();
            an.prop.export_text = export_text;
            new_property = a;
        }
        self.insert_child_after(scope, prev, new_property);
        Ok(Some(new_property))
    }

    // ----- declarations ----------------------------------------------------------

    fn parse_editable_category(&mut self, default_name: &str, tag: &str) -> CResult<(u64, Option<String>)> {
        let mut ed_category: Option<String> = None;
        let mut ed_flags = 0u64;
        if self.match_symbol("(")? {
            ed_flags |= cpf::EDIT;
            let mut path = String::new();
            loop {
                if let Some(c) = self.get_identifier(true)? {
                    if !path.is_empty() {
                        path.push(',');
                    }
                    path.push_str(&c.ident);
                } else {
                    ed_category = Some(default_name.to_string());
                    break;
                }
                if !self.match_symbol(",")? {
                    break;
                }
            }
            self.require_symbol(")", tag)?;
            if ed_category.is_none() {
                ed_category = Some(path);
            }
        }
        Ok((ed_flags, ed_category))
    }

    /// `CompileVariableDeclaration`.
    fn compile_variable_declaration(&mut self, token: &Token, need_semicolon: &mut bool) -> CResult<()> {
        let disallow: u64;
        if token.matches_ident("var") {
            self.check_allow("'Var'", allow::VAR_DECL)?;
            // The game's engine also has state-scoped variables (`EX_StateVariable`).
            if !matches!(self.top_nest_ref().nest_type, NestType::Class | NestType::State) {
                return self.error("Instance variables are only allowed at class scope (use 'local'?)");
            }
            disallow = cpf::PARM_FLAGS;
        } else {
            self.check_allow("'Local'", allow::VAR_DECL)?;
            if self.top_nest_ref().nest_type == NestType::Class {
                return self.error("Local variables are only allowed in functions");
            }
            disallow = !0;
        }
        let class_name = self.class_node().name.clone();
        let (ed_flags, mut ed_category) = self.parse_editable_category(&class_name, "Missing ')' after editable category")?;
        let top = self.top_node_id()?;
        let mut original = PropType::none();
        let mut object_flags = 0u64;
        self.get_var_type(top, &mut original, &mut object_flags, disallow, Some("Variable declaration"), None)?;
        original.flags |= ed_flags;
        if original.flags & cpf::EDIT != 0 && ed_category.is_none() {
            ed_category = Some(class_name);
        }
        if original.flags & (cpf::TRANSIENT | cpf::NATIVE) != 0 && self.top_nest_ref().nest_type != NestType::Class {
            return self.error("Static and local variables may not be transient or native");
        }
        if original.flags & cpf::PARM_FLAGS != 0 {
            return self.error("Illegal type modifiers in variable");
        }
        loop {
            let mut property = original.clone();
            self.get_var_name_and_dim(top, &mut property, object_flags, false, false, None, "Variable declaration", ed_category.clone(), false)?;
            if !self.match_symbol(",")? {
                break;
            }
        }
        self.require_symbol(";", "'var'")?;
        *need_semicolon = false;
        Ok(())
    }

    /// `CompileEnum`.
    pub fn compile_enum(&mut self, scope: NodeId, _need_semicolon: &mut bool) -> CResult<FieldRef> {
        let nt = self.top_nest_ref().nest_type;
        if nt != NestType::Class && nt != NestType::Interface {
            return self.error("Enums can only be declared at class scope");
        }
        let decl = self.location();
        let Some(enum_tok) = self.get_identifier(false)? else { return self.error("Missing enumeration name") };
        if self.find_own_field(scope, &enum_tok.ident).is_some() {
            return self.error(format!("enum: '{}' already defined here", enum_tok.ident));
        }
        if self.parse_property_meta_data(&enum_tok.ident).is_err() {
            self.return_to(decl, false, true);
            return self.error("Metadata was not formatted properly");
        }
        self.require_symbol("{", "'Enum'")?;
        let mut names: Vec<String> = Vec::new();
        let mut tag_locations = Vec::new();
        while let Some(tag) = self.get_identifier(false)? {
            tag_locations.push(self.location());
            if names.iter().any(|n| n.eq_ignore_ascii_case(&tag.ident)) {
                return self.error(format!("Duplicate enumeration tag {}", tag.ident));
            }
            if names.len() < 256 {
                names.push(tag.ident.clone());
            } else {
                return self.error("Exceeded maximum of 255 enumerators");
            }
            self.parse_property_meta_data(&tag.ident)?;
            if !self.match_symbol(",")? {
                break;
            }
        }
        if names.is_empty() {
            return self.error("Enumeration must contain at least one enumerator");
        }
        self.require_symbol("}", "'Enum'")?;
        // SetEnums / GenerateMaxEnum
        let mut prefix = generate_enum_prefix(&names);
        if prefix.is_empty() {
            // No common `Prefix_` among the tags: the enum's own name.
            prefix = enum_tok.ident.clone();
        }
        let max_name = format!("{prefix}_MAX");
        let max_idx = names.iter().position(|n| n.eq_ignore_ascii_case(&max_name));
        match max_idx {
            // `GenerateMaxEnum`: only the exact `<Prefix>_MAX` counts; `TGPIDX_MAX` before a
            // `TG_PAWN_PROPERTIES_MAX` (prefix broken by misspelt tags) is an ordinary tag.
            None => names.push(max_name),
            Some(i) if i != names.len() - 1 => {
                self.return_to(tag_locations[i], false, true);
                return self.error(format!("Illegal enumeration tag specified.  Conflicts with auto-generated tag '{max_name}'"));
            }
            _ => {}
        }
        let mut n = Node::new(NodeKind::Enum, &enum_tok.ident, Some(scope));
        n.object_flags = rf::PUBLIC;
        n.enum_names = names;
        let id = self.model.add(n);
        self.prepend_child(scope, id);
        Ok(FieldRef::Local(id))
    }

    /// `CompileStruct`.
    pub fn compile_struct(&mut self, scope: NodeId, _need_semicolon: &mut bool) -> CResult<FieldRef> {
        let nt = self.top_nest_ref().nest_type;
        if nt != NestType::Class && nt != NestType::Interface {
            return self.error("Structs can only be declared in class or struct scope");
        }
        let decl = self.location();
        if self.parse_property_export_text("struct")?.is_some() {
            if self.peek_symbol(";")? {
                self.return_to(decl, false, true);
                return self.error("Missing struct name");
            }
            if !self.class_has(cf::NATIVE) {
                self.return_to(decl, false, true);
                return self.error("Export text should only be used in native classes");
            }
        }
        let mut struct_flags = 0u32;
        let struct_tok;
        loop {
            let Some(t) = self.get_identifier(false)? else { return self.error("Missing struct name") };
            match t.ident.to_ascii_lowercase().as_str() {
                "native" => struct_flags |= stf::NATIVE,
                "export" => struct_flags |= stf::EXPORT,
                "transient" => struct_flags |= stf::TRANSIENT,
                "atomic" => struct_flags |= stf::ATOMIC,
                "immutable" => struct_flags |= stf::IMMUTABLE | stf::ATOMIC,
                "immutablewhencooked" => struct_flags |= stf::IMMUTABLE_WHEN_COOKED | stf::ATOMIC_WHEN_COOKED,
                "strictconfig" => struct_flags |= stf::STRICT_CONFIG,
                _ => {
                    struct_tok = t;
                    break;
                }
            }
        }
        let struct_name = struct_tok.ident.clone();
        if self.find_own_field(scope, &struct_name).is_some() {
            return self.error(format!("struct: '{struct_name}' already defined here"));
        } else if self.res().find_class(&struct_name).is_some() {
            return self.error(format!("struct: '{struct_name}' conflicts with class name"));
        }
        let mut base_struct: Option<FieldRef> = None;
        if self.match_identifier("extends")? {
            let Some(parent_scope) = self.get_identifier(false)? else { return self.error("'struct': Missing parent struct after 'Extends'") };
            if !self.match_symbol(".")? {
                let f = self.res().find_field(Some(FieldRef::Local(scope)), &parent_scope.ident, true, None);
                match f {
                    Some(f) if self.res().kind_of(f) == FieldKind::ScriptStruct => base_struct = Some(f),
                    _ => return self.error(format!("'struct': Can't find parent struct '{}'", parent_scope.ident)),
                }
            } else {
                let Some(parent_name) = self.get_identifier(false)? else {
                    return self.error(format!("'struct': Missing parent struct type after '{}.'", parent_scope.ident));
                };
                let Some(tc) = self.res().find_class(&parent_scope.ident) else {
                    return self.error(format!("'struct': Can't find parent struct class '{}'", parent_scope.ident));
                };
                let f = self.res().find_field(Some(tc), &parent_name.ident, true, None);
                match f {
                    Some(f) if self.res().kind_of(f) == FieldKind::ScriptStruct => base_struct = Some(f),
                    _ => return self.error(format!("'struct': Can't find parent struct class '{}.{}'", parent_scope.ident, parent_name.ident)),
                }
            }
        }
        if let Some(b) = base_struct {
            struct_flags |= self.res().struct_flags(b) & stf::INHERIT;
        }
        let mut n = Node::new(NodeKind::ScriptStruct, &struct_name, Some(scope));
        n.object_flags = rf::PUBLIC;
        n.strct.super_struct = base_struct;
        n.strct.struct_flags = struct_flags;
        let st = self.model.add(n);
        self.prepend_child(scope, st);
        self.require_symbol("{", "'struct'")?;
        loop {
            let Some(token) = self.get_token_hint(None, false)? else { return self.error("'struct': Unexpected end of file") };
            if token.matches_ident("struct") {
                let mut inner = true;
                self.compile_struct(st, &mut inner)?;
                if inner {
                    self.require_symbol(";", "'struct'")?;
                }
            } else if token.matches_ident("const") {
                let mut inner = true;
                self.compile_const(st, &mut inner)?;
                if inner {
                    self.require_symbol(";", "'const'")?;
                }
            } else if token.matches_ident("var") {
                let (ed_flags, mut ed_category) = self.parse_editable_category(&struct_name, "Editable 'struct' member variable")?;
                let mut object_flags = 0u64;
                let mut original = PropType::none();
                self.get_var_type(st, &mut original, &mut object_flags, cpf::PARM_FLAGS, Some("'struct' member variable"), None)?;
                original.flags |= ed_flags;
                if original.flags & cpf::EDIT != 0 && ed_category.is_none() {
                    ed_category = Some(struct_name.clone());
                }
                if original.flags & cpf::PARM_FLAGS != 0 {
                    return self.error("Illegal type modifiers in variable");
                }
                loop {
                    let mut property = original.clone();
                    let np = self.get_var_name_and_dim(st, &mut property, object_flags, false, false, None, "Variable declaration", ed_category.clone(), false)?;
                    if let Some(np) = np {
                        if self.model.node(np).prop.flags & cpf::COMPONENT != 0 {
                            self.model.node_mut(st).strct.struct_flags |= stf::HAS_COMPONENTS;
                        }
                        if self.model.node(st).strct.struct_flags & stf::TRANSIENT != 0 {
                            self.model.node_mut(np).prop.flags |= cpf::ALWAYS_INIT;
                        }
                    }
                    if !self.match_symbol(",")? {
                        break;
                    }
                }
                self.require_symbol(";", "'struct'")?;
            } else if token.matches_ident("structdefaultproperties") {
                // Capture the block text (comments stripped, with a line number header).
                let line = self.lexer_line;
                self.require_symbol("{", "structdefaultproperties")?;
                let body = self.raw_block()?;
                let text = format!("linenumber={}\r\n{}", line, strip_comments(&body));
                self.model.node_mut(st).class.default_props_text = Some((text, line));
            } else if token.matches_ident("structcpptext") {
                if !self.class_has(cf::NATIVE) {
                    return self.error("Cannot use cpptext blocks in non-native classes");
                }
                self.require_symbol("{", "'structcpptext'")?;
                let body = self.raw_block()?;
                if !body.is_empty() {
                    if self.model.node(st).strct.cpp_text.is_some() {
                        return self.error(format!("Multiple structcpptext definitions for '{struct_name}'"));
                    }
                    self.model.node_mut(st).strct.cpp_text = Some(body);
                }
            } else if token.is_symbol("}") {
                break;
            } else {
                return self.error(format!("'struct': Unexpected '{}'", token.ident));
            }
        }
        let (size, al) = self.link_struct(FieldRef::Local(st));
        if size < al && struct_flags & stf::NATIVE != 0 {
            self.warn(format!("{struct_name} must contain at least {al} bytes (currently only contains {size})"));
        }
        Ok(FieldRef::Local(st))
    }

    /// `CompileConst`.
    pub fn compile_const(&mut self, scope: NodeId, _need_semicolon: &mut bool) -> CResult<()> {
        let Some(ct) = self.get_identifier(false)? else { return self.error("Missing constant name") };
        if self.find_own_field(scope, &ct.ident).is_some() {
            return self.error(format!("const: '{}' already defined", ct.ident));
        }
        self.require_symbol("=", "'const'")?;
        let start = self.lexer_pos;
        let Some(vt) = self.get_token_hint(None, false)? else { return self.error(format!("const {}: Missing value", ct.ident)) };
        let value: String;
        if vt.kind != TokKind::Const {
            if vt.matches_ident("sizeof") {
                self.require_symbol("(", "'sizeof'")?;
                let Some(cn) = self.get_identifier(false)? else { return self.error("Missing class name") };
                let Some(c) = self.res().find_class(&cn.ident) else { return self.error(format!("Specified class name not found '{}'", cn.ident)) };
                self.require_symbol(")", "'sizeof'")?;
                value = self.struct_layout(c).0.to_string();
            } else {
                return self.error(format!("const {}: Value is not constant", ct.ident));
            }
        } else {
            let end = self.lexer_pos;
            let mut s = start;
            let b = &self.input;
            while s < end && (b[s] == b' ' || b[s] == b'\t') {
                s += 1;
            }
            value = super::engine::cp1252_string(&super::engine::latin1_string(&self.input[s..end]));
        }
        let mut n = Node::new(NodeKind::Const, &ct.ident, Some(scope));
        n.object_flags = rf::PUBLIC;
        n.const_value = value;
        let id = self.model.add(n);
        self.prepend_child(scope, id);
        Ok(())
    }

    /// `CompileFunctionDeclaration`.
    fn compile_function_declaration(&mut self, token: &mut Token, need_semicolon: &mut bool) -> CResult<()> {
        let mut nest_name: Option<&'static str> = None;
        let mut func_name_retry = self.location();
        let mut function_flags: u32 = func::PUBLIC;
        let mut precedence: i32 = 0;
        let mut inative: i32 = 0;
        let mut expect_parms: i32 = 0;
        let scope = self.top_node_id()?;
        let mut return_variable_object_flags: u64 = rf::PUBLIC;
        let mut delegate_property_flags: u64 = 0;
        let mut specified_unreliable = false;
        let mut specified_function_type = false;
        let mut tok = token.clone();
        loop {
            func_name_retry = self.location_at(&tok);
            let l = tok.ident.to_ascii_lowercase();
            let function_type_error = |flags: u32| -> String {
                let ft = if flags & func::DELEGATE != 0 {
                    "Delegate"
                } else if flags & func::EVENT != 0 {
                    "Event"
                } else {
                    "Function"
                };
                format!("Function type already declared as '{ft}'")
            };
            match l.as_str() {
                "function" => {
                    self.check_allow("'Function'", allow::FUNCTION)?;
                    nest_name = Some("function");
                    if specified_function_type {
                        return self.error(function_type_error(function_flags));
                    }
                    specified_function_type = true;
                    function_flags &= !func::DELEGATE;
                }
                "delegate" => {
                    self.check_allow("'Function'", allow::FUNCTION)?;
                    nest_name = Some("delegate");
                    if specified_function_type {
                        return self.error(function_type_error(function_flags));
                    }
                    specified_function_type = true;
                    function_flags |= func::DELEGATE;
                }
                "event" => {
                    self.check_allow("'Function'", allow::FUNCTION)?;
                    nest_name = Some("event");
                    if specified_function_type {
                        return self.error(function_type_error(function_flags));
                    }
                    specified_function_type = true;
                    function_flags |= func::EVENT;
                }
                "operator" => {
                    self.check_allow("'Operator'", allow::FUNCTION)?;
                    nest_name = Some("operator");
                    function_flags |= func::OPERATOR;
                    expect_parms = 3;
                    if !self.match_symbol("(")? {
                        return self.error("Missing '(' and precedence after 'Operator'");
                    }
                    match self.get_const_int(None)? {
                        None => return self.error("Missing precedence value"),
                        Some(p) if !(0..=255).contains(&p) => return self.error("Bad precedence value"),
                        Some(p) => precedence = p,
                    }
                    if !self.match_symbol(")")? {
                        return self.error("Missing ')' after operator precedence");
                    }
                }
                "preoperator" => {
                    self.check_allow("'PreOperator'", allow::FUNCTION)?;
                    nest_name = Some("preoperator");
                    expect_parms = 2;
                    function_flags |= func::OPERATOR | func::PRE_OPERATOR;
                }
                "postoperator" => {
                    self.check_allow("'PostOperator'", allow::FUNCTION)?;
                    nest_name = Some("postoperator");
                    function_flags |= func::OPERATOR;
                    expect_parms = 2;
                }
                "native" => {
                    if !self.class_has(cf::NATIVE) {
                        return self.error("Only functions of native classes can be declared native");
                    }
                    function_flags |= func::NATIVE;
                    if self.match_symbol("(")? {
                        let Some(v) = self.get_const_int(None)? else { return self.error("Missing native id") };
                        inative = v;
                        if !self.match_symbol(")")? {
                            return self.error("Missing ')' after internal id");
                        }
                    }
                }
                "static" => {
                    function_flags |= func::STATIC;
                    if self.node_kind_is(scope, NodeKind::State) {
                        return self.error("Static functions cannot exist in a state");
                    }
                }
                "simulated" => function_flags |= func::SIMULATED,
                "iterator" => function_flags |= func::ITERATOR,
                "singular" => function_flags |= func::SINGULAR,
                "latent" => function_flags |= func::LATENT,
                "exec" => {
                    function_flags |= func::EXEC;
                    if function_flags & func::NET != 0 {
                        return self.error("Exec functions cannot be replicated!");
                    }
                }
                "final" => {
                    function_flags |= func::FINAL;
                    if self.class_has(cf::INTERFACE) {
                        return self.error("Interface functions cannot be declared 'final'");
                    }
                }
                "server" => {
                    function_flags |= func::NET | func::NET_SERVER;
                    if function_flags & func::EXEC != 0 {
                        return self.error("Exec functions cannot be replicated!");
                    }
                }
                "client" => function_flags |= func::NET | func::NET_CLIENT,
                "reliable" => function_flags |= func::NET_RELIABLE,
                "unreliable" => specified_unreliable = true,
                "private" => {
                    if function_flags & func::DELEGATE == 0 {
                        function_flags &= !func::PUBLIC;
                        function_flags |= func::PRIVATE | func::FINAL;
                    } else {
                        return_variable_object_flags &= !(rf::PUBLIC | rf::PROTECTED);
                    }
                }
                "protected" => {
                    if function_flags & func::DELEGATE == 0 {
                        function_flags &= !func::PUBLIC;
                        function_flags |= func::PROTECTED;
                    } else {
                        return_variable_object_flags |= rf::PUBLIC | rf::PROTECTED;
                    }
                }
                "public" => {
                    if function_flags & func::DELEGATE == 0 {
                        function_flags |= func::PUBLIC;
                    } else {
                        return_variable_object_flags |= rf::PUBLIC;
                    }
                }
                "noexportheader" | "noexport" | "virtual" => {}
                "transient" => delegate_property_flags |= cpf::TRANSIENT,
                "dllimport" => {
                    if self.class_node().class.dll_bind_name.is_empty() {
                        return self.error("Only DLLBind classes can declare DLLImport functions");
                    }
                    function_flags |= func::DLL_IMPORT;
                }
                "pcomnetapproved" => function_flags |= func::PCOM_NET_APPROVED,
                "pcomservercmd" => function_flags |= func::PCOM_SERVER_CMD,
                _ => break,
            }
            let Some(t) = self.get_token_hint(None, false)? else { break };
            tok = t;
        }
        self.unget(&tok);
        let Some(nest_name) = nest_name else { return self.error("Missing 'function'") };
        if self.class_has(cf::NATIVE_ONLY) && function_flags & func::NATIVE == 0 {
            return self.error("Classes marked as 'nativeonly' cannot contain script functions.");
        }
        if function_flags & func::DELEGATE == 0 {
            if delegate_property_flags & cpf::TRANSIENT != 0 {
                return self.error(format!("'Transient' can only be used in delegate function declarations: {nest_name}"));
            }
            return_variable_object_flags = rf::PUBLIC;
        }
        if inative != 0 {
            if let Some(existing) = self.find_native_index(inative as u16) {
                return self.error(format!("Native function id '{inative}' specified for function '{nest_name}' is already assigned to '{existing}'"));
            }
        }
        let start = self.location();
        let mut return_value_flags: u64 = 0;
        let mut return_type = PropType::none();
        let mut has_return_value = false;
        let top = self.top_node_id()?;
        if self.get_identifier(true)?.is_some() {
            if !self.peek_symbol("(")? {
                self.return_to(start, true, true);
                let auto_cast_return = self.peek_identifier("coerce")?;
                has_return_value = self.get_var_type(top, &mut return_type, &mut return_value_flags, !cpf::COERCE_PARM, None, None)?;
                if auto_cast_return {
                    if function_flags & func::NATIVE == 0 {
                        return self.error("Only native functions can auto-cast their return types");
                    }
                    if has_return_value && return_type.flags & cpf::COERCE_PARM != 0 {
                        if return_type.cpt != Cpt::Object || return_type.meta_class.is_some() {
                            return self.error("The return value for auto-cast functions must be an object type");
                        }
                    } else {
                        return self.error("Auto-cast functions must return a object type");
                    }
                }
            } else {
                self.return_to(start, true, true);
            }
        }
        let func_ident = match self.get_identifier(false)? {
            Some(t) => t,
            None => {
                if function_flags & func::OPERATOR == 0 {
                    return self.error(format!("Missing {nest_name} name"));
                }
                match self.get_symbol_token()? {
                    Some(t) => t,
                    None => return self.error(format!("Missing {nest_name} name")),
                }
            }
        };
        if !self.match_symbol("(")? {
            return self.error(format!("Bad {nest_name} definition"));
        }
        if function_flags & func::NATIVE != 0 {
            if inative != 0 && function_flags & func::FINAL == 0 {
                return self.error("Numbered native functions must be final");
            }
        } else {
            if function_flags & func::LATENT != 0 {
                return self.error("Only native functions may use 'Latent'");
            }
            if function_flags & func::ITERATOR != 0 {
                return self.error("Only native functions may use 'Iterator'");
            }
        }
        if function_flags & func::DLL_IMPORT != 0 {
            if function_flags & func::FINAL == 0 {
                return self.error("DLLImport functions must be final");
            }
            if function_flags & func::EXEC != 0 {
                return self.error("DLLImport functions cannot be declared exec");
            }
            if function_flags & func::STATIC != 0 {
                return self.error("DLLImport functions cannot be declared static");
            }
        }
        if function_flags & func::NET != 0 {
            if function_flags & func::DLL_IMPORT != 0 {
                return self.error("DLLImport functions can't be replicated");
            } else if function_flags & func::STATIC != 0 {
                return self.error("Static functions can't be replicated");
            } else if function_flags & func::NET_RELIABLE == 0 && !specified_unreliable {
                return self.error("Replicated function: 'reliable' or 'unreliable' is required");
            } else if function_flags & func::NET_RELIABLE != 0 && specified_unreliable {
                return self.error("'reliable' and 'unreliable' are mutually exclusive");
            }
        } else if function_flags & func::NET_RELIABLE != 0 {
            return self.error("'reliable' specified without 'client' or 'server'");
        } else if specified_unreliable {
            return self.error("'unreliable' specified without 'client' or 'server'");
        }
        // Signature.
        let mut signature = String::new();
        if function_flags & func::OPERATOR != 0 {
            for b in func_ident.ident.bytes() {
                if (32..128).contains(&b) {
                    signature.push_str(CPP_TAGS[(b - 32) as usize]);
                }
            }
            let retry = self.location();
            if !self.match_symbol(")")? {
                signature.push('_');
                if function_flags & func::PRE_OPERATOR != 0 {
                    signature.push_str("Pre");
                }
                loop {
                    let mut property = PropType::none();
                    let mut of = 0u64;
                    self.get_var_type(top, &mut property, &mut of, !(cpf::PARM_FLAGS | cpf::CONST | cpf::ALWAYS_INIT), Some("Function parameter"), None)?;
                    self.get_var_name_and_dim(top, &mut property, of, false, true, None, "Function parameter", None, true)?;
                    if property.is_object() || property.cpt == Cpt::Struct {
                        let n = property.inner.map(|c| self.res().name_of(c)).unwrap_or_default();
                        signature.push_str(&n);
                    } else {
                        signature.push_str(cpt_signature_name(property.cpt));
                    }
                    if !self.match_symbol(",")? {
                        break;
                    }
                }
                self.require_symbol(")", "parameter list")?;
            }
            self.return_to(retry, true, true);
        } else {
            signature = func_ident.ident.clone();
        }
        self.push_nest(NestType::Function, &signature, None)?;
        let top_function = self.top_node_id()?;
        {
            let f = self.model.node_mut(top_function);
            f.func.flags |= function_flags;
            f.func.precedence = precedence as u8;
            f.func.native_index = inative as u16;
            f.func.friendly_name = func_ident.ident.clone();
            f.line = func_ident.line;
        }
        if !self.match_symbol(")")? {
            let mut encountered_optional = false;
            loop {
                let mut property = PropType::none();
                let mut of = 0u64;
                self.get_var_type(top_function, &mut property, &mut of, !(cpf::PARM_FLAGS | cpf::CONST | cpf::ALWAYS_INIT), Some("Function parameter"), None)?;
                property.flags |= cpf::PARM;
                let prop = self
                    .get_var_name_and_dim(top_function, &mut property, of, false, true, None, "Function parameter", None, false)?
                    .expect("parameter");
                self.model.node_mut(top_function).func.num_parms += 1;
                let num_parms = self.model.node(top_function).func.num_parms;
                let class_class = self.class_class();
                if num_parms == 1 && has_return_value && return_type.flags & cpf::COERCE_PARM != 0 {
                    if property.cpt != Cpt::Object || property.inner != class_class {
                        return self.error("The first parameter of an auto-cast function must be of type 'class'");
                    } else if property.meta_class != return_type.inner {
                        return self.error("The first parameter of an auto-cast function must match the class of the return value");
                    }
                }
                let tf = self.model.node(top_function).func.flags;
                if function_flags & func::OPERATOR != 0 && property.flags & !(cpf::PARM_FLAGS | cpf::CONST | cpf::ALWAYS_INIT) != 0 {
                    return self.error("Operator parameters may not have modifiers");
                } else if property.cpt == Cpt::Bool && property.flags & cpf::OUT_PARM != 0 {
                    return self.error("Booleans may not be out parameters");
                } else if property.flags & cpf::SKIP_PARM != 0 && (tf & func::NATIVE == 0 || tf & func::OPERATOR == 0 || num_parms != 2) {
                    return self.error("Only parameter 2 of native operators may be 'Skip'");
                } else if tf & func::NET != 0 && property.flags & cpf::OUT_PARM != 0 {
                    return self.error("Replicated functions cannot contain out parameters");
                } else if tf & func::NET != 0 && self.model.node(prop).kind == NodeKind::Property(PropertyKind::Delegate) {
                    return self.error("Replicated functions cannot contain delegate parameters (this would be insecure)");
                }
                if property.flags & cpf::OUT_PARM != 0 {
                    self.model.node_mut(top_function).func.flags |= func::HAS_OUT_PARMS;
                }
                if self.match_symbol("=")? {
                    if self.model.node(prop).prop.flags & cpf::OUT_PARM != 0 {
                        return self.error(format!(
                            "{}: Not allowed to specify a default value for optional out parms ({})",
                            signature,
                            self.model.node(prop).name
                        ));
                    } else if tf & func::DLL_IMPORT != 0 {
                        return self.error(format!("{}: Not allowed to specify a default value for DLLImport function parm ({})", signature, self.model.node(prop).name));
                    }
                    self.model.node_mut(prop).prop.flags |= cpf::OPTIONAL_PARM;
                    let pos = self.lexer_pos;
                    let line = self.lexer_line;
                    self.model.node_mut(top_function).func.param_defaults.push((prop, pos, line));
                    let mut paren = 0;
                    while let Some(st) = self.get_token_hint(None, false)? {
                        if paren == 0 && (st.is_symbol(")") || st.is_symbol(",")) {
                            self.unget(&st);
                            break;
                        }
                        if st.is_symbol("(") {
                            paren += 1;
                        } else if st.is_symbol(")") {
                            paren -= 1;
                        }
                    }
                }
                if self.model.node(prop).prop.flags & cpf::OPTIONAL_PARM != 0 {
                    encountered_optional = true;
                    self.model.node_mut(top_function).func.flags |= func::HAS_OPTIONAL_PARMS;
                } else if encountered_optional {
                    return self.error("After an optional parameters, all other parameters must be optional");
                }
                if !self.match_symbol(",")? {
                    break;
                }
            }
            self.require_symbol(")", "parameter list")?;
        } else if has_return_value && return_type.flags & cpf::COERCE_PARM != 0 {
            return self.error("Auto-cast functions must specify a class for the first parameter");
        }
        if has_return_value {
            return_type.flags |= cpf::PARM | cpf::OUT_PARM | cpf::RETURN_PARM;
            self.get_var_name_and_dim(top_function, &mut return_type, return_value_flags, true, true, Some("ReturnValue"), "Function return type", None, false)?;
            self.model.node_mut(top_function).func.num_parms += 1;
        }
        if function_flags & func::OPERATOR != 0 {
            let n = self.model.node(top_function).func.num_parms;
            if n != expect_parms {
                return self.error(format!("{nest_name} must have {} parameters", expect_parms - 1));
            }
            if self.res().func_return(FieldRef::Local(top_function)).is_none() {
                return self.error("Operator must have a return value");
            }
            if function_flags & func::FINAL == 0 {
                return self.error("Operators must be declared as 'Final'");
            }
        }
        if self.model.node(top_function).func.flags & func::DELEGATE != 0 && !self.class_has(cf::INTERFACE) {
            let prev = self.own_properties(scope).last().copied();
            let mangled = format!("__{}__Delegate", func_ident.ident);
            let mut n = Node::new(NodeKind::Property(PropertyKind::Delegate), &mangled, Some(scope));
            n.prop.flags |= delegate_property_flags;
            n.object_flags = return_variable_object_flags;
            n.prop.ref1 = Some(FieldRef::Local(top_function));
            n.prop.array_dim = 1;
            let id = self.model.add(n);
            self.insert_child_after(scope, prev, id);
        }
        if self.match_identifier("const")? && function_flags & func::NATIVE == 0 {
            return self.error("'const' may only be used for native functions");
        }
        if self.peek_symbol(";")? {
            // declaration only
        } else {
            if function_flags & func::DLL_IMPORT != 0 {
                return self.error("DLLImport functions must not declare a function body");
            }
            self.require_symbol("{", nest_name)?;
            if !self.match_symbol("}")? {
                self.model.node_mut(top_function).func.flags |= func::DEFINED;
                self.model.node_mut(top_function).func.defined = true;
                if function_flags & func::NATIVE != 0 {
                    return self.error("Native functions may only be declared, not defined");
                }
            }
            *need_semicolon = false;
        }
        // Super function lookup.
        let tf_flags = self.model.node(top_function).func.flags;
        let tf_nparms = self.model.node(top_function).func.num_parms;
        let tf_has_ret = self.res().func_return(FieldRef::Local(top_function)).is_some();
        let tf_prec = self.model.node(top_function).func.precedence;
        let mut found = false;
        'outer: for i in (1..self.nests.len().saturating_sub(1)).rev() {
            let Some(node) = self.nests[i].node else { continue };
            let mut cur = Some(FieldRef::Local(node));
            let mut guard = 0;
            while let Some(st) = cur {
                let children = self.res().children(st);
                for function in children {
                    if !self.res().is_function(function) || function == FieldRef::Local(top_function) {
                        continue;
                    }
                    if !self.res().name_of(function).eq_ignore_ascii_case(&signature) {
                        continue;
                    }
                    let fflags = self.res().func_flags(function);
                    if fflags & func::PRIVATE != 0 {
                        self.model.node_mut(top_function).strct.super_struct = None;
                        found = true;
                        break 'outer;
                    }
                    if (fflags ^ tf_flags) & (func::OPERATOR | func::PRE_OPERATOR) != 0 {
                        continue;
                    }
                    let f_nparms = self.res().func_num_parms(function);
                    if self.res().func_precedence(function) != tf_prec && f_nparms == tf_nparms {
                        self.return_to(func_name_retry, true, true);
                        return self.error("Overloaded operator differs in precedence");
                    }
                    if tf_has_ret != self.res().func_return(function).is_some() {
                        self.return_to(func_name_retry, true, true);
                        return self.error(format!("Redefinition of '{nest_name} {}' differs from original: return value mismatch", func_ident.ident));
                    } else if tf_nparms != f_nparms {
                        self.return_to(func_name_retry, true, true);
                        return self.error(format!("Redefinition of '{nest_name} {}' differs from original; different number of parameters", func_ident.ident));
                    }
                    let cur_parms = self.res().func_parms(FieldRef::Local(top_function));
                    let sup_parms = self.res().func_parms(function);
                    let mut count = 0;
                    let mut mismatch = false;
                    for (cp, sp) in cur_parms.iter().zip(sup_parms.iter()) {
                        if count >= f_nparms {
                            break;
                        }
                        let ct = self.res().prop_type(*cp, RefType::None);
                        let st = self.res().prop_type(*sp, RefType::None);
                        if !self.matches_type(&ct, &st, true, false) {
                            if ct.flags & cpf::RETURN_PARM != 0 {
                                self.return_to(func_name_retry, true, true);
                                return self.error(format!("Redefinition of {nest_name} {} differs only by return type", func_ident.ident));
                            } else if function_flags & func::OPERATOR == 0 {
                                self.return_to(func_name_retry, true, true);
                                return self.error(format!("Redefinition of '{nest_name} {}' differs from original", func_ident.ident));
                            }
                            mismatch = true;
                            break;
                        } else if (ct.flags & cpf::OUT_PARM != 0) != (st.flags & cpf::OUT_PARM != 0) {
                            self.return_to(func_name_retry, true, true);
                            return self.error(format!(
                                "Redefinition of '{nest_name} {}' differs from original - 'out' mismatch on parameter {}",
                                func_ident.ident,
                                count + 1
                            ));
                        } else if (ct.flags & cpf::OPTIONAL_PARM != 0) != (st.flags & cpf::OPTIONAL_PARM != 0) {
                            self.return_to(func_name_retry, true, true);
                            return self.error(format!(
                                "Redefinition of '{nest_name} {}' differs from original - 'optional' mismatch on parameter {}",
                                func_ident.ident,
                                count + 1
                            ));
                        }
                        count += 1;
                    }
                    if mismatch || count < tf_nparms {
                        continue;
                    }
                    if (function_flags & func::NET_FUNC_FLAGS) != (fflags & func::NET_FUNC_FLAGS) {
                        return self.error(format!("Redefinition of replication conditions for function '{}'", func_ident.ident));
                    }
                    if self.res().outer_of(function) == Some(FieldRef::Local(top_function)).and_then(|_| Some(FieldRef::Local(top_function))) {
                        // unreachable: a function is never its own outer
                    }
                    if self.res().outer_of(function) == self.res().outer_of(FieldRef::Local(top_function)) {
                        self.return_to(func_name_retry, true, true);
                        return self.error(format!("Duplicate function '{}'", self.res().name_of(function)));
                    } else if fflags & func::FINAL != 0 {
                        self.return_to(func_name_retry, true, true);
                        return self.error(format!("{}: Can't override a 'final' function", self.res().name_of(function)));
                    }
                    // The game's packages override `exec` functions without the specifier.
                    let check = func::FUNC_OVERRIDE_MATCH & !func::EXEC;
                    if (fflags & check) != (function_flags & check) {
                        return self.error(format!("Function '{}' specifiers differ from original", self.res().name_of(function)));
                    }
                    if fflags & func::SIMULATED != 0 && function_flags & func::SIMULATED == 0 {
                        self.warn(format!("Superclass version is simulated so '{}' should be!", func_ident.ident));
                    }
                    self.model.node_mut(top_function).strct.super_struct = Some(function);
                    found = true;
                    break 'outer;
                }
                cur = self.res().inheritance_super(st);
                guard += 1;
                if guard > 100 {
                    break;
                }
            }
        }
        let _ = found;
        let tf = self.model.node(top_function);
        if tf.func.flags & func::NET != 0 && tf.strct.super_struct.is_none() && !self.node_kind_is(scope, NodeKind::Class) {
            return self.error(format!("Function '{}': Base implementation of RPCs cannot be in a state. Add a stub outside state scope.", signature));
        }
        if self.model.node(top_function).func.flags & func::DEFINED == 0 {
            self.pop_nest(NestType::Function, nest_name)?;
        }
        Ok(())
    }

    /// Location of a token (for retries).
    fn location_at(&self, t: &Token) -> super::engine::Location {
        let mut l = self.location();
        l.pos = t.start;
        l.line = t.line;
        l
    }

    /// Any loaded or local function with this native index.
    fn find_native_index(&self, idx: u16) -> Option<String> {
        for (i, n) in self.model.nodes.iter().enumerate() {
            if n.kind == NodeKind::Function && n.func.native_index == idx && n.func.native_index != 0 {
                return Some(self.model.path(i));
            }
        }
        self.ctx.native_function(idx).map(|r| self.ctx.full_path(r))
    }

    /// `CompileStateDeclaration`.
    fn compile_state_declaration(&mut self, token: &Token, need_semicolon: &mut bool) -> CResult<()> {
        self.check_allow("'State'", allow::STATE)?;
        let mut state_flags = 0u32;
        let mut got_state = false;
        let mut tok = token.clone();
        loop {
            if tok.matches_ident("state") {
                got_state = true;
                if self.match_symbol("(")? {
                    self.require_symbol(")", "'State'")?;
                    state_flags |= sf::EDITABLE;
                }
            } else if tok.matches_ident("simulated") {
                state_flags |= sf::SIMULATED;
            } else if tok.matches_ident("auto") {
                state_flags |= sf::AUTO;
            } else {
                self.unget(&tok);
                break;
            }
            let Some(t) = self.get_token_hint(None, false)? else { break };
            tok = t;
        }
        if !got_state {
            return self.error("Missing 'State'");
        }
        let Some(name_tok) = self.get_identifier(false)? else { return self.error("Missing state name") };
        let top = self.top_node_id()?;
        let mut parent_state = match self.res().find_field(Some(FieldRef::Local(top)), &name_tok.ident, true, None) {
            Some(f) if self.res().kind_of(f) == FieldKind::State => Some(f),
            Some(f) => {
                return self.error(format!("'state': expecting State, got {}", kind_name(self.res().kind_of(f))));
            }
            None => None,
        };
        if let Some(ps) = parent_state {
            if self.res().owner_class(ps) == Some(self.class_ref()) {
                return self.error(format!("Duplicate state '{}'", name_tok.ident));
            }
        }
        if self.match_identifier("extends")? {
            if parent_state.is_some() {
                return self.error(format!("'Extends' not allowed here: state '{}' overrides version in parent class", name_tok.ident));
            }
            let Some(pt) = self.get_identifier(false)? else { return self.error("Missing parent state name") };
            parent_state = match self.res().find_field(Some(FieldRef::Local(top)), &pt.ident, true, None) {
                Some(f) if self.res().kind_of(f) == FieldKind::State => Some(f),
                Some(f) => return self.error(format!("'state': expecting State, got {}", kind_name(self.res().kind_of(f)))),
                None => return self.error(format!("'extends': Parent state '{}' not found", pt.ident)),
            };
        }
        self.push_nest(NestType::State, &name_tok.ident, None)?;
        let st = self.top_node_id()?;
        {
            let n = self.model.node_mut(st);
            n.state.state_flags |= state_flags;
            n.strct.super_struct = parent_state;
            n.line = name_tok.line;
        }
        self.require_symbol("{", "'State'")?;
        *need_semicolon = false;
        self.supress_debug_info = -1;
        Ok(())
    }

    /// `CompileIgnoreDeclaration`.
    fn compile_ignore_declaration(&mut self, _token: &Token, _need_semicolon: &mut bool) -> CResult<()> {
        let mut ignored: Vec<String> = Vec::new();
        self.check_allow("'Ignores'", allow::IGNORES)?;
        loop {
            let Some(ig) = self.get_token_hint(None, false)? else { return self.error("'Ignores': Missing probe function name") };
            if ignored.iter().any(|n| n.eq_ignore_ascii_case(&ig.ident)) {
                return self.error(format!("'Ignores': Function '{}' specified multiple times", ig.ident));
            }
            ignored.push(ig.ident.clone());
            let mut found: Option<FieldRef> = None;
            // `Nest[NestLevel-2]` downwards: the class and its super classes, never the state.
            for i in (1..self.nests.len().saturating_sub(1)).rev() {
                let Some(node) = self.nests[i].node else { continue };
                let mut cur = Some(FieldRef::Local(node));
                let mut guard = 0;
                while let Some(st) = cur {
                    for f in self.res().children(st) {
                        if self.res().is_function(f) && self.res().name_of(f).eq_ignore_ascii_case(&ig.ident) {
                            found = Some(f);
                            break;
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                    cur = self.res().inheritance_super(st);
                    guard += 1;
                    if guard > 100 {
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            let Some(function) = found else { return self.error(format!("'Ignores': '{}' is not a function", ig.ident)) };
            let fflags = self.res().func_flags(function);
            if fflags & func::FINAL != 0 {
                return self.error(format!("'{}': Cannot ignore final functions", self.res().name_of(function)));
            }
            let fname = self.res().name_of(function);
            self.push_nest(NestType::Function, &fname, None)?;
            let tf = self.top_node_id()?;
            {
                let n = self.model.node_mut(tf);
                n.func.flags |= fflags & func::FUNC_OVERRIDE_MATCH;
                n.func.friendly_name = fname.clone();
                n.strct.super_struct = Some(function);
            }
            let nparms = self.res().func_num_parms(function);
            self.model.node_mut(tf).func.num_parms = nparms;
            let parms = self.res().func_parms(function);
            for p in parms {
                let pf = self.res().prop_flags(p);
                if pf & cpf::PARM == 0 {
                    break;
                }
                let kind = self.res().prop_kind(p).unwrap_or(PropertyKind::Int);
                let (r1, r2) = self.res().prop_refs(p);
                let kind = match kind {
                    PropertyKind::Map => return self.error("Using maps as function parameters is currently unsupported!"),
                    PropertyKind::Array => return self.error("Unknown property type ArrayProperty"),
                    // `CompileIgnores` recreates every object reference (components included)
                    // as a plain UObjectProperty.
                    PropertyKind::Component | PropertyKind::SafeObject => PropertyKind::Object,
                    k => k,
                };
                let mut n = Node::new(NodeKind::Property(kind), &self.res().name_of(p), Some(tf));
                n.object_flags = rf::PUBLIC;
                n.prop.ref1 = r1;
                n.prop.ref2 = if kind == PropertyKind::Class { r2 } else { None };
                n.prop.array_dim = self.res().prop_array_dim(p);
                n.prop.flags = pf;
                let id = self.model.add(n);
                self.model.node_mut(tf).strct.children.push(id);
                if pf & cpf::OPTIONAL_PARM != 0 {
                    self.model.node_mut(tf).func.flags |= func::HAS_OPTIONAL_PARMS;
                }
            }
            self.pop_nest(NestType::Function, "Ignores")?;
            if !self.match_symbol(",")? {
                break;
            }
        }
        Ok(())
    }

    // ----- nest popping (parse pass) ----------------------------------------------

    /// `PopNest` for both passes.
    pub fn pop_nest(&mut self, nest_type: NestType, descr: &str) -> CResult<()> {
        if let Some(top) = self.top_node {
            if self.model.node(top).strct.script_mem.len() > 65534 {
                return self.error(format!("Code space for {} overflowed by {} bytes", self.model.node(top).name, self.model.node(top).strct.script_mem.len() - 65534));
            }
        }
        if self.nests.is_empty() {
            return self.error(format!("Unexpected '{descr}' at global scope"));
        }
        let nest_type = if nest_type == NestType::None {
            self.top_nest_ref().nest_type
        } else {
            if self.top_nest_ref().nest_type != nest_type {
                return self.error(format!("Unexpected end of {descr} in '{}' block", nest_type_name(self.top_nest_ref().nest_type)));
            }
            nest_type
        };
        if self.pass == Pass::Parse {
            let top = self.top_node_id()?;
            match nest_type {
                NestType::State | NestType::Function => {
                    let pos = self.lexer_pos as i32;
                    let line = self.lexer_line as i32;
                    let n = self.model.node_mut(top);
                    n.strct.text_pos = pos;
                    n.strct.line = line;
                    if nest_type == NestType::Function {
                        let owner = FieldRef::Local(self.class);
                        self.fixup_delegate_properties(FieldRef::Local(top), owner)?;
                    }
                }
                NestType::Class => {
                    let owner = FieldRef::Local(self.class);
                    self.fixup_delegate_properties(FieldRef::Local(top), owner)?;
                    self.check_interface_implementations()?;
                }
                NestType::Interface => {
                    let owner = FieldRef::Local(self.class);
                    self.fixup_delegate_properties(FieldRef::Local(top), owner)?;
                }
                other => return self.error(format!("Bad first pass NestType {}", other as u8)),
            }
            self.link_struct(FieldRef::Local(top));
        } else {
            self.pop_nest_compile(nest_type)?;
            if self.nests.last().map(|n| n.nest_type == NestType::If && n.chain_placeholder.is_none() && false).unwrap_or(false) {
                // placeholder
            }
            // `else` handling returns early inside pop_nest_compile; detect via flag.
            if self.else_continued {
                self.else_continued = false;
                return Ok(());
            }
        }
        if self.top_nest_ref().chain_placeholder.is_some() {
            return self.error("Internal error: unresolved chain placeholder");
        }
        let popped = self.nests.pop().expect("nest");
        let _ = popped;
        self.top_node = self.nests.last().and_then(|n| n.node);
        if nest_type == NestType::Function || nest_type == NestType::State {
            if let Some(n) = self.nests.last_mut() {
                n.allow &= !(allow::VAR_DECL | allow::TYPE_DECL);
            }
        }
        Ok(())
    }

    /// Interface implementation validation at class pop (parse pass).
    fn check_interface_implementations(&mut self) -> CResult<()> {
        let class = self.class_ref();
        let interfaces = self.class_node().class.interfaces.clone();
        for (iface_root, _) in interfaces {
            let mut iface = Some(iface_root);
            while let Some(i) = iface {
                if !self.res().is_child_of(class, i) {
                    let ifuncs: Vec<FieldRef> = self.res().children(i).into_iter().filter(|f| self.res().is_function(*f)).collect();
                    for ifunc in ifuncs {
                        let iname = self.res().name_of(ifunc);
                        let iflags = self.res().func_flags(ifunc);
                        let mut implemented = false;
                        // TFieldIterator<UFunction>(TopNode): class and supers.
                        let mut cur = Some(class);
                        let mut guard = 0;
                        'search: while let Some(st) = cur {
                            for cf_ in self.res().children(st) {
                                if !self.res().is_function(cf_) || !self.res().name_of(cf_).eq_ignore_ascii_case(&iname) {
                                    continue;
                                }
                                let cflags = self.res().func_flags(cf_);
                                if iflags & func::NATIVE != 0 && cflags & func::NATIVE == 0 {
                                    return self.error(format!("Implementation of function '{iname}' must be native to match declaration in interface '{}'", self.res().name_of(i)));
                                }
                                if iflags & func::EVENT != 0 && cflags & func::EVENT == 0 {
                                    return self.error(format!(
                                        "Implementation of function '{iname}' must be declared as 'event' to match declaration in interface '{}'",
                                        self.res().name_of(i)
                                    ));
                                }
                                if iflags & func::DELEGATE != 0 && cflags & func::DELEGATE == 0 {
                                    return self.error(format!(
                                        "Implementation of function '{iname}' must be declared as 'delegate' to match declaration in interface '{}'",
                                        self.res().name_of(i)
                                    ));
                                }
                                implemented = true;
                                let cn = self.res().func_num_parms(cf_);
                                let inn = self.res().func_num_parms(ifunc);
                                if cn != inn {
                                    return self.error(format!(
                                        "Implementation of function '{iname}' conflicts with interface '{}' - different number of parameters ({cn}/{inn})",
                                        self.res().name_of(i)
                                    ));
                                }
                                let ip = self.res().func_parms(ifunc);
                                let cp = self.res().func_parms(cf_);
                                for (k, (a, b)) in ip.iter().zip(cp.iter()).enumerate() {
                                    let at = self.res().prop_type(*a, RefType::None);
                                    let bt = self.res().prop_type(*b, RefType::None);
                                    if !self.matches_type(&at, &bt, true, false) {
                                        if at.flags & cpf::RETURN_PARM != 0 {
                                            return self.error(format!(
                                                "Implementation of function '{iname}' conflicts only by return type with interface '{}'",
                                                self.res().name_of(i)
                                            ));
                                        }
                                        return self.error(format!(
                                            "Implementation of function '{iname}' conflicts with interface '{}' - parameter {} '{}'",
                                            self.res().name_of(i),
                                            k + 1,
                                            self.res().name_of(*a)
                                        ));
                                    }
                                }
                                break 'search;
                            }
                            cur = self.res().inheritance_super(st);
                            guard += 1;
                            if guard > 100 {
                                break;
                            }
                        }
                        if !implemented {
                            return self.error(format!("Missing required implementation of function '{iname}' from interface '{}'", self.res().name_of(i)));
                        }
                    }
                }
                iface = self.res().super_of(i);
            }
        }
        Ok(())
    }

    /// `FixupDelegateProperties`.
    fn fixup_delegate_properties(&mut self, scope: FieldRef, owner_class: FieldRef) -> CResult<()> {
        let FieldRef::Local(scope_id) = scope else { return Ok(()) };
        let children = self.model.node(scope_id).strct.children.clone();
        for child in children {
            let kind = self.model.node(child).kind.clone();
            match kind {
                NodeKind::Property(pk) => {
                    let delegate_prop = match pk {
                        PropertyKind::Delegate => Some(child),
                        PropertyKind::Array => match self.model.node(child).prop.ref1 {
                            Some(FieldRef::Local(inner)) if self.model.node(inner).kind == NodeKind::Property(PropertyKind::Delegate) => Some(inner),
                            _ => None,
                        },
                        _ => None,
                    };
                    let Some(dp) = delegate_prop else { continue };
                    if self.model.node(dp).prop.ref1.is_some() {
                        continue;
                    }
                    let Some(name) = self.model.node(dp).prop.delegate_name.clone() else { continue };
                    let source: Option<FieldRef>;
                    if !name.contains('.') {
                        source = self.res().find_field(Some(owner_class), &name, true, None).filter(|f| self.res().is_function(*f));
                    } else {
                        let (cn, dn) = name.split_once('.').unwrap();
                        if cn.is_empty() {
                            return self.error(format!("Invalid scope specified in delegate property function reference: '{name}'"));
                        }
                        if dn.is_empty() {
                            return self.error(format!("Invalid delegate name specified in delegate property function reference '{name}'"));
                        }
                        let Some(oc) = self.res().find_class(cn) else { return self.error(format!("Class '{cn}' not found.")) };
                        source = self.res().find_field(Some(oc), dn, false, None).filter(|f| self.res().is_function(*f));
                    }
                    let Some(sf_) = source else { return self.error(format!("Failed to find delegate function '{name}'")) };
                    if self.res().func_flags(sf_) & func::DELEGATE == 0 {
                        return self.error(format!("Only delegate functions can be used as the type for a delegate property; '{name}' is not a delegate."));
                    }
                    let n = self.model.node_mut(dp);
                    n.prop.ref1 = Some(sf_);
                    n.prop.ref2 = Some(sf_);
                }
                NodeKind::Function | NodeKind::State | NodeKind::ScriptStruct => {
                    self.fixup_delegate_properties(FieldRef::Local(child), owner_class)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// `PostParse`: probe masks.
    pub fn post_parse(&mut self, node: NodeId) {
        let kind = self.model.node(node).kind.clone();
        if matches!(kind, NodeKind::State | NodeKind::Class) {
            let mut mask = 0u32;
            let mut cur = Some(FieldRef::Local(node));
            let mut guard = 0;
            while let Some(st) = cur {
                for f in self.res().children(st) {
                    if !self.res().is_function(f) {
                        continue;
                    }
                    let name = self.res().name_of(f);
                    if let Some(bit) = Self::probe_bit(&name) {
                        let fl = self.res().func_flags(f);
                        if fl & (func::DEFINED | func::NATIVE) != 0 {
                            mask |= 1u32 << bit;
                        }
                    }
                }
                cur = self.res().inheritance_super(st);
                guard += 1;
                if guard > 100 {
                    break;
                }
            }
            self.model.node_mut(node).state.probe_mask = mask;
        }
        let children = self.model.node(node).strct.children.clone();
        for c in children {
            if self.model.node(c).is_struct_like() {
                self.post_parse(c);
            }
        }
    }

    /// `ContainsInstancedObjectProperty`.
    fn contains_instanced_object_property(&mut self, p: FieldRef) -> bool {
        match self.res().prop_kind(p) {
            Some(PropertyKind::Object | PropertyKind::Component | PropertyKind::SafeObject | PropertyKind::Class) => self.res().prop_flags(p) & cpf::NEED_CTOR_LINK != 0,
            Some(PropertyKind::Array) => match self.res().prop_refs(p).0 {
                Some(i) => self.contains_instanced_object_property(i),
                None => false,
            },
            Some(PropertyKind::Struct) => {
                let Some(s) = self.res().prop_refs(p).0 else { return false };
                let mut cur = Some(s);
                let mut guard = 0;
                while let Some(st) = cur {
                    for ch in self.res().children(st) {
                        if self.res().is_property(ch) && self.contains_instanced_object_property(ch) {
                            return true;
                        }
                    }
                    cur = self.res().inheritance_super(st);
                    guard += 1;
                    if guard > 100 {
                        break;
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// `CompileScript` for one class and one pass.
    pub fn compile_script(&mut self, class: NodeId, pass: Pass) -> CResult<()> {
        self.class = class;
        self.pass = pass;
        // Compiling Core: the Vector/Rotator structs appear while parsing Object.
        if self.vector_struct.is_none() {
            self.vector_struct = self.res().find_struct_any("Vector");
        }
        if self.rotator_struct.is_none() {
            self.rotator_struct = self.res().find_struct_any("Rotator");
        }
        self.original_variable_index = 0;
        self.supress_debug_info = 0;
        self.nests.clear();
        self.top_node = None;
        let source = self.sources.get(&class).cloned().unwrap_or_default();
        if pass == Pass::Parse {
            let super_class = self.super_class();
            {
                let c = self.model.node_mut(class);
                c.strct.script_mem.clear();
                c.properties_size = 0;
                c.class.class_flags &= !cf::RECOMPILER_CLEAR;
            }
            if let Some(s) = super_class {
                let sflags = self.res().class_flags(s);
                let config = self.res().class_config_name(s);
                let within = self.res().class_within(s);
                let (hide, expand, collapse, dontsort, force) = self.class_category_lists(s);
                let c = self.model.node_mut(class);
                c.class.class_flags |= sflags & cf::SCRIPT_INHERIT;
                c.class.config_name = config;
                if c.class.class_within.is_none() {
                    c.class.class_within = within;
                }
                c.class.hide_categories = hide;
                c.class.auto_expand_categories = expand;
                c.class.auto_collapse_categories = collapse;
                c.class.dont_sort_categories = dontsort;
                c.class.force_script_order = force;
            }
        } else {
            self.model.node_mut(class).strct.script_mem.clear();
            for p in self.own_properties(class) {
                let n = self.model.node_mut(p);
                n.prop.flags &= !cpf::NET;
                n.prop.rep_offset = MAXWORD as u16;
            }
        }
        self.set_input(super::engine::latin1_bytes(&source));
        self.push_nest(NestType::None, "", None)?;
        if pass == Pass::Parse {
            while self.compile_statement()? {}
            self.model.node_mut(class).strct.script_mem.clear();
            self.model.node_mut(class).class.class_flags |= cf::PARSED;
        } else {
            self.compile_second_pass(class)?;
            self.model.node_mut(class).class.class_flags |= cf::COMPILED | cf::NEEDS_DEF_PROPS;
        }
        match self.nests.len() {
            0 => return self.error("Internal nest inconsistency"),
            1 => return self.error("Missing 'Class' definition"),
            2 => {
                if self.class_has(cf::INTERFACE) {
                    self.pop_nest(NestType::Interface, "'Interface'")?;
                } else {
                    self.pop_nest(NestType::Class, "'Class'")?;
                }
            }
            _ => return self.error(format!("Unexpected end of script in '{}' block", nest_type_name(self.top_nest_ref().nest_type))),
        }
        if pass == Pass::Parse {
            self.post_parse(class);
            let mut add_flags = 0u32;
            for p in self.own_properties(class) {
                let f = self.model.node(p).prop.flags;
                // `UProperty::IsLocalized`: the flag, or a struct/array holding a localized member.
                if self.property_is_localized(FieldRef::Local(p), 0) {
                    add_flags |= cf::LOCALIZED;
                }
                if f & cpf::CONFIG != 0 {
                    add_flags |= cf::CONFIG;
                }
                if f & cpf::COMPONENT != 0 {
                    add_flags |= cf::HAS_COMPONENTS;
                }
                if self.contains_instanced_object_property(FieldRef::Local(p)) {
                    add_flags |= cf::HAS_INSTANCED_PROPS;
                }
                if f & (cpf::CROSS_LEVEL_ACTIVE | cpf::CROSS_LEVEL_PASSIVE) != 0 {
                    add_flags |= cf::HAS_CROSS_LEVEL_REFS;
                }
            }
            self.model.node_mut(class).class.class_flags |= add_flags;
            if self.class_has(cf::CONFIG) && (self.class_node().class.config_name.is_empty() || self.class_node().class.config_name.eq_ignore_ascii_case("None")) {
                let cn = self.super_class().map(|s| self.res().class_config_name(s)).unwrap_or_else(|| "None".into());
                self.class_node_mut().class.config_name = cn;
                if self.class_node().class.config_name.eq_ignore_ascii_case("None") {
                    return self.error("Classes with config / globalconfig member variables need to specify config file.");
                }
            }
            let mut found_auto = false;
            let mut cur = Some(self.class_ref());
            let mut guard = 0;
            while let Some(st) = cur {
                for ch in self.res().children(st) {
                    if self.res().kind_of(ch) == FieldKind::State && self.res().state_flags(ch) & sf::AUTO != 0 {
                        if found_auto && self.res().outer_of(ch) == Some(self.class_ref()) {
                            return self.error("Multiple auto states defined");
                        }
                        found_auto = true;
                    }
                }
                cur = self.res().inheritance_super(st);
                guard += 1;
                if guard > 100 {
                    break;
                }
            }
            if !found_auto {
                self.class_node_mut().state.state_flags |= sf::AUTO;
            }
        }
        Ok(())
    }

    fn class_category_lists(&self, c: FieldRef) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>, bool) {
        match c {
            FieldRef::Local(n) => {
                let k = &self.model.node(n).class;
                (
                    k.hide_categories.clone(),
                    k.auto_expand_categories.clone(),
                    k.auto_collapse_categories.clone(),
                    k.dont_sort_categories.clone(),
                    k.force_script_order,
                )
            }
            FieldRef::External(o) => self.ctx.class_category_lists(o),
        }
    }
}

pub fn nest_type_name(t: NestType) -> &'static str {
    match t {
        NestType::None => "Global Scope",
        NestType::Class => "Class",
        NestType::Interface => "Interface",
        NestType::State => "State",
        NestType::Function => "Function",
        NestType::If => "If",
        NestType::Loop => "Loop",
        NestType::Switch => "Switch",
        NestType::For => "For",
        NestType::ForEach => "ForEach",
        NestType::FilterEditorOnly => "FilterEditorOnly",
    }
}

fn kind_name(k: FieldKind) -> String {
    match k {
        FieldKind::Class => "Class".into(),
        FieldKind::State => "State".into(),
        FieldKind::Function => "Function".into(),
        FieldKind::ScriptStruct => "ScriptStruct".into(),
        FieldKind::Enum => "Enum".into(),
        FieldKind::Const => "Const".into(),
        FieldKind::Property(p) => format!("{p:?}Property"),
        FieldKind::Other => "Field".into(),
    }
}

/// `UEnum::GenerateEnumPrefix`.
pub fn generate_enum_prefix(names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let mut prefix: Vec<char> = names[0].chars().collect();
    for n in &names[1..] {
        let cs: Vec<char> = n.chars().collect();
        let mut i = 0;
        while i < prefix.len() && i < cs.len() && prefix[i] == cs[i] {
            i += 1;
        }
        prefix.truncate(i);
    }
    let s: String = prefix.into_iter().collect();
    match s.rfind('_') {
        Some(i) if i > 0 => s[..i].to_string(),
        _ => String::new(),
    }
}

/// `FCommentStrippingFilter::Process`.
pub fn strip_comments(src: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut quoted: Option<char> = None;
    let mut escaped = false;
    let mut single = false;
    let mut multi = false;
    let mut whitespace_generated = false;
    while i < chars.len() {
        let ch = chars[i];
        let next = chars.get(i + 1).copied();
        i += 1;
        if multi {
            if ch == '*' && next == Some('/') {
                i += 1;
                if !whitespace_generated {
                    out.push(' ');
                }
                multi = false;
            } else if ch == '\r' || ch == '\n' {
                out.push(ch);
                if next == Some('\n') {
                    out.push('\n');
                    i += 1;
                }
                whitespace_generated = true;
            }
        } else if single {
            if ch == '\r' || ch == '\n' {
                out.push(ch);
                if next == Some('\n') {
                    out.push('\n');
                    i += 1;
                }
                single = false;
            }
        } else if let Some(q) = quoted {
            out.push(ch);
            if !escaped {
                if ch == '\\' {
                    escaped = true;
                } else if ch == q {
                    quoted = None;
                }
            } else {
                escaped = false;
            }
        } else if ch == '/' && next == Some('/') {
            i += 1;
            single = true;
        } else if ch == '/' && next == Some('*') {
            i += 1;
            whitespace_generated = false;
            multi = true;
        } else if ch == '"' || ch == '\'' {
            out.push(ch);
            quoted = Some(ch);
        } else {
            out.push(ch);
        }
    }
    out
}
