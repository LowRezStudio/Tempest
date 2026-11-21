//! Direct3D 11 shader model 5 bytecode (`ShaderCodeStorage-PC-D3D-SM5.bin`):
//! the `DXBC` container, its input and output signatures (`ISGN`, `OSGN`)
//! and the token stream (`SHEX`), decoded into instructions with typed
//! operands. Hi-Rez's blobs keep no `RDEF`: constant buffers are bound by
//! the engine's own parameter records (see `material_shader`).
//!
//! References: d3d11TokenizedProgramFormat.hpp (the token layouts, opcode
//! and operand type numbers), the shader model 5 assembly reference.

use std::fmt::Write as _;

/// A signature element: a semantic, its register and the components used.
#[derive(Clone, Debug, PartialEq)]
pub struct SigElement {
    pub name: String,
    pub index: u32,
    /// `D3D_NAME`: 0 none, 1 position, 9 is front face, 64 target…
    pub system_value: u32,
    /// `D3D_REGISTER_COMPONENT_TYPE`: 1 uint, 2 sint, 3 float.
    pub component_type: u32,
    pub register: u32,
    pub mask: u8,
}

impl SigElement {
    /// `TEXCOORD3`, `POSITION0`… (the system values by their own name).
    pub fn semantic(&self) -> String {
        format!("{}{}", self.name.to_ascii_uppercase(), self.index)
    }
}

/// Program type of the version token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pixel,
    Vertex,
    Other(u32),
}

/// How an operand index is given.
#[derive(Clone, Debug, PartialEq)]
pub enum Index {
    Imm(u32),
    /// `[operand + imm]`.
    Rel(Box<Operand>, u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Operand {
    /// `D3D10_SB_OPERAND_TYPE`.
    pub ty: u32,
    /// Components: 0, 1 or 4.
    pub components: u8,
    /// Selection: a write mask (destinations), a swizzle, or one component.
    pub mask: u8,
    pub swizzle: [u8; 4],
    pub indices: Vec<Index>,
    /// Immediate values (`l(…)`), as bits.
    pub imm: Vec<u32>,
    /// 1 negate, 2 absolute, 3 both.
    pub modifier: u8,
}

pub mod op_type {
    pub const TEMP: u32 = 0;
    pub const INPUT: u32 = 1;
    pub const OUTPUT: u32 = 2;
    pub const INDEXABLE_TEMP: u32 = 3;
    pub const IMMEDIATE32: u32 = 4;
    pub const IMMEDIATE64: u32 = 5;
    pub const SAMPLER: u32 = 6;
    pub const RESOURCE: u32 = 7;
    pub const CONSTANT_BUFFER: u32 = 8;
    pub const IMMEDIATE_CONSTANT_BUFFER: u32 = 9;
    pub const LABEL: u32 = 10;
    pub const INPUT_PRIMITIVEID: u32 = 11;
    pub const OUTPUT_DEPTH: u32 = 12;
    pub const NULL: u32 = 13;
    pub const INPUT_COVERAGE_MASK: u32 = 35;
    pub const OUTPUT_DEPTH_GE: u32 = 38;
    pub const OUTPUT_DEPTH_LE: u32 = 39;
}

/// An instruction: its opcode, the opcode's control bits, the extended
/// tokens' texel offsets, and the operands (or a declaration's raw dwords).
#[derive(Clone, Debug, PartialEq)]
pub struct Instruction {
    pub opcode: u32,
    pub control: u32,
    pub offsets: [i8; 3],
    pub operands: Vec<Operand>,
    /// Declarations and custom data: the dwords after the opcode token.
    pub raw: Vec<u32>,
}

impl Instruction {
    pub fn saturate(&self) -> bool {
        self.control & (1 << 2) != 0
    }
    /// `_nz` of `if`, `breakc`, `discard`…: test non-zero (else zero).
    pub fn test_nonzero(&self) -> bool {
        self.control & (1 << 7) != 0
    }
}

pub mod opcode {
    pub const ADD: u32 = 0;
    pub const AND: u32 = 1;
    pub const BREAK: u32 = 2;
    pub const BREAKC: u32 = 3;
    pub const CASE: u32 = 6;
    pub const CONTINUE: u32 = 7;
    pub const CONTINUEC: u32 = 8;
    pub const DEFAULT: u32 = 10;
    pub const DERIV_RTX: u32 = 11;
    pub const DERIV_RTY: u32 = 12;
    pub const DISCARD: u32 = 13;
    pub const DIV: u32 = 14;
    pub const DP2: u32 = 15;
    pub const DP3: u32 = 16;
    pub const DP4: u32 = 17;
    pub const ELSE: u32 = 18;
    pub const ENDIF: u32 = 21;
    pub const ENDLOOP: u32 = 22;
    pub const ENDSWITCH: u32 = 23;
    pub const EQ: u32 = 24;
    pub const EXP: u32 = 25;
    pub const FRC: u32 = 26;
    pub const FTOI: u32 = 27;
    pub const FTOU: u32 = 28;
    pub const GE: u32 = 29;
    pub const IADD: u32 = 30;
    pub const IF: u32 = 31;
    pub const IEQ: u32 = 32;
    pub const IGE: u32 = 33;
    pub const ILT: u32 = 34;
    pub const IMAD: u32 = 35;
    pub const IMAX: u32 = 36;
    pub const IMIN: u32 = 37;
    pub const IMUL: u32 = 38;
    pub const INE: u32 = 39;
    pub const INEG: u32 = 40;
    pub const ISHL: u32 = 41;
    pub const ISHR: u32 = 42;
    pub const ITOF: u32 = 43;
    pub const LD: u32 = 45;
    pub const LOG: u32 = 47;
    pub const LOOP: u32 = 48;
    pub const LT: u32 = 49;
    pub const MAD: u32 = 50;
    pub const MIN: u32 = 51;
    pub const MAX: u32 = 52;
    pub const CUSTOMDATA: u32 = 53;
    pub const MOV: u32 = 54;
    pub const MOVC: u32 = 55;
    pub const MUL: u32 = 56;
    pub const NE: u32 = 57;
    pub const NOP: u32 = 58;
    pub const NOT: u32 = 59;
    pub const OR: u32 = 60;
    pub const RESINFO: u32 = 61;
    pub const RET: u32 = 62;
    pub const RETC: u32 = 63;
    pub const ROUND_NE: u32 = 64;
    pub const ROUND_NI: u32 = 65;
    pub const ROUND_PI: u32 = 66;
    pub const ROUND_Z: u32 = 67;
    pub const RSQ: u32 = 68;
    pub const SAMPLE: u32 = 69;
    pub const SAMPLE_C: u32 = 70;
    pub const SAMPLE_C_LZ: u32 = 71;
    pub const SAMPLE_L: u32 = 72;
    pub const SAMPLE_D: u32 = 73;
    pub const SAMPLE_B: u32 = 74;
    pub const SQRT: u32 = 75;
    pub const SWITCH: u32 = 76;
    pub const SINCOS: u32 = 77;
    pub const UDIV: u32 = 78;
    pub const ULT: u32 = 79;
    pub const UGE: u32 = 80;
    pub const UMUL: u32 = 81;
    pub const UMAD: u32 = 82;
    pub const UMAX: u32 = 83;
    pub const UMIN: u32 = 84;
    pub const USHR: u32 = 85;
    pub const UTOF: u32 = 86;
    pub const XOR: u32 = 87;
    pub const DCL_RESOURCE: u32 = 88;
    pub const DCL_CONSTANT_BUFFER: u32 = 89;
    pub const DCL_SAMPLER: u32 = 90;
    pub const DCL_INPUT: u32 = 95;
    pub const DCL_INPUT_SGV: u32 = 96;
    pub const DCL_INPUT_SIV: u32 = 97;
    pub const DCL_INPUT_PS: u32 = 98;
    pub const DCL_INPUT_PS_SGV: u32 = 99;
    pub const DCL_INPUT_PS_SIV: u32 = 100;
    pub const DCL_OUTPUT: u32 = 101;
    pub const DCL_OUTPUT_SGV: u32 = 102;
    pub const DCL_OUTPUT_SIV: u32 = 103;
    pub const DCL_TEMPS: u32 = 104;
    pub const DCL_INDEXABLE_TEMP: u32 = 105;
    pub const DCL_GLOBAL_FLAGS: u32 = 106;
    pub const LOD: u32 = 108;
    pub const GATHER4: u32 = 109;
    pub const DERIV_RTX_COARSE: u32 = 122;
    pub const DERIV_RTX_FINE: u32 = 123;
    pub const DERIV_RTY_COARSE: u32 = 124;
    pub const DERIV_RTY_FINE: u32 = 125;
    pub const RCP: u32 = 129;
    pub const F32TOF16: u32 = 130;
    pub const F16TOF32: u32 = 131;
    pub const COUNTBITS: u32 = 134;
    pub const UBFE: u32 = 138;
    pub const IBFE: u32 = 139;
    pub const BFI: u32 = 140;
    pub const BFREV: u32 = 141;
    pub const SWAPC: u32 = 142;
    pub const DCL_RESOURCE_RAW: u32 = 161;
    pub const DCL_RESOURCE_STRUCTURED: u32 = 162;
}

/// Opcodes that declare (their dwords are kept raw, except those with a register operand first).
fn is_declaration(op: u32) -> bool {
    (88..=106).contains(&op) || (143..=162).contains(&op) || op == opcode::CUSTOMDATA
}

/// A parsed shader.
#[derive(Clone, Debug)]
pub struct Shader {
    pub kind: Kind,
    pub inputs: Vec<SigElement>,
    pub outputs: Vec<SigElement>,
    pub instructions: Vec<Instruction>,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

fn signature(chunk: &[u8]) -> Result<Vec<SigElement>, String> {
    let count = u32_at(chunk, 0).ok_or("a signature without its count")? as usize;
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let at = 8 + 24 * k;
        let name_at = u32_at(chunk, at).ok_or("a signature element out of range")? as usize;
        let name = chunk.get(name_at..).map(|b| String::from_utf8_lossy(&b[..b.iter().position(|c| *c == 0).unwrap_or(b.len())]).to_string()).unwrap_or_default();
        out.push(SigElement {
            name,
            index: u32_at(chunk, at + 4).unwrap_or(0),
            system_value: u32_at(chunk, at + 8).unwrap_or(0),
            component_type: u32_at(chunk, at + 12).unwrap_or(0),
            register: u32_at(chunk, at + 16).unwrap_or(0),
            mask: chunk.get(at + 20).copied().unwrap_or(0),
        });
    }
    Ok(out)
}

struct Tokens<'a> {
    d: &'a [u32],
    at: usize,
}

impl Tokens<'_> {
    fn next(&mut self) -> Result<u32, String> {
        let v = *self.d.get(self.at).ok_or("the token stream ends inside an instruction")?;
        self.at += 1;
        Ok(v)
    }

    fn operand(&mut self) -> Result<Operand, String> {
        let t = self.next()?;
        let components = match t & 3 {
            0 => 0,
            1 => 1,
            2 => 4,
            _ => return Err("an operand with N components".into()),
        };
        let selection = (t >> 2) & 3;
        let (mut mask, mut swizzle) = (0xf, [0, 1, 2, 3]);
        if components == 4 {
            match selection {
                0 => mask = ((t >> 4) & 0xf) as u8,
                1 => swizzle = [0, 1, 2, 3].map(|k| ((t >> (4 + 2 * k)) & 3) as u8),
                _ => {
                    let c = ((t >> 4) & 3) as u8;
                    swizzle = [c; 4];
                    mask = 1 << c;
                }
            }
        }
        let ty = (t >> 12) & 0xff;
        let dims = (t >> 20) & 3;
        let reps = [(t >> 22) & 7, (t >> 25) & 7, (t >> 28) & 7];
        let mut modifier = 0;
        let mut ext = t & 0x8000_0000 != 0;
        while ext {
            let e = self.next()?;
            if e & 0x3f == 1 {
                modifier = ((e >> 6) & 0xff) as u8;
            }
            ext = e & 0x8000_0000 != 0;
        }
        let mut imm = Vec::new();
        match ty {
            op_type::IMMEDIATE32 => {
                for _ in 0..components.max(1) {
                    imm.push(self.next()?);
                }
            }
            op_type::IMMEDIATE64 => {
                for _ in 0..components.max(1) * 2 {
                    imm.push(self.next()?);
                }
            }
            _ => {}
        }
        let mut indices = Vec::new();
        for rep in reps.iter().take(dims as usize) {
            indices.push(match rep {
                0 => Index::Imm(self.next()?),
                1 => {
                    let lo = self.next()?;
                    let _hi = self.next()?;
                    Index::Imm(lo)
                }
                2 => Index::Rel(Box::new(self.operand()?), 0),
                3 => {
                    let base = self.next()?;
                    Index::Rel(Box::new(self.operand()?), base)
                }
                r => return Err(format!("operand index representation {r}")),
            });
        }
        Ok(Operand { ty, components, mask, swizzle, indices, imm, modifier })
    }
}

impl Shader {
    /// Parse a `DXBC` container (from its magic).
    pub fn parse(blob: &[u8]) -> Result<Shader, String> {
        if blob.get(0..4) != Some(b"DXBC") {
            return Err("not a DXBC container".into());
        }
        let count = u32_at(blob, 28).ok_or("a truncated container")? as usize;
        let (mut inputs, mut outputs, mut code) = (Vec::new(), Vec::new(), None);
        for k in 0..count {
            let at = u32_at(blob, 32 + 4 * k).ok_or("a truncated chunk table")? as usize;
            let tag = blob.get(at..at + 4).ok_or("a chunk out of range")?;
            let size = u32_at(blob, at + 4).ok_or("a chunk out of range")? as usize;
            let body = blob.get(at + 8..at + 8 + size).ok_or("a chunk out of range")?;
            match tag {
                b"ISGN" => inputs = signature(body)?,
                b"OSGN" => outputs = signature(body)?,
                b"SHEX" | b"SHDR" => code = Some(body),
                _ => {}
            }
        }
        let code = code.ok_or("no SHEX chunk")?;
        let d: Vec<u32> = code.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let version = *d.first().ok_or("an empty program")?;
        let kind = match version >> 16 {
            0 => Kind::Pixel,
            1 => Kind::Vertex,
            k => Kind::Other(k),
        };
        let len = (*d.get(1).ok_or("an empty program")? as usize).min(d.len());
        let mut t = Tokens { d: &d[..len], at: 2 };
        let mut instructions = Vec::new();
        while t.at < len {
            let start = t.at;
            let tok = t.next()?;
            let op = tok & 0x7ff;
            let control = (tok >> 11) & 0x1fff;
            let mut n = ((tok >> 24) & 0x7f) as usize;
            if op == opcode::CUSTOMDATA {
                n = t.next()? as usize;
            }
            if n == 0 {
                return Err(format!("instruction {op} has no length"));
            }
            let end = start + n;
            let mut offsets = [0i8; 3];
            let mut ext = tok & 0x8000_0000 != 0 && op != opcode::CUSTOMDATA;
            while ext {
                let e = t.next()?;
                if e & 0x3f == 1 {
                    let s = |shift: u32| ((((e >> shift) & 0xf) << 4) as u8 as i8) >> 4;
                    offsets = [s(9), s(13), s(17)];
                }
                ext = e & 0x8000_0000 != 0;
            }
            let mut ins = Instruction { opcode: op, control, offsets, operands: Vec::new(), raw: Vec::new() };
            if is_declaration(op) {
                ins.raw = d.get(t.at..end).ok_or("a declaration out of range")?.to_vec();
                // Declarations naming a register: decode it too.
                if !matches!(op, opcode::CUSTOMDATA | opcode::DCL_TEMPS | opcode::DCL_GLOBAL_FLAGS | opcode::DCL_INDEXABLE_TEMP) {
                    let mut sub = Tokens { d: &d[..end], at: t.at };
                    if let Ok(o) = sub.operand() {
                        ins.operands.push(o);
                    }
                }
            } else {
                while t.at < end {
                    ins.operands.push(t.operand()?);
                }
            }
            t.at = end;
            instructions.push(ins);
        }
        Ok(Shader { kind, inputs, outputs, instructions })
    }

    /// The constant buffers declared: (slot, size in registers).
    pub fn constant_buffers(&self) -> Vec<(u32, u32)> {
        self.instructions
            .iter()
            .filter(|i| i.opcode == opcode::DCL_CONSTANT_BUFFER)
            .filter_map(|i| {
                let o = i.operands.first()?;
                match (o.indices.first(), o.indices.get(1)) {
                    (Some(Index::Imm(slot)), Some(Index::Imm(size))) => Some((*slot, *size)),
                    _ => None,
                }
            })
            .collect()
    }

    /// A short listing (opcodes, operand types and indices), for debugging.
    pub fn listing(&self) -> String {
        let mut s = String::new();
        for i in &self.instructions {
            let _ = write!(s, "{}", i.opcode);
            for o in &i.operands {
                let idx: Vec<String> = o
                    .indices
                    .iter()
                    .map(|x| match x {
                        Index::Imm(v) => v.to_string(),
                        Index::Rel(r, b) => format!("{}{:?}+{b}", r.ty, r.indices),
                    })
                    .collect();
                let _ = write!(s, " t{}[{}]", o.ty, idx.join(","));
            }
            if !i.raw.is_empty() {
                let _ = write!(s, " raw{:?}", i.raw);
            }
            s.push('\n');
        }
        s
    }
}

// ----- translation to WGSL -------------------------------------------------------

/// Where a stage's constant buffers live in its one flat register array
/// (`vsc` / `psc` of the WGSL, the same uniforms as a `d3d9` translation):
/// constant buffer slot → first register.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub buffers: Vec<(u32, u32)>,
    pub registers: u32,
}

impl Layout {
    /// Every buffer the shader declares, one after the other in slot order.
    pub fn of(sh: &Shader) -> Layout {
        let mut cbs = sh.constant_buffers();
        cbs.sort();
        let mut at = 0;
        let mut buffers = Vec::new();
        for (slot, size) in cbs {
            buffers.push((slot, at));
            at += size.max(1);
        }
        Layout { buffers, registers: at.max(1) }
    }

    pub fn base(&self, slot: u32) -> Option<u32> {
        self.buffers.iter().find(|b| b.0 == slot).map(|b| b.1)
    }
}

/// A texture a stage samples: its slot and dimension (`D3D10_SB_RESOURCE_DIMENSION`: 3 2D, 5 3D, 6 cube).
pub fn textures(sh: &Shader) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = sh
        .instructions
        .iter()
        .filter(|i| i.opcode == opcode::DCL_RESOURCE)
        .filter_map(|i| match i.operands.first()?.indices.first()? {
            Index::Imm(t) => Some((*t, i.control & 0x1f)),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

const XYZW: [char; 4] = ['x', 'y', 'z', 'w'];

struct Emit<'a> {
    sh: &'a Shader,
    /// `v` or `p`: the stage's prefix for its registers.
    p: char,
    consts: &'static str,
    layout: &'a Layout,
    /// Flat registers read from the placement's constants: register → slot.
    draw: &'a std::collections::BTreeMap<u32, u32>,
    out: String,
    indent: usize,
    n: usize,
    /// Open `switch` blocks: whether a case body is open.
    switches: Vec<bool>,
}

impl Emit<'_> {
    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn tmp(&mut self) -> String {
        self.n += 1;
        format!("t{}", self.n)
    }

    fn imm_index(i: &Index) -> Result<u32, String> {
        match i {
            Index::Imm(v) => Ok(*v),
            Index::Rel(..) => Err("a register index that should be immediate is relative".into()),
        }
    }

    /// An index as an `i32` expression (immediate, or a register component + offset).
    fn index_expr(&mut self, i: &Index, base: i64) -> Result<String, String> {
        Ok(match i {
            Index::Imm(v) => format!("{}", *v as i64 + base),
            Index::Rel(op, imm) => {
                let r = self.raw(op)?;
                let c = XYZW[op.swizzle[0] as usize];
                format!("(bitcast<i32>({r}.{c}) + {})", *imm as i64 + base)
            }
        })
    }

    /// An operand's register as a `vec4<u32>` expression (no swizzle).
    fn raw(&mut self, o: &Operand) -> Result<String, String> {
        let p = self.p;
        Ok(match o.ty {
            op_type::TEMP => format!("{p}r{}", Self::imm_index(&o.indices[0])?),
            op_type::INPUT => format!("{p}v{}", Self::imm_index(&o.indices[0])?),
            op_type::OUTPUT => format!("{p}o{}", Self::imm_index(&o.indices[0])?),
            op_type::INDEXABLE_TEMP => {
                let i = self.index_expr(&o.indices[1], 0)?;
                format!("{p}x{}[{i}]", Self::imm_index(&o.indices[0])?)
            }
            op_type::IMMEDIATE32 => {
                let v: Vec<String> = (0..4).map(|k| format!("{}u", o.imm.get(k).or(o.imm.first()).copied().unwrap_or(0))).collect();
                format!("vec4<u32>({})", v.join(", "))
            }
            op_type::CONSTANT_BUFFER => {
                let slot = Self::imm_index(&o.indices[0])?;
                let base = self.layout.base(slot).ok_or_else(|| format!("constant buffer {slot} is not declared"))? as i64;
                match &o.indices[1] {
                    Index::Imm(r) => {
                        let flat = base as u32 + r;
                        match self.draw.get(&flat) {
                            Some(s) => format!("bitcast<vec4<u32>>(vsd.c[{s}])"),
                            None => format!("bitcast<vec4<u32>>({}.c[{flat}])", self.consts),
                        }
                    }
                    rel => {
                        let i = self.index_expr(rel, base)?;
                        format!("bitcast<vec4<u32>>({}.c[u32(clamp({i}, 0, {}))])", self.consts, self.layout.registers - 1)
                    }
                }
            }
            op_type::IMMEDIATE_CONSTANT_BUFFER => {
                let i = self.index_expr(&o.indices[0], 0)?;
                format!("{p}icb[u32(max({i}, 0))]")
            }
            op_type::INPUT_PRIMITIVEID => "vec4<u32>(0u)".to_string(),
            op_type::INPUT_COVERAGE_MASK => "vec4<u32>(0xffffffffu)".to_string(),
            t => return Err(format!("operand type {t} is not handled")),
        })
    }

    fn swizzled(&mut self, o: &Operand) -> Result<String, String> {
        let r = self.raw(o)?;
        if o.components == 1 && o.ty == op_type::IMMEDIATE32 {
            return Ok(r);
        }
        let sw: String = o.swizzle.iter().map(|c| XYZW[*c as usize]).collect();
        Ok(if sw == "xyzw" { r } else { format!("{r}.{sw}") })
    }

    /// A source as floats, with its modifiers.
    fn f(&mut self, o: &Operand) -> Result<String, String> {
        let s = self.swizzled(o)?;
        let v = format!("bitcast<vec4<f32>>({s})");
        Ok(match o.modifier {
            1 => format!("(-{v})"),
            2 => format!("abs({v})"),
            3 => format!("(-abs({v}))"),
            _ => v,
        })
    }

    /// A source as signed integers (`-` negates).
    fn i(&mut self, o: &Operand) -> Result<String, String> {
        let s = self.swizzled(o)?;
        let v = format!("bitcast<vec4<i32>>({s})");
        Ok(if o.modifier & 1 != 0 { format!("(-{v})") } else { v })
    }

    fn u(&mut self, o: &Operand) -> Result<String, String> {
        self.swizzled(o)
    }

    /// Write a `vec4<u32>` value to the destination's masked components.
    fn store(&mut self, d: &Operand, value: String) -> Result<(), String> {
        if matches!(d.ty, op_type::NULL | op_type::OUTPUT_DEPTH | op_type::OUTPUT_DEPTH_GE | op_type::OUTPUT_DEPTH_LE) || d.ty == 15 {
            return Ok(());
        }
        let target = self.raw(d)?;
        let t = self.tmp();
        self.line(&format!("let {t} = {value};"));
        for (k, c) in XYZW.iter().enumerate() {
            if d.mask >> k & 1 != 0 {
                self.line(&format!("{target}.{c} = {t}.{c};"));
            }
        }
        Ok(())
    }

    /// Store a float result (saturated when the instruction says so).
    fn store_f(&mut self, ins: &Instruction, d: &Operand, value: String) -> Result<(), String> {
        let v = if ins.saturate() { format!("clamp({value}, vec4<f32>(0.0), vec4<f32>(1.0))") } else { value };
        self.store(d, format!("bitcast<vec4<u32>>({v})"))
    }

    fn cond(&mut self, ins: &Instruction, o: &Operand) -> Result<String, String> {
        let s = self.u(o)?;
        Ok(format!("({s}.x {} 0u)", if ins.test_nonzero() { "!=" } else { "==" }))
    }

    fn texture_call(&mut self, ins: &Instruction) -> Result<(), String> {
        let ops = &ins.operands;
        let (d, coord, res) = (&ops[0], &ops[1], &ops[2]);
        let t = Self::imm_index(&res.indices[0])?;
        let dim = textures(self.sh).iter().find(|x| x.0 == t).map(|x| x.1).unwrap_or(3);
        let c = self.f(coord)?;
        let (tex, smp) = (format!("{}tex{t}", if self.p == 'v' { "v" } else { "" }), format!("{}smp{t}", if self.p == 'v' { "v" } else { "" }));
        let (uv, offset) = match dim {
            6 | 5 => (format!("{c}.xyz"), String::new()),
            _ => (format!("{c}.xy"), if ins.offsets != [0; 3] { format!(", vec2<i32>({}, {})", ins.offsets[0], ins.offsets[1]) } else { String::new() }),
        };
        let vertex = self.p == 'v';
        let call = match ins.opcode {
            opcode::SAMPLE if vertex => format!("textureSampleLevel({tex}, {smp}, {uv}, 0.0{offset})"),
            opcode::SAMPLE => format!("textureSample({tex}, {smp}, {uv}{offset})"),
            opcode::SAMPLE_L => {
                let l = self.f(&ops[4])?;
                format!("textureSampleLevel({tex}, {smp}, {uv}, {l}.x{offset})")
            }
            opcode::SAMPLE_B if vertex => format!("textureSampleLevel({tex}, {smp}, {uv}, 0.0{offset})"),
            opcode::SAMPLE_B => {
                let b = self.f(&ops[4])?;
                format!("textureSampleBias({tex}, {smp}, {uv}, {b}.x{offset})")
            }
            opcode::SAMPLE_D => {
                let (dx, dy) = (self.f(&ops[4])?, self.f(&ops[5])?);
                let dims = if matches!(dim, 5 | 6) { "xyz" } else { "xy" };
                format!("textureSampleGrad({tex}, {smp}, {uv}, {dx}.{dims}, {dy}.{dims}{offset})")
            }
            opcode::LD => {
                let a = self.i(coord)?;
                format!("textureLoad({tex}, {a}.xy, {a}.w)")
            }
            o => return Err(format!("texture instruction {o} is not handled")),
        };
        // The resource's swizzle orders the result's channels.
        let sw: String = res.swizzle.iter().map(|c| XYZW[*c as usize]).collect();
        let v = if sw == "xyzw" { call } else { format!("{call}.{sw}") };
        self.store_f(ins, d, v)
    }

    fn instruction(&mut self, ins: &Instruction) -> Result<(), String> {
        use opcode::*;
        let ops = &ins.operands;
        let op = |k: usize| -> Result<&Operand, String> { ops.get(k).ok_or_else(|| format!("instruction {} lacks operand {k}", ins.opcode)) };
        macro_rules! float2 {
            ($f:expr) => {{
                let (a, b) = (self.f(op(1)?)?, self.f(op(2)?)?);
                let v = $f(a, b);
                self.store_f(ins, op(0)?, v)?
            }};
        }
        macro_rules! float1 {
            ($f:expr) => {{
                let a = self.f(op(1)?)?;
                let v = $f(a);
                self.store_f(ins, op(0)?, v)?
            }};
        }
        macro_rules! cmp {
            ($conv:ident, $o:expr) => {{
                let (a, b) = (self.$conv(op(1)?)?, self.$conv(op(2)?)?);
                self.store(op(0)?, format!("select(vec4<u32>(0u), vec4<u32>(0xffffffffu), {a} {} {b})", $o))?
            }};
        }
        macro_rules! int2 {
            ($conv:ident, $ty:expr, $f:expr) => {{
                let (a, b) = (self.$conv(op(1)?)?, self.$conv(op(2)?)?);
                let v = $f(a, b);
                self.store(op(0)?, if $ty == "u32" { v } else { format!("bitcast<vec4<u32>>({v})") })?
            }};
        }
        match ins.opcode {
            op if is_declaration(op) => {}
            NOP => {}
            MOV => {
                let s = op(1)?;
                if s.modifier != 0 || ins.saturate() {
                    float1!(|a| a)
                } else {
                    let v = self.u(s)?;
                    self.store(op(0)?, v)?
                }
            }
            MOVC => {
                let (c, a, b) = (self.u(op(1)?)?, self.u(op(2)?)?, self.u(op(3)?)?);
                if ins.saturate() {
                    let v = format!("bitcast<vec4<f32>>(select({b}, {a}, {c} != vec4<u32>(0u)))");
                    self.store_f(ins, op(0)?, v)?
                } else {
                    self.store(op(0)?, format!("select({b}, {a}, {c} != vec4<u32>(0u))"))?
                }
            }
            ADD => float2!(|a, b| format!("({a} + {b})")),
            MUL => float2!(|a, b| format!("({a} * {b})")),
            DIV => float2!(|a, b| format!("({a} / {b})")),
            MIN => float2!(|a, b| format!("min({a}, {b})")),
            MAX => float2!(|a, b| format!("max({a}, {b})")),
            DP2 => float2!(|a, b| format!("vec4<f32>(dot({a}.xy, {b}.xy))")),
            DP3 => float2!(|a, b| format!("vec4<f32>(dot({a}.xyz, {b}.xyz))")),
            DP4 => float2!(|a, b| format!("vec4<f32>(dot({a}, {b}))")),
            MAD => {
                let (a, b, c) = (self.f(op(1)?)?, self.f(op(2)?)?, self.f(op(3)?)?);
                self.store_f(ins, op(0)?, format!("({a} * {b} + {c})"))?
            }
            EXP => float1!(|a| format!("exp2({a})")),
            LOG => float1!(|a| format!("log2({a})")),
            FRC => float1!(|a| format!("fract({a})")),
            RSQ => float1!(|a| format!("inverseSqrt({a})")),
            SQRT => float1!(|a| format!("sqrt({a})")),
            RCP => float1!(|a| format!("(vec4<f32>(1.0) / {a})")),
            ROUND_NE => float1!(|a| format!("round({a})")),
            ROUND_NI => float1!(|a| format!("floor({a})")),
            ROUND_PI => float1!(|a| format!("ceil({a})")),
            ROUND_Z => float1!(|a| format!("trunc({a})")),
            DERIV_RTX | DERIV_RTX_COARSE => float1!(|a| format!("dpdxCoarse({a})")),
            DERIV_RTX_FINE => float1!(|a| format!("dpdxFine({a})")),
            DERIV_RTY | DERIV_RTY_COARSE => float1!(|a| format!("dpdyCoarse({a})")),
            DERIV_RTY_FINE => float1!(|a| format!("dpdyFine({a})")),
            SINCOS => {
                let a = self.f(op(2)?)?;
                let t = self.tmp();
                self.line(&format!("let {t} = {a};"));
                let (s, c) = (op(0)?.clone(), op(1)?.clone());
                self.store_f(ins, &s, format!("sin({t})"))?;
                self.store_f(ins, &c, format!("cos({t})"))?;
            }
            EQ => cmp!(f, "=="),
            NE => cmp!(f, "!="),
            LT => cmp!(f, "<"),
            GE => cmp!(f, ">="),
            IEQ => cmp!(i, "=="),
            INE => cmp!(i, "!="),
            ILT => cmp!(i, "<"),
            IGE => cmp!(i, ">="),
            ULT => cmp!(u, "<"),
            UGE => cmp!(u, ">="),
            AND => int2!(u, "u32", |a: String, b: String| format!("({a} & {b})")),
            OR => int2!(u, "u32", |a: String, b: String| format!("({a} | {b})")),
            XOR => int2!(u, "u32", |a: String, b: String| format!("({a} ^ {b})")),
            NOT => {
                let a = self.u(op(1)?)?;
                self.store(op(0)?, format!("(~{a})"))?
            }
            IADD => int2!(i, "i32", |a: String, b: String| format!("({a} + {b})")),
            IMAX => int2!(i, "i32", |a: String, b: String| format!("max({a}, {b})")),
            IMIN => int2!(i, "i32", |a: String, b: String| format!("min({a}, {b})")),
            UMAX => int2!(u, "u32", |a: String, b: String| format!("max({a}, {b})")),
            UMIN => int2!(u, "u32", |a: String, b: String| format!("min({a}, {b})")),
            ISHL => int2!(i, "i32", |a: String, b: String| format!("({a} << (bitcast<vec4<u32>>({b}) & vec4<u32>(31u)))")),
            ISHR => int2!(i, "i32", |a: String, b: String| format!("({a} >> (bitcast<vec4<u32>>({b}) & vec4<u32>(31u)))")),
            USHR => int2!(u, "u32", |a: String, b: String| format!("({a} >> ({b} & vec4<u32>(31u)))")),
            INEG => {
                let a = self.i(op(1)?)?;
                self.store(op(0)?, format!("bitcast<vec4<u32>>(-{a})"))?
            }
            IMAD => {
                let (a, b, c) = (self.i(op(1)?)?, self.i(op(2)?)?, self.i(op(3)?)?);
                self.store(op(0)?, format!("bitcast<vec4<u32>>({a} * {b} + {c})"))?
            }
            UMAD => {
                let (a, b, c) = (self.u(op(1)?)?, self.u(op(2)?)?, self.u(op(3)?)?);
                self.store(op(0)?, format!("({a} * {b} + {c})"))?
            }
            IMUL | UMUL => {
                // Destinations: the high bits (only a null one is supported) and the low bits.
                let (a, b) = if ins.opcode == IMUL { (self.i(op(2)?)?, self.i(op(3)?)?) } else { (self.u(op(2)?)?, self.u(op(3)?)?) };
                let v = if ins.opcode == IMUL { format!("bitcast<vec4<u32>>({a} * {b})") } else { format!("({a} * {b})") };
                let lo = op(1)?.clone();
                self.store(&lo, v)?
            }
            UDIV => {
                let (a, b) = (self.u(op(2)?)?, self.u(op(3)?)?);
                let t = self.tmp();
                self.line(&format!("let {t} = max({b}, vec4<u32>(1u));"));
                let (q, r) = (op(0)?.clone(), op(1)?.clone());
                self.store(&q, format!("({a} / {t})"))?;
                self.store(&r, format!("({a} % {t})"))?;
            }
            ITOF => {
                let a = self.i(op(1)?)?;
                self.store_f(ins, op(0)?, format!("vec4<f32>({a})"))?
            }
            UTOF => {
                let a = self.u(op(1)?)?;
                self.store_f(ins, op(0)?, format!("vec4<f32>({a})"))?
            }
            FTOI => {
                let a = self.f(op(1)?)?;
                self.store(op(0)?, format!("bitcast<vec4<u32>>(vec4<i32>({a}))"))?
            }
            FTOU => {
                let a = self.f(op(1)?)?;
                self.store(op(0)?, format!("vec4<u32>(max({a}, vec4<f32>(0.0)))"))?
            }
            F16TOF32 => {
                let a = self.u(op(1)?)?;
                let parts: Vec<String> = (0..4).map(|k| format!("unpack2x16float({a}.{}).x", XYZW[k])).collect();
                self.store_f(ins, op(0)?, format!("vec4<f32>({})", parts.join(", ")))?
            }
            F32TOF16 => {
                let a = self.f(op(1)?)?;
                let parts: Vec<String> = (0..4).map(|k| format!("(pack2x16float(vec2<f32>({a}.{}, 0.0)) & 0xffffu)", XYZW[k])).collect();
                self.store(op(0)?, format!("vec4<u32>({})", parts.join(", ")))?
            }
            COUNTBITS => {
                let a = self.u(op(1)?)?;
                self.store(op(0)?, format!("countOneBits({a})"))?
            }
            BFREV => {
                let a = self.u(op(1)?)?;
                self.store(op(0)?, format!("reverseBits({a})"))?
            }
            UBFE | IBFE => {
                let (w, o, v) = (self.u(op(1)?)?, self.u(op(2)?)?, if ins.opcode == UBFE { self.u(op(3)?)? } else { self.i(op(3)?)? });
                let parts: Vec<String> = (0..4).map(|k| format!("extractBits({v}.{c}, {o}.{c} & 31u, {w}.{c} & 31u)", c = XYZW[k])).collect();
                let e = if ins.opcode == UBFE { format!("vec4<u32>({})", parts.join(", ")) } else { format!("bitcast<vec4<u32>>(vec4<i32>({}))", parts.join(", ")) };
                self.store(op(0)?, e)?
            }
            BFI => {
                let (w, o, ins_v, base) = (self.u(op(1)?)?, self.u(op(2)?)?, self.u(op(3)?)?, self.u(op(4)?)?);
                let parts: Vec<String> = (0..4).map(|k| format!("insertBits({base}.{c}, {ins_v}.{c}, {o}.{c} & 31u, {w}.{c} & 31u)", c = XYZW[k])).collect();
                self.store(op(0)?, format!("vec4<u32>({})", parts.join(", ")))?
            }
            SAMPLE | SAMPLE_L | SAMPLE_B | SAMPLE_D | LD => self.texture_call(ins)?,
            // Shadow map comparisons: the previews have no shadows, everything is lit.
            SAMPLE_C | SAMPLE_C_LZ => self.store_f(ins, op(0)?, "vec4<f32>(1.0)".to_string())?,
            SWAPC => {
                let (c, a, b) = (self.u(op(2)?)?, self.u(op(3)?)?, self.u(op(4)?)?);
                let t = self.tmp();
                self.line(&format!("let {t} = {c} != vec4<u32>(0u);"));
                let (d0, d1) = (op(0)?.clone(), op(1)?.clone());
                self.store(&d0, format!("select({a}, {b}, {t})"))?;
                self.store(&d1, format!("select({b}, {a}, {t})"))?;
            }
            RESINFO => {
                let t = Self::imm_index(&op(2)?.indices[0])?;
                let tex = format!("{}tex{t}", if self.p == 'v' { "v" } else { "" });
                let v = format!("vec4<u32>(textureDimensions({tex}).xy, 0u, textureNumLevels({tex}))");
                match ins.control & 3 {
                    2 => self.store(op(0)?, v)?,
                    1 => self.store_f(ins, op(0)?, format!("(vec4<f32>(1.0) / vec4<f32>({v}))"))?,
                    _ => self.store_f(ins, op(0)?, format!("vec4<f32>({v})"))?,
                }
            }
            IF => {
                let c = self.cond(ins, op(0)?)?;
                self.line(&format!("if {c} {{"));
                self.indent += 1;
            }
            ELSE => {
                self.indent = self.indent.saturating_sub(1);
                self.line("} else {");
                self.indent += 1;
            }
            ENDIF | ENDLOOP => {
                self.indent = self.indent.saturating_sub(1);
                self.line("}");
            }
            LOOP => {
                self.line("loop {");
                self.indent += 1;
            }
            BREAK => self.line("break;"),
            CONTINUE => self.line("continue;"),
            BREAKC | CONTINUEC | RETC | DISCARD => {
                let c = self.cond(ins, op(0)?)?;
                let what = match ins.opcode {
                    BREAKC => "break;",
                    CONTINUEC => "continue;",
                    RETC => "return;",
                    _ => "discard;",
                };
                self.line(&format!("if {c} {{ {what} }}"));
            }
            RET => self.line("return;"),
            SWITCH => {
                let v = self.i(op(0)?)?;
                self.line(&format!("switch ({v}.x) {{"));
                self.indent += 1;
                self.switches.push(false);
            }
            CASE | DEFAULT => {
                if self.switches.last() == Some(&true) {
                    self.indent = self.indent.saturating_sub(1);
                    self.line("}");
                }
                let label = if ins.opcode == CASE { format!("case {}", op(0)?.imm.first().map(|v| *v as i32).unwrap_or(0)) } else { "default".to_string() };
                self.line(&format!("{label}: {{"));
                self.indent += 1;
                if let Some(s) = self.switches.last_mut() {
                    *s = true;
                }
            }
            ENDSWITCH => {
                if self.switches.pop() == Some(true) {
                    self.indent = self.indent.saturating_sub(1);
                    self.line("}");
                }
                self.indent = self.indent.saturating_sub(1);
                self.line("}");
            }
            o => return Err(format!("instruction {o} is not handled")),
        }
        Ok(())
    }

    /// The stage's registers (module-scope, so `ret` anywhere returns from its body function) and its body.
    fn stage(&mut self) -> Result<String, String> {
        let p = self.p;
        let mut decl = String::new();
        let temps = self.sh.instructions.iter().find(|i| i.opcode == opcode::DCL_TEMPS).and_then(|i| i.raw.first().copied()).unwrap_or(0);
        for k in 0..temps {
            let _ = writeln!(decl, "var<private> {p}r{k}: vec4<u32>;");
        }
        for i in self.sh.instructions.iter().filter(|i| i.opcode == opcode::DCL_INDEXABLE_TEMP) {
            if let (Some(reg), Some(count)) = (i.raw.first(), i.raw.get(1)) {
                let _ = writeln!(decl, "var<private> {p}x{reg}: array<vec4<u32>, {}>;", count.max(&1));
            }
        }
        // The immediate constant buffer (custom data of class 3).
        if let Some(icb) = self.sh.instructions.iter().find(|i| i.opcode == opcode::CUSTOMDATA && i.control == 3) {
            let rows: Vec<String> = icb.raw.chunks(4).map(|c| format!("vec4<u32>({}u, {}u, {}u, {}u)", c.first().unwrap_or(&0), c.get(1).unwrap_or(&0), c.get(2).unwrap_or(&0), c.get(3).unwrap_or(&0))).collect();
            let _ = writeln!(decl, "const {p}icb = array<vec4<u32>, {}>({});", rows.len().max(1), if rows.is_empty() { "vec4<u32>(0u)".to_string() } else { rows.join(", ") });
        }
        let regs = |sig: &[SigElement]| sig.iter().filter(|e| e.register != u32::MAX).map(|e| e.register).collect::<std::collections::BTreeSet<_>>();
        for r in regs(&self.sh.inputs) {
            let _ = writeln!(decl, "var<private> {p}v{r}: vec4<u32>;");
        }
        for r in regs(&self.sh.outputs) {
            let _ = writeln!(decl, "var<private> {p}o{r}: vec4<u32>;");
        }
        let _ = writeln!(decl, "fn {p}s_body() {{");
        let ins = self.sh.instructions.clone();
        for i in &ins {
            self.instruction(i)?;
        }
        if self.indent != 1 {
            return Err("unbalanced flow control".into());
        }
        decl.push_str(&std::mem::take(&mut self.out));
        decl.push_str("}\n");
        Ok(decl)
    }
}

/// A vertex and pixel shader pair as one WGSL module, laid out like a
/// `d3d9` translation (see [`crate::d3d9::Module`]): group 0 `vsc`, `psc`
/// (each stage's constant buffers flattened, [`Layout`]) and `vsd` (the
/// per-draw registers, `draw`: flat vertex register → slot); group 1 the
/// pixel shader's textures (texture `2k`, a sampler `2k + 1`, in texture slot
/// order: SM5's separate samplers are the renderer's own), group 2 the
/// vertex shader's.
pub fn translate(vs: &Shader, ps: &Shader, draw: &std::collections::BTreeMap<u32, u32>, draw_floats: usize) -> Result<crate::d3d9::Module, String> {
    if vs.kind != Kind::Vertex || ps.kind != Kind::Pixel {
        return Err("translate takes a vertex shader then a pixel shader".into());
    }
    let (vl, pl) = (Layout::of(vs), Layout::of(ps));
    let mut w = String::new();
    let _ = writeln!(w, "// Translated from the game's Direct3D 11 shader model 5 bytecode.");
    let _ = writeln!(w, "struct VsConstants {{ c: array<vec4<f32>, {}> }}", vl.registers);
    let _ = writeln!(w, "struct PsConstants {{ c: array<vec4<f32>, {}> }}", pl.registers);
    let _ = writeln!(w, "struct DrawConstants {{ c: array<vec4<f32>, {}> }}", draw_floats.max(1));
    let _ = writeln!(w, "@group(0) @binding(0) var<uniform> vsc: VsConstants;");
    let _ = writeln!(w, "@group(0) @binding(1) var<uniform> psc: PsConstants;");
    let _ = writeln!(w, "@group(0) @binding(2) var<storage, read> draws: array<DrawConstants>;");
    let _ = writeln!(w, "var<private> vsd: DrawConstants;");
    let kind = |dim: u32| match dim {
        6 => crate::d3d9::SamplerKind::Cube,
        5 => crate::d3d9::SamplerKind::Volume,
        _ => crate::d3d9::SamplerKind::D2,
    };
    let (pt, vt) = (textures(ps), textures(vs));
    for (group, prefix, list) in [(1, "", &pt), (2, "v", &vt)] {
        for (k, (t, dim)) in list.iter().enumerate() {
            let ty = match kind(*dim) {
                crate::d3d9::SamplerKind::Cube => "texture_cube<f32>",
                crate::d3d9::SamplerKind::Volume => "texture_3d<f32>",
                crate::d3d9::SamplerKind::D2 => "texture_2d<f32>",
            };
            let _ = writeln!(w, "@group({group}) @binding({}) var {prefix}tex{t}: {ty};", 2 * k);
            let _ = writeln!(w, "@group({group}) @binding({}) var {prefix}smp{t}: sampler;", 2 * k + 1);
        }
    }
    let empty = std::collections::BTreeMap::new();
    let mut ve = Emit { sh: vs, p: 'v', consts: "vsc", layout: &vl, draw, out: String::new(), indent: 1, n: 0, switches: Vec::new() };
    w.push_str(&ve.stage()?);
    let mut pe = Emit { sh: ps, p: 'p', consts: "psc", layout: &pl, draw: &empty, out: String::new(), indent: 1, n: 0, switches: Vec::new() };
    w.push_str(&pe.stage()?);

    // Vertex inputs, in register order.
    let mut vin: Vec<&SigElement> = vs.inputs.iter().filter(|e| e.system_value == 0).collect();
    vin.sort_by_key(|e| e.register);
    let _ = writeln!(w, "struct VsIn {{");
    for (k, e) in vin.iter().enumerate() {
        let _ = writeln!(w, "    @location({k}) a{}: vec4<f32>,", e.register);
    }
    let _ = writeln!(w, "}}");
    // Interpolants: the vertex outputs but the position, matched with the pixel inputs by semantic.
    let varying: Vec<&SigElement> = vs.outputs.iter().filter(|e| e.system_value != 1).collect();
    let flat = |e: &SigElement| {
        ps.inputs.iter().find(|p| p.semantic() == e.semantic()).is_some_and(|p| {
            ps.instructions.iter().any(|i| i.opcode == opcode::DCL_INPUT_PS && i.control & 0xf == 1 && matches!(i.operands.first().and_then(|o| o.indices.first()), Some(Index::Imm(r)) if *r == p.register))
        })
    };
    let _ = writeln!(w, "struct Varyings {{");
    let _ = writeln!(w, "    @builtin(position) position: vec4<f32>,");
    for (k, e) in varying.iter().enumerate() {
        let interp = if flat(e) { " @interpolate(flat)" } else { "" };
        let ty = if flat(e) { "vec4<u32>" } else { "vec4<f32>" };
        let _ = writeln!(w, "    @location({k}){interp} i{k}: {ty}, // {}", e.semantic());
    }
    let _ = writeln!(w, "}}");
    let _ = writeln!(w, "@vertex fn vs_main(input: VsIn, @builtin(instance_index) instance: u32) -> Varyings {{");
    let _ = writeln!(w, "    vsd = draws[instance];");
    for e in &vin {
        let _ = writeln!(w, "    vv{r} = bitcast<vec4<u32>>(input.a{r});", r = e.register);
    }
    let _ = writeln!(w, "    vs_body();");
    let _ = writeln!(w, "    var out: Varyings;");
    let pos = vs.outputs.iter().find(|e| e.system_value == 1).ok_or("the vertex shader writes no position")?;
    let _ = writeln!(w, "    out.position = bitcast<vec4<f32>>(vo{});", pos.register);
    for (k, e) in varying.iter().enumerate() {
        if flat(e) {
            let _ = writeln!(w, "    out.i{k} = vo{};", e.register);
        } else {
            let _ = writeln!(w, "    out.i{k} = bitcast<vec4<f32>>(vo{});", e.register);
        }
    }
    let _ = writeln!(w, "    return out;");
    let _ = writeln!(w, "}}");
    let _ = writeln!(w, "@fragment fn fs_main(input: Varyings, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {{");
    for e in ps.inputs.iter().filter(|e| e.register != u32::MAX) {
        let v = match e.system_value {
            1 => "bitcast<vec4<u32>>(input.position)".to_string(),
            9 => "vec4<u32>(select(0u, 0xffffffffu, front))".to_string(),
            _ => match varying.iter().position(|o| o.semantic() == e.semantic()) {
                Some(k) if flat(varying[k]) => format!("input.i{k}"),
                Some(k) => format!("bitcast<vec4<u32>>(input.i{k})"),
                None => "vec4<u32>(0u)".to_string(),
            },
        };
        let _ = writeln!(w, "    pv{} = {v};", e.register);
    }
    let _ = writeln!(w, "    ps_body();");
    let target = ps.outputs.iter().find(|e| e.name.eq_ignore_ascii_case("SV_TARGET") && e.index == 0).map(|e| e.register).unwrap_or(0);
    let _ = writeln!(w, "    return bitcast<vec4<f32>>(po{target});");
    let _ = writeln!(w, "}}");
    let semantic = |e: &SigElement| {
        let usage = match e.name.to_ascii_uppercase().as_str() {
            "POSITION" => crate::d3d9::Semantic::POSITION,
            "BLENDWEIGHT" => crate::d3d9::Semantic::BLENDWEIGHT,
            "BLENDINDICES" => crate::d3d9::Semantic::BLENDINDICES,
            "NORMAL" => crate::d3d9::Semantic::NORMAL,
            "TANGENT" => crate::d3d9::Semantic::TANGENT,
            "BINORMAL" => crate::d3d9::Semantic::BINORMAL,
            "COLOR" => crate::d3d9::Semantic::COLOR,
            _ => crate::d3d9::Semantic::TEXCOORD,
        };
        crate::d3d9::Semantic { usage, index: e.index as u8 }
    };
    Ok(crate::d3d9::Module {
        wgsl: w,
        vertex_inputs: vin.iter().map(|e| semantic(e)).collect(),
        vertex_floats: vl.registers as usize,
        pixel_floats: pl.registers as usize,
        samplers: pt.iter().map(|(t, d)| (*t as u16, kind(*d))).collect(),
        vertex_samplers: vt.iter().map(|(t, d)| (*t as u16, kind(*d))).collect(),
        draw_constants: Vec::new(),
        draw_floats: draw_floats.max(1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A signature chunk: (name, index, system value, register).
    fn sig(elems: &[(&str, u32, u32, u32)]) -> Vec<u8> {
        let mut head = Vec::new();
        let mut names = Vec::new();
        let names_at = 8 + 24 * elems.len();
        head.extend((elems.len() as u32).to_le_bytes());
        head.extend(8u32.to_le_bytes());
        for (name, index, sv, reg) in elems {
            let at = (names_at + names.len()) as u32;
            names.extend(name.as_bytes());
            names.push(0);
            for v in [at, *index, *sv, 3, *reg] {
                head.extend(v.to_le_bytes());
            }
            head.extend([0xf, 0xf, 0, 0]);
        }
        head.extend(names);
        head
    }

    fn container(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = b"DXBC".to_vec();
        out.extend([0u8; 16]);
        out.extend(1u32.to_le_bytes());
        let total_at = out.len();
        out.extend(0u32.to_le_bytes());
        out.extend((chunks.len() as u32).to_le_bytes());
        let mut at = out.len() + 4 * chunks.len();
        let mut body = Vec::new();
        for (tag, data) in chunks {
            out.extend((at as u32).to_le_bytes());
            body.extend(*tag);
            body.extend((data.len() as u32).to_le_bytes());
            body.extend(data);
            at += 8 + data.len();
        }
        out.extend(body);
        let total = out.len() as u32;
        out[total_at..total_at + 4].copy_from_slice(&total.to_le_bytes());
        out
    }

    fn program(kind: u32, body: &[u32]) -> Vec<u8> {
        let mut d = vec![(kind << 16) | 0x50, (body.len() + 2) as u32];
        d.extend(body);
        d.iter().flat_map(|t| t.to_le_bytes()).collect()
    }

    // Operand tokens: 4 components, mask or swizzle mode, a type, one or two immediate indices.
    fn masked(ty: u32) -> u32 {
        2 | (0xf << 4) | (ty << 12) | (1 << 20)
    }
    fn swizzled(ty: u32, dims: u32) -> u32 {
        2 | (1 << 2) | (0xe4 << 4) | (ty << 12) | (dims << 20)
    }

    #[test]
    fn a_minimal_pair_translates() {
        // vs: dcl_constantbuffer cb0[4]; dcl_input v0; dcl_output_siv o0, position; dp4 o0.x, v0, cb0[0]; mov o0.yzw, v0; ret
        let vs = program(1, &[
            89 | (4 << 24), swizzled(8, 2), 0, 4,
            95 | (3 << 24), masked(1), 0,
            103 | (4 << 24), masked(2), 0, 1,
            17 | (8 << 24), 2 | (0x1 << 4) | (2 << 12) | (1 << 20), 0, swizzled(1, 1), 0, swizzled(8, 2), 0, 0,
            54 | (5 << 24), 2 | (0xe << 4) | (2 << 12) | (1 << 20), 0, swizzled(1, 1), 0,
            62 | (1 << 24),
        ]);
        // ps: dcl_output o0; mov_sat o0, l(2, 0, 0, 1); ret
        let ps = program(0, &[
            101 | (3 << 24), masked(2), 0,
            54 | (1 << 13) | (8 << 24), masked(2), 0, 2 | (4 << 12), 2.0f32.to_bits(), 0, 0, 1.0f32.to_bits(),
            62 | (1 << 24),
        ]);
        let vs = Shader::parse(&container(&[(b"ISGN", sig(&[("POSITION", 0, 0, 0)])), (b"OSGN", sig(&[("SV_Position", 0, 1, 0)])), (b"SHEX", vs)])).unwrap();
        let ps = Shader::parse(&container(&[(b"ISGN", sig(&[("SV_Position", 0, 1, 0)])), (b"OSGN", sig(&[("SV_Target", 0, 0, 0)])), (b"SHEX", ps)])).unwrap();
        assert_eq!(vs.kind, Kind::Vertex);
        assert_eq!(vs.constant_buffers(), [(0, 4)]);
        assert_eq!(vs.inputs[0].semantic(), "POSITION0");
        let m = translate(&vs, &ps, &std::collections::BTreeMap::new(), 1).unwrap();
        // The constant buffer is the flat `vsc`; the dot product writes x only; the saturated move clamps.
        assert!(m.wgsl.contains("vec4<f32>(dot(bitcast<vec4<f32>>(vv0), bitcast<vec4<f32>>(bitcast<vec4<u32>>(vsc.c[0]))))"), "{}", m.wgsl);
        assert!(m.wgsl.contains("vo0.x = t1.x;") && !m.wgsl.contains("vo0.y = t1.y;"), "{}", m.wgsl);
        assert!(m.wgsl.contains("clamp(bitcast<vec4<f32>>(vec4<u32>(1073741824u, 0u, 0u, 1065353216u))"), "{}", m.wgsl);
        assert_eq!((m.vertex_floats, m.pixel_floats), (4, 1));
        assert_eq!(m.vertex_inputs.len(), 1);
    }
}
