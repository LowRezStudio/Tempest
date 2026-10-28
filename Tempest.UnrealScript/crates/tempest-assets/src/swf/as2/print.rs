//! ActionScript 2 source text from the syntax tree.

use super::ast::{is_ident, is_reserved, Bin, Expr, Func, Stmt, Un};
use super::pcode::{fmt_flags, quote};

const IND: &str = "    ";

pub fn program(body: &[Stmt]) -> String {
    let mut p = Printer { out: String::new(), depth: 0 };
    for s in body {
        p.stmt(s);
    }
    p.out
}

pub fn number(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else if n == 0.0 && n.is_sign_negative() {
        "-0".into()
    } else if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{n:.0}")
    } else {
        let s = format!("{n:?}");
        s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
    }
}

struct Printer {
    out: String,
    depth: usize,
}

/// Does the printed expression start with `function` or `{` (which would read as a declaration or block)?
fn starts_ambiguous(e: &Expr) -> bool {
    match e {
        Expr::Function(_) | Expr::Object(_) => true,
        Expr::Member(o, _) | Expr::Call(o, _) | Expr::Binary(_, o, _) | Expr::And(o, _) | Expr::Or(o, _) | Expr::Cond(o, _, _) | Expr::Assign(o, _) | Expr::PostInc(o, _) => starts_ambiguous(o),
        _ => false,
    }
}

impl Printer {
    fn line(&mut self, s: &str) {
        for _ in 0..self.depth {
            self.out.push_str(IND);
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// Text whose first line starts here and whose other lines are indented already (a function).
    fn text(&mut self, t: &str, prefix: &str) {
        let mut lines = t.lines();
        if let Some(first) = lines.next() {
            self.line(&format!("{prefix}{first}"));
        }
        for l in lines {
            self.out.push_str(l);
            self.out.push('\n');
        }
    }

    fn body(&mut self, head: &str, body: &[Stmt], tail: &str) {
        self.line(&format!("{head}{{"));
        self.depth += 1;
        for s in body {
            self.stmt(s);
        }
        self.depth -= 1;
        self.line(&format!("}}{tail}"));
    }

    fn simple(&mut self, s: &Stmt) -> String {
        match s {
            Stmt::Expr(e) => self.expr(e, 0),
            Stmt::Var(vars) => format!("var {}", self.vars(vars)),
            _ => String::new(),
        }
    }

    fn vars(&mut self, vars: &[(String, Option<Expr>)]) -> String {
        vars.iter()
            .map(|(n, v)| match v {
                Some(v) => format!("{n} = {}", self.expr(v, 3)),
                None => n.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) => {
                let t = self.expr(e, 0);
                if starts_ambiguous(e) {
                    self.line(&format!("({t});"));
                } else {
                    self.line(&format!("{t};"));
                }
            }
            Stmt::Var(vars) => {
                let t = self.vars(vars);
                self.line(&format!("var {t};"));
            }
            Stmt::If(c, then, els) => {
                let mut head = format!("if ({}) ", self.expr(c, 0));
                let mut then = then;
                let mut els = els.as_ref();
                loop {
                    self.line(&format!("{head}{{"));
                    self.depth += 1;
                    for s in then {
                        self.stmt(s);
                    }
                    self.depth -= 1;
                    match els {
                        None => {
                            self.line("}");
                            break;
                        }
                        Some(e) => {
                            if let [Stmt::If(c2, t2, e2)] = e.as_slice() {
                                head = format!("}} else if ({}) ", self.expr(c2, 0));
                                then = t2;
                                els = e2.as_ref();
                                // `line` prints head with a `{`; drop the closing brace printed separately.
                                continue;
                            }
                            self.line("} else {");
                            self.depth += 1;
                            for s in e {
                                self.stmt(s);
                            }
                            self.depth -= 1;
                            self.line("}");
                            break;
                        }
                    }
                }
            }
            Stmt::While(c, body) => {
                let h = format!("while ({}) ", self.expr(c, 0));
                self.body(&h, body, "");
            }
            Stmt::DoWhile(body, c) => {
                let t = format!(" while ({});", self.expr(c, 0));
                self.body("do ", body, &t);
            }
            Stmt::For(init, cond, step, body) => {
                let i = init.as_ref().map(|s| self.simple(s)).unwrap_or_default();
                let c = cond.as_ref().map(|c| self.expr(c, 0)).unwrap_or_default();
                let st = step.as_ref().map(|s| self.simple(s)).unwrap_or_default();
                self.body(&format!("for ({i}; {c}; {st}) "), body, "");
            }
            Stmt::ForIn(target, declare, obj, body) => {
                let t = self.expr(target, 17);
                let o = self.expr(obj, 0);
                self.body(&format!("for ({}{t} in {o}) ", if *declare { "var " } else { "" }), body, "");
            }
            Stmt::Switch(x, clauses) => {
                let h = format!("switch ({}) {{", self.expr(x, 0));
                self.line(&h);
                self.depth += 1;
                for (c, body) in clauses {
                    match c {
                        Some(c) => {
                            let t = format!("case {}:", self.expr(c, 0));
                            self.line(&t);
                        }
                        None => self.line("default:"),
                    }
                    self.depth += 1;
                    for s in body {
                        self.stmt(s);
                    }
                    self.depth -= 1;
                }
                self.depth -= 1;
                self.line("}");
            }
            Stmt::Break => self.line("break;"),
            Stmt::Continue => self.line("continue;"),
            Stmt::Return(None) | Stmt::Return(Some(Expr::Undefined)) => self.line("return;"),
            Stmt::Return(Some(e)) => {
                let t = self.expr(e, 0);
                self.line(&format!("return {t};"));
            }
            Stmt::Throw(e) => {
                let t = self.expr(e, 0);
                self.line(&format!("throw {t};"));
            }
            Stmt::Try(body, catch, finally) => {
                self.line("try {");
                self.depth += 1;
                for s in body {
                    self.stmt(s);
                }
                self.depth -= 1;
                if let Some((name, c)) = catch {
                    self.line(&format!("}} catch ({name}) {{"));
                    self.depth += 1;
                    for s in c {
                        self.stmt(s);
                    }
                    self.depth -= 1;
                }
                if let Some(f) = finally {
                    self.line("} finally {");
                    self.depth += 1;
                    for s in f {
                        self.stmt(s);
                    }
                    self.depth -= 1;
                }
                self.line("}");
            }
            Stmt::With(o, body) => {
                let h = format!("with ({}) ", self.expr(o, 0));
                self.body(&h, body, "");
            }
            Stmt::Function(f) => {
                let t = self.function(f);
                self.text(&t, "");
            }
            Stmt::Block(body) => self.body("", body, ""),
            Stmt::Class(c) => self.class(c),
            Stmt::Raw(actions) => {
                self.line("__pcode {");
                let t = super::pcode::print_body(actions, self.depth + 1, IND);
                self.out.push_str(&t);
                self.line("}");
            }
        }
    }

    fn class(&mut self, c: &super::class::Class) {
        use super::class::{Kind, Member};
        let name = c.path.join(".");
        if c.interface {
            self.line(&format!("interface {name} {{"));
            self.line("}");
            return;
        }
        let mut head = format!("class {name}");
        if let Some(b) = &c.extends {
            head.push_str(&format!(" extends {}", self.expr(b, 19)));
        }
        if !c.implements.is_empty() {
            let list: Vec<String> = c.implements.iter().map(|i| self.expr(i, 19)).collect();
            head.push_str(&format!(" implements {}", list.join(", ")));
        }
        self.line(&format!("{head} {{"));
        self.depth += 1;
        let mut ctor = c.ctor.clone();
        ctor.name = c.path.last().cloned().unwrap_or_default();
        let t = self.function(&ctor);
        self.text(&t, "");
        for m in &c.members {
            match m {
                Member::Var { is_static, name, value } => {
                    let st = if *is_static { "static " } else { "" };
                    let t = match value {
                        Some(v) => format!("{st}var {name} = {};", self.expr(v, 3)),
                        None => format!("{st}var {name};"),
                    };
                    self.line(&t);
                }
                Member::Function { is_static, kind, name, func } => {
                    let mut f = func.clone();
                    f.name = match kind {
                        Kind::Method => name.clone(),
                        Kind::Get => format!("get {name}"),
                        Kind::Set => format!("set {name}"),
                    };
                    let t = self.function(&f);
                    self.text(&t, if *is_static { "static " } else { "" });
                }
            }
        }
        self.depth -= 1;
        self.line("}");
    }

    /// A function, printed at the current depth; lines after the first are indented.
    fn function(&mut self, f: &Func) -> String {
        let mut head = format!("function {}({}) ", f.name, f.params.join(", "));
        if f.name.is_empty() {
            head = format!("function ({}) ", f.params.join(", "));
        }
        let mut sub = Printer { out: String::new(), depth: self.depth + 1 };
        match &f.raw {
            Some(raw) => {
                head.push_str("__pcode");
                if let Some((regs, flags)) = raw.v2 {
                    head.push_str(&format!("({regs}, \"{}\"", fmt_flags(flags)));
                    for r in &raw.param_regs {
                        head.push_str(&format!(", {r}"));
                    }
                    head.push(')');
                }
                head.push(' ');
                sub.out = super::pcode::print_body(&raw.actions, self.depth + 1, IND);
            }
            None => {
                for s in &f.body {
                    sub.stmt(s);
                }
            }
        }
        let mut out = format!("{head}{{\n");
        out.push_str(&sub.out);
        for _ in 0..self.depth {
            out.push_str(IND);
        }
        out.push('}');
        out
    }

    fn args(&mut self, args: &[Expr]) -> String {
        args.iter().map(|a| self.expr(a, 3)).collect::<Vec<_>>().join(", ")
    }

    fn member_name(&mut self, obj: &str, name: &Expr) -> String {
        match name {
            Expr::Str(s) if is_ident(s) => format!("{obj}.{s}"),
            n => format!("{obj}[{}]", self.expr(n, 0)),
        }
    }

    /// The expression, in parentheses when it binds looser than `prec`.
    fn expr(&mut self, e: &Expr, prec: u8) -> String {
        let (text, own) = self.expr_inner(e);
        if own < prec { format!("({text})") } else { text }
    }

    /// An operand of `.`, `[]` or a call.
    fn object(&mut self, e: &Expr) -> String {
        match e {
            Expr::Int(_) | Expr::Num(_) | Expr::Function(_) | Expr::New(..) => format!("({})", self.expr(e, 0)),
            e => self.expr(e, 19),
        }
    }

    fn expr_inner(&mut self, e: &Expr) -> (String, u8) {
        match e {
            Expr::Undefined => ("undefined".into(), 20),
            Expr::Null => ("null".into(), 20),
            Expr::Bool(b) => (b.to_string(), 20),
            Expr::Int(i) => (i.to_string(), if *i < 0 { 16 } else { 20 }),
            Expr::Num(n) => {
                let t = number(*n);
                let p = if t.starts_with('-') { 16 } else { 20 };
                (t, p)
            }
            Expr::Str(s) => (quote(s), 20),
            Expr::Var(n) => (n.clone(), 20),
            Expr::Eval(e) => (format!("eval({})", self.expr(e, 3)), 19),
            Expr::Member(o, n) => {
                let o = self.object(o);
                (self.member_name(&o, n), 19)
            }
            Expr::Call(f, args) => {
                let callee = match &**f {
                    // A user function sharing a built-in's name: call it by name so it is not compiled as the built-in.
                    Expr::Var(n) if super::ast::intrinsic(n).is_some() || is_reserved(n) => format!("eval({})", quote(n)),
                    f => self.object(f),
                };
                (format!("{callee}({})", self.args(args)), 19)
            }
            Expr::New(f, args) => {
                let callee = match &**f {
                    Expr::Var(_) | Expr::Member(..) if !contains_call(f) => self.expr(f, 19),
                    f => format!("({})", self.expr(f, 0)),
                };
                (format!("new {callee}({})", self.args(args)), 19)
            }
            Expr::Unary(op, x) => {
                let x = self.expr(x, 16);
                (
                    match op {
                        Un::Not => format!("!{x}"),
                        Un::Neg => {
                            if x.starts_with('-') {
                                format!("- {x}")
                            } else {
                                format!("-{x}")
                            }
                        }
                        Un::BitNot => format!("~{x}"),
                        Un::TypeOf => format!("typeof {x}"),
                    },
                    16,
                )
            }
            Expr::Binary(op, a, b) => {
                let p = op.prec();
                let a = self.expr(a, p);
                let b = self.expr(b, p + 1);
                (format!("{a} {} {b}", op.text()), p)
            }
            Expr::And(a, b) => {
                let a = self.expr(a, 6);
                let b = self.expr(b, 7);
                (format!("{a} && {b}"), 6)
            }
            Expr::Or(a, b) => {
                let a = self.expr(a, 5);
                let b = self.expr(b, 6);
                (format!("{a} || {b}"), 5)
            }
            Expr::Cond(c, a, b) => {
                let c = self.expr(c, 5);
                let a = self.expr(a, 3);
                let b = self.expr(b, 3);
                (format!("{c} ? {a} : {b}"), 4)
            }
            Expr::Assign(t, v) => {
                // `x = x + y` → `x += y` (the same tree once parsed).
                if let Expr::Binary(op, a, b) = &**v {
                    if **a == **t && op_assign(*op).is_some() && matches!(&**t, Expr::Var(_) | Expr::Member(..)) {
                        let t = self.expr(t, 17);
                        let b = self.expr(b, 3);
                        return (format!("{t} {} {b}", op_assign(*op).unwrap()), 3);
                    }
                }
                let t = self.expr(t, 17);
                let v = self.expr(v, 3);
                (format!("{t} = {v}"), 3)
            }
            Expr::PreInc(t, inc) => (format!("{}{}", if *inc { "++" } else { "--" }, self.expr(t, 17)), 16),
            Expr::PostInc(t, inc) => (format!("{}{}", self.expr(t, 17), if *inc { "++" } else { "--" }), 17),
            Expr::Array(items) => (format!("[{}]", self.args(items)), 20),
            Expr::Object(items) => {
                if items.is_empty() {
                    return ("{}".into(), 20);
                }
                let parts: Vec<String> = items
                    .iter()
                    .map(|(k, v)| {
                        let k = match k {
                            Expr::Str(s) if is_ident(s) => s.clone(),
                            Expr::Str(s) => quote(s),
                            k => self.expr(k, 20),
                        };
                        format!("{k}: {}", self.expr(v, 3))
                    })
                    .collect();
                (format!("{{{}}}", parts.join(", ")), 20)
            }
            Expr::Function(f) => (self.function(f), 20),
            Expr::Delete(t) => (format!("delete {}", self.expr(t, 16)), 16),
            Expr::Intrinsic(name, args) => (format!("{name}({})", self.args(args)), 19),
        }
    }
}

fn contains_call(e: &Expr) -> bool {
    match e {
        Expr::Call(..) => true,
        Expr::Member(o, _) => contains_call(o),
        _ => false,
    }
}

/// `Bin` for a compound assignment operator (`+=` → `Add`).
pub fn compound(op: &str) -> Option<Bin> {
    Some(match op {
        "+=" => Bin::Add,
        "-=" => Bin::Sub,
        "*=" => Bin::Mul,
        "/=" => Bin::Div,
        "%=" => Bin::Mod,
        "&=" => Bin::BitAnd,
        "|=" => Bin::BitOr,
        "^=" => Bin::BitXor,
        "<<=" => Bin::Shl,
        ">>=" => Bin::Shr,
        ">>>=" => Bin::UShr,
        _ => return None,
    })
}

pub fn op_assign(op: Bin) -> Option<&'static str> {
    Some(match op {
        Bin::Add => "+=",
        Bin::Sub => "-=",
        Bin::Mul => "*=",
        Bin::Div => "/=",
        Bin::Mod => "%=",
        Bin::BitAnd => "&=",
        Bin::BitOr => "|=",
        Bin::BitXor => "^=",
        Bin::Shl => "<<=",
        Bin::Shr => ">>=",
        Bin::UShr => ">>>=",
        _ => return None,
    })
}
