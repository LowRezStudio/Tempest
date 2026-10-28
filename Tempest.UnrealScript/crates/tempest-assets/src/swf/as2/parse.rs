//! ActionScript 2 source → syntax tree. ECMAScript 3 statements and
//! expressions with AS2 type annotations (`var x:Number`, `function f(a:String):Void`,
//! read and ignored), `class` definitions (see `class`), and `__pcode { … }`
//! blocks of P-code (`pcode`) as statements or function bodies.

use super::ast::{is_reserved, Bin, Expr, Func, RawBody, Stmt, Un};
use super::print::compound;
use tempest_upk::io::{Error, Result};

#[derive(Clone, Debug, PartialEq)]
enum T {
    Ident(String),
    Num(f64, bool),
    Str(String),
    P(&'static str),
    /// The text of a `__pcode { … }` block, and the line it starts on.
    Raw(String, usize),
    Eof,
}

#[derive(Clone, Debug)]
struct Tok {
    t: T,
    line: usize,
    /// A line break comes before it (for `return` / `break` and optional semicolons).
    nl: bool,
}

const PUNCT: [&str; 49] = [
    ">>>=", "===", "!==", ">>>", "<<=", ">>=", "...", "==", "!=", "<=", ">=", "&&", "||", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "{", "}", "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "?", ":", "=", ".",
];

fn err<T>(line: usize, msg: impl std::fmt::Display) -> Result<T> {
    Err(Error::Format(format!("line {line}: {msg}")))
}

fn lex(src: &str) -> Result<Vec<Tok>> {
    let c: Vec<char> = src.chars().collect();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    let mut line = 1;
    let mut nl = true;
    // After `__pcode`: the next `{` at parenthesis depth 0 opens a raw block.
    let mut raw_pending = false;
    let mut depth = 0i32;
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            nl = true;
            i += 1;
            continue;
        }
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                if c[i] == '\n' {
                    line += 1;
                    nl = true;
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        let start_line = line;
        let t = if raw_pending && depth == 0 && ch == '{' {
            // Raw P-code up to the `}` that closes it: function headers end their line with `{`, bodies close with a lone `}`.
            raw_pending = false;
            i += 1;
            let rest: String = c[i..].iter().collect();
            let mut level = 0;
            let mut taken = 0;
            let mut text = String::new();
            let mut closed = false;
            for l in rest.split_inclusive('\n') {
                let trimmed = l.trim();
                let code = trimmed.split(';').next().unwrap_or("").trim();
                if code == "}" {
                    if level == 0 {
                        taken += l.find('}').unwrap() + 1;
                        closed = true;
                        break;
                    }
                    level -= 1;
                } else if code.ends_with('{') && !code.starts_with('"') {
                    level += 1;
                }
                text.push_str(l);
                taken += l.len();
            }
            if !closed {
                return err(start_line, "__pcode block without its closing }");
            }
            let n = text.matches('\n').count();
            i += rest[..taken].chars().count();
            line += n;
            T::Raw(text, start_line)
        } else if ch == '"' || ch == '\'' {
            let (s, next) = super::pcode::unquote(&c, i).or_else(|e| err(line, e))?;
            i = next;
            T::Str(s)
        } else if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            if ch == '0' && matches!(c.get(i + 1), Some('x' | 'X')) {
                i += 2;
                while i < c.len() && c[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let s: String = c[start + 2..i].iter().collect();
                let v = u64::from_str_radix(&s, 16).or_else(|_| err(line, format!("bad number 0x{s}")))?;
                T::Num(v as f64, true)
            } else {
                let mut int = true;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                    if c[i] == '.' {
                        int = false;
                    }
                    i += 1;
                }
                if i < c.len() && (c[i] == 'e' || c[i] == 'E') {
                    int = false;
                    i += 1;
                    if i < c.len() && (c[i] == '+' || c[i] == '-') {
                        i += 1;
                    }
                    while i < c.len() && c[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let s: String = c[start..i].iter().collect();
                T::Num(s.parse().or_else(|_| err(line, format!("bad number {s}")))?, int)
            }
        } else if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '$') {
                i += 1;
            }
            let s: String = c[start..i].iter().collect();
            if s == "__pcode" {
                raw_pending = true;
                depth = 0;
            }
            T::Ident(s)
        } else {
            let rest: String = c[i..(i + 4).min(c.len())].iter().collect();
            let p = PUNCT.iter().find(|p| rest.starts_with(**p)).ok_or_else(|| Error::Format(format!("line {line}: unexpected character {ch:?}")))?;
            i += p.chars().count();
            if raw_pending {
                match *p {
                    "(" => depth += 1,
                    ")" => depth -= 1,
                    _ => {}
                }
            }
            T::P(p)
        };
        out.push(Tok { t, line: start_line, nl });
        nl = false;
    }
    out.push(Tok { t: T::Eof, line, nl: true });
    Ok(out)
}

pub fn parse(src: &str) -> Result<Vec<Stmt>> {
    let mut p = Parser { toks: lex(src)?, pos: 0, no_in: false };
    let mut out = Vec::new();
    while p.peek() != &T::Eof {
        out.extend(p.stmt_or_class()?);
    }
    Ok(out)
}

pub(super) struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    /// Inside a `for (…;` header: `in` is not an operator.
    no_in: bool,
}

impl Parser {
    fn peek(&self) -> &T {
        &self.toks[self.pos].t
    }
    fn peek_at(&self, n: usize) -> &T {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].t
    }
    fn line(&self) -> usize {
        self.toks[self.pos].line
    }
    fn nl(&self) -> bool {
        self.toks[self.pos].nl
    }
    fn next(&mut self) -> T {
        let t = self.toks[self.pos].t.clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }
    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), T::P(x) if *x == p)
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), T::Ident(x) if x == k)
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.next();
            true
        } else {
            false
        }
    }
    fn eat_kw(&mut self, k: &str) -> bool {
        if self.is_kw(k) {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, p: &str) -> Result<()> {
        if self.eat(p) {
            Ok(())
        } else {
            err(self.line(), format!("expected {p}, found {}", self.describe()))
        }
    }
    fn describe(&self) -> String {
        match self.peek() {
            T::Ident(s) => format!("{s:?}"),
            T::Num(n, _) => n.to_string(),
            T::Str(s) => super::pcode::quote(s),
            T::P(p) => format!("'{p}'"),
            T::Raw(..) => "a P-code block".into(),
            T::Eof => "the end".into(),
        }
    }
    fn ident(&mut self) -> Result<String> {
        match self.peek().clone() {
            T::Ident(s) if !is_reserved(&s) => {
                self.next();
                Ok(s)
            }
            _ => err(self.line(), format!("expected a name, found {}", self.describe())),
        }
    }
    /// A name after `.` (reserved words allowed).
    fn prop_name(&mut self) -> Result<String> {
        match self.next() {
            T::Ident(s) => Ok(s),
            _ => err(self.line(), "expected a property name"),
        }
    }
    /// The end of a statement: `;`, or nothing before `}`, a line break or the end.
    fn semi(&mut self) -> Result<()> {
        if self.eat(";") || self.is("}") || self.nl() || self.peek() == &T::Eof {
            return Ok(());
        }
        err(self.line(), format!("expected ;, found {}", self.describe()))
    }
    /// `: Type` (dotted, maybe `Array`), ignored.
    fn skip_type(&mut self) -> Result<()> {
        if self.eat(":") {
            self.prop_name()?;
            while self.eat(".") {
                self.prop_name()?;
            }
        }
        Ok(())
    }

    fn stmt_or_class(&mut self) -> Result<Vec<Stmt>> {
        if self.is_kw("class") || self.is_kw("interface") || (self.is_kw("dynamic") && matches!(self.peek_at(1), T::Ident(k) if k == "class") || (self.is_kw("intrinsic"))) {
            return super::class::parse_class(self);
        }
        if self.is_kw("import") {
            // `import a.b.C;` and `import a.b.*;`: names are written in full here.
            self.next();
            while !self.is(";") && self.peek() != &T::Eof && !self.nl() {
                self.next();
            }
            self.eat(";");
            return Ok(Vec::new());
        }
        Ok(vec![self.stmt()?])
    }

    fn block(&mut self) -> Result<Vec<Stmt>> {
        self.expect("{")?;
        let mut out = Vec::new();
        while !self.is("}") {
            if self.peek() == &T::Eof {
                return err(self.line(), "missing }");
            }
            out.push(self.stmt()?);
        }
        self.next();
        Ok(out)
    }

    /// A statement used as a body (`if (…) stmt`): blocks are flattened.
    fn body(&mut self) -> Result<Vec<Stmt>> {
        if self.is("{") {
            self.block()
        } else {
            Ok(vec![self.stmt()?])
        }
    }

    fn var_list(&mut self) -> Result<Vec<(String, Option<Expr>)>> {
        let mut vars = Vec::new();
        loop {
            let name = self.ident()?;
            self.skip_type()?;
            let init = if self.eat("=") { Some(self.assign()?) } else { None };
            vars.push((name, init));
            if !self.eat(",") {
                break;
            }
        }
        Ok(vars)
    }

    pub(super) fn stmt(&mut self) -> Result<Stmt> {
        let line = self.line();
        if let T::Raw(text, l) = self.peek().clone() {
            let _ = l;
            return err(line, format!("P-code block without __pcode ({} lines)", text.lines().count()));
        }
        if self.eat(";") {
            return Ok(Stmt::Block(Vec::new()));
        }
        if self.is("{") {
            return Ok(Stmt::Block(self.block()?));
        }
        let T::Ident(kw) = self.peek().clone() else {
            let e = self.expr()?;
            self.semi()?;
            return Ok(Stmt::Expr(e));
        };
        match kw.as_str() {
            "var" => {
                self.next();
                let v = self.var_list()?;
                self.semi()?;
                Ok(Stmt::Var(v))
            }
            "function" if matches!(self.peek_at(1), T::Ident(_)) => {
                self.next();
                let name = self.ident()?;
                Ok(Stmt::Function(Box::new(self.function_rest(name)?)))
            }
            "if" => {
                self.next();
                self.expect("(")?;
                let c = self.expr()?;
                self.expect(")")?;
                let then = self.body()?;
                let els = if self.eat_kw("else") { Some(self.body()?) } else { None };
                Ok(Stmt::If(c, then, els))
            }
            "while" => {
                self.next();
                self.expect("(")?;
                let c = self.expr()?;
                self.expect(")")?;
                Ok(Stmt::While(c, self.body()?))
            }
            "do" => {
                self.next();
                let body = self.body()?;
                if !self.eat_kw("while") {
                    return err(self.line(), "expected while after do { … }");
                }
                self.expect("(")?;
                let c = self.expr()?;
                self.expect(")")?;
                self.semi()?;
                Ok(Stmt::DoWhile(body, c))
            }
            "for" => {
                self.next();
                self.expect("(")?;
                self.no_in = true;
                let init = if self.is(";") {
                    None
                } else if self.eat_kw("var") {
                    let v = self.var_list()?;
                    if v.len() == 1 && v[0].1.is_none() && self.eat_kw("in") {
                        self.no_in = false;
                        let obj = self.expr()?;
                        self.expect(")")?;
                        let body = self.body()?;
                        return Ok(Stmt::ForIn(Expr::Var(v[0].0.clone()), true, obj, body));
                    }
                    Some(Box::new(Stmt::Var(v)))
                } else {
                    let e = self.expr()?;
                    if self.eat_kw("in") {
                        self.no_in = false;
                        let obj = self.expr()?;
                        self.expect(")")?;
                        let body = self.body()?;
                        return Ok(Stmt::ForIn(e, false, obj, body));
                    }
                    Some(Box::new(Stmt::Expr(e)))
                };
                self.no_in = false;
                self.expect(";")?;
                let cond = if self.is(";") { None } else { Some(self.expr()?) };
                self.expect(";")?;
                let step = if self.is(")") { None } else { Some(Box::new(Stmt::Expr(self.expr()?))) };
                self.expect(")")?;
                Ok(Stmt::For(init, cond, step, self.body()?))
            }
            "switch" => {
                self.next();
                self.expect("(")?;
                let x = self.expr()?;
                self.expect(")")?;
                self.expect("{")?;
                let mut clauses = Vec::new();
                while !self.eat("}") {
                    let c = if self.eat_kw("case") {
                        Some(self.expr()?)
                    } else if self.eat_kw("default") {
                        None
                    } else {
                        return err(self.line(), format!("expected case or default, found {}", self.describe()));
                    };
                    self.expect(":")?;
                    let mut body = Vec::new();
                    while !self.is("}") && !self.is_kw("case") && !self.is_kw("default") {
                        if self.peek() == &T::Eof {
                            return err(self.line(), "missing } after switch");
                        }
                        body.push(self.stmt()?);
                    }
                    clauses.push((c, body));
                }
                Ok(Stmt::Switch(x, clauses))
            }
            "break" | "continue" => {
                self.next();
                self.semi()?;
                Ok(if kw == "break" { Stmt::Break } else { Stmt::Continue })
            }
            "return" => {
                self.next();
                if self.eat(";") || self.is("}") || self.nl() || self.peek() == &T::Eof {
                    return Ok(Stmt::Return(None));
                }
                let e = self.expr()?;
                self.semi()?;
                Ok(Stmt::Return(Some(e)))
            }
            "throw" => {
                self.next();
                let e = self.expr()?;
                self.semi()?;
                Ok(Stmt::Throw(e))
            }
            "try" => {
                self.next();
                let body = self.block()?;
                let catch = if self.eat_kw("catch") {
                    self.expect("(")?;
                    let n = self.ident()?;
                    self.skip_type()?;
                    self.expect(")")?;
                    Some((n, self.block()?))
                } else {
                    None
                };
                let finally = if self.eat_kw("finally") { Some(self.block()?) } else { None };
                if catch.is_none() && finally.is_none() {
                    return err(line, "try without catch or finally");
                }
                Ok(Stmt::Try(body, catch, finally))
            }
            "with" => {
                self.next();
                self.expect("(")?;
                let o = self.expr()?;
                self.expect(")")?;
                Ok(Stmt::With(o, self.body()?))
            }
            "__pcode" => {
                self.next();
                match self.next() {
                    T::Raw(text, l) => Ok(Stmt::Raw(super::pcode::parse_body(&text).map_err(|e| Error::Format(format!("in the P-code block at line {l}: {e}")))?)),
                    _ => err(line, "expected { after __pcode"),
                }
            }
            _ => {
                let e = self.expr()?;
                self.semi()?;
                Ok(Stmt::Expr(e))
            }
        }
    }

    /// Parameters and body after `function name`.
    pub(super) fn function_rest(&mut self, name: String) -> Result<Func> {
        self.expect("(")?;
        let mut params = Vec::new();
        while !self.eat(")") {
            params.push(self.ident()?);
            self.skip_type()?;
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        self.skip_type()?;
        if self.eat_kw("__pcode") {
            // `__pcode(registers, "flags", register of each parameter…)` for DefineFunction2, plain for DefineFunction.
            let mut v2 = None;
            let mut param_regs = vec![0; params.len()];
            if self.eat("(") {
                let regs = self.number()? as u8;
                self.expect(",")?;
                let flags = match self.next() {
                    T::Str(s) => parse_flags(&s).ok_or_else(|| Error::Format(format!("line {}: bad flags {s:?}", self.line())))?,
                    _ => return err(self.line(), "expected the flags as a string"),
                };
                for r in param_regs.iter_mut() {
                    self.expect(",")?;
                    *r = self.number()? as u8;
                }
                self.expect(")")?;
                v2 = Some((regs, flags));
            }
            let line = self.line();
            let T::Raw(text, l) = self.next() else { return err(line, "expected { after __pcode") };
            let actions = super::pcode::parse_body(&text).map_err(|e| Error::Format(format!("in the P-code body at line {l}: {e}")))?;
            return Ok(Func { name, params, body: Vec::new(), raw: Some(RawBody { v2, param_regs, actions }) });
        }
        let body = self.block()?;
        Ok(Func { name, params, body, raw: None })
    }

    fn number(&mut self) -> Result<f64> {
        match self.next() {
            T::Num(n, _) => Ok(n),
            _ => err(self.line(), "expected a number"),
        }
    }

    pub(super) fn expr(&mut self) -> Result<Expr> {
        let e = self.assign()?;
        if self.is(",") {
            // `a, b`: both evaluated, the value is the last.
            let mut items = vec![e];
            while self.eat(",") {
                items.push(self.assign()?);
            }
            return Ok(Expr::Intrinsic("__comma".into(), items));
        }
        Ok(e)
    }

    pub(super) fn assign(&mut self) -> Result<Expr> {
        let line = self.line();
        let lhs = self.cond()?;
        let T::P(p) = self.peek().clone() else { return Ok(lhs) };
        if p == "=" || compound(p).is_some() {
            if !matches!(lhs, Expr::Var(_) | Expr::Member(..) | Expr::Eval(_)) {
                return err(line, "cannot assign to this expression");
            }
            self.next();
            let rhs = self.assign()?;
            let v = match compound(p) {
                Some(op) => Expr::Binary(op, Box::new(lhs.clone()), Box::new(rhs)),
                None => rhs,
            };
            return Ok(Expr::Assign(Box::new(lhs), Box::new(v)));
        }
        Ok(lhs)
    }

    fn cond(&mut self) -> Result<Expr> {
        let c = self.binary(5)?;
        if self.eat("?") {
            let saved = self.no_in;
            self.no_in = false;
            let a = self.assign()?;
            self.no_in = saved;
            self.expect(":")?;
            let b = self.assign()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    /// The binary operator at the cursor with its precedence (`&&` 6, `||` 5).
    fn bin_op(&self) -> Option<(u8, Option<Bin>, &'static str)> {
        let p = match self.peek() {
            T::P(p) => *p,
            T::Ident(k) if k == "instanceof" => return Some((11, Some(Bin::InstanceOf), "instanceof")),
            _ => return None,
        };
        let b = match p {
            "||" => return Some((5, None, "||")),
            "&&" => return Some((6, None, "&&")),
            "|" => Bin::BitOr,
            "^" => Bin::BitXor,
            "&" => Bin::BitAnd,
            "==" => Bin::Eq,
            "!=" => Bin::Ne,
            "===" => Bin::StrictEq,
            "!==" => Bin::StrictNe,
            "<" => Bin::Lt,
            ">" => Bin::Gt,
            "<=" => Bin::Le,
            ">=" => Bin::Ge,
            "<<" => Bin::Shl,
            ">>" => Bin::Shr,
            ">>>" => Bin::UShr,
            "+" => Bin::Add,
            "-" => Bin::Sub,
            "*" => Bin::Mul,
            "/" => Bin::Div,
            "%" => Bin::Mod,
            _ => return None,
        };
        Some((b.prec(), Some(b), p))
    }

    fn binary(&mut self, min: u8) -> Result<Expr> {
        let mut lhs = self.unary()?;
        while let Some((prec, op, _)) = self.bin_op() {
            if prec < min {
                break;
            }
            self.next();
            let rhs = self.binary(prec + 1)?;
            lhs = match op {
                Some(b) => Expr::Binary(b, Box::new(lhs), Box::new(rhs)),
                None if prec == 6 => Expr::And(Box::new(lhs), Box::new(rhs)),
                None => Expr::Or(Box::new(lhs), Box::new(rhs)),
            };
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr> {
        let line = self.line();
        if let T::P(p) = self.peek().clone() {
            let op = match p {
                "!" => Some(Un::Not),
                "-" => Some(Un::Neg),
                "~" => Some(Un::BitNot),
                "+" => {
                    self.next();
                    let x = self.unary()?;
                    return Ok(Expr::Intrinsic("Number".into(), vec![x]));
                }
                "++" | "--" => {
                    self.next();
                    let x = self.unary()?;
                    if !matches!(x, Expr::Var(_) | Expr::Member(..)) {
                        return err(line, format!("{p} needs a variable or property"));
                    }
                    return Ok(Expr::PreInc(Box::new(x), p == "++"));
                }
                _ => None,
            };
            if let Some(op) = op {
                self.next();
                let x = self.unary()?;
                // Negative literals are constants.
                if op == Un::Neg {
                    match x {
                        Expr::Int(0) => return Ok(Expr::Num(-0.0)),
                        Expr::Int(i) => return Ok(Expr::Int(-i)),
                        Expr::Num(n) => return Ok(Expr::Num(-n)),
                        _ => {}
                    }
                }
                return Ok(Expr::Unary(op, Box::new(x)));
            }
        }
        if self.eat_kw("typeof") {
            let x = self.unary()?;
            return Ok(Expr::Unary(Un::TypeOf, Box::new(x)));
        }
        if self.eat_kw("delete") {
            let x = self.unary()?;
            if !matches!(x, Expr::Var(_) | Expr::Member(..)) {
                return err(line, "delete needs a variable or property");
            }
            return Ok(Expr::Delete(Box::new(x)));
        }
        if self.eat_kw("void") {
            let x = self.unary()?;
            return Ok(Expr::Intrinsic("__void".into(), vec![x]));
        }
        let e = self.postfix()?;
        Ok(e)
    }

    fn postfix(&mut self) -> Result<Expr> {
        let e = self.call()?;
        if !self.nl() && (self.is("++") || self.is("--")) {
            let inc = self.is("++");
            if !matches!(e, Expr::Var(_) | Expr::Member(..)) {
                return err(self.line(), "++ / -- needs a variable or property");
            }
            self.next();
            return Ok(Expr::PostInc(Box::new(e), inc));
        }
        Ok(e)
    }

    fn args(&mut self) -> Result<Vec<Expr>> {
        self.expect("(")?;
        let mut args = Vec::new();
        let saved = self.no_in;
        self.no_in = false;
        while !self.eat(")") {
            args.push(self.assign()?);
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        self.no_in = saved;
        Ok(args)
    }

    fn call(&mut self) -> Result<Expr> {
        let mut e = if self.eat_kw("new") {
            let callee = self.member_only()?;
            let args = if self.is("(") { self.args()? } else { Vec::new() };
            Expr::New(Box::new(callee), args)
        } else {
            self.primary()?
        };
        loop {
            if self.eat(".") {
                let n = self.prop_name()?;
                e = Expr::Member(Box::new(e), Box::new(Expr::Str(n)));
            } else if self.is("[") {
                self.next();
                let saved = self.no_in;
                self.no_in = false;
                let n = self.expr()?;
                self.no_in = saved;
                self.expect("]")?;
                e = Expr::Member(Box::new(e), Box::new(n));
            } else if self.is("(") {
                let args = self.args()?;
                e = match e {
                    // `eval(x)` reads a variable by a computed name.
                    Expr::Var(n) if n == "eval" && args.len() == 1 => Expr::Eval(Box::new(args.into_iter().next().unwrap())),
                    e => Expr::Call(Box::new(e), args),
                };
            } else {
                break;
            }
        }
        Ok(e)
    }

    /// The callee of `new`: a primary with member accesses, no calls.
    fn member_only(&mut self) -> Result<Expr> {
        let mut e = if self.eat_kw("new") {
            let callee = self.member_only()?;
            let args = if self.is("(") { self.args()? } else { Vec::new() };
            Expr::New(Box::new(callee), args)
        } else {
            self.primary()?
        };
        loop {
            if self.eat(".") {
                let n = self.prop_name()?;
                e = Expr::Member(Box::new(e), Box::new(Expr::Str(n)));
            } else if self.eat("[") {
                let n = self.expr()?;
                self.expect("]")?;
                e = Expr::Member(Box::new(e), Box::new(n));
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr> {
        let line = self.line();
        match self.next() {
            T::Num(n, int) => Ok(if int && n.fract() == 0.0 && n <= i32::MAX as f64 { Expr::Int(n as i32) } else { Expr::Num(n) }),
            T::Str(s) => Ok(Expr::Str(s)),
            T::P("(") => {
                let saved = self.no_in;
                self.no_in = false;
                let e = self.expr()?;
                self.no_in = saved;
                self.expect(")")?;
                Ok(e)
            }
            T::P("[") => {
                let mut items = Vec::new();
                while !self.eat("]") {
                    items.push(self.assign()?);
                    if !self.eat(",") {
                        self.expect("]")?;
                        break;
                    }
                }
                Ok(Expr::Array(items))
            }
            T::P("{") => {
                let mut items = Vec::new();
                while !self.eat("}") {
                    let k = match self.next() {
                        T::Ident(s) | T::Str(s) => Expr::Str(s),
                        T::Num(n, _) => Expr::Str(super::print::number(n)),
                        _ => return err(self.line(), "expected a property name in the object"),
                    };
                    self.expect(":")?;
                    items.push((k, self.assign()?));
                    if !self.eat(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                Ok(Expr::Object(items))
            }
            T::Ident(k) => match k.as_str() {
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "null" => Ok(Expr::Null),
                "undefined" => Ok(Expr::Undefined),
                "function" => {
                    let name = if let T::Ident(_) = self.peek() { self.ident()? } else { String::new() };
                    Ok(Expr::Function(Box::new(self.function_rest(name)?)))
                }
                k if is_reserved(k) => err(line, format!("unexpected {k}")),
                _ => Ok(Expr::Var(k)),
            },
            T::Eof => err(line, "unexpected end of the script"),
            T::Raw(..) => err(line, "unexpected P-code block"),
            T::P(p) => err(line, format!("unexpected '{p}'")),
        }
    }

    // Helpers for `class` parsing.
    pub(super) fn at_p(&self, p: &str) -> bool {
        self.is(p)
    }
    pub(super) fn take_p(&mut self, p: &str) -> bool {
        self.eat(p)
    }
    pub(super) fn take_kw(&mut self, k: &str) -> bool {
        self.eat_kw(k)
    }
    pub(super) fn need(&mut self, p: &str) -> Result<()> {
        self.expect(p)
    }
    pub(super) fn name(&mut self) -> Result<String> {
        self.prop_name()
    }
    pub(super) fn line_no(&self) -> usize {
        self.line()
    }
    pub(super) fn end_stmt(&mut self) -> Result<()> {
        self.semi()
    }
    pub(super) fn ty(&mut self) -> Result<()> {
        self.skip_type()
    }
    pub(super) fn at_end(&self) -> bool {
        self.peek() == &T::Eof
    }
}

fn parse_flags(s: &str) -> Option<u16> {
    let mut f = 0u16;
    for part in s.split('|').map(str::trim).filter(|p| !p.is_empty() && *p != "0") {
        f |= match super::action::FLAG_NAMES.iter().find(|(_, n)| n.eq_ignore_ascii_case(part)) {
            Some((b, _)) => *b,
            None => u16::from_str_radix(part.strip_prefix("0x")?, 16).ok()?,
        };
    }
    Some(f)
}
