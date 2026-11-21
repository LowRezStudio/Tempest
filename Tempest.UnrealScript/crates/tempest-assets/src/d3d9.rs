//! Direct3D 9 shader model 3 bytecode (`vs_3_0` / `ps_3_0`, what the game's
//! PC renderer runs and `ShaderCodeStorage-PC-D3D-SM3.bin` holds): the
//! instruction stream, the constant table the HLSL compiler leaves in a
//! comment (`CTAB`: every constant's name and registers), and a translation
//! of a vertex and pixel shader pair into one WGSL module, so the game's own
//! shaders can draw a preview through wgpu.
//!
//! The translation keeps the registers: `r#` are `vec4<f32>` variables, the
//! float constants `c#` come from one uniform array per stage (the caller
//! fills them by `CTAB` name), `def`ined constants are literals, samplers
//! are texture + sampler pairs (`tex#` + `smp#`; `vtex#` + `vsmp#` in a vertex shader), and the interpolants are matched between
//! the stages by their declared usage (`TEXCOORD3`, `COLOR0`…).
//!
//! References: d3d9types.h (`D3DSHADER_INSTRUCTION_OPCODE_TYPE`, the
//! parameter tokens), the Direct3D 9 shader assembly reference for what each
//! instruction computes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// A named constant of the `CTAB`.
#[derive(Clone, Debug, PartialEq)]
pub struct Constant {
    pub name: String,
    pub set: RegisterSet,
    /// First register and how many it spans.
    pub register: u16,
    pub count: u16,
    pub rows: u16,
    pub columns: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterSet {
    Bool,
    Int,
    Float,
    Sampler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Vertex,
    Pixel,
}

/// A vertex input, interpolant or pixel input: `D3DDECLUSAGE` and its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Semantic {
    pub usage: u8,
    pub index: u8,
}

impl Semantic {
    pub const POSITION: u8 = 0;
    pub const BLENDWEIGHT: u8 = 1;
    pub const BLENDINDICES: u8 = 2;
    pub const NORMAL: u8 = 3;
    pub const PSIZE: u8 = 4;
    pub const TEXCOORD: u8 = 5;
    pub const TANGENT: u8 = 6;
    pub const BINORMAL: u8 = 7;
    pub const COLOR: u8 = 10;
    pub const FOG: u8 = 11;

    pub fn name(self) -> String {
        let u = match self.usage {
            0 => "POSITION",
            1 => "BLENDWEIGHT",
            2 => "BLENDINDICES",
            3 => "NORMAL",
            4 => "PSIZE",
            5 => "TEXCOORD",
            6 => "TANGENT",
            7 => "BINORMAL",
            8 => "TESSFACTOR",
            9 => "POSITIONT",
            10 => "COLOR",
            11 => "FOG",
            12 => "DEPTH",
            _ => "SAMPLE",
        };
        format!("{u}{}", self.index)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SamplerKind {
    D2,
    Cube,
    Volume,
}

// Register types (`D3DSHADER_PARAM_REGISTER_TYPE`).
const R_TEMP: u8 = 0;
const R_INPUT: u8 = 1;
const R_CONST: u8 = 2;
const R_ADDR_OR_TEX: u8 = 3;
const R_RASTOUT: u8 = 4;
const R_ATTROUT: u8 = 5;
const R_OUTPUT: u8 = 6;
const R_COLOROUT: u8 = 8;
const R_DEPTHOUT: u8 = 9;
const R_SAMPLER: u8 = 10;
const R_LOOP: u8 = 15;
const R_MISC: u8 = 17;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Dst {
    ty: u8,
    num: u16,
    mask: u8,
    saturate: bool,
}

/// Relative addressing: the address register's type, number and component.
type Relative = (u8, u16, u8);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Src {
    ty: u8,
    num: u16,
    swizzle: [u8; 4],
    modifier: u8,
    /// Relative addressing: the address register's type, number and component (`c[a0.x + n]`, `c[aL + n]`).
    relative: Option<Relative>,
}

#[derive(Clone, Debug, PartialEq)]
struct Ins {
    op: u16,
    /// The opcode's control bits (comparison of `ifc`/`breakc`, projection and bias of `texld`).
    control: u8,
    dst: Option<Dst>,
    src: Vec<Src>,
}

/// A parsed shader.
#[derive(Clone, Debug, PartialEq)]
pub struct Shader {
    pub stage: Stage,
    pub constants: Vec<Constant>,
    /// Declared inputs: (register, semantic). Pixel shaders also read `vPos` and `vFace`.
    pub inputs: Vec<(u16, Semantic)>,
    /// A vertex shader's declared outputs: (register, semantic, write mask).
    pub outputs: Vec<(u16, Semantic, u8)>,
    pub samplers: Vec<(u16, SamplerKind)>,
    /// `def c#`: literal float constants.
    pub float_defs: BTreeMap<u16, [f32; 4]>,
    pub int_defs: BTreeMap<u16, [i32; 4]>,
    pub bool_defs: BTreeMap<u16, bool>,
    ins: Vec<Ins>,
}

fn err<T>(text: impl Into<String>) -> Result<T, String> {
    Err(text.into())
}

/// The `CTAB` comment's constants.
fn constant_table(body: &[u8]) -> Vec<Constant> {
    let u32_at = |o: usize| body.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let u16_at = |o: usize| body.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let text = |o: usize| body.get(o..).map(|b| String::from_utf8_lossy(&b[..b.iter().position(|c| *c == 0).unwrap_or(b.len())]).to_string()).unwrap_or_default();
    let (Some(count), Some(info)) = (u32_at(12), u32_at(16)) else { return Vec::new() };
    let mut out = Vec::new();
    for k in 0..count {
        let at = info + 20 * k;
        let (Some(name), Some(set), Some(register), Some(count), Some(type_info)) = (u32_at(at), u16_at(at + 4), u16_at(at + 6), u16_at(at + 8), u32_at(at + 12)) else { break };
        let set = match set {
            0 => RegisterSet::Bool,
            1 => RegisterSet::Int,
            2 => RegisterSet::Float,
            _ => RegisterSet::Sampler,
        };
        out.push(Constant { name: text(name), set, register, count, rows: u16_at(type_info + 4).unwrap_or(1), columns: u16_at(type_info + 6).unwrap_or(4) });
    }
    out
}

impl Shader {
    /// Parse shader model 3 bytecode (starting at its version token).
    pub fn parse(code: &[u8]) -> Result<Shader, String> {
        if code.len() < 8 || !code.len().is_multiple_of(4) {
            return err("not Direct3D 9 bytecode");
        }
        let tokens: Vec<u32> = code.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let stage = match tokens[0] {
            0xFFFE_0300 => Stage::Vertex,
            0xFFFF_0300 => Stage::Pixel,
            v => return err(format!("shader version {v:08x}: only vs_3_0 and ps_3_0 are handled")),
        };
        let mut s = Shader { stage, constants: Vec::new(), inputs: Vec::new(), outputs: Vec::new(), samplers: Vec::new(), float_defs: BTreeMap::new(), int_defs: BTreeMap::new(), bool_defs: BTreeMap::new(), ins: Vec::new() };
        let reg_type = |t: u32| (((t >> 28) & 7) | ((t >> 8) & 0x18)) as u8;
        let mut i = 1;
        while i < tokens.len() {
            let t = tokens[i];
            let op = (t & 0xffff) as u16;
            if op == 0xFFFF {
                break;
            }
            if op == 0xFFFE {
                let n = ((t >> 16) & 0x7fff) as usize;
                let body = &code[(i + 1) * 4..((i + 1 + n) * 4).min(code.len())];
                if body.starts_with(b"CTAB") {
                    s.constants = constant_table(&body[4..]);
                }
                i += 1 + n;
                continue;
            }
            let len = ((t >> 24) & 0xf) as usize;
            let args = tokens.get(i + 1..i + 1 + len).ok_or("an instruction runs past the end")?;
            i += 1 + len;
            let control = ((t >> 16) & 0xff) as u8;
            match op {
                // dcl
                31 => {
                    let (info, d) = (args[0], args[1]);
                    let (ty, num) = (reg_type(d), (d & 0x7ff) as u16);
                    let sem = Semantic { usage: (info & 0x1f) as u8, index: ((info >> 16) & 0xf) as u8 };
                    match ty {
                        R_SAMPLER => s.samplers.push((
                            num,
                            match (info >> 27) & 0xf {
                                3 => SamplerKind::Cube,
                                4 => SamplerKind::Volume,
                                _ => SamplerKind::D2,
                            },
                        )),
                        R_INPUT => s.inputs.push((num, sem)),
                        R_OUTPUT => s.outputs.push((num, sem, ((d >> 16) & 0xf) as u8)),
                        _ => {}
                    }
                }
                // def, defi, defb
                81 => {
                    s.float_defs.insert((args[0] & 0x7ff) as u16, [f32::from_bits(args[1]), f32::from_bits(args[2]), f32::from_bits(args[3]), f32::from_bits(args[4])]);
                }
                48 => {
                    s.int_defs.insert((args[0] & 0x7ff) as u16, [args[1] as i32, args[2] as i32, args[3] as i32, args[4] as i32]);
                }
                47 => {
                    s.bool_defs.insert((args[0] & 0x7ff) as u16, args[1] != 0);
                }
                _ => {
                    // Which instructions write a register first.
                    let has_dst = !matches!(op, 0 | 25..=30 | 38..=45 | 96);
                    let mut k = 0;
                    let read = |k: &mut usize| -> Result<(u32, Option<Relative>), String> {
                        let p = *args.get(*k).ok_or("an instruction lacks a parameter")?;
                        *k += 1;
                        let relative = if p & 0x2000 != 0 {
                            let a = *args.get(*k).ok_or("a relative address lacks its register")?;
                            *k += 1;
                            Some((reg_type(a), (a & 0x7ff) as u16, ((a >> 16) & 3) as u8))
                        } else {
                            None
                        };
                        Ok((p, relative))
                    };
                    let dst = if has_dst {
                        let (p, _) = read(&mut k)?;
                        Some(Dst { ty: reg_type(p), num: (p & 0x7ff) as u16, mask: ((p >> 16) & 0xf) as u8, saturate: (p >> 20) & 1 != 0 })
                    } else {
                        None
                    };
                    let mut src = Vec::new();
                    while k < args.len() {
                        let (p, relative) = read(&mut k)?;
                        let sw = (p >> 16) & 0xff;
                        src.push(Src { ty: reg_type(p), num: (p & 0x7ff) as u16, swizzle: [(sw & 3) as u8, ((sw >> 2) & 3) as u8, ((sw >> 4) & 3) as u8, ((sw >> 6) & 3) as u8], modifier: ((p >> 24) & 0xf) as u8, relative });
                    }
                    s.ins.push(Ins { op, control, dst, src });
                }
            }
        }
        Ok(s)
    }

    /// The float constant registers the shader reads from the caller: the highest one + 1.
    pub fn float_registers(&self) -> usize {
        let named = self.constants.iter().filter(|c| c.set == RegisterSet::Float).map(|c| (c.register + c.count) as usize).max().unwrap_or(0);
        let mut used = 0;
        for i in &self.ins {
            // A matrix instruction reads the rows after its register too.
            let rows = match i.op {
                20 | 22 => 4,
                21 | 23 => 3,
                24 => 2,
                _ => 1,
            };
            for (k, s) in i.src.iter().enumerate() {
                if s.ty == R_CONST && !self.float_defs.contains_key(&s.num) {
                    used = used.max(s.num as usize + if k == 1 { rows } else { 1 });
                }
            }
        }
        named.max(used).max(1)
    }

    pub fn constant(&self, name: &str) -> Option<&Constant> {
        self.constants.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn instruction_count(&self) -> usize {
        self.ins.len()
    }

    /// Whether every instruction can be translated (the reason when one cannot).
    pub fn check(&self) -> Result<(), String> {
        let mut e = Emit { sh: self, consts: if self.stage == Stage::Vertex { "vsc" } else { "psc" }, bools: &|_| false, out: String::new(), indent: 1, temps: BTreeSet::new(), n: 0, loops: Vec::new(), draw: BTreeMap::new() };
        e.body().map(|_| ())
    }
}

// ----- WGSL ---------------------------------------------------------------------------------

const XYZW: [char; 4] = ['x', 'y', 'z', 'w'];

fn literal(v: f32) -> String {
    if v.is_nan() {
        "0.0".into()
    } else if v.is_infinite() {
        if v > 0.0 { "3.4e38".into() } else { "-3.4e38".into() }
    } else {
        let s = format!("{v:?}");
        if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") }
    }
}

struct Emit<'a> {
    sh: &'a Shader,
    /// `vsc` / `psc`: the stage's uniform of float constants.
    consts: &'static str,
    /// Bool constants the caller sets (`CTAB` registers): false unless listed.
    bools: &'a dyn Fn(u16) -> bool,
    out: String,
    indent: usize,
    temps: BTreeSet<u16>,
    /// Counter for the block-scoped `let`s.
    n: usize,
    /// Registers read from the placement's constants (`vsd`) instead: register → its slot there.
    draw: BTreeMap<u16, u16>,
    /// Loop nesting: each loop's `aL` variable and its step (`rep` has none: an empty name).
    loops: Vec<(String, i32)>,
}

impl Emit<'_> {
    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// The register a parameter names, as a `vec4<f32>` expression (before swizzle and modifier).
    fn register(&mut self, ty: u8, num: u16, relative: Option<Relative>) -> Result<String, String> {
        Ok(match ty {
            R_TEMP => {
                self.temps.insert(num);
                format!("r{num}")
            }
            R_INPUT => format!("v{num}"),
            R_CONST => match relative {
                Some((aty, _, comp)) => {
                    let base = if aty == R_LOOP { self.loop_register()? } else { format!("a0.{}", XYZW[comp as usize]) };
                    format!("{}.c[u32(clamp({base} + {num}, 0, FLOATS - 1))]", self.consts)
                }
                None => match self.sh.float_defs.get(&num) {
                    Some(v) => format!("vec4<f32>({}, {}, {}, {})", literal(v[0]), literal(v[1]), literal(v[2]), literal(v[3])),
                    None => match self.draw.get(&num) {
                        Some(slot) => format!("vsd.c[{slot}]"),
                        None => format!("{}.c[{num}]", self.consts),
                    },
                },
            },
            R_ADDR_OR_TEX if self.sh.stage == Stage::Vertex => "vec4<f32>(a0)".to_string(),
            R_OUTPUT | R_RASTOUT | R_ATTROUT => format!("o{num}"),
            R_COLOROUT => format!("oc{num}"),
            R_DEPTHOUT => "odepth".to_string(),
            R_MISC => match num {
                0 => "vpos".to_string(),
                _ => "vface".to_string(),
            },
            R_LOOP => format!("vec4<f32>(f32({}))", self.loop_register()?),
            other => return err(format!("register type {other} is not handled")),
        })
    }

    /// The innermost `loop`'s `aL`.
    fn loop_register(&self) -> Result<String, String> {
        self.loops.iter().rev().find(|l| !l.0.is_empty()).map(|l| l.0.clone()).ok_or_else(|| "aL outside a loop".to_string())
    }

    /// A source parameter as a `vec4<f32>` expression.
    fn src(&mut self, s: &Src) -> Result<String, String> {
        let base = self.register(s.ty, s.num, s.relative)?;
        let sw: String = s.swizzle.iter().map(|c| XYZW[*c as usize]).collect();
        let v = if sw == "xyzw" { base } else { format!("{base}.{sw}") };
        Ok(match s.modifier {
            0 => v,
            1 => format!("(-{v})"),
            2 => format!("({v} - vec4<f32>(0.5))"),
            3 => format!("(-({v} - vec4<f32>(0.5)))"),
            4 => format!("(({v} - vec4<f32>(0.5)) * 2.0)"),
            5 => format!("(-(({v} - vec4<f32>(0.5)) * 2.0))"),
            6 => format!("(vec4<f32>(1.0) - {v})"),
            7 => format!("({v} * 2.0)"),
            8 => format!("(-({v} * 2.0))"),
            11 => format!("abs({v})"),
            12 => format!("(-abs({v}))"),
            m => return err(format!("source modifier {m} is not handled")),
        })
    }

    /// Write `value` (a `vec4<f32>` expression) to the destination's masked components.
    fn store(&mut self, d: &Dst, value: &str) -> Result<(), String> {
        let target = match d.ty {
            R_ADDR_OR_TEX if self.sh.stage == Stage::Vertex => "a0".to_string(),
            _ => self.register(d.ty, d.num, None)?,
        };
        let value = if d.saturate { format!("clamp({value}, vec4<f32>(0.0), vec4<f32>(1.0))") } else { value.to_string() };
        if target == "a0" {
            // mova: the address register holds the rounded value.
            self.line(&format!("a0 = vec4<i32>(round({value}));"));
            return Ok(());
        }
        if d.mask == 0xf {
            self.line(&format!("{target} = {value};"));
            return Ok(());
        }
        self.n += 1;
        let t = format!("t{}", self.n);
        self.line(&format!("let {t} = {value};"));
        let parts: Vec<String> = (0..4).map(|k| if d.mask >> k & 1 != 0 { format!("{t}.{}", XYZW[k]) } else { format!("{target}.{}", XYZW[k]) }).collect();
        self.line(&format!("{target} = vec4<f32>({});", parts.join(", ")));
        Ok(())
    }

    fn sampler_kind(&self, num: u16) -> SamplerKind {
        self.sh.samplers.iter().find(|s| s.0 == num).map(|s| s.1).unwrap_or(SamplerKind::D2)
    }

    /// The condition of `ifc` / `breakc`: scalars (the first swizzle component of each side).
    fn compare(&mut self, control: u8, a: &Src, b: &Src) -> Result<String, String> {
        let op = match control & 7 {
            1 => ">",
            2 => "==",
            3 => ">=",
            4 => "<",
            5 => "!=",
            6 => "<=",
            c => return err(format!("comparison {c} is not handled")),
        };
        Ok(format!("({}.x {op} {}.x)", self.src(a)?, self.src(b)?))
    }

    fn instruction(&mut self, ins: &Ins) -> Result<(), String> {
        let s = |this: &mut Self, k: usize| -> Result<String, String> { this.src(ins.src.get(k).ok_or("an instruction lacks a source")?) };
        let d = ins.dst;
        let store = |this: &mut Self, value: String| -> Result<(), String> { this.store(&d.ok_or("an instruction lacks its destination")?, &value) };
        // Rows of a matrix operand: the constant register and the ones after it.
        let rows = |this: &mut Self, n: usize, dims: &str| -> Result<String, String> {
            let v = s(this, 0)?;
            let m = *ins.src.get(1).ok_or("a matrix instruction lacks its matrix")?;
            let mut parts = Vec::new();
            for k in 0..4 {
                if k < n {
                    let row = this.src(&Src { num: m.num + k as u16, ..m })?;
                    parts.push(if dims == "3" { format!("dot({v}.xyz, {row}.xyz)") } else { format!("dot({v}, {row})") });
                } else {
                    parts.push("0.0".to_string());
                }
            }
            Ok(format!("vec4<f32>({})", parts.join(", ")))
        };
        match ins.op {
            0 => {}
            1 | 46 => {
                let v = s(self, 0)?;
                store(self, v)?
            }
            2 => {
                let v = format!("({} + {})", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            3 => {
                let v = format!("({} - {})", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            4 => {
                let v = format!("({} * {} + {})", s(self, 0)?, s(self, 1)?, s(self, 2)?);
                store(self, v)?
            }
            5 => {
                let v = format!("({} * {})", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            6 => {
                let v = format!("vec4<f32>(1.0 / {}.x)", s(self, 0)?);
                store(self, v)?
            }
            7 => {
                let v = format!("vec4<f32>(inverseSqrt(abs({}.x)))", s(self, 0)?);
                store(self, v)?
            }
            8 => {
                let v = format!("vec4<f32>(dot({}.xyz, {}.xyz))", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            9 => {
                let v = format!("vec4<f32>(dot({}, {}))", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            10 => {
                let v = format!("min({}, {})", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            11 => {
                let v = format!("max({}, {})", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            12 => {
                let v = format!("select(vec4<f32>(0.0), vec4<f32>(1.0), {} < {})", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            13 => {
                let v = format!("select(vec4<f32>(0.0), vec4<f32>(1.0), {} >= {})", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            14 => {
                let v = format!("vec4<f32>(exp2({}.x))", s(self, 0)?);
                store(self, v)?
            }
            15 => {
                let v = format!("vec4<f32>(log2(abs({}.x)))", s(self, 0)?);
                store(self, v)?
            }
            16 => {
                // lit: (1, max(n·l, 0), n·l > 0 ? pow(max(n·h, 0), power) : 0, 1) from src.xyw.
                let v = s(self, 0)?;
                self.n += 1;
                let t = format!("t{}", self.n);
                self.line(&format!("let {t} = {v};"));
                let e = format!("vec4<f32>(1.0, max({t}.x, 0.0), select(0.0, pow(max({t}.y, 0.0), clamp({t}.w, -127.9961, 127.9961)), {t}.x > 0.0), 1.0)");
                store(self, e)?
            }
            18 => {
                let v = format!("mix({}, {}, {})", s(self, 2)?, s(self, 1)?, s(self, 0)?);
                store(self, v)?
            }
            19 => {
                let v = format!("fract({})", s(self, 0)?);
                store(self, v)?
            }
            20 => {
                let v = rows(self, 4, "4")?;
                store(self, v)?
            }
            21 => {
                let v = rows(self, 3, "4")?;
                store(self, v)?
            }
            22 => {
                let v = rows(self, 4, "3")?;
                store(self, v)?
            }
            23 => {
                let v = rows(self, 3, "3")?;
                store(self, v)?
            }
            24 => {
                let v = rows(self, 2, "3")?;
                store(self, v)?
            }
            32 => {
                let v = format!("vec4<f32>(pow(abs({}.x), {}.x))", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            33 => {
                let v = format!("vec4<f32>(cross({}.xyz, {}.xyz), 0.0)", s(self, 0)?, s(self, 1)?);
                store(self, v)?
            }
            34 => {
                let v = format!("sign({})", s(self, 0)?);
                store(self, v)?
            }
            35 => {
                let v = format!("abs({})", s(self, 0)?);
                store(self, v)?
            }
            36 => {
                let v = s(self, 0)?;
                self.n += 1;
                let t = format!("t{}", self.n);
                self.line(&format!("let {t} = {v};"));
                store(self, format!("({t} * inverseSqrt(max(dot({t}.xyz, {t}.xyz), 1e-30)))"))?
            }
            37 => {
                let v = s(self, 0)?;
                self.n += 1;
                let t = format!("t{}", self.n);
                self.line(&format!("let {t} = {v}.x;"));
                store(self, format!("vec4<f32>(cos({t}), sin({t}), 0.0, 0.0)"))?
            }
            // rep i# / loop aL, i#: a counted loop (count, start, step of the integer constant).
            38 | 27 => {
                let i = ins.src.last().ok_or("a loop lacks its count")?;
                let c = *self.sh.int_defs.get(&i.num).ok_or("a loop's count is not a defined integer constant")?;
                self.n += 1;
                let k = format!("k{}", self.n);
                if ins.op == 27 {
                    let name = format!("l{}", self.n);
                    self.line(&format!("var {name}: i32 = {};", c[1]));
                    self.loops.push((name, c[2]));
                } else {
                    self.loops.push((String::new(), 0));
                }
                self.line(&format!("for (var {k}: i32 = 0; {k} < {}; {k} += 1) {{", c[0]));
                self.indent += 1;
            }
            29 | 39 => {
                let (name, step) = self.loops.pop().ok_or("endloop without a loop")?;
                if !name.is_empty() {
                    self.line(&format!("{name} += {step};"));
                }
                self.indent = self.indent.saturating_sub(1);
                self.line("}");
            }
            40 => {
                let b = ins.src.first().ok_or("if lacks its condition")?;
                let on = match self.sh.bool_defs.get(&b.num) {
                    Some(v) => *v,
                    None => (self.bools)(b.num),
                };
                let on = if b.modifier == 13 { !on } else { on };
                self.line(&format!("if ({on}) {{"));
                self.indent += 1;
            }
            41 => {
                let c = self.compare(ins.control, &ins.src[0], &ins.src[1])?;
                self.line(&format!("if {c} {{"));
                self.indent += 1;
            }
            42 => {
                self.indent = self.indent.saturating_sub(1);
                self.line("} else {");
                self.indent += 1;
            }
            43 => {
                self.indent = self.indent.saturating_sub(1);
                self.line("}");
            }
            44 => self.line("break;"),
            45 => {
                let c = self.compare(ins.control, &ins.src[0], &ins.src[1])?;
                self.line(&format!("if {c} {{ break; }}"));
            }
            // texkill: discard when a masked component is negative.
            65 => {
                let d = d.ok_or("texkill lacks its register")?;
                let r = self.register(d.ty, d.num, None)?;
                let tests: Vec<String> = (0..4).filter(|k| d.mask >> k & 1 != 0).map(|k| format!("{r}.{} < 0.0", XYZW[k])).collect();
                self.line(&format!("if ({}) {{ discard; }}", tests.join(" || ")));
            }
            // texld (control bit 0: projected, bit 1: biased), texldd, texldl
            66 | 93 | 95 => {
                let coord = s(self, 0)?;
                let sampler = ins.src.get(1).ok_or("texld lacks its sampler")?;
                let (n, kind) = (sampler.num, self.sampler_kind(sampler.num));
                self.n += 1;
                let t = format!("t{}", self.n);
                self.line(&format!("let {t} = {coord};"));
                let uv = match kind {
                    SamplerKind::D2 if ins.op == 66 && ins.control & 1 != 0 => format!("({t}.xy / {t}.w)"),
                    SamplerKind::D2 => format!("{t}.xy"),
                    _ => format!("{t}.xyz"),
                };
                // A vertex shader's samplers are their own bindings, and it has no derivatives: it reads a mip.
                let vertex = self.sh.stage == Stage::Vertex;
                let (tex, smp) = if vertex { (format!("vtex{n}"), format!("vsmp{n}")) } else { (format!("tex{n}"), format!("smp{n}")) };
                let call = match ins.op {
                    66 if vertex => format!("textureSampleLevel({tex}, {smp}, {uv}, 0.0)"),
                    66 if ins.control & 2 != 0 => format!("textureSampleBias({tex}, {smp}, {uv}, {t}.w)"),
                    66 => format!("textureSample({tex}, {smp}, {uv})"),
                    95 => format!("textureSampleLevel({tex}, {smp}, {uv}, {t}.w)"),
                    _ => {
                        let (dx, dy) = (s(self, 2)?, s(self, 3)?);
                        let dims = if kind == SamplerKind::D2 { "xy" } else { "xyz" };
                        format!("textureSampleGrad({tex}, {smp}, {uv}, {dx}.{dims}, {dy}.{dims})")
                    }
                };
                // The sampler's own swizzle orders the result's channels.
                let sw: String = sampler.swizzle.iter().map(|c| XYZW[*c as usize]).collect();
                store(self, if sw == "xyzw" { call } else { format!("{call}.{sw}") })?
            }
            88 => {
                let v = format!("select({}, {}, {} >= vec4<f32>(0.0))", s(self, 2)?, s(self, 1)?, s(self, 0)?);
                store(self, v)?
            }
            90 => {
                let v = format!("vec4<f32>(dot({}.xy, {}.xy) + {}.x)", s(self, 0)?, s(self, 1)?, s(self, 2)?);
                store(self, v)?
            }
            91 => {
                let v = format!("dpdx({})", s(self, 0)?);
                store(self, v)?
            }
            92 => {
                let v = format!("dpdy({})", s(self, 0)?);
                store(self, v)?
            }
            other => return err(format!("instruction {other} is not handled")),
        }
        Ok(())
    }

    fn body(&mut self) -> Result<String, String> {
        let ins = self.sh.ins.clone();
        for i in &ins {
            self.instruction(i)?;
        }
        if self.indent != 1 || !self.loops.is_empty() {
            return err("unbalanced flow control");
        }
        Ok(std::mem::take(&mut self.out))
    }
}

/// A vertex and pixel shader as one WGSL module.
#[derive(Clone, Debug)]
pub struct Module {
    pub wgsl: String,
    /// Vertex inputs in `@location` order.
    pub vertex_inputs: Vec<Semantic>,
    /// Float constant registers each stage's uniform holds (`array<vec4<f32>, N>`).
    pub vertex_floats: usize,
    pub pixel_floats: usize,
    /// The pixel shader's samplers: (register, kind); texture at binding `2 * k`, sampler at `2 * k + 1` of group 1, in this order.
    pub samplers: Vec<(u16, SamplerKind)>,
    /// The vertex shader's samplers, likewise in group 2 (there is no group 2 when it has none).
    pub vertex_samplers: Vec<(u16, SamplerKind)>,
    /// The vertex constants read per placement (group 0 binding 2, `draws`, a storage
    /// array indexed by the instance; `vsd` is the instance's block):
    /// (name, first slot, rows), and how many rows a block holds (at least 1).
    pub draw_constants: Vec<(String, u16, u16)>,
    pub draw_floats: usize,
}

/// Vertex constants that differ for each object drawn (`FLocalVertexFactory`'s
/// placement): the translation reads them from their own uniform, so a renderer
/// drawing many objects rewrites only these between draws.
pub const PER_DRAW: &[&str] = &["LocalToWorld", "WorldToLocal", "LocalToWorldRotDeterminantFlip"];

/// Translate a `vs_3_0` and `ps_3_0` pair. Group 0 binding 0 is the vertex
/// stage's float constants, binding 1 the pixel stage's; group 1 the pixel
/// shader's textures and samplers, group 2 the vertex shader's when it has any. `vertex_bools` / `pixel_bools` answer the
/// boolean constants (`if b#`) by register.
pub fn translate(vs: &Shader, ps: &Shader, vertex_bools: &dyn Fn(u16) -> bool, pixel_bools: &dyn Fn(u16) -> bool) -> Result<Module, String> {
    if vs.stage != Stage::Vertex || ps.stage != Stage::Pixel {
        return err("translate takes a vertex shader then a pixel shader");
    }
    let (vfloats, pfloats) = (vs.float_registers(), ps.float_registers());
    let mut w = String::new();
    let _ = writeln!(w, "// Translated from the game's Direct3D 9 shader model 3 bytecode.");
    let _ = writeln!(w, "struct VsConstants {{ c: array<vec4<f32>, {vfloats}> }}");
    let _ = writeln!(w, "struct PsConstants {{ c: array<vec4<f32>, {pfloats}> }}");
    let _ = writeln!(w, "@group(0) @binding(0) var<uniform> vsc: VsConstants;");
    let _ = writeln!(w, "@group(0) @binding(1) var<uniform> psc: PsConstants;");
    // The per-draw constants, in `PER_DRAW` order.
    let mut draw_map = BTreeMap::new();
    let mut draw_constants = Vec::new();
    let mut slot = 0u16;
    for name in PER_DRAW {
        if let Some(c) = vs.constants.iter().find(|c| c.set == RegisterSet::Float && c.name.eq_ignore_ascii_case(name)) {
            for k in 0..c.count {
                draw_map.insert(c.register + k, slot + k);
            }
            draw_constants.push((c.name.clone(), slot, c.count));
            slot += c.count;
        }
    }
    let draw_floats = (slot as usize).max(1);
    let _ = writeln!(w, "struct DrawConstants {{ c: array<vec4<f32>, {draw_floats}> }}");
    // Per placement, read by instance: a level draws every placement of a part in one call.
    let _ = writeln!(w, "@group(0) @binding(2) var<storage, read> draws: array<DrawConstants>;");
    let _ = writeln!(w, "var<private> vsd: DrawConstants;");
    let (mut samplers, mut vertex_samplers) = (ps.samplers.clone(), vs.samplers.clone());
    samplers.sort_by_key(|s| s.0);
    vertex_samplers.sort_by_key(|s| s.0);
    for (group, prefix, list) in [(1, "", &samplers), (2, "v", &vertex_samplers)] {
        for (k, (n, kind)) in list.iter().enumerate() {
            let ty = match kind {
                SamplerKind::D2 => "texture_2d<f32>",
                SamplerKind::Cube => "texture_cube<f32>",
                SamplerKind::Volume => "texture_3d<f32>",
            };
            let _ = writeln!(w, "@group({group}) @binding({}) var {prefix}tex{n}: {ty};", 2 * k);
            let _ = writeln!(w, "@group({group}) @binding({}) var {prefix}smp{n}: sampler;", 2 * k + 1);
        }
    }
    // Vertex inputs, in register order.
    let mut vin = vs.inputs.clone();
    vin.sort_by_key(|i| i.0);
    let _ = writeln!(w, "struct VsIn {{");
    for (k, (n, _)) in vin.iter().enumerate() {
        let _ = writeln!(w, "    @location({k}) v{n}: vec4<f32>,");
    }
    let _ = writeln!(w, "}}");
    // Interpolants: every vertex output but the position (and point size, fog) gets a location; the pixel inputs find theirs by usage.
    let varying: Vec<(u16, Semantic)> = vs.outputs.iter().filter(|o| !matches!(o.1.usage, Semantic::POSITION | Semantic::PSIZE | Semantic::FOG) || (o.1.usage == Semantic::POSITION && o.1.index > 0)).map(|o| (o.0, o.1)).collect();
    let _ = writeln!(w, "struct Varyings {{");
    let _ = writeln!(w, "    @builtin(position) position: vec4<f32>,");
    for (k, (_, sem)) in varying.iter().enumerate() {
        let _ = writeln!(w, "    @location({k}) i{k}: vec4<f32>, // {}", sem.name());
    }
    let _ = writeln!(w, "}}");

    // The vertex stage.
    let _ = writeln!(w, "@vertex fn vs_main(input: VsIn, @builtin(instance_index) instance: u32) -> Varyings {{");
    let _ = writeln!(w, "    vsd = draws[instance];");
    let _ = writeln!(w, "    const FLOATS: i32 = {vfloats};");
    for (n, _) in &vin {
        let _ = writeln!(w, "    let v{n} = input.v{n};");
    }
    let mut e = Emit { sh: vs, consts: "vsc", bools: vertex_bools, out: String::new(), indent: 1, temps: BTreeSet::new(), n: 0, loops: Vec::new(), draw: draw_map.clone() };
    let body = e.body()?;
    let outs: BTreeSet<u16> = vs.outputs.iter().map(|o| o.0).collect();
    for n in &outs {
        let _ = writeln!(w, "    var o{n}: vec4<f32> = vec4<f32>(0.0, 0.0, 0.0, 1.0);");
    }
    for n in &e.temps {
        let _ = writeln!(w, "    var r{n}: vec4<f32> = vec4<f32>(0.0);");
    }
    let _ = writeln!(w, "    var a0: vec4<i32> = vec4<i32>(0);");
    w.push_str(&body);
    let _ = writeln!(w, "    var out: Varyings;");
    match vs.outputs.iter().find(|o| o.1 == Semantic { usage: Semantic::POSITION, index: 0 }) {
        Some((n, _, _)) => {
            let _ = writeln!(w, "    out.position = o{n};");
        }
        None => return err("the vertex shader writes no position"),
    }
    for (k, (n, _)) in varying.iter().enumerate() {
        let _ = writeln!(w, "    out.i{k} = o{n};");
    }
    let _ = writeln!(w, "    return out;");
    let _ = writeln!(w, "}}");

    // The pixel stage.
    let _ = writeln!(w, "@fragment fn fs_main(input: Varyings, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {{");
    let _ = writeln!(w, "    const FLOATS: i32 = {pfloats};");
    let _ = writeln!(w, "    let vpos = vec4<f32>(input.position.xy, 0.0, 0.0);");
    let _ = writeln!(w, "    let vface = vec4<f32>(select(-1.0, 1.0, front));");
    for (n, sem) in &ps.inputs {
        match varying.iter().position(|v| v.1 == *sem) {
            Some(k) => {
                let _ = writeln!(w, "    let v{n} = input.i{k};");
            }
            None => {
                let _ = writeln!(w, "    let v{n} = vec4<f32>(0.0); // {} is not written by the vertex shader", sem.name());
            }
        }
    }
    let mut e = Emit { sh: ps, consts: "psc", bools: pixel_bools, out: String::new(), indent: 1, temps: BTreeSet::new(), n: 0, loops: Vec::new(), draw: BTreeMap::new() };
    let body = e.body()?;
    for n in &e.temps {
        let _ = writeln!(w, "    var r{n}: vec4<f32> = vec4<f32>(0.0);");
    }
    for k in 0..4 {
        let _ = writeln!(w, "    var oc{k}: vec4<f32> = vec4<f32>(0.0);");
    }
    let _ = writeln!(w, "    var odepth: vec4<f32> = vec4<f32>(0.0);");
    let _ = writeln!(w, "    var a0: vec4<i32> = vec4<i32>(0);");
    w.push_str(&body);
    let _ = writeln!(w, "    return oc0;");
    let _ = writeln!(w, "}}");
    Ok(Module { wgsl: w, vertex_inputs: vin.iter().map(|i| i.1).collect(), vertex_floats: vfloats, pixel_floats: pfloats, samplers, vertex_samplers, draw_constants, draw_floats })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assemble tokens: version, instructions, end.
    fn code(tokens: &[u32]) -> Vec<u8> {
        tokens.iter().flat_map(|t| t.to_le_bytes()).collect()
    }

    fn dst(ty: u32, num: u32, mask: u32) -> u32 {
        0x8000_0000 | ((ty & 7) << 28) | ((ty & 0x18) << 8) | (mask << 16) | num
    }

    fn src(ty: u32, num: u32, swizzle: u32) -> u32 {
        0x8000_0000 | ((ty & 7) << 28) | ((ty & 0x18) << 8) | (swizzle << 16) | num
    }

    const XYZW_SW: u32 = 0xe4;

    #[test]
    fn a_minimal_pair_translates() {
        // vs: dcl_position v0; dcl_position o0; dcl_texcoord o1.xy; m4x4 o0, v0, c0; mov o1.xy, v0
        let vs = code(&[
            0xFFFE_0300,
            0x0200_001f, 0x8000_0000, dst(1, 0, 0xf),
            0x0200_001f, 0x8000_0000, dst(6, 0, 0xf),
            0x0200_001f, 0x8000_0005, dst(6, 1, 0x3),
            0x0300_0014, dst(6, 0, 0xf), src(1, 0, XYZW_SW), src(2, 0, XYZW_SW),
            0x0200_0001, dst(6, 1, 0x3), src(1, 0, XYZW_SW),
            0x0000_FFFF,
        ]);
        // ps: def c1, 0.5, 0, 0, 1; dcl_texcoord v0.xy; dcl_2d s0; texld r0, v0, s0; mul_sat oC0, r0, c1.x
        let ps = code(&[
            0xFFFF_0300,
            0x0500_0051, dst(2, 1, 0xf), 0.5f32.to_bits(), 0, 0, 1.0f32.to_bits(),
            0x0200_001f, 0x8000_0005, dst(1, 0, 0x3),
            0x0200_001f, 0x9000_0000, dst(10, 0, 0xf),
            0x0300_0042, dst(0, 0, 0xf), src(1, 0, XYZW_SW), src(10, 0, XYZW_SW),
            0x0300_0005, dst(8, 0, 0xf) | (1 << 20), src(0, 0, XYZW_SW), src(2, 1, 0),
            0x0000_FFFF,
        ]);
        let (vs, ps) = (Shader::parse(&vs).unwrap(), Shader::parse(&ps).unwrap());
        assert_eq!(vs.inputs, [(0, Semantic { usage: 0, index: 0 })]);
        assert_eq!(vs.outputs.len(), 2);
        assert_eq!(ps.samplers, [(0, SamplerKind::D2)]);
        assert_eq!(ps.float_defs[&1], [0.5, 0.0, 0.0, 1.0]);
        let m = translate(&vs, &ps, &|_| false, &|_| false).unwrap();
        // The position is four dot products with the rows c0..c3; the interpolant is matched by usage.
        assert!(m.wgsl.contains("o0 = vec4<f32>(dot(v0, vsc.c[0]), dot(v0, vsc.c[1]), dot(v0, vsc.c[2]), dot(v0, vsc.c[3]));"), "{}", m.wgsl);
        assert!(m.wgsl.contains("let v0 = input.i0;"));
        assert!(m.wgsl.contains("textureSample(tex0, smp0, t1.xy)"));
        // The defined constant is a literal, swizzled, and the result saturated.
        assert!(m.wgsl.contains("oc0 = clamp((r0 * vec4<f32>(0.5, 0.0, 0.0, 1.0).xxxx), vec4<f32>(0.0), vec4<f32>(1.0));"), "{}", m.wgsl);
        assert_eq!((m.vertex_floats, m.pixel_floats), (4, 1));
        // No placement constant: a placement's block has one unused row.
        assert!(m.draw_constants.is_empty() && m.draw_floats == 1);
    }

    #[test]
    fn a_masked_write_keeps_the_other_components() {
        let ps = code(&[0xFFFF_0300, 0x0200_0001, dst(0, 2, 0x5), src(2, 3, XYZW_SW), 0x0000_FFFF]);
        let ps = Shader::parse(&ps).unwrap();
        let mut e = Emit { sh: &ps, consts: "psc", bools: &|_| false, out: String::new(), indent: 1, temps: BTreeSet::new(), n: 0, loops: Vec::new(), draw: BTreeMap::new() };
        let body = e.body().unwrap();
        assert!(body.contains("let t1 = psc.c[3];"));
        assert!(body.contains("r2 = vec4<f32>(t1.x, r2.y, t1.z, r2.w);"), "{body}");
    }
}
