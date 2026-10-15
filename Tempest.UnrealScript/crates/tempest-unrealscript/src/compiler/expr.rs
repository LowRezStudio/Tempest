//! Expression compiler: ports of `GetToken` (with type hints), `CompileExpr`,
//! `CompileFieldExpr`, `CompileDynamicCast`, `CompileAffector`, the
//! conversion machinery and the emit helpers they use.

use tempest_upk::flags::{class as cf, cpf, func};

use super::engine::{allow, CResult, Compiler, ConstVal, Fixup, Location, NestType, TokKind, Token};
use super::model::FieldRef;
use super::scope::FieldKind;
use super::types::{
    conversion_index, get_conversion_raw, safe_object_conversion, Cpt, PropType, RefType, AUTOCONVERT, CONVERSION_EXPANSION,
    CONVERSION_INT_TO_FLOAT, CONVERSION_TRUNCATION, CONVERT_MASK, CST_MAX, MAXINT, TRUNCATE,
};
use crate::bytecode::{cast, op};
use crate::objects::PropertyKind;

/// `DEFAULT_OUTERCONTEXT_SIZE = 1 + sizeof(ScriptPointerType)`
const DEFAULT_OUTERCONTEXT_SIZE: u16 = 1 + 8;
const MAX_SIMPLE_RETURN_VALUE_SIZE: i32 = 64;

impl<'a> Compiler<'a> {
    // ----- helpers ---------------------------------------------------------

    pub fn class_class(&self) -> Option<FieldRef> {
        self.res().find_class("Class")
    }
    pub fn object_class(&self) -> Option<FieldRef> {
        self.res().find_class("Object")
    }
    pub fn actor_class(&self) -> Option<FieldRef> {
        self.res().find_class("Actor")
    }

    pub fn is_vector(&self, t: &PropType) -> bool {
        t.cpt == Cpt::Struct && t.inner.is_some() && t.inner == self.vector_struct
    }
    pub fn is_rotator(&self, t: &PropType) -> bool {
        t.cpt == Cpt::Struct && t.inner.is_some() && t.inner == self.rotator_struct
    }

    fn struct_name_is(&self, t: &PropType, name: &str) -> bool {
        t.cpt == Cpt::Struct && t.inner.map(|s| self.res().name_of(s).eq_ignore_ascii_case(name)).unwrap_or(false)
    }

    /// `GetConversion(Dest, Src)`.
    pub fn get_conversion(&self, dest: &PropType, src: &PropType) -> u32 {
        if let Some(c) = safe_object_conversion(dest, src, dest.safe && dest.is_object(), src.safe && src.is_object()) {
            return c;
        }
        let d = conversion_index(dest, self.is_vector(dest), self.is_rotator(dest));
        let s = conversion_index(src, self.is_vector(src), self.is_rotator(src));
        get_conversion_raw(d, s)
    }

    /// `FPropertyBase::MatchesType`.
    pub fn matches_type(&self, this: &PropType, other: &PropType, mut disallow_generalization: bool, ignore_implemented_interfaces: bool) -> bool {
        let r = self.res();
        let is_obj = this.is_object();
        let other_is_obj = other.is_object();
        let is_obj_cmp = is_obj && other_is_obj;
        let mut reverse_chain = true;
        if this.flags & cpf::OUT_PARM != 0 {
            if other.flags & cpf::OUT_PARM == 0 {
                return false;
            }
            if other.flags & cpf::CONST != 0 && this.flags & cpf::CONST == 0 {
                return false;
            }
            if this.cpt == Cpt::Struct {
                disallow_generalization = disallow_generalization || this.is_dynamic_array() || other.is_dynamic_array();
            } else if this.flags & cpf::CONST == 0 || !this.is_object() {
                disallow_generalization = true;
            } else if is_obj_cmp && this.cpt != other.cpt {
                return false;
            }
        } else if this.cpt == Cpt::Object
            && other.cpt != Cpt::Interface
            && (this.ref_type == RefType::AssignmentReference || this.ref_type == RefType::DualReference || this.flags & cpf::RETURN_PARM != 0)
        {
            reverse_chain = false;
        }
        if this.cpt == Cpt::None && (other.cpt == Cpt::None || !disallow_generalization) {
            return true;
        } else if this.cpt != other.cpt && !is_obj_cmp {
            return false;
        } else if this.array_dim != other.array_dim {
            return false;
        } else if this.cpt == Cpt::Byte {
            // The game's compiler also lets a plain byte stand in for an enum (its packages pass
            // byte locals to enum parameters without a cast).
            return this.inner == other.inner || ((this.inner.is_none() || other.inner.is_none()) && !disallow_generalization);
        } else if is_obj {
            let Some(pc) = this.inner else { return false };
            // A safe reference passes for a typed object (the game's packages pass them to
            // component parameters uncast) but needs the conversion for a plain `Object`
            // destination (operator parameters) and when the destination itself is safe.
            if this.safe != other.safe && (this.safe || Some(pc) == self.object_class()) {
                return false;
            }
            if disallow_generalization {
                return this.inner == other.inner && this.meta_class == other.meta_class;
            }
            let Some(opc) = other.inner else { return true };
            let class_class = self.class_class();
            if r.is_child_of(opc, pc) {
                if !ignore_implemented_interfaces || ((this.cpt == Cpt::Interface) == (other.cpt == Cpt::Interface)) {
                    let meta_ok = Some(pc) != class_class
                        || this.meta_class.is_none()
                        || match (other.meta_class, this.meta_class) {
                            (Some(om), Some(tm)) => r.is_child_of(om, tm) || (reverse_chain && r.is_child_of(tm, om)),
                            (None, _) => reverse_chain,
                            _ => false,
                        };
                    if meta_ok {
                        return true;
                    }
                }
            } else if reverse_chain && this.cpt != Cpt::Interface && is_obj_cmp && r.is_child_of(pc, opc) {
                let ok = Some(opc) != class_class
                    || this.meta_class.is_none()
                    || other.meta_class.is_none()
                    || match (this.meta_class, other.meta_class) {
                        (Some(tm), Some(om)) => r.is_child_of(tm, om) || r.is_child_of(om, tm),
                        _ => true,
                    };
                if ok {
                    return true;
                }
            }
            if r.class_is_interface(pc) && !ignore_implemented_interfaces && r.implements_interface(opc, pc) {
                return true;
            }
            return false;
        } else if this.cpt == Cpt::Struct {
            let (Some(s1), Some(s2)) = (this.inner, other.inner) else { return false };
            if s1 == s2 {
                return true;
            }
            if disallow_generalization {
                return false;
            }
            if !this.is_dynamic_array() && !other.is_dynamic_array() && r.is_child_of(s1, s2) {
                return true;
            }
            return false;
        }
        true
    }

    /// `ConversionCost(Dest, Source)`.
    pub fn conversion_cost(&self, dest: &PropType, src: &PropType) -> i32 {
        let r = self.res();
        let conversion = self.get_conversion(dest, src);
        if self.matches_type(dest, src, true, false) {
            return 0;
        } else if dest.flags & cpf::OUT_PARM != 0 {
            return MAXINT;
        } else if self.matches_type(dest, src, false, false) {
            let mut result = 1;
            if src.is_object() && src.inner.is_some() {
                let dpc = dest.inner.expect("dest class");
                let spc = src.inner.unwrap();
                let mut tmp = 0;
                let mut test = Some(spc);
                while let Some(t) = test {
                    if t == dpc {
                        break;
                    }
                    tmp += 1;
                    test = r.super_of(t);
                }
                if test.is_none() && dest.is_object() {
                    tmp = 0;
                    test = Some(dpc);
                    while let Some(t) = test {
                        if t == spc {
                            break;
                        }
                        tmp += 1;
                        test = r.super_of(t);
                    }
                }
                result += tmp;
                if test.is_none() && dest.cpt == Cpt::Interface {
                    result = 1;
                    let mut check = Some(spc);
                    while let Some(c) = check {
                        if !r.implements_interface(c, dpc) {
                            break;
                        }
                        test = Some(c);
                        result += 1;
                        check = r.super_of(c);
                    }
                    result = result.max(1);
                }
                if test.is_none() {
                    return MAXINT;
                }
            }
            return result;
        } else if dest.array_dim != 1 || src.array_dim != 1 {
            return MAXINT;
        } else if dest.cpt == Cpt::Byte && dest.inner.is_some() {
            return MAXINT;
        } else if dest.is_object() && dest.inner.is_some() {
            return MAXINT;
        } else if if dest.flags & cpf::COERCE_PARM != 0 { conversion == CST_MAX } else { conversion & AUTOCONVERT == 0 } {
            return MAXINT;
        } else if conversion & TRUNCATE != 0 {
            return CONVERSION_TRUNCATION;
        } else if (src.cpt == Cpt::Int || src.cpt == Cpt::Byte) && dest.cpt == Cpt::Float {
            return CONVERSION_INT_TO_FLOAT;
        }
        CONVERSION_EXPANSION
    }

    /// `FToken::AttemptToConvertConstant`.
    pub fn attempt_to_convert_constant(&self, tok: &mut Token, new_type: &PropType) {
        if tok.kind != TokKind::Const {
            return;
        }
        match new_type.cpt {
            Cpt::Int => {
                if let Some(v) = tok.get_const_int() {
                    tok.set_const_int(v);
                }
            }
            Cpt::Bool => {
                if let Some(v) = tok.get_const_bool() {
                    tok.set_const_bool(v);
                }
            }
            Cpt::Float => {
                if let Some(v) = tok.get_const_float() {
                    tok.set_const_float(v);
                }
            }
            Cpt::Name => {
                if let Some(v) = tok.get_const_name() {
                    tok.set_const_name(&v);
                }
            }
            Cpt::Struct => {}
            Cpt::String => {}
            Cpt::Byte => {
                if new_type.inner.is_none() {
                    if let Some(v) = tok.get_const_byte() {
                        tok.set_const_byte(None, v);
                    }
                }
            }
            Cpt::Object => {
                if let ConstVal::Object(o) = tok.value.clone() {
                    let ok = match (new_type.inner, tok.ty.inner) {
                        (None, _) => true,
                        (Some(dc), Some(tc)) => self.res().is_child_of(tc, dc),
                        (Some(_), None) => o.is_none(),
                    };
                    if ok {
                        self.set_const_object(tok, o);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn set_const_object(&self, tok: &mut Token, obj: Option<FieldRef>) {
        let mut t = PropType::new(Cpt::Object);
        let class_class = self.class_class();
        match obj {
            Some(o) => {
                let oc = if self.res().is_class(o) { class_class } else { self.object_class_of(o) };
                t.inner = oc;
                if oc.is_some() && oc == class_class {
                    t.meta_class = Some(o);
                }
            }
            None => {
                t.inner = None;
            }
        }
        tok.set_const(t, ConstVal::Object(obj));
    }

    /// Class of an arbitrary export (for object literals).
    pub fn object_class_of(&self, o: FieldRef) -> Option<FieldRef> {
        match o {
            FieldRef::External(r) => self.ctx.class_of(r).map(FieldRef::External),
            FieldRef::Local(n) => self.model.node(n).object_class,
        }
    }

    // ----- GetToken ----------------------------------------------------------

    /// `GetToken(Token, Hint, NoConsts)`.
    pub fn get_token_hint(&mut self, hint: Option<&PropType>, no_consts: bool) -> CResult<Option<Token>> {
        let Some(mut tok) = self.get_raw(no_consts)? else { return Ok(None) };
        if tok.kind != TokKind::Identifier {
            if tok.kind == TokKind::Const {
                // numeric constants under a byte hint become bytes
                if let Some(h) = hint {
                    if h.cpt == Cpt::Byte {
                        if let ConstVal::Int(v) = tok.value {
                            tok.set_const_byte(h.inner, (v & 0xFF) as u8);
                        }
                    }
                }
            }
            return Ok(Some(tok));
        }
        if no_consts {
            return Ok(Some(tok));
        }
        let ident = tok.ident.clone();
        let lower = ident.to_ascii_lowercase();
        match lower.as_str() {
            "vect" if self.match_symbol("(")? => {
                let x = self.get_const_float()?.ok_or_else(|| self.err("Missing X component of vector"))?;
                self.require_symbol(",", "vector")?;
                let y = self.get_const_float()?.ok_or_else(|| self.err("Missing Y component of vector"))?;
                self.require_symbol(",", "vector")?;
                let z = self.get_const_float()?.ok_or_else(|| self.err("Missing Z component of vector"))?;
                self.require_symbol(")", "vector")?;
                let vs = self.vector_struct.ok_or_else(|| self.err("Vector struct not found"))?;
                tok.set_const_vector((x, y, z), vs);
                return Ok(Some(tok));
            }
            "rot" if self.match_symbol("(")? => {
                let p = self.get_const_int(None)?.ok_or_else(|| self.err("Missing Pitch component of rotation"))?;
                self.require_symbol(",", "rotation")?;
                let y = self.get_const_int(None)?.ok_or_else(|| self.err("Missing Yaw component of rotation"))?;
                self.require_symbol(",", "rotation")?;
                let r = self.get_const_int(None)?.ok_or_else(|| self.err("Missing Roll component of rotation"))?;
                self.require_symbol(")", "rotation")?;
                let rs = self.rotator_struct.ok_or_else(|| self.err("Rotator struct not found"))?;
                tok.set_const_rotator((p, y, r), rs);
                return Ok(Some(tok));
            }
            "true" => {
                tok.set_const_bool(true);
                return Ok(Some(tok));
            }
            "false" => {
                tok.set_const_bool(false);
                return Ok(Some(tok));
            }
            "arraycount" => {
                let tt = self.require_sizeof_parm("'ArrayCount'")?;
                if tt.ty.array_dim <= 1 {
                    return self.error("ArrayCount argument is not an array");
                }
                tok.set_const_int(tt.ty.array_dim);
                return Ok(Some(tok));
            }
            "nameof" => {
                let tt = self.require_sizeof_parm("'NameOf'")?;
                if tt.kind != TokKind::Identifier && (tt.kind != TokKind::Const || (tt.prop.is_none() && tt.func.is_none())) {
                    return self.error("NameOf argument is not an identifier");
                }
                tok.set_const_name(&tt.ident);
                return Ok(Some(tok));
            }
            "none" => {
                if hint.map(|h| h.cpt == Cpt::Delegate).unwrap_or(false) {
                    tok.set_const_delegate();
                } else {
                    self.set_const_object(&mut tok, None);
                }
                return Ok(Some(tok));
            }
            _ => {}
        }
        if let Some(h) = hint {
            if h.cpt == Cpt::Byte {
                if let Some(e) = h.inner {
                    if let Some(i) = self.res().find_enum_index(e, &ident) {
                        tok.set_const_byte(Some(e), i as u8);
                        return Ok(Some(tok));
                    }
                }
            }
            if h.cpt == Cpt::Int || h.cpt == Cpt::Byte {
                if let Some(i) = self.res().global_enum_tag(&ident) {
                    tok.set_const_int(i);
                    return Ok(Some(tok));
                }
            }
        }
        if self.peek_symbol("'")? {
            if let Some(class) = self.res().find_class(&ident) {
                let is_actor = self.actor_class().map(|a| self.res().is_child_of(class, a)).unwrap_or(false);
                if !is_actor && self.match_symbol("'")? {
                    let start_line = self.lexer_line;
                    let mut s = String::new();
                    loop {
                        let c = self.input.get(self.lexer_pos).copied().unwrap_or(0);
                        if c == b'\'' {
                            self.lexer_pos += 1;
                            break;
                        }
                        if c == 0 || c == b'\n' || c == b'\r' || self.lexer_line != start_line {
                            return self.error(format!("Unterminated constant {} name", self.res().name_of(class)));
                        }
                        s.push(c as char);
                        self.lexer_pos += 1;
                    }
                    if s.is_empty() {
                        return self.error(format!("Missing {} name", self.res().name_of(class)));
                    }
                    let obj = self.find_object_literal(class, &s);
                    if obj.is_none() {
                        let cn = self.res().name_of(class);
                        self.warn(format!("Unresolved reference to {cn} '{s}'"));
                    }
                    self.set_const_object(&mut tok, obj);
                    tok.ty.inner = Some(class);
                    if Some(class) == self.class_class() {
                        tok.ty.meta_class = obj;
                    }
                    return Ok(Some(tok));
                }
            }
        }
        Ok(Some(tok))
    }

    /// `StaticFindObject(Type, ANY_PACKAGE, Str)` over the model and loaded packages.
    pub fn find_object_literal(&mut self, class: FieldRef, path: &str) -> Option<FieldRef> {
        let class_class = self.class_class();
        if Some(class) == class_class {
            let parts: Vec<&str> = path.split('.').collect();
            if parts.len() == 2 {
                // Qualified `Package.Class`: look in that package only.
                if parts[0].eq_ignore_ascii_case(&self.model.package_name) {
                    return self.model.find_class(parts[1]).map(FieldRef::Local);
                }
                let _ = self.ctx.package_by_name(parts[0]);
                if let Some(slot) = self.ctx.slot_of(parts[0]) {
                    if let Some(r) = self.ctx.find_export(slot, parts[1]) {
                        if self.ctx.class_name(r) == "Class" {
                            return Some(FieldRef::External(r));
                        }
                    }
                }
                {
                    // Intrinsic / phantom classes: match on their import package.
                    let hit = self.model.intrinsics.iter().copied().find(|&n| {
                        let node = self.model.node(n);
                        node.name.eq_ignore_ascii_case(parts[1]) && node.intrinsic_package.as_deref().unwrap_or("Core").eq_ignore_ascii_case(parts[0])
                    });
                    if hit.is_some() {
                        return hit.map(FieldRef::Local);
                    }
                }
                return None;
            }
            let name = path.rsplit('.').next().unwrap_or(path);
            return self.res().find_class(name);
        }
        // Local defaults objects (sub-objects): by full path, or by name alone.
        if path.contains('.') {
            let want = path.strip_prefix(&format!("{}.", self.model.package_name)).unwrap_or(path).to_ascii_lowercase();
            for i in 0..self.model.nodes.len() {
                if self.model.node(i).kind == super::model::NodeKind::Object && self.node_path(i).to_ascii_lowercase() == want {
                    return Some(FieldRef::Local(i));
                }
            }
        } else {
            for (i, n) in self.model.nodes.iter().enumerate() {
                if n.kind == super::model::NodeKind::Object && n.name.eq_ignore_ascii_case(path) {
                    return Some(FieldRef::Local(i));
                }
            }
        }
        let class_name = self.res().name_of(class);
        let parts: Vec<&str> = path.split('.').collect();
        // Fields of the classes being compiled (`StaticFindObject` sees them before any
        // package on disk, including a same-named reference package).
        if parts.len() >= 2 {
            let local_parts: &[&str] = if parts.len() >= 3 && parts[0].eq_ignore_ascii_case(&self.model.package_name) { &parts[1..] } else { &parts[..] };
            if let Some(r) = self.find_local_field_path(local_parts) {
                if self.local_field_class(r).map(|c| self.res().is_child_of(c, class)).unwrap_or(false) {
                    return Some(r);
                }
            }
        }
        // Try loading the package named by the path's first segment.
        if parts.len() >= 2 {
            let _ = self.ctx.package_by_name(parts[0]);
            if let Some(slot) = self.ctx.slot_of(parts[0]) {
                let inner = parts[1..].join(".");
                if let Some(r) = self.ctx.find_export(slot, &inner) {
                    let cn = self.ctx.class_name(r);
                    if cn.eq_ignore_ascii_case(&class_name) || self.ctx.class_of(r).map(|c| self.ctx.is_a(c, &class_name)).unwrap_or(false) {
                        return Some(FieldRef::External(r));
                    }
                }
            }
        }
        // Search every loaded package for an export with that path.
        for (p, lp) in self.ctx.packages.iter().enumerate() {
            let full = path.to_ascii_lowercase();
            for i in 0..lp.pkg.exports.len() {
                let r = crate::context::ObjRef { pkg: p, export: i };
                let cn = self.ctx.class_name(r);
                if !cn.eq_ignore_ascii_case(&class_name) {
                    continue;
                }
                let op = self.ctx.path(r).to_ascii_lowercase();
                if op == full || self.ctx.full_path(r).to_ascii_lowercase() == full || op.rsplit('.').next() == Some(full.as_str()) {
                    return Some(FieldRef::External(r));
                }
            }
        }
        None
    }

    /// `Class.Member[.Member...]` among the local classes.
    fn find_local_field_path(&self, parts: &[&str]) -> Option<FieldRef> {
        let mut cur = FieldRef::Local(self.model.find_class(parts[0])?);
        for p in &parts[1..] {
            cur = self.res().children(cur).into_iter().find(|&c| self.res().name_of(c).eq_ignore_ascii_case(p))?;
        }
        Some(cur)
    }

    /// The (intrinsic) class of a local field node.
    fn local_field_class(&self, r: FieldRef) -> Option<FieldRef> {
        let FieldRef::Local(n) = r else { return None };
        let name = match &self.model.node(n).kind {
            super::model::NodeKind::Class => "Class",
            super::model::NodeKind::State => "State",
            super::model::NodeKind::Function => "Function",
            super::model::NodeKind::ScriptStruct => "ScriptStruct",
            super::model::NodeKind::Enum => "Enum",
            super::model::NodeKind::Const => "Const",
            super::model::NodeKind::Property(k) => k.class_name(),
            super::model::NodeKind::Object => return self.model.node(n).object_class,
        };
        self.res().find_class(name)
    }

    /// `RequireSizeOfParm`: compile an expression to learn its type, then discard the code.
    pub fn require_sizeof_parm(&mut self, tag: &str) -> CResult<Token> {
        let retry = self.location();
        self.require_symbol("(", tag)?;
        let mut tt = Token::empty();
        if self.compile_expr(PropType::with_ref(Cpt::None, RefType::SimpleReference), Some(tag), Some(&mut tt), MAXINT, None, false, None, false, None)? == 0 {
            return self.error(format!("Bad or missing expression in {tag}"));
        }
        self.require_symbol(")", tag)?;
        self.return_to(retry, true, false);
        Ok(tt)
    }

    // ----- emit helpers ------------------------------------------------------

    pub fn emit_constant(&mut self, tok: &Token) -> CResult<()> {
        match &tok.value {
            ConstVal::Int(v) => {
                let v = *v;
                if v == 0 {
                    self.emit_op(op::INT_ZERO);
                } else if v == 1 {
                    self.emit_op(op::INT_ONE);
                } else if (0..=255).contains(&v) {
                    self.emit_op(op::INT_CONST_BYTE);
                    self.emit_u8(v as u8);
                } else {
                    self.emit_op(op::INT_CONST);
                    self.emit_i32(v);
                }
            }
            ConstVal::Byte(b) => {
                self.emit_op(op::BYTE_CONST);
                self.emit_u8(*b);
            }
            ConstVal::Bool(b) => self.emit_op(if *b { op::TRUE } else { op::FALSE }),
            ConstVal::Float(f) => {
                self.emit_op(op::FLOAT_CONST);
                self.emit_f32(*f);
            }
            ConstVal::Str(s) => {
                if s.chars().all(|c| (c as u32) < 0x80) {
                    self.emit_op(op::STRING_CONST);
                    for b in s.bytes() {
                        self.emit_u8(b);
                    }
                    self.emit_u8(0);
                } else {
                    self.emit_op(op::UNICODE_STRING_CONST);
                    for u in super::engine::cp1252_string(s).encode_utf16() {
                        self.emit_u16(u);
                    }
                    self.emit_u16(0);
                }
            }
            ConstVal::Object(o) => match o {
                None => self.emit_op(op::NO_OBJECT),
                Some(o) => {
                    if let (Some(pc), Some(actor)) = (tok.ty.inner, self.actor_class()) {
                        if self.res().is_child_of(pc, actor) {
                            return self.error("Illegal actor constant");
                        }
                    }
                    self.emit_op(op::OBJECT_CONST);
                    self.emit_ptr(Some(*o));
                }
            },
            ConstVal::Delegate => self.emit_op(op::EMPTY_DELEGATE),
            ConstVal::Name(n) => {
                self.emit_op(op::NAME_CONST);
                self.emit_name_text(n);
            }
            ConstVal::Vector(x, y, z) => {
                self.emit_op(op::VECTOR_CONST);
                self.emit_f32(*x);
                self.emit_f32(*y);
                self.emit_f32(*z);
            }
            ConstVal::Rotator(p, y, r) => {
                self.emit_op(op::ROTATION_CONST);
                self.emit_i32(*p);
                self.emit_i32(*y);
                self.emit_i32(*r);
            }
            ConstVal::None => return self.error("Internal EmitConstant token type error"),
        }
        Ok(())
    }

    /// `EmitStackNodeLinkFunction`.
    pub fn emit_stack_node_link_function(&mut self, node: FieldRef, force_final: bool, global: bool, delegate_prop: Option<FieldRef>) -> CResult<()> {
        let flags = self.res().func_flags(node);
        let is_final = flags & func::FINAL != 0 || force_final;
        *self.func_emit_count.entry(node).or_insert(0) += 1;
        let inative = self.res().func_native_index(node);
        if flags & func::DELEGATE != 0 {
            let dp = match delegate_prop {
                Some(d) => d,
                None => {
                    let outer = self.res().outer_of(node);
                    let mangled = format!("__{}__Delegate", self.res().name_of(node));
                    self.res().find_field(outer, &mangled, true, None).ok_or_else(|| self.err("Missing delegate property"))?
                }
            };
            self.emit_op(op::DELEGATE_FUNCTION);
            let local = self.res().outer_of(dp).map(|o| self.res().kind_of(o) == FieldKind::Function).unwrap_or(false);
            self.emit_u8(local as u8);
            self.emit_ptr(Some(dp));
            let n = self.res().name_of(node);
            self.emit_name_text(&n);
        } else if is_final && inative != 0 && inative < 256 {
            self.emit_u8(inative as u8);
        } else if is_final && inative != 0 {
            self.emit_u8(op::EXTENDED_NATIVE + (inative / 256) as u8);
            self.emit_u8((inative % 256) as u8);
        } else if is_final {
            self.emit_op(op::FINAL_FUNCTION);
            self.emit_ptr(Some(node));
        } else if global {
            self.emit_op(op::GLOBAL_FUNCTION);
            let n = self.res().name_of(node);
            self.emit_name_text(&n);
        } else {
            self.emit_op(op::VIRTUAL_FUNCTION);
            let n = self.res().name_of(node);
            self.emit_name_text(&n);
        }
        Ok(())
    }

    pub fn uobject_outer_property(&self) -> Option<FieldRef> {
        let obj = self.object_class()?;
        self.res().children(obj).into_iter().find(|c| self.res().name_of(*c) == "Outer")
    }

    /// `EmitOuterContext`.
    pub fn emit_outer_context(&mut self, start: Location, has_existing_context: bool) -> u32 {
        let outer_prop = self.uobject_outer_property();
        let current = self.location();
        self.emit_op(op::CONTEXT);
        self.move_compiled_code(start, current);
        if !has_existing_context {
            self.emit_op(op::INSTANCE_VARIABLE);
            self.emit_ptr(outer_prop);
        }
        let fixup = self.code_len();
        self.emit_u16(DEFAULT_OUTERCONTEXT_SIZE);
        self.emit_ptr(outer_prop);
        self.emit_u8(0);
        if has_existing_context {
            self.emit_op(op::INSTANCE_VARIABLE);
            self.emit_ptr(outer_prop);
        }
        fixup
    }

    /// `EmitFieldPlaceholderFixup`: write the field pointer after the skip size at `fixup`.
    pub fn emit_field_placeholder_fixup(&mut self, fixup: u32, field: Option<FieldRef>) {
        self.write_ptr_at(fixup + 2, field);
    }

    /// `EmitProperty`.
    pub fn emit_property(&mut self, tok: &Token) {
        let mut null_type = Cpt::None;
        let skipover = if let Some(f) = tok.func {
            self.res().func_return(f)
        } else {
            if tok.prop.is_none() && tok.ty.cpt != Cpt::None && tok.kind == TokKind::None {
                null_type = tok.ty.cpt;
            }
            tok.prop
        };
        self.emit_ptr(skipover);
        self.emit_u8(null_type as u8);
    }

    /// `EmitLet`.
    pub fn emit_let(&mut self, ty: &PropType) -> CResult<()> {
        if ty.flags & cpf::CONST != 0 {
            return self.error("Can't assign Const variables");
        }
        if ty.array_dim > 1 {
            return self.error("Can only assign individual elements, not arrays");
        }
        self.emit_debug_info(0);
        if ty.cpt == Cpt::Bool && ty.array_dim != 0 {
            self.emit_op(op::LET_BOOL);
        } else if ty.cpt == Cpt::Delegate && ty.array_dim != 0 {
            self.emit_op(op::LET_DELEGATE);
        } else {
            self.emit_op(op::LET);
        }
        Ok(())
    }

    /// `EmitConversion`.
    pub fn emit_conversion(&mut self, conversion: u32, insertion: Location, dest: &PropType) {
        let current = self.location();
        if conversion & CONVERT_MASK == cast::OBJECT_TO_INTERFACE as u32 {
            self.emit_op(op::INTERFACE_CAST);
            self.emit_ptr(dest.inner);
        } else {
            self.emit_op(op::PRIMITIVE_CAST);
            self.emit_u8((conversion & CONVERT_MASK) as u8);
        }
        self.move_compiled_code(insertion, current);
    }

    /// `GetSize()` of a property type in bytes (64-bit layout).
    pub fn type_size(&mut self, t: &PropType) -> i32 {
        let elem = match t.cpt {
            Cpt::None => 0,
            Cpt::Byte => 1,
            Cpt::Int | Cpt::Bool | Cpt::Float => 4,
            Cpt::Object => self.pointer_size as i32,
            Cpt::Name => 8,
            Cpt::Delegate => self.pointer_size as i32 + 8,
            Cpt::Interface => self.pointer_size as i32 * 2,
            Cpt::Struct => match t.inner {
                Some(s) => {
                    if self.struct_name_is(t, "Pointer") {
                        self.pointer_size as i32
                    } else {
                        self.struct_size(s)
                    }
                }
                None => 0,
            },
            Cpt::Map => 80,
            _ => 0,
        };
        elem * t.array_dim.max(0)
    }

    // ----- CompileDynamicCast --------------------------------------------------

    pub fn compile_dynamic_cast(&mut self, token: &Token, result: &mut Token) -> CResult<bool> {
        let low = self.location();
        if !self.match_symbol("(")? && !self.peek_symbol("<")? {
            return Ok(false);
        }
        let Some(dest_class) = self.res().find_class(&token.ident) else {
            let scope = FieldRef::Local(self.class);
            if let Some(dest_enum) = self.res().find_enum_scoped(scope, &token.ident) {
                let mut temp = Token::empty();
                let required = PropType::with_ref(Cpt::Byte, RefType::SimpleReference);
                if self.compile_expr(required, None, Some(&mut temp), MAXINT, None, false, None, false, None)? != 1 || !self.match_symbol(")")? {
                    self.return_to(low, true, true);
                    return Ok(false);
                }
                *result = Token::from_type(PropType::enum_byte(Some(dest_enum)));
                return Ok(true);
            }
            self.return_to(low, true, true);
            return Ok(false);
        };
        let class_class = self.class_class();
        let mut meta_class = self.object_class();
        if Some(dest_class) == class_class && self.match_symbol("<")? {
            let Some(mt) = self.get_identifier(false)? else { return Ok(false) };
            meta_class = self.res().find_class(&mt.ident);
            if meta_class.is_none() || !self.match_symbol(">")? || !self.match_symbol("(")? {
                self.return_to(low, true, true);
                return Ok(false);
            }
        }
        let mut temp = Token::empty();
        let mut required = PropType::object(self.object_class().expect("Object class"), false);
        required.ref_type = RefType::AssignmentReference;
        if self.compile_expr(required, None, Some(&mut temp), MAXINT, None, false, None, false, None)? != 1 || !self.match_symbol(")")? {
            self.return_to(low, true, true);
            return Ok(false);
        }
        let r = self.res();
        let temp_pc = temp.ty.inner;
        // Phantom classes (unknown hierarchy): always emit the dynamic cast.
        let phantom = |c: Option<FieldRef>| match c {
            Some(FieldRef::Local(n)) => r.model.node(n).intrinsic && r.model.node(n).intrinsic_package.is_some(),
            _ => false,
        };
        let any_phantom = phantom(temp_pc) || phantom(Some(dest_class));
        let unnecessary = !any_phantom
            && !temp.ty.interface
            && (temp_pc.is_none() || r.is_child_of(temp_pc.unwrap(), dest_class))
            && (temp_pc != class_class
                || Some(dest_class) != class_class
                || match (temp.ty.meta_class, meta_class) {
                    (Some(tm), Some(mc)) => r.is_child_of(tm, mc),
                    _ => false,
                });
        if unnecessary {
            // UE3 reports a formatting error (non-fatal); keep going like the engine does.
            self.warn(format!("Cast from '{}' to '{}' is unnecessary", temp_pc.map(|c| r.name_of(c)).unwrap_or("None".into()), r.name_of(dest_class)));
        } else if any_phantom
            || temp.ty.interface
            || (temp_pc.is_some_and(|pc| r.is_child_of(dest_class, pc))
                && (Some(dest_class) != class_class
                    || temp.ty.meta_class.is_none()
                    || match (meta_class, temp.ty.meta_class) {
                        (Some(mc), Some(tm)) => r.is_child_of(mc, tm),
                        _ => true,
                    }))
            || r.class_is_interface(dest_class)
        {
            let high = self.location();
            if Some(dest_class) == class_class {
                self.emit_op(op::META_CAST);
                self.emit_ptr(meta_class);
            } else {
                self.emit_op(op::DYNAMIC_CAST);
                self.emit_ptr(Some(dest_class));
            }
            self.move_compiled_code(low, high);
        } else {
            return self.error(format!(
                "Cast from '{}' to '{}' will always fail",
                temp_pc.map(|c| self.res().name_of(c)).unwrap_or_default(),
                self.res().name_of(dest_class)
            ));
        }
        if self.top_nest_ref().nest_type == NestType::Switch
            && matches!(token.ty.cpt, Cpt::String | Cpt::Object | Cpt::Interface | Cpt::Delegate | Cpt::Struct)
        {
            return self.error(format!("Can't switch on dynamic cast expressions of this type: {}", token.ident));
        }
        let is_iface = self.res().class_is_interface(dest_class);
        *result = Token::from_type(PropType::object(dest_class, is_iface));
        if Some(dest_class) == class_class {
            result.ty.meta_class = meta_class;
        }
        Ok(true)
    }

    // ----- CompileExpr ---------------------------------------------------------

    /// `CompileExpr`. Returns 0 (no expression), 1 (ok) or -1 (type mismatch).
    #[allow(clippy::too_many_arguments)]
    pub fn compile_expr(
        &mut self,
        required: PropType,
        error_tag: Option<&str>,
        result_token: Option<&mut Token>,
        max_precedence: i32,
        hint_type: Option<&Token>,
        in_parentheses: bool,
        mut token_list: Option<&mut Vec<Token>>,
        entering_nest: bool,
        struct_mod_byte: Option<&mut Option<u32>>,
    ) -> CResult<i32> {
        let start_of_expression = self.location();
        let mut token = Token::empty();
        let mut last_token = Token::empty();
        let hint_pt = hint_type.map(|h| h.ty.clone()).unwrap_or_else(|| required.clone());
        let got = self.get_token_hint(Some(&hint_pt), false)?;
        match got {
            None => {
                token.ty = PropType::none();
            }
            Some(t) => {
                token = t;
                last_token = token.clone();
                let after_last = self.location();
                if token.ident.eq_ignore_ascii_case("None") && required.cpt == Cpt::Delegate && token.kind == TokKind::Const {
                    self.emit_op(op::DELEGATE_PROPERTY);
                    self.emit_name_text("None");
                    self.emit_ptr(None);
                    token.ty.cpt = Cpt::Delegate;
                } else if token.kind == TokKind::Const {
                    self.attempt_to_convert_constant(&mut token, &required);
                    token.ty.flags &= !cpf::OUT_PARM;
                    self.emit_constant(&token)?;
                } else if token.is_symbol("(") {
                    let mut inner = Token::empty();
                    if self.compile_expr(required.clone(), None, Some(&mut inner), MAXINT, None, true, None, false, None)? == 0 {
                        return self.error(format!(
                            "Bad or missing expression in parenthesis{}",
                            error_tag.map(|t| format!(" for {t}")).unwrap_or_default()
                        ));
                    }
                    token = inner;
                    self.require_symbol(")", "expression")?;
                    if token.ty.cpt == Cpt::None {
                        return self.error(format!(
                            "Bad or missing expression in parenthesis{}",
                            error_tag.map(|t| format!(" for {t}")).unwrap_or_default()
                        ));
                    }
                } else if self.is_primitive_cast_keyword(&token) && self.match_symbol("(")? {
                    let kw = token.ident.to_ascii_lowercase();
                    let to_type = match kw.as_str() {
                        "vector" => PropType::strct(self.vector_struct.expect("Vector")),
                        "rotator" => PropType::strct(self.rotator_struct.expect("Rotator")),
                        "byte" => PropType::new(Cpt::Byte),
                        "int" => PropType::new(Cpt::Int),
                        "bool" => PropType::new(Cpt::Bool),
                        "float" => PropType::new(Cpt::Float),
                        "name" => PropType::new(Cpt::Name),
                        "string" => PropType::new(Cpt::String),
                        _ => PropType::new(Cpt::Struct),
                    };
                    let mut from = Token::empty();
                    let tag = token.ident.clone();
                    self.compile_expr(PropType::with_ref(Cpt::None, RefType::AssignmentReference), Some(&tag), Some(&mut from), MAXINT, None, false, None, false, None)?;
                    if from.ty.cpt == Cpt::None {
                        return self.error(format!("'{tag}' conversion: Bad or missing expression"));
                    } else if from.ty.array_dim != 1 {
                        return self.error(format!("'{tag}' conversion: Expression is an array"));
                    }
                    let conversion = self.get_conversion(&to_type, &from.ty);
                    if conversion != CST_MAX {
                        self.emit_conversion(conversion, start_of_expression, &to_type);
                        token = Token::from_type(to_type);
                    } else if to_type.cpt == from.ty.cpt && (to_type.cpt != Cpt::Struct || to_type.inner == from.ty.inner) {
                        return self.error(format!("No need to cast '{tag}' to itself"));
                    } else {
                        return self.error(format!("Can't convert to '{tag}'"));
                    }
                    token.ty.flags &= !cpf::OUT_PARM;
                    if !self.match_symbol(")")? {
                        return self.error("Missing ')' in type conversion");
                    }
                } else if token.matches_ident("self") {
                    if required.flags & cpf::OUT_PARM != 0 {
                        return self.error("Not allowed to use 'Self' as the value for an out parameter");
                    }
                    self.check_allow("'self'", allow::INSTANCE)?;
                    if required.safe && required.is_object() {
                        self.emit_op(op::SELF_SAFE_OBJECT);
                    } else {
                        self.emit_op(op::SELF);
                    }
                    let class = FieldRef::Local(self.class);
                    token = Token::from_type(PropType::object(class, false));
                    token.ident = "self".into();
                    token.kind = TokKind::Identifier;
                } else if token.matches_ident("new") {
                    token = self.compile_new()?;
                } else {
                    let mut dc = Token::empty();
                    let snapshot = token.clone();
                    if self.compile_dynamic_cast(&snapshot, &mut dc)? {
                        token = dc;
                    } else {
                        let top = self.top_node.expect("top node");
                        let mut req = required.clone();
                        let is_concrete = self.top_nest_ref().allow & allow::INSTANCE != 0;
                        let mut res = Token::empty();
                        let ok = self.compile_field_expr(FieldRef::Local(top), &mut req, snapshot.clone(), &mut res, true, is_concrete, start_of_expression)?;
                        if ok {
                            token = res;
                            if token.ty.cpt == Cpt::None && required.cpt != Cpt::None {
                                if let Some(tag) = error_tag {
                                    return self.error(format!("{tag}: Expression does not return a value"));
                                }
                                return Ok(0);
                            }
                            if let Some(tl) = token_list.as_deref_mut() {
                                if last_token.matches_ident("default") || last_token.matches_ident("static") {
                                    tl.push(last_token.clone());
                                }
                                if last_token.matches_ident("super") {
                                    let current = self.location();
                                    self.return_to(after_last, false, true);
                                    if self.peek_symbol("(")? {
                                        self.require_symbol("(", "'super'")?;
                                        if let Some(st) = self.get_identifier(false)? {
                                            let top = self.top_node.expect("top");
                                            if let Some(sc) = self.get_super_scope(FieldRef::Local(top), &st.ident) {
                                                last_token.ty.meta_class = Some(sc);
                                            }
                                        }
                                    }
                                    tl.push(last_token.clone());
                                    self.return_to(current, false, true);
                                }
                            }
                        } else {
                            self.unget(&snapshot);
                            token = Token::empty();
                            token.ty = PropType::none();
                        }
                    }
                }
            }
        }

        // Postfix: member access, array ops, element indexing.
        let mut dyn_array_length_ref = false;
        let top = self.top_node.expect("top node");
        let mut scope = FieldRef::Local(top);
        let mut local_struct_mod: Option<u32> = None;
        let use_local_smb = struct_mod_byte.is_none();
        let smb: &mut Option<u32> = match struct_mod_byte {
            Some(s) => s,
            None => &mut local_struct_mod,
        };
        let mut processed_base_struct = false;
        loop {
            let token_identifier = if last_token.matches_ident("default") { token.ident.clone() } else { last_token.ident.clone() };
            let mut outer_context_count = 0;
            let mut prop = None;
            if token.kind != TokKind::Const {
                prop = self.res().find_field(Some(scope), &token_identifier, true, Some(&mut outer_context_count)).filter(|f| self.res().is_property(*f));
            }
            if prop.is_none() && scope != FieldRef::Local(top) {
                prop = self.res().find_field(Some(FieldRef::Local(top)), &token_identifier, true, Some(&mut outer_context_count)).filter(|f| self.res().is_property(*f));
            }
            if let Some(p) = prop {
                let pf = self.res().prop_flags(p);
                let owner = self.res().owner_class(p);
                let class = FieldRef::Local(self.class);
                if pf & cpf::CONST != 0
                    || (last_token.matches_ident("default") && pf & cpf::CONFIG == 0)
                    || (pf & cpf::PRIVATE_WRITE != 0 && owner != Some(class))
                    || (pf & cpf::PROTECTED_WRITE != 0 && !owner.map(|o| self.res().is_child_of(class, o)).unwrap_or(false))
                {
                    token.ty.flags |= cpf::CONST;
                }
            }
            if self.match_symbol(".")? {
                if token.ty.is_dynamic_array() {
                    if self.match_identifier("Length")? {
                        let high = self.location();
                        self.emit_op(op::DYN_ARRAY_LENGTH);
                        self.move_compiled_code(start_of_expression, high);
                        if let Some(tl) = token_list.as_deref_mut() {
                            tl.push(token.clone());
                        }
                        let const_array = token.ty.flags & cpf::CONST != 0;
                        token = Token::from_type(PropType::new(Cpt::Int));
                        token.ty.array_dim = 1;
                        if const_array {
                            token.ty.flags |= cpf::CONST;
                        } else {
                            token.ty.flags |= cpf::OUT_PARM;
                        }
                        if let Some(h) = hint_type {
                            if h.kind == TokKind::Symbol && (h.ident == "++" || h.ident == "--") {
                                return self.error(format!("Illegal to use operator {} to modify the length of a dynamic array", h.ident));
                            }
                        }
                        dyn_array_length_ref = true;
                    } else if self.match_identifier("Add")? {
                        if token.ty.flags & cpf::CONST != 0 {
                            return self.error("Attempting to call 'Add' on const dynamic array");
                        }
                        let high = self.location();
                        self.emit_op(op::DYN_ARRAY_ADD);
                        self.move_compiled_code(start_of_expression, high);
                        self.require_symbol("(", "'add(...)'")?;
                        self.compile_expr(PropType::with_ref(Cpt::Int, RefType::SimpleReference), Some("'add(...)'"), None, MAXINT, None, false, None, false, None)?;
                        self.require_symbol(")", "'add(...)'")?;
                        self.emit_op(op::END_FUNCTION_PARMS);
                        self.emit_debug_info(0);
                        if let Some(tl) = token_list.as_deref_mut() {
                            tl.push(token.clone());
                        }
                        token = Token::from_type(PropType::new(Cpt::Int));
                        token.ty.array_dim = 1;
                        self.got_affector = true;
                        break;
                    } else if self.match_identifier("AddItem")? {
                        let Some(ap) = prop.filter(|p| self.res().prop_kind(*p) == Some(PropertyKind::Array)) else {
                            return self.error("Failed to find array property!");
                        };
                        if token.ty.flags & cpf::CONST != 0 {
                            return self.error("Attempting to call 'AddItem' on const dynamic array");
                        }
                        let high = self.location();
                        self.emit_op(op::DYN_ARRAY_ADD_ITEM);
                        self.move_compiled_code(start_of_expression, high);
                        let null_ctx_begin = self.location();
                        self.require_symbol("(", "'additem(...)'")?;
                        let inner = self.res().prop_refs(ap).0.expect("array inner");
                        let req = self.res().prop_type(inner, RefType::AssignmentReference);
                        self.compile_expr(req, Some("additem(...)"), None, MAXINT, None, false, None, false, None)?;
                        self.require_symbol(")", "'additem(...)'")?;
                        self.emit_op(op::END_FUNCTION_PARMS);
                        self.emit_debug_info(0);
                        self.emit_null_context_skip(null_ctx_begin);
                        if let Some(tl) = token_list.as_deref_mut() {
                            tl.push(token.clone());
                        }
                        token = Token::from_type(PropType::new(Cpt::Int));
                        token.ty.array_dim = 1;
                        self.got_affector = true;
                        self.affector_return_property = None;
                        break;
                    } else if self.match_identifier("Insert")? {
                        if token.ty.flags & cpf::CONST != 0 {
                            return self.error("Attempting to call 'Insert' on const dynamic array");
                        }
                        let high = self.location();
                        self.emit_op(op::DYN_ARRAY_INSERT);
                        self.move_compiled_code(start_of_expression, high);
                        self.require_symbol("(", "'insert(...)'")?;
                        self.compile_expr(PropType::with_ref(Cpt::Int, RefType::SimpleReference), Some("'insert(...)'"), None, MAXINT, None, false, None, false, None)?;
                        self.require_symbol(",", "'insert(...)'")?;
                        self.compile_expr(PropType::with_ref(Cpt::Int, RefType::SimpleReference), Some("'insert(...)'"), None, MAXINT, None, false, None, false, None)?;
                        self.require_symbol(")", "'insert(...)'")?;
                        self.emit_op(op::END_FUNCTION_PARMS);
                        self.emit_debug_info(0);
                        if let Some(tl) = token_list.as_deref_mut() {
                            tl.push(token.clone());
                        }
                        token = Token::from_type(PropType::none());
                        self.got_affector = true;
                        self.affector_return_property = None;
                        break;
                    } else if self.match_identifier("InsertItem")? {
                        let Some(ap) = prop.filter(|p| self.res().prop_kind(*p) == Some(PropertyKind::Array)) else {
                            return self.error("Failed to find array property!");
                        };
                        if token.ty.flags & cpf::CONST != 0 {
                            return self.error("Attempting to call 'InsertItem' on const dynamic array");
                        }
                        let high = self.location();
                        self.emit_op(op::DYN_ARRAY_INSERT_ITEM);
                        self.move_compiled_code(start_of_expression, high);
                        let null_ctx_begin = self.location();
                        self.require_symbol("(", "'insertitem(...)'")?;
                        self.compile_expr(PropType::with_ref(Cpt::Int, RefType::SimpleReference), Some("InsertItem(InsertIndex,NewValue): InsertIndex"), None, MAXINT, None, false, None, false, None)?;
                        self.require_symbol(",", "'insertitem(...)'")?;
                        let inner = self.res().prop_refs(ap).0.expect("array inner");
                        let req = self.res().prop_type(inner, RefType::SimpleReference);
                        self.compile_expr(req, Some("InsertItem(InsertIndex,NewValue): NewValue"), None, MAXINT, None, false, None, false, None)?;
                        self.require_symbol(")", "'insertitem(...)'")?;
                        self.emit_op(op::END_FUNCTION_PARMS);
                        self.emit_debug_info(0);
                        self.emit_null_context_skip(null_ctx_begin);
                        if let Some(tl) = token_list.as_deref_mut() {
                            tl.push(token.clone());
                        }
                        token = Token::from_type(PropType::new(Cpt::Int));
                        token.ty.array_dim = 1;
                        self.got_affector = true;
                        self.affector_return_property = None;
                    } else if self.match_identifier("Remove")? {
                        if token.ty.flags & cpf::CONST != 0 {
                            return self.error("Attempting to call 'Remove' on const dynamic array");
                        }
                        let high = self.location();
                        self.emit_op(op::DYN_ARRAY_REMOVE);
                        self.move_compiled_code(start_of_expression, high);
                        self.require_symbol("(", "'remove(...)'")?;
                        self.compile_expr(PropType::with_ref(Cpt::Int, RefType::SimpleReference), Some("'remove(...)'"), None, MAXINT, None, false, None, false, None)?;
                        self.require_symbol(",", "'remove(...)'")?;
                        self.compile_expr(PropType::with_ref(Cpt::Int, RefType::SimpleReference), Some("'remove(...)'"), None, MAXINT, None, false, None, false, None)?;
                        self.require_symbol(")", "'remove(...)'")?;
                        self.emit_op(op::END_FUNCTION_PARMS);
                        self.emit_debug_info(0);
                        if let Some(tl) = token_list.as_deref_mut() {
                            tl.push(token.clone());
                        }
                        token = Token::from_type(PropType::none());
                        self.got_affector = true;
                        self.affector_return_property = None;
                        break;
                    } else if self.match_identifier("RemoveItem")? {
                        let Some(ap) = prop.filter(|p| self.res().prop_kind(*p) == Some(PropertyKind::Array)) else {
                            return self.error("Failed to find array property!");
                        };
                        if token.ty.flags & cpf::CONST != 0 {
                            return self.error("Attempting to call 'RemoveItem' on const dynamic array");
                        }
                        let high = self.location();
                        self.emit_op(op::DYN_ARRAY_REMOVE_ITEM);
                        self.move_compiled_code(start_of_expression, high);
                        let null_ctx_begin = self.location();
                        self.require_symbol("(", "'removeitem(...)'")?;
                        let inner = self.res().prop_refs(ap).0.expect("array inner");
                        let req = self.res().prop_type(inner, RefType::SimpleReference);
                        self.compile_expr(req, Some("removeitem(...)"), None, MAXINT, None, false, None, false, None)?;
                        self.require_symbol(")", "'removeitem(...)'")?;
                        self.emit_op(op::END_FUNCTION_PARMS);
                        self.emit_debug_info(0);
                        self.emit_null_context_skip(null_ctx_begin);
                        if let Some(tl) = token_list.as_deref_mut() {
                            tl.push(token.clone());
                        }
                        self.got_affector = true;
                        token = Token::from_type(PropType::new(Cpt::Int));
                        token.ty.array_dim = 1;
                        break;
                    } else if self.match_identifier("Find")? {
                        let Some(ap) = prop.filter(|p| self.res().prop_kind(*p) == Some(PropertyKind::Array)) else {
                            return self.error("Failed to find array property!");
                        };
                        let inner = self.res().prop_refs(ap).0.expect("array inner");
                        let inner_struct = if self.res().prop_kind(inner) == Some(PropertyKind::Struct) { self.res().prop_refs(inner).0 } else { None };
                        let high = self.location();
                        self.emit_op(if inner_struct.is_some() { op::DYN_ARRAY_FIND_STRUCT } else { op::DYN_ARRAY_FIND });
                        self.move_compiled_code(start_of_expression, high);
                        let null_ctx_begin = self.location();
                        self.require_symbol("(", "'find(...)'")?;
                        if let Some(st) = inner_struct {
                            let name_hint = PropType::with_ref(Cpt::Name, RefType::SimpleReference);
                            let pt = self.get_token_hint(Some(&name_hint), false)?.ok_or_else(|| self.err("Missing struct member name"))?;
                            self.unget(&pt);
                            let member_name = match &pt.value {
                                ConstVal::Name(n) => n.clone(),
                                _ => pt.ident.clone(),
                            };
                            let Some(sp) = self.res().find_field(Some(st), &member_name, true, None).filter(|f| self.res().is_property(*f)) else {
                                return self.error(format!("Unknown member '{}' in struct '{}'", member_name, self.res().name_of(st)));
                            };
                            self.compile_expr(name_hint, Some("'find('property',item)'"), None, MAXINT, None, false, None, false, None)?;
                            self.require_symbol(",", "'find('property',item)'")?;
                            let req = self.res().prop_type(sp, RefType::AssignmentReference);
                            self.compile_expr(req, Some("'find('property',item)'"), None, MAXINT, None, false, None, false, None)?;
                        } else {
                            let req = self.res().prop_type(inner, RefType::SimpleReference);
                            self.compile_expr(req, Some("'find(...)'"), None, MAXINT, None, false, None, false, None)?;
                        }
                        self.require_symbol(")", "'find(...)'")?;
                        self.emit_op(op::END_FUNCTION_PARMS);
                        self.emit_debug_info(0);
                        self.emit_null_context_skip(null_ctx_begin);
                        if let Some(tl) = token_list.as_deref_mut() {
                            tl.push(token.clone());
                        }
                        token = Token::from_type(PropType::new(Cpt::Int));
                        token.ty.array_dim = 1;
                    } else if self.match_identifier("Sort")? {
                        let Some(ap) = prop.filter(|p| self.res().prop_kind(*p) == Some(PropertyKind::Array)) else {
                            return self.error("Failed to find array property!");
                        };
                        if token.ty.flags & cpf::CONST != 0 {
                            return self.error("Attempting to call 'Sort' on const dynamic array");
                        }
                        let high = self.location();
                        self.emit_op(op::DYN_ARRAY_SORT);
                        self.move_compiled_code(start_of_expression, high);
                        let null_ctx_begin = self.location();
                        self.require_symbol("(", "'sort(...)'")?;
                        let mut dt = Token::empty();
                        self.compile_expr(PropType::with_ref(Cpt::Delegate, RefType::AssignmentReference), Some("Sort(CompareFunction)"), Some(&mut dt), MAXINT, None, false, None, false, None)?;
                        self.require_symbol(")", "'sort(...)'")?;
                        self.emit_op(op::END_FUNCTION_PARMS);
                        self.emit_debug_info(0);
                        self.emit_null_context_skip(null_ctx_begin);
                        if let Some(tl) = token_list.as_deref_mut() {
                            tl.push(token.clone());
                        }
                        let inner = self.res().prop_refs(ap).0.expect("array inner");
                        token = Token::from_type(self.res().prop_type(inner, RefType::None));
                        token.ty.array_dim = 1;
                        self.got_affector = true;
                        self.affector_return_property = Some(inner);
                    } else {
                        return self.error("Invalid property or function call on a dynamic array");
                    }
                } else if token.ty.array_dim > 1 {
                    return self.error(format!("Missing array index after expression: {}", token.ident));
                } else if token.ty.cpt == Cpt::Struct {
                    let st = token.ty.inner.expect("struct");
                    let Some(tag) = self.get_token_hint(None, false)? else { return self.error("Missing struct member") };
                    let member = self.res().find_field(Some(st), &tag.ident, true, None).filter(|f| self.res().is_property(*f));
                    let Some(member) = member else {
                        return self.error(format!("Unknown member '{}' in struct '{}'", tag.ident, self.res().name_of(st)));
                    };
                    last_token = tag.clone();
                    scope = st;
                    let requires_copy: u8 = if token.ty.flags & cpf::OUT_PARM == 0 { 1 } else { 0 };
                    let original_flags = token.ty.flags;
                    if let Some(tl) = token_list.as_deref_mut() {
                        tl.push(token.clone());
                    }
                    let mut nt = self.res().prop_type(member, RefType::None);
                    nt.flags = original_flags | (nt.flags & cpf::PROPAGATE_FROM_STRUCT);
                    token.ty = nt;
                    token.prop = Some(member);
                    token.ident = self.res().name_of(member);
                    token.kind = TokKind::Identifier;
                    let high = self.location();
                    if self.res().prop_kind(member) == Some(PropertyKind::Bool) {
                        self.emit_op(op::BOOL_VARIABLE);
                    }
                    self.emit_op(op::STRUCT_MEMBER);
                    self.emit_ptr(Some(member));
                    self.emit_ptr(Some(scope));
                    self.emit_u8(requires_copy);
                    let modified: u8 = if !processed_base_struct && required.flags & cpf::OUT_PARM != 0 { 1 } else { 0 };
                    self.emit_u8(modified);
                    if !processed_base_struct {
                        let pos = self.code_len() - 1;
                        self.top_nest().struct_mod_bytes.push(pos);
                        *smb = Some(self.top_nest_ref().struct_mod_bytes.len() as u32 - 1);
                        processed_base_struct = true;
                    }
                    self.move_compiled_code(start_of_expression, high);
                } else if token.ty.is_object() {
                    let Some(pc) = token.ty.inner else { return self.error("Unresolved class for context expression") };
                    let original = token.clone();
                    if let Some(tl) = token_list.as_deref_mut() {
                        tl.push(token.clone());
                    }
                    let class_class = self.class_class();
                    if Some(pc) == class_class {
                        let Some(meta) = token.ty.meta_class else {
                            return self.error("Unresolved class for 'class' expression");
                        };
                        if self.peek_identifier("const")? {
                            self.return_to(start_of_expression, true, false);
                            let t = self.get_token_hint(None, false)?.unwrap();
                            let mut req = required.clone();
                            let mut res = Token::empty();
                            if !self.compile_field_expr(meta, &mut req, t, &mut res, false, false, start_of_expression)? {
                                return self.error(format!("'{}': Bad const context expression", self.res().name_of(meta)));
                            }
                            token = res;
                            if let Some(tl) = token_list.as_deref_mut() {
                                tl.push(token.clone());
                            }
                            continue;
                        } else if self.peek_identifier("default")? || self.peek_identifier("static")? {
                            let high = self.location();
                            self.emit_op(op::CLASS_CONTEXT);
                            self.move_compiled_code(start_of_expression, high);
                            scope = meta;
                            let t = self.get_token_hint(None, false)?.unwrap();
                            last_token = t.clone();
                            let context_start = self.location();
                            let mut req = required.clone();
                            let mut res = Token::empty();
                            if !self.compile_field_expr(meta, &mut req, t, &mut res, false, false, start_of_expression)? {
                                return self.error(format!("'{}': Bad context expression", self.res().name_of(meta)));
                            }
                            token = res;
                            if token.ty.cpt == Cpt::None && required.cpt != Cpt::None {
                                if let Some(tag) = error_tag {
                                    self.warn(format!("{tag}: Expression does not return a value"));
                                }
                            }
                            let context_end = self.location();
                            self.emit_context_skip(entering_nest, context_start)?;
                            self.emit_property(&token);
                            self.move_compiled_code(context_start, context_end);
                            continue;
                        }
                    }
                    let Some(ctx_tok) = self.get_token_hint(None, false)? else { return self.error("Missing member after '.'") };
                    let mut occ = 0;
                    let next_field = self.res().find_field(Some(pc), &ctx_tok.ident, true, Some(&mut occ));
                    if next_field.is_some() {
                        while occ > 0 {
                            occ -= 1;
                            self.emit_outer_context(start_of_expression, true);
                        }
                    }
                    let high = self.location();
                    if token.ty.safe {
                        self.emit_op(op::SAFE_OBJECT_CONTEXT);
                    } else {
                        self.emit_op(op::CONTEXT);
                    }
                    if token.ty.cpt == Cpt::Interface {
                        self.emit_op(op::INTERFACE_CONTEXT);
                    }
                    self.move_compiled_code(start_of_expression, high);
                    let context_start = self.location();
                    let mut req = required.clone();
                    let mut res = Token::empty();
                    if !self.compile_field_expr(pc, &mut req, ctx_tok.clone(), &mut res, false, true, start_of_expression)? {
                        return self.error(format!("Unrecognized member '{}' in class '{}'", ctx_tok.ident, self.res().name_of(pc)));
                    }
                    // A constant member leaves the token's property as the context object's.
                    if res.kind == TokKind::Const {
                        res.prop = original.prop;
                    }
                    token = res;
                    last_token = ctx_tok;
                    scope = original.ty.inner.unwrap_or(scope);
                    let context_end = self.location();
                    self.emit_context_skip(entering_nest, context_start)?;
                    self.emit_property(&token);
                    self.move_compiled_code(context_start, context_end);
                } else {
                    return self.error(format!("Unexpected '.' following '{}'", token.ident));
                }
            } else if token.ty.array_dim != 1 {
                if !self.match_symbol("[")? {
                    break;
                }
                if token.ty.is_dynamic_array() && required.flags & cpf::OUT_PARM != 0 {
                    let locally_declared = token
                        .prop
                        .map(|p| self.res().outer_of(p).map(|o| self.res().kind_of(o) == FieldKind::Function).unwrap_or(false) && self.res().prop_flags(p) & cpf::OUT_PARM == 0)
                        .unwrap_or(false);
                    if token.prop.is_none() || (!locally_declared && token.prop.map(|p| self.res().prop_flags(p) & cpf::CONST == 0).unwrap_or(true)) {
                        return self.error(format!("{}: Not allowed to pass a dynamic array element as the value for an out parameter", error_tag.unwrap_or("")));
                    }
                }
                let high = self.location();
                self.emit_op(if token.ty.array_dim > 1 { op::ARRAY_ELEMENT } else { op::DYN_ARRAY_ELEMENT });
                token.ty.array_dim = 1;
                self.compile_expr(PropType::with_ref(Cpt::Int, RefType::SimpleReference), Some("array index"), None, MAXINT, None, false, None, false, None)?;
                if !self.match_symbol("]")? {
                    return self.error(format!("{} is an array; expecting ']'", token.ident));
                }
                self.move_compiled_code(start_of_expression, high);
            } else {
                break;
            }
        }

        // Binary / pre-operators.
        'test: loop {
            let is_pre_operator = token.ty.cpt == Cpt::None;
            let required_fn_flags = (if is_pre_operator { func::PRE_OPERATOR } else { 0 }) | func::OPERATOR;
            let Some(oper_token) = self.get_token_hint(None, true)? else { break 'test };
            if oper_token.kind == TokKind::None || oper_token.ident.is_empty() {
                self.unget(&oper_token);
                break 'test;
            }
            let oper_name = oper_token.ident.clone();
            let oper_string = oper_name.replace('%', "[Modulo]");
            let mut oper_links: Vec<FieldRef> = Vec::new();
            let mut precedence: i32 = 0;
            let mut num_parms: i32 = 3;
            let mut affector_operation = false;
            for i in (1..self.nests.len()).rev() {
                let Some(node) = self.nests[i].node else { continue };
                let r = self.res();
                // TFieldIterator<UFunction>(Node): includes supers.
                let mut cur = Some(FieldRef::Local(node));
                let mut guard = 0;
                while let Some(st) = cur {
                    for ch in r.children(st) {
                        if r.is_function(ch)
                            && r.func_friendly_name(ch).eq_ignore_ascii_case(&oper_name)
                            && (r.func_flags(ch) & (func::PRE_OPERATOR | func::OPERATOR)) == required_fn_flags
                        {
                            oper_links.push(ch);
                            precedence = r.func_precedence(ch) as i32;
                            num_parms = num_parms.min(r.func_num_parms(ch));
                            if !affector_operation && r.func_num_parms(ch) > 1 {
                                if let Some(p0) = r.func_parms(ch).first() {
                                    if r.prop_flags(*p0) & cpf::OUT_PARM != 0 {
                                        affector_operation = true;
                                    }
                                }
                            }
                        }
                    }
                    cur = r.inheritance_super(st);
                    guard += 1;
                    if guard > 100 {
                        break;
                    }
                }
            }
            if oper_links.is_empty() || precedence >= max_precedence {
                self.unget(&oper_token);
                break 'test;
            }
            let mid_retry = self.location();
            let ref_type = if affector_operation { RefType::DualReference } else { required.ref_type };
            let new_required = PropType::with_ref(Cpt::None, ref_type);
            let mut new_result = Token::empty();
            if num_parms == 3 || is_pre_operator {
                let tag = format!("Following '{oper_string}'");
                let hint = token.clone();
                self.compile_expr(new_required, Some(&tag), Some(&mut new_result), precedence, Some(&hint), false, None, false, None)?;
                if new_result.ty.cpt == Cpt::None {
                    return self.error(format!("Bad or missing expression after '{oper_string}': '{}'", new_result.ident));
                }
            }
            // Best operator.
            let mut best: Option<FieldRef> = None;
            let mut best_match = 0;
            let mut matches = 0;
            let (mut any_left, mut any_left_ic, mut any_right, mut any_right_ic, mut obj_cmp_base) = (false, false, false, false, false);
            for &node in &oper_links {
                let r = self.res();
                let parms = r.func_parms(node);
                let nparms = r.func_num_parms(node);
                let mut this_match = 0;
                let mut pi = 0;
                if nparms == 3 || !is_pre_operator {
                    let parm1 = parms[pi];
                    pi += 1;
                    let mut parm_info = r.prop_type(parm1, RefType::None);
                    let node_name = r.name_of(node);
                    if (node_name == "EqualEqual_ObjectObject" || node_name == "NotEqual_ObjectObject")
                        && parm_info.cpt == Cpt::Object
                        && new_result.ty.inner.is_some()
                        && new_result.ty.cpt != Cpt::Interface
                    {
                        obj_cmp_base = self.conversion_cost(&parm_info, &token.ty) != MAXINT;
                        parm_info.inner = new_result.ty.inner;
                    } else if parm_info.cpt == Cpt::Interface {
                        if token.ty.cpt == Cpt::Interface {
                            parm_info.inner = token.ty.inner;
                        } else if new_result.ty.cpt == Cpt::Interface {
                            parm_info.inner = new_result.ty.inner;
                        }
                    }
                    let cost = self.conversion_cost(&parm_info, &token.ty);
                    this_match = cost;
                    any_left = any_left || cost != MAXINT;
                    if any_left {
                        any_left_ic = true;
                    } else if !any_left_ic && token.ty.flags & cpf::CONST != 0 {
                        let mut nt = token.ty.clone();
                        nt.flags &= !cpf::CONST;
                        any_left_ic = self.conversion_cost(&parm_info, &nt) != MAXINT;
                    }
                }
                if nparms == 3 || is_pre_operator {
                    let r = self.res();
                    let parm2 = parms[pi];
                    let mut parm_info = r.prop_type(parm2, RefType::None);
                    if parm_info.cpt == Cpt::Interface {
                        if new_result.ty.cpt == Cpt::Interface {
                            parm_info.inner = new_result.ty.inner;
                        } else if token.ty.cpt == Cpt::Interface {
                            parm_info.inner = token.ty.inner;
                        }
                    }
                    let cost = self.conversion_cost(&parm_info, &new_result.ty);
                    this_match = this_match.max(cost);
                    any_right = any_right || cost != MAXINT;
                    if any_right {
                        any_right_ic = true;
                    } else if !any_right_ic && new_result.ty.flags & cpf::CONST != 0 {
                        let mut nt = new_result.ty.clone();
                        nt.flags &= !cpf::CONST;
                        any_right_ic = self.conversion_cost(&parm_info, &nt) != MAXINT;
                    }
                }
                if std::env::var("USC_DEBUG_OPS").is_ok() {
                    let pi = parms.first().map(|p| self.res().prop_type(*p, RefType::None));
                    eprintln!(
                        "  candidate {} cost {} parm1={:?}/{:?} left={:?}/{:?}",
                        self.res().name_of(node),
                        this_match,
                        pi.as_ref().map(|t| t.cpt),
                        pi.as_ref().and_then(|t| t.inner).map(|c| self.res().name_of(c)),
                        token.ty.cpt,
                        token.ty.inner.map(|c| self.res().name_of(c))
                    );
                }
                if (best.is_none() || this_match < best_match) && nparms == num_parms {
                    best = Some(node);
                    best_match = this_match;
                    matches = 1;
                } else if this_match == best_match {
                    matches += 1;
                }
            }
            if best_match == MAXINT {
                let valid_struct_cmp = token.ty.cpt == Cpt::Struct
                    && new_result.ty.cpt == Cpt::Struct
                    && token.ty.inner == new_result.ty.inner
                    && token.ty.array_dim == 1
                    && new_result.ty.array_dim == 1;
                let valid_delegate_cmp = token.ty.cpt == Cpt::Delegate && new_result.ty.cpt == Cpt::Delegate;
                let equality = oper_name == "==" || oper_name == "!=";
                let special = equality && (valid_struct_cmp || valid_delegate_cmp);
                if equality && token.kind == TokKind::Const && new_result.kind == TokKind::Const && (special || (any_left && any_right)) {
                    self.warn("Comparison between two constants will always have the same result");
                }
                if special {
                    if valid_struct_cmp {
                        let high = self.location();
                        self.emit_op(if oper_name == "==" { op::STRUCT_CMP_EQ } else { op::STRUCT_CMP_NE });
                        self.emit_ptr(token.ty.inner);
                        self.move_compiled_code(start_of_expression, high);
                        token = Token::from_type(PropType::new(Cpt::Bool));
                        continue 'test;
                    } else {
                        if let (Some(a), Some(b)) = (token.ty.inner, new_result.ty.inner) {
                            if a != b {
                                return self.error(format!("Mismatched delegates in {oper_string}"));
                            }
                        }
                        let high = self.location();
                        if new_result.func.is_some() {
                            self.emit_op(if oper_name == "==" { op::EQUAL_EQUAL_DEL_FUNC } else { op::NOT_EQUAL_DEL_FUNC });
                        } else {
                            self.emit_op(if oper_name == "==" { op::EQUAL_EQUAL_DEL_DEL } else { op::NOT_EQUAL_DEL_DEL });
                        }
                        self.move_compiled_code(start_of_expression, high);
                        self.emit_op(op::END_FUNCTION_PARMS);
                        self.emit_debug_info(0);
                        token = Token::from_type(PropType::new(Cpt::Bool));
                        continue 'test;
                    }
                } else if any_left && !any_right {
                    return self.error(format!("Right type is incompatible with '{oper_string}'"));
                } else if any_right && !any_left {
                    if obj_cmp_base {
                        if oper_name == "==" {
                            return self.error("Operator '==': Comparison will always fail");
                        } else {
                            return self.error("Operator '!=': Comparison will always succeed");
                        }
                    }
                    return self.error(format!("Left type is incompatible with '{oper_string}'"));
                } else if any_left_ic || any_right_ic {
                    return self.error(format!("Const mismatch with '{oper_string}'"));
                } else {
                    return self.error(format!("Types are incompatible with '{oper_string}'"));
                }
            } else if matches > 1 {
                let cands: Vec<String> = oper_links.iter().map(|f| format!("{}({})", self.res().name_of(*f), self.res().outer_of(*f).map(|o| self.res().name_of(o)).unwrap_or_default())).collect();
                return self.error(format!("Operator '{oper_string}': Can't resolve overload ({matches} matches of quality {best_match}): {}", cands.join(", ")));
            }
            let best = best.expect("best operator");
            let parms = self.res().func_parms(best);
            let mut it = 0;
            let oper_parm1 = parms.get(it).copied();
            if !is_pre_operator {
                it += 1;
            }
            let oper_parm2 = parms.get(it).copied();
            let oper_return = self.res().func_return(best).ok_or_else(|| self.err("Operator must have a return value"))?;
            if let (Some(_), Some(_)) = (oper_parm1, oper_parm2) {
                if token.ty.is_enum_variable() && new_result.ty.is_enum_variable() && token.ty.inner != new_result.ty.inner {
                    let p1c = self.res().prop_flags(oper_parm1.unwrap()) & cpf::COERCE_PARM != 0;
                    let p2c = self.res().prop_flags(oper_parm2.unwrap()) & cpf::COERCE_PARM != 0;
                    if !p1c || !p2c {
                        let a = self.res().enum_names(token.ty.inner.unwrap());
                        let b = self.res().enum_names(new_result.ty.inner.unwrap());
                        let mirrored = self.res().name_of(token.ty.inner.unwrap()).eq_ignore_ascii_case(&self.res().name_of(new_result.ty.inner.unwrap()))
                            && a.len() == b.len()
                            && a.iter().zip(&b).all(|(x, y)| x.eq_ignore_ascii_case(y));
                        if !mirrored {
                            return self.error(format!("Operator '{oper_string}': Attempting to perform comparison between variables of different enum types"));
                        }
                    }
                }
            }
            let best_nparms = self.res().func_num_parms(best);
            if best_nparms == 3 || is_pre_operator {
                let p2 = oper_parm2.expect("parm2");
                let p2flags = self.res().prop_flags(p2);
                if p2flags & cpf::OUT_PARM != 0 {
                    if dyn_array_length_ref {
                        return self.error(format!("Illegal to use operator {oper_string} to modify the length of a dynamic array"));
                    }
                    self.got_affector = true;
                }
                let p2type = self.res().prop_type(p2, RefType::None);
                if new_result.ty.cpt != p2type.cpt || (new_result.ty.safe != p2type.safe && p2type.is_object()) {
                    let mut dest = p2type.clone();
                    if dest.cpt == Cpt::Interface {
                        dest.inner = token.ty.inner;
                    }
                    let conv = self.get_conversion(&p2type, &new_result.ty);
                    self.emit_conversion(conv, mid_retry, &dest);
                }
                if p2flags & cpf::SKIP_PARM != 0 {
                    let high = self.location();
                    let offset = 1 + high.code_top - mid_retry.code_top;
                    self.emit_op(op::SKIP);
                    self.emit_u16(offset as u16);
                    self.move_compiled_code(mid_retry, high);
                }
            }
            if !is_pre_operator {
                let p1 = oper_parm1.expect("parm1");
                let p1flags = self.res().prop_flags(p1);
                if p1flags & cpf::OUT_PARM != 0 {
                    if dyn_array_length_ref {
                        return self.error(format!("Illegal to use operator {oper_string} to modify the length of a dynamic array"));
                    }
                    if let Some(idx) = *smb {
                        let pos = self.top_nest_ref().struct_mod_bytes[idx as usize];
                        self.write_u8_at(pos, 1);
                    }
                    self.got_affector = true;
                }
                let p1type = self.res().prop_type(p1, RefType::None);
                if token.ty.cpt != p1type.cpt || (token.ty.safe != p1type.safe && p1type.is_object()) {
                    let mut dest = p1type.clone();
                    if dest.cpt == Cpt::Interface {
                        dest.inner = new_result.ty.inner;
                    }
                    let conv = self.get_conversion(&p1type, &token.ty);
                    self.emit_conversion(conv, start_of_expression, &dest);
                }
            }
            let high = self.location();
            self.emit_stack_node_link_function(best, true, false, None)?;
            self.move_compiled_code(start_of_expression, high);
            self.emit_op(op::END_FUNCTION_PARMS);
            let skip2 = oper_parm2.map(|p| self.res().prop_flags(p) & cpf::SKIP_PARM != 0).unwrap_or(false);
            if !skip2 {
                self.emit_debug_info(0);
            }
            self.affector_return_property = Some(oper_return);
            token = Token::from_type(self.res().prop_type(oper_return, RefType::None));
            token.ty.flags &= !cpf::OUT_PARM;
        }

        if use_local_smb {
            if let Some(idx) = local_struct_mod {
                let list = &mut self.top_nest().struct_mod_bytes;
                if (idx as usize) < list.len() {
                    list.truncate(idx as usize);
                }
            }
        }

        if token.ty.cpt == Cpt::None && required.cpt != Cpt::None {
            if required.flags & cpf::OPTIONAL_PARM == 0 {
                if let Some(tag) = error_tag {
                    if token.ident.is_empty() {
                        return self.error(format!("Bad or missing expression in {tag}"));
                    }
                    return self.error(format!("Bad or missing expression for token: {}, in {tag}", token.ident));
                }
            }
            if let Some(rt) = result_token {
                *rt = token.clone();
            }
            if let Some(tl) = token_list.as_deref_mut() {
                tl.push(token);
            }
            return Ok(0);
        }

        // Conditional operator.
        if token.ty.cpt == Cpt::Bool && self.match_symbol("?")? {
            let high = self.location();
            self.emit_op(op::CONDITIONAL);
            self.move_compiled_code(start_of_expression, high);
            let mid = self.location();
            let required_hint = Token::from_type(required.clone());
            let mut res_a = Token::empty();
            self.compile_expr(PropType::with_ref(Cpt::None, RefType::SimpleReference), Some("conditional operator"), Some(&mut res_a), MAXINT, Some(&required_hint), false, None, false, None)?;
            self.require_symbol(":", "conditional operator")?;
            let high = self.location();
            let skip = high.code_top - mid.code_top;
            self.emit_u16(skip as u16);
            self.move_compiled_code(mid, high);
            let mid = self.location();
            let mut res_b = Token::empty();
            self.compile_expr(PropType::with_ref(Cpt::None, RefType::SimpleReference), Some("conditional operator"), Some(&mut res_b), MAXINT, Some(&required_hint), false, None, false, None)?;
            if res_a.ty.cpt != res_b.ty.cpt {
                return self.error("True/false result types in conditional operator must match. Apply an explicit conversion if necessary.");
            }
            if required.flags & cpf::OUT_PARM != 0 && (res_a.ty.flags & cpf::OUT_PARM) != (res_b.ty.flags & cpf::OUT_PARM) {
                return self.error("Both properties in conditional must be l-values when expected type is an out parameter.");
            }
            let high = self.location();
            let skip2 = high.code_top - mid.code_top;
            self.emit_u16(skip2 as u16);
            self.move_compiled_code(mid, high);
            token = res_a.clone();
            if res_a.ty.cpt == Cpt::Object {
                if res_a.ty.inner.is_none() {
                    token.ty.inner = res_b.ty.inner;
                } else if let (Some(a), Some(b)) = (res_a.ty.inner, res_b.ty.inner) {
                    let r = self.res();
                    let mut base_a = Some(a);
                    'outer: while let Some(ba) = base_a {
                        let mut base_b = Some(b);
                        while let Some(bb) = base_b {
                            if ba == bb {
                                token.ty.inner = Some(ba);
                                break 'outer;
                            }
                            base_b = r.super_of(bb);
                        }
                        base_a = r.super_of(ba);
                    }
                }
            }
        }

        if !in_parentheses && !self.matches_type(&required, &token.ty, false, true) {
            let conversion = self.get_conversion(&required, &token.ty);
            if required.flags & cpf::OUT_PARM != 0 {
                if let Some(tag) = error_tag {
                    if token.kind == TokKind::Const {
                        return self.error(format!("{tag}: Expecting a variable, not a constant"));
                    } else if token.ty.flags & cpf::CONST != 0 {
                        return self.error(format!("{tag}: Const mismatch in Out variable"));
                    } else {
                        return self.error(format!("{tag}: Type mismatch in Out variable"));
                    }
                }
                if let Some(rt) = result_token {
                    *rt = token;
                }
                return Ok(-1);
            } else if required.array_dim != 1 || token.ty.array_dim != 1 {
                if let Some(tag) = error_tag {
                    return self.error(format!("Array mismatch in {tag}"));
                }
                if let Some(rt) = result_token {
                    *rt = token;
                }
                return Ok(-1);
            // (The engine's `RequiredType.Enum==NULL` guard is absent in the game's compiler:
            // its packages auto-convert int locals passed to enum parameters.)
            } else if (if required.flags & cpf::COERCE_PARM != 0 { conversion != CST_MAX } else { conversion & AUTOCONVERT != 0 })
                && ((conversion & CONVERT_MASK) != cast::INTERFACE_TO_OBJECT as u32 || required.inner == self.object_class())
            {
                self.emit_conversion(conversion, start_of_expression, &required);
                token.ty.flags &= !cpf::OUT_PARM;
                let is_iface = token.prop.map(|p| matches!(self.res().prop_kind(p), Some(PropertyKind::Interface))).unwrap_or(false)
                    || token.prop.map(|p| self.res().prop_kind(p) == Some(PropertyKind::Array) && self.res().prop_refs(p).0.map(|i| self.res().prop_kind(i) == Some(PropertyKind::Interface)).unwrap_or(false)).unwrap_or(false);
                let mut nt = PropType::new(required.cpt);
                nt.safe = required.safe;
                if required.is_object() {
                    nt.inner = required.inner;
                }
                token = Token::from_type(nt);
                token.ty.interface = is_iface;
            } else {
                if let Some(tag) = error_tag {
                    return self.error(format!("Type mismatch in {tag}"));
                }
                if let Some(rt) = result_token {
                    *rt = token;
                }
                return Ok(-1);
            }
        }
        let ok = token.ty.cpt != Cpt::None;
        if let Some(rt) = result_token {
            *rt = token.clone();
        }
        if let Some(tl) = token_list.as_deref_mut() {
            tl.push(token);
        }
        Ok(ok as i32)
    }

    fn is_primitive_cast_keyword(&self, t: &Token) -> bool {
        t.kind == TokKind::Identifier
            && matches!(t.ident.to_ascii_lowercase().as_str(), "byte" | "int" | "bool" | "float" | "name" | "string" | "struct" | "vector" | "rotator")
    }

    /// Emit the null-context skip size for array operations with an item expression.
    fn emit_null_context_skip(&mut self, begin: Location) {
        let end = self.location();
        let skip = self.code_len() - begin.code_top;
        self.emit_u16(skip as u16);
        self.move_compiled_code(begin, end);
    }

    /// Context skip size, or an iterator-skip placeholder when entering a foreach nest.
    fn emit_context_skip(&mut self, entering_nest: bool, context_start: Location) -> CResult<()> {
        if !entering_nest || self.top_nest_ref().nest_type != NestType::ForEach {
            let skip = self.code_len() - context_start.code_top;
            self.emit_u16(skip as u16);
        } else {
            let ni = self.nests.len() - 1;
            self.remove_jump_placeholders(ni, Fixup::IteratorSkip);
            let skip = self.code_len() - context_start.code_top;
            self.emit_placeholder_for_jump(ni, Fixup::IteratorSkip, None, Some(context_start.code_top), skip as u16);
        }
        Ok(())
    }

    /// `new [(outer[, name[, flags]])] class [(template)]`
    fn compile_new(&mut self) -> CResult<Token> {
        self.emit_op(op::NEW);
        let paren = self.match_symbol("(")? && !self.match_symbol(")")?;
        let mut remaining = paren;
        let mut parent_class = Some(FieldRef::Local(self.class));
        if remaining {
            let mut np = Token::empty();
            let mut req = PropType::object(self.object_class().expect("Object"), false);
            req.ref_type = RefType::AssignmentReference;
            self.compile_expr(req, Some("'new' parent object"), Some(&mut np), MAXINT, None, false, None, false, None)?;
            parent_class = np.ty.inner.or_else(|| self.object_class());
            remaining = self.match_symbol(",")?;
        } else {
            self.emit_op(op::NOTHING);
        }
        if remaining {
            self.compile_expr(PropType::with_ref(Cpt::String, RefType::AssignmentReference), Some("'new' name"), None, MAXINT, None, false, None, false, None)?;
            remaining = self.match_symbol(",")?;
        } else {
            self.emit_op(op::NOTHING);
        }
        if remaining {
            self.compile_expr(PropType::with_ref(Cpt::Int, RefType::AssignmentReference), Some("'new' flags"), None, MAXINT, None, false, None, false, None)?;
        } else {
            self.emit_op(op::NOTHING);
        }
        if paren {
            self.require_symbol(")", "'new'")?;
        }
        let mut nc = Token::empty();
        let mut req = PropType::class_ref(self.object_class(), self.class_class().expect("Class"));
        req.ref_type = RefType::AssignmentReference;
        self.compile_expr(req, Some("'new'"), Some(&mut nc), MAXINT, None, false, None, false, None)?;
        let Some(meta) = nc.ty.meta_class else { return self.error("'new': Invalid class") };
        let token = Token::from_type(PropType::object(meta, false));
        if let Some(actor) = self.actor_class() {
            if self.res().is_child_of(meta, actor) {
                return self.error(format!("'new': Class '{}' is an Actor subclass, use 'Spawn' instead", self.res().name_of(meta)));
            }
        }
        let within = self.res().class_within(meta).or_else(|| self.object_class());
        if let (Some(pc), Some(w)) = (parent_class, within) {
            if !self.res().is_child_of(pc, w) {
                return self.error(format!(
                    "'new': Illegal new expression for class '{}' object.  Outer must be of class '{}', not class '{}'",
                    self.res().name_of(meta),
                    self.res().name_of(w),
                    self.res().name_of(pc)
                ));
            }
        }
        if self.match_symbol("(")? {
            if !self.peek_symbol(")")? {
                let mut req = PropType::object(self.object_class().expect("Object"), false);
                req.meta_class = self.object_class();
                req.ref_type = RefType::AssignmentReference;
                self.compile_expr(req, Some("'new'"), None, MAXINT, None, false, None, false, None)?;
            } else {
                self.emit_op(op::NOTHING);
            }
            self.require_symbol(")", "'new' constructor parameters")?;
        } else {
            self.emit_op(op::NOTHING);
        }
        Ok(token)
    }

    /// `GetSuperScope`.
    pub fn get_super_scope(&self, current: FieldRef, search: &str) -> Option<FieldRef> {
        let r = self.res();
        let mut super_scope = Some(current);
        while let Some(s) = super_scope {
            if r.inheritance_super(s).is_some() {
                break;
            }
            super_scope = r.outer_of(s).filter(|o| r.is_struct_like(*o));
        }
        let class = FieldRef::Local(self.class);
        let mut guard = 0;
        while let Some(s) = super_scope {
            let next = r.inheritance_super(s);
            super_scope = match next {
                Some(n) => Some(n),
                None => {
                    if !r.is_class(s) {
                        Some(class)
                    } else {
                        None
                    }
                }
            };
            if let Some(s2) = super_scope {
                if r.name_of(s2).eq_ignore_ascii_case(search) {
                    return Some(s2);
                }
            }
            guard += 1;
            if guard > 200 {
                break;
            }
        }
        None
    }

    /// `struct_size` for structs (layout computed on demand).
    pub fn struct_size(&mut self, s: FieldRef) -> i32 {
        self.link_struct_size(s)
    }
}

#[allow(dead_code)]
fn _flags_used() -> u32 {
    cf::ABSTRACT
}

pub const MAX_SIMPLE_RETURN_VALUE_SIZE_BYTES: i32 = MAX_SIMPLE_RETURN_VALUE_SIZE;
