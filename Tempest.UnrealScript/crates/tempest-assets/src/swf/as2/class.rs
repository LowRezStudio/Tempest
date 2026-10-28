//! AS2 `class` / `interface` syntax over the statements the Flash compiler
//! emits for them in `DoInitAction` tags:
//!
//! ```text
//! if (!_global.a) { _global.a = new Object(); }          // each package level
//! if (!_global.a.B) {
//!     a.B = _loc1_ = function (…) { constructor };
//!     __extends(a.B, Base);
//!     var _loc2_ = _loc1_.prototype;
//!     _loc2_.method = function (…) { … };                  // members, in order
//!     _loc1_.staticMethod = function (…) { … };
//!     _loc2_.__get__x = function () { … };                // function get x()
//!     _loc1_.CONST = 1;                                    // static var CONST = 1
//!     _loc2_.field = 0;                                    // var field = 0
//!     _loc2_.addProperty("x", _loc2_.__get__x, function () {});
//!     ASSetPropFlags(a.B.prototype, null, 1);
//! }
//! ```
//!
//! `lower` turns a class into exactly those statements (members in the order
//! written, `addProperty` for each property in name order, as the Flash
//! compiler registers them); the decompiler only shows a class when lowering it gives back the
//! statements it found (`raise`), so the class form never changes what runs.
//! A constructor runs what it says: write `super()` when the base needs it.

use super::ast::{Expr, Func, Stmt, Un};
use super::parse::Parser;
use tempest_upk::io::{Error, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct Class {
    pub path: Vec<String>,
    pub interface: bool,
    pub extends: Option<Expr>,
    pub implements: Vec<Expr>,
    pub ctor: Func,
    pub members: Vec<Member>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Method,
    Get,
    Set,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Member {
    Function { is_static: bool, kind: Kind, name: String, func: Func },
    Var { is_static: bool, name: String, value: Option<Expr> },
}

/// The registers the Flash compiler keeps the class and its prototype in.
const CLASS_REG: &str = "_loc1_";
const PROTO_REG: &str = "_loc2_";

fn empty_fn() -> Expr {
    Expr::Function(Box::new(Func { name: String::new(), params: Vec::new(), body: Vec::new(), raw: None }))
}

/// `a.b.C` as member accesses, from `_global` or from the first name.
fn path_expr(path: &[String], global: bool) -> Expr {
    let mut e = if global { Expr::member(Expr::var("_global"), &path[0]) } else { Expr::var(&path[0]) };
    for p in &path[1..] {
        e = Expr::member(e, p);
    }
    e
}

/// The `if (!_global.a.b) { _global.a.b = new Object(); }` of each package level.
fn packages(path: &[String]) -> Vec<Stmt> {
    (1..path.len())
        .map(|k| {
            let g = path_expr(&path[..k], true);
            Stmt::If(Expr::Unary(Un::Not, Box::new(g.clone())), vec![Stmt::Expr(Expr::Assign(Box::new(g), Box::new(Expr::New(Box::new(Expr::var("Object")), Vec::new()))))], None)
        })
        .collect()
}

fn accessor(kind: Kind, name: &str) -> String {
    match kind {
        Kind::Method => name.to_string(),
        Kind::Get => format!("__get__{name}"),
        Kind::Set => format!("__set__{name}"),
    }
}

/// The statements a class compiles to.
pub fn lower(c: &Class) -> Vec<Stmt> {
    let mut out = vec![Stmt::Var(vec![(CLASS_REG.to_string(), None)])];
    out.extend(packages(&c.path));
    let global = path_expr(&c.path, true);
    let cond = Expr::Unary(Un::Not, Box::new(global.clone()));
    if c.interface {
        out.push(Stmt::If(cond, vec![Stmt::Expr(Expr::Assign(Box::new(global), Box::new(empty_fn())))], None));
        return out;
    }
    // A class outside any package is reached through `_global`.
    let dotted = path_expr(&c.path, c.path.len() == 1);
    let class = Expr::var(CLASS_REG);
    let proto = Expr::var(PROTO_REG);
    let mut body = vec![Stmt::Expr(Expr::Assign(Box::new(dotted.clone()), Box::new(Expr::Assign(Box::new(class.clone()), Box::new(Expr::Function(Box::new(c.ctor.clone())))))))];
    if let Some(base) = &c.extends {
        body.push(Stmt::Expr(Expr::Intrinsic("__extends".into(), vec![dotted.clone(), base.clone()])));
    }
    if !c.implements.is_empty() {
        let mut args = vec![class.clone()];
        args.extend(c.implements.iter().cloned());
        body.push(Stmt::Expr(Expr::Intrinsic("__implements".into(), args)));
    }
    body.push(Stmt::Var(vec![(PROTO_REG.to_string(), Some(Expr::member(class.clone(), "prototype")))]));
    // Properties in the order their accessors first appear: (static, name, getter, setter).
    let mut props: Vec<(bool, String, bool, bool)> = Vec::new();
    for m in &c.members {
        match m {
            Member::Function { is_static, kind, name, func } => {
                let owner = if *is_static { &class } else { &proto };
                body.push(Stmt::Expr(Expr::Assign(Box::new(Expr::member(owner.clone(), &accessor(*kind, name))), Box::new(Expr::Function(Box::new(func.clone()))))));
                if *kind != Kind::Method {
                    match props.iter_mut().find(|p| p.0 == *is_static && p.1 == *name) {
                        Some(p) => {
                            p.2 |= *kind == Kind::Get;
                            p.3 |= *kind == Kind::Set;
                        }
                        None => props.push((*is_static, name.clone(), *kind == Kind::Get, *kind == Kind::Set)),
                    }
                }
            }
            Member::Var { is_static, name, value: Some(v) } => {
                let owner = if *is_static { &class } else { &proto };
                body.push(Stmt::Expr(Expr::Assign(Box::new(Expr::member(owner.clone(), name)), Box::new(v.clone()))));
            }
            Member::Var { value: None, .. } => {}
        }
    }
    // Registered in name order (bytewise), static and instance properties alike.
    props.sort_by(|a, b| a.1.as_bytes().cmp(b.1.as_bytes()));
    for (is_static, name, get, set) in props {
        let owner = if is_static { &class } else { &proto };
        let get = if get { Expr::member(owner.clone(), &accessor(Kind::Get, &name)) } else { empty_fn() };
        let set = if set { Expr::member(owner.clone(), &accessor(Kind::Set, &name)) } else { empty_fn() };
        body.push(Stmt::Expr(Expr::Call(Box::new(Expr::member(owner.clone(), "addProperty")), vec![Expr::Str(name), get, set])));
    }
    body.push(Stmt::Expr(Expr::Call(Box::new(Expr::var("ASSetPropFlags")), vec![Expr::member(dotted, "prototype"), Expr::Null, Expr::Int(1)])));
    out.push(Stmt::If(cond, body, None));
    out
}

/// Replace every class in the statements by what it compiles to.
pub fn lower_all(stmts: &[Stmt]) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            Stmt::Class(c) => out.extend(lower(c)),
            s => out.push(s.clone()),
        }
    }
    out
}

/// `!_global.a.b.C` → `[a, b, C]`.
fn global_path(e: &Expr) -> Option<Vec<String>> {
    let Expr::Unary(Un::Not, x) = e else { return None };
    let mut names = Vec::new();
    let mut cur = &**x;
    loop {
        match cur {
            Expr::Member(o, n) => {
                let Expr::Str(n) = &**n else { return None };
                names.push(n.clone());
                cur = o;
            }
            Expr::Var(v) if v == "_global" => break,
            _ => return None,
        }
    }
    names.reverse();
    (!names.is_empty()).then_some(names)
}

/// The class the statements of a class `if` define, before checking it lowers back to them.
fn read_class(path: &[String], body: &[Stmt]) -> Option<Class> {
    let class = Expr::var(CLASS_REG);
    let proto = Expr::var(PROTO_REG);
    let global = path_expr(path, true);
    if let [Stmt::Expr(Expr::Assign(t, f))] = body {
        if **t == global && matches!(&**f, Expr::Function(func) if func.body.is_empty() && func.params.is_empty() && func.raw.is_none()) {
            return Some(Class { path: path.to_vec(), interface: true, extends: None, implements: Vec::new(), ctor: Func { name: String::new(), params: Vec::new(), body: Vec::new(), raw: None }, members: Vec::new() });
        }
    }
    let mut it = body.iter().peekable();
    let ctor = match it.next()? {
        Stmt::Expr(Expr::Assign(_, v)) => match &**v {
            Expr::Assign(r, f) if **r == class => match &**f {
                Expr::Function(f) => (**f).clone(),
                _ => return None,
            },
            _ => return None,
        },
        _ => return None,
    };
    let mut c = Class { path: path.to_vec(), interface: false, extends: None, implements: Vec::new(), ctor, members: Vec::new() };
    if let Some(Stmt::Expr(Expr::Intrinsic(n, args))) = it.peek() {
        if n == "__extends" && args.len() == 2 {
            c.extends = Some(args[1].clone());
            it.next();
        }
    }
    if let Some(Stmt::Expr(Expr::Intrinsic(n, args))) = it.peek() {
        if n == "__implements" && !args.is_empty() {
            c.implements = args[1..].to_vec();
            it.next();
        }
    }
    it.next()?;
    for s in it {
        let Stmt::Expr(e) = s else { return None };
        match e {
            Expr::Assign(t, v) => {
                let Expr::Member(o, n) = &**t else { return None };
                let Expr::Str(name) = &**n else { return None };
                let is_static = if **o == class {
                    true
                } else if **o == proto {
                    false
                } else {
                    return None;
                };
                match &**v {
                    Expr::Function(f) => {
                        let (kind, name) = if let Some(n) = name.strip_prefix("__get__") {
                            (Kind::Get, n)
                        } else if let Some(n) = name.strip_prefix("__set__") {
                            (Kind::Set, n)
                        } else {
                            (Kind::Method, name.as_str())
                        };
                        c.members.push(Member::Function { is_static, kind, name: name.to_string(), func: (**f).clone() });
                    }
                    v => c.members.push(Member::Var { is_static, name: name.clone(), value: Some(v.clone()) }),
                }
            }
            // `addProperty` and `ASSetPropFlags` come back from `lower`.
            Expr::Call(..) => {}
            _ => return None,
        }
    }
    // Accessors without a property registration are ordinary methods named `__get__x`.
    Some(c)
}

/// Turn the class definitions among the statements of a script into
/// `class` statements, where they lower back to exactly what is there.
pub fn raise(stmts: &mut Vec<Stmt>) {
    let mut i = 0;
    let mut found = false;
    while i < stmts.len() {
        let Stmt::If(cond, body, None) = &stmts[i] else {
            i += 1;
            continue;
        };
        let Some(path) = global_path(cond) else {
            i += 1;
            continue;
        };
        let levels = path.len() - 1;
        if i < levels {
            i += 1;
            continue;
        }
        let start = i - levels;
        let Some(class) = read_class(&path, body) else {
            i += 1;
            continue;
        };
        let lowered = lower(&class);
        // `lower` starts with `var _loc1_;`, which the decompiler wrote at the top of the script.
        // Compared as text: `NaN` initialisers are not equal to themselves.
        if format!("{:?}", &lowered[1..]) == format!("{:?}", &stmts[start..=i]) {
            stmts.splice(start..=i, [Stmt::Class(Box::new(class))]);
            found = true;
            i = start + 1;
        } else {
            i += 1;
        }
    }
    if found {
        // The class register's declaration is part of the class now.
        if let Some(Stmt::Var(v)) = stmts.first_mut() {
            if v.iter().all(|(_, init)| init.is_none()) {
                v.retain(|(n, _)| n != CLASS_REG);
                if v.is_empty() {
                    stmts.remove(0);
                }
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Parsing
// ----------------------------------------------------------------------------

fn perr<T>(line: usize, msg: impl std::fmt::Display) -> Result<T> {
    Err(Error::Format(format!("line {line}: {msg}")))
}

fn dotted_name(p: &mut Parser) -> Result<Vec<String>> {
    let mut path = vec![p.name()?];
    while p.take_p(".") {
        path.push(p.name()?);
    }
    Ok(path)
}

fn type_expr(path: &[String]) -> Expr {
    path_expr(path, false)
}

/// `class a.B extends C implements D { … }`, `interface a.I { … }`, and
/// `intrinsic` / `dynamic` declarations (intrinsic ones declare types only).
pub(super) fn parse_class(p: &mut Parser) -> Result<Vec<Stmt>> {
    let line = p.line_no();
    let intrinsic = p.take_kw("intrinsic");
    p.take_kw("dynamic");
    let interface = if p.take_kw("interface") {
        true
    } else if p.take_kw("class") {
        false
    } else {
        return perr(line, "expected class or interface");
    };
    let path = dotted_name(p)?;
    let mut extends = None;
    let mut implements = Vec::new();
    if p.take_kw("extends") {
        extends = Some(type_expr(&dotted_name(p)?));
        // Interfaces may extend several.
        while interface && p.take_p(",") {
            dotted_name(p)?;
        }
    }
    if p.take_kw("implements") {
        loop {
            implements.push(type_expr(&dotted_name(p)?));
            if !p.take_p(",") {
                break;
            }
        }
    }
    p.need("{")?;
    let short = path.last().unwrap().clone();
    let mut ctor = None;
    let mut members = Vec::new();
    while !p.take_p("}") {
        if p.at_end() {
            return perr(p.line_no(), format!("class {} is missing its }}", path.join(".")));
        }
        let mut is_static = false;
        loop {
            if p.take_kw("static") {
                is_static = true;
            } else if !(p.take_kw("public") || p.take_kw("private")) {
                break;
            }
        }
        let mline = p.line_no();
        if p.take_kw("var") {
            loop {
                let name = p.name()?;
                p.ty()?;
                let value = if p.take_p("=") { Some(p.assign()?) } else { None };
                members.push(Member::Var { is_static, name, value });
                if !p.take_p(",") {
                    break;
                }
            }
            p.end_stmt()?;
        } else if p.take_kw("function") {
            let mut kind = Kind::Method;
            let mut name = p.name()?;
            if (name == "get" || name == "set") && !p.at_p("(") {
                kind = if name == "get" { Kind::Get } else { Kind::Set };
                name = p.name()?;
            }
            if interface || intrinsic {
                // A declaration: parameters and a type, no body.
                p.need("(")?;
                while !p.take_p(")") {
                    p.name()?;
                    p.ty()?;
                    p.take_p(",");
                }
                p.ty()?;
                p.end_stmt()?;
                continue;
            }
            let func = p.function_rest(String::new())?;
            if name == short && !is_static && kind == Kind::Method {
                if ctor.is_some() {
                    return perr(mline, format!("{short} has two constructors"));
                }
                ctor = Some(func);
            } else {
                members.push(Member::Function { is_static, kind, name, func });
            }
        } else {
            return perr(mline, "expected var or function in the class");
        }
    }
    if intrinsic {
        return Ok(Vec::new());
    }
    let ctor = ctor.unwrap_or(Func { name: String::new(), params: Vec::new(), body: Vec::new(), raw: None });
    Ok(vec![Stmt::Class(Box::new(Class { path, interface, extends, implements, ctor, members }))])
}
