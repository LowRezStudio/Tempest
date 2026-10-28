//! Syntax tree → bytecode, in the shapes the Flash compiler emits (so
//! `decompile` reads its output back): one constant pool per script,
//! `DefineFunction2` with parameters and locals in registers (unless a
//! nested function reads them, or the function uses `with` / `eval`),
//! `this` / `super` / `_global` … preloaded when used, `&&` / `||` with
//! `PushDuplicate`, `switch` on register 0, `for … in` with `Enumerate2`.
//!
//! At the top of a script, `var` declares timeline variables; the names
//! `_loc1_` … `_loc3_` (the decompiler's names for the global registers the
//! Flash compiler uses there) stay in registers.

use std::collections::{HashMap, HashSet};

use super::action::{Action, Catch, Function, Script, Try, Value, PRELOAD_ARGUMENTS, PRELOAD_GLOBAL, PRELOAD_PARENT, PRELOAD_ROOT, PRELOAD_SUPER, PRELOAD_THIS, SUPPRESS_ARGUMENTS, SUPPRESS_SUPER, SUPPRESS_THIS};
use super::ast::{intrinsic, Bin, Expr, Func, Stmt, Un};
use super::decompile::{collect_names, visit_stmts, Names};
use tempest_upk::io::{Error, Result};

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Format(msg.into()))
}

/// Compile ActionScript source into a script (an action stream ending in `End`).
pub fn compile(src: &str) -> Result<Script> {
    compile_stmts(&super::parse::parse(src)?)
}

pub fn compile_stmts(body: &[Stmt]) -> Result<Script> {
    let body = &super::class::lower_all(body)[..];
    let mut g = Gen { pool: Vec::new(), pool_index: HashMap::new(), pool_bytes: 2 };
    let scope = Scope::top(body);
    let mut f = FnState { code: Code::default(), scope, ctx: Vec::new() };
    g.stmts(&mut f, body)?;
    f.code.emit(Action::Op(0));
    let offset = usize::from(!g.pool.is_empty());
    let mut actions = f.code.finish(offset)?;
    if !g.pool.is_empty() {
        actions.insert(0, Action::ConstantPool(g.pool));
    }
    Ok(Script { actions, trailing: Vec::new() })
}

/// One block of actions under construction; branches name label ids until `finish`.
#[derive(Default)]
struct Code {
    actions: Vec<Action>,
    labels: Vec<Option<usize>>,
    /// A label points at the next action (it must not merge into the previous `Push`).
    label_at: Option<usize>,
}

impl Code {
    fn label(&mut self) -> usize {
        self.labels.push(None);
        self.labels.len() - 1
    }
    fn place(&mut self, l: usize) {
        self.labels[l] = Some(self.actions.len());
        self.label_at = Some(self.actions.len());
    }
    fn emit(&mut self, a: Action) {
        self.actions.push(a);
    }
    fn push(&mut self, v: Value) {
        if self.label_at != Some(self.actions.len()) {
            if let Some(Action::Push(items)) = self.actions.last_mut() {
                items.push(v);
                return;
            }
        }
        self.actions.push(Action::Push(vec![v]));
    }
    fn finish(self, offset: usize) -> Result<Vec<Action>> {
        let labels = self.labels;
        let at = |l: usize| -> Result<usize> { labels.get(l).copied().flatten().map(|i| i + offset).ok_or_else(|| Error::Format("internal: a label was never placed".into())) };
        let mut out = self.actions;
        for a in &mut out {
            match a {
                Action::Jump(l) | Action::If(l) | Action::With(l) => *l = at(*l)?,
                Action::Try(t) => {
                    t.catch_at = at(t.catch_at)?;
                    t.finally_at = at(t.finally_at)?;
                    t.end = at(t.end)?;
                }
                _ => {}
            }
        }
        Ok(out)
    }
}

/// Where names live in the function being compiled.
struct Scope {
    regs: HashMap<String, u8>,
    /// Locals kept as variables (read by nested functions, or the function is dynamic).
    vars: HashSet<String>,
}

const PRELOADS: [(&str, u16, u16); 6] = [
    ("this", PRELOAD_THIS, SUPPRESS_THIS),
    ("arguments", PRELOAD_ARGUMENTS, SUPPRESS_ARGUMENTS),
    ("super", PRELOAD_SUPER, SUPPRESS_SUPER),
    ("_root", PRELOAD_ROOT, 0),
    ("_parent", PRELOAD_PARENT, 0),
    ("_global", PRELOAD_GLOBAL, 0),
];

/// `var` names of a body in order of appearance (not inside nested functions).
fn declared_vars(stmts: &[Stmt], out: &mut Vec<String>) {
    for s in stmts {
        match s {
            Stmt::Var(v) => out.extend(v.iter().map(|(n, _)| n.clone())),
            Stmt::ForIn(Expr::Var(n), true, _, b) => {
                out.push(n.clone());
                declared_vars(b, out);
            }
            Stmt::If(_, a, b) => {
                declared_vars(a, out);
                if let Some(b) = b {
                    declared_vars(b, out);
                }
            }
            Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::With(_, b) | Stmt::Block(b) | Stmt::ForIn(_, _, _, b) => declared_vars(b, out),
            Stmt::For(i, _, _, b) => {
                if let Some(i) = i {
                    declared_vars(std::slice::from_ref(&**i), out);
                }
                declared_vars(b, out);
            }
            Stmt::Switch(_, cl) => cl.iter().for_each(|(_, b)| declared_vars(b, out)),
            Stmt::Try(a, c, f) => {
                declared_vars(a, out);
                if let Some((_, c)) = c {
                    declared_vars(c, out);
                }
                if let Some(f) = f {
                    declared_vars(f, out);
                }
            }
            _ => {}
        }
    }
}

/// `with`, `eval` or `set` read names the compiler cannot see: no register locals then.
fn dynamic(stmts: &[Stmt]) -> bool {
    let mut d = false;
    visit_stmts(stmts, &mut |e| match e {
        Expr::Eval(_) => d = true,
        Expr::Intrinsic(n, _) if n == "set" => d = true,
        Expr::Call(c, _) if matches!(&**c, Expr::Var(n) if n == "set") => d = true,
        _ => {}
    });
    fn has_with(stmts: &[Stmt]) -> bool {
        stmts.iter().any(|s| match s {
            Stmt::With(..) => true,
            Stmt::If(_, a, b) => has_with(a) || b.as_ref().is_some_and(|b| has_with(b)),
            Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::Block(b) | Stmt::ForIn(_, _, _, b) | Stmt::For(_, _, _, b) => has_with(b),
            Stmt::Switch(_, cl) => cl.iter().any(|(_, b)| has_with(b)),
            Stmt::Try(a, c, f) => has_with(a) || c.as_ref().is_some_and(|(_, c)| has_with(c)) || f.as_ref().is_some_and(|f| has_with(f)),
            _ => false,
        })
    }
    d || has_with(stmts)
}

/// `_locN_` → N.
fn loc_hint(name: &str) -> Option<u8> {
    name.strip_prefix("_loc")?.strip_suffix('_')?.parse().ok()
}

impl Scope {
    fn top(body: &[Stmt]) -> Scope {
        let mut n = Names::default();
        collect_names(body, &mut n);
        let mut declared = Vec::new();
        declared_vars(body, &mut declared);
        let mut regs = HashMap::new();
        for name in declared {
            if let Some(r) = loc_hint(&name).filter(|r| (1..=3).contains(r)) {
                if !n.inner_free.contains(&name) && !regs.values().any(|v| *v == r) {
                    regs.insert(name, r);
                }
            }
        }
        Scope { regs, vars: HashSet::new() }
    }

    /// Registers of a function: preloads, parameters, then locals. Returns
    /// the scope, the flags, the register count and the parameters' registers.
    fn function(f: &Func) -> (Scope, u16, u8, Vec<u8>) {
        let mut n = Names::default();
        collect_names(&f.body, &mut n);
        let mut declared = Vec::new();
        declared_vars(&f.body, &mut declared);
        let dynamic = dynamic(&f.body);
        let locals: HashSet<&String> = declared.iter().chain(f.params.iter()).collect();
        let mut regs = HashMap::new();
        let mut flags = 0u16;
        let mut next = 1u8;
        for (name, pre, sup) in PRELOADS {
            let name = name.to_string();
            if n.used.contains(&name) && !locals.contains(&name) {
                regs.insert(name, next);
                next += 1;
                flags |= pre;
            } else {
                flags |= sup;
            }
        }
        let mut vars = HashSet::new();
        let keep = |name: &String| dynamic || n.inner_free.contains(name);
        // `_locN_` locals take register N (the decompiler's names keep their registers), then the
        // parameters take the lowest free ones, then the other locals.
        let mut used: HashSet<u8> = regs.values().copied().collect();
        let params: HashSet<&String> = f.params.iter().collect();
        let mut rest = Vec::new();
        for name in &declared {
            if regs.contains_key(name) || vars.contains(name) || params.contains(name) {
                continue;
            }
            if keep(name) {
                vars.insert(name.clone());
                continue;
            }
            match loc_hint(name) {
                Some(r) if (1..255).contains(&r) && !used.contains(&r) => {
                    used.insert(r);
                    regs.insert(name.clone(), r);
                }
                _ => {
                    if !rest.contains(name) {
                        rest.push(name.clone());
                    }
                }
            }
        }
        let mut free = 1u8;
        let mut take = |used: &mut HashSet<u8>| -> Option<u8> {
            while used.contains(&free) {
                free = free.checked_add(1)?;
            }
            if free == 255 {
                return None;
            }
            used.insert(free);
            Some(free)
        };
        let mut param_regs = Vec::new();
        for p in &f.params {
            let r = if keep(p) || regs.contains_key(p) { None } else { take(&mut used) };
            match r {
                Some(r) => {
                    regs.insert(p.clone(), r);
                    param_regs.push(r);
                }
                None => {
                    vars.insert(p.clone());
                    param_regs.push(0);
                }
            }
        }
        for name in rest {
            match take(&mut used) {
                Some(r) => {
                    regs.insert(name, r);
                }
                None => {
                    vars.insert(name);
                }
            }
        }
        let count = used.iter().copied().max().map_or(1, |m| m + 1).max(next);
        (Scope { regs, vars }, flags, count, param_regs)
    }
}

#[derive(Clone, Copy)]
enum Ctx {
    /// `brk`, `cont`, and for `for … in` the drain label `break` goes to.
    Loop { brk: usize, cont: usize, drain: Option<usize> },
    Switch { brk: usize },
}

struct FnState {
    code: Code,
    scope: Scope,
    ctx: Vec<Ctx>,
}

struct Gen {
    pool: Vec<String>,
    pool_index: HashMap<String, u16>,
    pool_bytes: usize,
}

/// The binary operator's opcode and whether `Not` follows.
fn bin_op(op: Bin) -> (u8, bool) {
    match op {
        Bin::Add => (0x47, false),
        Bin::Sub => (0x0B, false),
        Bin::Mul => (0x0C, false),
        Bin::Div => (0x0D, false),
        Bin::Mod => (0x3F, false),
        Bin::Lt => (0x48, false),
        Bin::Gt => (0x67, false),
        Bin::Le => (0x67, true),
        Bin::Ge => (0x48, true),
        Bin::Eq => (0x49, false),
        Bin::Ne => (0x49, true),
        Bin::StrictEq => (0x66, false),
        Bin::StrictNe => (0x66, true),
        Bin::BitAnd => (0x60, false),
        Bin::BitOr => (0x61, false),
        Bin::BitXor => (0x62, false),
        Bin::Shl => (0x63, false),
        Bin::Shr => (0x64, false),
        Bin::UShr => (0x65, false),
        Bin::InstanceOf => (0x54, false),
    }
}

/// Intrinsics that are one opcode over their arguments in order.
fn simple_intrinsic(name: &str) -> Option<u8> {
    Some(match name {
        "trace" => 0x26,
        "getTimer" => 0x34,
        "int" => 0x18,
        "random" => 0x30,
        "chr" => 0x33,
        "ord" => 0x32,
        "mbchr" => 0x37,
        "mbord" => 0x36,
        "length" => 0x14,
        "mblength" => 0x31,
        "substring" => 0x15,
        "mbsubstring" => 0x35,
        "String" => 0x4B,
        "Number" => 0x4A,
        "targetPath" => 0x45,
        "getProperty" => 0x22,
        "setProperty" => 0x23,
        "duplicateMovieClip" => 0x24,
        "removeMovieClip" => 0x25,
        "stop" => 0x07,
        "play" => 0x06,
        "nextFrame" => 0x04,
        "prevFrame" => 0x05,
        "stopAllSounds" => 0x09,
        "toggleHighQuality" => 0x08,
        "stopDrag" => 0x28,
        "__cast" => 0x2B,
        "__extends" => 0x69,
        "__add" => 0x0A,
        "__less" => 0x0F,
        "__equals" => 0x0E,
        "__and" => 0x10,
        "__or" => 0x11,
        "__stringEquals" => 0x13,
        "__stringAdd" => 0x21,
        "__stringLess" => 0x29,
        "__stringGreater" => 0x68,
        "__increment" => 0x50,
        "__decrement" => 0x51,
        "__setTarget2" => 0x20,
        _ => return None,
    })
}

fn number(n: f64) -> Value {
    if n == 0.0 && !n.is_sign_negative() {
        Value::Double(0.0)
    } else if n.fract() == 0.0 && n >= i32::MIN as f64 && n <= i32::MAX as f64 && !(n == 0.0) {
        Value::Int(n as i32)
    } else {
        Value::Double(n)
    }
}

fn count(n: usize) -> Value {
    number(n as f64)
}

impl Gen {
    fn string(&mut self, f: &mut FnState, s: &str) {
        let v = match self.pool_index.get(s) {
            Some(&i) => Value::Const(i, false),
            None if self.pool.len() < 65535 && self.pool_bytes + s.len() + 1 < 65000 => {
                let i = self.pool.len() as u16;
                self.pool.push(s.to_string());
                self.pool_index.insert(s.to_string(), i);
                self.pool_bytes += s.len() + 1;
                Value::Const(i, false)
            }
            None => Value::Str(s.to_string()),
        };
        f.code.push(v);
    }

    fn op(&mut self, f: &mut FnState, op: u8) {
        f.code.emit(Action::Op(op));
    }

    fn reg_of(&self, f: &FnState, name: &str) -> Option<u8> {
        f.scope.regs.get(name).copied()
    }

    /// Is `name` a built-in here (not shadowed by a local)?
    fn is_intrinsic(&self, f: &FnState, name: &str) -> bool {
        intrinsic(name).is_some() && !f.scope.regs.contains_key(name) && !f.scope.vars.contains(name)
    }

    fn stmts(&mut self, f: &mut FnState, body: &[Stmt]) -> Result<()> {
        for s in body {
            self.stmt(f, s)?;
        }
        Ok(())
    }

    fn cond_jump_false(&mut self, f: &mut FnState, c: &Expr, target: usize) -> Result<()> {
        self.expr(f, c)?;
        self.op(f, 0x12);
        f.code.emit(Action::If(target));
        Ok(())
    }

    fn stmt(&mut self, f: &mut FnState, s: &Stmt) -> Result<()> {
        match s {
            Stmt::Expr(e) => self.effect(f, e),
            Stmt::Var(vars) => {
                for (name, init) in vars {
                    if let Some(r) = self.reg_of(f, name) {
                        if let Some(v) = init {
                            self.expr(f, v)?;
                            f.code.emit(Action::StoreRegister(r));
                            self.op(f, 0x17);
                        }
                    } else {
                        self.string(f, name);
                        match init {
                            Some(v) => {
                                self.expr(f, v)?;
                                self.op(f, 0x3C);
                            }
                            None => self.op(f, 0x41),
                        }
                    }
                }
                Ok(())
            }
            Stmt::If(c, then, els) => {
                let l_else = f.code.label();
                self.cond_jump_false(f, c, l_else)?;
                self.stmts(f, then)?;
                match els {
                    Some(e) => {
                        let l_end = f.code.label();
                        f.code.emit(Action::Jump(l_end));
                        f.code.place(l_else);
                        self.stmts(f, e)?;
                        f.code.place(l_end);
                    }
                    None => f.code.place(l_else),
                }
                Ok(())
            }
            Stmt::While(c, body) => {
                let (start, end) = (f.code.label(), f.code.label());
                f.code.place(start);
                if *c != Expr::Bool(true) {
                    self.cond_jump_false(f, c, end)?;
                }
                f.ctx.push(Ctx::Loop { brk: end, cont: start, drain: None });
                self.stmts(f, body)?;
                f.ctx.pop();
                f.code.emit(Action::Jump(start));
                f.code.place(end);
                Ok(())
            }
            Stmt::DoWhile(body, c) => {
                let (start, cond, end) = (f.code.label(), f.code.label(), f.code.label());
                f.code.place(start);
                f.ctx.push(Ctx::Loop { brk: end, cont: cond, drain: None });
                self.stmts(f, body)?;
                f.ctx.pop();
                f.code.place(cond);
                self.expr(f, c)?;
                f.code.emit(Action::If(start));
                f.code.place(end);
                Ok(())
            }
            Stmt::For(init, c, step, body) => {
                if let Some(i) = init {
                    self.stmt(f, i)?;
                }
                let (start, cont, end) = (f.code.label(), f.code.label(), f.code.label());
                f.code.place(start);
                if let Some(c) = c {
                    self.cond_jump_false(f, c, end)?;
                }
                f.ctx.push(Ctx::Loop { brk: end, cont, drain: None });
                self.stmts(f, body)?;
                f.ctx.pop();
                f.code.place(cont);
                if let Some(s) = step {
                    self.stmt(f, s)?;
                }
                f.code.emit(Action::Jump(start));
                f.code.place(end);
                Ok(())
            }
            Stmt::ForIn(target, declare, obj, body) => {
                self.expr(f, obj)?;
                self.op(f, 0x55);
                let (head, end, drain) = (f.code.label(), f.code.label(), f.code.label());
                f.code.place(head);
                f.code.emit(Action::StoreRegister(0));
                f.code.push(Value::Null);
                self.op(f, 0x49);
                f.code.emit(Action::If(end));
                self.store_from_reg0(f, target, *declare)?;
                let mut uses_break = false;
                f.ctx.push(Ctx::Loop { brk: end, cont: head, drain: Some(drain) });
                let r = self.stmts(f, body);
                f.ctx.pop();
                r?;
                if f.code.actions.iter().any(|a| matches!(a, Action::Jump(l) if *l == drain)) {
                    uses_break = true;
                }
                f.code.emit(Action::Jump(head));
                if uses_break {
                    f.code.place(drain);
                    f.code.push(Value::Null);
                    self.op(f, 0x49);
                    self.op(f, 0x12);
                    f.code.emit(Action::If(drain));
                } else {
                    f.code.place(drain);
                }
                f.code.place(end);
                Ok(())
            }
            Stmt::Switch(x, clauses) => {
                let end = f.code.label();
                let labels: Vec<usize> = clauses.iter().map(|_| f.code.label()).collect();
                self.expr(f, x)?;
                f.code.emit(Action::StoreRegister(0));
                let mut first = true;
                let mut default = None;
                for (i, (c, _)) in clauses.iter().enumerate() {
                    let Some(c) = c else {
                        default = Some(labels[i]);
                        continue;
                    };
                    if !first {
                        f.code.push(Value::Reg(0));
                    }
                    first = false;
                    self.expr(f, c)?;
                    self.op(f, 0x66);
                    f.code.emit(Action::If(labels[i]));
                }
                if first {
                    // Only a default: the value is not compared.
                    self.op(f, 0x17);
                }
                f.code.emit(Action::Jump(default.unwrap_or(end)));
                f.ctx.push(Ctx::Switch { brk: end });
                for (i, (_, body)) in clauses.iter().enumerate() {
                    f.code.place(labels[i]);
                    self.stmts(f, body)?;
                }
                f.ctx.pop();
                f.code.place(end);
                Ok(())
            }
            Stmt::Break => {
                let target = match f.ctx.last() {
                    Some(Ctx::Loop { drain: Some(d), .. }) => *d,
                    Some(Ctx::Loop { brk, .. }) | Some(Ctx::Switch { brk }) => *brk,
                    None => return err("break outside a loop or switch"),
                };
                f.code.emit(Action::Jump(target));
                Ok(())
            }
            Stmt::Continue => {
                let target = f.ctx.iter().rev().find_map(|c| if let Ctx::Loop { cont, .. } = c { Some(*cont) } else { None }).ok_or_else(|| Error::Format("continue outside a loop".into()))?;
                f.code.emit(Action::Jump(target));
                Ok(())
            }
            Stmt::Return(e) => {
                // Pop what every enclosing `for … in` still has on the stack.
                let drains = f.ctx.iter().filter(|c| matches!(c, Ctx::Loop { drain: Some(_), .. })).count();
                for _ in 0..drains {
                    let l = f.code.label();
                    f.code.place(l);
                    f.code.push(Value::Null);
                    self.op(f, 0x49);
                    self.op(f, 0x12);
                    f.code.emit(Action::If(l));
                }
                match e {
                    Some(e) => self.expr(f, e)?,
                    None => f.code.push(Value::Undefined),
                }
                self.op(f, 0x3E);
                Ok(())
            }
            Stmt::Throw(e) => {
                self.expr(f, e)?;
                self.op(f, 0x2A);
                Ok(())
            }
            Stmt::Try(body, catch, finally) => {
                let (c_at, f_at, end) = (f.code.label(), f.code.label(), f.code.label());
                let mut flags = 0u8;
                if catch.is_some() {
                    flags |= 1;
                }
                if finally.is_some() {
                    flags |= 2;
                }
                let name = catch.as_ref().map(|(n, _)| n.clone()).unwrap_or_default();
                f.code.emit(Action::Try(Box::new(Try { flags, catch: Catch::Name(name), catch_at: c_at, finally_at: f_at, end })));
                self.stmts(f, body)?;
                if let Some((_, c)) = catch {
                    f.code.emit(Action::Jump(f_at));
                    f.code.place(c_at);
                    self.stmts(f, c)?;
                } else {
                    f.code.place(c_at);
                }
                f.code.place(f_at);
                if let Some(fin) = finally {
                    self.stmts(f, fin)?;
                }
                f.code.place(end);
                Ok(())
            }
            Stmt::With(o, body) => {
                let end = f.code.label();
                self.expr(f, o)?;
                f.code.emit(Action::With(end));
                self.stmts(f, body)?;
                f.code.place(end);
                Ok(())
            }
            Stmt::Function(func) => {
                let a = self.function(func)?;
                f.code.emit(a);
                Ok(())
            }
            Stmt::Block(b) => self.stmts(f, b),
            Stmt::Class(_) => err("a class can only be defined at the top of a script"),
            Stmt::Raw(actions) => {
                // Branches inside the P-code index its own actions: give each target a label.
                let base = f.code.actions.len();
                f.code.label_at = Some(base);
                let mut ids = HashMap::new();
                let mut id = |f: &mut FnState, t: usize| -> usize {
                    *ids.entry(t).or_insert_with(|| {
                        let l = f.code.label();
                        f.code.labels[l] = Some(base + t);
                        l
                    })
                };
                for a in actions {
                    let a = match a {
                        Action::Jump(t) => Action::Jump(id(f, *t)),
                        Action::If(t) => Action::If(id(f, *t)),
                        Action::With(t) => Action::With(id(f, *t)),
                        Action::Try(t) => {
                            let mut t = (**t).clone();
                            t.catch_at = id(f, t.catch_at);
                            t.finally_at = id(f, t.finally_at);
                            t.end = id(f, t.end);
                            Action::Try(Box::new(t))
                        }
                        Action::Op(0) => continue,
                        a => a.clone(),
                    };
                    f.code.emit(a);
                }
                f.code.label_at = Some(f.code.actions.len());
                Ok(())
            }
        }
    }

    /// The target of `for … in` gets register 0.
    fn store_from_reg0(&mut self, f: &mut FnState, target: &Expr, declare: bool) -> Result<()> {
        match target {
            Expr::Var(n) => {
                if let Some(r) = self.reg_of(f, n) {
                    f.code.push(Value::Reg(0));
                    f.code.emit(Action::StoreRegister(r));
                    self.op(f, 0x17);
                } else {
                    self.string(f, n);
                    f.code.push(Value::Reg(0));
                    self.op(f, if declare { 0x3C } else { 0x1D });
                }
                Ok(())
            }
            Expr::Member(o, n) => {
                self.expr(f, o)?;
                self.expr(f, n)?;
                f.code.push(Value::Reg(0));
                self.op(f, 0x4F);
                Ok(())
            }
            _ => err("for … in needs a variable or property"),
        }
    }

    /// An expression evaluated for its effect (nothing left on the stack).
    fn effect(&mut self, f: &mut FnState, e: &Expr) -> Result<()> {
        match e {
            Expr::Assign(t, v) => self.assign(f, t, v, false),
            Expr::PreInc(t, inc) | Expr::PostInc(t, inc) => self.increment(f, t, *inc, false),
            Expr::Intrinsic(n, items) if n == "__comma" => {
                for i in items {
                    self.effect(f, i)?;
                }
                Ok(())
            }
            Expr::Call(c, args) if matches!(&**c, Expr::Var(n) if self.is_intrinsic(f, n) && !intrinsic(n).unwrap().1) => {
                let Expr::Var(n) = &**c else { unreachable!() };
                self.intrinsic(f, n, args, false)
            }
            Expr::Intrinsic(n, args) if intrinsic(n).is_some_and(|i| !i.1) => self.intrinsic(f, n, args, false),
            e => {
                self.expr(f, e)?;
                self.op(f, 0x17);
                Ok(())
            }
        }
    }

    /// `target = v`; `value`: leave the assigned value on the stack.
    fn assign(&mut self, f: &mut FnState, t: &Expr, v: &Expr, value: bool) -> Result<()> {
        // `t = t + 1` written by the decompiler as `__increment(t)` is an increment.
        match t {
            Expr::Var(n) if self.reg_of(f, n).is_some() => {
                let r = self.reg_of(f, n).unwrap();
                self.expr(f, v)?;
                f.code.emit(Action::StoreRegister(r));
                if !value {
                    self.op(f, 0x17);
                }
                Ok(())
            }
            Expr::Var(n) => {
                self.string(f, n);
                self.expr(f, v)?;
                self.set_tail(f, 0x1D, value);
                Ok(())
            }
            Expr::Eval(n) => {
                self.expr(f, n)?;
                self.expr(f, v)?;
                self.set_tail(f, 0x1D, value);
                Ok(())
            }
            Expr::Member(o, n) => {
                self.expr(f, o)?;
                self.expr(f, n)?;
                self.expr(f, v)?;
                self.set_tail(f, 0x4F, value);
                Ok(())
            }
            _ => err("cannot assign to this expression"),
        }
    }

    /// `SetVariable` / `SetMember`, keeping the value in register 0 when it is needed.
    fn set_tail(&mut self, f: &mut FnState, op: u8, value: bool) {
        if value {
            f.code.emit(Action::StoreRegister(0));
            self.op(f, op);
            f.code.push(Value::Reg(0));
        } else {
            self.op(f, op);
        }
    }

    /// `++t`, `t++` (as a statement when `!value`; `post` only matters for the value).
    fn increment(&mut self, f: &mut FnState, t: &Expr, inc: bool, value: bool) -> Result<()> {
        self.increment_full(f, t, inc, value, false)
    }

    fn increment_full(&mut self, f: &mut FnState, t: &Expr, inc: bool, value: bool, post: bool) -> Result<()> {
        let op = if inc { 0x50 } else { 0x51 };
        match t {
            Expr::Var(n) if self.reg_of(f, n).is_some() => {
                let r = self.reg_of(f, n).unwrap();
                f.code.push(Value::Reg(r));
                if value && post {
                    f.code.push(Value::Reg(r));
                }
                self.op(f, op);
                f.code.emit(Action::StoreRegister(r));
                if !value || post {
                    self.op(f, 0x17);
                }
                Ok(())
            }
            Expr::Var(_) | Expr::Member(..) => {
                if value && post {
                    // The old value stays below the update.
                    self.expr(f, t)?;
                }
                match t {
                    Expr::Var(n) => self.string(f, n),
                    Expr::Member(o, n) => {
                        self.expr(f, o)?;
                        self.expr(f, n)?;
                    }
                    _ => unreachable!(),
                }
                self.expr(f, t)?;
                self.op(f, op);
                let set = if matches!(t, Expr::Var(_)) { 0x1D } else { 0x4F };
                self.set_tail(f, set, value && !post);
                Ok(())
            }
            _ => err("++ / -- needs a variable or property"),
        }
    }

    fn args_reversed(&mut self, f: &mut FnState, args: &[Expr]) -> Result<()> {
        for a in args.iter().rev() {
            self.expr(f, a)?;
        }
        f.code.push(count(args.len()));
        Ok(())
    }

    fn intrinsic(&mut self, f: &mut FnState, name: &str, args: &[Expr], value: bool) -> Result<()> {
        let (arity, pushes) = intrinsic(name).unwrap();
        if arity != 255 && args.len() != arity as usize {
            return err(format!("{name} takes {arity} argument{}", if arity == 1 { "" } else { "s" }));
        }
        match name {
            "eval" => {
                self.expr(f, &args[0])?;
                self.op(f, 0x1C);
            }
            "set" => {
                self.expr(f, &args[0])?;
                self.expr(f, &args[1])?;
                self.op(f, 0x1D);
            }
            "gotoAndPlay" | "gotoAndStop" => {
                self.expr(f, &args[0])?;
                f.code.emit(Action::GotoFrame2(u8::from(name == "gotoAndPlay"), None));
            }
            "getURL" => match (&args[0], &args[1]) {
                (Expr::Str(u), Expr::Str(t)) => f.code.emit(Action::GetUrl(u.clone(), t.clone())),
                (u, t) => {
                    self.expr(f, u)?;
                    self.expr(f, t)?;
                    f.code.emit(Action::GetUrl2(0));
                }
            },
            "__getURL2" => {
                let Expr::Int(m) = args[2] else { return err("__getURL2 needs a constant method") };
                self.expr(f, &args[0])?;
                self.expr(f, &args[1])?;
                f.code.emit(Action::GetUrl2(m as u8));
            }
            "__gotoFrame" => {
                let Expr::Int(n) = args[0] else { return err("__gotoFrame needs a constant frame") };
                f.code.emit(Action::GotoFrame(n as u16));
            }
            "__gotoLabel" | "__setTarget" => {
                let Expr::Str(s) = &args[0] else { return err(format!("{name} needs a string")) };
                f.code.emit(if name == "__gotoLabel" { Action::GoToLabel(s.clone()) } else { Action::SetTarget(s.clone()) });
            }
            "__implements" => {
                if args.is_empty() {
                    return err("__implements needs a class");
                }
                for a in args[1..].iter().rev() {
                    self.expr(f, a)?;
                }
                f.code.push(count(args.len() - 1));
                self.expr(f, &args[0])?;
                self.op(f, 0x2C);
            }
            "__swap" | "__call" | "__fscommand2" | "__startDrag" => return err(format!("{name} is not supported")),
            _ => {
                let op = simple_intrinsic(name).ok_or_else(|| Error::Format(format!("{name} is not supported")))?;
                for a in args {
                    self.expr(f, a)?;
                }
                self.op(f, op);
            }
        }
        if value && !pushes {
            f.code.push(Value::Undefined);
        } else if !value && pushes {
            self.op(f, 0x17);
        }
        Ok(())
    }

    /// Push the value of `e`.
    fn expr(&mut self, f: &mut FnState, e: &Expr) -> Result<()> {
        match e {
            Expr::Undefined => f.code.push(Value::Undefined),
            Expr::Null => f.code.push(Value::Null),
            Expr::Bool(b) => f.code.push(Value::Bool(*b)),
            Expr::Int(i) => f.code.push(number(*i as f64)),
            Expr::Num(n) => f.code.push(number(*n)),
            Expr::Str(s) => self.string(f, s),
            Expr::Var(n) => match self.reg_of(f, n) {
                Some(r) => f.code.push(Value::Reg(r)),
                None => {
                    self.string(f, n);
                    self.op(f, 0x1C);
                }
            },
            Expr::Eval(n) => {
                self.expr(f, n)?;
                self.op(f, 0x1C);
            }
            Expr::Member(o, n) => {
                self.expr(f, o)?;
                self.expr(f, n)?;
                self.op(f, 0x4E);
            }
            Expr::Call(callee, args) => {
                if let Expr::Var(n) = &**callee {
                    if self.is_intrinsic(f, n) {
                        return self.intrinsic(f, n, args, true);
                    }
                }
                self.args_reversed(f, args)?;
                match &**callee {
                    Expr::Var(n) if self.reg_of(f, n).is_none() => {
                        self.string(f, n);
                        self.op(f, 0x3D);
                    }
                    Expr::Eval(n) => {
                        self.expr(f, n)?;
                        self.op(f, 0x3D);
                    }
                    Expr::Member(o, n) => {
                        self.expr(f, o)?;
                        self.expr(f, n)?;
                        self.op(f, 0x52);
                    }
                    c => {
                        self.expr(f, c)?;
                        f.code.push(Value::Undefined);
                        self.op(f, 0x52);
                    }
                }
            }
            Expr::New(callee, args) => {
                self.args_reversed(f, args)?;
                match &**callee {
                    Expr::Var(n) if self.reg_of(f, n).is_none() => {
                        self.string(f, n);
                        self.op(f, 0x40);
                    }
                    Expr::Eval(n) => {
                        self.expr(f, n)?;
                        self.op(f, 0x40);
                    }
                    Expr::Member(o, n) => {
                        self.expr(f, o)?;
                        self.expr(f, n)?;
                        self.op(f, 0x53);
                    }
                    c => {
                        self.expr(f, c)?;
                        f.code.push(Value::Undefined);
                        self.op(f, 0x53);
                    }
                }
            }
            Expr::Unary(op, x) => match op {
                Un::Not => {
                    self.expr(f, x)?;
                    self.op(f, 0x12);
                }
                Un::Neg => {
                    f.code.push(Value::Double(0.0));
                    self.expr(f, x)?;
                    self.op(f, 0x0B);
                }
                Un::BitNot => {
                    self.expr(f, x)?;
                    f.code.push(Value::Double(4294967295.0));
                    self.op(f, 0x62);
                }
                Un::TypeOf => {
                    self.expr(f, x)?;
                    self.op(f, 0x44);
                }
            },
            Expr::Binary(op, a, b) => {
                self.expr(f, a)?;
                self.expr(f, b)?;
                let (code, not) = bin_op(*op);
                self.op(f, code);
                if not {
                    self.op(f, 0x12);
                }
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                let end = f.code.label();
                self.expr(f, a)?;
                self.op(f, 0x4C);
                if matches!(e, Expr::And(..)) {
                    self.op(f, 0x12);
                }
                f.code.emit(Action::If(end));
                self.op(f, 0x17);
                self.expr(f, b)?;
                f.code.place(end);
            }
            Expr::Cond(c, a, b) => {
                let (yes, end) = (f.code.label(), f.code.label());
                self.expr(f, c)?;
                f.code.emit(Action::If(yes));
                self.expr(f, b)?;
                f.code.emit(Action::Jump(end));
                f.code.place(yes);
                self.expr(f, a)?;
                f.code.place(end);
            }
            Expr::Assign(t, v) => self.assign(f, t, v, true)?,
            Expr::PreInc(t, inc) => self.increment_full(f, t, *inc, true, false)?,
            Expr::PostInc(t, inc) => self.increment_full(f, t, *inc, true, true)?,
            Expr::Array(items) => {
                self.args_reversed(f, items)?;
                self.op(f, 0x42);
            }
            Expr::Object(items) => {
                for (k, v) in items {
                    self.expr(f, k)?;
                    self.expr(f, v)?;
                }
                f.code.push(count(items.len()));
                self.op(f, 0x43);
            }
            Expr::Function(func) => {
                let a = self.function(func)?;
                f.code.emit(a);
            }
            Expr::Delete(t) => match &**t {
                Expr::Member(o, n) => {
                    self.expr(f, o)?;
                    self.expr(f, n)?;
                    self.op(f, 0x3A);
                }
                Expr::Var(n) => {
                    self.string(f, n);
                    self.op(f, 0x3B);
                }
                _ => return err("delete needs a variable or property"),
            },
            Expr::Intrinsic(n, items) if n == "__comma" => {
                let (last, rest) = items.split_last().ok_or_else(|| Error::Format("empty comma expression".into()))?;
                for i in rest {
                    self.effect(f, i)?;
                }
                self.expr(f, last)?;
            }
            Expr::Intrinsic(n, items) if n == "__void" => {
                self.effect(f, &items[0])?;
                f.code.push(Value::Undefined);
            }
            Expr::Intrinsic(n, args) => {
                if intrinsic(n).is_none() {
                    return err(format!("unknown built-in {n}"));
                }
                self.intrinsic(f, n, args, true)?;
            }
        }
        Ok(())
    }

    fn function(&mut self, func: &Func) -> Result<Action> {
        if let Some(raw) = &func.raw {
            let params = raw.param_regs.iter().copied().chain(std::iter::repeat(0)).zip(func.params.iter().cloned()).collect();
            return Ok(Action::Function(Box::new(Function { name: func.name.clone(), v2: raw.v2, params, body: raw.actions.clone() })));
        }
        if func.body.is_empty() {
            return Ok(Action::Function(Box::new(Function { name: func.name.clone(), v2: None, params: func.params.iter().map(|p| (0, p.clone())).collect(), body: Vec::new() })));
        }
        let (scope, flags, regs, param_regs) = Scope::function(func);
        let mut inner = FnState { code: Code::default(), scope, ctx: Vec::new() };
        self.stmts(&mut inner, &func.body)?;
        let body = inner.code.finish(0)?;
        let params = param_regs.into_iter().zip(func.params.iter().cloned()).collect();
        Ok(Action::Function(Box::new(Function { name: func.name.clone(), v2: Some((regs, flags)), params, body })))
    }
}
