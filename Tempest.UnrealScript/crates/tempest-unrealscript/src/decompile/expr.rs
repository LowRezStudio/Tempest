//! Expression printing.

use tempest_upk::flags::{cpf, func};
use tempest_upk::io::{FName, PackageIndex};

use super::{escape_string, escape_utf16, format_float, name_literal, Decompiler};
use crate::bytecode::{cast, Args, ContextKind, Token};
use crate::context::ObjRef;
use crate::objects::{ObjectBody, PropertyKind};

/// Precedence used for parenthesisation: 0 = atom. Larger binds weaker.
pub const PREC_ATOM: u8 = 0;
pub const PREC_UNARY: u8 = 1;
pub const PREC_TERNARY: u8 = 100;

/// Typing hint for printing constants.
#[derive(Clone, Copy, Default)]
pub struct Hint {
    pub enum_: Option<ObjRef>,
    /// The expression is in a boolean context (`if`, `!`, `&&`), so a
    /// trailing `bool(...)` cast can be left implicit.
    pub bool_ctx: bool,
}

/// Where the printing happens; affects `self`, `Super` and member naming.
#[derive(Clone, Copy)]
pub struct Scope {
    pub class: ObjRef,
    /// Enclosing state when inside state code or a state function.
    pub state: Option<ObjRef>,
    /// Enclosing function (None for state code / replication).
    pub function: Option<ObjRef>,
}

pub struct Printed {
    pub text: String,
    pub prec: u8,
}

impl Printed {
    fn atom(text: String) -> Printed {
        Printed { text, prec: PREC_ATOM }
    }
}

impl<'a> Decompiler<'a> {
    fn pkgname(&self, n: FName) -> String {
        self.ctx.packages[self.pkg].name_of(n)
    }

    fn res(&self, idx: PackageIndex) -> Option<ObjRef> {
        self.ctx.resolve(self.pkg, idx)
    }

    /// Name of an object reference as it appears in script (just the name).
    /// Stand-in for a cooked-out editor-only variable of the given type.
    pub fn stripped_var(&self, ty: &str) -> String {
        let mut refs = self.stripped_refs.borrow_mut();
        if let Some((_, n)) = refs.iter().find(|(t, _)| t.eq_ignore_ascii_case(ty)) {
            return n.clone();
        }
        let name = format!("EditorOnlyStripped_{}", ty.replace(['.', '<', '>'], "_"));
        refs.push((ty.to_string(), name.clone()));
        name
    }

    pub fn ref_name(&self, idx: PackageIndex) -> String {
        let n = match self.res(idx) {
            Some(r) => self.ctx.name(r),
            None => self.ctx.packages[self.pkg].pkg.object_name(idx),
        };
        // Compiler-generated delegate properties are written by their delegate's name.
        if let Some(inner) = n.strip_prefix("__").and_then(|x| x.strip_suffix("__Delegate")) {
            return inner.to_string();
        }
        n
    }

    /// `Class'Package.Path'` literal for an object reference.
    pub fn object_literal(&self, idx: PackageIndex) -> String {
        if idx.is_null() {
            return "None".to_string();
        }
        let pkg = &self.ctx.packages[self.pkg].pkg;
        let class = pkg.class_name_of(idx);
        let path = if idx.is_import() {
            pkg.object_path(idx)
        } else {
            format!("{}.{}", self.ctx.packages[self.pkg].name, pkg.object_path(idx))
        };
        if class == "Class" {
            format!("class'{path}'")
        } else {
            format!("{class}'{path}'")
        }
    }

    // ----- type queries ------------------------------------------------

    /// The property an expression reads/writes, if it is a variable-like expression.
    pub fn expr_property(&self, t: &Token) -> Option<ObjRef> {
        match t {
            Token::LocalVariable(p)
            | Token::InstanceVariable(p)
            | Token::DefaultVariable(p)
            | Token::StateVariable(p)
            | Token::LocalOutVariable(p)
            | Token::NativeParm(p) => self.res(*p),
            Token::Context { object, member, .. } => {
                // A call by name on another object: its return value, through that object's class.
                if let Token::VirtualFunction { name, .. } | Token::GlobalFunction { name, .. } = &**member {
                    let class = self.expr_object_class(object)?;
                    let f = self.ctx.find_function(class, None, &self.pkgname(*name))?;
                    return self.ctx.function_return(f);
                }
                self.expr_property(member)
            }
            Token::VirtualFunction { name, .. } | Token::GlobalFunction { name, .. } => {
                let f = self.ctx.find_function(self.class, None, &self.pkgname(*name))?;
                self.ctx.function_return(f)
            }
            Token::StructMember { prop, .. } => self.res(*prop),
            Token::BoolVariable(e) | Token::InterfaceContext(e) => self.expr_property(e),
            Token::ArrayElement { base, .. } => self.expr_property(base),
            Token::DynArrayElement { base, .. } => {
                let arr = self.expr_property(base)?;
                let (k, d) = self.ctx.property(arr)?;
                if k == PropertyKind::Array {
                    self.ctx.resolve(arr.pkg, d.ref1?)
                } else {
                    None
                }
            }
            Token::FinalFunction { func, .. } => self.ctx.function_return(self.res(*func)?),
            Token::NativeFunction { index, .. } => self.ctx.function_return(self.ctx.native_function(*index)?),
            Token::Skip { expr, .. } => self.expr_property(expr),
            Token::PrimitiveCast { cast: c, expr }
                if matches!(*c, cast::BYTE_TO_INT | cast::BYTE_TO_FLOAT | cast::INT_TO_FLOAT) =>
            {
                self.expr_property(expr)
            }
            _ => None,
        }
    }

    pub fn expr_enum(&self, t: &Token) -> Option<ObjRef> {
        let p = self.expr_property(t)?;
        self.ctx.enum_of_property(p)
    }

    /// Drop an auto-conversion cast (`ByteToInt`, `ByteToFloat`, `IntToFloat`)
    /// that the compiler re-inserts on its own in a typed context.
    pub fn strip_autocast<'t>(&self, t: &'t Token) -> &'t Token {
        match t {
            Token::PrimitiveCast { cast: c, expr }
                if matches!(
                    *c,
                    cast::BYTE_TO_INT | cast::BYTE_TO_FLOAT | cast::INT_TO_FLOAT | cast::SAFE_OBJECT_TO_OBJECT | cast::OBJECT_TO_SAFE_OBJECT | cast::SAFE_OBJECT_TO_INTERFACE | cast::INTERFACE_TO_SAFE_OBJECT
                ) =>
            {
                expr
            }
            Token::InterfaceCast { expr, .. } => expr,
            _ => t,
        }
    }

    /// Like `strip_autocast`, for a value assigned/passed/returned to a known type:
    /// int→byte is automatic there too, unless the target is an enum (`Enum(x)`).
    pub fn strip_autocast_to<'t>(&self, t: &'t Token, hint: Hint) -> &'t Token {
        // A byte constant converted to int can only have been an enum tag: keep the cast so
        // the printer spells the tag (a bare number would read back as an int).
        if let Token::PrimitiveCast { cast: c, expr } = t {
            if *c == cast::BYTE_TO_INT {
                if let Token::ByteConst(v) = **expr {
                    if self.max_tag_for(v).is_some() {
                        return t;
                    }
                }
            }
        }
        if hint.enum_.is_none() {
            if let Token::PrimitiveCast { cast: c, expr } = t {
                // Written bare, the operand would be re-read under the byte hint (literals and
                // conditionals become byte constants): only an explicit `byte(...)` keeps it int.
                if *c == cast::INT_TO_BYTE && !self.hint_sensitive(expr) {
                    return expr;
                }
            }
        }
        self.strip_autocast(t)
    }

    /// Whether the first token the compiler reads for this expression takes its type
    /// from the required-type hint (numeric literals, conditional branches).
    fn hint_sensitive(&self, t: &Token) -> bool {
        match t {
            Token::Conditional { .. } => true,
            Token::IntConst(_) | Token::IntConstByte(_) | Token::IntZero | Token::IntOne | Token::FloatConst(_) => true,
            Token::Skip { expr, .. } => self.hint_sensitive(expr),
            Token::NativeFunction { index, args } => {
                // An operator passes the hint on to its left operand.
                let is_operator = self
                    .ctx
                    .native_function(*index)
                    .and_then(|f| match self.ctx.object(f).map(|o| &o.body) {
                        Some(ObjectBody::Function { function, .. }) => Some(function.function_flags & func::OPERATOR != 0),
                        _ => None,
                    })
                    .unwrap_or(false);
                is_operator && args.args.first().map(|a| self.hint_sensitive(a)).unwrap_or(false)
            }
            _ => false,
        }
    }

    /// Coarse value type of an expression, for operator overload reasoning.
    pub fn operand_kind(&self, t: &Token) -> OperandKind {
        match t {
            Token::IntConst(_) | Token::IntZero | Token::IntOne | Token::IntConstByte(_) => OperandKind::Int,
            Token::FloatConst(_) => OperandKind::Float,
            Token::ByteConst(_) => OperandKind::Byte,
            Token::PrimitiveCast { cast: c, .. } => match cast_type_name(*c) {
                Some("int") => OperandKind::Int,
                Some("float") => OperandKind::Float,
                Some("byte") => OperandKind::Byte,
                _ => OperandKind::Other,
            },
            Token::FinalFunction { func, .. } => self.res(*func).and_then(|f| self.ctx.function_return(f)).map(|p| self.prop_operand_kind(p)).unwrap_or(OperandKind::Other),
            Token::NativeFunction { index, .. } => self.ctx.native_function(*index).and_then(|f| self.ctx.function_return(f)).map(|p| self.prop_operand_kind(p)).unwrap_or(OperandKind::Other),
            Token::DynArrayLength(_) => OperandKind::Int,
            _ => self.expr_property(t).map(|p| self.prop_operand_kind(p)).unwrap_or(OperandKind::Other),
        }
    }

    fn prop_operand_kind(&self, p: ObjRef) -> OperandKind {
        match self.ctx.property_kind(p) {
            Some(PropertyKind::Byte) => OperandKind::Byte,
            Some(PropertyKind::Int) => OperandKind::Int,
            Some(PropertyKind::Float) => OperandKind::Float,
            _ => OperandKind::Other,
        }
    }

    /// For a binary operator operand: the token to print, with its auto-cast
    /// removed when overload resolution would pick the same operator anyway.
    fn operator_operand<'t>(&self, me: &'t Token, other: &'t Token) -> &'t Token {
        // An object operand converted to the operator's interface parameter type: the
        // conversion is automatic (and `None` needs no spelling either).
        if let Token::InterfaceCast { expr, .. } = me {
            return expr;
        }
        let Token::PrimitiveCast { cast: c, expr } = me else { return me };
        // A byte literal was only read as a byte because of the other operand's type;
        // written bare it compiles back the same way (or as its enum tag).
        if let Token::ByteConst(v) = **expr {
            if matches!(*c, cast::BYTE_TO_INT | cast::BYTE_TO_FLOAT) {
                // Unless it was an enum's `_MAX` tag, which the printer spells out.
                if !(*c == cast::BYTE_TO_INT && self.max_tag_for(v).is_some()) {
                    return expr;
                }
                return me;
            }
        }
        let target = match *c {
            cast::BYTE_TO_INT => OperandKind::Int,
            cast::BYTE_TO_FLOAT | cast::INT_TO_FLOAT => OperandKind::Float,
            _ => return me,
        };
        let other_kind = self.operand_kind(self.strip_autocast(other));
        // An int literal next to a byte operand would be re-read as a byte constant
        // (the left operand's type is the hint), so an explicit `int()` must stay.
        let other_int_literal = matches!(other, Token::IntZero | Token::IntOne | Token::IntConst(_) | Token::IntConstByte(_));
        if *c == cast::BYTE_TO_INT && other_int_literal {
            return me;
        }
        let drop = other_kind == target || (target == OperandKind::Int && other_kind == OperandKind::Byte);
        if drop { expr } else { me }
    }

    /// The `<Prefix>_MAX` tag with index `v` of an enum declared along the class chain.
    fn max_tag_for(&self, v: u8) -> Option<String> {
        let mut cur = Some(self.class);
        let mut guard = 0;
        while let Some(c) = cur {
            for ch in self.ctx.children(c) {
                if self.ctx.class_name(ch) == "Enum" {
                    let names = self.ctx.enum_names(ch);
                    if names.len() == v as usize + 1 && names.last().map(|l| l.to_ascii_uppercase().ends_with("_MAX")).unwrap_or(false) {
                        // Qualified: a bare tag would resolve to an int constant.
                        return names.last().map(|t| format!("{}.{t}", self.ctx.name(ch)));
                    }
                }
            }
            cur = self.ctx.super_of(c);
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        None
    }

    fn enum_tag(&self, e: ObjRef, value: u8) -> Option<String> {
        let names = self.ctx.enum_names(e);
        names.get(value as usize).cloned()
    }

    // ----- functions ----------------------------------------------------

    fn function_info(&self, f: ObjRef) -> Option<(u32, u8, String, Vec<ObjRef>)> {
        match &self.ctx.object(f)?.body {
            crate::objects::ObjectBody::Function { function, .. } => Some((
                function.function_flags,
                function.oper_precedence,
                self.ctx.packages[f.pkg].name_of(function.friendly_name),
                self.ctx.function_params(f),
            )),
            _ => None,
        }
    }

    /// Print call arguments. `params` gives per-argument hints.
    fn print_args(&self, args: &Args, params: &[ObjRef], scope: Scope) -> String {
        let mut parts: Vec<String> = Vec::new();
        let n = args.args.len();
        // Trim trailing empty optional parameters.
        let mut last = n;
        while last > 0 && matches!(args.args[last - 1], Token::EmptyParmValue) {
            last -= 1;
        }
        for (i, a) in args.args[..last].iter().enumerate() {
            if matches!(a, Token::EmptyParmValue) {
                parts.push(String::new());
                continue;
            }
            let hint = Hint {
                enum_: params.get(i).and_then(|&p| self.ctx.enum_of_property(p)),
                bool_ctx: params
                    .get(i)
                    .and_then(|&p| self.ctx.property(p))
                    .map(|(k, d)| k == PropertyKind::Bool && d.property_flags & cpf::OUT_PARM == 0)
                    .unwrap_or(false),
            };
            if let Some(text) = self.null_var_text(a, params.get(i).copied(), scope) {
                parts.push(text);
                continue;
            }
            parts.push(self.expr(self.strip_autocast_to(a, hint), hint, scope).text);
        }
        parts.join(", ")
    }

    /// A reference the cooker nulled (`InstanceVariable(None)`): a stand-in variable of the
    /// type the use site expects (`typed_by`'s type), so the expression compiles the same way.
    fn null_var_text(&self, t: &Token, typed_by: Option<ObjRef>, scope: Scope) -> Option<String> {
        let Token::InstanceVariable(p) = t else { return None };
        if !p.is_null() {
            return None;
        }
        let ty = typed_by.map(|p| self.type_name(p, scope.class)).unwrap_or_else(|| "int".into());
        Some(self.stripped_var(&ty))
    }

    fn print_call(&self, f: ObjRef, args: &Args, scope: Scope, force_name: Option<&str>) -> Printed {
        let Some((flags, prec, friendly, params)) = self.function_info(f) else {
            return Printed::atom(format!("{}({})", self.ctx.name(f), self.print_args(args, &[], scope)));
        };
        let name = force_name.map(|s| s.to_string()).unwrap_or(friendly);
        if flags & func::OPERATOR != 0 {
            let a: Vec<&Token> = args.args.iter().collect();
            if params.len() == 2 && a.len() == 2 {
                let lh = self.expr_enum(a[0]).or_else(|| self.ctx.enum_of_property(params[0]));
                let rh = self.expr_enum(a[1]).or_else(|| self.ctx.enum_of_property(params[1]));
                let a0 = self.operator_operand(a[0], a[1]);
                let a1 = self.operator_operand(a[1], a[0]);
                let l = match self.null_var_text(a0, Some(params[0]), scope) {
                    Some(t) => Printed::atom(t),
                    None => self.expr(a0, Hint { enum_: rh.or(lh), bool_ctx: self.is_bool_param(params[0]) }, scope),
                };
                let r = match self.null_var_text(a1, Some(params[1]), scope) {
                    Some(t) => Printed::atom(t),
                    None => self.expr(a1, Hint { enum_: lh.or(rh), bool_ctx: self.is_bool_param(params[1]) }, scope),
                };
                let lt = if l.prec > prec || (l.prec == prec && false) { format!("({})", l.text) } else { l.text };
                let rt = if r.prec >= prec { format!("({})", r.text) } else { r.text };
                return Printed { text: format!("{lt} {name} {rt}"), prec };
            }
            if params.len() == 1 && a.len() == 1 {
                let x = self.expr(a[0], Hint { enum_: None, bool_ctx: self.is_bool_param(params[0]) }, scope);
                // `-3.5` lexes as a negative literal; a negated constant needs parentheses.
                let numeric_literal = matches!(a[0], Token::IntConst(_) | Token::IntConstByte(_) | Token::IntZero | Token::IntOne | Token::FloatConst(_) | Token::ByteConst(_));
                let xt = if x.prec > PREC_UNARY || (name == "-" && numeric_literal) { format!("({})", x.text) } else { x.text };
                if flags & func::PRE_OPERATOR != 0 {
                    return Printed { text: format!("{name}{xt}"), prec: PREC_UNARY };
                }
                return Printed { text: format!("{xt}{name}"), prec: PREC_UNARY };
            }
        }
        Printed::atom(format!("{}({})", self.ctx.name(f), self.print_args(args, &params, scope)))
    }

    fn is_bool_param(&self, p: ObjRef) -> bool {
        self.ctx.property(p).map(|(k, _)| k == PropertyKind::Bool).unwrap_or(false)
    }

    /// Prefix needed to call `f` with `EX_FinalFunction` from `scope`.
    fn super_prefix(&self, f: ObjRef, scope: Scope) -> String {
        let Some((flags, ..)) = self.function_info(f) else { return String::new() };
        if flags & (func::FINAL | func::PRIVATE | func::DELEGATE) != 0 {
            return String::new();
        }
        // Where does f live?
        let owner = self.ctx.outer_of(f);
        let Some(owner) = owner else { return String::new() };
        if flags & func::STATIC != 0 {
            // Static calls bind at compile time: a plain call reaches `f` unless a class
            // between the scope and the owner overrides it, in which case it was `Super.f()`.
            let fname = self.ctx.name(f);
            let mut cur = Some(scope.class);
            let mut overridden = false;
            let mut guard = 0;
            while let Some(c) = cur {
                if c == owner {
                    break;
                }
                if self.ctx.children(c).into_iter().any(|ch| self.ctx.class_name(ch) == "Function" && self.ctx.name(ch).eq_ignore_ascii_case(&fname)) {
                    overridden = true;
                    break;
                }
                cur = self.ctx.super_of(c);
                guard += 1;
                if guard > 100 {
                    break;
                }
            }
            if !overridden {
                return String::new();
            }
        }
        // Calling a function of the current scope (same class, same state) finally: that is what
        // `Super.` resolves to only when the owner is above us.
        if let Some(st) = scope.state {
            if owner == st {
                return String::new();
            }
        }
        if owner == scope.class && scope.state.is_none() {
            return String::new();
        }
        let is_class = self.ctx.class_name(owner) == "Class";
        if is_class {
            let direct_super = self.ctx.super_of(scope.class);
            if Some(owner) == direct_super {
                return "Super.".to_string();
            }
            if owner == scope.class {
                // in a state, calling the class version
                return "Super.".to_string();
            }
            return format!("Super({}).", self.ctx.name(owner));
        }
        // owner is a state
        "Super.".to_string()
    }

    // ----- main entry ---------------------------------------------------

    pub fn expr_text(&self, t: &Token, hint: Hint, scope: Scope) -> String {
        self.expr(t, hint, scope).text
    }

    pub fn expr(&self, t: &Token, hint: Hint, scope: Scope) -> Printed {
        match t {
            Token::LocalVariable(p) | Token::LocalOutVariable(p) | Token::NativeParm(p) | Token::StateVariable(p) => {
                Printed::atom(self.ref_name(*p))
            }
            Token::InstanceVariable(p) if p.is_null() => Printed::atom(self.stripped_var("int")),
            Token::InstanceVariable(p) => Printed::atom(self.ref_name(*p)),
            Token::DefaultVariable(p) => Printed::atom(format!("default.{}", self.ref_name(*p))),
            Token::BoolVariable(e) | Token::InterfaceContext(e) => self.expr(e, hint, scope),
            Token::Skip { expr, .. } => self.expr(expr, hint, scope),
            Token::Context { kind, object, member, skip, .. } => {
                // Implicit access through the `within` class: the compiler emits an outer
                // context with a fixed skip (`DEFAULT_OUTERCONTEXT_SIZE`), not the member size.
                if *kind == ContextKind::Object && *skip == 9 && self.is_implicit_outer_chain(object) {
                    return self.expr(member, hint, scope);
                }
                let obj = self.expr(object, Hint::default(), scope);
                let objt = if obj.prec > PREC_ATOM { format!("({})", obj.text) } else { obj.text };
                // `Object.CONSTNAME`: the compiler inlines the constant inside the context.
                if let Some(lit) = self.literal_text(member) {
                    let class = self.expr_object_class(object);
                    return match class.and_then(|c| self.const_named_for(c, &lit)) {
                        Some(n) => Printed::atom(format!("{objt}.{n}")),
                        None => self.expr(member, hint, scope),
                    };
                }
                let mem = self.member_text(member, hint, scope);
                let text = match kind {
                    ContextKind::Class => {
                        let inner: &Token = match &**member {
                            Token::BoolVariable(e) => e,
                            m => m,
                        };
                        if matches!(inner, Token::DefaultVariable(_)) {
                            format!("{objt}.{mem}")
                        } else {
                            format!("{objt}.static.{mem}")
                        }
                    }
                    _ => format!("{objt}.{mem}"),
                };
                Printed::atom(text)
            }
            Token::StructMember { prop, strct, expr, .. } => {
                // A struct variable the cooker removed: the member access survives on a null reference.
                if let Token::InstanceVariable(p) = &**expr {
                    if p.is_null() {
                        let bt = self.stripped_var(&self.ref_name(*strct));
                        return Printed::atom(format!("{bt}.{}", self.ref_name(*prop)));
                    }
                }
                let base = self.expr(expr, Hint::default(), scope);
                let bt = if base.prec > PREC_ATOM { format!("({})", base.text) } else { base.text };
                Printed::atom(format!("{bt}.{}", self.ref_name(*prop)))
            }
            Token::ArrayElement { index, base } | Token::DynArrayElement { index, base } => {
                let b = self.expr(base, Hint::default(), scope);
                let bt = if b.prec > PREC_ATOM { format!("({})", b.text) } else { b.text };
                let i = self.expr_text(index, Hint::default(), scope);
                Printed::atom(format!("{bt}[{i}]"))
            }
            Token::DynArrayLength(base) => {
                let b = self.expr(base, Hint::default(), scope);
                let bt = if b.prec > PREC_ATOM { format!("({})", b.text) } else { b.text };
                Printed::atom(format!("{bt}.Length"))
            }
            Token::DynArrayAdd { base, count, .. } => {
                Printed::atom(format!("{}.Add({})", self.base_text(base, scope), self.expr_text(count, Hint::default(), scope)))
            }
            Token::DynArrayAddItem { base, item, .. } => {
                let h = Hint { enum_: self.inner_enum(base), bool_ctx: false };
                // The item is compiled against the element type: conversions are automatic.
                let item = self.strip_autocast_to(item, h);
                Printed::atom(format!("{}.AddItem({})", self.base_text(base, scope), self.expr_text(item, h, scope)))
            }
            Token::DynArrayRemoveItem { base, item, .. } => {
                let h = Hint { enum_: self.inner_enum(base), bool_ctx: false };
                // The item is compiled against the element type: conversions are automatic.
                let item = self.strip_autocast_to(item, h);
                Printed::atom(format!("{}.RemoveItem({})", self.base_text(base, scope), self.expr_text(item, h, scope)))
            }
            Token::DynArrayInsert { base, index, count, .. } => Printed::atom(format!(
                "{}.Insert({}, {})",
                self.base_text(base, scope),
                self.expr_text(index, Hint::default(), scope),
                self.expr_text(count, Hint::default(), scope)
            )),
            Token::DynArrayRemove { base, index, count, .. } => Printed::atom(format!(
                "{}.Remove({}, {})",
                self.base_text(base, scope),
                self.expr_text(index, Hint::default(), scope),
                self.expr_text(count, Hint::default(), scope)
            )),
            Token::DynArrayInsertItem { base, index, item, .. } => {
                let h = Hint { enum_: self.inner_enum(base), bool_ctx: false };
                let item = self.strip_autocast_to(item, h);
                Printed::atom(format!(
                    "{}.InsertItem({}, {})",
                    self.base_text(base, scope),
                    self.expr_text(index, Hint::default(), scope),
                    self.expr_text(item, h, scope)
                ))
            }
            Token::DynArrayFind { base, item, .. } => {
                let h = Hint { enum_: self.inner_enum(base), bool_ctx: false };
                // The item is compiled against the element type: conversions are automatic.
                let item = self.strip_autocast_to(item, h);
                Printed::atom(format!("{}.Find({})", self.base_text(base, scope), self.expr_text(item, h, scope)))
            }
            Token::DynArrayFindStruct { base, member, item, .. } => Printed::atom(format!(
                "{}.Find({}, {})",
                self.base_text(base, scope),
                self.expr_text(member, Hint::default(), scope),
                self.expr_text(item, Hint::default(), scope)
            )),
            Token::DynArraySort { base, delegate, .. } => Printed::atom(format!(
                "{}.Sort({})",
                self.base_text(base, scope),
                self.expr_text(delegate, Hint::default(), scope)
            )),
            Token::VirtualFunction { name, args } => {
                let n = self.pkgname(*name);
                let f = self.ctx.find_function(scope.class, scope.state, &n);
                let params = f.map(|f| self.ctx.function_params(f)).unwrap_or_default();
                Printed::atom(format!("{n}({})", self.print_args(args, &params, scope)))
            }
            Token::GlobalFunction { name, args } => {
                let n = self.pkgname(*name);
                let f = self.ctx.find_function(scope.class, None, &n);
                let params = f.map(|f| self.ctx.function_params(f)).unwrap_or_default();
                Printed::atom(format!("Global.{n}({})", self.print_args(args, &params, scope)))
            }
            Token::FinalFunction { func, args } => match self.res(*func) {
                Some(f) => {
                    let p = self.print_call(f, args, scope, None);
                    if p.prec == PREC_ATOM {
                        let prefix = self.super_prefix(f, scope);
                        Printed::atom(format!("{prefix}{}", p.text))
                    } else {
                        p
                    }
                }
                None => Printed::atom(format!("{}({})", self.ref_name(*func), self.print_args(args, &[], scope))),
            },
            Token::NativeFunction { index, args } => match self.ctx.native_function(*index) {
                Some(f) => self.print_call(f, args, scope, None),
                None => Printed::atom(format!("__native{index}({})", self.print_args(args, &[], scope))),
            },
            Token::DelegateFunction { name, args, prop, .. } => {
                // Calls through a delegate variable use the variable's name.
                let fname = self.pkgname(*name);
                let pname = self.res(*prop).map(|p| self.ctx.name(p)).unwrap_or_default();
                let n = if pname == format!("__{fname}__Delegate") || pname.is_empty() { fname } else { pname };
                let params = self
                    .res(*prop)
                    .and_then(|p| self.ctx.property(p).and_then(|(_, d)| d.ref1))
                    .and_then(|f| self.ctx.resolve(self.res(*prop).unwrap().pkg, f))
                    .map(|f| self.ctx.function_params(f))
                    .unwrap_or_default();
                Printed::atom(format!("{n}({})", self.print_args(args, &params, scope)))
            }
            Token::DelegateProperty { name, .. } => Printed::atom(self.pkgname(*name)),
            Token::InstanceDelegate(name) => Printed::atom(self.pkgname(*name)),
            Token::DelegateCmp { op, args } => {
                let a: Vec<&Token> = args.args.iter().collect();
                let eq = matches!(op, 0x3b | 0x3d);
                let (l, r) = if a.len() == 2 {
                    (self.expr(a[0], Hint::default(), scope), self.expr(a[1], Hint::default(), scope))
                } else {
                    return Printed::atom("/*bad delegate cmp*/".to_string());
                };
                let prec = 24;
                let lt = if l.prec > prec { format!("({})", l.text) } else { l.text };
                let rt = if r.prec >= prec { format!("({})", r.text) } else { r.text };
                Printed { text: format!("{lt} {} {rt}", if eq { "==" } else { "!=" }), prec }
            }
            Token::EmptyDelegate => Printed::atom("None".to_string()),
            Token::StructCmpEq { a, b, .. } | Token::StructCmpNe { a, b, .. } => {
                let eq = matches!(t, Token::StructCmpEq { .. });
                let l = self.expr(a, Hint::default(), scope);
                let r = self.expr(b, Hint::default(), scope);
                let prec = 24;
                let lt = if l.prec > prec { format!("({})", l.text) } else { l.text };
                let rt = if r.prec >= prec { format!("({})", r.text) } else { r.text };
                Printed { text: format!("{lt} {} {rt}", if eq { "==" } else { "!=" }), prec }
            }
            Token::IntConst(v) => Printed::atom(v.to_string()),
            Token::IntConstByte(v) => Printed::atom(v.to_string()),
            Token::IntZero => Printed::atom("0".to_string()),
            Token::IntOne => Printed::atom("1".to_string()),
            Token::FloatConst(v) => Printed::atom(format_float(*v)),
            Token::StringConst(b) => Printed::atom(escape_string(b)),
            Token::UnicodeStringConst(u) => Printed::atom(escape_utf16(u)),
            Token::ObjectConst(o) => Printed::atom(self.object_literal(*o)),
            Token::NameConst(n) => Printed::atom(name_literal(&self.pkgname(*n))),
            Token::RotationConst(a, b, c) => Printed::atom(format!("rot({a}, {b}, {c})")),
            Token::VectorConst(a, b, c) => {
                Printed::atom(format!("vect({}, {}, {})", format_float(*a), format_float(*b), format_float(*c)))
            }
            Token::ByteConst(v) => {
                if let Some(e) = hint.enum_ {
                    if let Some(tag) = self.enum_tag(e, *v) {
                        return Printed::atom(tag);
                    }
                }
                Printed::atom(v.to_string())
            }
            Token::True => Printed::atom("true".to_string()),
            Token::False => Printed::atom("false".to_string()),
            Token::NoObject => Printed::atom("None".to_string()),
            Token::SelfRef | Token::SelfSafeObject => Printed::atom("self".to_string()),
            Token::Nothing => Printed::atom(String::new()),
            Token::EmptyParmValue => Printed::atom(String::new()),
            Token::PrimitiveCast { cast: c, expr } => {
                // `ByteToInt` of a byte constant: an enum tag (the context's enum, else a
                // `_MAX` tag used as a count).
                if *c == cast::BYTE_TO_INT {
                    if let Token::ByteConst(v) = **expr {
                        if let Some(tag) = hint.enum_.and_then(|e| self.enum_tag(e, v)) {
                            return Printed::atom(tag);
                        }
                        if let Some(tag) = self.max_tag_for(v) {
                            return Printed::atom(tag);
                        }
                    }
                }
                let keep_enum = matches!(*c, cast::BYTE_TO_INT | cast::BYTE_TO_FLOAT);
                let inner_hint = Hint { enum_: if keep_enum { hint.enum_ } else { None }, bool_ctx: false };
                let inner = self.expr(expr, inner_hint, scope);
                // A conversion to an enum-typed byte was written as `EnumName(expr)`;
                // to a plain byte it is automatic (truncating) and was not written at all.
                if matches!(*c, cast::INT_TO_BYTE | cast::FLOAT_TO_BYTE) {
                    if let Some(e) = hint.enum_ {
                        return Printed::atom(format!("{}({})", self.ctx.name(e), inner.text));
                    }
                }
                let ty = cast_type_name(*c);
                // No conversion to bool is automatic (`GConversions`), so `bool(x)` was explicit.
                match ty {
                    None => inner,
                    Some(ty) => Printed::atom(format!("{ty}({})", inner.text)),
                }
            }
            Token::DynamicCast { class, expr } | Token::InterfaceCast { class, expr } => {
                // Interface → class casts go through an implicit InterfaceToObject conversion.
                let operand: &Token = match &**expr {
                    Token::PrimitiveCast { cast: c, expr: e } if *c == cast::INTERFACE_TO_OBJECT => e,
                    e => e,
                };
                let inner = self.expr_text(operand, Hint::default(), scope);
                Printed::atom(format!("{}({inner})", self.ref_name(*class)))
            }
            Token::MetaCast { class, expr } => {
                let inner = self.expr_text(expr, Hint::default(), scope);
                Printed::atom(format!("class<{}>({inner})", self.ref_name(*class)))
            }
            Token::Conditional { cond, a, b, .. } => {
                let c = self.expr(cond, Hint { enum_: None, bool_ctx: true }, scope);
                let x = self.expr(a, hint, scope);
                let y = self.expr(b, hint, scope);
                // The compiler consumes `? :` right after the operand it just parsed, so
                // anything but an atom as the condition needs parentheses.
                let ct = if c.prec > PREC_ATOM { format!("({})", c.text) } else { c.text };
                let xt = if x.prec >= PREC_TERNARY { format!("({})", x.text) } else { x.text };
                let yt = if y.prec >= PREC_TERNARY { format!("({})", y.text) } else { y.text };
                Printed { text: format!("{ct} ? {xt} : {yt}"), prec: PREC_TERNARY }
            }
            Token::New { outer, name, flags, class, template } => {
                let mut inner: Vec<String> = Vec::new();
                for e in [outer, name, flags] {
                    inner.push(self.expr_text(e, Hint::default(), scope));
                }
                while inner.last().map(|s| s.is_empty()).unwrap_or(false) {
                    inner.pop();
                }
                let c = self.expr(class, Hint::default(), scope);
                let mut ct = if c.prec > PREC_ATOM { format!("({})", c.text) } else { c.text };
                if !matches!(**template, Token::Nothing) {
                    ct.push_str(&format!("({})", self.expr_text(template, Hint::default(), scope)));
                }
                if inner.is_empty() {
                    Printed { text: format!("new {ct}"), prec: PREC_UNARY }
                } else {
                    Printed { text: format!("new({}) {ct}", inner.join(", ")), prec: PREC_UNARY }
                }
            }
            Token::Let(a, b) | Token::LetBool(a, b) | Token::LetDelegate(a, b) => {
                let h = Hint { enum_: self.expr_enum(a), bool_ctx: false };
                let lhs = match self.null_var_text(a, self.expr_property(b), scope) {
                    Some(t) => t,
                    None => self.expr_text(a, Hint::default(), scope),
                };
                let rhs = match self.null_var_text(b, self.expr_property(a), scope) {
                    Some(t) => t,
                    None => self.expr_text(self.strip_autocast_to(b, h), h, scope),
                };
                Printed { text: format!("{lhs} = {rhs}"), prec: PREC_TERNARY }
            }
            Token::DefaultParmValue { expr, .. } => self.expr(self.strip_autocast_to(expr, hint), hint, scope),
            Token::Return(e) => Printed::atom(format!("return {}", self.expr_text(self.strip_autocast_to(e, hint), hint, scope))),
            Token::ReturnNothing(_) => Printed::atom(String::new()),
            Token::EatReturnValue(_) => Printed::atom(String::new()),
            Token::GotoLabel(e) => Printed::atom(format!("goto {}", self.expr_text(e, Hint::default(), scope))),
            Token::Stop => Printed::atom("stop".to_string()),
            Token::Assert { expr, .. } => {
                Printed::atom(format!("assert({})", self.expr_text(expr, Hint { enum_: None, bool_ctx: true }, scope)))
            }
            Token::Iterator { expr, .. } => self.expr(expr, hint, scope),
            Token::DynArrayIterator { array, item, has_index, index, .. } => {
                let mut s = format!("{}({}", self.base_text(array, scope), self.expr_text(item, Hint::default(), scope));
                if *has_index != 0 {
                    s.push_str(", ");
                    s.push_str(&self.expr_text(index, Hint::default(), scope));
                }
                s.push(')');
                Printed::atom(s)
            }
            Token::EndFunctionParms
            | Token::EndParmValue
            | Token::EndOfScript
            | Token::IteratorPop
            | Token::IteratorNext
            | Token::DebugInfo { .. }
            | Token::LabelTable(_)
            | Token::Jump { .. }
            | Token::JumpIfNot { .. }
            | Token::JumpIfFilterEditorOnly { .. }
            | Token::Switch { .. }
            | Token::Case { .. } => Printed::atom(format!("/* {t:?} */")),
        }
    }

    fn base_text(&self, base: &Token, scope: Scope) -> String {
        let b = self.expr(base, Hint::default(), scope);
        if b.prec > PREC_ATOM {
            format!("({})", b.text)
        } else {
            b.text
        }
    }

    fn inner_enum(&self, base: &Token) -> Option<ObjRef> {
        let arr = self.expr_property(base)?;
        let (k, d) = self.ctx.property(arr)?;
        if k != PropertyKind::Array {
            return None;
        }
        let inner = self.ctx.resolve(arr.pkg, d.ref1?)?;
        self.ctx.enum_of_property(inner)
    }

    /// Member expression inside a context: bare names, `default.X`, calls.
    /// `Outer`, `Outer.Outer`, ... as the compiler emits them for implicit access
    /// through `within` classes (`EmitOuterContext`: fixed skip of 9).
    fn is_implicit_outer_chain(&self, t: &Token) -> bool {
        let is_outer_prop = |p: PackageIndex| {
            self.ref_name(p).eq_ignore_ascii_case("Outer")
                && self.res(p).map(|r| self.ctx.owner_class(r).map(|c| self.ctx.name(c).eq_ignore_ascii_case("Object")).unwrap_or(false)).unwrap_or(false)
        };
        match t {
            Token::InstanceVariable(p) => is_outer_prop(*p),
            Token::Context { kind: ContextKind::Object, object, skip: 9, member, .. } => {
                matches!(&**member, Token::InstanceVariable(p) if is_outer_prop(*p)) && self.is_implicit_outer_chain(object)
            }
            _ => false,
        }
    }

    /// Canonical text of a literal token used as a context member, if it is one.
    fn literal_text(&self, t: &Token) -> Option<String> {
        Some(match t {
            Token::IntConst(v) => v.to_string(),
            Token::IntConstByte(v) => v.to_string(),
            Token::IntZero => "0".into(),
            Token::IntOne => "1".into(),
            Token::True => "true".into(),
            Token::False => "false".into(),
            Token::FloatConst(f) => format!("{f}"),
            Token::StringConst(b) => format!("\"{}\"", String::from_utf8_lossy(b)),
            Token::NameConst(n) => format!("'{}'", self.pkgname(*n)),
            _ => return None,
        })
    }

    /// Class of an object-valued expression, from its property or function return type.
    fn expr_object_class(&self, t: &Token) -> Option<ObjRef> {
        let p = self.expr_property(t)?;
        let (k, d) = self.ctx.property(p)?;
        match k {
            PropertyKind::Object | PropertyKind::Component | PropertyKind::SafeObject | PropertyKind::Interface => self.ctx.resolve(p.pkg, d.ref1?),
            _ => None,
        }
    }

    /// A `const` declared in `class` (or a parent) whose value equals `lit`.
    fn const_named_for(&self, class: ObjRef, lit: &str) -> Option<String> {
        let same = |value: &str| -> bool {
            let v = value.trim();
            if v == lit || v.eq_ignore_ascii_case(lit) {
                return true;
            }
            let parse_int = |x: &str| -> Option<i64> {
                let x = x.trim();
                if let Some(h) = x.strip_prefix("0x").or_else(|| x.strip_prefix("0X")) {
                    i64::from_str_radix(h, 16).ok()
                } else {
                    x.parse::<i64>().ok()
                }
            };
            if let (Some(a), Some(b)) = (parse_int(v), parse_int(lit)) {
                return a == b;
            }
            let parse_f = |x: &str| -> Option<f32> { x.trim().trim_end_matches(['f', 'F']).parse::<f32>().ok() };
            if let (Some(a), Some(b)) = (parse_f(v), parse_f(lit)) {
                return a == b && (v.contains('.') || lit.contains('.'));
            }
            false
        };
        let mut cur = Some(class);
        let mut guard = 0;
        while let Some(c) = cur {
            for ch in self.ctx.children(c) {
                if self.ctx.class_name(ch) == "Const" {
                    if let Some(ObjectBody::Const(d)) = self.ctx.object(ch).map(|o| &o.body) {
                        if same(&d.value) {
                            return Some(self.ctx.name(ch));
                        }
                    }
                }
            }
            cur = self.ctx.super_of(c);
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        None
    }

    fn member_text(&self, member: &Token, hint: Hint, scope: Scope) -> String {
        match member {
            Token::FinalFunction { func, args } => match self.res(*func) {
                Some(f) => {
                    let params = self.ctx.function_params(f);
                    format!("{}({})", self.ctx.name(f), self.print_args(args, &params, scope))
                }
                None => self.expr_text(member, hint, scope),
            },
            _ => self.expr_text(member, hint, scope),
        }
    }
}

/// Target type name of a primitive cast, or `None` when the cast is
/// implicit in source (interface/safe-object conversions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperandKind {
    Byte,
    Int,
    Float,
    Other,
}

pub fn cast_type_name(c: u8) -> Option<&'static str> {
    Some(match c {
        cast::INTERFACE_TO_OBJECT => "Object",
        cast::INTERFACE_TO_STRING | cast::BYTE_TO_STRING | cast::INT_TO_STRING | cast::BOOL_TO_STRING
        | cast::FLOAT_TO_STRING | cast::OBJECT_TO_STRING | cast::NAME_TO_STRING | cast::VECTOR_TO_STRING
        | cast::ROTATOR_TO_STRING | cast::DELEGATE_TO_STRING | cast::SAFE_OBJECT_TO_STRING => "string",
        cast::INTERFACE_TO_BOOL | cast::BYTE_TO_BOOL | cast::INT_TO_BOOL | cast::FLOAT_TO_BOOL
        | cast::OBJECT_TO_BOOL | cast::NAME_TO_BOOL | cast::STRING_TO_BOOL | cast::VECTOR_TO_BOOL
        | cast::ROTATOR_TO_BOOL | cast::SAFE_OBJECT_TO_BOOL => "bool",
        cast::ROTATOR_TO_VECTOR | cast::STRING_TO_VECTOR => "vector",
        cast::BYTE_TO_INT | cast::BOOL_TO_INT | cast::FLOAT_TO_INT | cast::STRING_TO_INT => "int",
        cast::BYTE_TO_FLOAT | cast::INT_TO_FLOAT | cast::BOOL_TO_FLOAT | cast::STRING_TO_FLOAT => "float",
        cast::INT_TO_BYTE | cast::BOOL_TO_BYTE | cast::FLOAT_TO_BYTE | cast::STRING_TO_BYTE => "byte",
        cast::STRING_TO_ROTATOR | cast::VECTOR_TO_ROTATOR => "rotator",
        cast::STRING_TO_NAME => "name",
        cast::SAFE_OBJECT_TO_OBJECT => "Object",
        cast::OBJECT_TO_INTERFACE | cast::SAFE_OBJECT_TO_INTERFACE | cast::OBJECT_TO_SAFE_OBJECT
        | cast::INTERFACE_TO_SAFE_OBJECT => return None,
        _ => return None,
    })
}
