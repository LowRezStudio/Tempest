//! `CompileFieldExpr`: variables, function calls, enum tags, consts and the
//! `default`/`static`/`const`/`global`/`super` specifiers.

use tempest_upk::flags::{class as cf, cpf, func, rf};

use super::engine::{allow, CResult, Compiler, Location, TokKind, Token};
use super::model::FieldRef;
use super::scope::FieldKind;
use super::types::{Cpt, PropType, RefType};
use crate::bytecode::op;
use crate::objects::PropertyKind;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldClass {
    Any,
    Property,
    Function,
    Const,
}

impl<'a> Compiler<'a> {
    pub fn object_flags_of(&self, f: FieldRef) -> u64 {
        match f {
            FieldRef::Local(n) => self.model.node(n).object_flags,
            FieldRef::External(o) => self.ctx.object_flags(o),
        }
    }

    fn field_matches_class(&self, f: FieldRef, fc: FieldClass) -> bool {
        let k = self.res().kind_of(f);
        match fc {
            FieldClass::Any => true,
            FieldClass::Property => matches!(k, FieldKind::Property(_)),
            FieldClass::Function => k == FieldKind::Function,
            FieldClass::Const => k == FieldKind::Const,
        }
    }

    /// `FindField` with a required field class: a name match of the wrong class yields `None`.
    fn find_field_typed(&self, scope: Option<FieldRef>, name: &str, fc: FieldClass, occ: Option<&mut i32>) -> Option<FieldRef> {
        let f = self.res().find_field(scope, name, true, occ)?;
        if self.field_matches_class(f, fc) {
            Some(f)
        } else {
            None
        }
    }

    /// Walk `Class` through its `ClassWithin` chain looking for a child of `owner`.
    fn within_chain_is_child_of(&self, owner: FieldRef) -> bool {
        let r = self.res();
        let object = self.object_class();
        let mut outer = Some(FieldRef::Local(self.class));
        let mut guard = 0;
        while let Some(oc) = outer {
            if Some(oc) == object {
                return false;
            }
            if r.is_child_of(oc, owner) {
                return true;
            }
            outer = r.class_within(oc);
            guard += 1;
            if guard > 50 {
                break;
            }
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    pub fn compile_field_expr(
        &mut self,
        mut scope: FieldRef,
        required: &mut PropType,
        mut token: Token,
        result: &mut Token,
        mut is_self: bool,
        is_concrete: bool,
        start_of_expression: Location,
    ) -> CResult<bool> {
        let start_of_field_expression = self.location();
        let mut force_final = false;
        let mut global = false;
        let mut is_super = false;
        let mut default = false;
        let mut is_static = false;
        let mut field_class = FieldClass::Any;
        let class = FieldRef::Local(self.class);
        let in_singular = self.res().is_function(scope) && self.res().func_flags(scope) & func::SINGULAR != 0;

        if token.matches_ident("default") {
            default = true;
            field_class = FieldClass::Property;
            self.require_symbol(".", "'default'")?;
            token = self.get_token_hint(None, false)?.unwrap_or_else(Token::empty);
        } else if token.matches_ident("static") {
            is_static = true;
            field_class = FieldClass::Function;
            self.require_symbol(".", "'static'")?;
            token = self.get_token_hint(None, false)?.unwrap_or_else(Token::empty);
        } else if token.matches_ident("const") {
            field_class = FieldClass::Const;
            self.require_symbol(".", "'const'")?;
            token = self.get_token_hint(None, false)?.unwrap_or_else(Token::empty);
        } else if token.matches_ident("global") {
            if !is_self {
                return self.error("Can only use 'global' with self");
            }
            if !is_concrete {
                return self.error("Can only use 'global' with concrete objects");
            }
            scope = self.res().owner_class(scope).ok_or_else(|| self.err("'global': no owner class"))?;
            field_class = FieldClass::Function;
            global = true;
            is_self = false;
            self.require_symbol(".", "'global'")?;
            token = self.get_token_hint(None, false)?.unwrap_or_else(Token::empty);
        } else if token.matches_ident("super") {
            if !self.peek_symbol("(")? {
                if !is_self {
                    return self.error("Can only use 'super' with self");
                }
                while self.res().is_function(scope) {
                    scope = self.res().outer_of(scope).ok_or_else(|| self.err("Can't use 'super': no superclass"))?;
                }
                scope = match self.res().super_of(scope) {
                    None => self.res().outer_of(scope).ok_or_else(|| self.err("Can't use 'super': no superclass"))?,
                    Some(s) => s,
                };
                field_class = FieldClass::Function;
                force_final = true;
                is_self = false;
                is_super = true;
                self.require_symbol(".", "'super'")?;
                token = self.get_token_hint(None, false)?.unwrap_or_else(Token::empty);
            } else {
                if !is_self {
                    return self.error("Can only use 'super(name)' with self");
                }
                self.require_symbol("(", "'super'")?;
                let Some(st) = self.get_identifier(false)? else { return self.error("Missing class or state name") };
                match self.get_super_scope(scope, &st.ident) {
                    Some(s) => scope = s,
                    None => {
                        if self.res().find_class(&st.ident).is_some() || self.res().find_struct_any(&st.ident).is_some() {
                            let on = self.res().outer_of(scope).map(|o| self.res().name_of(o)).unwrap_or_default();
                            return self.error(format!("'Super(name)': '{}' does not extend '{}'", on, st.ident));
                        }
                        return self.error(format!("Bad class or state name '{}'", st.ident));
                    }
                }
                self.require_symbol(")", "'super(name)'")?;
                self.require_symbol(".", "'super(name)'")?;
                field_class = FieldClass::Function;
                force_final = true;
                is_self = false;
                is_super = true;
                token = self.get_token_hint(None, false)?.unwrap_or_else(Token::empty);
            }
        }

        let mut outer_context_count = 0;
        let mut field = if is_self {
            self.find_field_typed(Some(scope), &token.ident, field_class, Some(&mut outer_context_count))
        } else {
            self.find_field_typed(Some(scope), &token.ident, field_class, None)
        };
        if field.is_none() && !is_self {
            let mut occ = 0;
            if self.find_field_typed(Some(scope), &token.ident, field_class, Some(&mut occ)).is_some() {
                return self.error(format!(
                    "Accessing a member of {}'s within class through a context expression requires explicit 'Outer'",
                    self.res().name_of(scope)
                ));
            }
        }
        let mut field_function = field.filter(|f| self.res().is_function(*f));

        if in_singular && (is_self || is_super) {
            if let Some(ff) = field_function {
                if self.res().func_flags(ff) & func::SINGULAR != 0 {
                    return self.error("Calling a singular function within a singular function on the same object will always fail");
                }
            }
        }

        let mut delegate_prop: Option<FieldRef> = None;
        if let Some(f) = field {
            if self.res().prop_kind(f) == Some(PropertyKind::Delegate) && self.peek_symbol("(")? {
                delegate_prop = Some(f);
                let func = self.res().prop_refs(f).0;
                field = func;
                field_function = func;
            }
        }

        if let Some(ff) = field_function {
            if self.res().func_flags(ff) & func::DELEGATE != 0
                && (required.cpt != Cpt::Delegate || required.ref_type != RefType::AssignmentReference)
                && !self.peek_symbol("(")?
            {
                let outer = self.res().outer_of(ff);
                let mangled = format!("__{}__Delegate", self.res().name_of(ff));
                field = self.res().find_field(outer, &mangled, true, None);
                field_function = None;
            }
        }

        let mut skipover_size_pos: u32 = 0;
        if is_self {
            while outer_context_count > 0 {
                skipover_size_pos = self.emit_outer_context(start_of_expression, false);
                outer_context_count -= 1;
            }
        }

        let field_kind = field.map(|f| self.res().kind_of(f));

        if let (Some(e), Some(FieldKind::Enum)) = (field, field_kind) {
            if self.match_symbol(".")? {
                let names = self.res().enum_names(e);
                let Some(tag) = self.get_token_hint(None, false)? else {
                    return self.error(format!("Missing enum tag after '{}'", self.res().name_of(e)));
                };
                let enum_index = if tag.ident.eq_ignore_ascii_case("EnumCount") {
                    names.len() as i32 - 1
                } else {
                    let idx = names.iter().position(|n| n.eq_ignore_ascii_case(&tag.ident));
                    match idx {
                        Some(i) if !tag.ident.eq_ignore_ascii_case("None") => i as i32,
                        _ => return self.error(format!("Missing enum tag after '{}'", self.res().name_of(e))),
                    }
                };
                *result = Token::empty();
                result.set_const_byte(Some(e), enum_index as u8);
                result.ty.flags &= !cpf::OUT_PARM;
                self.emit_constant(result)?;
                if skipover_size_pos > 0 {
                    self.emit_field_placeholder_fixup(skipover_size_pos, field);
                }
                return Ok(true);
            }
        }

        if let (Some(property), Some(FieldKind::Property(pk))) = (field, field_kind) {
            let r = self.res();
            let owner_class = r.owner_class(property);
            if skipover_size_pos > 0 {
                self.emit_field_placeholder_fixup(skipover_size_pos, field);
                skipover_size_pos = 0;
            }
            let r = self.res();
            let outer = r.outer_of(property);
            let outer_kind = outer.map(|o| r.kind_of(o));
            let is_member = outer_kind == Some(FieldKind::Class);
            let is_local = !is_member && outer_kind == Some(FieldKind::Function);
            let is_state = !is_member && outer_kind == Some(FieldKind::State);
            let pflags = r.prop_flags(property);
            let oflags = self.object_flags_of(property);
            let pname = r.name_of(property);
            let owner_name = owner_class.map(|c| r.name_of(c)).unwrap_or_default();

            if oflags & rf::PUBLIC == 0 && owner_class != Some(class) {
                return self.error(format!("Can't access private variable '{pname}' in '{owner_name}'"));
            }
            if let Some(oc) = owner_class {
                if self.res().class_flags(oc) & cf::NATIVE_ONLY != 0 {
                    return self.error(format!("Can't access variable '{pname}' in native-only class '{owner_name}'"));
                }
            }
            if oflags & rf::PROTECTED != 0 {
                if let Some(oc) = owner_class {
                    if !self.within_chain_is_child_of(oc) {
                        return self.error(format!("Can't access protected variable '{pname}' in '{owner_name}'"));
                    }
                }
            }
            if default && !is_member {
                return self.error("You can't access the default value of struct and local variables");
            }
            if !is_concrete && !default && !is_local {
                return self.error("You can only access default values of variables here");
            }
            if pflags & cpf::DEPRECATED != 0 {
                self.warn(format!("Reference to deprecated property '{pname}'"));
            }
            if is_member {
                if let Some(oc) = owner_class {
                    if self.res().class_is_interface(oc) {
                        return self.error("Cannot access interface variables");
                    }
                }
            }
            if pk == PropertyKind::Bool {
                self.emit_op(op::BOOL_VARIABLE);
            }
            let expr_token = if default {
                op::DEFAULT_VARIABLE
            } else if is_local {
                if pflags & cpf::OUT_PARM != 0 {
                    op::LOCAL_OUT_VARIABLE
                } else {
                    op::LOCAL_VARIABLE
                }
            } else if is_state {
                op::STATE_VARIABLE
            } else {
                op::INSTANCE_VARIABLE
            };
            self.emit_op(expr_token);
            self.emit_ptr(Some(property));

            let mut t = Token::from_type(self.res().prop_type(property, RefType::None));
            t.kind = TokKind::Identifier;
            t.ident = pname.clone();
            t.prop = Some(property);
            t.ty.flags |= cpf::OUT_PARM;
            if outer == self.object_class() {
                if pname == "Class" {
                    t.ty.meta_class = self.res().owner_class(scope);
                } else if pname == "Outer" {
                    t.ty.inner = self.res().owner_class(scope).and_then(|c| self.res().class_within(c));
                }
            }
            // Local reference tracking (only the reference-type switch affects codegen).
            if self.res().is_function(scope) && is_local {
                let out_param = required.flags & cpf::OUT_PARM != 0;
                if (required.ref_type == RefType::AssignValue || required.ref_type == RefType::DualReference || out_param)
                    && t.ty.is_object()
                    && self.peek_symbol(".")?
                    && !t.ty.is_dynamic_array()
                {
                    required.ref_type = RefType::AssignmentReference;
                }
            }
            *result = t;
            return Ok(true);
        }

        if required.cpt == Cpt::Delegate {
            if let Some(ff) = field_function {
                if let Some(rf_) = required.inner {
                    let r = self.res();
                    let rp = r.func_params(rf_);
                    let fp = r.func_params(ff);
                    let mismatch = r.func_num_parms(rf_) != r.func_num_parms(ff) || r.func_return(rf_).is_none() != r.func_return(ff).is_none();
                    if mismatch {
                        return self.error(format!("'{}' mismatches delegate '{}'", r.func_friendly_name(ff), r.func_friendly_name(rf_)));
                    }
                    for (d, s) in rp.iter().zip(fp.iter()) {
                        let dt = r.prop_type(*d, RefType::None);
                        let st = r.prop_type(*s, RefType::None);
                        if !self.matches_type(&dt, &st, true, false) {
                            return self.error(format!("'{}' mismatches delegate '{}'", r.func_friendly_name(ff), r.func_friendly_name(rf_)));
                        }
                    }
                }
                *self.func_emit_count.entry(ff).or_insert(0) += 1;
                self.emit_op(op::DELEGATE_PROPERTY);
                let fname = self.res().func_friendly_name(ff);
                self.emit_name_text(&fname);
                let mut dp: Option<FieldRef> = None;
                if self.res().func_flags(ff) & func::DELEGATE != 0 {
                    let outer = self.res().outer_of(ff);
                    let mangled = format!("__{}__Delegate", self.res().name_of(ff));
                    dp = self.res().find_field(outer, &mangled, true, None).filter(|f| self.res().prop_kind(*f) == Some(PropertyKind::Delegate));
                }
                self.emit_ptr(dp);
                if skipover_size_pos > 0 {
                    self.emit_field_placeholder_fixup(skipover_size_pos, dp);
                }
                // `ResultType = FToken(FPropertyBase(CPT_Delegate))`: the delegate function is
                // the type's `Function`, not a `TokenFunction` (EmitProperty writes CPT_Delegate).
                *result = Token::from_type(PropType::new(Cpt::Delegate));
                result.ty.inner = Some(ff);
                return Ok(true);
            } else if field.is_none() {
                if required.flags & cpf::OPTIONAL_PARM == 0 {
                    let dn = required.inner.map(|f| self.res().func_friendly_name(f)).unwrap_or_default();
                    return self.error(format!("Delegate assignment failed ({dn}): Invalid or unknown function '{}'", token.ident));
                }
                return Ok(false);
            } else {
                let dn = required.inner.map(|f| self.res().func_friendly_name(f)).unwrap_or_default();
                return self.error(format!("Delegate assignment failed ({dn}): '{}' is not a function", token.ident));
            }
        }

        if let Some(ff) = field_function {
            let fflags = self.res().func_flags(ff);
            let fname = self.res().name_of(ff);
            if is_super && fflags & func::FINAL != 0 {
                return self.error(format!("Cannot call 'Super' of final function '{}'", token.ident));
            }
            if self.match_symbol("(")? {
                self.got_affector = true;
                let owner = self.res().owner_class(ff);
                let owner_name = owner.map(|c| self.res().name_of(c)).unwrap_or_default();
                if fflags & func::PRIVATE != 0 && owner != Some(class) {
                    if matches!(owner, Some(FieldRef::Local(_))) {
                        return self.error(format!("Can't access private function '{fname}' in '{owner_name}'"));
                    }
                    // Another package's private function (the game's Core made `LogInternal`
                    // private; mods built against UDK's Core call it): the call compiles the same.
                    self.warn(format!("Accessing private function '{fname}' in '{owner_name}'"));
                }
                if fflags & func::PROTECTED != 0 {
                    let owner = owner.expect("owner class");
                    if !self.res().is_child_of(class, owner) {
                        if self.res().is_child_of(owner, class) {
                            let mut occ = 0;
                            let scoped = self.find_field_typed(Some(class), &fname, FieldClass::Function, Some(&mut occ));
                            match scoped {
                                None => return self.error(format!("Can't access protected function '{fname}' in '{owner_name}' (not in scope)")),
                                Some(sf) => {
                                    if self.res().func_flags(sf) & func::PRIVATE != 0 && self.res().owner_class(sf) != Some(class) {
                                        return self.error(format!("Can't access private function '{fname}' in '{owner_name}' (not in scope)"));
                                    }
                                }
                            }
                        } else if !self.within_chain_is_child_of(owner) {
                            return self.error(format!("Can't access protected function '{fname}' in '{owner_name}'"));
                        }
                    }
                }
                if fflags & func::LATENT != 0 {
                    self.check_allow(&fname, allow::STATE_CMD)?;
                }
                if fflags & func::STATIC == 0 && !is_concrete {
                    return self.error("Can't call instance functions from within static functions");
                }
                if is_static && fflags & func::STATIC == 0 {
                    return self.error(format!("Function '{fname}' is not static"));
                }
                if fflags & func::ITERATOR != 0 {
                    self.check_allow(&fname, allow::ITERATOR)?;
                    self.top_nest().allow &= !allow::ITERATOR;
                }
                if fflags & func::DELEGATE != 0 && owner.map(|o| self.res().class_is_interface(o)).unwrap_or(false) {
                    return self.error("Can't call delegates through interface references");
                }

                self.emit_stack_node_link_function(ff, force_final, global, delegate_prop)?;

                let parms = self.res().func_params(ff);
                let mut is_iterator_cast = false;
                if fflags & func::ITERATOR != 0 && self.res().func_num_parms(ff) >= 2 && parms.len() >= 2 {
                    let r = self.res();
                    let a = parms[0];
                    let b = parms[1];
                    let a_obj = matches!(r.prop_kind(a), Some(PropertyKind::Object | PropertyKind::Class | PropertyKind::Component | PropertyKind::SafeObject));
                    let b_obj = matches!(r.prop_kind(b), Some(PropertyKind::Object | PropertyKind::Class | PropertyKind::Component | PropertyKind::SafeObject));
                    if a_obj && b_obj && r.prop_refs(a).0 == self.class_class() {
                        is_iterator_cast = true;
                    }
                }
                let mut iterator_class: Option<FieldRef> = None;
                let mut parm_tokens: Vec<Token> = Vec::new();
                let mut count = 0usize;
                let mut idx = 0usize;
                while idx < parms.len() {
                    let p = parms[idx];
                    let pf = self.res().prop_flags(p);
                    if pf & cpf::RETURN_PARM != 0 {
                        break;
                    }
                    let ref_type = if pf & cpf::OUT_PARM != 0 { RefType::DualReference } else { RefType::AssignmentReference };
                    let mut parm = self.res().prop_type(p, ref_type);
                    if count == 1 {
                        if let Some(ic) = iterator_class {
                            parm.inner = Some(ic);
                        }
                    }
                    if count != 0 && !self.match_symbol(",")? {
                        if parm.flags & cpf::OPTIONAL_PARM == 0 {
                            return self.error(format!("Call to '{fname}': missing or bad parameter {}", count + 1));
                        }
                        break;
                    }
                    let tag = format!("Call to '{}', parameter {}", token.ident, count + 1);
                    let mut pt = Token::empty();
                    let res = self.compile_expr(parm.clone(), Some(&tag), Some(&mut pt), i32::MAX, None, false, None, false, None)?;
                    if res == -1 {
                        return self.error(format!("Call to '{}': type mismatch in parameter {}", token.ident, count + 1));
                    } else if res == 0 {
                        if parm.flags & cpf::OPTIONAL_PARM == 0 || (!self.peek_symbol(",")? && !self.peek_symbol(")")?) {
                            return self.error(format!("Call to '{}': bad or missing parameter {}", token.ident, count + 1));
                        }
                        self.emit_op(op::EMPTY_PARM_VALUE);
                    } else if is_iterator_cast && count == 0 {
                        if let super::engine::ConstVal::Object(o) = &pt.value {
                            if pt.kind == TokKind::Const {
                                iterator_class = *o;
                            }
                        }
                    }
                    parm_tokens.push(pt);
                    count += 1;
                    idx += 1;
                }
                while idx < parms.len() {
                    let p = parms[idx];
                    let pf = self.res().prop_flags(p);
                    if pf & cpf::RETURN_PARM != 0 {
                        break;
                    }
                    self.emit_op(op::EMPTY_PARM_VALUE);
                    idx += 1;
                }
                let temp = self.get_token_hint(None, false)?;
                let Some(temp) = temp.filter(|t| t.kind == TokKind::Symbol) else {
                    return self.error(format!("Call to '{}': Bad expression or missing ')'", token.ident));
                };
                if !temp.is_symbol(")") {
                    if temp.is_symbol(",") {
                        let n = self.res().func_num_parms(ff) - if self.res().func_return(ff).is_some() { 1 } else { 0 };
                        return self.error(format!("Call to '{}': Function only takes {} parameter(s)", token.ident, n));
                    }
                    return self.error(format!("Call to '{}': Bad '{}' or missing ')'", token.ident, temp.ident));
                }
                self.emit_op(op::END_FUNCTION_PARMS);
                self.emit_debug_info(0);

                let ret = self.res().func_return(ff);
                if let Some(rp) = ret {
                    let mut t = Token::from_type(self.res().prop_type(rp, RefType::None));
                    t.ty.flags &= !cpf::OUT_PARM;
                    if self.res().prop_flags(rp) & cpf::COERCE_PARM != 0 {
                        t.ty.inner = parm_tokens.first().and_then(|p| p.ty.meta_class);
                    }
                    *result = t;
                    self.affector_return_property = Some(rp);
                } else {
                    *result = Token::from_type(PropType::none());
                    self.affector_return_property = None;
                }
                if skipover_size_pos > 0 {
                    let arp = self.affector_return_property;
                    self.emit_field_placeholder_fixup(skipover_size_pos, arp);
                }
            } else {
                if skipover_size_pos > 0 {
                    self.emit_field_placeholder_fixup(skipover_size_pos, Some(ff));
                }
                *result = token.clone();
                result.ty.cpt = Cpt::Delegate;
                result.kind = TokKind::Const;
                self.emit_op(op::INSTANCE_DELEGATE);
                let fr = self.res().func_friendly_name(ff);
                self.emit_name_text(&fr);
            }
            result.ty.flags &= !cpf::OUT_PARM;
            result.func = Some(ff);
            return Ok(true);
        }

        if let (Some(c), Some(FieldKind::Const)) = (field, field_kind) {
            let value = self.res().const_value(c);
            let parsed = self.parse_const_text_hint(&value, Some(required))?;
            let Some(mut ct) = parsed.filter(|t| t.kind == TokKind::Const) else {
                return self.error("Error in constant");
            };
            self.attempt_to_convert_constant(&mut ct, required);
            ct.ty.flags &= !cpf::OUT_PARM;
            self.emit_constant(&ct)?;
            if skipover_size_pos > 0 {
                self.emit_field_placeholder_fixup(skipover_size_pos, field);
            }
            *result = ct;
            return Ok(true);
        }

        if field_class != FieldClass::Any {
            let what = match field_class {
                FieldClass::Property => "Property",
                FieldClass::Function => "Function",
                FieldClass::Const => "Const",
                FieldClass::Any => "Field",
            };
            return self.error(format!("Unknown {what} '{}' in '{}'", token.ident, self.res().name_of(scope)));
        }
        self.return_to(start_of_field_expression, true, true);
        Ok(false)
    }

    /// Tokenise a const's value text with a type hint.
    pub fn parse_const_text_hint(&mut self, text: &str, hint: Option<&PropType>) -> CResult<Option<Token>> {
        let save_input = std::mem::take(&mut self.input);
        let save = (self.lexer_pos, self.lexer_line, self.prev_pos, self.prev_line);
        self.input = super::engine::latin1_bytes(text);
        self.lexer_pos = 0;
        let r = self.get_token_hint(hint, false);
        self.input = save_input;
        self.lexer_pos = save.0;
        self.lexer_line = save.1;
        self.prev_pos = save.2;
        self.prev_line = save.3;
        r
    }
}
