//! P-code: the text form of `action::Script`, one action per line, that
//! assembles back to the same bytes.
//!
//! ```text
//! ConstantPool "_global", "Foo"
//! Push c0:"_global", 1, 2.5, "text", r1, true, null, undefined
//! If L1
//! DefineFunction2 "name", 3, preloadThis|suppressArguments, r1:"a", "b" {
//!   Push r1
//!   Return
//! }
//! L1:
//! ```
//!
//! Push values: `"…"` string, `12` integer, `12.0` / `1e21` / `NaN` /
//! `Infinity` double, `1.5f` float, `c3:"…"` constant pool entry (the text is
//! a reminder; the index is what counts; `cw3` stores a 16-bit index), `r1`
//! register, `true`, `false`, `null`, `undefined`. Labels are `name:` on
//! their own line and branch targets name them. `;` starts a comment.

use std::collections::HashMap;
use std::fmt::Write;

use super::action::{op_name, op_of, Action, Catch, Function, Script, Try, Value, FLAG_NAMES};
use tempest_upk::io::{Error, Result};

/// A string literal with the escapes both P-code and ActionScript read.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c if c == '\u{2028}' || c == '\u{2029}' || c == '\u{feff}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn fmt_double(d: f64) -> String {
    if d.is_nan() {
        "NaN".into()
    } else if d.is_infinite() {
        if d > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else {
        format!("{d:?}")
    }
}

fn fmt_value(v: &Value, pool: &[String]) -> String {
    match v {
        Value::Str(s) => quote(s),
        Value::Float(f) => {
            if f.is_nan() {
                "NaNf".into()
            } else if f.is_infinite() {
                if *f > 0.0 { "Infinityf".into() } else { "-Infinityf".into() }
            } else {
                format!("{f:?}f")
            }
        }
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Reg(r) => format!("r{r}"),
        Value::Bool(b) => b.to_string(),
        Value::Double(d) => fmt_double(*d),
        Value::Int(i) => i.to_string(),
        Value::Const(i, wide) => {
            let w = if *wide { "cw" } else { "c" };
            match pool.get(*i as usize) {
                Some(s) => format!("{w}{i}:{}", quote(s)),
                None => format!("{w}{i}"),
            }
        }
    }
}

pub fn fmt_flags(flags: u16) -> String {
    let mut parts: Vec<String> = FLAG_NAMES.iter().filter(|(b, _)| flags & b != 0).map(|(_, n)| n.to_string()).collect();
    let known: u16 = FLAG_NAMES.iter().map(|(b, _)| b).sum();
    if flags & !known != 0 {
        parts.push(format!("0x{:x}", flags & !known));
    }
    if parts.is_empty() { "0".into() } else { parts.join("|") }
}

/// Labels of a block: every branch target, named in order.
fn labels(actions: &[Action]) -> HashMap<usize, String> {
    let mut targets: Vec<usize> = Vec::new();
    for a in actions {
        match a {
            Action::Jump(l) | Action::If(l) | Action::With(l) => targets.push(*l),
            Action::Try(t) => targets.extend([t.catch_at, t.finally_at, t.end]),
            _ => {}
        }
    }
    targets.sort_unstable();
    targets.dedup();
    targets.into_iter().enumerate().map(|(i, t)| (t, format!("L{}", i + 1))).collect()
}

fn print_block(actions: &[Action], pool: &mut Vec<String>, depth: usize, out: &mut String) {
    let labels = labels(actions);
    let ind = "  ".repeat(depth);
    let name = |l: &usize| labels[l].clone();
    for (i, a) in actions.iter().enumerate() {
        if let Some(l) = labels.get(&i) {
            let _ = writeln!(out, "{ind}{l}:");
        }
        out.push_str(&ind);
        match a {
            Action::Op(op) => out.push_str(op_name(*op).map(str::to_string).unwrap_or_else(|| format!("Op 0x{op:02x}")).as_str()),
            Action::ConstantPool(p) => {
                out.push_str("ConstantPool");
                for (i, s) in p.iter().enumerate() {
                    out.push_str(if i == 0 { " " } else { ", " });
                    out.push_str(&quote(s));
                }
                *pool = p.clone();
            }
            Action::Push(items) => {
                out.push_str("Push ");
                out.push_str(&items.iter().map(|v| fmt_value(v, pool)).collect::<Vec<_>>().join(", "));
            }
            Action::StoreRegister(r) => {
                let _ = write!(out, "StoreRegister r{r}");
            }
            Action::Jump(l) => {
                let _ = write!(out, "Jump {}", name(l));
            }
            Action::If(l) => {
                let _ = write!(out, "If {}", name(l));
            }
            Action::GotoFrame(f) => {
                let _ = write!(out, "GotoFrame {f}");
            }
            Action::GetUrl(u, t) => {
                let _ = write!(out, "GetURL {}, {}", quote(u), quote(t));
            }
            Action::WaitForFrame(f, s) => {
                let _ = write!(out, "WaitForFrame {f}, {s}");
            }
            Action::SetTarget(s) => {
                let _ = write!(out, "SetTarget {}", quote(s));
            }
            Action::GoToLabel(s) => {
                let _ = write!(out, "GoToLabel {}", quote(s));
            }
            Action::WaitForFrame2(s) => {
                let _ = write!(out, "WaitForFrame2 {s}");
            }
            Action::GetUrl2(m) => {
                let _ = write!(out, "GetURL2 {m}");
            }
            Action::GotoFrame2(f, bias) => {
                let _ = write!(out, "GotoFrame2 {f}");
                if let Some(b) = bias {
                    let _ = write!(out, ", {b}");
                }
            }
            Action::Function(f) => {
                let params: Vec<String> = f.params.iter().map(|(r, n)| if *r == 0 { quote(n) } else { format!("r{r}:{}", quote(n)) }).collect();
                match f.v2 {
                    Some((regs, flags)) => {
                        let _ = write!(out, "DefineFunction2 {}, {regs}, {}", quote(&f.name), fmt_flags(flags));
                    }
                    None => {
                        let _ = write!(out, "DefineFunction {}", quote(&f.name));
                    }
                }
                for p in params {
                    out.push_str(", ");
                    out.push_str(&p);
                }
                out.push_str(" {\n");
                let mut inner = pool.clone();
                print_block(&f.body, &mut inner, depth + 1, out);
                out.push_str(&ind);
                out.push('}');
            }
            Action::Try(t) => {
                let c = match &t.catch {
                    Catch::Name(n) => quote(n),
                    Catch::Reg(r) => format!("r{r}"),
                };
                let _ = write!(out, "Try {}, {c}, {}, {}, {}", t.flags, name(&t.catch_at), name(&t.finally_at), name(&t.end));
            }
            Action::With(l) => {
                let _ = write!(out, "With {}", name(l));
            }
            Action::Raw(op, data) => {
                let _ = write!(out, "Raw 0x{op:02x}, \"{}\"", data.iter().map(|b| format!("{b:02x}")).collect::<String>());
            }
        }
        out.push('\n');
    }
    if let Some(l) = labels.get(&actions.len()) {
        let _ = writeln!(out, "{ind}{l}:");
    }
}

pub fn print(s: &Script) -> String {
    let mut out = String::new();
    let mut pool = Vec::new();
    print_block(&s.actions, &mut pool, 0, &mut out);
    if !s.trailing.is_empty() {
        let _ = writeln!(out, "Trailing \"{}\"", s.trailing.iter().map(|b| format!("{b:02x}")).collect::<String>());
    }
    out
}

// ----------------------------------------------------------------------------
// Assembler
// ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(String),
    Punct(char),
}

fn perr<T>(line: usize, msg: impl std::fmt::Display) -> Result<T> {
    Err(Error::Format(format!("line {line}: {msg}")))
}

/// Read a string literal starting at the opening quote; returns the text and the index after it.
pub fn unquote(chars: &[char], start: usize) -> std::result::Result<(String, usize), String> {
    let q = chars[start];
    let mut i = start + 1;
    let mut s = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == q {
            return Ok((s, i + 1));
        }
        if c == '\\' {
            i += 1;
            let e = *chars.get(i).ok_or("unterminated string")?;
            match e {
                'n' => s.push('\n'),
                'r' => s.push('\r'),
                't' => s.push('\t'),
                'b' => s.push('\u{8}'),
                'f' => s.push('\u{c}'),
                'v' => s.push('\u{b}'),
                '0' if !chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) => s.push('\0'),
                'x' | 'u' => {
                    let n = if e == 'x' { 2 } else { 4 };
                    let hex: String = chars.get(i + 1..i + 1 + n).ok_or("bad escape")?.iter().collect();
                    let v = u32::from_str_radix(&hex, 16).map_err(|_| format!("bad escape \\{e}{hex}"))?;
                    s.push(char::from_u32(v).ok_or("bad escape")?);
                    i += n;
                }
                '\n' => {}
                c => s.push(c),
            }
            i += 1;
            continue;
        }
        if c == '\n' {
            return Err("unterminated string".into());
        }
        s.push(c);
        i += 1;
    }
    Err("unterminated string".into())
}

fn tokens(line: &str, n: usize) -> Result<Vec<Tok>> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == ';' || (c == '/' && chars.get(i + 1) == Some(&'/')) {
            break;
        } else if c == '"' {
            let (s, next) = unquote(&chars, i).or_else(|e| perr(n, e))?;
            out.push(Tok::Str(s));
            i = next;
        } else if c.is_ascii_digit() || ((c == '-' || c == '+' || c == '.') && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit() || *d == 'I' || *d == 'N')) {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || ((chars[i] == '-' || chars[i] == '+') && matches!(chars[i - 1], 'e' | 'E'))) {
                i += 1;
            }
            out.push(Tok::Num(chars[start..i].iter().collect()));
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else {
            out.push(Tok::Punct(c));
            i += 1;
        }
    }
    Ok(out)
}

fn parse_number(s: &str) -> Option<Value> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if let Some(f) = body.strip_suffix('f').filter(|b| !b.starts_with("0x")) {
        let v: f32 = match f {
            "NaN" => f32::NAN,
            "Infinity" => f32::INFINITY,
            _ => f.parse().ok()?,
        };
        return Some(Value::Float(if neg { -v } else { v }));
    }
    if body == "NaN" {
        return Some(Value::Double(f64::NAN));
    }
    if body == "Infinity" {
        return Some(Value::Double(if neg { f64::NEG_INFINITY } else { f64::INFINITY }));
    }
    if let Some(h) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        let v = i64::from_str_radix(h, 16).ok()?;
        return i32::try_from(if neg { -v } else { v }).ok().map(Value::Int);
    }
    if body.contains(['.', 'e', 'E']) {
        let v: f64 = body.parse().ok()?;
        return Some(Value::Double(if neg { -v } else { v }));
    }
    let v: i64 = body.parse().ok()?;
    i32::try_from(if neg { -v } else { v }).ok().map(Value::Int)
}

/// One parsed line before labels are resolved.
enum Line {
    Label(String),
    Action(Action, Vec<String>),
    Open(Box<Function>),
    Close,
    Trailing(Vec<u8>),
}

fn hex(s: &str, n: usize) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return perr(n, "odd hex length");
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).or_else(|_| perr(n, "bad hex"))).collect()
}

struct Args {
    toks: Vec<Tok>,
    pos: usize,
    line: usize,
}

impl Args {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn comma(&mut self) -> bool {
        if self.peek() == Some(&Tok::Punct(',')) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn need_comma(&mut self) -> Result<()> {
        if self.comma() { Ok(()) } else { perr(self.line, "expected ,") }
    }
    fn str(&mut self) -> Result<String> {
        match self.next() {
            Some(Tok::Str(s)) => Ok(s),
            t => perr(self.line, format!("expected a string, found {t:?}")),
        }
    }
    fn int<T: TryFrom<i64>>(&mut self) -> Result<T> {
        match self.next() {
            Some(Tok::Num(s)) => {
                let v = if let Some(h) = s.strip_prefix("0x") { i64::from_str_radix(h, 16).ok() } else { s.parse().ok() };
                v.and_then(|v| T::try_from(v).ok()).ok_or_else(|| Error::Format(format!("line {}: {s} is out of range", self.line)))
            }
            t => perr(self.line, format!("expected a number, found {t:?}")),
        }
    }
    fn label(&mut self) -> Result<String> {
        match self.next() {
            Some(Tok::Ident(s)) => Ok(s),
            t => perr(self.line, format!("expected a label, found {t:?}")),
        }
    }
    fn reg(&mut self) -> Result<u8> {
        match self.next() {
            Some(Tok::Ident(s)) if s.starts_with('r') => s[1..].parse().or_else(|_| perr(self.line, format!("bad register {s}"))),
            Some(Tok::Num(s)) => s.parse().or_else(|_| perr(self.line, format!("bad register {s}"))),
            t => perr(self.line, format!("expected a register, found {t:?}")),
        }
    }
    fn end(&self) -> Result<()> {
        match self.peek() {
            None => Ok(()),
            Some(t) => perr(self.line, format!("unexpected {t:?}")),
        }
    }
    fn value(&mut self) -> Result<Value> {
        let line = self.line;
        match self.next() {
            Some(Tok::Str(s)) => Ok(Value::Str(s)),
            Some(Tok::Num(s)) => parse_number(&s).ok_or_else(|| Error::Format(format!("line {line}: bad number {s}"))),
            Some(Tok::Ident(s)) => match s.as_str() {
                "null" => Ok(Value::Null),
                "undefined" => Ok(Value::Undefined),
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                "NaN" | "NaNf" | "Infinity" | "Infinityf" => parse_number(&s).ok_or_else(|| Error::Format(format!("line {line}: bad number"))),
                _ if s.starts_with('r') && s[1..].parse::<u8>().is_ok() => Ok(Value::Reg(s[1..].parse().unwrap())),
                _ if s.starts_with("cw") || s.starts_with('c') => {
                    let wide = s.starts_with("cw");
                    let i: u16 = s[if wide { 2 } else { 1 }..].parse().or_else(|_| perr(line, format!("unknown value {s}")))?;
                    if self.peek() == Some(&Tok::Punct(':')) {
                        self.pos += 1;
                        self.str()?;
                    }
                    Ok(Value::Const(i, wide && i < 256))
                }
                _ => perr(line, format!("unknown value {s}")),
            },
            t => perr(line, format!("expected a value, found {t:?}")),
        }
    }
}

fn parse_line(text: &str, n: usize) -> Result<Option<Line>> {
    let toks = tokens(text, n)?;
    if toks.is_empty() {
        return Ok(None);
    }
    if toks == [Tok::Punct('}')] {
        return Ok(Some(Line::Close));
    }
    if let [Tok::Ident(l), Tok::Punct(':')] = toks.as_slice() {
        return Ok(Some(Line::Label(l.clone())));
    }
    let Tok::Ident(name) = &toks[0] else { return perr(n, "expected an action") };
    let name = name.clone();
    let mut a = Args { toks, pos: 1, line: n };
    let mut refs = Vec::new();
    let action = match name.to_ascii_lowercase().as_str() {
        "constantpool" => {
            let mut pool = Vec::new();
            if a.peek().is_some() {
                pool.push(a.str()?);
                while a.comma() {
                    pool.push(a.str()?);
                }
            }
            Action::ConstantPool(pool)
        }
        "push" => {
            let mut items = vec![a.value()?];
            while a.comma() {
                items.push(a.value()?);
            }
            Action::Push(items)
        }
        "storeregister" => Action::StoreRegister(a.reg()?),
        "jump" | "if" => {
            refs.push(a.label()?);
            if name.eq_ignore_ascii_case("jump") { Action::Jump(0) } else { Action::If(0) }
        }
        "with" => {
            refs.push(a.label()?);
            Action::With(0)
        }
        "gotoframe" => Action::GotoFrame(a.int()?),
        "geturl" => {
            let u = a.str()?;
            a.need_comma()?;
            Action::GetUrl(u, a.str()?)
        }
        "waitforframe" => {
            let f = a.int()?;
            a.need_comma()?;
            Action::WaitForFrame(f, a.int()?)
        }
        "settarget" => Action::SetTarget(a.str()?),
        "gotolabel" => Action::GoToLabel(a.str()?),
        "waitforframe2" => Action::WaitForFrame2(a.int()?),
        "geturl2" => Action::GetUrl2(a.int()?),
        "gotoframe2" => {
            let f = a.int()?;
            let bias = if a.comma() { Some(a.int()?) } else { None };
            Action::GotoFrame2(f, bias)
        }
        "try" => {
            let flags = a.int()?;
            a.need_comma()?;
            let catch = match a.peek() {
                Some(Tok::Str(_)) => Catch::Name(a.str()?),
                _ => Catch::Reg(a.reg()?),
            };
            for _ in 0..3 {
                a.need_comma()?;
                refs.push(a.label()?);
            }
            Action::Try(Box::new(Try { flags, catch, catch_at: 0, finally_at: 0, end: 0 }))
        }
        "raw" => {
            let op = a.int()?;
            a.need_comma()?;
            Action::Raw(op, hex(&a.str()?, n)?)
        }
        "trailing" => {
            let b = hex(&a.str()?, n)?;
            a.end()?;
            return Ok(Some(Line::Trailing(b)));
        }
        "definefunction" | "definefunction2" => {
            let fname = a.str()?;
            let mut v2 = None;
            if name.eq_ignore_ascii_case("definefunction2") {
                a.need_comma()?;
                let regs = a.int()?;
                a.need_comma()?;
                let mut flags = 0u16;
                loop {
                    match a.next() {
                        Some(Tok::Ident(f)) => flags |= FLAG_NAMES.iter().find(|(_, n)| n.eq_ignore_ascii_case(&f)).map(|(b, _)| *b).ok_or_else(|| Error::Format(format!("line {n}: unknown flag {f}")))?,
                        Some(Tok::Num(v)) => flags |= v.strip_prefix("0x").and_then(|h| u16::from_str_radix(h, 16).ok()).or_else(|| v.parse().ok()).ok_or_else(|| Error::Format(format!("line {n}: bad flags {v}")))?,
                        t => return perr(n, format!("expected flags, found {t:?}")),
                    }
                    if a.peek() != Some(&Tok::Punct('|')) {
                        break;
                    }
                    a.pos += 1;
                }
                v2 = Some((regs, flags));
            }
            let mut params = Vec::new();
            while a.comma() {
                let reg = if matches!(a.peek(), Some(Tok::Ident(_))) {
                    let r = a.reg()?;
                    if a.next() != Some(Tok::Punct(':')) {
                        return perr(n, "expected : after the parameter register");
                    }
                    r
                } else {
                    0
                };
                params.push((reg, a.str()?));
            }
            if a.next() != Some(Tok::Punct('{')) {
                return perr(n, "expected { after the function header");
            }
            a.end()?;
            return Ok(Some(Line::Open(Box::new(Function { name: fname, v2, params, body: Vec::new() }))));
        }
        _ => {
            if let Some(op) = op_of(&name).filter(|&op| op < 0x80) {
                Action::Op(op)
            } else if name.eq_ignore_ascii_case("op") {
                Action::Op(a.int()?)
            } else {
                return perr(n, format!("unknown action {name}"));
            }
        }
    };
    a.end()?;
    Ok(Some(Line::Action(action, refs)))
}

struct Block {
    actions: Vec<Action>,
    refs: Vec<(usize, Vec<String>, usize)>,
    labels: HashMap<String, usize>,
    func: Option<Box<Function>>,
}

impl Block {
    fn new(func: Option<Box<Function>>) -> Block {
        Block { actions: Vec::new(), refs: Vec::new(), labels: HashMap::new(), func }
    }
    fn resolve(mut self) -> Result<Vec<Action>> {
        for (i, names, line) in std::mem::take(&mut self.refs) {
            let mut ls = Vec::new();
            for n in &names {
                ls.push(*self.labels.get(n).ok_or_else(|| Error::Format(format!("line {line}: no label {n} in this block")))?);
            }
            match &mut self.actions[i] {
                Action::Jump(t) | Action::If(t) | Action::With(t) => *t = ls[0],
                Action::Try(t) => {
                    t.catch_at = ls[0];
                    t.finally_at = ls[1];
                    t.end = ls[2];
                }
                _ => {}
            }
        }
        Ok(self.actions)
    }
}

pub fn parse(text: &str) -> Result<Script> {
    let mut stack = vec![Block::new(None)];
    let mut trailing = Vec::new();
    for (i, l) in text.lines().enumerate() {
        let n = i + 1;
        let Some(line) = parse_line(l, n)? else { continue };
        let top = stack.last_mut().unwrap();
        match line {
            Line::Label(name) => {
                if top.labels.insert(name.clone(), top.actions.len()).is_some() {
                    return perr(n, format!("label {name} defined twice"));
                }
            }
            Line::Action(a, refs) => {
                if !refs.is_empty() {
                    top.refs.push((top.actions.len(), refs, n));
                }
                top.actions.push(a);
            }
            Line::Open(f) => stack.push(Block::new(Some(f))),
            Line::Close => {
                if stack.len() == 1 {
                    return perr(n, "} without a function");
                }
                let mut b = stack.pop().unwrap();
                let mut f = b.func.take().unwrap();
                f.body = b.resolve()?;
                stack.last_mut().unwrap().actions.push(Action::Function(f));
            }
            Line::Trailing(b) => trailing = b,
        }
    }
    if stack.len() != 1 {
        return Err(Error::Format("a function is missing its }".into()));
    }
    let actions = stack.pop().unwrap().resolve()?;
    Ok(Script { actions, trailing })
}

/// Actions as P-code lines for a source file: `depth` levels of `ind` before each line.
pub fn print_body(actions: &[Action], depth: usize, ind: &str) -> String {
    let mut text = String::new();
    let mut pool = Vec::new();
    print_block(actions, &mut pool, 0, &mut text);
    let mut out = String::new();
    for l in text.lines() {
        let trimmed = l.trim_start_matches(' ');
        let level = (l.len() - trimmed.len()) / 2 + depth;
        for _ in 0..level {
            out.push_str(ind);
        }
        out.push_str(trimmed);
        out.push('\n');
    }
    out
}

/// P-code lines (no `Trailing`) as actions.
pub fn parse_body(text: &str) -> Result<Vec<Action>> {
    let s = parse(text)?;
    if !s.trailing.is_empty() {
        return Err(Error::Format("Trailing is only allowed at the end of a script".into()));
    }
    Ok(s.actions)
}
