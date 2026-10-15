//! Second pass (`PASS_Compile`): statements, affectors, nest epilogues and
//! `CompileSecondPass`.

use tempest_upk::flags::{cpf, func};

use super::engine::{allow, CResult, Compiler, Fixup, NestType, Token, MAXWORD, NESTF_RETURN_VALUE_FOUND};
use super::expr::MAX_SIMPLE_RETURN_VALUE_SIZE_BYTES;
use super::model::{FieldRef, NodeId, NodeKind};
use super::types::{Cpt, PropType, RefType, MAXINT};
use crate::bytecode::op;
use crate::objects::PropertyKind;

impl<'a> Compiler<'a> {
    /// `CompileAffector`.
    pub fn compile_affector(&mut self, left_ref: RefType, right_ref: RefType) -> CResult<()> {
        let low = self.location();
        self.got_affector = false;
        self.affector_return_property = None;
        let required = PropType::with_ref(Cpt::None, left_ref);
        let mut result = Token::empty();
        let mut chain: Vec<Token> = Vec::new();
        let mut smb: Option<u32> = None;
        if self.compile_expr(required, None, Some(&mut result), MAXINT, None, false, Some(&mut chain), false, Some(&mut smb))? < 0 {
            let t = self.get_token_hint(None, false)?.unwrap_or_else(Token::empty);
            return self.error(format!("'{}': Bad command or expression", t.ident));
        }
        let smb_pos = smb.and_then(|idx| self.top_nest_ref().struct_mod_bytes.get(idx as usize).copied());
        if let Some(idx) = smb {
            let list = &mut self.top_nest().struct_mod_bytes;
            if (idx as usize) < list.len() {
                list.truncate(idx as usize);
            }
        }
        if self.match_symbol("=")? {
            if result.ty.flags & cpf::OUT_PARM == 0 {
                if result.ty.flags & cpf::CONST != 0 {
                    return self.error("'=': Left value is const");
                }
                return self.error("'=': Left value is not a variable");
            }
            if let Some(pos) = smb_pos {
                self.write_u8_at(pos, 1);
            }
            if result.ty.flags & cpf::CONST != 0 && result.prop.map(|p| self.res().prop_flags(p) & cpf::CONST == 0).unwrap_or(false) {
                let pf = self.res().prop_flags(result.prop.unwrap());
                if pf & cpf::PROTECTED_WRITE != 0 {
                    return self.error(format!("{}: ProtectedWrite property is const here", result.ident));
                } else if pf & cpf::PRIVATE_WRITE != 0 {
                    return self.error(format!("{}: PrivateWrite property is const here", result.ident));
                } else {
                    if chain.len() >= 2 {
                        for t in chain[..chain.len() - 1].iter().rev() {
                            if t.matches_ident("default") {
                                return self.error(format!("'{}': Can't change default values of non-config properties", result.ident));
                            }
                        }
                    }
                    return self.error("'=': Left value is const");
                }
            }
            let mut required = result.ty.clone();
            required.flags &= !cpf::OUT_PARM;
            required.ref_type = right_ref;
            self.compile_expr(required, Some("'='"), None, MAXINT, None, false, None, false, None)?;
            let high = self.location();
            self.emit_let(&result.ty)?;
            self.move_compiled_code(low, high);
        } else if self.got_affector {
            if let Some(arp) = self.affector_return_property {
                let flags = self.res().prop_flags(arp);
                let t = self.res().prop_type(arp, RefType::None);
                let size = self.type_size(&t);
                if flags & cpf::NEED_CTOR_LINK != 0 || size > MAX_SIMPLE_RETURN_VALUE_SIZE_BYTES {
                    let high = self.location();
                    self.emit_op(op::EAT_RETURN_VALUE);
                    self.emit_ptr(Some(arp));
                    self.move_compiled_code(low, high);
                }
            }
        } else if result.ty.cpt != Cpt::None {
            let t = self.get_token_hint(None, false)?.unwrap_or_else(Token::empty);
            return self.error(format!("'{}': Expression has no effect", t.ident));
        } else {
            let t = self.get_token_hint(None, false)?.unwrap_or_else(Token::empty);
            return self.error(format!("'{}': Bad command or expression", t.ident));
        }
        Ok(())
    }

    fn label_nest_index(&self) -> Option<usize> {
        (2..self.nests.len()).rev().find(|&i| matches!(self.nests[i].nest_type, NestType::State | NestType::Function | NestType::ForEach))
    }

    /// `CompileCommand`.
    pub fn compile_command(&mut self, token: &Token, need_semicolon: &mut bool) -> CResult<()> {
        let l = token.ident.to_ascii_lowercase();
        let is_ident = token.kind == super::engine::TokKind::Identifier;
        if token.is_symbol("#") || (is_ident && l.starts_with('#')) {
            // Extension: `#linenumber` between statements (the decompiler restores assert lines with it).
            let name = if is_ident { Some(l[1..].to_string()) } else { None };
            self.compile_directive_named(name.as_deref())?;
            *need_semicolon = false;
        } else if is_ident && l == "switch" {
            self.check_allow("'Switch'", allow::CMD)?;
            self.push_nest(NestType::Switch, "", None)?;
            self.emit_debug_info(0);
            self.emit_op(op::SWITCH);
            let start = self.location();
            self.require_symbol("(", "'switch expression'")?;
            let mut st = Token::empty();
            self.compile_expr(PropType::with_ref(Cpt::None, RefType::SimpleReference), Some("'Switch'"), Some(&mut st), MAXINT, None, false, None, true, None)?;
            if st.ty.array_dim != 1 {
                return self.error("Can't switch on arrays");
            }
            self.require_symbol(")", "'switch expression'")?;
            let next = self.location();
            self.emit_property(&st);
            self.move_compiled_code(start, next);
            st.ty.flags &= !cpf::OUT_PARM;
            self.top_nest().switch_type = st;
            self.require_symbol("{", "'Switch'")?;
            *need_semicolon = false;
        } else if is_ident && l == "case" {
            self.check_allow("'Case'", allow::CASE)?;
            self.emit_chain_address_value();
            self.emit_op(op::CASE);
            self.emit_placeholder_for_chain();
            let st = self.top_nest_ref().switch_type.ty.clone();
            self.compile_expr(st, Some("'Case'"), None, MAXINT, None, false, None, true, None)?;
            self.require_symbol(":", "'Case'")?;
            *need_semicolon = false;
            self.top_nest().allow |= allow::CMD | allow::LABEL | allow::BREAK;
        } else if is_ident && l == "default" && self.top_nest_ref().allow & allow::CASE != 0 && !self.peek_symbol(".")? {
            self.check_allow("'Default'", allow::CASE)?;
            self.emit_chain_address_value();
            self.emit_op(op::CASE);
            self.emit_u16(MAXWORD as u16);
            self.require_symbol(":", "'Default'")?;
            *need_semicolon = false;
            self.top_nest().allow &= !allow::CASE;
            self.top_nest().allow |= allow::CMD | allow::LABEL | allow::BREAK;
        } else if is_ident && l == "return" {
            self.check_allow("'Return'", allow::RETURN)?;
            let mut i = self.nests.len() - 1;
            while i > 0 {
                if self.nests[i].nest_type == NestType::Function {
                    break;
                } else if self.nests[i].nest_type == NestType::ForEach {
                    self.emit_op(op::ITERATOR_POP);
                }
                i -= 1;
            }
            if i == 0 {
                return self.error("Internal consistency error on 'Return'");
            }
            let function = self.nests[i].node.expect("function node");
            let ret = self.res().func_return(FieldRef::Local(function));
            self.emit_debug_info(0);
            self.emit_op(op::RETURN);
            if let Some(rp) = ret {
                let mut property = self.res().prop_type(rp, RefType::SimpleReference);
                property.flags &= !cpf::OUT_PARM;
                self.compile_expr(property, Some("'Return'"), None, MAXINT, None, false, None, false, None)?;
                self.nests[i].nest_flags |= NESTF_RETURN_VALUE_FOUND;
            } else {
                self.emit_op(op::NOTHING);
            }
            self.emit_debug_info(0);
        } else if is_ident && l == "if" {
            self.check_allow("'If'", allow::CMD)?;
            self.push_nest(NestType::If, "", None)?;
            self.emit_debug_info(0);
            self.emit_op(op::JUMP_IF_NOT);
            self.emit_placeholder_for_chain();
            self.require_symbol("(", "'If'")?;
            self.compile_expr(PropType::with_ref(Cpt::Bool, RefType::SimpleReference), Some("'If'"), None, MAXINT, None, false, None, true, None)?;
            self.require_symbol(")", "'If'")?;
            *need_semicolon = false;
            if !self.match_symbol("{")? {
                self.compile_statements()?;
                self.pop_nest(NestType::If, "'If'")?;
            }
        } else if is_ident && l == "while" {
            self.check_allow("'While'", allow::CMD)?;
            self.push_nest(NestType::Loop, "", None)?;
            self.top_nest().allow |= allow::IN_WHILE;
            let here = self.code_len();
            self.top_nest().set_jump_target(Fixup::LoopStart, here);
            self.emit_debug_info(0);
            self.emit_op(op::JUMP_IF_NOT);
            let ni = self.nests.len() - 1;
            self.emit_placeholder_for_jump(ni, Fixup::LoopEnd, None, None, 0);
            self.require_symbol("(", "'While'")?;
            self.compile_expr(PropType::with_ref(Cpt::Bool, RefType::SimpleReference), Some("'While'"), None, MAXINT, None, false, None, true, None)?;
            self.require_symbol(")", "'While'")?;
            *need_semicolon = false;
            if !self.match_symbol("{")? {
                self.compile_statements()?;
                self.pop_nest(NestType::Loop, "'While'")?;
            }
        } else if is_ident && l == "do" {
            self.check_allow("'Do'", allow::CMD)?;
            self.push_nest(NestType::Loop, "", None)?;
            let here = self.code_len();
            self.top_nest().set_jump_target(Fixup::LoopStart, here);
            *need_semicolon = false;
            if !self.match_symbol("{")? {
                self.compile_statements()?;
                self.pop_nest(NestType::Loop, "'Do'")?;
            }
        } else if is_ident && l == "break" {
            self.check_allow("'Break'", allow::BREAK)?;
            let mut inest = self.find_nest(NestType::Loop).map(|i| i as i32).unwrap_or(-1);
            inest = inest.max(self.find_nest(NestType::For).map(|i| i as i32).unwrap_or(-1));
            inest = inest.max(self.find_nest(NestType::ForEach).map(|i| i as i32).unwrap_or(-1));
            inest = inest.max(self.find_nest(NestType::Switch).map(|i| i as i32).unwrap_or(-1));
            if inest <= 0 {
                return self.error("Internal consistency error on 'Break'");
            }
            let inest = inest as usize;
            self.emit_debug_info(0);
            self.emit_op(op::JUMP);
            let fixup = match self.nests[inest].nest_type {
                NestType::Switch => Fixup::SwitchEnd,
                NestType::ForEach => Fixup::IteratorEnd,
                NestType::For => Fixup::ForEnd,
                NestType::Loop => Fixup::LoopEnd,
                _ => Fixup::SwitchEnd,
            };
            self.emit_placeholder_for_jump(inest, fixup, None, None, 0);
        } else if is_ident && l == "continue" {
            self.check_allow("'Continue'", allow::CONTINUE)?;
            let mut inest = self.find_nest(NestType::Loop).map(|i| i as i32).unwrap_or(-1);
            inest = inest.max(self.find_nest(NestType::For).map(|i| i as i32).unwrap_or(-1));
            inest = inest.max(self.find_nest(NestType::ForEach).map(|i| i as i32).unwrap_or(-1));
            if inest <= 0 {
                return self.error("Internal consistency error on 'Continue'");
            }
            let inest = inest as usize;
            match self.nests[inest].nest_type {
                NestType::ForEach => {
                    self.emit_debug_info(0);
                    self.emit_op(op::ITERATOR_NEXT);
                    self.emit_op(op::JUMP);
                    self.emit_placeholder_for_jump(inest, Fixup::IteratorEnd, None, None, 0);
                }
                NestType::For => {
                    self.emit_debug_info(0);
                    self.emit_op(op::JUMP);
                    self.emit_placeholder_for_jump(inest, Fixup::ForInc, None, None, 0);
                }
                NestType::Loop => {
                    self.emit_debug_info(0);
                    self.emit_op(op::JUMP);
                    self.emit_placeholder_for_jump(inest, Fixup::LoopPostCond, None, None, 0);
                }
                _ => {}
            }
        } else if is_ident && l == "for" {
            self.check_allow("'For'", allow::CMD)?;
            self.push_nest(NestType::For, "", None)?;
            self.require_symbol("(", "'For'")?;
            self.emit_debug_info(0);
            self.supress_debug_info += 1;
            self.compile_affector(RefType::AssignValue, RefType::AssignmentReference)?;
            self.require_symbol(";", "'For'")?;
            let here = self.code_len();
            self.top_nest().set_jump_target(Fixup::ForStart, here);
            self.emit_debug_info(0);
            self.emit_op(op::JUMP_IF_NOT);
            let ni = self.nests.len() - 1;
            self.emit_placeholder_for_jump(ni, Fixup::ForEnd, None, None, 0);
            self.compile_expr(PropType::with_ref(Cpt::Bool, RefType::SimpleReference), Some("'For'"), None, MAXINT, None, false, None, true, None)?;
            self.require_symbol(";", "'For'")?;
            let retry = self.location();
            self.top_nest().for_retry = Some(retry);
            self.compile_affector(RefType::AssignValue, RefType::DualReference)?;
            self.return_to(retry, true, false);
            self.require_symbol(")", "'For'")?;
            *need_semicolon = false;
            if !self.match_symbol("{")? {
                self.compile_statements()?;
                self.pop_nest(NestType::For, "'If'")?;
            }
        } else if is_ident && l == "foreach" {
            self.check_allow("'ForEach'", allow::CMD)?;
            self.push_nest(NestType::ForEach, "", None)?;
            let low = self.location();
            let mut type_token = Token::empty();
            self.compile_expr(PropType::with_ref(Cpt::None, RefType::AssignmentReference), Some("'ForEach'"), Some(&mut type_token), MAXINT, None, false, None, true, None)?;
            let high = self.location();
            if type_token.ty.is_dynamic_array() {
                if type_token.func.is_some() {
                    return self.error("Cannot execute dynamic array iterator on the return value of a function");
                }
                self.emit_op(op::DYN_ARRAY_ITERATOR);
                self.move_compiled_code(low, high);
                self.require_symbol("(", "'foreach array'")?;
                let mut out_token = Token::empty();
                let mut required = PropType::with_ref(type_token.ty.cpt, RefType::AssignValue);
                required.inner = type_token.ty.inner;
                required.meta_class = type_token.ty.meta_class;
                required.flags |= cpf::OUT_PARM;
                self.compile_expr(required, Some("'foreach array'"), Some(&mut out_token), MAXINT, None, false, None, true, None)?;
                if self.match_symbol(",")? {
                    self.emit_u8(1);
                    let mut required = PropType::with_ref(Cpt::Int, RefType::AssignValue);
                    required.flags |= cpf::OUT_PARM;
                    self.compile_expr(required, Some("'foreach array'"), Some(&mut out_token), MAXINT, None, false, None, true, None)?;
                } else {
                    self.emit_u8(0);
                    self.emit_op(op::EMPTY_PARM_VALUE);
                }
                self.require_symbol(")", "'foreach array'")?;
            } else {
                self.emit_op(op::ITERATOR);
                self.move_compiled_code(low, high);
                if self.top_nest_ref().allow & allow::ITERATOR != 0 {
                    return self.error("'ForEach': An iterator expression is required");
                }
            }
            let ni = self.nests.len() - 1;
            self.emit_placeholder_for_jump(ni, Fixup::IteratorEnd, None, None, 0);
            self.emit_debug_info(0);
            *need_semicolon = false;
            if !self.match_symbol("{")? {
                self.compile_statements()?;
                self.pop_nest(NestType::ForEach, "'ForEach'")?;
            }
        } else if is_ident && l == "assert" {
            self.check_allow("'Assert'", allow::CMD)?;
            self.emit_debug_info(0);
            let line = self.lexer_line as u16;
            self.emit_op(op::ASSERT);
            self.emit_u16(line);
            self.emit_u8(0);
            self.compile_expr(PropType::with_ref(Cpt::Bool, RefType::SimpleReference), Some("'Assert'"), None, MAXINT, None, false, None, false, None)?;
        } else if is_ident && l == "goto" {
            self.check_allow("'Goto'", allow::LABEL)?;
            if self.top_nest_ref().allow & allow::STATE_CMD != 0 {
                self.emit_op(op::GOTO_LABEL);
                self.compile_expr(PropType::with_ref(Cpt::Name, RefType::AssignmentReference), Some("'Goto'"), None, MAXINT, None, false, None, false, None)?;
            } else {
                let Some(inest) = self.label_nest_index() else { return self.error("Goto is not allowed here") };
                let Some(label) = self.get_token_hint(None, false)? else { return self.error("Goto: Missing label") };
                if label.ident.is_empty() || label.ident.eq_ignore_ascii_case("None") {
                    return self.error(format!("Invalid label '{}'", label.ident));
                }
                self.emit_op(op::JUMP);
                self.emit_placeholder_for_jump(inest, Fixup::Label, Some(label.ident.clone()), None, 0);
            }
        } else if is_ident && l == "stop" {
            self.check_allow("'Stop'", allow::STATE_CMD)?;
            self.emit_debug_info(0);
            self.emit_op(op::STOP);
        } else if is_ident && l == "filtereditoronly" {
            self.check_allow("'FilterEditorOnly'", allow::CMD)?;
            self.push_nest(NestType::FilterEditorOnly, "", None)?;
            self.emit_debug_info(0);
            self.emit_op(op::JUMP_IF_FILTER_EDITOR_ONLY);
            let ni = self.nests.len() - 1;
            self.emit_placeholder_for_jump(ni, Fixup::FilterEditorOnly, None, None, 0);
            *need_semicolon = false;
            self.require_symbol("{", "'FilterEditorOnly'")?;
        } else if token.is_symbol("}") {
            match self.top_nest_ref().nest_type {
                NestType::Class | NestType::Interface => return self.error("Unexpected '}' at class scope"),
                NestType::None => return self.error("Unexpected '}' at global scope"),
                _ => {}
            }
            self.pop_nest(NestType::None, "'}'")?;
            *need_semicolon = false;
        } else if token.is_symbol(";") {
            *need_semicolon = false;
        } else if self.match_symbol(":")? {
            self.check_allow("Label", allow::LABEL)?;
            if token.ident.is_empty() || token.ident.eq_ignore_ascii_case("None") {
                return self.error(format!("Invalid label name '{}'", token.ident));
            }
            if self.top_nest_ref().allow & allow::CMD == 0 {
                self.top_nest().allow |= allow::CMD;
                self.top_nest().allow &= !(allow::FUNCTION | allow::VAR_DECL | allow::TYPE_DECL);
            } else if self.top_nest_ref().node.map(|n| self.model.node(n).kind == NodeKind::State).unwrap_or(false) {
                self.emit_debug_info(0);
            }
            let Some(inest) = self.label_nest_index() else { return self.error("Labels are not allowed here") };
            if self.nests[inest].labels.iter().any(|(n, _)| n.eq_ignore_ascii_case(&token.ident)) {
                return self.error(format!("Duplicate label '{}'", token.ident));
            }
            let here = self.code_len();
            // FLabelRecord list is prepended; the label table is emitted in list order.
            self.nests[inest].labels.insert(0, (token.ident.clone(), here));
            *need_semicolon = false;
        } else {
            self.check_allow("Expression", allow::CMD)?;
            self.unget(token);
            self.compile_affector(RefType::AssignValue, RefType::AssignmentReference)?;
        }
        if !self.top_nest_ref().struct_mod_bytes.is_empty() {
            self.top_nest().struct_mod_bytes.clear();
        }
        Ok(())
    }

    /// Compile-pass half of `PopNest`. Sets `else_continued` when the `if`
    /// nest is kept alive by an `else`.
    pub fn pop_nest_compile(&mut self, nest_type: NestType) -> CResult<()> {
        let ni = self.nests.len() - 1;
        match nest_type {
            NestType::State => {
                self.emit_debug_info(0);
                self.emit_op(op::STOP);
                if !self.nests[ni].labels.is_empty() {
                    while (self.code_len() & 3) != 3 {
                        self.emit_op(op::NOTHING);
                    }
                    self.emit_op(op::LABEL_TABLE);
                    let off = self.code_len() as u16;
                    let top = self.top_node.expect("state node");
                    self.model.node_mut(top).state.label_table_offset = off;
                    let labels = self.nests[ni].labels.clone();
                    for (name, icode) in labels {
                        self.emit_name_text(&name);
                        self.emit_i32(icode as i32);
                    }
                    self.emit_name_text("None");
                    self.emit_i32(MAXWORD as i32);
                }
            }
            NestType::Function => {
                let top = self.top_node.expect("function node");
                let fflags = self.model.node(top).func.flags;
                if fflags & func::NATIVE == 0 {
                    self.emit_debug_info(0);
                    self.emit_op(op::RETURN);
                    let ret = self.res().func_return(FieldRef::Local(top));
                    if let Some(rp) = ret {
                        self.emit_op(op::RETURN_NOTHING);
                        self.emit_ptr(Some(rp));
                    } else {
                        self.emit_op(op::NOTHING);
                    }
                    self.emit_debug_info(0);
                    if fflags & func::DEFINED != 0 && ret.is_some() && self.nests[ni].nest_flags & NESTF_RETURN_VALUE_FOUND == 0 {
                        self.warn(format!("{}: Missing return value", self.model.node(top).name));
                    }
                }
            }
            NestType::Switch => {
                if self.nests[ni].allow & allow::CASE != 0 {
                    self.emit_chain_address_value();
                    self.emit_op(op::CASE);
                    self.emit_u16(MAXWORD as u16);
                }
                let here = self.code_len();
                self.nests[ni].set_jump_target(Fixup::SwitchEnd, here);
            }
            NestType::If => {
                if self.match_identifier("else")? {
                    self.emit_op(op::JUMP);
                    self.emit_placeholder_for_jump(ni, Fixup::IfEnd, None, None, 0);
                    self.emit_chain_address_value();
                    if self.match_identifier("if")? {
                        self.check_allow("'Else If'", allow::ELSE_IF)?;
                        self.emit_debug_info(0);
                        self.emit_op(op::JUMP_IF_NOT);
                        self.emit_placeholder_for_chain();
                        self.require_symbol("(", "'Else If'")?;
                        self.compile_expr(PropType::with_ref(Cpt::Bool, RefType::SimpleReference), Some("'Else If'"), None, MAXINT, None, false, None, false, None)?;
                        self.require_symbol(")", "'Else If'")?;
                        if !self.match_symbol("{")? {
                            self.compile_statement()?;
                            self.pop_nest(NestType::If, "'ElseIf'")?;
                        }
                    } else {
                        self.check_allow("'Else'", allow::ELSE_IF)?;
                        self.top_nest().allow &= !allow::ELSE_IF;
                        if !self.match_symbol("{")? {
                            self.compile_statement()?;
                            self.pop_nest(NestType::If, "'Else'")?;
                        }
                    }
                    self.else_continued = true;
                    return Ok(());
                } else {
                    self.emit_chain_address_value();
                    let here = self.code_len();
                    self.nests[ni].set_jump_target(Fixup::IfEnd, here);
                }
            }
            NestType::For => {
                let here = self.code_len();
                self.nests[ni].set_jump_target(Fixup::ForInc, here);
                let retry = self.location();
                let for_retry = self.nests[ni].for_retry.expect("for retry");
                self.return_to(for_retry, false, true);
                self.emit_debug_info(0);
                self.compile_affector(RefType::AssignValue, RefType::AssignmentReference)?;
                self.return_to(retry, false, true);
                self.emit_op(op::JUMP);
                self.emit_placeholder_for_jump(ni, Fixup::ForStart, None, None, 0);
                let here = self.code_len();
                self.nests[ni].set_jump_target(Fixup::ForEnd, here);
            }
            NestType::ForEach => {
                self.emit_op(op::ITERATOR_NEXT);
                let here = self.code_len();
                self.nests[ni].set_jump_target(Fixup::IteratorEnd, here);
                self.emit_op(op::ITERATOR_POP);
                let here = self.code_len();
                self.nests[ni].set_jump_target(Fixup::IteratorSkip, here);
            }
            NestType::Loop => {
                let here = self.code_len();
                self.nests[ni].set_jump_target(Fixup::LoopPostCond, here);
                if self.match_identifier("until")? {
                    self.emit_op(op::JUMP_IF_NOT);
                    self.emit_placeholder_for_jump(ni, Fixup::LoopStart, None, None, 0);
                    self.require_symbol("(", "'Until'")?;
                    self.compile_expr(PropType::with_ref(Cpt::Bool, RefType::AssignmentReference), Some("'Until'"), None, MAXINT, None, false, None, false, None)?;
                    let end = self.location();
                    self.require_symbol(")", "'Until'")?;
                    if !self.match_symbol(";")? {
                        self.return_to(end, false, true);
                        return self.error("Missing ';' in 'Until'");
                    }
                    let here = self.code_len();
                    self.nests[ni].set_jump_target(Fixup::LoopEnd, here);
                } else {
                    if self.nests[ni].allow & allow::IN_WHILE == 0 {
                        if self.peek_identifier("while")? {
                            return self.error("The loop syntax is do...until, not do...while");
                        }
                        return self.error("Do loops must end with 'until(condition)' statement");
                    }
                    self.emit_op(op::JUMP);
                    self.emit_placeholder_for_jump(ni, Fixup::LoopStart, None, None, 0);
                    let here = self.code_len();
                    self.nests[ni].set_jump_target(Fixup::LoopEnd, here);
                }
            }
            NestType::FilterEditorOnly => {
                let here = self.code_len();
                self.nests[ni].set_jump_target(Fixup::FilterEditorOnly, here);
            }
            _ => {}
        }
        self.replace_placeholder_values(ni)?;
        Ok(())
    }

    /// `CompileParameterValue`.
    fn compile_parameter_value(&mut self, prop: NodeId, required: &mut PropType) -> CResult<()> {
        self.emit_debug_info(0);
        let optional = required.flags & cpf::OPTIONAL_PARM;
        required.flags &= !cpf::OPTIONAL_PARM;
        required.ref_type = RefType::AssignmentReference;
        let hint = Token::from_type(self.res().prop_type(FieldRef::Local(prop), RefType::None));
        self.compile_expr(required.clone(), Some("'='"), None, MAXINT, Some(&hint), false, None, false, None)?;
        required.flags |= optional;
        self.emit_debug_info(0);
        self.emit_op(op::END_PARM_VALUE);
        Ok(())
    }

    /// `CompileSecondPass`.
    pub fn compile_second_pass(&mut self, node: NodeId) -> CResult<()> {
        let start_nest_level = self.nests.len();
        let kind = self.model.node(node).kind.clone();
        let new_nest = match kind {
            NodeKind::Function => NestType::Function,
            NodeKind::Class => {
                if self.model.node(node).class.class_flags & tempest_upk::flags::class::INTERFACE != 0 {
                    NestType::Interface
                } else {
                    NestType::Class
                }
            }
            NodeKind::State => NestType::State,
            _ => return self.error("Bad node for second pass"),
        };
        let name = self.model.node(node).name.clone();
        self.push_nest(new_nest, &name, Some(node))?;
        self.model.node_mut(node).strct.script_mem.clear();
        let is_function = kind == NodeKind::Function;
        if is_function {
            if let Some(sup) = self.model.node(node).strct.super_struct {
                let sflags = self.res().func_flags(sup);
                let f = self.model.node_mut(node);
                f.func.flags &= !func::NET_FUNC_FLAGS;
                f.func.flags |= sflags & func::NET_FUNC_FLAGS;
            }
            let previous = self.location();
            let parms = self.res().func_parms(FieldRef::Local(node));
            let defaults = self.model.node(node).func.param_defaults.clone();
            for p in parms {
                let FieldRef::Local(pid) = p else { continue };
                let pflags = self.model.node(pid).prop.flags;
                if pflags & cpf::OPTIONAL_PARM == 0 {
                    continue;
                }
                if let Some(&(_, pos, line)) = defaults.iter().find(|(id, _, _)| *id == pid) {
                    self.lexer_pos = pos;
                    self.lexer_line = line;
                    self.emit_op(op::DEFAULT_PARM_VALUE);
                    let start = self.code_len();
                    self.emit_u16(0);
                    let mut required = self.res().prop_type(p, RefType::None);
                    self.compile_parameter_value(pid, &mut required)?;
                    if !self.match_symbol(",")? && !self.match_symbol(")")? {
                        return self.error(format!("Missing ',' or ')' after value for optional parameter '{}'", self.model.node(pid).name));
                    }
                    let skip = self.code_len() - (start + 2);
                    self.write_u16_at(start, skip as u16);
                } else {
                    self.emit_op(op::NOTHING);
                }
            }
            self.return_to(previous, false, true);
        }
        if node == self.class && self.model.node(node).strct.line != -1 {
            let pos = self.model.node(node).strct.text_pos as usize;
            let line = self.model.node(node).strct.line as u32;
            self.lexer_pos = pos;
            self.prev_pos = pos;
            self.lexer_line = line;
            self.prev_line = line;
            loop {
                let Some(t) = self.get_token_hint(None, false)? else { return self.error("Unexpected end of replication block") };
                if t.is_symbol("}") {
                    break;
                } else if t.matches_ident("reliable") || t.matches_ident("unreliable") {
                    self.warn("Reliabe/Unreliable keywords are no longer used in 'replication {}'");
                } else {
                    self.unget(&t);
                }
                self.require_identifier("if", "Replication statement")?;
                self.require_symbol("(", "Replication condition")?;
                let rep_offset = self.code_len() as u16;
                self.compile_expr(PropType::new(Cpt::Bool), Some("Replication condition"), None, MAXINT, None, false, None, false, None)?;
                self.require_symbol(")", "Replication condition")?;
                loop {
                    let Some(vt) = self.get_identifier(false)? else { return self.error("Missing variable name in replication definition") };
                    let mut found = false;
                    let class_props: Vec<NodeId> = self.model.node(self.class).strct.children.iter().copied().filter(|&c| matches!(self.model.node(c).kind, NodeKind::Property(_))).collect();
                    for p in class_props {
                        if self.model.node(p).name.eq_ignore_ascii_case(&vt.ident) {
                            let n = self.model.node(p);
                            if n.prop.flags & cpf::NET != 0 {
                                return self.error(format!("Variable '{}' already has a replication definition", vt.ident));
                            }
                            if n.kind == NodeKind::Property(PropertyKind::Array) {
                                return self.error(format!("Dynamic arrays cannot be replicated '{}'", vt.ident));
                            } else if n.kind == NodeKind::Property(PropertyKind::Delegate) {
                                return self.error(format!("Delegates cannot be replicated (replicated delegates are insecure) '{}'", vt.ident));
                            } else if n.prop.array_dim > 256 {
                                self.warn("Static array elements beyond index 255 cannot be replicated");
                            }
                            let n = self.model.node_mut(p);
                            n.prop.flags |= cpf::NET;
                            n.prop.rep_offset = rep_offset;
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        let class_funcs: Vec<NodeId> = self.model.node(self.class).strct.children.iter().copied().filter(|&c| self.model.node(c).kind == NodeKind::Function).collect();
                        for f in class_funcs {
                            if self.model.node(f).name.eq_ignore_ascii_case(&vt.ident) {
                                self.warn(format!("Functions are no longer handled by 'replication {{}}', '{}'", vt.ident));
                                found = true;
                                break;
                            }
                        }
                    }
                    if !found {
                        return self.error(format!("Bad variable or '{}' in replication definition", vt.ident));
                    }
                    if !self.match_symbol(",")? {
                        break;
                    }
                }
                self.require_symbol(";", "Replication definition")?;
            }
            let class_props: Vec<NodeId> = self.model.node(self.class).strct.children.iter().copied().filter(|&c| matches!(self.model.node(c).kind, NodeKind::Property(_))).collect();
            for p in class_props {
                let f = self.model.node(p).prop.flags;
                if f & cpf::REP_NOTIFY != 0 && f & cpf::NET == 0 {
                    self.warn(format!("Property '{}' declared with 'repnotify' but is not replicated", self.model.node(p).name));
                }
            }
        }
        let children = self.model.node(node).strct.children.clone();
        for c in children {
            if matches!(self.model.node(c).kind, NodeKind::Function | NodeKind::State) {
                self.compile_second_pass(c)?;
            }
        }
        if self.top_node != Some(node) {
            return self.error("Internal error: top node mismatch after children");
        }
        let mut do_compile = false;
        if self.model.node(node).strct.line != -1 && node != self.class {
            let pos = self.model.node(node).strct.text_pos as usize;
            let line = self.model.node(node).strct.line as u32;
            self.lexer_pos = pos;
            self.prev_pos = pos;
            self.lexer_line = line;
            self.prev_line = line;
            if is_function {
                let fflags = self.model.node(node).func.flags;
                if fflags & func::NATIVE == 0 {
                    do_compile = fflags & func::DEFINED != 0;
                    self.emit_debug_info(0);
                } else {
                    let parms = self.res().func_parms(FieldRef::Local(node));
                    for p in parms {
                        let pf = self.res().prop_flags(p);
                        if pf & cpf::RETURN_PARM != 0 {
                            break;
                        }
                        self.emit_op(op::NATIVE_PARM);
                        self.emit_ptr(Some(p));
                    }
                    self.emit_op(op::NOTHING);
                }
            } else if kind == NodeKind::State {
                do_compile = true;
            }
        }
        if do_compile {
            while self.nests.len() > start_nest_level {
                if !self.compile_statement()? {
                    return self.error(format!("Unexpected end of code in {}", if is_function { "Function" } else { "State" }));
                }
            }
        } else if node != self.class {
            let descr = if is_function { "Function" } else { "State" };
            self.pop_nest(new_nest, descr)?;
        }
        Ok(())
    }
}
