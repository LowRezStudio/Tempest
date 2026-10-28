//! Bytecode → ActionScript 2.
//!
//! The decompiler simulates the operand stack over a block and rebuilds
//! statements and expressions; control flow is structured from the shapes
//! the Flash compiler (and `compile`) emits: `if`/`else` (a forward `If`
//! over the then-part, a `Jump` over the else-part), `?:`, `&&` / `||`
//! (`PushDuplicate`, `If`, `Pop`), `while` / `for` / `do … while` (a branch
//! back to the header), `for … in` (`Enumerate2` with register 0),
//! `switch` (a chain of `StrictEquals` tests on a stored value), `try`,
//! `with`. A function whose body does not fit is printed as P-code inside
//! the source (`function (…) __pcode(…) { … }`), so decompiled scripts
//! always compile back to working code.
//!
//! Register locals are named `_locN_` (parameters keep their names), and a
//! register whose name would collide with a variable the function (or a
//! function inside it) reads by name is renamed.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::action::{Action, Catch, Function, Script, Value, PRELOAD_ARGUMENTS, PRELOAD_GLOBAL, PRELOAD_PARENT, PRELOAD_ROOT, PRELOAD_SUPER, PRELOAD_THIS};
use super::ast::{intrinsic, is_ident, is_reserved, Bin, Expr, Func, RawBody, Stmt, Un};
use super::print;

/// Register names are written as this prefix + the register number until
/// the function is finished, then renamed (`finish_names`).
const REG: char = '\u{1}';

enum DErr {
    /// A branch to an action the current structure does not explain.
    Goto(usize),
    Fail(String),
}

type R<T> = std::result::Result<T, DErr>;

fn fail<T>(msg: impl Into<String>) -> R<T> {
    Err(DErr::Fail(msg.into()))
}

pub fn source(s: &Script) -> String {
    print::program(&decompile(s))
}

/// The whole script as statements (a `Raw` statement when it cannot be structured).
pub fn decompile(s: &Script) -> Vec<Stmt> {
    let mut code = &s.actions[..];
    if let Some(Action::Op(0)) = code.last() {
        code = &code[..code.len() - 1];
    }
    let mut d = Dec::new(code, Rc::new(Vec::new()), HashMap::new());
    match d.body() {
        Ok(mut body) => {
            finish_names(&mut body, &mut [], Vec::new());
            prettify(&mut body);
            super::class::raise(&mut body);
            body
        }
        Err(_) => vec![Stmt::Raw(resolve_consts(code, &[]))],
    }
}

/// Why the script (or which of its functions) did not decompile, for `upk as2 verify`.
pub fn problems(s: &Script) -> Vec<String> {
    let mut out = Vec::new();
    let mut code = &s.actions[..];
    if let Some(Action::Op(0)) = code.last() {
        code = &code[..code.len() - 1];
    }
    let mut d = Dec::new(code, Rc::new(Vec::new()), HashMap::new());
    d.problems = Some(Vec::new());
    match d.body() {
        Ok(_) => {}
        Err(DErr::Fail(m)) => out.push(format!("script: {m}")),
        Err(DErr::Goto(t)) => out.push(format!("script: unstructured branch to action {t}")),
    }
    out.extend(d.problems.take().unwrap_or_default());
    out
}

/// Constant pool references replaced by their strings (for P-code kept in source).
fn resolve_consts(code: &[Action], inherited: &[String]) -> Vec<Action> {
    let mut pool: Vec<String> = inherited.to_vec();
    let mut out = Vec::with_capacity(code.len());
    for a in code {
        match a {
            Action::ConstantPool(p) => {
                pool = p.clone();
                out.push(a.clone());
            }
            Action::Push(vals) => out.push(Action::Push(
                vals.iter()
                    .map(|v| match v {
                        Value::Const(i, _) => pool.get(*i as usize).map(|s| Value::Str(s.clone())).unwrap_or_else(|| v.clone()),
                        v => v.clone(),
                    })
                    .collect(),
            )),
            Action::Function(f) => {
                let mut f = (**f).clone();
                f.body = resolve_consts(&f.body, &pool);
                out.push(Action::Function(Box::new(f)));
            }
            a => out.push(a.clone()),
        }
    }
    // With every reference resolved, a pool is only a no-op... unless it is referenced by an unresolvable index.
    let unresolved = out.iter().any(|a| matches!(a, Action::Push(v) if v.iter().any(|v| matches!(v, Value::Const(..)))));
    if !unresolved {
        out.retain(|a| !matches!(a, Action::ConstantPool(_)));
        // Branch targets index actions: removing pools shifts them.
        return reindex(code, out);
    }
    out
}

/// After removing `ConstantPool` actions from `out` (a copy of `orig` with
/// the same actions otherwise), move branch labels to the new indices.
fn reindex(orig: &[Action], mut out: Vec<Action>) -> Vec<Action> {
    let mut map = Vec::with_capacity(orig.len() + 1);
    let mut n = 0;
    for a in orig {
        map.push(n);
        if !matches!(a, Action::ConstantPool(_)) {
            n += 1;
        }
    }
    map.push(n);
    for a in &mut out {
        match a {
            Action::Jump(l) | Action::If(l) | Action::With(l) => *l = map[*l],
            Action::Try(t) => {
                t.catch_at = map[t.catch_at];
                t.finally_at = map[t.finally_at];
                t.end = map[t.end];
            }
            _ => {}
        }
    }
    out
}

#[derive(Clone, Copy)]
struct Ctx {
    brk: usize,
    cont: Option<usize>,
    forin: bool,
    is_loop: bool,
}

struct Dec<'a> {
    code: &'a [Action],
    /// The constant pool in effect at each action.
    pool_at: Vec<Rc<Vec<String>>>,
    /// Preloaded registers and register parameters.
    fixed: HashMap<u8, String>,
    /// Register locals declared so far (`var` printed).
    declared: HashSet<u8>,
    /// How often each register is referenced (temporaries of `switch` and `for … in` are taken back out).
    used: HashMap<u8, usize>,
    /// Back branches by target: the actions branching there.
    back: HashMap<usize, Vec<usize>>,
    ctx: Vec<Ctx>,
    /// Names the caller's registers must not take (unused here; filled for nested functions).
    problems: Option<Vec<String>>,
}

fn reg_name(r: u8) -> String {
    format!("{REG}{r}")
}

fn num_arg(e: &Expr) -> R<usize> {
    match e {
        Expr::Int(i) if *i >= 0 => Ok(*i as usize),
        Expr::Num(n) if *n >= 0.0 && n.fract() == 0.0 && *n < 65536.0 => Ok(*n as usize),
        _ => fail("argument count is not a constant"),
    }
}

/// Names as a plain variable reference when possible, else `eval(name)`.
fn by_name(name: Expr) -> Expr {
    match name {
        Expr::Str(s) if is_ident(&s) && !is_reserved(&s) => Expr::Var(s),
        n => Expr::Eval(Box::new(n)),
    }
}


impl<'a> Dec<'a> {
    fn new(code: &'a [Action], pool: Rc<Vec<String>>, fixed: HashMap<u8, String>) -> Dec<'a> {
        let mut pool_at = Vec::with_capacity(code.len());
        let mut cur = pool;
        let mut back: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, a) in code.iter().enumerate() {
            if let Action::ConstantPool(p) = a {
                cur = Rc::new(p.clone());
            }
            pool_at.push(cur.clone());
            if let Action::Jump(t) | Action::If(t) = a {
                if *t <= i {
                    back.entry(*t).or_default().push(i);
                }
            }
        }
        pool_at.push(cur);
        Dec { code, pool_at, fixed, declared: HashSet::new(), used: HashMap::new(), back, ctx: Vec::new(), problems: None }
    }

    /// Decompile the whole block: statements with the register names settled.
    fn body(&mut self) -> R<Vec<Stmt>> {
        let mut st = Vec::new();
        let mut out = Vec::new();
        self.run(0, self.code.len(), &mut st, &mut out)?;
        Self::flush(&mut st, &mut out);
        // Registers read or assigned inside expressions before any `var`: declare them first.
        let mut late: Vec<u8> = self.used.iter().filter(|(r, n)| **n > 0 && !self.declared.contains(r) && !self.fixed.contains_key(r)).map(|(r, _)| *r).collect();
        late.sort_unstable();
        if !late.is_empty() {
            out.insert(0, Stmt::Var(late.iter().map(|r| (reg_name(*r), None)).collect()));
        }
        Ok(out)
    }

    fn reg(&mut self, r: u8) -> Expr {
        if let Some(n) = self.fixed.get(&r) {
            return Expr::Var(n.clone());
        }
        *self.used.entry(r).or_default() += 1;
        Expr::Var(reg_name(r))
    }

    fn value(&mut self, v: &Value, at: usize) -> Expr {
        match v {
            Value::Str(s) => Expr::Str(s.clone()),
            Value::Float(f) => Expr::Num(*f as f64),
            Value::Null => Expr::Null,
            Value::Undefined => Expr::Undefined,
            Value::Reg(r) => self.reg(*r),
            Value::Bool(b) => Expr::Bool(*b),
            Value::Double(d) => Expr::Num(*d),
            Value::Int(i) => Expr::Int(*i),
            Value::Const(i, _) => match self.pool_at[at].get(*i as usize) {
                Some(s) => Expr::Str(s.clone()),
                None => Expr::Undefined,
            },
        }
    }

    /// Run `a..b` with an empty stack; it must produce exactly one value and no statements.
    fn expr_range(&mut self, a: usize, b: usize) -> R<Expr> {
        let saved = (self.declared.clone(), self.used.clone());
        let mut st = Vec::new();
        let mut out = Vec::new();
        let r = self.run(a, b, &mut st, &mut out);
        if r.is_ok() && out.is_empty() && st.len() == 1 {
            return Ok(st.pop().unwrap());
        }
        (self.declared, self.used) = saved;
        r?;
        fail("not an expression")
    }

    /// Run `a..b` as statements with an empty stack.
    fn block(&mut self, a: usize, b: usize) -> R<Vec<Stmt>> {
        let mut st = Vec::new();
        let mut out = Vec::new();
        self.run(a, b, &mut st, &mut out)?;
        Self::flush(&mut st, &mut out);
        Ok(out)
    }

    fn pop(st: &mut Vec<Expr>) -> R<Expr> {
        // An empty stack pops `undefined` (the Flash compiler's class prologue relies on it).
        match st.pop() {
            Some(e) if is_spent(&e) => fail("a value already run as a statement is used"),
            e => Ok(e.unwrap_or(Expr::Undefined)),
        }
    }

    /// Values left on the stack before a statement were evaluated before it:
    /// they become statements of their own (the Flash compiler leaves unused
    /// call results there), and their slots are marked spent.
    fn flush(st: &mut [Expr], out: &mut Vec<Stmt>) {
        for e in st.iter_mut() {
            if !(e.is_literal() || is_spent(e) || matches!(e, Expr::Function(_))) {
                let v = std::mem::replace(e, spent());
                out.push(Stmt::Expr(v));
            }
        }
    }

    fn pop_n(st: &mut Vec<Expr>, n: usize) -> R<Vec<Expr>> {
        (0..n).map(|_| Self::pop(st)).collect()
    }

    /// Add a statement; pending values on the stack would be evaluated after it, so they must be constants.
    fn emit(&mut self, s: Stmt, st: &mut [Expr], out: &mut Vec<Stmt>) -> R<()> {
        Self::flush(st, out);
        // The first plain assignment of a register local declares it.
        if let Stmt::Expr(Expr::Assign(t, v)) = &s {
            if let Expr::Var(n) = &**t {
                if let Some(r) = n.strip_prefix(REG).and_then(|r| r.parse::<u8>().ok()) {
                    if !self.declared.contains(&r) && !self.fixed.contains_key(&r) {
                        self.declared.insert(r);
                        out.push(Stmt::Var(vec![(n.clone(), Some((**v).clone()))]));
                        return Ok(());
                    }
                }
            }
        }
        out.push(s);
        Ok(())
    }

    /// The last back branch to `i` that lies before `end` (not the `continue`s of a loop already open there).
    fn back_edge(&self, i: usize, end: usize) -> Option<usize> {
        if self.ctx.iter().any(|c| c.is_loop && c.cont == Some(i)) {
            return None;
        }
        self.back.get(&i)?.iter().copied().filter(|&j| j < end).max()
    }

    fn unuse(&mut self, e: &Expr, n: usize) {
        if let Expr::Var(name) = e {
            if let Some(r) = name.strip_prefix(REG).and_then(|r| r.parse::<u8>().ok()) {
                if let Some(c) = self.used.get_mut(&r) {
                    *c = c.saturating_sub(n);
                }
            }
        }
    }

    /// What a jump to `t` means here: `break` or `continue`.
    fn jump_stmt(&self, t: usize) -> R<Stmt> {
        if let Some(c) = self.ctx.last() {
            if c.brk == t {
                return Ok(Stmt::Break);
            }
        }
        for c in self.ctx.iter().rev() {
            if c.is_loop {
                if c.cont == Some(t) {
                    return Ok(Stmt::Continue);
                }
                break;
            }
        }
        Err(DErr::Goto(t))
    }

    fn with_ctx<T>(&mut self, c: Ctx, f: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        self.ctx.push(c);
        let r = f(self);
        self.ctx.pop();
        r
    }

    fn run(&mut self, a: usize, b: usize, st: &mut Vec<Expr>, out: &mut Vec<Stmt>) -> R<()> {
        let mut i = a;
        while i < b {
            // Leaving a `for … in` by `return` first pops its remaining names.
            if i + 4 <= b && self.ctx.iter().any(|c| c.forin) && self.is_drain(i) {
                i += 4;
                continue;
            }
            if let Some(j) = self.back_edge(i, b) {
                Self::flush(st, out);
                i = self.lp(i, j, out)?;
                continue;
            }
            i = match self.step(i, b, st, out) {
                Err(DErr::Fail(m)) if !m.contains(" (at ") => {
                    let what = if std::env::var_os("AS2_DEBUG").is_some() { format!(": {:?} …", &self.code[i.saturating_sub(4)..(i + 3).min(self.code.len())]) } else { String::new() };
                    return Err(DErr::Fail(format!("{m} (at {i}){what}")));
                }
                r => r?,
            };
        }
        Ok(())
    }

    /// `L: Push null, Equals2, Not, If L`: pops what a `for … in` left on the stack.
    fn is_drain(&self, i: usize) -> bool {
        matches!(self.code.get(i..i + 4), Some([Action::Push(v), Action::Op(0x49), Action::Op(0x12), Action::If(t)]) if v.as_slice() == [Value::Null] && *t == i)
    }

    /// A loop whose header is `s` and whose back branch is at `j`.
    fn lp(&mut self, s: usize, j: usize, out: &mut Vec<Stmt>) -> R<usize> {
        let brk = j + 1;
        match self.code[j] {
            Action::Jump(_) => {
                // `while (c)`: the header computes the condition and leaves when it fails.
                let mut cond = None;
                let mut start = s;
                for k in s..j {
                    if let Action::If(t) = self.code[k] {
                        if t == brk {
                            if let Ok(c) = self.expr_range(s, k) {
                                cond = Some(c.negate());
                                start = k + 1;
                            }
                            break;
                        }
                    }
                }
                let c = Ctx { brk, cont: Some(s), forin: false, is_loop: true };
                match self.with_ctx(c, |d| d.block(start, j)) {
                    Ok(body) => out.push(Stmt::While(cond.unwrap_or(Expr::Bool(true)), body)),
                    Err(DErr::Goto(k)) if k > start && k < j => {
                        // `for (; c; step)` whose `continue`s jump to the step.
                        let c = Ctx { brk, cont: Some(k), forin: false, is_loop: true };
                        let body = self.with_ctx(c, |d| d.block(start, k))?;
                        let mut step = self.block(k, j)?;
                        let step = match step.len() {
                            0 => None,
                            1 if matches!(step[0], Stmt::Expr(_)) => Some(Box::new(step.pop().unwrap())),
                            _ => return fail("a for loop's step is more than one expression"),
                        };
                        out.push(Stmt::For(None, cond, step, body));
                    }
                    Err(e) => return Err(e),
                }
                Ok(brk)
            }
            Action::If(_) => {
                let c = Ctx { brk, cont: None, forin: false, is_loop: true };
                let (body, cond) = self.with_ctx(c, |d| {
                    let mut st = Vec::new();
                    let mut body = Vec::new();
                    d.run(s, j, &mut st, &mut body)?;
                    if st.len() != 1 {
                        return fail("do-while without a condition");
                    }
                    Ok((body, st.pop().unwrap()))
                })?;
                out.push(Stmt::DoWhile(body, cond));
                Ok(brk)
            }
            _ => unreachable!(),
        }
    }

    /// One action (or a structure starting with it); returns the next index.
    fn step(&mut self, i: usize, b: usize, st: &mut Vec<Expr>, out: &mut Vec<Stmt>) -> R<usize> {
        let code = self.code;
        match &code[i] {
            Action::Op(op) => self.op(i, *op, b, st, out),
            Action::ConstantPool(_) => Ok(i + 1),
            Action::Push(vals) => {
                for v in vals {
                    let e = self.value(v, i);
                    st.push(e);
                }
                Ok(i + 1)
            }
            Action::StoreRegister(r) => {
                let v = Self::pop(st)?;
                let target = self.reg(*r);
                let popped = i + 1 < b && matches!(code[i + 1], Action::Op(0x17));
                // `x++` / `x--` as a value: the old value is on the stack, the new one stored.
                if popped {
                    if let Expr::Intrinsic(name, args) = &v {
                        if (name == "__increment" || name == "__decrement") && args.len() == 1 && args[0] == target && st.last() == Some(&target) {
                            st.pop();
                            st.push(Expr::PostInc(Box::new(target), name == "__increment"));
                            return Ok(i + 2);
                        }
                    }
                }
                let assign = match v {
                    Expr::Intrinsic(name, args) if (name == "__increment" || name == "__decrement") && args.len() == 1 && args[0] == target => {
                        if popped {
                            Expr::PostInc(Box::new(target), name == "__increment")
                        } else {
                            Expr::PreInc(Box::new(target), name == "__increment")
                        }
                    }
                    v => Expr::Assign(Box::new(target), Box::new(v)),
                };
                if popped {
                    self.emit(Stmt::Expr(assign), st, out)?;
                    Ok(i + 2)
                } else {
                    st.push(assign);
                    Ok(i + 1)
                }
            }
            Action::Jump(t) => {
                match self.jump_stmt(*t) {
                    Ok(s) => self.emit(s, st, out)?,
                    // A jump to the next action does nothing.
                    Err(DErr::Goto(_)) if *t == i + 1 => {}
                    Err(e) => return Err(e),
                }
                Ok(i + 1)
            }
            Action::If(t) => self.branch(i, *t, b, st, out),
            Action::Function(f) => {
                let func = self.function(f, i);
                if f.name.is_empty() {
                    st.push(Expr::Function(Box::new(func)));
                } else {
                    self.emit(Stmt::Function(Box::new(func)), st, out)?;
                }
                Ok(i + 1)
            }
            Action::Try(t) => {
                let t = (**t).clone();
                Self::flush(st, out);
                let try_end = if t.flags & 1 != 0 { t.catch_at } else { t.finally_at };
                let mut body_end = try_end;
                if body_end > i + 1 {
                    if let Action::Jump(x) = code[body_end - 1] {
                        if x == t.finally_at || x == t.end {
                            body_end -= 1;
                        }
                    }
                }
                let body = self.block(i + 1, body_end)?;
                let catch = if t.flags & 1 != 0 {
                    let name = match &t.catch {
                        Catch::Name(n) => n.clone(),
                        Catch::Reg(r) => {
                            let e = self.reg(*r);
                            self.declared.insert(*r);
                            match e {
                                Expr::Var(n) => n,
                                _ => unreachable!(),
                            }
                        }
                    };
                    let mut cend = t.finally_at;
                    if t.flags & 2 != 0 && cend > t.catch_at {
                        if let Action::Jump(x) = code[cend - 1] {
                            if x == t.finally_at {
                                cend -= 1;
                            }
                        }
                    }
                    Some((name, self.block(t.catch_at, cend)?))
                } else {
                    None
                };
                let finally = if t.flags & 2 != 0 { Some(self.block(t.finally_at, t.end)?) } else { None };
                out.push(Stmt::Try(body, catch, finally));
                Ok(t.end.max(i + 1))
            }
            Action::With(end) => {
                let o = Self::pop(st)?;
                let body = self.block(i + 1, *end)?;
                self.emit(Stmt::With(o, body), st, out)?;
                Ok(*end)
            }
            Action::GotoFrame(f) => {
                self.emit(Stmt::Expr(Expr::Intrinsic("__gotoFrame".into(), vec![Expr::Int(*f as i32)])), st, out)?;
                Ok(i + 1)
            }
            Action::GoToLabel(l) => {
                self.emit(Stmt::Expr(Expr::Intrinsic("__gotoLabel".into(), vec![Expr::Str(l.clone())])), st, out)?;
                Ok(i + 1)
            }
            Action::GetUrl(u, t) => {
                self.emit(Stmt::Expr(Expr::Intrinsic("getURL".into(), vec![Expr::Str(u.clone()), Expr::Str(t.clone())])), st, out)?;
                Ok(i + 1)
            }
            Action::SetTarget(t) => {
                self.emit(Stmt::Expr(Expr::Intrinsic("__setTarget".into(), vec![Expr::Str(t.clone())])), st, out)?;
                Ok(i + 1)
            }
            Action::GetUrl2(m) => {
                let target = Self::pop(st)?;
                let url = Self::pop(st)?;
                self.emit(Stmt::Expr(Expr::Intrinsic("__getURL2".into(), vec![url, target, Expr::Int(*m as i32)])), st, out)?;
                Ok(i + 1)
            }
            Action::GotoFrame2(f, None) => {
                let frame = Self::pop(st)?;
                let name = if f & 1 != 0 { "gotoAndPlay" } else { "gotoAndStop" };
                self.emit(Stmt::Expr(Expr::Intrinsic(name.into(), vec![frame])), st, out)?;
                Ok(i + 1)
            }
            a => fail(format!("{a:?} is not decompiled")),
        }
    }

    fn function(&mut self, f: &Function, at: usize) -> Func {
        let pool = self.pool_at[at].clone();
        let mut fixed = HashMap::new();
        if let Some((_, flags)) = f.v2 {
            let mut r = 1u8;
            for (bit, name) in [(PRELOAD_THIS, "this"), (PRELOAD_ARGUMENTS, "arguments"), (PRELOAD_SUPER, "super"), (PRELOAD_ROOT, "_root"), (PRELOAD_PARENT, "_parent"), (PRELOAD_GLOBAL, "_global")] {
                if flags & bit != 0 {
                    fixed.insert(r, name.to_string());
                    r += 1;
                }
            }
        }
        let mut params = Vec::new();
        for (i, (r, name)) in f.params.iter().enumerate() {
            if *r != 0 {
                fixed.insert(*r, format!("{REG}p{i}"));
            }
            params.push(name.clone());
        }
        let mut d = Dec::new(&f.body, pool.clone(), fixed);
        if self.problems.is_some() {
            d.problems = Some(Vec::new());
        }
        let result = d.body();
        if let Some(p) = &mut self.problems {
            p.extend(d.problems.take().unwrap_or_default());
            match &result {
                Err(DErr::Fail(m)) => p.push(format!("function {:?}: {m}", f.name)),
                Err(DErr::Goto(t)) => p.push(format!("function {:?}: unstructured branch to action {t}", f.name)),
                Ok(_) => {}
            }
        }
        match result {
            Ok(mut body) => {
                finish_names(&mut body, &mut params, f.params.iter().enumerate().filter(|(_, (r, _))| *r != 0).map(|(i, _)| i).collect());
                prettify(&mut body);
                Func { name: f.name.clone(), params, body, raw: None }
            }
            Err(_) => Func {
                name: f.name.clone(),
                params,
                body: Vec::new(),
                raw: Some(RawBody { v2: f.v2, param_regs: f.params.iter().map(|(r, _)| *r).collect(), actions: resolve_consts(&f.body, &pool) }),
            },
        }
    }

    /// `If t` at `i`: a conditional statement, a `?:`, a `switch`, or a conditional `break`/`continue`.
    fn branch(&mut self, i: usize, t: usize, b: usize, st: &mut Vec<Expr>, out: &mut Vec<Stmt>) -> R<usize> {
        let x = Self::pop(st)?;
        let code = self.code;
        if let Expr::Binary(Bin::StrictEq, lhs, c1) = &x {
            if let Expr::Assign(target, value) = &**lhs {
                if t > i && t <= b {
                    return self.switch(i, t, (**target).clone(), (**value).clone(), (**c1).clone(), b, st, out);
                }
            }
        }
        if t <= i || t > b {
            let s = self.jump_stmt(t)?;
            self.emit(Stmt::If(x, vec![s], None), st, out)?;
            return Ok(i + 1);
        }
        if t > i + 1 {
            if let Action::Jump(t2) = code[t - 1] {
                if t2 > t && t2 <= b {
                    // `c ? a : b`: both arms push one value.
                    if let Ok(no) = self.expr_range(i + 1, t - 1) {
                        if let Ok(yes) = self.expr_range(t, t2) {
                            st.push(Expr::Cond(Box::new(x), Box::new(yes), Box::new(no)));
                            return Ok(t2);
                        }
                    }
                    let then = self.block(i + 1, t - 1)?;
                    let els = self.block(t, t2)?;
                    self.emit(Stmt::If(x.negate(), then, Some(els)), st, out)?;
                    return Ok(t2);
                }
            }
        }
        // The Flash compiler pops a statement's value after the label the `if` skips to.
        let mut inner = Vec::new();
        let mut then = Vec::new();
        self.run(i + 1, t, &mut inner, &mut then)?;
        let mut next = t;
        if inner.last().is_some_and(|e| !is_spent(e)) && t < b && matches!(code[t], Action::Op(0x17)) {
            next = t + 1;
        }
        Self::flush(&mut inner, &mut then);
        self.emit(Stmt::If(x.negate(), then, None), st, out)?;
        Ok(next)
    }

    #[allow(clippy::too_many_arguments)]
    fn switch(&mut self, i: usize, t1: usize, target: Expr, value: Expr, c1: Expr, b: usize, st: &mut [Expr], out: &mut Vec<Stmt>) -> R<usize> {
        let code = self.code;
        let mut cases = vec![(t1, c1)];
        let mut k = i + 1;
        let default_t = loop {
            if k >= b {
                return fail("switch without its final jump");
            }
            if let Action::Jump(d) = code[k] {
                k += 1;
                break d;
            }
            let m = (k..b).find(|&m| matches!(code[m], Action::If(_))).ok_or_else(|| DErr::Fail("switch case without a test".into()))?;
            let e = self.expr_range(k, m)?;
            let Action::If(tm) = code[m] else { unreachable!() };
            match e {
                Expr::Binary(Bin::StrictEq, a, c) if *a == target => cases.push((tm, *c)),
                _ => return fail("switch case is not a test of the switch value"),
            }
            k = m + 1;
        };
        let first = k;
        let mut labels: Vec<usize> = cases.iter().map(|(t, _)| *t).collect();
        labels.push(default_t);
        if labels.iter().any(|&l| l < first || l > b) {
            return fail("switch case outside the switch");
        }
        // The end: the last label, or further when `break`s jump past it.
        let mut end = *labels.iter().max().unwrap();
        loop {
            let further = (first..end).filter_map(|p| if let Action::Jump(t) = code[p] { Some(t) } else { None }).filter(|&t| t > end && t <= b).max();
            match further {
                Some(e) => end = e,
                None => break,
            }
        }
        let mut clauses: Vec<(usize, Option<Expr>)> = cases.into_iter().map(|(t, c)| (t, Some(c))).collect();
        if default_t < end {
            clauses.push((default_t, None));
        }
        clauses.sort_by_key(|(t, c)| (*t, c.is_none()));
        let c = Ctx { brk: end, cont: None, forin: false, is_loop: false };
        let mut out_clauses = Vec::new();
        for n in 0..clauses.len() {
            let start = clauses[n].0;
            let stop = clauses.get(n + 1).map(|c| c.0).unwrap_or(end);
            let body = self.with_ctx(c, |d| d.block(start, stop))?;
            out_clauses.push((clauses[n].1.clone(), body));
        }
        // The stored value and its tests are the switch's own.
        let tests = out_clauses.iter().filter(|(c, _)| c.is_some()).count();
        self.unuse(&target, tests);
        self.emit(Stmt::Switch(value, out_clauses), st, out)?;
        Ok(end)
    }

    fn op(&mut self, i: usize, op: u8, b: usize, st: &mut Vec<Expr>, out: &mut Vec<Stmt>) -> R<usize> {
        let code = self.code;
        let bin = |op: u8| -> Option<Bin> {
            Some(match op {
                0x47 => Bin::Add,
                0x0B => Bin::Sub,
                0x0C => Bin::Mul,
                0x0D => Bin::Div,
                0x3F => Bin::Mod,
                0x48 => Bin::Lt,
                0x67 => Bin::Gt,
                0x49 => Bin::Eq,
                0x66 => Bin::StrictEq,
                0x60 => Bin::BitAnd,
                0x61 => Bin::BitOr,
                0x62 => Bin::BitXor,
                0x63 => Bin::Shl,
                0x64 => Bin::Shr,
                0x65 => Bin::UShr,
                0x54 => Bin::InstanceOf,
                _ => return None,
            })
        };
        if let Some(bop) = bin(op) {
            let y = Self::pop(st)?;
            let x = Self::pop(st)?;
            let e = match (bop, &x, &y) {
                (Bin::Sub, Expr::Num(z), _) if *z == 0.0 && !z.is_sign_negative() => Expr::Unary(Un::Neg, Box::new(y)),
                (Bin::BitXor, _, Expr::Num(m)) if *m == 4294967295.0 => Expr::Unary(Un::BitNot, Box::new(x)),
                _ => Expr::Binary(bop, Box::new(x), Box::new(y)),
            };
            st.push(e);
            return Ok(i + 1);
        }
        let legacy = |op: u8| -> Option<&'static str> {
            Some(match op {
                0x0A => "__add",
                0x0F => "__less",
                0x0E => "__equals",
                0x10 => "__and",
                0x11 => "__or",
                0x13 => "__stringEquals",
                0x21 => "__stringAdd",
                0x29 => "__stringLess",
                0x68 => "__stringGreater",
                _ => return None,
            })
        };
        if let Some(name) = legacy(op) {
            let y = Self::pop(st)?;
            let x = Self::pop(st)?;
            st.push(Expr::Intrinsic(name.into(), vec![x, y]));
            return Ok(i + 1);
        }
        // Built-ins: (opcode, name, arguments, pushes).
        let builtin = |op: u8| -> Option<(&'static str, usize, bool)> {
            Some(match op {
                0x26 => ("trace", 1, false),
                0x34 => ("getTimer", 0, true),
                0x18 => ("int", 1, true),
                0x30 => ("random", 1, true),
                0x33 => ("chr", 1, true),
                0x32 => ("ord", 1, true),
                0x37 => ("mbchr", 1, true),
                0x36 => ("mbord", 1, true),
                0x14 => ("length", 1, true),
                0x31 => ("mblength", 1, true),
                0x15 => ("substring", 3, true),
                0x35 => ("mbsubstring", 3, true),
                0x4B => ("String", 1, true),
                0x4A => ("Number", 1, true),
                0x45 => ("targetPath", 1, true),
                0x22 => ("getProperty", 2, true),
                0x23 => ("setProperty", 3, false),
                0x24 => ("duplicateMovieClip", 3, false),
                0x25 => ("removeMovieClip", 1, false),
                0x07 => ("stop", 0, false),
                0x06 => ("play", 0, false),
                0x04 => ("nextFrame", 0, false),
                0x05 => ("prevFrame", 0, false),
                0x09 => ("stopAllSounds", 0, false),
                0x08 => ("toggleHighQuality", 0, false),
                0x28 => ("stopDrag", 0, false),
                0x50 => ("__increment", 1, true),
                0x51 => ("__decrement", 1, true),
                0x20 => ("__setTarget2", 1, false),
                _ => return None,
            })
        };
        if let Some((name, n, pushes)) = builtin(op) {
            let mut args = Self::pop_n(st, n)?;
            args.reverse();
            let e = Expr::Intrinsic(name.into(), args);
            if pushes {
                st.push(e);
            } else {
                self.emit(Stmt::Expr(e), st, out)?;
            }
            return Ok(i + 1);
        }
        match op {
            0x00 => Ok(b),
            0x12 => {
                let x = Self::pop(st)?;
                st.push(x.not_value());
                Ok(i + 1)
            }
            0x17 => {
                if let Some(e) = st.pop() {
                    self.emit(Stmt::Expr(e), st, out)?;
                }
                Ok(i + 1)
            }
            0x4C => {
                // `a || b`: PushDuplicate, If L, Pop, b, L:   `a && b`: PushDuplicate, Not, If L, Pop, b, L:
                let (and, at) = match (code.get(i + 1), code.get(i + 2), code.get(i + 3)) {
                    (Some(Action::If(t)), Some(Action::Op(0x17)), _) => (false, (*t, i + 3)),
                    (Some(Action::Op(0x12)), Some(Action::If(t)), Some(Action::Op(0x17))) => (true, (*t, i + 4)),
                    _ => return fail("PushDuplicate outside && / ||"),
                };
                let (t, from) = at;
                if t < from || t > b {
                    return fail("&& / || jumps out of its expression");
                }
                let lhs = Self::pop(st)?;
                let rhs = self.expr_range(from, t)?;
                st.push(if and { Expr::And(Box::new(lhs), Box::new(rhs)) } else { Expr::Or(Box::new(lhs), Box::new(rhs)) });
                Ok(t)
            }
            0x1C => {
                let n = Self::pop(st)?;
                st.push(by_name(n));
                Ok(i + 1)
            }
            0x1D => {
                let v = Self::pop(st)?;
                let n = Self::pop(st)?;
                let s = match by_name(n) {
                    Expr::Eval(n) => Expr::Intrinsic("set".into(), vec![*n, v]),
                    target => {
                        if let Some(next) = self.chain(i, &target, &v, st) {
                            return Ok(next);
                        }
                        assign_or_inc(target, v)
                    }
                };
                self.emit(Stmt::Expr(s), st, out)?;
                Ok(i + 1)
            }
            0x4E => {
                let n = Self::pop(st)?;
                let o = Self::pop(st)?;
                st.push(Expr::Member(Box::new(o), Box::new(n)));
                Ok(i + 1)
            }
            0x4F => {
                let v = Self::pop(st)?;
                let n = Self::pop(st)?;
                let o = Self::pop(st)?;
                let target = Expr::Member(Box::new(o), Box::new(n));
                if let Some(next) = self.chain(i, &target, &v, st) {
                    return Ok(next);
                }
                let s = assign_or_inc(target, v);
                self.emit(Stmt::Expr(s), st, out)?;
                Ok(i + 1)
            }
            0x3D => {
                let n = Self::pop(st)?;
                let count = num_arg(&Self::pop(st)?)?;
                let args = Self::pop_n(st, count)?;
                st.push(Expr::Call(Box::new(by_name(n)), args));
                Ok(i + 1)
            }
            0x52 | 0x53 => {
                let n = Self::pop(st)?;
                let o = Self::pop(st)?;
                let count = num_arg(&Self::pop(st)?)?;
                let args = Self::pop_n(st, count)?;
                let callee = match n {
                    Expr::Undefined => o,
                    Expr::Str(s) if s.is_empty() => o,
                    n => Expr::Member(Box::new(o), Box::new(n)),
                };
                st.push(if op == 0x52 { Expr::Call(Box::new(callee), args) } else { Expr::New(Box::new(callee), args) });
                Ok(i + 1)
            }
            0x40 => {
                let n = Self::pop(st)?;
                let count = num_arg(&Self::pop(st)?)?;
                let args = Self::pop_n(st, count)?;
                st.push(Expr::New(Box::new(by_name(n)), args));
                Ok(i + 1)
            }
            0x42 => {
                let count = num_arg(&Self::pop(st)?)?;
                let items = Self::pop_n(st, count)?;
                st.push(Expr::Array(items));
                Ok(i + 1)
            }
            0x43 => {
                let count = num_arg(&Self::pop(st)?)?;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    let v = Self::pop(st)?;
                    let k = Self::pop(st)?;
                    items.push((k, v));
                }
                // The pairs were pushed in source order.
                items.reverse();
                st.push(Expr::Object(items));
                Ok(i + 1)
            }
            0x44 => {
                let x = Self::pop(st)?;
                st.push(Expr::Unary(Un::TypeOf, Box::new(x)));
                Ok(i + 1)
            }
            0x3A => {
                let n = Self::pop(st)?;
                let o = Self::pop(st)?;
                st.push(Expr::Delete(Box::new(Expr::Member(Box::new(o), Box::new(n)))));
                Ok(i + 1)
            }
            0x3B => {
                let n = Self::pop(st)?;
                match by_name(n) {
                    v @ Expr::Var(_) => st.push(Expr::Delete(Box::new(v))),
                    _ => return fail("delete of a computed name"),
                }
                Ok(i + 1)
            }
            0x3C | 0x41 => {
                let v = if op == 0x3C { Some(Self::pop(st)?) } else { None };
                let n = Self::pop(st)?;
                match n {
                    Expr::Str(s) if is_ident(&s) && !is_reserved(&s) => self.emit(Stmt::Var(vec![(s, v)]), st, out)?,
                    _ => return fail("var with a computed name"),
                }
                Ok(i + 1)
            }
            0x3E => {
                let v = Self::pop(st)?;
                self.emit(Stmt::Return(Some(v)), st, out)?;
                Ok(i + 1)
            }
            0x2A => {
                let v = Self::pop(st)?;
                self.emit(Stmt::Throw(v), st, out)?;
                Ok(i + 1)
            }
            0x69 => {
                let sup = Self::pop(st)?;
                let sub = Self::pop(st)?;
                self.emit(Stmt::Expr(Expr::Intrinsic("__extends".into(), vec![sub, sup])), st, out)?;
                Ok(i + 1)
            }
            0x2C => {
                let ctor = Self::pop(st)?;
                let count = num_arg(&Self::pop(st)?)?;
                let mut args = vec![ctor];
                args.extend(Self::pop_n(st, count)?);
                self.emit(Stmt::Expr(Expr::Intrinsic("__implements".into(), args)), st, out)?;
                Ok(i + 1)
            }
            0x2B => {
                let obj = Self::pop(st)?;
                let ctor = Self::pop(st)?;
                st.push(Expr::Intrinsic("__cast".into(), vec![ctor, obj]));
                Ok(i + 1)
            }
            0x55 => self.forin(i, b, st, out),
            _ => fail(format!("{} is not decompiled", super::action::op_name(op).unwrap_or("?"))),
        }
    }

    /// `a = b = v` with a member or variable `b`: `v, StoreRegister r0, Set…(b), Push r0, Set…(a)`.
    /// Pushes `b = v` as the value of the next assignment and returns where it continues.
    fn chain(&mut self, i: usize, target: &Expr, v: &Expr, st: &mut Vec<Expr>) -> Option<usize> {
        if !matches!(self.code.get(i + 1), Some(Action::Push(p)) if p.as_slice() == [Value::Reg(0)]) || !matches!(v, Expr::Assign(..)) {
            return None;
        }
        let value = match v {
            Expr::Assign(t, val) if matches!(&**t, Expr::Var(n) if *n == reg_name(0)) && !self.fixed.contains_key(&0) => {
                self.unuse(t, 1);
                (**val).clone()
            }
            v => v.clone(),
        };
        st.push(Expr::Assign(Box::new(target.clone()), Box::new(value)));
        Some(i + 2)
    }

    /// `obj, Enumerate2, L: StoreRegister r0, Push null, Equals2, If E, <target = r0>, body, Jump L, E:`.
    fn forin(&mut self, i: usize, b: usize, st: &mut Vec<Expr>, out: &mut Vec<Stmt>) -> R<usize> {
        let code = self.code;
        let obj = Self::pop(st)?;
        let h = i + 1;
        let end = match code.get(h..h + 4) {
            Some([Action::StoreRegister(0), Action::Push(v), Action::Op(0x49), Action::If(e)]) if v.as_slice() == [Value::Null] => *e,
            _ => return fail("Enumerate2 outside for … in"),
        };
        if end > b || end < h + 5 {
            return fail("for … in without its loop");
        }
        // `break` jumps to a drain placed after the loop, which falls through to the exit.
        let (back, brk) = if end >= h + 9 && self.is_drain(end - 4) && matches!(code[end - 5], Action::Jump(x) if x == h) {
            (end - 5, end - 4)
        } else if matches!(code[end - 1], Action::Jump(x) if x == h) {
            (end - 1, end)
        } else {
            return fail("for … in without its loop");
        };
        let item = self.reg(0);
        let c = Ctx { brk, cont: Some(h), forin: true, is_loop: true };
        let mut body = self.with_ctx(c, |d| d.block(h + 4, back))?;
        if body.is_empty() {
            return fail("for … in without its variable");
        }
        let (target, declare) = match body.remove(0) {
            Stmt::Var(v) if v.len() == 1 && v[0].1.as_ref() == Some(&item) => (Expr::Var(v[0].0.clone()), true),
            Stmt::Expr(Expr::Assign(t, v)) if *v == item => (*t, false),
            _ => return fail("for … in does not assign its variable first"),
        };
        if contains_var(&body, &item) {
            return fail("for … in body reads register 0");
        }
        self.unuse(&item, 2);
        self.emit(Stmt::ForIn(target, declare, obj, body), st, out)?;
        Ok(end)
    }
}

fn spent() -> Expr {
    Expr::Intrinsic("\u{1}spent".into(), Vec::new())
}

fn is_spent(e: &Expr) -> bool {
    matches!(e, Expr::Intrinsic(n, a) if a.is_empty() && n == "\u{1}spent")
}

/// `target = v`, or `target++` when `v` is `__increment(target)`.
fn assign_or_inc(target: Expr, v: Expr) -> Expr {
    match v {
        Expr::Intrinsic(name, args) if (name == "__increment" || name == "__decrement") && args.len() == 1 && args[0] == target && pure(&target) => Expr::PostInc(Box::new(target), name == "__increment"),
        v => Expr::Assign(Box::new(target), Box::new(v)),
    }
}

/// Evaluating it twice is the same as once (names, members of names, literals).
fn pure(e: &Expr) -> bool {
    match e {
        Expr::Var(_) => true,
        Expr::Member(o, n) => pure(o) && pure(n),
        e => e.is_literal(),
    }
}

fn contains_var(stmts: &[Stmt], v: &Expr) -> bool {
    let mut found = false;
    visit_stmts(stmts, &mut |e| found |= e == v);
    found
}

/// Call `f` on every expression of the statements (not inside nested functions).
pub(super) fn visit_stmts(stmts: &[Stmt], f: &mut dyn FnMut(&Expr)) {
    for s in stmts {
        visit_stmt(s, f);
    }
}

fn visit_stmt(s: &Stmt, f: &mut dyn FnMut(&Expr)) {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => visit_expr(e, f),
        Stmt::Var(v) => v.iter().filter_map(|(_, e)| e.as_ref()).for_each(|e| visit_expr(e, f)),
        Stmt::If(c, a, b) => {
            visit_expr(c, f);
            visit_stmts(a, f);
            if let Some(b) = b {
                visit_stmts(b, f);
            }
        }
        Stmt::While(c, b) | Stmt::DoWhile(b, c) | Stmt::With(c, b) => {
            visit_expr(c, f);
            visit_stmts(b, f);
        }
        Stmt::For(i, c, s, b) => {
            if let Some(i) = i {
                visit_stmt(i, f);
            }
            if let Some(c) = c {
                visit_expr(c, f);
            }
            if let Some(s) = s {
                visit_stmt(s, f);
            }
            visit_stmts(b, f);
        }
        Stmt::ForIn(t, _, o, b) => {
            visit_expr(t, f);
            visit_expr(o, f);
            visit_stmts(b, f);
        }
        Stmt::Switch(x, cl) => {
            visit_expr(x, f);
            for (c, b) in cl {
                if let Some(c) = c {
                    visit_expr(c, f);
                }
                visit_stmts(b, f);
            }
        }
        Stmt::Try(a, c, fin) => {
            visit_stmts(a, f);
            if let Some((_, c)) = c {
                visit_stmts(c, f);
            }
            if let Some(fin) = fin {
                visit_stmts(fin, f);
            }
        }
        Stmt::Block(b) => visit_stmts(b, f),
        Stmt::Break | Stmt::Continue | Stmt::Return(None) | Stmt::Function(_) | Stmt::Raw(_) | Stmt::Class(_) => {}
    }
}

pub(super) fn visit_expr(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Eval(x) | Expr::Unary(_, x) | Expr::PreInc(x, _) | Expr::PostInc(x, _) | Expr::Delete(x) => visit_expr(x, f),
        Expr::Member(a, b) | Expr::Binary(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) | Expr::Assign(a, b) => {
            visit_expr(a, f);
            visit_expr(b, f);
        }
        Expr::Cond(a, b, c) => {
            visit_expr(a, f);
            visit_expr(b, f);
            visit_expr(c, f);
        }
        Expr::Call(c, args) | Expr::New(c, args) => {
            visit_expr(c, f);
            args.iter().for_each(|a| visit_expr(a, f));
        }
        Expr::Array(items) | Expr::Intrinsic(_, items) => items.iter().for_each(|a| visit_expr(a, f)),
        Expr::Object(items) => items.iter().for_each(|(k, v)| {
            visit_expr(k, f);
            visit_expr(v, f);
        }),
        _ => {}
    }
}

/// Every name the statements use, declare or leave free; with nested functions.
#[derive(Default)]
pub(super) struct Names {
    /// Read or written by name here.
    pub used: HashSet<String>,
    pub declared: HashSet<String>,
    /// Free in nested functions (they resolve through this scope).
    pub inner_free: HashSet<String>,
}

pub(super) fn collect_names(stmts: &[Stmt], n: &mut Names) {
    fn expr(e: &Expr, n: &mut Names) {
        let mut funcs = Vec::new();
        visit_expr(e, &mut |x| match x {
            Expr::Var(v) => {
                n.used.insert(v.clone());
            }
            Expr::Function(f) => funcs.push((**f).clone()),
            _ => {}
        });
        for f in funcs {
            n.inner_free.extend(free_names(&f));
        }
    }
    for s in stmts {
        match s {
            Stmt::Var(v) => {
                for (name, e) in v {
                    n.declared.insert(name.clone());
                    if let Some(e) = e {
                        expr(e, n);
                    }
                }
            }
            Stmt::Function(f) => {
                n.declared.insert(f.name.clone());
                n.inner_free.extend(free_names(f));
            }
            Stmt::Try(a, c, fin) => {
                collect_names(a, n);
                if let Some((name, c)) = c {
                    n.declared.insert(name.clone());
                    collect_names(c, n);
                }
                if let Some(fin) = fin {
                    collect_names(fin, n);
                }
            }
            Stmt::ForIn(t, declare, o, b) => {
                if *declare {
                    if let Expr::Var(v) = t {
                        n.declared.insert(v.clone());
                    }
                }
                expr(t, n);
                expr(o, n);
                collect_names(b, n);
            }
            Stmt::If(c, a, b) => {
                expr(c, n);
                collect_names(a, n);
                if let Some(b) = b {
                    collect_names(b, n);
                }
            }
            Stmt::While(c, b) | Stmt::DoWhile(b, c) | Stmt::With(c, b) => {
                expr(c, n);
                collect_names(b, n);
            }
            Stmt::For(i, c, s, b) => {
                if let Some(i) = i {
                    collect_names(std::slice::from_ref(&**i), n);
                }
                if let Some(c) = c {
                    expr(c, n);
                }
                if let Some(s) = s {
                    collect_names(std::slice::from_ref(&**s), n);
                }
                collect_names(b, n);
            }
            Stmt::Switch(x, cl) => {
                expr(x, n);
                for (c, b) in cl {
                    if let Some(c) = c {
                        expr(c, n);
                    }
                    collect_names(b, n);
                }
            }
            Stmt::Block(b) => collect_names(b, n),
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => expr(e, n),
            Stmt::Break | Stmt::Continue | Stmt::Return(None) | Stmt::Raw(_) => {}
            Stmt::Class(c) => {
                // What it compiles to declares and reads the same names.
                collect_names(&super::class::lower(c), n);
            }
        }
    }
}

/// Names a function reads through the scope chain (not its own parameters or locals).
pub(super) fn free_names(f: &Func) -> HashSet<String> {
    if let Some(raw) = &f.raw {
        // P-code reads names we do not track; its strings are a safe over-approximation.
        let mut out = HashSet::new();
        raw_strings(&raw.actions, &mut out);
        return out;
    }
    let mut n = Names::default();
    collect_names(&f.body, &mut n);
    let mut out: HashSet<String> = n.used.union(&n.inner_free).cloned().collect();
    for d in n.declared.iter().chain(f.params.iter()) {
        out.remove(d);
    }
    out
}

fn raw_strings(actions: &[Action], out: &mut HashSet<String>) {
    for a in actions {
        match a {
            Action::Push(v) => {
                for v in v {
                    if let Value::Str(s) = v {
                        out.insert(s.clone());
                    }
                }
            }
            Action::Function(f) => raw_strings(&f.body, out),
            _ => {}
        }
    }
}

/// Give the function's registers their final names: parameters keep their
/// own, locals become `_locN_`, renamed when a variable of that name is used
/// by name here or in a nested function.
fn finish_names(body: &mut [Stmt], params: &mut [String], reg_params: Vec<usize>) -> HashMap<String, String> {
    let mut n = Names::default();
    collect_names(body, &mut n);
    let mut taken: HashSet<String> = n.used.iter().chain(n.declared.iter()).filter(|s| !s.starts_with(REG)).cloned().collect();
    taken.extend(n.inner_free.iter().cloned());
    for (i, p) in params.iter().enumerate() {
        if !reg_params.contains(&i) {
            taken.insert(p.clone());
        }
    }
    let mut map: HashMap<String, String> = HashMap::new();
    let fresh = |want: String, taken: &mut HashSet<String>| -> String {
        let mut name = want.clone();
        let mut k = 2;
        while taken.contains(&name) || is_reserved(&name) || intrinsic(&name).is_some() {
            name = format!("{want}_{k}");
            k += 1;
        }
        taken.insert(name.clone());
        name
    };
    for &i in &reg_params {
        let name = fresh(params[i].clone(), &mut taken);
        map.insert(format!("{REG}p{i}"), name.clone());
        params[i] = name;
    }
    let mut regs: Vec<String> = n.used.iter().chain(n.declared.iter()).filter(|s| s.starts_with(REG) && !s[1..].starts_with('p')).cloned().collect();
    regs.sort_by_key(|s| s[1..].parse::<u32>().unwrap_or(0));
    regs.dedup();
    for r in regs {
        let name = fresh(format!("_loc{}_", &r[1..]), &mut taken);
        map.insert(r, name);
    }
    rename(body, &map);
    map
}

fn rename(stmts: &mut [Stmt], map: &HashMap<String, String>) {
    fn name(s: &mut String, map: &HashMap<String, String>) {
        if let Some(n) = map.get(s) {
            *s = n.clone();
        }
    }
    fn expr(e: &mut Expr, map: &HashMap<String, String>) {
        match e {
            Expr::Var(v) => name(v, map),
            Expr::Eval(x) | Expr::Unary(_, x) | Expr::PreInc(x, _) | Expr::PostInc(x, _) | Expr::Delete(x) => expr(x, map),
            Expr::Member(a, b) | Expr::Binary(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) | Expr::Assign(a, b) => {
                expr(a, map);
                expr(b, map);
            }
            Expr::Cond(a, b, c) => {
                expr(a, map);
                expr(b, map);
                expr(c, map);
            }
            Expr::Call(c, args) | Expr::New(c, args) => {
                expr(c, map);
                args.iter_mut().for_each(|a| expr(a, map));
            }
            Expr::Array(items) | Expr::Intrinsic(_, items) => items.iter_mut().for_each(|a| expr(a, map)),
            Expr::Object(items) => items.iter_mut().for_each(|(k, v)| {
                expr(k, map);
                expr(v, map);
            }),
            _ => {}
        }
    }
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => expr(e, map),
            Stmt::Var(v) => {
                for (n, e) in v {
                    name(n, map);
                    if let Some(e) = e {
                        expr(e, map);
                    }
                }
            }
            Stmt::If(c, a, b) => {
                expr(c, map);
                rename(a, map);
                if let Some(b) = b {
                    rename(b, map);
                }
            }
            Stmt::While(c, b) | Stmt::DoWhile(b, c) | Stmt::With(c, b) => {
                expr(c, map);
                rename(b, map);
            }
            Stmt::For(i, c, st, b) => {
                if let Some(i) = i {
                    rename(std::slice::from_mut(&mut **i), map);
                }
                if let Some(c) = c {
                    expr(c, map);
                }
                if let Some(st) = st {
                    rename(std::slice::from_mut(&mut **st), map);
                }
                rename(b, map);
            }
            Stmt::ForIn(t, _, o, b) => {
                expr(t, map);
                expr(o, map);
                rename(b, map);
            }
            Stmt::Switch(x, cl) => {
                expr(x, map);
                for (c, b) in cl {
                    if let Some(c) = c {
                        expr(c, map);
                    }
                    rename(b, map);
                }
            }
            Stmt::Try(a, c, f) => {
                rename(a, map);
                if let Some((n, c)) = c {
                    name(n, map);
                    rename(c, map);
                }
                if let Some(f) = f {
                    rename(f, map);
                }
            }
            Stmt::Block(b) => rename(b, map),
            Stmt::Break | Stmt::Continue | Stmt::Return(None) | Stmt::Function(_) | Stmt::Raw(_) | Stmt::Class(_) => {}
        }
    }
}

/// `init; while (c) { …; step; }` → `for (init; c; step) { … }` when the
/// loop has no `continue` (which would skip the step in a `for`), in every
/// block of the statements (not inside nested functions, done on their own).
fn prettify(stmts: &mut Vec<Stmt>) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::If(_, a, b) => {
                prettify(a);
                if let Some(b) = b {
                    prettify(b);
                }
            }
            Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::With(_, b) | Stmt::Block(b) | Stmt::ForIn(_, _, _, b) | Stmt::For(_, _, _, b) => prettify(b),
            Stmt::Switch(_, cl) => cl.iter_mut().for_each(|(_, b)| prettify(b)),
            Stmt::Try(a, c, f) => {
                prettify(a);
                if let Some((_, c)) = c {
                    prettify(c);
                }
                if let Some(f) = f {
                    prettify(f);
                }
            }
            _ => {}
        }
    }
    let mut i = 1;
    while i < stmts.len() {
        if let Some(var) = loop_var(&stmts[i - 1]) {
            if let Stmt::While(c, body) = &stmts[i] {
                let uses = {
                    let mut found = false;
                    visit_expr(c, &mut |e| found |= matches!(e, Expr::Var(n) if *n == var));
                    found
                };
                let steps = body.last().is_some_and(|l| updates(l, &var));
                if uses && steps && !has_continue(body) {
                    let init = stmts.remove(i - 1);
                    let Stmt::While(c, mut body) = stmts.remove(i - 1) else { unreachable!() };
                    let step = body.pop().unwrap();
                    stmts.insert(i - 1, Stmt::For(Some(Box::new(init)), Some(c), Some(Box::new(step)), body));
                    continue;
                }
            }
        }
        i += 1;
    }
}

/// The variable a statement initialises (`var i = …` / `i = …`).
fn loop_var(s: &Stmt) -> Option<String> {
    match s {
        Stmt::Var(v) if v.len() == 1 && v[0].1.is_some() => Some(v[0].0.clone()),
        Stmt::Expr(Expr::Assign(t, _)) => match &**t {
            Expr::Var(n) => Some(n.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn updates(s: &Stmt, var: &str) -> bool {
    match s {
        Stmt::Expr(Expr::PostInc(t, _) | Expr::PreInc(t, _) | Expr::Assign(t, _)) => matches!(&**t, Expr::Var(n) if n == var),
        _ => false,
    }
}

/// A `continue` of this loop (not of a loop nested in it).
fn has_continue(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Continue => true,
        Stmt::If(_, a, b) => has_continue(a) || b.as_ref().is_some_and(|b| has_continue(b)),
        Stmt::Block(b) | Stmt::With(_, b) => has_continue(b),
        Stmt::Switch(_, cl) => cl.iter().any(|(_, b)| has_continue(b)),
        Stmt::Try(a, c, f) => has_continue(a) || c.as_ref().is_some_and(|(_, c)| has_continue(c)) || f.as_ref().is_some_and(|f| has_continue(f)),
        _ => false,
    })
}
