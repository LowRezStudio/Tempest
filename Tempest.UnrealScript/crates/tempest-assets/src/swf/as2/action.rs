//! ActionScript 2 bytecode as a list of actions: decoded from an action
//! stream (`DoAction`, `DoInitAction`, clip and button actions) and encoded
//! back byte-identically. Branch offsets become instruction indices of the
//! enclosing block (`Label`), so inserting or removing actions keeps every
//! branch, `Try`/`With` size and `DefineFunction` body length right.

use tempest_upk::io::{Error, Result};

/// An instruction index in the block holding the branch; the block's
/// length is a valid target (the end).
pub type Label = usize;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Float(f32),
    Null,
    Undefined,
    Reg(u8),
    Bool(bool),
    Double(f64),
    Int(i32),
    /// A constant pool index; `wide` when stored as a 16-bit index although it is below 256.
    Const(u16, bool),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Catch {
    Name(String),
    Reg(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Try {
    /// The flag byte as stored (catch block, finally block, catch in register).
    pub flags: u8,
    pub catch: Catch,
    /// Where the catch block starts, the finally block starts and the statement ends.
    pub catch_at: Label,
    pub finally_at: Label,
    pub end: Label,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: String,
    /// `DefineFunction2`: register count and flags; `None` for `DefineFunction`.
    pub v2: Option<(u8, u16)>,
    /// Parameters with their register (0: an ordinary variable; always 0 in `DefineFunction`).
    pub params: Vec<(u8, String)>,
    pub body: Vec<Action>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Any action without a payload (opcode below 0x80).
    Op(u8),
    ConstantPool(Vec<String>),
    Push(Vec<Value>),
    StoreRegister(u8),
    Jump(Label),
    If(Label),
    GotoFrame(u16),
    GetUrl(String, String),
    WaitForFrame(u16, u8),
    SetTarget(String),
    GoToLabel(String),
    WaitForFrame2(u8),
    GetUrl2(u8),
    /// The flag byte (play, scene bias) and the scene bias.
    GotoFrame2(u8, Option<u16>),
    Function(Box<Function>),
    Try(Box<Try>),
    /// The end of the `with` body.
    With(Label),
    /// An action with a payload this module does not model, as stored.
    Raw(u8, Vec<u8>),
}

/// A whole action stream: the actions up to and including `End`, and any bytes after it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Script {
    pub actions: Vec<Action>,
    pub trailing: Vec<u8>,
}

pub const PRELOAD_THIS: u16 = 0x0001;
pub const SUPPRESS_THIS: u16 = 0x0002;
pub const PRELOAD_ARGUMENTS: u16 = 0x0004;
pub const SUPPRESS_ARGUMENTS: u16 = 0x0008;
pub const PRELOAD_SUPER: u16 = 0x0010;
pub const SUPPRESS_SUPER: u16 = 0x0020;
pub const PRELOAD_ROOT: u16 = 0x0040;
pub const PRELOAD_PARENT: u16 = 0x0080;
pub const PRELOAD_GLOBAL: u16 = 0x0100;

/// `DefineFunction2` flag names in bit order.
pub const FLAG_NAMES: [(u16, &str); 9] = [
    (PRELOAD_THIS, "preloadThis"),
    (SUPPRESS_THIS, "suppressThis"),
    (PRELOAD_ARGUMENTS, "preloadArguments"),
    (SUPPRESS_ARGUMENTS, "suppressArguments"),
    (PRELOAD_SUPER, "preloadSuper"),
    (SUPPRESS_SUPER, "suppressSuper"),
    (PRELOAD_ROOT, "preloadRoot"),
    (PRELOAD_PARENT, "preloadParent"),
    (PRELOAD_GLOBAL, "preloadGlobal"),
];

pub fn op_name(op: u8) -> Option<&'static str> {
    Some(match op {
        0x00 => "End",
        0x04 => "NextFrame",
        0x05 => "PreviousFrame",
        0x06 => "Play",
        0x07 => "Stop",
        0x08 => "ToggleQuality",
        0x09 => "StopSounds",
        0x0A => "Add",
        0x0B => "Subtract",
        0x0C => "Multiply",
        0x0D => "Divide",
        0x0E => "Equals",
        0x0F => "Less",
        0x10 => "And",
        0x11 => "Or",
        0x12 => "Not",
        0x13 => "StringEquals",
        0x14 => "StringLength",
        0x15 => "StringExtract",
        0x17 => "Pop",
        0x18 => "ToInteger",
        0x1C => "GetVariable",
        0x1D => "SetVariable",
        0x20 => "SetTarget2",
        0x21 => "StringAdd",
        0x22 => "GetProperty",
        0x23 => "SetProperty",
        0x24 => "CloneSprite",
        0x25 => "RemoveSprite",
        0x26 => "Trace",
        0x27 => "StartDrag",
        0x28 => "EndDrag",
        0x29 => "StringLess",
        0x2A => "Throw",
        0x2B => "CastOp",
        0x2C => "ImplementsOp",
        0x2D => "FSCommand2",
        0x30 => "RandomNumber",
        0x31 => "MBStringLength",
        0x32 => "CharToAscii",
        0x33 => "AsciiToChar",
        0x34 => "GetTime",
        0x35 => "MBStringExtract",
        0x36 => "MBCharToAscii",
        0x37 => "MBAsciiToChar",
        0x3A => "Delete",
        0x3B => "Delete2",
        0x3C => "DefineLocal",
        0x3D => "CallFunction",
        0x3E => "Return",
        0x3F => "Modulo",
        0x40 => "NewObject",
        0x41 => "DefineLocal2",
        0x42 => "InitArray",
        0x43 => "InitObject",
        0x44 => "TypeOf",
        0x45 => "TargetPath",
        0x46 => "Enumerate",
        0x47 => "Add2",
        0x48 => "Less2",
        0x49 => "Equals2",
        0x4A => "ToNumber",
        0x4B => "ToString",
        0x4C => "PushDuplicate",
        0x4D => "StackSwap",
        0x4E => "GetMember",
        0x4F => "SetMember",
        0x50 => "Increment",
        0x51 => "Decrement",
        0x52 => "CallMethod",
        0x53 => "NewMethod",
        0x54 => "InstanceOf",
        0x55 => "Enumerate2",
        0x60 => "BitAnd",
        0x61 => "BitOr",
        0x62 => "BitXor",
        0x63 => "BitLShift",
        0x64 => "BitRShift",
        0x65 => "BitURShift",
        0x66 => "StrictEquals",
        0x67 => "Greater",
        0x68 => "StringGreater",
        0x69 => "Extends",
        0x81 => "GotoFrame",
        0x83 => "GetURL",
        0x87 => "StoreRegister",
        0x88 => "ConstantPool",
        0x8A => "WaitForFrame",
        0x8B => "SetTarget",
        0x8C => "GoToLabel",
        0x8D => "WaitForFrame2",
        0x8E => "DefineFunction2",
        0x8F => "Try",
        0x94 => "With",
        0x96 => "Push",
        0x99 => "Jump",
        0x9A => "GetURL2",
        0x9B => "DefineFunction",
        0x9D => "If",
        0x9E => "Call",
        0x9F => "GotoFrame2",
        _ => return None,
    })
}

pub fn op_of(name: &str) -> Option<u8> {
    (0..=255u8).find(|&op| op_name(op).is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Format(msg.into()))
}

struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8> {
        let v = *self.d.get(self.pos).ok_or_else(|| Error::Format("truncated action".into()))?;
        self.pos += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes([self.u8()?, self.u8()?]))
    }
    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        let s = self.d.get(self.pos..self.pos + n).ok_or_else(|| Error::Format("truncated action".into()))?;
        self.pos += n;
        Ok(s)
    }
    fn str(&mut self) -> Result<String> {
        let rest = &self.d[self.pos.min(self.d.len())..];
        let end = rest.iter().position(|&b| b == 0).ok_or_else(|| Error::Format("unterminated string".into()))?;
        let s = std::str::from_utf8(&rest[..end]).map_err(|_| Error::Format("string is not UTF-8".into()))?.to_string();
        self.pos += end + 1;
        Ok(s)
    }
    fn done(&self) -> bool {
        self.pos >= self.d.len()
    }
}

/// A branch to resolve once every instruction's offset is known.
enum Fix {
    Jump(usize, i64),
    Try(usize, i64, i64, i64),
    With(usize, i64),
}

/// Decode `d` as one block. `top`: stop after `End` (a script); function
/// bodies run to the end of their bytes. Returns the actions and where decoding stopped.
fn decode_block(d: &[u8], top: bool) -> Result<(Vec<Action>, usize)> {
    let mut out = Vec::new();
    let mut offsets = Vec::new();
    let mut fixes = Vec::new();
    let mut pos = 0;
    while pos < d.len() {
        offsets.push(pos);
        let op = d[pos];
        pos += 1;
        if op < 0x80 {
            out.push(Action::Op(op));
            if op == 0 && top {
                break;
            }
            continue;
        }
        let len = u16::from_le_bytes([*d.get(pos).ok_or_else(|| Error::Format("truncated action".into()))?, *d.get(pos + 1).ok_or_else(|| Error::Format("truncated action".into()))?]) as usize;
        pos += 2;
        let payload = d.get(pos..pos + len).ok_or_else(|| Error::Format(format!("action 0x{op:02x} runs past the end")))?;
        pos += len;
        let mut r = Reader { d: payload, pos: 0 };
        let a = match op {
            0x88 => {
                let n = r.u16()? as usize;
                let mut pool = Vec::with_capacity(n);
                for _ in 0..n {
                    pool.push(r.str()?);
                }
                Action::ConstantPool(pool)
            }
            0x96 => {
                let mut items = Vec::new();
                while !r.done() {
                    items.push(match r.u8()? {
                        0 => Value::Str(r.str()?),
                        1 => Value::Float(f32::from_le_bytes(r.bytes(4)?.try_into().unwrap())),
                        2 => Value::Null,
                        3 => Value::Undefined,
                        4 => Value::Reg(r.u8()?),
                        5 => match r.u8()? {
                            0 => Value::Bool(false),
                            1 => Value::Bool(true),
                            b => return err(format!("boolean {b}")),
                        },
                        6 => {
                            // Stored as two little-endian 32-bit halves, high word first.
                            let b = r.bytes(8)?;
                            let mut x = [0u8; 8];
                            x[..4].copy_from_slice(&b[4..]);
                            x[4..].copy_from_slice(&b[..4]);
                            Value::Double(f64::from_le_bytes(x))
                        }
                        7 => Value::Int(i32::from_le_bytes(r.bytes(4)?.try_into().unwrap())),
                        8 => Value::Const(r.u8()? as u16, false),
                        9 => {
                            let i = r.u16()?;
                            Value::Const(i, i < 256)
                        }
                        t => return err(format!("push type {t}")),
                    });
                }
                Action::Push(items)
            }
            0x87 => Action::StoreRegister(r.u8()?),
            0x99 | 0x9D => {
                let off = r.u16()? as i16 as i64;
                fixes.push(Fix::Jump(out.len(), pos as i64 + off));
                if op == 0x99 { Action::Jump(0) } else { Action::If(0) }
            }
            0x81 => Action::GotoFrame(r.u16()?),
            0x83 => Action::GetUrl(r.str()?, r.str()?),
            0x8A => Action::WaitForFrame(r.u16()?, r.u8()?),
            0x8B => Action::SetTarget(r.str()?),
            0x8C => Action::GoToLabel(r.str()?),
            0x8D => Action::WaitForFrame2(r.u8()?),
            0x9A => Action::GetUrl2(r.u8()?),
            0x9F => {
                let f = r.u8()?;
                let bias = if f & 2 != 0 { Some(r.u16()?) } else { None };
                Action::GotoFrame2(f, bias)
            }
            0x9B | 0x8E => {
                let name = r.str()?;
                let n = r.u16()? as usize;
                let mut v2 = None;
                let mut params = Vec::with_capacity(n);
                if op == 0x8E {
                    let regs = r.u8()?;
                    v2 = Some((regs, r.u16()?));
                    for _ in 0..n {
                        let reg = r.u8()?;
                        params.push((reg, r.str()?));
                    }
                } else {
                    for _ in 0..n {
                        params.push((0, r.str()?));
                    }
                }
                let size = r.u16()? as usize;
                let body_bytes = d.get(pos..pos + size).ok_or_else(|| Error::Format(format!("function {name:?} runs past the end")))?;
                let (body, used) = decode_block(body_bytes, false)?;
                if used != size {
                    return err(format!("function {name:?} body does not end on an action"));
                }
                pos += size;
                Action::Function(Box::new(Function { name, v2, params, body }))
            }
            0x8F => {
                let flags = r.u8()?;
                let try_size = r.u16()? as i64;
                let catch_size = r.u16()? as i64;
                let finally_size = r.u16()? as i64;
                let catch = if flags & 4 != 0 { Catch::Reg(r.u8()?) } else { Catch::Name(r.str()?) };
                let start = pos as i64;
                fixes.push(Fix::Try(out.len(), start + try_size, start + try_size + catch_size, start + try_size + catch_size + finally_size));
                Action::Try(Box::new(Try { flags, catch, catch_at: 0, finally_at: 0, end: 0 }))
            }
            0x94 => {
                let size = r.u16()? as i64;
                fixes.push(Fix::With(out.len(), pos as i64 + size));
                Action::With(0)
            }
            _ => {
                out.push(Action::Raw(op, payload.to_vec()));
                continue;
            }
        };
        if !r.done() {
            // Extra payload bytes would not survive a rewrite.
            return err(format!("action {} has {} unexpected payload bytes", op_name(op).unwrap_or("?"), payload.len() - r.pos));
        }
        out.push(a);
    }
    let end = pos;
    offsets.push(end);
    let index = |at: i64| -> Result<Label> {
        usize::try_from(at).ok().and_then(|at| offsets.binary_search(&at).ok()).ok_or_else(|| Error::Format(format!("branch to offset {at}, not an action")))
    };
    for f in fixes {
        match f {
            Fix::Jump(i, at) => {
                let l = index(at)?;
                match &mut out[i] {
                    Action::Jump(t) | Action::If(t) => *t = l,
                    _ => unreachable!(),
                }
            }
            Fix::Try(i, c, f, e) => {
                let (c, f, e) = (index(c)?, index(f)?, index(e)?);
                if let Action::Try(t) = &mut out[i] {
                    t.catch_at = c;
                    t.finally_at = f;
                    t.end = e;
                }
            }
            Fix::With(i, e) => {
                let l = index(e)?;
                if let Action::With(t) = &mut out[i] {
                    *t = l;
                }
            }
        }
    }
    Ok((out, end))
}

impl Script {
    pub fn decode(d: &[u8]) -> Result<Script> {
        let (actions, used) = decode_block(d, true)?;
        Ok(Script { actions, trailing: d[used..].to_vec() })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = encode_block(&self.actions)?;
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }
}

fn str_bytes(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

/// The payload of an action, and the encoded body of a function.
fn payload(a: &Action) -> Result<(u8, Option<Vec<u8>>, Vec<u8>)> {
    let mut p = Vec::new();
    let mut body = Vec::new();
    let op = match a {
        Action::Op(op) => return Ok((*op, None, body)),
        Action::ConstantPool(pool) => {
            p.extend_from_slice(&(pool.len() as u16).to_le_bytes());
            for s in pool {
                str_bytes(&mut p, s);
            }
            0x88
        }
        Action::Push(items) => {
            for v in items {
                match v {
                    Value::Str(s) => {
                        p.push(0);
                        str_bytes(&mut p, s);
                    }
                    Value::Float(f) => {
                        p.push(1);
                        p.extend_from_slice(&f.to_le_bytes());
                    }
                    Value::Null => p.push(2),
                    Value::Undefined => p.push(3),
                    Value::Reg(r) => p.extend_from_slice(&[4, *r]),
                    Value::Bool(b) => p.extend_from_slice(&[5, *b as u8]),
                    Value::Double(d) => {
                        p.push(6);
                        let b = d.to_le_bytes();
                        p.extend_from_slice(&b[4..]);
                        p.extend_from_slice(&b[..4]);
                    }
                    Value::Int(i) => {
                        p.push(7);
                        p.extend_from_slice(&i.to_le_bytes());
                    }
                    Value::Const(i, wide) => {
                        if *i < 256 && !wide {
                            p.extend_from_slice(&[8, *i as u8]);
                        } else {
                            p.push(9);
                            p.extend_from_slice(&i.to_le_bytes());
                        }
                    }
                }
            }
            0x96
        }
        Action::StoreRegister(r) => {
            p.push(*r);
            0x87
        }
        // Offsets are written by `encode_block`.
        Action::Jump(_) => {
            p.extend_from_slice(&[0, 0]);
            0x99
        }
        Action::If(_) => {
            p.extend_from_slice(&[0, 0]);
            0x9D
        }
        Action::GotoFrame(f) => {
            p.extend_from_slice(&f.to_le_bytes());
            0x81
        }
        Action::GetUrl(u, t) => {
            str_bytes(&mut p, u);
            str_bytes(&mut p, t);
            0x83
        }
        Action::WaitForFrame(f, skip) => {
            p.extend_from_slice(&f.to_le_bytes());
            p.push(*skip);
            0x8A
        }
        Action::SetTarget(s) => {
            str_bytes(&mut p, s);
            0x8B
        }
        Action::GoToLabel(s) => {
            str_bytes(&mut p, s);
            0x8C
        }
        Action::WaitForFrame2(skip) => {
            p.push(*skip);
            0x8D
        }
        Action::GetUrl2(m) => {
            p.push(*m);
            0x9A
        }
        Action::GotoFrame2(f, bias) => {
            p.push(*f);
            if let Some(b) = bias {
                p.extend_from_slice(&b.to_le_bytes());
            }
            0x9F
        }
        Action::Function(f) => {
            str_bytes(&mut p, &f.name);
            p.extend_from_slice(&(f.params.len() as u16).to_le_bytes());
            if let Some((regs, flags)) = f.v2 {
                p.push(regs);
                p.extend_from_slice(&flags.to_le_bytes());
                for (r, name) in &f.params {
                    p.push(*r);
                    str_bytes(&mut p, name);
                }
            } else {
                for (_, name) in &f.params {
                    str_bytes(&mut p, name);
                }
            }
            body = encode_block(&f.body)?;
            let size = u16::try_from(body.len()).map_err(|_| Error::Format(format!("function {:?} is larger than 64 KiB", f.name)))?;
            p.extend_from_slice(&size.to_le_bytes());
            if f.v2.is_some() { 0x8E } else { 0x9B }
        }
        Action::Try(t) => {
            p.push(t.flags);
            p.extend_from_slice(&[0; 6]);
            match &t.catch {
                Catch::Reg(r) => p.push(*r),
                Catch::Name(n) => str_bytes(&mut p, n),
            }
            0x8F
        }
        Action::With(_) => {
            p.extend_from_slice(&[0, 0]);
            0x94
        }
        Action::Raw(op, data) => {
            p.extend_from_slice(data);
            *op
        }
    };
    Ok((op, Some(p), body))
}

pub fn encode_block(actions: &[Action]) -> Result<Vec<u8>> {
    let mut parts = Vec::with_capacity(actions.len());
    let mut offsets = Vec::with_capacity(actions.len() + 1);
    let mut at = 0usize;
    for a in actions {
        offsets.push(at);
        let (op, p, body) = payload(a)?;
        at += 1 + p.as_ref().map_or(0, |p| 2 + p.len()) + body.len();
        parts.push((op, p, body));
    }
    offsets.push(at);
    let target = |l: Label, what: &str| -> Result<usize> { offsets.get(l).copied().ok_or_else(|| Error::Format(format!("{what} to action {l}, past the end"))) };
    let rel = |from: usize, to: usize| -> Result<[u8; 2]> {
        let d = to as i64 - from as i64;
        i16::try_from(d).map(|d| d.to_le_bytes()).map_err(|_| Error::Format("branch farther than 32 KiB".into()))
    };
    let size = |from: usize, to: usize| -> Result<[u8; 2]> {
        u16::try_from(to as i64 - from as i64).map(|d| d.to_le_bytes()).map_err(|_| Error::Format("block size out of range".into()))
    };
    let mut out = Vec::with_capacity(at);
    for (i, (a, (op, p, body))) in actions.iter().zip(parts).enumerate() {
        let Some(mut p) = p else {
            out.push(op);
            continue;
        };
        let next = offsets[i] + 3 + p.len();
        match a {
            Action::Jump(l) | Action::If(l) => p[..2].copy_from_slice(&rel(next, target(*l, "branch")?)?),
            Action::With(l) => p[..2].copy_from_slice(&size(next, target(*l, "with")?)?),
            Action::Try(t) => {
                let (c, f, e) = (target(t.catch_at, "try")?, target(t.finally_at, "try")?, target(t.end, "try")?);
                p[1..3].copy_from_slice(&size(next, c)?);
                p[3..5].copy_from_slice(&size(c, f)?);
                p[5..7].copy_from_slice(&size(f, e)?);
            }
            _ => {}
        }
        out.push(op);
        out.extend_from_slice(&u16::try_from(p.len()).map_err(|_| Error::Format("action payload larger than 64 KiB".into()))?.to_le_bytes());
        out.extend_from_slice(&p);
        out.extend_from_slice(&body);
    }
    Ok(out)
}
