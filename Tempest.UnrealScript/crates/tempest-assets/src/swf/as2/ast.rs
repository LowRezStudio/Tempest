//! The ActionScript 2 syntax tree shared by the decompiler (bytecode →
//! tree → text) and the compiler (text → tree → bytecode).
//!
//! Opcodes without an ActionScript spelling get `__name(…)` intrinsics
//! (`__cast(Type, value)` for `CastOp`, `__extends(Sub, Super)`, …) so a
//! decompiled script compiles back to the same operations; a function body
//! the decompiler cannot structure is kept as P-code (`Func::raw`).

use super::action::Action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    InstanceOf,
}

impl Bin {
    pub fn text(self) -> &'static str {
        match self {
            Bin::Add => "+",
            Bin::Sub => "-",
            Bin::Mul => "*",
            Bin::Div => "/",
            Bin::Mod => "%",
            Bin::Lt => "<",
            Bin::Gt => ">",
            Bin::Le => "<=",
            Bin::Ge => ">=",
            Bin::Eq => "==",
            Bin::Ne => "!=",
            Bin::StrictEq => "===",
            Bin::StrictNe => "!==",
            Bin::BitAnd => "&",
            Bin::BitOr => "|",
            Bin::BitXor => "^",
            Bin::Shl => "<<",
            Bin::Shr => ">>",
            Bin::UShr => ">>>",
            Bin::InstanceOf => "instanceof",
        }
    }

    /// Binding strength (higher binds tighter), as in ECMAScript.
    pub fn prec(self) -> u8 {
        match self {
            Bin::Mul | Bin::Div | Bin::Mod => 14,
            Bin::Add | Bin::Sub => 13,
            Bin::Shl | Bin::Shr | Bin::UShr => 12,
            Bin::Lt | Bin::Gt | Bin::Le | Bin::Ge | Bin::InstanceOf => 11,
            Bin::Eq | Bin::Ne | Bin::StrictEq | Bin::StrictNe => 10,
            Bin::BitAnd => 9,
            Bin::BitXor => 8,
            Bin::BitOr => 7,
        }
    }

    /// The comparison that is true exactly when this one is false, as
    /// compiled (`a <= b` is `!(a > b)`).
    pub fn negated(self) -> Option<Bin> {
        Some(match self {
            Bin::Lt => Bin::Ge,
            Bin::Ge => Bin::Lt,
            Bin::Gt => Bin::Le,
            Bin::Le => Bin::Gt,
            Bin::Eq => Bin::Ne,
            Bin::Ne => Bin::Eq,
            Bin::StrictEq => Bin::StrictNe,
            Bin::StrictNe => Bin::StrictEq,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Un {
    Not,
    Neg,
    BitNot,
    TypeOf,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Undefined,
    Null,
    Bool(bool),
    Int(i32),
    Num(f64),
    Str(String),
    /// A name: a variable, a register local, `this`, `_global`, …
    Var(String),
    /// A variable looked up by a computed name (`eval(x)`).
    Eval(Box<Expr>),
    Member(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    New(Box<Expr>, Vec<Expr>),
    Unary(Un, Box<Expr>),
    Binary(Bin, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `target = value` (target: `Var`, `Member` or `Eval`); also `target op= value` once compiled.
    Assign(Box<Expr>, Box<Expr>),
    /// `++x` / `--x` (`true`: increment) and `x++` / `x--`.
    PreInc(Box<Expr>, bool),
    PostInc(Box<Expr>, bool),
    Array(Vec<Expr>),
    Object(Vec<(Expr, Expr)>),
    Function(Box<Func>),
    /// `delete obj.name` / `delete name`.
    Delete(Box<Expr>),
    /// A built-in that compiles to an opcode (`trace`, `getTimer`, `__cast`, …).
    Intrinsic(String, Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Expr(Expr),
    Var(Vec<(String, Option<Expr>)>),
    If(Expr, Vec<Stmt>, Option<Vec<Stmt>>),
    While(Expr, Vec<Stmt>),
    DoWhile(Vec<Stmt>, Expr),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Box<Stmt>>, Vec<Stmt>),
    /// `for (target in obj)`; `declare`: `for (var name in obj)`.
    ForIn(Expr, bool, Expr, Vec<Stmt>),
    /// Clauses in order: `None` is `default`.
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    Break,
    Continue,
    Return(Option<Expr>),
    Throw(Expr),
    Try(Vec<Stmt>, Option<(String, Vec<Stmt>)>, Option<Vec<Stmt>>),
    With(Expr, Vec<Stmt>),
    /// A named function declaration (`DefineFunction` with a name, as a statement).
    Function(Box<Func>),
    Block(Vec<Stmt>),
    /// Actions kept as P-code (a script the decompiler could not structure).
    Raw(Vec<Action>),
    /// A `class` / `interface` definition (`class::lower` gives its statements).
    Class(Box<super::class::Class>),
}

/// A `DefineFunction` / `DefineFunction2` body kept as P-code: the header
/// fields the actions depend on (registers and preloads).
#[derive(Clone, Debug, PartialEq)]
pub struct RawBody {
    pub v2: Option<(u8, u16)>,
    pub param_regs: Vec<u8>,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Func {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
    pub raw: Option<RawBody>,
}

/// Is `s` usable as an identifier (and after a `.`)?
pub fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch.is_alphabetic() || ch == '_' || ch == '$') && c.all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$')
}

/// Words that cannot name a variable.
pub fn is_reserved(s: &str) -> bool {
    matches!(
        s,
        "break" | "case" | "catch" | "class" | "continue" | "default" | "delete" | "do" | "dynamic" | "else" | "extends" | "false" | "finally" | "for" | "function" | "if" | "implements" | "import" | "in" | "instanceof" | "interface" | "intrinsic" | "new" | "null" | "private" | "public" | "return" | "static" | "switch" | "throw" | "true" | "try" | "typeof" | "undefined" | "var" | "void" | "while" | "with"
    )
}

/// Names the compiler turns into opcodes when called as plain functions:
/// `(name, arguments, pushes a value)`. `None` arguments: any count.
pub const INTRINSICS: &[(&str, u8, bool)] = &[
    ("trace", 1, false),
    ("getTimer", 0, true),
    ("int", 1, true),
    ("random", 1, true),
    ("chr", 1, true),
    ("ord", 1, true),
    ("mbchr", 1, true),
    ("mbord", 1, true),
    ("length", 1, true),
    ("mblength", 1, true),
    ("substring", 3, true),
    ("mbsubstring", 3, true),
    ("String", 1, true),
    ("Number", 1, true),
    ("targetPath", 1, true),
    ("getProperty", 2, true),
    ("setProperty", 3, false),
    ("eval", 1, true),
    ("set", 2, false),
    ("duplicateMovieClip", 3, false),
    ("removeMovieClip", 1, false),
    ("stop", 0, false),
    ("play", 0, false),
    ("nextFrame", 0, false),
    ("prevFrame", 0, false),
    ("stopAllSounds", 0, false),
    ("toggleHighQuality", 0, false),
    ("stopDrag", 0, false),
    ("gotoAndPlay", 1, false),
    ("gotoAndStop", 1, false),
    ("__cast", 2, true),
    ("__extends", 2, false),
    ("__implements", 255, false),
    ("__add", 2, true),
    ("__less", 2, true),
    ("__equals", 2, true),
    ("__and", 2, true),
    ("__or", 2, true),
    ("__stringEquals", 2, true),
    ("__stringAdd", 2, true),
    ("__stringLess", 2, true),
    ("__stringGreater", 2, true),
    ("__increment", 1, true),
    ("__decrement", 1, true),
    ("__swap", 0, false),
    ("__gotoFrame", 1, false),
    ("__gotoLabel", 1, false),
    ("__getURL2", 3, false),
    ("getURL", 2, false),
    ("__setTarget", 1, false),
    ("__setTarget2", 1, false),
    ("__call", 1, false),
    ("__fscommand2", 255, true),
    ("__startDrag", 255, false),
];

pub fn intrinsic(name: &str) -> Option<(u8, bool)> {
    INTRINSICS.iter().find(|(n, _, _)| *n == name).map(|(_, a, p)| (*a, *p))
}

impl Expr {
    pub fn str(s: &str) -> Expr {
        Expr::Str(s.to_string())
    }

    pub fn var(s: &str) -> Expr {
        Expr::Var(s.to_string())
    }

    pub fn member(obj: Expr, name: &str) -> Expr {
        Expr::Member(Box::new(obj), Box::new(Expr::str(name)))
    }

    /// No side effects and the same value wherever it is evaluated.
    pub fn is_literal(&self) -> bool {
        matches!(self, Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Int(_) | Expr::Num(_) | Expr::Str(_))
    }

    /// `Not` applied to a value, exactly: `!(a < b)` is `a >= b` (which
    /// compiles to `Less2, Not`), anything else gets a `!`.
    pub fn not_value(self) -> Expr {
        match self {
            Expr::Binary(op @ (Bin::Lt | Bin::Gt | Bin::Eq | Bin::StrictEq), a, b) => Expr::Binary(op.negated().unwrap(), a, b),
            e => Expr::Unary(Un::Not, Box::new(e)),
        }
    }

    /// The negation of a condition (only its truth matters): also folds
    /// `!!x` and turns `a >= b` back into `a < b`.
    pub fn negate(self) -> Expr {
        match self {
            Expr::Unary(Un::Not, e) => *e,
            Expr::Binary(op, a, b) if op.negated().is_some() => Expr::Binary(op.negated().unwrap(), a, b),
            e => Expr::Unary(Un::Not, Box::new(e)),
        }
    }
}
