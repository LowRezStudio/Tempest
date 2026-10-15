//! Default properties: port of `ImportProperties` / `ImportDefaultProperties`
//! (UnEdObject.cpp) and the per-type `ImportText` parsers, producing value
//! trees for class default objects, sub-objects and script structs.

use tempest_upk::flags::{class as cf, cpf, func, rf};

use super::driver::DriverError;
use super::engine::Compiler;
use super::model::{FieldRef, Node, NodeId, NodeKind};
use super::scope::FieldKind;
use super::values::{find_slot, find_slot_mut, replace_refs_slots, Slot, Value};
use crate::objects::PropertyKind;

type DResult<T> = Result<T, DriverError>;

fn derr(s: impl Into<String>) -> DriverError {
    DriverError(s.into())
}

/// `UStruct::PropagateStructDefaults` for a class: flag functions that have
/// struct locals with defaults.
pub fn propagate_struct_defaults(c: &mut Compiler, class: NodeId) {
    let funcs: Vec<NodeId> = c.model.node(class).strct.children.iter().copied().filter(|&f| c.model.node(f).kind == NodeKind::Function).collect();
    for f in funcs {
        let props: Vec<NodeId> = c.model.node(f).strct.children.iter().copied().filter(|&p| c.model.node(p).kind == NodeKind::Property(PropertyKind::Struct)).collect();
        for p in props {
            let n = c.model.node(p);
            if n.prop.flags & cpf::PARM != 0 {
                continue;
            }
            let Some(st) = n.prop.ref1 else { continue };
            let (size, al) = c.struct_layout(st);
            let aligned = if al > 1 { (size + al - 1) / al * al } else { size };
            if aligned > 0 {
                c.model.node_mut(f).func.flags |= func::HAS_DEFAULTS;
                break;
            }
        }
    }
}

/// Split the `linenumber=N` header off a defaults text.
fn split_defaults_text(text: &str) -> (u32, Vec<String>) {
    let mut lines = super::preprocess::split_lines(text);
    let mut line = 0;
    if let Some(first) = lines.first() {
        if let Some(n) = first.strip_prefix("linenumber=") {
            line = n.trim().parse().unwrap_or(0);
            lines.remove(0);
        }
    }
    (line, lines)
}

struct Cursor {
    lines: Vec<String>,
    pos: usize,
    line_no: u32,
}

impl Cursor {
    fn next_line(&mut self) -> Option<String> {
        if self.pos < self.lines.len() {
            let l = self.lines[self.pos].clone();
            self.pos += 1;
            self.line_no += 1;
            Some(l)
        } else {
            None
        }
    }
}

/// `ReadStructValue`: a value starting with `{` spans lines until braces balance.
fn read_struct_value(s: &str, cur: &mut Cursor) -> String {
    let t = s.trim_start_matches([' ', '\t']);
    if !t.starts_with('{') {
        return s.to_string();
    }
    let mut result = String::new();
    let mut bracket = 0i32;
    let mut in_string = false;
    let mut chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    loop {
        if i >= chars.len() {
            match cur.next_line() {
                Some(l) => {
                    chars = l.chars().collect();
                    i = 0;
                    if chars.is_empty() {
                        continue;
                    }
                }
                None => break,
            }
        }
        let ch = chars[i];
        if ch == '{' {
            bracket += 1;
        } else if ch == '}' {
            bracket -= 1;
        } else if ch == '"' {
            in_string = !in_string;
        }
        if in_string || (ch != '\n' && ch != '\r' && !ch.is_whitespace() && ch != '{' && ch != '}') {
            result.push(ch);
        }
        i += 1;
        if bracket == 0 {
            break;
        }
    }
    result
}

/// `GetBEGIN(&Str, "Object")`-style: `Begin Object ...` → rest of line.
fn get_begin<'a>(line: &'a str, what: &str) -> Option<&'a str> {
    let s = line.trim_start();
    let s = s.strip_prefix("Begin").or_else(|| s.strip_prefix("BEGIN")).or_else(|| s.strip_prefix("begin"))?;
    let s = s.trim_start();
    if s.len() >= what.len() && s[..what.len()].eq_ignore_ascii_case(what) {
        let rest = &s[what.len()..];
        if rest.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) {
            return None;
        }
        return Some(rest.trim_start());
    }
    None
}

fn get_end(line: &str, what: &str) -> bool {
    let s = line.trim_start();
    let Some(s) = s.strip_prefix("End").or_else(|| s.strip_prefix("END")).or_else(|| s.strip_prefix("end")) else { return false };
    let s = s.trim_start();
    s.len() >= what.len() && s[..what.len()].eq_ignore_ascii_case(what) && !s[what.len()..].chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false)
}

/// `Parse(Str, "Key=", Value)`: find `Key=` in a line and read a token after it.
fn parse_kv(line: &str, key: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let k = key.to_ascii_lowercase();
    let mut from = 0;
    while let Some(p) = lower[from..].find(&k) {
        let at = from + p;
        let boundary = at == 0 || !line.as_bytes()[at - 1].is_ascii_alphanumeric();
        if boundary {
            let rest = &line[at + key.len()..];
            return Some(parse_token_value(rest));
        }
        from = at + 1;
    }
    None
}

/// `ParseToken`: a quoted string or a run of non-whitespace characters.
fn parse_token_value(s: &str) -> String {
    let s = s.trim_start();
    if let Some(q) = s.strip_prefix('"') {
        let end = q.find('"').unwrap_or(q.len());
        return q[..end].to_string();
    }
    s.chars().take_while(|c| !c.is_whitespace()).collect()
}

/// `ReadToken`: quoted string (with escapes) or alnum/_/- run (optionally dotted).
fn read_token(s: &str, dotted: bool) -> Option<(String, usize)> {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    if b.first() == Some(&'"') {
        i = 1;
        while i < b.len() && b[i] != '"' && b[i] != '\n' && b[i] != '\r' {
            if b[i] != '\\' {
                out.push(b[i]);
                i += 1;
            } else {
                i += 1;
                match b.get(i) {
                    Some('\\') => {
                        out.push('\\');
                        i += 1;
                    }
                    Some('"') => {
                        out.push('"');
                        i += 1;
                    }
                    Some('n') => {
                        out.push('\n');
                        i += 1;
                    }
                    Some('r') => {
                        out.push('\r');
                        i += 1;
                    }
                    Some(_) => {
                        let h = |c: char| c.to_digit(16).unwrap_or(0);
                        let v = h(b[i]) * 16 + b.get(i + 1).map(|c| h(*c)).unwrap_or(0);
                        out.push(char::from_u32(v).unwrap_or('?'));
                        i += 2;
                    }
                    None => break,
                }
            }
        }
        if b.get(i) != Some(&'"') {
            return None;
        }
        i += 1;
    } else if b.first().map(|c| c.is_ascii_alphanumeric() || *c == '_').unwrap_or(false) {
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_' || b[i] == '-' || (dotted && (b[i] == '.' || b[i] == ':'))) {
            out.push(b[i]);
            i += 1;
        }
    } else if let Some(&c) = b.first() {
        out.push(c);
        // ReadToken does not advance past a lone symbol.
    }
    Some((out, b[..i].iter().collect::<String>().len()))
}

fn skip_ws(s: &str) -> &str {
    s.trim_start_matches([' ', '\t'])
}

struct ImportCtx {
    /// `SubobjectRoot`: the top-most object (the CDO) when importing sub-objects.
    root: Option<NodeId>,
    /// `SubobjectOuter`: the object whose properties are being imported (None for struct defaults).
    outer: Option<NodeId>,
    /// `ComponentOwnerClass`.
    owner_class: Option<FieldRef>,
    depth: i32,
}

impl<'a> Compiler<'a> {
    // ----- object/struct helpers ------------------------------------------------------

    fn is_cdo_node(&self, n: NodeId) -> bool {
        self.model.node(n).object_flags & rf::CLASS_DEFAULT_OBJECT != 0
    }

    /// Sub-object of `outer` named `name` (local nodes or loaded exports).
    fn find_subobject(&self, outer: FieldRef, name: &str) -> Option<FieldRef> {
        match outer {
            FieldRef::Local(o) => self
                .model
                .nodes
                .iter()
                .enumerate()
                .find(|(_, n)| n.kind == NodeKind::Object && n.outer == Some(o) && n.name.eq_ignore_ascii_case(name))
                .map(|(i, _)| FieldRef::Local(i)),
            FieldRef::External(o) => {
                let path = format!("{}.{}", self.ctx.path(o), name);
                self.ctx.find_export(o.pkg, &path).map(FieldRef::External)
            }
        }
    }

    fn object_archetype(&self, obj: FieldRef) -> Option<FieldRef> {
        match obj {
            FieldRef::Local(n) => self.model.node(n).archetype,
            FieldRef::External(o) => {
                let idx = self.ctx.packages[o.pkg].pkg.exports[o.export].archetype;
                self.res().resolve_field(o.pkg, idx)
            }
        }
    }

    /// Whether `node`'s outer chain contains `root`.
    fn is_inside(&self, node: NodeId, root: NodeId) -> bool {
        let mut cur = self.model.node(node).outer;
        let mut guard = 0;
        while let Some(o) = cur {
            if o == root {
                return true;
            }
            cur = self.model.node(o).outer;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        false
    }

    fn object_outer(&self, obj: FieldRef) -> Option<FieldRef> {
        match obj {
            FieldRef::Local(n) => self.model.node(n).outer.map(FieldRef::Local),
            FieldRef::External(o) => self.ctx.outer_of(o).map(FieldRef::External),
        }
    }

    fn obj_class(&self, obj: FieldRef) -> Option<FieldRef> {
        match obj {
            FieldRef::Local(n) => self.model.node(n).object_class,
            // An object of a shadow package belongs to the class being compiled here.
            FieldRef::External(o) => match self.ctx.class_of(o) {
                Some(c) if self.ctx.packages[c.pkg].shadow => self.res().find_class(&self.ctx.name(c)).or(Some(FieldRef::External(c))),
                Some(c) => Some(FieldRef::External(c)),
                None => self.res().find_class(&self.ctx.class_name(o)),
            },
        }
    }

    fn is_template(&self, obj: FieldRef) -> bool {
        // Objects under a class default object chain are templates.
        let mut cur = Some(obj);
        let mut guard = 0;
        while let Some(o) = cur {
            match o {
                FieldRef::Local(n) => {
                    if self.model.node(n).object_flags & rf::CLASS_DEFAULT_OBJECT != 0 {
                        return true;
                    }
                }
                FieldRef::External(r) => {
                    if self.ctx.object_flags(r) & rf::CLASS_DEFAULT_OBJECT != 0 {
                        return true;
                    }
                }
            }
            cur = self.object_outer(o);
            guard += 1;
            if guard > 32 {
                break;
            }
        }
        false
    }

    /// `UObjectProperty::FindImportedObject` under `PPF_ParsingDefaultProperties`.
    fn find_imported_object(&mut self, object_class: FieldRef, text: &str, owner: Option<NodeId>) -> Option<FieldRef> {
        let class_class = self.class_class();
        let want_class = Some(object_class) == class_class;
        let matches_class = |c: &Compiler, obj: FieldRef| -> bool {
            match c.obj_class(obj) {
                Some(oc) => c.res().is_child_of(oc, object_class),
                None => false,
            }
        };
        if !text.contains('.') {
            // Scoped search through the owner, its archetypes and outers (templates only).
            let mut search_start = owner.map(FieldRef::Local);
            while let Some(ss) = search_start {
                let mut scoped = Some(ss);
                while let Some(sc) = scoped {
                    if let Some(found) = self.find_subobject(sc, text) {
                        if self.is_template(found) && matches_class(self, found) {
                            return Some(found);
                        }
                    }
                    scoped = self.object_archetype(sc);
                }
                let is_cdo = match ss {
                    FieldRef::Local(n) => self.is_cdo_node(n),
                    FieldRef::External(r) => self.ctx.object_flags(r) & rf::CLASS_DEFAULT_OBJECT != 0,
                };
                if is_cdo {
                    break;
                }
                search_start = self.object_outer(ss);
            }
        }
        if want_class {
            let name = text.rsplit('.').next().unwrap_or(text);
            if let Some(c) = self.res().find_class(name) {
                return Some(c);
            }
        }
        // Any package: by full path or name.
        let found = self.find_object_literal(object_class, text);
        if let Some(f) = found {
            if matches_class(self, f) || want_class {
                return Some(f);
            }
        }
        None
    }

    // ----- per-type ImportText ---------------------------------------------------------

    /// `UProperty::ImportText` for one element. Returns the number of bytes consumed.
    fn import_text(&mut self, prop: FieldRef, text: &str, value: &mut Value, ctx: &ImportCtx, scope: FieldRef) -> Option<usize> {
        let kind = self.prop_kind_of(prop);
        // ValidateImportFlags: under `make` config/localized values cannot come from defaults.
        let pf = self.res().prop_flags(prop);
        if !self.allow_config_defaults && pf & (cpf::CONFIG | cpf::LOCALIZED) != 0 {
            let what = if pf & cpf::CONFIG != 0 { if pf & cpf::LOCALIZED != 0 { "config/localized" } else { "config" } } else { "localized" };
            self.warn(format!(
                "Import failed for '{}': property is {what} (Check to see if the property is listed in the DefaultProperties.  It should only be listed in the specific .ini/.int file)",
                self.res().name_of(prop)
            ));
            return None;
        }
        match kind {
            PropertyKind::Byte => {
                if let Some(e) = self.res().prop_refs(prop).0 {
                    if let Some((tok, n)) = read_token(text, false) {
                        if !tok.is_empty() {
                            if let Some(i) = self.res().find_enum_index(e, &tok) {
                                *value = Value::Byte(i as u8);
                                return Some(n);
                            }
                        }
                    }
                }
                let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
                if digits.is_empty() {
                    return None;
                }
                *value = Value::Byte(digits.parse::<i64>().unwrap_or(0) as u8);
                Some(digits.len())
            }
            PropertyKind::Int => {
                let mut i = 0;
                let b = text.as_bytes();
                let v: i32;
                if text.len() >= 2 && text[..2].eq_ignore_ascii_case("0x") {
                    i = 2;
                    while i < b.len() && b[i].is_ascii_hexdigit() {
                        i += 1;
                    }
                    v = i64::from_str_radix(&text[2..i], 16).unwrap_or(0) as i32;
                } else {
                    while i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                        i += 1;
                    }
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i == 0 {
                        // GIsUCC: enum tag or const name.
                        let mut j = 0;
                        while j < b.len() && b[j] != b')' && b[j] != b',' {
                            j += 1;
                        }
                        let name = text[..j].trim();
                        if let Some(ev) = self.res().global_enum_tag(name) {
                            *value = Value::Int(ev);
                            return Some(j);
                        }
                        if let Some(cv) = self.find_const_value_anywhere(name) {
                            return self.import_text(prop, &cv, value, ctx, scope).map(|_| j);
                        }
                        return None;
                    }
                    v = text[..i].parse::<i64>().unwrap_or_else(|_| text[..i].trim_start_matches('+').parse::<i64>().unwrap_or(0)) as i32;
                }
                *value = Value::Int(v);
                Some(i)
            }
            PropertyKind::Float => {
                let b = text.as_bytes();
                let mut i = 0;
                while i < b.len() && (b[i] == b'+' || b[i] == b'-' || b[i] == b'.' || b[i].is_ascii_digit()) {
                    i += 1;
                }
                if i > 0 {
                    *value = Value::Float(appatof(&text[..i]));
                    if i < b.len() && (b[i] == b'f' || b[i] == b'F') {
                        i += 1;
                    }
                }
                Some(i)
            }
            PropertyKind::Bool => {
                let (tok, n) = read_token(text, false)?;
                if ["1", "True", "Yes"].iter().any(|s| tok.eq_ignore_ascii_case(s)) {
                    *value = Value::Bool(true);
                } else if ["0", "False", "No"].iter().any(|s| tok.eq_ignore_ascii_case(s)) {
                    *value = Value::Bool(false);
                } else {
                    return None;
                }
                Some(n)
            }
            PropertyKind::Name => {
                let (tok, n) = read_token(text, false)?;
                *value = Value::Name(tok);
                Some(n)
            }
            PropertyKind::Str => {
                let (tok, n) = read_token(text, false)?;
                *value = Value::Str(tok);
                Some(n)
            }
            PropertyKind::Object | PropertyKind::Class | PropertyKind::Component | PropertyKind::SafeObject | PropertyKind::Interface => {
                let (tok, mut n) = read_token(text, true)?;
                let obj: Option<FieldRef>;
                if tok.eq_ignore_ascii_case("None") {
                    obj = None;
                } else {
                    let required = self.res().prop_refs(prop).0.or_else(|| self.object_class())?;
                    let rest = &text[n..];
                    let rest_trim = skip_ws(rest);
                    n += rest.len() - rest_trim.len();
                    if rest_trim.starts_with('\'') {
                        let (otext, m) = read_token(&rest_trim[1..], true)?;
                        if rest_trim[1 + m..].chars().next() != Some('\'') {
                            return None;
                        }
                        n += 1 + m + 1;
                        let Some(oc) = self.res().find_class(&tok) else {
                            self.warn(format!("{}: unresolved cast in '{}'", self.res().name_of(prop), text));
                            return None;
                        };
                        if !self.res().is_child_of(oc, required) {
                            self.warn(format!("{}: invalid cast in '{}'", self.res().name_of(prop), text));
                            return None;
                        }
                        obj = self.find_imported_object(oc, &otext, ctx.outer);
                    } else {
                        obj = self.find_imported_object(required, &tok, ctx.outer);
                    }
                    if obj.is_none() {
                        self.warn(format!("{}: unresolved reference to '{}'", self.res().name_of(prop), text));
                        return None;
                    }
                    if kind == PropertyKind::Class {
                        if let (Some(o), Some(meta)) = (obj, self.res().prop_refs(prop).1) {
                            if !self.res().is_class(o) || !self.res().is_child_of(o, meta) {
                                self.warn(format!("Invalid object '{}' specified for property '{}'", self.res().name_of(o), self.res().name_of(prop)));
                                return None;
                            }
                        }
                    }
                }
                *value = if kind == PropertyKind::Interface { Value::Interface(obj) } else { Value::Object(obj) };
                Some(n)
            }
            PropertyKind::Delegate => {
                let b: Vec<char> = text.chars().collect();
                let mut i = 0;
                let mut obj_name = String::new();
                while i < b.len() && b[i] != '.' && b[i] != ')' && b[i] != ',' {
                    obj_name.push(b[i]);
                    i += 1;
                }
                let mut func_name = String::new();
                if i < b.len() && b[i] == '.' {
                    i += 1;
                    while i < b.len() && b[i] != ')' && b[i] != ',' {
                        func_name.push(b[i]);
                        i += 1;
                    }
                }
                let consumed: usize = b[..i].iter().collect::<String>().len();
                let cls = self.res().find_class(&obj_name);
                let (object, class) = match cls {
                    Some(c) => {
                        let cdo = self.class_cdo_ref(c);
                        let parent_is_cdo = ctx.outer.map(|o| Some(FieldRef::Local(o)) == cdo).unwrap_or(false);
                        (if parent_is_cdo { None } else { cdo }, Some(c))
                    }
                    None => {
                        let o = self.find_object_literal(self.object_class().unwrap(), &obj_name);
                        (o, o.and_then(|x| self.obj_class(x)))
                    }
                };
                let Some(class) = class else {
                    self.warn(format!("Unable to find delegate function {func_name} in object {obj_name}"));
                    return None;
                };
                let func_ = self.res().find_field(Some(class), &func_name, true, None).filter(|f| self.res().is_function(*f));
                let Some(f) = func_ else {
                    self.warn(format!("Unable to find delegate function {func_name} in object {obj_name} (found class: {})", self.res().name_of(class)));
                    return None;
                };
                if let Some(delegate) = self.res().prop_refs(prop).0 {
                    let fp = self.res().func_parms(f);
                    let dp = self.res().func_parms(delegate);
                    if self.res().func_num_parms(f) != self.res().func_num_parms(delegate) {
                        self.warn(format!("Function {} does not match number of params with delegate {}", func_name, self.res().name_of(delegate)));
                        return None;
                    }
                    for (a, b) in fp.iter().zip(dp.iter()) {
                        if self.res().prop_kind(*a) != self.res().prop_kind(*b) || (self.res().prop_flags(*a) & cpf::OUT_PARM) != (self.res().prop_flags(*b) & cpf::OUT_PARM) {
                            self.warn(format!("Function {} does not match param types with delegate {}", func_name, self.res().name_of(delegate)));
                            return None;
                        }
                    }
                }
                *value = Value::Delegate(object, self.res().name_of(f));
                Some(consumed)
            }
            PropertyKind::Struct => self.import_struct_text(prop, text, value, ctx, scope),
            PropertyKind::Array => self.import_array_text(prop, text, value, ctx, scope),
            PropertyKind::Map => None,
        }
    }

    /// Native serialization tail of an object (what follows its tagged properties).
    pub(super) fn node_path(&self, id: NodeId) -> String {
        let mut parts = vec![self.model.node(id).name.clone()];
        let mut cur = self.model.node(id).outer;
        while let Some(o) = cur {
            parts.push(self.model.node(o).name.clone());
            cur = self.model.node(o).outer;
        }
        parts.reverse();
        parts.join(".")
    }

    /// Native data of the object `outer.name` of that class in any loaded package
    /// (a reference package in a round trip is the only source for per-instance
    /// native payloads).
    fn native_rest_by_path(&mut self, outer: Option<NodeId>, name: &str, class: Option<FieldRef>) -> Vec<u8> {
        if self.native_rest_index.is_none() {
            let mut idx = std::collections::HashMap::new();
            for (pi, lp) in self.ctx.packages.iter().enumerate() {
                for e in 0..lp.pkg.exports.len() {
                    let x = crate::context::ObjRef { pkg: pi, export: e };
                    if let Some(crate::objects::ObjectBody::Other { rest }) = self.ctx.object(x).map(|b| &b.body) {
                        if !rest.is_empty() {
                            idx.entry(self.ctx.path(x).to_ascii_lowercase()).or_insert((self.ctx.class_name(x), rest.clone()));
                        }
                    }
                }
            }
            self.native_rest_index = Some(idx);
        }
        let mut parts = vec![name.to_string()];
        let mut cur = outer;
        while let Some(o) = cur {
            parts.push(self.model.node(o).name.clone());
            cur = self.model.node(o).outer;
        }
        parts.reverse();
        let path = parts.join(".").to_ascii_lowercase();
        let class_name = class.map(|c| self.res().name_of(c)).unwrap_or_default();
        match self.native_rest_index.as_ref().and_then(|i| i.get(&path)) {
            Some((cn, rest)) if cn.eq_ignore_ascii_case(&class_name) => rest.clone(),
            _ => Vec::new(),
        }
    }

    fn native_rest_of(&self, r: Option<FieldRef>) -> Vec<u8> {
        let direct = match r {
            Some(FieldRef::Local(n)) => self.model.node(n).native_rest.clone(),
            Some(FieldRef::External(o)) => match self.ctx.object(o).map(|x| &x.body) {
                Some(crate::objects::ObjectBody::Other { rest }) => rest.clone(),
                _ => Vec::new(),
            },
            None => Vec::new(),
        };
        if !direct.is_empty() {
            return direct;
        }
        // Some component classes serialize native data for instances only (the CDO has
        // none): take a zero-filled sample from an existing instance of the class.
        let class = match r {
            Some(FieldRef::Local(n)) => self.model.node(n).object_class,
            Some(FieldRef::External(o)) => self.ctx.class_of(o).map(FieldRef::External),
            None => None,
        };
        let Some(class) = class else { return Vec::new() };
        let class_name = self.res().name_of(class);
        for (pi, lp) in self.ctx.packages.iter().enumerate() {
            for e in 0..lp.pkg.exports.len() {
                let x = crate::context::ObjRef { pkg: pi, export: e };
                if self.ctx.object_flags(x) & rf::CLASS_DEFAULT_OBJECT != 0 || !self.ctx.class_name(x).eq_ignore_ascii_case(&class_name) {
                    continue;
                }
                if let Some(crate::objects::ObjectBody::Other { rest }) = self.ctx.object(x).map(|b| &b.body) {
                    if !rest.is_empty() && rest.iter().all(|&b| b == 0) {
                        return rest.clone();
                    }
                }
            }
        }
        Vec::new()
    }

    fn class_cdo_ref(&self, c: FieldRef) -> Option<FieldRef> {
        match c {
            FieldRef::Local(n) => self.model.node(n).class.cdo.map(FieldRef::Local),
            FieldRef::External(o) => self.ctx.class_default_object(o).map(FieldRef::External),
        }
    }

    /// `UConst` lookup anywhere (`ExactCast<UConst>` iteration): local first, then loaded packages.
    fn find_const_value_anywhere(&self, name: &str) -> Option<String> {
        for n in &self.model.nodes {
            if n.kind == NodeKind::Const && n.name.eq_ignore_ascii_case(name) {
                return Some(n.const_value.clone());
            }
        }
        for (p, lp) in self.ctx.packages.iter().enumerate() {
            for i in 0..lp.pkg.exports.len() {
                let r = crate::context::ObjRef { pkg: p, export: i };
                if self.ctx.class_name(r) == "Const" && self.ctx.name(r).eq_ignore_ascii_case(name) {
                    return Some(self.res().const_value(FieldRef::External(r)));
                }
            }
        }
        None
    }

    /// `UStructProperty::ImportText`.
    fn import_struct_text(&mut self, prop: FieldRef, text: &str, value: &mut Value, ctx: &ImportCtx, scope: FieldRef) -> Option<usize> {
        let Some(strct) = self.res().prop_refs(prop).0 else { return None };
        let b = text.as_bytes();
        if b.first() != Some(&b'(') {
            return None;
        }
        let mut i = 1;
        let Value::Struct(slots) = value else { return None };
        while i < b.len() && b[i] != b')' {
            while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
                i += 1;
            }
            let start = i;
            while i < b.len() && b[i] != b'=' && b[i] != b'[' && b[i] != b'(' && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            let name = text[start..i].to_string();
            let (array_index, used) = self.read_array_index(&text[i..], scope, ctx.owner_class);
            i += used;
            while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
                i += 1;
            }
            if i >= b.len() || b[i] != b'=' {
                self.warn(format!("ImportText ({}): Missing value for property '{}'", self.res().name_of(prop), name));
                return None;
            }
            i += 1;
            let mut parsed = false;
            let members = self.props_in_layout_order(strct);
            for m in members {
                if !self.res().name_of(m).eq_ignore_ascii_case(&name) {
                    continue;
                }
                let mk = self.prop_kind_of(m);
                let dim = self.res().prop_array_dim(m).max(1);
                if array_index >= dim && mk != PropertyKind::Array {
                    self.warn(format!("ImportText ({}): Array index out of bounds for property {} in: {}", self.res().name_of(prop), name, text));
                    return None;
                }
                while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
                    i += 1;
                }
                let specified = i < b.len() && b[i] != b',' && b[i] != b')';
                if mk == PropertyKind::Array && array_index >= 0 {
                    if specified {
                        let Some(inner) = self.res().prop_refs(m).0 else { return None };
                        let mut cur = find_slot(slots, m).map(|s| s.elems[0].clone()).unwrap_or(Value::Array(Vec::new()));
                        if let Value::Array(items) = &mut cur {
                            while (array_index as usize) >= items.len() {
                                let z = self.zero_value(inner);
                                items.push(z);
                            }
                            let mut elem = items[array_index as usize].clone();
                            let n = self.import_text(inner, &text[i..], &mut elem, ctx, scope)?;
                            items[array_index as usize] = elem;
                            i += n;
                        }
                        if let Some(s) = find_slot_mut(slots, m) {
                            s.elems[0] = cur;
                        }
                    }
                } else {
                    let idx = array_index.max(0) as usize;
                    if specified {
                        let mut elem = find_slot(slots, m).and_then(|s| s.elems.get(idx).cloned()).unwrap_or_else(|| Value::Int(0));
                        let n = self.import_text(m, &text[i..], &mut elem, ctx, scope)?;
                        if let Some(s) = find_slot_mut(slots, m) {
                            if idx < s.elems.len() {
                                s.elems[idx] = elem;
                            }
                        }
                        i += n;
                    }
                }
                parsed = true;
                break;
            }
            if !parsed {
                self.warn(format!("Unknown member {} in struct {}", name, self.res().name_of(strct)));
                // skip to next , or )
                let mut depth = 0;
                while i < b.len() {
                    if b[i] == b'(' {
                        depth += 1;
                    } else if b[i] == b')' {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    } else if b[i] == b',' && depth == 0 {
                        break;
                    }
                    i += 1;
                }
            }
            while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
                i += 1;
            }
            if i < b.len() && b[i] == b',' {
                i += 1;
            }
        }
        if i >= b.len() || b[i] != b')' {
            return None;
        }
        Some(i + 1)
    }

    /// `UArrayProperty::ImportText`.
    fn import_array_text(&mut self, prop: FieldRef, text: &str, value: &mut Value, ctx: &ImportCtx, scope: FieldRef) -> Option<usize> {
        let Some(inner) = self.res().prop_refs(prop).0 else { return None };
        let b = text.as_bytes();
        if b.first() != Some(&b'(') {
            return None;
        }
        let mut items: Vec<Value> = Vec::new();
        let mut i = 1;
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        let mut index = 0usize;
        while i < b.len() && b[i] != b')' {
            while i < b.len() && b[i] == b',' {
                i += 1;
                if index >= items.len() {
                    let z = self.zero_value(inner);
                    items.push(z);
                }
                index += 1;
                if i < b.len() && b[i] == b')' {
                    *value = Value::Array(items);
                    return Some(i + 1);
                }
            }
            if index >= items.len() {
                let z = self.zero_value(inner);
                items.push(z);
            }
            let mut elem = items[index].clone();
            let n = self.import_text(inner, &text[i..], &mut elem, ctx, scope)?;
            items[index] = elem;
            i += n;
            index += 1;
            while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
                i += 1;
            }
            if i >= b.len() || b[i] != b',' {
                break;
            }
            i += 1;
            while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
                i += 1;
            }
        }
        if i >= b.len() || b[i] != b')' {
            return None;
        }
        *value = Value::Array(items);
        Some(i + 1)
    }

    /// `ReadArrayIndex`: `(expr)` / `[expr]` with enum tags and consts. Returns `(index, bytes consumed)`; -1 when absent.
    fn read_array_index(&mut self, s: &str, scope: FieldRef, top_outer: Option<FieldRef>) -> (i32, usize) {
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        if i >= b.len() || (b[i] != b'(' && b[i] != b'[') {
            return (-1, 0);
        }
        i += 1;
        let start = i;
        while i < b.len() && b[i] != b')' && b[i] != b']' {
            if b[i] == b'=' {
                self.warn(format!("Missing ')' in default properties subscript: {s}"));
                return (0, i);
            }
            i += 1;
        }
        let index_text = s[start..i].to_string();
        if i >= b.len() {
            self.warn(format!("Missing ')' in default properties subscript: {s}"));
            return (0, i);
        }
        i += 1;
        if index_text.is_empty() {
            self.warn(format!("Invalid subscript in default properties: {s}"));
            return (0, i);
        }
        if index_text.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false) {
            if let Some(v) = self.res().global_enum_tag(&index_text) {
                return (v, i);
            }
            let mut c = self.res().find_field(Some(scope), &index_text, true, None).filter(|f| self.res().kind_of(*f) == FieldKind::Const);
            if c.is_none() {
                if let Some(t) = top_outer {
                    c = self.res().find_field(Some(t), &index_text, true, None).filter(|f| self.res().kind_of(*f) == FieldKind::Const);
                }
            }
            match c {
                Some(cv) => (self.res().const_value(cv).trim().parse::<i32>().unwrap_or(0), i),
                None => {
                    self.warn(format!("Invalid subscript in default properties: {s}"));
                    (0, i)
                }
            }
        } else {
            (index_text.trim().parse::<i32>().unwrap_or(0), i)
        }
    }

    // ----- ImportProperties ---------------------------------------------------------------

    /// `ImportProperties`. `object_struct` is the class/struct that owns `data`.
    fn import_properties(&mut self, cur: &mut Cursor, object_struct: FieldRef, data: &mut Vec<Slot>, ctx: &ImportCtx) -> DResult<()> {
        let subobjects_allowed = self.res().kind_of(object_struct) != FieldKind::ScriptStruct && ctx.outer.is_some();
        let is_subobject = ctx.root.map(|r| self.is_cdo_node(r)).unwrap_or(false);
        let root = ctx.root.or(ctx.outer);
        let owner_class = ctx.owner_class;
        let mut defined: Vec<(FieldRef, i32)> = Vec::new();
        while let Some(line) = cur.next_line() {
            let str_ = line.as_str();
            if str_.trim().is_empty() {
                continue;
            }
            if let Some(rest) = get_begin(str_, "Object") {
                if !subobjects_allowed {
                    return Err(derr("BEGIN OBJECT: Subobjects are not allowed in this context"));
                }
                let replacements = self.import_subobject(cur, rest, str_, root, ctx)?;
                if !replacements.is_empty() {
                    replace_refs_slots(data, &|r| replacements.iter().find(|(o, _)| *o == r).map(|(_, n)| *n));
                }
                continue;
            }
            if get_end(str_, "Actor") || get_end(str_, "DefaultProperties") || get_end(str_, "structdefaultproperties") || (get_end(str_, "Object") && ctx.depth > 0) {
                break;
            }
            // Property assignment.
            let s = skip_ws(str_);
            let b = s.as_bytes();
            let mut i = 0;
            while i < b.len() && b[i] != b'=' && b[i] != b'(' && b[i] != b'[' && b[i] != b'.' {
                i += 1;
            }
            if i >= b.len() {
                continue;
            }
            let token = s[..i].trim_end_matches([' ', '\t']).to_string();
            let mut rest = &s[i..];
            #[derive(PartialEq)]
            enum ArrayOp {
                None,
                Add,
                Remove,
                RemoveIndex,
                Empty,
            }
            let mut op = ArrayOp::None;
            if rest.starts_with('.') {
                rest = &rest[1..];
                let l = rest.to_ascii_lowercase();
                if l.starts_with("empty") {
                    op = ArrayOp::Empty;
                    rest = &rest[5..];
                } else if l.starts_with("add") && !l.starts_with("addi") {
                    op = ArrayOp::Add;
                    rest = &rest[3..];
                } else if l.starts_with("removeindex") {
                    op = ArrayOp::RemoveIndex;
                    rest = &rest[11..];
                } else if l.starts_with("remove") {
                    op = ArrayOp::Remove;
                    rest = &rest[6..];
                }
            }
            let mut prop = self.res().find_field(Some(object_struct), &token, true, None).filter(|f| self.res().is_property(*f));
            if prop.is_none() {
                let dn = format!("__{token}__Delegate");
                prop = self.res().find_field(Some(object_struct), &dn, true, None).filter(|f| self.res().prop_kind(*f) == Some(PropertyKind::Delegate));
                if prop.is_none() {
                    self.warn(format!("Unknown property in defaults: {} (looked in {})", str_.trim(), self.res().name_of(object_struct)));
                    continue;
                }
            }
            let prop = prop.unwrap();
            let kind = self.prop_kind_of(prop);
            if op != ArrayOp::None {
                if kind != PropertyKind::Array {
                    self.warn(format!("Array operation performed on non-array variable: {}", str_.trim()));
                    continue;
                }
                let inner = self.res().prop_refs(prop).0.ok_or_else(|| derr("array without inner"))?;
                let mut arr = find_slot(data, prop).map(|s| s.elems[0].clone()).unwrap_or(Value::Array(Vec::new()));
                let Value::Array(items) = &mut arr else { continue };
                match op {
                    ArrayOp::Empty => items.clear(),
                    ArrayOp::Add | ArrayOp::Remove => {
                        let r = skip_ws(rest);
                        if !r.starts_with('(') {
                            self.warn(format!("Missing '(' in default properties array operation: {}", str_.trim()));
                            continue;
                        }
                        let vt = read_struct_value(skip_ws(&r[1..]), cur);
                        let vt = vt.trim_end_matches(')').to_string();
                        let vt = vt.trim_end_matches([';', ' ', '\t']).to_string();
                        if op == ArrayOp::Add {
                            let mut elem = self.zero_value(inner);
                            let ok = if self.prop_kind_of(inner) == PropertyKind::Delegate {
                                let t = if vt.contains('.') { vt.clone() } else { format!("{}.{}", if ctx.depth > 0 { self.node_name(root) } else { self.res().name_of(object_struct) }, vt) };
                                self.import_text(inner, &t, &mut elem, ctx, object_struct).is_some()
                            } else {
                                self.import_text(inner, &vt, &mut elem, ctx, object_struct).is_some()
                            };
                            if ok {
                                items.push(elem);
                            } else {
                                self.warn(format!("Unable to parse parameter value '{}' in defaultproperties array operation: {}", vt, str_.trim()));
                            }
                        } else {
                            let mut temp = self.raw_zero_value(inner);
                            if self.import_text(inner, &vt, &mut temp, ctx, object_struct).is_none() {
                                self.warn(format!("Unable to parse parameter value '{}' in defaultproperties array operation: {}", vt, str_.trim()));
                            } else {
                                let before = items.len();
                                // `Components.Remove(X)` on an actor records the component
                                // (`RemovedComponents`) for `RemoveComponentsFromClass`.
                                let is_components_array = self.res().name_of(prop).eq_ignore_ascii_case("Components")
                                    && self.res().owner_class(prop).map(|c| self.res().name_of(c).eq_ignore_ascii_case("Actor")).unwrap_or(false);
                                let mut removed: Vec<FieldRef> = Vec::new();
                                {
                                    let me: &Compiler = &*self;
                                    items.retain(|x| {
                                        let same = me.identical(&temp, Some(x));
                                        if same && is_components_array {
                                            if let Value::Object(Some(r)) = x {
                                                removed.push(*r);
                                            }
                                        }
                                        !same
                                    });
                                }
                                for r in removed {
                                    if !self.removed_components.contains(&r) {
                                        self.removed_components.push(r);
                                    }
                                }
                                if items.len() == before {
                                    self.warn(format!("{}.Remove(): Value not found in array", self.res().name_of(prop)));
                                }
                            }
                        }
                    }
                    ArrayOp::RemoveIndex => {
                        let r = skip_ws(rest);
                        if !r.starts_with('(') {
                            self.warn(format!("Missing '(' in default properties array operation:: {}", str_.trim()));
                            continue;
                        }
                        let idx_text: String = r[1..].chars().take_while(|c| *c != ')').collect();
                        let idx = idx_text.trim().parse::<usize>().unwrap_or(0);
                        if idx < items.len() {
                            items.remove(idx);
                        }
                    }
                    ArrayOp::None => {}
                }
                if let Some(s) = find_slot_mut(data, prop) {
                    s.elems[0] = arr;
                }
                continue;
            }
            let (index, used) = self.read_array_index(rest, object_struct, owner_class.filter(|_| is_subobject));
            rest = &rest[used..];
            if kind != PropertyKind::Array && index >= self.res().prop_array_dim(prop).max(1) {
                self.warn(format!("Out of bound array default property ({}/{}): {}", index, self.res().prop_array_dim(prop), str_.trim()));
                continue;
            }
            if defined.iter().any(|(p, i)| *p == prop && *i == index) {
                self.warn(format!("redundant data: {}", str_.trim()));
                continue;
            }
            defined.push((prop, index));
            let r = skip_ws(rest);
            if !r.starts_with('=') {
                self.warn(format!("Missing '=' in default properties assignment: {}", str_.trim()));
                continue;
            }
            let full = read_struct_value(skip_ws(&r[1..]), cur);
            let mut vt = full.as_str();
            if !self.res().name_of(prop).eq_ignore_ascii_case("Name") {
                vt = vt.trim_end_matches([';', ' ', '\t']);
                if kind == PropertyKind::Str && !(vt.starts_with('"') && vt.ends_with('"') && vt.len() >= 2) {
                    self.warn(format!("Missing '\"' in string default properties: {}", str_.trim()));
                }
                let vt = vt.to_string();
                if index > -1 && kind == PropertyKind::Array {
                    // single dynamic array element
                    let inner = self.res().prop_refs(prop).0.ok_or_else(|| derr("array without inner"))?;
                    let mut arr = find_slot(data, prop).map(|s| s.elems[0].clone()).unwrap_or(Value::Array(Vec::new()));
                    if let Value::Array(items) = &mut arr {
                        while (index as usize) >= items.len() {
                            let z = self.zero_value(inner);
                            items.push(z);
                        }
                        let mut elem = items[index as usize].clone();
                        let text = if self.prop_kind_of(inner) == PropertyKind::Delegate {
                            if vt.contains('.') {
                                vt.clone()
                            } else {
                                format!("{}.{}", if ctx.depth > 0 { self.node_name(root) } else { self.res().name_of(object_struct) }, vt)
                            }
                        } else {
                            vt.clone()
                        };
                        let mut ok = self.import_text(inner, &text, &mut elem, ctx, object_struct).is_some();
                        if !ok {
                            if let Some(cv) = self.find_const_in_scope(&vt, object_struct, owner_class, is_subobject) {
                                ok = self.import_text(inner, &cv, &mut elem, ctx, object_struct).is_some();
                            }
                        }
                        if ok {
                            items[index as usize] = elem;
                        } else {
                            self.warn(format!("Invalid property value in defaults: {}", str_.trim()));
                        }
                    }
                    if let Some(s) = find_slot_mut(data, prop) {
                        s.elems[0] = arr;
                    }
                } else if kind == PropertyKind::Delegate {
                    let idx = index.max(0) as usize;
                    let text = if vt.contains('.') { vt.clone() } else { format!("{}.{}", if ctx.depth > 0 { self.node_name(root) } else { self.res().name_of(object_struct) }, vt) };
                    let mut elem = Value::Delegate(None, "None".into());
                    if vt.trim().eq_ignore_ascii_case("None") || self.import_text(prop, &text, &mut elem, ctx, object_struct).is_some() {
                        if let Some(s) = find_slot_mut(data, prop) {
                            if idx < s.elems.len() {
                                s.elems[idx] = elem;
                            }
                        }
                    } else {
                        self.warn(format!("Delegate assignment failed: {}", str_.trim()));
                    }
                } else {
                    let idx = index.max(0) as usize;
                    let mut elem = find_slot(data, prop).and_then(|s| s.elems.get(idx).cloned()).unwrap_or(Value::Int(0));
                    let mut ok = self.import_text(prop, &vt, &mut elem, ctx, object_struct).is_some();
                    if !ok {
                        if let Some(cv) = self.find_const_in_scope(&vt, object_struct, owner_class, is_subobject) {
                            elem = find_slot(data, prop).and_then(|s| s.elems.get(idx).cloned()).unwrap_or(Value::Int(0));
                            ok = self.import_text(prop, &cv, &mut elem, ctx, object_struct).is_some();
                        }
                    }
                    if ok {
                        if let Some(s) = find_slot_mut(data, prop) {
                            if idx < s.elems.len() {
                                s.elems[idx] = elem;
                            }
                        }
                    } else {
                        self.warn(format!("Invalid property value in defaults: {}", str_.trim()));
                    }
                }
            }
        }
        Ok(())
    }

    fn node_name(&self, n: Option<NodeId>) -> String {
        n.map(|x| self.model.node(x).name.clone()).unwrap_or_default()
    }

    fn find_const_in_scope(&self, name: &str, object_struct: FieldRef, owner_class: Option<FieldRef>, is_subobject: bool) -> Option<String> {
        let first = if is_subobject { owner_class } else { Some(object_struct) };
        let mut c = first.and_then(|s| self.res().find_field(Some(s), name, true, None)).filter(|f| self.res().kind_of(*f) == FieldKind::Const);
        if c.is_none() && is_subobject {
            c = self.res().find_field(Some(object_struct), name, true, None).filter(|f| self.res().kind_of(*f) == FieldKind::Const);
        }
        c.map(|cv| self.res().const_value(cv))
    }

    /// `Begin Object ...` handling.
    fn import_subobject(&mut self, cur: &mut Cursor, rest: &str, line: &str, root: Option<NodeId>, ctx: &ImportCtx) -> DResult<Vec<(FieldRef, FieldRef)>> {
        let outer = ctx.outer.ok_or_else(|| derr("BEGIN OBJECT: no outer"))?;
        let mut replacements: Vec<(FieldRef, FieldRef)> = Vec::new();
        let mut template_class = parse_kv(rest, "Class=").and_then(|c| self.res().find_class(&c));
        let template_name = parse_kv(rest, "Name=").unwrap_or_default();
        if template_name.is_empty() || template_name.eq_ignore_ascii_case("None") {
            return Err(derr(format!("BEGIN OBJECT: Must specify valid name for subobject/component: {}", line.trim())));
        }
        let owner_class = ctx.owner_class.ok_or_else(|| derr("BEGIN OBJECT: no owner class"))?;
        let mut base_template: Option<FieldRef> = None;
        if let Some(tc) = template_class {
            if self.res().class_flags(tc) & (cf::COMPILED | cf::INTRINSIC) == 0 && matches!(tc, FieldRef::Local(_)) {
                return Err(derr(format!("BEGIN OBJECT: Can't create subobject as Class {} hasn't been compiled yet.", self.res().name_of(tc))));
            }
            if self.res().class_flags(tc) & cf::NEEDS_DEF_PROPS != 0 {
                return Err(derr(format!("BEGIN OBJECT: defaults of class {} are not available yet (circular BEGIN OBJECT dependency)", self.res().name_of(tc))));
            }
        } else {
            if !self.is_cdo_node(outer) {
                return Err(derr(format!("BEGIN OBJECT: Missing class in subobject/component definition: {}", line.trim())));
            }
            let parent_class = self.res().super_of(owner_class).ok_or_else(|| derr("BEGIN OBJECT: no parent class"))?;
            let parent_cdo = self.class_cdo_ref(parent_class).ok_or_else(|| derr("BEGIN OBJECT: no parent CDO"))?;
            base_template = self.find_subobject(parent_cdo, &template_name);
            let Some(bt) = base_template else {
                return Err(derr(format!(
                    "BEGIN OBJECT: No base template named {} found in parent class {}: {}",
                    template_name,
                    self.res().name_of(parent_class),
                    line.trim()
                )));
            };
            template_class = self.obj_class(bt);
        }
        let template_class = template_class.ok_or_else(|| derr("BEGIN OBJECT: unresolved class"))?;
        let outer_flags = self.model.node(outer).object_flags;
        let tcf = self.res().class_flags(template_class);
        let per_obj_loc = tcf & cf::LOCALIZED != 0 || (tcf & (cf::PER_OBJECT_CONFIG | cf::LOCALIZED)) == (cf::PER_OBJECT_CONFIG | cf::LOCALIZED) || tcf & cf::PER_OBJECT_LOCALIZED != 0;
        let new_flags = outer_flags & (rf::PUBLIC | rf::ARCHETYPE_OBJECT | rf::TRANSACTIONAL) | if per_obj_loc { rf::PER_OBJECT_LOCALIZED } else { 0 };
        let is_component = self.res().is_component_class(template_class);
        if is_component && self.is_cdo_node(outer) {
            let override_component = self.model.node_of_class(owner_class).and_then(|oc| self.model.node(oc).class.component_map.iter().find(|(n, _)| n.eq_ignore_ascii_case(&template_name)).and_then(|(_, r)| *r));
            if let Some(oc) = override_component {
                if base_template.is_none() {
                    return Err(derr(format!(
                        "BEGIN OBJECT: The component name {} is already used (if you want to override the component, don't specify a class): {}",
                        template_name,
                        line.trim()
                    )));
                }
                if Some(oc) != base_template {
                    return Err(derr(format!("OverrideComponent mismatch for {template_name}")));
                }
            }
            let archetype = override_component.or_else(|| self.class_cdo_ref(template_class));
            let mut n = Node::new(NodeKind::Object, &template_name, Some(outer));
            n.object_flags = new_flags;
            n.object_class = Some(template_class);
            n.archetype = archetype;
            n.component_template = Some((None, Some(template_name.clone())));
            n.native_rest = self.native_rest_of(archetype.or_else(|| self.class_cdo_ref(template_class)));
            if n.native_rest.is_empty() {
                n.native_rest = self.native_rest_by_path(Some(outer), &template_name, Some(template_class));
            }
            let id = self.model.add(n);
            let mut values = self.initial_object_values(Some(template_class), archetype);
            if override_component.is_some() {
                // Constructed from the inherited template with instancing on: its
                // instanced sub-objects get copies under the override.
                self.instance_subobject_templates(&mut values, id);
            }
            if let Some(oc) = override_component {
                // FArchiveReplaceObjectRef on the outer: old component → new one.
                replacements.push((oc, FieldRef::Local(id)));
            }
            let sub_ctx = ImportCtx { root: root.or(Some(outer)), outer: Some(id), owner_class: Some(owner_class), depth: ctx.depth + 1 };
            self.import_properties(cur, template_class, &mut values, &sub_ctx)?;
            self.model.node_mut(id).values = Some(values);
            self.values.object_values.remove(&FieldRef::Local(id));
            let is_subobject = ctx.root.map(|r| self.is_cdo_node(r)).unwrap_or(false);
            if !is_subobject || Some(outer) == ctx.root {
                if let Some(ocn) = self.model.node_of_class(owner_class) {
                    let map = &mut self.model.node_mut(ocn).class.component_map;
                    match map.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(&template_name)) {
                        Some(e) => e.1 = Some(FieldRef::Local(id)),
                        None => map.push((template_name.clone(), Some(FieldRef::Local(id)))),
                    }
                }
            }
        } else {
            if is_component && root.map(|r| self.is_cdo_node(r)).unwrap_or(false) {
                return Err(derr(format!("Nested component definitions ({} in {}) in class files are currently not supported: {}", template_name, self.model.node(outer).name, line.trim())));
            }
            if let Some(actor) = self.actor_class() {
                if self.res().is_child_of(template_class, actor) {
                    return Err(derr(format!("Cannot create subobjects from Actor-derived classes: {}", line.trim())));
                }
            }
            let object_name = parse_kv(rest, "ObjName=").unwrap_or_else(|| template_name.clone());
            let mut archetype: Option<FieldRef> = None;
            if let Some(arch_text) = parse_kv(rest, "Archetype=") {
                let parts: Vec<&str> = arch_text.split('\'').filter(|s| !s.is_empty()).collect();
                if parts.len() >= 2 {
                    if let Some(ac) = self.res().find_class(parts[0]) {
                        archetype = self.find_object_literal(ac, parts[1]);
                    }
                }
            } else {
                archetype = base_template;
            }
            if let Some(existing) = self.find_subobject(FieldRef::Local(outer), &object_name) {
                if base_template.is_none() {
                    if let FieldRef::Local(eid) = existing {
                        if self.model.node(eid).object_class == Some(template_class) && self.model.node(eid).outer == Some(outer) {
                            // Instanced from the template already (`InstanceSubobjectTemplates`):
                            // the block refines that copy.
                            let mut values = self.object_values(existing);
                            let sub_ctx = ImportCtx { root: root.or(Some(outer)), outer: Some(eid), owner_class: Some(owner_class), depth: ctx.depth + 1 };
                            self.import_properties(cur, template_class, &mut values, &sub_ctx)?;
                            self.model.node_mut(eid).values = Some(values);
                            self.values.object_values.remove(&existing);
                            return Ok(replacements);
                        }
                    }
                }
                if self.object_archetype(existing) != base_template {
                    return Err(derr(format!("BEGIN OBJECT: name {} redefined: {}", object_name, line.trim())));
                } else if base_template.is_none() {
                    return Err(derr(format!(
                        "BEGIN OBJECT: A subobject named {} is already declared in a parent class.  If you intended to override that subobject, don't specify a class in the derived subobject definition: {}",
                        template_name,
                        line.trim()
                    )));
                }
            }
            // A fresh sub-object is constructed from its class default object
            // (`StaticConstructObject` with the CDO as template): that is its archetype.
            let archetype = archetype.or_else(|| self.class_cdo_ref(template_class));
            let mut n = Node::new(NodeKind::Object, &object_name, Some(outer));
            n.object_flags = new_flags;
            n.object_class = Some(template_class);
            n.archetype = archetype;
            n.native_rest = self.native_rest_of(archetype);
            if n.native_rest.is_empty() {
                n.native_rest = self.native_rest_by_path(Some(outer), &object_name, Some(template_class));
            }
            if is_component {
                n.component_template = Some((None, Some(template_name.clone())));
            }
            let id = self.model.add(n);
            let mut values = self.initial_object_values(Some(template_class), archetype);
            if let Some(a) = archetype {
                let a_is_cdo = match a {
                    FieldRef::Local(x) => self.is_cdo_node(x),
                    FieldRef::External(r) => self.ctx.object_flags(r) & rf::CLASS_DEFAULT_OBJECT != 0,
                };
                if !a_is_cdo {
                    replacements.push((a, FieldRef::Local(id)));
                }
            }
            // `InstanceSubobjectTemplates`: the template's own sub-objects are copied
            // under the new object and references to them redirected.
            {
                let template = archetype.or_else(|| self.class_cdo_ref(template_class));
                let mut inst: Vec<(FieldRef, FieldRef)> = Vec::new();
                if let Some(t) = template {
                    let v = values.clone();
                    self.instance_components_in(&v, t, id, &mut inst);
                }
                if !inst.is_empty() {
                    replace_refs_slots(&mut values, &|r| inst.iter().find(|(o, _)| *o == r).map(|(_, n)| *n));
                }
            }
            let sub_ctx = ImportCtx { root: root.or(Some(outer)), outer: Some(id), owner_class: Some(owner_class), depth: ctx.depth + 1 };
            self.import_properties(cur, template_class, &mut values, &sub_ctx)?;
            self.model.node_mut(id).values = Some(values);
            self.values.object_values.remove(&FieldRef::Local(id));
        }
        Ok(replacements)
    }

    /// Sub-objects directly inside `obj` (local nodes or loaded exports).
    fn subobjects_of(&self, obj: FieldRef) -> Vec<FieldRef> {
        match obj {
            FieldRef::Local(o) => (0..self.model.nodes.len())
                .filter(|&i| self.model.node(i).kind == NodeKind::Object && self.model.node(i).outer == Some(o))
                .map(FieldRef::Local)
                .collect(),
            FieldRef::External(r) => {
                let lp = &self.ctx.packages[r.pkg];
                (0..lp.pkg.exports.len())
                    .filter(|&e| lp.pkg.exports[e].outer == tempest_upk::io::PackageIndex::export(r.export))
                    .map(|e| crate::context::ObjRef { pkg: r.pkg, export: e })
                    .filter(|x| self.ctx.property_kind(*x).is_none() && !matches!(self.ctx.class_name(*x).as_str(), "Class" | "Function" | "State" | "ScriptStruct" | "Enum" | "Const"))
                    .map(FieldRef::External)
                    .collect()
            }
        }
    }

    /// Components referenced from `values` that live inside `template` are instanced
    /// under `new_outer` (component instancing stays on while a `Begin Object`
    /// sub-object is constructed from its class defaults; plain sub-object instancing
    /// is off, and deprecated properties are not followed).
    fn instance_components_in(&mut self, values: &[Slot], template: FieldRef, new_outer: NodeId, inst: &mut Vec<(FieldRef, FieldRef)>) {
        let refs = {
            let me: &Compiler = &*self;
            collect_object_refs_filtered(values, &|p| me.res().prop_flags(p) & cpf::DEPRECATED != 0)
        };
        for r in refs {
            if inst.iter().any(|(o, _)| *o == r) {
                continue;
            }
            let Some(sub_class) = self.obj_class(r) else { continue };
            if !self.res().is_component_class(sub_class) || !self.is_inside_ref(r, template) {
                continue;
            }
            let name = match r {
                FieldRef::Local(x) => self.model.node(x).name.clone(),
                FieldRef::External(o) => self.ctx.name(o),
            };
            let mut n = Node::new(NodeKind::Object, &name, Some(new_outer));
            n.object_flags = rf::PUBLIC | if self.res().class_flags(sub_class) & cf::LOCALIZED != 0 { rf::PER_OBJECT_LOCALIZED } else { 0 };
            n.object_class = Some(sub_class);
            n.archetype = Some(r);
            n.component_template = Some((None, Some(name.clone())));
            n.native_rest = self.native_rest_of(Some(r));
            if n.native_rest.is_empty() {
                n.native_rest = self.native_rest_by_path(Some(new_outer), &name, Some(sub_class));
            }
            let id = self.model.add(n);
            let cv = self.object_values(r);
            self.model.node_mut(id).values = Some(cv.clone());
            inst.push((r, FieldRef::Local(id)));
            self.instance_components_in(&cv, template, id, inst);
        }
        // Redirect references among the copies to each other.
        let ids: Vec<NodeId> = inst.iter().filter_map(|(_, n)| if let FieldRef::Local(i) = n { Some(*i) } else { None }).collect();
        for id in ids {
            if let Some(mut v) = self.model.node(id).values.clone() {
                replace_refs_slots(&mut v, &|r| inst.iter().find(|(o, _)| *o == r).map(|(_, n)| *n));
                self.model.node_mut(id).values = Some(v);
            }
        }
    }

    /// `UObjectProperty::InstanceSubobjects` for a freshly constructed template: every
    /// constructor-linked object property whose value is a template object (the source's
    /// sub-object) gets a copy of the same name under `new_outer`, recursively.
    fn instance_subobject_templates(&mut self, values: &mut Vec<Slot>, new_outer: NodeId) {
        for si in 0..values.len() {
            let prop = values[si].prop;
            let flags = self.res().prop_flags(prop);
            if flags & cpf::NEED_CTOR_LINK == 0 {
                continue;
            }
            let kind = self.res().prop_kind(prop);
            let inner = match kind {
                Some(PropertyKind::Object | PropertyKind::SafeObject) => prop,
                Some(PropertyKind::Array) => match self.res().prop_refs(prop).0 {
                    Some(i) if matches!(self.res().prop_kind(i), Some(PropertyKind::Object | PropertyKind::SafeObject)) && self.res().prop_flags(i) & cpf::NEED_CTOR_LINK != 0 => i,
                    _ => continue,
                },
                _ => continue,
            };
            let _ = inner;
            let mut elems = values[si].elems.clone();
            let mut changed = false;
            for e in elems.iter_mut() {
                match e {
                    Value::Object(Some(v)) => {
                        if let Some(c) = self.instance_subobject_copy(*v, new_outer) {
                            *e = Value::Object(Some(c));
                            changed = true;
                        }
                    }
                    Value::Array(items) => {
                        for it in items.iter_mut() {
                            if let Value::Object(Some(v)) = it {
                                if let Some(c) = self.instance_subobject_copy(*v, new_outer) {
                                    *it = Value::Object(Some(c));
                                    changed = true;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            if changed {
                values[si].elems = elems;
            }
        }
    }

    fn instance_subobject_copy(&mut self, v: FieldRef, new_outer: NodeId) -> Option<FieldRef> {
        if !self.is_template(v) || self.object_outer(v) == Some(FieldRef::Local(new_outer)) {
            return None;
        }
        let class = self.obj_class(v)?;
        if self.res().is_component_class(class) {
            return None;
        }
        let name = match v {
            FieldRef::Local(x) => self.model.node(x).name.clone(),
            FieldRef::External(o) => self.ctx.name(o),
        };
        let mut n = Node::new(NodeKind::Object, &name, Some(new_outer));
        n.object_flags = rf::PUBLIC | if self.res().class_flags(class) & cf::LOCALIZED != 0 { rf::PER_OBJECT_LOCALIZED } else { 0 };
        n.object_class = Some(class);
        n.archetype = Some(v);
        n.native_rest = self.native_rest_of(Some(v));
        if n.native_rest.is_empty() {
            n.native_rest = self.native_rest_by_path(Some(new_outer), &name, Some(class));
        }
        let id = self.model.add(n);
        let mut cv = self.object_values(v);
        self.instance_subobject_templates(&mut cv, id);
        self.model.node_mut(id).values = Some(cv);
        Some(FieldRef::Local(id))
    }

    fn is_inside_ref(&self, obj: FieldRef, root: FieldRef) -> bool {
        let mut cur = self.object_outer(obj);
        let mut guard = 0;
        while let Some(o) = cur {
            if o == root {
                return true;
            }
            cur = self.object_outer(o);
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        false
    }

    // ----- CopyInheritedComponents ------------------------------------------------------------

    fn copy_inherited_components(&mut self, class: NodeId, cdo: NodeId, values: &mut Vec<Slot>) {
        let entries = self.model.node(class).class.component_map.clone();
        let mut replacements: Vec<(FieldRef, FieldRef)> = Vec::new();
        for (name, comp) in entries {
            let Some(comp) = comp else { continue };
            let outer = self.object_outer(comp);
            if outer != Some(FieldRef::Local(cdo)) {
                let comp_class = self.obj_class(comp);
                let mut n = Node::new(NodeKind::Object, &name, Some(cdo));
                n.object_flags = rf::PUBLIC | if comp_class.map(|c| self.res().class_flags(c) & cf::LOCALIZED != 0).unwrap_or(false) { rf::PER_OBJECT_LOCALIZED } else { 0 };
                n.object_class = comp_class;
                n.archetype = Some(comp);
                n.component_template = Some((None, Some(name.clone())));
                n.native_rest = self.native_rest_of(Some(comp));
                if n.native_rest.is_empty() {
                    n.native_rest = self.native_rest_by_path(Some(cdo), &name, comp_class);
                }
                let id = self.model.add(n);
                let mut cv = self.initial_object_values(comp_class, Some(comp));
                // Component references inside the copy that point at the old owner's sub-objects
                // are instanced; without a counterpart they become None (instancing graph).
                let old_cdo = outer;
                let me = &*self;
                replace_refs_slots(&mut cv, &|r| {
                    if let Some(oc) = old_cdo {
                        let mut cur = me.object_outer(r);
                        let mut guard = 0;
                        while let Some(c) = cur {
                            if c == oc {
                                return Some(FieldRef::Local(usize::MAX));
                            }
                            cur = me.object_outer(c);
                            guard += 1;
                            if guard > 16 {
                                break;
                            }
                        }
                    }
                    None
                });
                for s in &mut cv {
                    for e in &mut s.elems {
                        clear_sentinel(e);
                    }
                }
                // `InstanceSubobjectTemplates`: instanced object properties still holding the
                // template's own sub-objects get copies under the new component.
                self.instance_subobject_templates(&mut cv, id);
                self.model.node_mut(id).values = Some(cv);
                replacements.push((comp, FieldRef::Local(id)));
                let map = &mut self.model.node_mut(class).class.component_map;
                if let Some(e) = map.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                    e.1 = Some(FieldRef::Local(id));
                }
            } else {
                let base = self.object_archetype(comp);
                if let Some(b) = base {
                    let b_is_cdo = match b {
                        FieldRef::Local(x) => self.is_cdo_node(x),
                        FieldRef::External(r) => self.ctx.object_flags(r) & rf::CLASS_DEFAULT_OBJECT != 0,
                    };
                    if !b_is_cdo {
                        replacements.push((b, comp));
                    }
                }
            }
        }
        if !replacements.is_empty() {
            replace_refs_slots(values, &|r| replacements.iter().find(|(o, _)| *o == r).map(|(_, n)| *n));
            // `FArchiveReplaceObjectRef` on the CDO also serializes every object inside it.
            let ids: Vec<NodeId> = (0..self.model.nodes.len())
                .filter(|&i| self.model.node(i).kind == NodeKind::Object && i != cdo && self.is_inside(i, cdo))
                .collect();
            for id in ids {
                if let Some(mut v) = self.model.node(id).values.clone() {
                    replace_refs_slots(&mut v, &|r| replacements.iter().find(|(o, _)| *o == r).map(|(_, n)| *n));
                    self.model.node_mut(id).values = Some(v);
                }
            }
        }
    }

    // ----- struct defaults ---------------------------------------------------------------------

    /// Compile `structdefaultproperties` for every local script struct.
    fn compile_struct_defaults(&mut self, class: NodeId) -> DResult<()> {
        self.class = class;
        let structs: Vec<NodeId> = self.model.nodes.iter().enumerate().filter(|(_, n)| n.kind == NodeKind::ScriptStruct && self.model.owner_class(n.outer.unwrap_or(usize::MAX)) == Some(class)).map(|(i, _)| i).collect();
        for st in structs {
            self.compile_one_struct_defaults(st, class)?;
        }
        Ok(())
    }

    fn compile_one_struct_defaults(&mut self, st: NodeId, class: NodeId) -> DResult<()> {
        if self.model.node(st).values.is_some() {
            return Ok(());
        }
        // Parent and nested struct defaults first.
        if let Some(FieldRef::Local(p)) = self.model.node(st).strct.super_struct {
            self.compile_one_struct_defaults(p, class)?;
        }
        let nested: Vec<NodeId> = self
            .model
            .node(st)
            .strct
            .children
            .iter()
            .copied()
            .filter_map(|c| match (&self.model.node(c).kind, self.model.node(c).prop.ref1) {
                (NodeKind::Property(PropertyKind::Struct), Some(FieldRef::Local(s))) if self.model.node(c).prop.flags & cpf::NATIVE == 0 => Some(s),
                _ => None,
            })
            .collect();
        for n in nested {
            if n != st {
                self.compile_one_struct_defaults(n, class)?;
            }
        }
        let sref = FieldRef::Local(st);
        self.values.struct_defaults.remove(&sref);
        let mut slots = self.struct_defaults(sref);
        if let Some((text, _)) = self.model.node(st).class.default_props_text.clone() {
            let (line, lines) = split_defaults_text(&text);
            let mut cur = Cursor { lines, pos: 0, line_no: line };
            let ctx = ImportCtx { root: None, outer: None, owner_class: Some(FieldRef::Local(class)), depth: 0 };
            self.import_properties(&mut cur, sref, &mut slots, &ctx)?;
        }
        self.model.node_mut(st).values = Some(slots);
        self.values.struct_defaults.remove(&sref);
        Ok(())
    }

    // ----- class defaults -----------------------------------------------------------------------

    /// Whether the class's defaults can be imported now: its parent's defaults must
    /// exist, and every `Begin Object Class=X` template class must have imported its
    /// own defaults (`ImportProperties` defers otherwise).
    fn class_defaults_ready(&self, class: NodeId) -> bool {
        if let Some(FieldRef::Local(p)) = self.model.node(class).strct.super_struct {
            if self.model.node(p).class.class_flags & cf::NEEDS_DEF_PROPS != 0 {
                return false;
            }
        }
        let text = self.model.node(class).class.default_props_text.clone().map(|(t, _)| t).unwrap_or_default();
        for line in text.lines() {
            let l = line.trim_start();
            if !l.to_ascii_lowercase().starts_with("begin object") {
                continue;
            }
            if let Some(c) = parse_kv(&l["begin object".len()..], "Class=") {
                if let Some(FieldRef::Local(tc)) = self.res().find_class(&c) {
                    if tc != class && self.model.node(tc).class.class_flags & cf::NEEDS_DEF_PROPS != 0 {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn compile_class_defaults(&mut self, class: NodeId) -> DResult<()> {
        self.class = class;
        let class_ref = FieldRef::Local(class);
        let super_class = self.model.node(class).strct.super_struct;
        let parent_cdo = super_class.and_then(|s| self.class_cdo_ref(s));
        let name = format!("Default__{}", self.model.node(class).name);
        let mut n = Node::new(NodeKind::Object, &name, None);
        n.object_flags = rf::CLASS_DEFAULT_OBJECT | rf::PUBLIC;
        {
            let cflags = self.model.node(class).class.class_flags;
            if (cflags & (cf::PER_OBJECT_CONFIG | cf::LOCALIZED)) == (cf::PER_OBJECT_CONFIG | cf::LOCALIZED) || cflags & cf::PER_OBJECT_LOCALIZED != 0 {
                n.object_flags |= rf::PER_OBJECT_LOCALIZED;
            }
        }
        n.object_class = Some(class_ref);
        n.archetype = parent_cdo;
        let cdo = self.model.add(n);
        self.model.node_mut(class).class.cdo = Some(cdo);
        let mut values = self.initial_object_values(Some(class_ref), parent_cdo);
        // ComponentNameToDefaultObjectMap starts as the parent's.
        if let Some(sc) = super_class {
            let parent_map: Vec<(String, Option<FieldRef>)> = match sc {
                FieldRef::Local(p) => self.model.node(p).class.component_map.clone(),
                FieldRef::External(o) => {
                    let lp = &self.ctx.packages[o.pkg];
                    match self.ctx.object(o).map(|x| &x.body) {
                        Some(crate::objects::ObjectBody::Class { class: cd, .. }) => {
                            cd.component_name_to_default_object.iter().map(|(n, i)| (lp.pkg.fname_to_string(*n), self.res().resolve_field(o.pkg, *i))).collect()
                        }
                        _ => Vec::new(),
                    }
                }
            };
            self.model.node_mut(class).class.component_map = parent_map;
        }
        let text = self.model.node(class).class.default_props_text.clone().map(|(t, _)| t).unwrap_or_default();
        let (line, lines) = split_defaults_text(&text);
        let mut cur = Cursor { lines, pos: 0, line_no: line };
        let ctx = ImportCtx { root: None, outer: Some(cdo), owner_class: Some(class_ref), depth: 0 };
        self.removed_components.clear();
        self.import_properties(&mut cur, class_ref, &mut values, &ctx)?;
        // RemoveComponentsFromClass: a component taken out of `Components` that nothing in
        // the class default object refers to any more leaves the component map.
        let removed = std::mem::take(&mut self.removed_components);
        if !removed.is_empty() {
            let refs = collect_object_refs(&values);
            for r in removed {
                if refs.contains(&r) {
                    continue;
                }
                let name = match r {
                    FieldRef::Local(x) => self.model.node(x).name.clone(),
                    FieldRef::External(o) => self.ctx.name(o),
                };
                self.model.node_mut(class).class.component_map.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
            }
        }
        // CopyInheritedComponents
        if let Some(sc) = super_class {
            let has_components = self.res().class_flags(sc) & cf::HAS_COMPONENTS != 0;
            if has_components && !self.model.node(class).class.component_map.is_empty() {
                self.copy_inherited_components(class, cdo, &mut values);
            }
        }
        // Duplicate inherited (non-component) sub-objects referenced by the CDO.
        if let Some(pc) = parent_cdo {
            let mut dups: Vec<(FieldRef, NodeId)> = Vec::new();
            let refs = collect_object_refs(&values);
            for r in refs {
                if self.object_outer(r) == Some(pc) && !self.obj_class(r).map(|c| self.res().is_component_class(c)).unwrap_or(false) {
                    if dups.iter().any(|(o, _)| *o == r) {
                        continue;
                    }
                    let name = match r {
                        FieldRef::Local(x) => self.model.node(x).name.clone(),
                        FieldRef::External(o) => self.ctx.name(o),
                    };
                    let mut n = Node::new(NodeKind::Object, &name, Some(cdo));
                    n.object_flags = rf::PUBLIC;
                    n.object_class = self.obj_class(r);
                    n.archetype = Some(r);
                    n.native_rest = self.native_rest_of(Some(r));
                    if n.native_rest.is_empty() {
                        n.native_rest = self.native_rest_by_path(Some(cdo), &name, self.obj_class(r));
                    }
                    let id = self.model.add(n);
                    let cv = self.object_values(r);
                    self.model.node_mut(id).values = Some(cv);
                    dups.push((r, id));
                }
            }
            if !dups.is_empty() {
                replace_refs_slots(&mut values, &|r| dups.iter().find(|(o, _)| *o == r).map(|(_, n)| FieldRef::Local(*n)));
            }
        }
        self.model.node_mut(cdo).values = Some(values);
        self.values.object_values.remove(&FieldRef::Local(cdo));
        self.model.node_mut(class).class.class_flags &= !cf::NEEDS_DEF_PROPS;
        propagate_struct_defaults(self, class);
        Ok(())
    }

    /// Encode the tagged properties of every local defaults object and struct.
    fn encode_all_defaults(&mut self) {
        let n = self.model.nodes.len();
        for i in 0..n {
            match self.model.node(i).kind.clone() {
                NodeKind::ScriptStruct => {
                    let sref = FieldRef::Local(i);
                    let slots = self.struct_defaults(sref);
                    // UScriptStruct::Serialize: DefaultsStruct = super struct, Defaults = super's defaults.
                    let sup = self.res().inheritance_super(sref);
                    let sup_defaults = sup.map(|p| self.struct_defaults(p));
                    let atomic = sup.map(|p| self.res().struct_flags(p) & tempest_upk::flags::strct::ATOMIC != 0 || (self.cooked && self.res().struct_flags(p) & tempest_upk::flags::strct::ATOMIC_WHEN_COOKED != 0)).unwrap_or(false);
                    self.bin_defaults_ctx = true;
                    let tags = self.encode_tagged(&slots, sref, sup_defaults.as_deref(), sup, false, true, atomic);
                    self.model.node_mut(i).default_props = tags;
                }
                NodeKind::Object => {
                    let Some(class) = self.model.node(i).object_class else { continue };
                    let values = self.object_values(FieldRef::Local(i));
                    let is_cdo = self.is_cdo_node(i);
                    let arch = self.model.node(i).archetype;
                    let defaults = arch.map(|a| self.object_values(a));
                    let defaults_class = arch.and_then(|a| self.obj_class(a));
                    self.bin_defaults_ctx = is_cdo;
                    let tags = self.encode_tagged(&values, class, defaults.as_deref(), defaults_class, true, is_cdo, false);
                    self.model.node_mut(i).default_props = tags;
                }
                _ => {}
            }
        }
    }
}

fn clear_sentinel(v: &mut Value) {
    match v {
        Value::Object(Some(FieldRef::Local(usize::MAX))) => *v = Value::Object(None),
        Value::Interface(Some(FieldRef::Local(usize::MAX))) => *v = Value::Interface(None),
        Value::Delegate(o, _) => {
            if *o == Some(FieldRef::Local(usize::MAX)) {
                *o = None;
            }
        }
        Value::Struct(slots) => {
            for s in slots {
                for e in &mut s.elems {
                    clear_sentinel(e);
                }
            }
        }
        Value::Array(items) => {
            for e in items {
                clear_sentinel(e);
            }
        }
        _ => {}
    }
}

/// `collect_object_refs` without the slots (at any depth) whose property `skip` selects.
fn collect_object_refs_filtered(slots: &[Slot], skip: &dyn Fn(FieldRef) -> bool) -> Vec<FieldRef> {
    fn walk(v: &Value, out: &mut Vec<FieldRef>, skip: &dyn Fn(FieldRef) -> bool) {
        match v {
            Value::Object(Some(r)) | Value::Interface(Some(r)) => out.push(*r),
            Value::Struct(s) => {
                for sl in s {
                    if skip(sl.prop) {
                        continue;
                    }
                    for e in &sl.elems {
                        walk(e, out, skip);
                    }
                }
            }
            Value::Array(items) => {
                for e in items {
                    walk(e, out, skip);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in slots {
        if skip(s.prop) {
            continue;
        }
        for e in &s.elems {
            walk(e, &mut out, skip);
        }
    }
    out
}

pub(super) fn collect_object_refs(slots: &[Slot]) -> Vec<FieldRef> {
    fn walk(v: &Value, out: &mut Vec<FieldRef>) {
        match v {
            Value::Object(Some(r)) | Value::Interface(Some(r)) => out.push(*r),
            Value::Struct(s) => {
                for sl in s {
                    for e in &sl.elems {
                        walk(e, out);
                    }
                }
            }
            Value::Array(items) => {
                for e in items {
                    walk(e, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in slots {
        for e in &s.elems {
            walk(e, &mut out);
        }
    }
    out
}

/// `appAtof`-like parse of a numeric prefix.
fn appatof(s: &str) -> f32 {
    let mut end = 0;
    let b = s.as_bytes();
    while end < b.len() && (b[end].is_ascii_digit() || matches!(b[end], b'.' | b'-' | b'+')) {
        end += 1;
    }
    s[..end].parse::<f32>().unwrap_or(0.0)
}

impl super::model::Model {
    /// Local node of a class reference.
    pub fn node_of_class(&self, c: FieldRef) -> Option<NodeId> {
        match c {
            FieldRef::Local(n) => Some(n),
            FieldRef::External(_) => None,
        }
    }
}

/// Entry point: struct defaults, then class defaults in parse order, then encoding.
pub fn compile_defaults(c: &mut Compiler, order: &[NodeId]) -> Result<(), DriverError> {
    for &class in order {
        c.compile_struct_defaults(class)?;
    }
    // Classes whose sub-object templates' defaults are not imported yet are deferred
    // and retried, as the engine does, until nothing is pending.
    let mut pending: Vec<NodeId> = order.to_vec();
    loop {
        let mut next: Vec<NodeId> = Vec::new();
        let mut progress = false;
        for class in pending {
            if c.class_defaults_ready(class) {
                c.compile_class_defaults(class)?;
                progress = true;
            } else {
                next.push(class);
            }
        }
        if next.is_empty() {
            break;
        }
        if !progress {
            let names: Vec<String> = next.iter().map(|&n| c.model.node(n).name.clone()).collect();
            return Err(DriverError(format!("BEGIN OBJECT: circular sub-object template dependency among: {}", names.join(", "))));
        }
        pending = next;
    }
    c.encode_all_defaults();
    Ok(())
}
