//! UnrealScript bytecode: token tree, disassembler (disk form → tree) and
//! assembler (tree → disk form) that tracks the in-memory layout the VM
//! uses for jump offsets (object references are 8 bytes in memory, 4 on
//! disk; everything else is identical).

use tempest_upk::io::{Error, FName, PackageIndex, Reader, Result, Writer};

/// `EExprToken` values for Paladins (two Hi-Rez additions marked).
pub mod op {
    pub const LOCAL_VARIABLE: u8 = 0x00;
    pub const INSTANCE_VARIABLE: u8 = 0x01;
    pub const DEFAULT_VARIABLE: u8 = 0x02;
    pub const STATE_VARIABLE: u8 = 0x03;
    pub const RETURN: u8 = 0x04;
    pub const SWITCH: u8 = 0x05;
    pub const JUMP: u8 = 0x06;
    pub const JUMP_IF_NOT: u8 = 0x07;
    pub const STOP: u8 = 0x08;
    pub const ASSERT: u8 = 0x09;
    pub const CASE: u8 = 0x0A;
    pub const NOTHING: u8 = 0x0B;
    pub const LABEL_TABLE: u8 = 0x0C;
    pub const GOTO_LABEL: u8 = 0x0D;
    pub const EAT_RETURN_VALUE: u8 = 0x0E;
    pub const LET: u8 = 0x0F;
    pub const DYN_ARRAY_ELEMENT: u8 = 0x10;
    pub const NEW: u8 = 0x11;
    pub const CLASS_CONTEXT: u8 = 0x12;
    pub const META_CAST: u8 = 0x13;
    pub const LET_BOOL: u8 = 0x14;
    pub const END_PARM_VALUE: u8 = 0x15;
    pub const END_FUNCTION_PARMS: u8 = 0x16;
    pub const SELF: u8 = 0x17;
    pub const SKIP: u8 = 0x18;
    pub const CONTEXT: u8 = 0x19;
    pub const ARRAY_ELEMENT: u8 = 0x1A;
    pub const VIRTUAL_FUNCTION: u8 = 0x1B;
    pub const FINAL_FUNCTION: u8 = 0x1C;
    pub const INT_CONST: u8 = 0x1D;
    pub const FLOAT_CONST: u8 = 0x1E;
    pub const STRING_CONST: u8 = 0x1F;
    pub const OBJECT_CONST: u8 = 0x20;
    pub const NAME_CONST: u8 = 0x21;
    pub const ROTATION_CONST: u8 = 0x22;
    pub const VECTOR_CONST: u8 = 0x23;
    pub const BYTE_CONST: u8 = 0x24;
    pub const INT_ZERO: u8 = 0x25;
    pub const INT_ONE: u8 = 0x26;
    pub const TRUE: u8 = 0x27;
    pub const FALSE: u8 = 0x28;
    pub const NATIVE_PARM: u8 = 0x29;
    pub const NO_OBJECT: u8 = 0x2A;
    /// Hi-Rez: `self` as a safe object.
    pub const SELF_SAFE_OBJECT: u8 = 0x2B;
    pub const INT_CONST_BYTE: u8 = 0x2C;
    pub const BOOL_VARIABLE: u8 = 0x2D;
    pub const DYNAMIC_CAST: u8 = 0x2E;
    pub const ITERATOR: u8 = 0x2F;
    pub const ITERATOR_POP: u8 = 0x30;
    pub const ITERATOR_NEXT: u8 = 0x31;
    pub const STRUCT_CMP_EQ: u8 = 0x32;
    pub const STRUCT_CMP_NE: u8 = 0x33;
    pub const UNICODE_STRING_CONST: u8 = 0x34;
    pub const STRUCT_MEMBER: u8 = 0x35;
    pub const DYN_ARRAY_LENGTH: u8 = 0x36;
    pub const GLOBAL_FUNCTION: u8 = 0x37;
    pub const PRIMITIVE_CAST: u8 = 0x38;
    pub const DYN_ARRAY_INSERT: u8 = 0x39;
    pub const RETURN_NOTHING: u8 = 0x3A;
    pub const EQUAL_EQUAL_DEL_DEL: u8 = 0x3B;
    pub const NOT_EQUAL_DEL_DEL: u8 = 0x3C;
    pub const EQUAL_EQUAL_DEL_FUNC: u8 = 0x3D;
    pub const NOT_EQUAL_DEL_FUNC: u8 = 0x3E;
    pub const EMPTY_DELEGATE: u8 = 0x3F;
    pub const DYN_ARRAY_REMOVE: u8 = 0x40;
    pub const DEBUG_INFO: u8 = 0x41;
    pub const DELEGATE_FUNCTION: u8 = 0x42;
    pub const DELEGATE_PROPERTY: u8 = 0x43;
    pub const LET_DELEGATE: u8 = 0x44;
    pub const CONDITIONAL: u8 = 0x45;
    pub const DYN_ARRAY_FIND: u8 = 0x46;
    pub const DYN_ARRAY_FIND_STRUCT: u8 = 0x47;
    pub const LOCAL_OUT_VARIABLE: u8 = 0x48;
    pub const DEFAULT_PARM_VALUE: u8 = 0x49;
    pub const EMPTY_PARM_VALUE: u8 = 0x4A;
    pub const INSTANCE_DELEGATE: u8 = 0x4B;
    /// Hi-Rez: member access through a safe object (same layout as `CONTEXT`).
    pub const SAFE_OBJECT_CONTEXT: u8 = 0x50;
    pub const INTERFACE_CONTEXT: u8 = 0x51;
    pub const INTERFACE_CAST: u8 = 0x52;
    pub const END_OF_SCRIPT: u8 = 0x53;
    pub const DYN_ARRAY_ADD: u8 = 0x54;
    pub const DYN_ARRAY_ADD_ITEM: u8 = 0x55;
    pub const DYN_ARRAY_REMOVE_ITEM: u8 = 0x56;
    pub const DYN_ARRAY_INSERT_ITEM: u8 = 0x57;
    pub const DYN_ARRAY_ITERATOR: u8 = 0x58;
    pub const DYN_ARRAY_SORT: u8 = 0x59;
    pub const JUMP_IF_FILTER_EDITOR_ONLY: u8 = 0x5A;
    pub const EXTENDED_NATIVE: u8 = 0x60;
    pub const FIRST_NATIVE: u8 = 0x70;
}

/// `ECastToken` (Hi-Rez additions marked).
pub mod cast {
    pub const INTERFACE_TO_OBJECT: u8 = 0x36;
    pub const INTERFACE_TO_STRING: u8 = 0x37;
    pub const INTERFACE_TO_BOOL: u8 = 0x38;
    pub const ROTATOR_TO_VECTOR: u8 = 0x39;
    pub const BYTE_TO_INT: u8 = 0x3A;
    pub const BYTE_TO_BOOL: u8 = 0x3B;
    pub const BYTE_TO_FLOAT: u8 = 0x3C;
    pub const INT_TO_BYTE: u8 = 0x3D;
    pub const INT_TO_BOOL: u8 = 0x3E;
    pub const INT_TO_FLOAT: u8 = 0x3F;
    pub const BOOL_TO_BYTE: u8 = 0x40;
    pub const BOOL_TO_INT: u8 = 0x41;
    pub const BOOL_TO_FLOAT: u8 = 0x42;
    pub const FLOAT_TO_BYTE: u8 = 0x43;
    pub const FLOAT_TO_INT: u8 = 0x44;
    pub const FLOAT_TO_BOOL: u8 = 0x45;
    pub const OBJECT_TO_INTERFACE: u8 = 0x46;
    pub const OBJECT_TO_BOOL: u8 = 0x47;
    pub const NAME_TO_BOOL: u8 = 0x48;
    pub const STRING_TO_BYTE: u8 = 0x49;
    pub const STRING_TO_INT: u8 = 0x4A;
    pub const STRING_TO_BOOL: u8 = 0x4B;
    pub const STRING_TO_FLOAT: u8 = 0x4C;
    pub const STRING_TO_VECTOR: u8 = 0x4D;
    pub const STRING_TO_ROTATOR: u8 = 0x4E;
    pub const VECTOR_TO_BOOL: u8 = 0x4F;
    pub const VECTOR_TO_ROTATOR: u8 = 0x50;
    pub const ROTATOR_TO_BOOL: u8 = 0x51;
    pub const BYTE_TO_STRING: u8 = 0x52;
    pub const INT_TO_STRING: u8 = 0x53;
    pub const BOOL_TO_STRING: u8 = 0x54;
    pub const FLOAT_TO_STRING: u8 = 0x55;
    pub const OBJECT_TO_STRING: u8 = 0x56;
    pub const NAME_TO_STRING: u8 = 0x57;
    pub const VECTOR_TO_STRING: u8 = 0x58;
    pub const ROTATOR_TO_STRING: u8 = 0x59;
    pub const DELEGATE_TO_STRING: u8 = 0x5A;
    pub const STRING_TO_NAME: u8 = 0x60;
    pub const SAFE_OBJECT_TO_INTERFACE: u8 = 0x61;
    pub const SAFE_OBJECT_TO_BOOL: u8 = 0x62;
    pub const SAFE_OBJECT_TO_STRING: u8 = 0x63;
    pub const SAFE_OBJECT_TO_OBJECT: u8 = 0x64;
    pub const OBJECT_TO_SAFE_OBJECT: u8 = 0x65;
    pub const INTERFACE_TO_SAFE_OBJECT: u8 = 0x66;
}

/// Debug-info opcode used by `HANDLE_OPTIONAL_DEBUG_INFO`.
pub const DEBUG_INFO_VERSION: i32 = 100;

/// A call's argument list (without the terminating `EX_EndFunctionParms`)
/// and the optional trailing `EX_DebugInfo` the VM allows after calls.
#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub args: Vec<Token>,
    pub debug: Option<Box<Token>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelEntry {
    pub name: FName,
    pub offset: u32,
}

/// One expression / statement token with its operands.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    LocalVariable(PackageIndex),
    InstanceVariable(PackageIndex),
    DefaultVariable(PackageIndex),
    StateVariable(PackageIndex),
    LocalOutVariable(PackageIndex),
    Return(Box<Token>),
    ReturnNothing(PackageIndex),
    EatReturnValue(PackageIndex),
    Switch { prop: PackageIndex, size: u8, expr: Box<Token> },
    Jump { target: u16 },
    JumpIfNot { target: u16, cond: Box<Token> },
    Stop,
    Assert { line: u16, debug: u8, expr: Box<Token> },
    /// `target == 0xFFFF` is the `default:` case and carries no expression.
    Case { target: u16, expr: Option<Box<Token>> },
    Nothing,
    LabelTable(Vec<LabelEntry>),
    GotoLabel(Box<Token>),
    Let(Box<Token>, Box<Token>),
    LetBool(Box<Token>, Box<Token>),
    LetDelegate(Box<Token>, Box<Token>),
    DynArrayElement { index: Box<Token>, base: Box<Token> },
    ArrayElement { index: Box<Token>, base: Box<Token> },
    /// `new(outer, name, flags) class(template)`.
    New { outer: Box<Token>, name: Box<Token>, flags: Box<Token>, class: Box<Token>, template: Box<Token> },
    /// `EX_ClassContext`, `EX_Context` and `EX_SafeObjectContext`.
    Context { kind: ContextKind, object: Box<Token>, skip: u16, prop: PackageIndex, size: u8, member: Box<Token> },
    MetaCast { class: PackageIndex, expr: Box<Token> },
    DynamicCast { class: PackageIndex, expr: Box<Token> },
    InterfaceCast { class: PackageIndex, expr: Box<Token> },
    EndParmValue,
    EndFunctionParms,
    SelfRef,
    SelfSafeObject,
    Skip { size: u16, expr: Box<Token> },
    VirtualFunction { name: FName, args: Args },
    GlobalFunction { name: FName, args: Args },
    FinalFunction { func: PackageIndex, args: Args },
    NativeFunction { index: u16, args: Args },
    IntConst(i32),
    FloatConst(f32),
    /// Raw bytes of an ANSI string constant (without the NUL).
    StringConst(Vec<u8>),
    UnicodeStringConst(Vec<u16>),
    ObjectConst(PackageIndex),
    NameConst(FName),
    RotationConst(i32, i32, i32),
    VectorConst(f32, f32, f32),
    ByteConst(u8),
    IntConstByte(u8),
    IntZero,
    IntOne,
    True,
    False,
    NativeParm(PackageIndex),
    NoObject,
    BoolVariable(Box<Token>),
    Iterator { expr: Box<Token>, end: u16 },
    IteratorPop,
    IteratorNext,
    StructCmpEq { strct: PackageIndex, a: Box<Token>, b: Box<Token> },
    StructCmpNe { strct: PackageIndex, a: Box<Token>, b: Box<Token> },
    StructMember { prop: PackageIndex, strct: PackageIndex, copy: u8, modify: u8, expr: Box<Token> },
    DynArrayLength(Box<Token>),
    PrimitiveCast { cast: u8, expr: Box<Token> },
    DynArrayInsert { base: Box<Token>, index: Box<Token>, count: Box<Token>, debug: Option<Box<Token>> },
    DynArrayRemove { base: Box<Token>, index: Box<Token>, count: Box<Token>, debug: Option<Box<Token>> },
    DynArrayAdd { base: Box<Token>, count: Box<Token>, debug: Option<Box<Token>> },
    DynArrayAddItem { base: Box<Token>, skip: u16, item: Box<Token>, debug: Option<Box<Token>> },
    DynArrayRemoveItem { base: Box<Token>, skip: u16, item: Box<Token>, debug: Option<Box<Token>> },
    DynArrayInsertItem { base: Box<Token>, skip: u16, index: Box<Token>, item: Box<Token>, debug: Option<Box<Token>> },
    DynArrayFind { base: Box<Token>, skip: u16, item: Box<Token>, debug: Option<Box<Token>> },
    DynArrayFindStruct { base: Box<Token>, skip: u16, member: Box<Token>, item: Box<Token>, debug: Option<Box<Token>> },
    DynArraySort { base: Box<Token>, skip: u16, delegate: Box<Token>, debug: Option<Box<Token>> },
    DynArrayIterator { array: Box<Token>, item: Box<Token>, has_index: u8, index: Box<Token>, end: u16 },
    DelegateCmp { op: u8, args: Args },
    EmptyDelegate,
    DebugInfo { version: i32, line: i32, pos: i32, opcode: u8 },
    DelegateFunction { local: u8, prop: PackageIndex, name: FName, args: Args },
    DelegateProperty { name: FName, prop: PackageIndex },
    InstanceDelegate(FName),
    Conditional { cond: Box<Token>, skip_a: u16, a: Box<Token>, skip_b: u16, b: Box<Token> },
    DefaultParmValue { skip: u16, debug_before: Option<Box<Token>>, expr: Box<Token>, debug_after: Option<Box<Token>>, end: u8 },
    EmptyParmValue,
    InterfaceContext(Box<Token>),
    EndOfScript,
    JumpIfFilterEditorOnly { target: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextKind {
    Class,
    Object,
    SafeObject,
}

/// A top-level token with the offsets at which it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub mem_offset: u32,
    pub disk_offset: u32,
    pub token: Token,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    pub statements: Vec<Statement>,
    /// In-memory size, must equal `UStruct::ScriptBytecodeSize`.
    pub mem_size: u32,
}

// --------------------------------------------------------------------------
// Disassembler
// --------------------------------------------------------------------------

pub struct Disassembler<'a> {
    r: Reader<'a>,
    /// Current memory offset (disk offset + 4 per object reference read so far).
    mem: u32,
    /// Name-table index of `None`, used to terminate label tables.
    none_index: Option<u32>,
    /// Bytes per object reference in the input: 4 (disk form) or 8 (memory form).
    ptr_size: u8,
}

impl<'a> Disassembler<'a> {
    pub fn new(script: &'a [u8]) -> Self {
        Disassembler { r: Reader::new(script), mem: 0, none_index: None, ptr_size: 4 }
    }

    /// Disassemble memory-form bytecode (8-byte references); each reference
    /// is returned as `PackageIndex(handle as i32)` for the caller to map.
    pub fn disassemble_memory(script: &'a [u8], none_index: Option<u32>) -> Result<Script> {
        let mut d = Disassembler { r: Reader::new(script), mem: 0, none_index, ptr_size: 8 };
        let mut out = Script::default();
        while !d.r.is_eof() {
            let mem_offset = d.mem;
            let disk_offset = d.r.pos() as u32;
            let token = d.expr()?;
            out.statements.push(Statement { mem_offset, disk_offset, token });
        }
        out.mem_size = d.mem;
        Ok(out)
    }

    pub fn disassemble(script: &'a [u8], none_index: Option<u32>) -> Result<Script> {
        let mut d = Disassembler { r: Reader::new(script), mem: 0, none_index, ptr_size: 4 };
        let mut out = Script::default();
        while !d.r.is_eof() {
            let mem_offset = d.mem;
            let disk_offset = d.r.pos() as u32;
            let token = d.expr()?;
            out.statements.push(Statement { mem_offset, disk_offset, token });
        }
        out.mem_size = d.mem;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        self.mem += 1;
        self.r.u8()
    }
    fn u16(&mut self) -> Result<u16> {
        self.mem += 2;
        self.r.u16()
    }
    fn i32(&mut self) -> Result<i32> {
        self.mem += 4;
        self.r.i32()
    }
    fn f32(&mut self) -> Result<f32> {
        self.mem += 4;
        self.r.f32()
    }
    fn name(&mut self) -> Result<FName> {
        self.mem += 8;
        self.r.fname()
    }
    fn ptr(&mut self) -> Result<PackageIndex> {
        self.mem += 8;
        if self.ptr_size == 8 {
            let v = self.r.u64()?;
            return Ok(PackageIndex(v as i32));
        }
        self.r.index()
    }

    fn args(&mut self) -> Result<Args> {
        let mut args = Vec::new();
        loop {
            let t = self.expr()?;
            if t == Token::EndFunctionParms {
                break;
            }
            args.push(t);
        }
        let debug = self.optional_debug_info()?;
        Ok(Args { args, debug })
    }

    /// `HANDLE_OPTIONAL_DEBUG_INFO`: consume a following `EX_DebugInfo` with
    /// version 100, if any.
    fn optional_debug_info(&mut self) -> Result<Option<Box<Token>>> {
        if self.r.remaining() >= 5 && self.r.peek_u8()? == op::DEBUG_INFO {
            let save = self.r.clone();
            let save_mem = self.mem;
            self.u8()?;
            let version = self.i32()?;
            self.r = save;
            self.mem = save_mem;
            if version == DEBUG_INFO_VERSION {
                return Ok(Some(Box::new(self.expr()?)));
            }
        }
        Ok(None)
    }

    pub fn expr(&mut self) -> Result<Token> {
        let b = self.u8()?;
        if b >= op::FIRST_NATIVE {
            let args = self.args()?;
            return Ok(Token::NativeFunction { index: b as u16, args });
        }
        if b >= op::EXTENDED_NATIVE {
            let lo = self.u8()?;
            let index = (((b - op::EXTENDED_NATIVE) as u16) << 8) | lo as u16;
            let args = self.args()?;
            return Ok(Token::NativeFunction { index, args });
        }
        Ok(match b {
            op::LOCAL_VARIABLE => Token::LocalVariable(self.ptr()?),
            op::INSTANCE_VARIABLE => Token::InstanceVariable(self.ptr()?),
            op::DEFAULT_VARIABLE => Token::DefaultVariable(self.ptr()?),
            op::STATE_VARIABLE => Token::StateVariable(self.ptr()?),
            op::LOCAL_OUT_VARIABLE => Token::LocalOutVariable(self.ptr()?),
            op::RETURN => Token::Return(Box::new(self.expr()?)),
            op::RETURN_NOTHING => Token::ReturnNothing(self.ptr()?),
            op::EAT_RETURN_VALUE => Token::EatReturnValue(self.ptr()?),
            op::SWITCH => {
                let prop = self.ptr()?;
                let size = self.u8()?;
                let expr = Box::new(self.expr()?);
                Token::Switch { prop, size, expr }
            }
            op::JUMP => Token::Jump { target: self.u16()? },
            op::JUMP_IF_NOT => {
                let target = self.u16()?;
                let cond = Box::new(self.expr()?);
                Token::JumpIfNot { target, cond }
            }
            op::STOP => Token::Stop,
            op::ASSERT => {
                let line = self.u16()?;
                let debug = self.u8()?;
                let expr = Box::new(self.expr()?);
                Token::Assert { line, debug, expr }
            }
            op::CASE => {
                let target = self.u16()?;
                let expr = if target != 0xFFFF { Some(Box::new(self.expr()?)) } else { None };
                Token::Case { target, expr }
            }
            op::NOTHING => Token::Nothing,
            op::LABEL_TABLE => {
                let mut entries = Vec::new();
                loop {
                    let name = self.name()?;
                    let offset = self.i32()? as u32;
                    let is_none = name.index == self.none_index.unwrap_or(u32::MAX) && name.number == 0;
                    entries.push(LabelEntry { name, offset });
                    if is_none || (self.none_index.is_none() && offset == 0xFFFF) {
                        break;
                    }
                }
                Token::LabelTable(entries)
            }
            op::GOTO_LABEL => Token::GotoLabel(Box::new(self.expr()?)),
            op::LET => Token::Let(Box::new(self.expr()?), Box::new(self.expr()?)),
            op::LET_BOOL => Token::LetBool(Box::new(self.expr()?), Box::new(self.expr()?)),
            op::LET_DELEGATE => Token::LetDelegate(Box::new(self.expr()?), Box::new(self.expr()?)),
            op::DYN_ARRAY_ELEMENT => {
                let index = Box::new(self.expr()?);
                let base = Box::new(self.expr()?);
                Token::DynArrayElement { index, base }
            }
            op::ARRAY_ELEMENT => {
                let index = Box::new(self.expr()?);
                let base = Box::new(self.expr()?);
                Token::ArrayElement { index, base }
            }
            op::NEW => {
                let outer = Box::new(self.expr()?);
                let name = Box::new(self.expr()?);
                let flags = Box::new(self.expr()?);
                let class = Box::new(self.expr()?);
                let template = Box::new(self.expr()?);
                Token::New { outer, name, flags, class, template }
            }
            op::CLASS_CONTEXT | op::CONTEXT | op::SAFE_OBJECT_CONTEXT => {
                let kind = match b {
                    op::CLASS_CONTEXT => ContextKind::Class,
                    op::CONTEXT => ContextKind::Object,
                    _ => ContextKind::SafeObject,
                };
                let object = Box::new(self.expr()?);
                let skip = self.u16()?;
                let prop = self.ptr()?;
                let size = self.u8()?;
                let member = Box::new(self.expr()?);
                Token::Context { kind, object, skip, prop, size, member }
            }
            op::META_CAST => {
                let class = self.ptr()?;
                Token::MetaCast { class, expr: Box::new(self.expr()?) }
            }
            op::DYNAMIC_CAST => {
                let class = self.ptr()?;
                Token::DynamicCast { class, expr: Box::new(self.expr()?) }
            }
            op::INTERFACE_CAST => {
                let class = self.ptr()?;
                Token::InterfaceCast { class, expr: Box::new(self.expr()?) }
            }
            op::END_PARM_VALUE => Token::EndParmValue,
            op::END_FUNCTION_PARMS => Token::EndFunctionParms,
            op::SELF => Token::SelfRef,
            op::SELF_SAFE_OBJECT => Token::SelfSafeObject,
            op::SKIP => {
                let size = self.u16()?;
                Token::Skip { size, expr: Box::new(self.expr()?) }
            }
            op::VIRTUAL_FUNCTION => {
                let name = self.name()?;
                Token::VirtualFunction { name, args: self.args()? }
            }
            op::GLOBAL_FUNCTION => {
                let name = self.name()?;
                Token::GlobalFunction { name, args: self.args()? }
            }
            op::FINAL_FUNCTION => {
                let func = self.ptr()?;
                Token::FinalFunction { func, args: self.args()? }
            }
            op::INT_CONST => Token::IntConst(self.i32()?),
            op::FLOAT_CONST => Token::FloatConst(self.f32()?),
            op::STRING_CONST => {
                let mut bytes = Vec::new();
                loop {
                    let c = self.u8()?;
                    if c == 0 {
                        break;
                    }
                    bytes.push(c);
                }
                Token::StringConst(bytes)
            }
            op::UNICODE_STRING_CONST => {
                let mut units = Vec::new();
                loop {
                    let c = self.u16()?;
                    if c == 0 {
                        break;
                    }
                    units.push(c);
                }
                Token::UnicodeStringConst(units)
            }
            op::OBJECT_CONST => Token::ObjectConst(self.ptr()?),
            op::NAME_CONST => Token::NameConst(self.name()?),
            op::ROTATION_CONST => Token::RotationConst(self.i32()?, self.i32()?, self.i32()?),
            op::VECTOR_CONST => Token::VectorConst(self.f32()?, self.f32()?, self.f32()?),
            op::BYTE_CONST => Token::ByteConst(self.u8()?),
            op::INT_CONST_BYTE => Token::IntConstByte(self.u8()?),
            op::INT_ZERO => Token::IntZero,
            op::INT_ONE => Token::IntOne,
            op::TRUE => Token::True,
            op::FALSE => Token::False,
            op::NATIVE_PARM => Token::NativeParm(self.ptr()?),
            op::NO_OBJECT => Token::NoObject,
            op::BOOL_VARIABLE => Token::BoolVariable(Box::new(self.expr()?)),
            op::ITERATOR => {
                let expr = Box::new(self.expr()?);
                let end = self.u16()?;
                Token::Iterator { expr, end }
            }
            op::ITERATOR_POP => Token::IteratorPop,
            op::ITERATOR_NEXT => Token::IteratorNext,
            op::STRUCT_CMP_EQ | op::STRUCT_CMP_NE => {
                let strct = self.ptr()?;
                let a = Box::new(self.expr()?);
                let bb = Box::new(self.expr()?);
                if b == op::STRUCT_CMP_EQ {
                    Token::StructCmpEq { strct, a, b: bb }
                } else {
                    Token::StructCmpNe { strct, a, b: bb }
                }
            }
            op::STRUCT_MEMBER => {
                let prop = self.ptr()?;
                let strct = self.ptr()?;
                let copy = self.u8()?;
                let modify = self.u8()?;
                let expr = Box::new(self.expr()?);
                Token::StructMember { prop, strct, copy, modify, expr }
            }
            op::DYN_ARRAY_LENGTH => Token::DynArrayLength(Box::new(self.expr()?)),
            op::PRIMITIVE_CAST => {
                let cast = self.u8()?;
                Token::PrimitiveCast { cast, expr: Box::new(self.expr()?) }
            }
            op::DYN_ARRAY_INSERT | op::DYN_ARRAY_REMOVE => {
                let base = Box::new(self.expr()?);
                let index = Box::new(self.expr()?);
                let count = Box::new(self.expr()?);
                self.expect_end_parms()?;
                let debug = self.optional_debug_info()?;
                if b == op::DYN_ARRAY_INSERT {
                    Token::DynArrayInsert { base, index, count, debug }
                } else {
                    Token::DynArrayRemove { base, index, count, debug }
                }
            }
            op::DYN_ARRAY_ADD => {
                let base = Box::new(self.expr()?);
                let count = Box::new(self.expr()?);
                self.expect_end_parms()?;
                let debug = self.optional_debug_info()?;
                Token::DynArrayAdd { base, count, debug }
            }
            op::DYN_ARRAY_ADD_ITEM | op::DYN_ARRAY_REMOVE_ITEM | op::DYN_ARRAY_FIND => {
                let base = Box::new(self.expr()?);
                let skip = self.u16()?;
                let item = Box::new(self.expr()?);
                self.expect_end_parms()?;
                let debug = self.optional_debug_info()?;
                match b {
                    op::DYN_ARRAY_ADD_ITEM => Token::DynArrayAddItem { base, skip, item, debug },
                    op::DYN_ARRAY_REMOVE_ITEM => Token::DynArrayRemoveItem { base, skip, item, debug },
                    _ => Token::DynArrayFind { base, skip, item, debug },
                }
            }
            op::DYN_ARRAY_INSERT_ITEM => {
                let base = Box::new(self.expr()?);
                let skip = self.u16()?;
                let index = Box::new(self.expr()?);
                let item = Box::new(self.expr()?);
                self.expect_end_parms()?;
                let debug = self.optional_debug_info()?;
                Token::DynArrayInsertItem { base, skip, index, item, debug }
            }
            op::DYN_ARRAY_FIND_STRUCT => {
                let base = Box::new(self.expr()?);
                let skip = self.u16()?;
                let member = Box::new(self.expr()?);
                let item = Box::new(self.expr()?);
                self.expect_end_parms()?;
                let debug = self.optional_debug_info()?;
                Token::DynArrayFindStruct { base, skip, member, item, debug }
            }
            op::DYN_ARRAY_SORT => {
                let base = Box::new(self.expr()?);
                let skip = self.u16()?;
                let delegate = Box::new(self.expr()?);
                self.expect_end_parms()?;
                let debug = self.optional_debug_info()?;
                Token::DynArraySort { base, skip, delegate, debug }
            }
            op::DYN_ARRAY_ITERATOR => {
                let array = Box::new(self.expr()?);
                let item = Box::new(self.expr()?);
                let has_index = self.u8()?;
                let index = Box::new(self.expr()?);
                let end = self.u16()?;
                Token::DynArrayIterator { array, item, has_index, index, end }
            }
            op::EQUAL_EQUAL_DEL_DEL | op::NOT_EQUAL_DEL_DEL | op::EQUAL_EQUAL_DEL_FUNC | op::NOT_EQUAL_DEL_FUNC => {
                Token::DelegateCmp { op: b, args: self.args()? }
            }
            op::EMPTY_DELEGATE => Token::EmptyDelegate,
            op::DEBUG_INFO => {
                let version = self.i32()?;
                let line = self.i32()?;
                let pos = self.i32()?;
                let opcode = self.u8()?;
                Token::DebugInfo { version, line, pos, opcode }
            }
            op::DELEGATE_FUNCTION => {
                let local = self.u8()?;
                let prop = self.ptr()?;
                let name = self.name()?;
                Token::DelegateFunction { local, prop, name, args: self.args()? }
            }
            op::DELEGATE_PROPERTY => {
                let name = self.name()?;
                let prop = self.ptr()?;
                Token::DelegateProperty { name, prop }
            }
            op::INSTANCE_DELEGATE => Token::InstanceDelegate(self.name()?),
            op::CONDITIONAL => {
                let cond = Box::new(self.expr()?);
                let skip_a = self.u16()?;
                let a = Box::new(self.expr()?);
                let skip_b = self.u16()?;
                let bb = Box::new(self.expr()?);
                Token::Conditional { cond, skip_a, a, skip_b, b: bb }
            }
            op::DEFAULT_PARM_VALUE => {
                let skip = self.u16()?;
                let debug_before = self.optional_debug_info()?;
                let expr = Box::new(self.expr()?);
                let debug_after = self.optional_debug_info()?;
                let end = self.u8()?;
                Token::DefaultParmValue { skip, debug_before, expr, debug_after, end }
            }
            op::EMPTY_PARM_VALUE => Token::EmptyParmValue,
            op::INTERFACE_CONTEXT => Token::InterfaceContext(Box::new(self.expr()?)),
            op::END_OF_SCRIPT => Token::EndOfScript,
            op::JUMP_IF_FILTER_EDITOR_ONLY => Token::JumpIfFilterEditorOnly { target: self.u16()? },
            _ => {
                return Err(Error::Format(format!(
                    "bad expr token {b:#04x} at disk offset {}",
                    self.r.pos() - 1
                )))
            }
        })
    }

    fn expect_end_parms(&mut self) -> Result<()> {
        let t = self.expr()?;
        if t != Token::EndFunctionParms {
            return Err(Error::Format(format!("expected EX_EndFunctionParms, got {t:?}")));
        }
        Ok(())
    }
}

// The label table needs to know which FName is `None`; keep it on the
// disassembler so callers can set it from the package name table.
impl<'a> Disassembler<'a> {
    pub fn with_none(script: &'a [u8], none_index: u32) -> Self {
        Disassembler { r: Reader::new(script), mem: 0, none_index: Some(none_index), ptr_size: 4 }
    }
}

// --------------------------------------------------------------------------
// Assembler
// --------------------------------------------------------------------------

/// Emits the disk form while tracking the memory offset (for callers that
/// need to resolve jump fixups).
pub struct Assembler {
    pub w: Writer,
    pub mem: u32,
}

impl Default for Assembler {
    fn default() -> Self {
        Self::new()
    }
}

impl Assembler {
    pub fn new() -> Assembler {
        Assembler { w: Writer::new(), mem: 0 }
    }

    pub fn assemble(script: &Script) -> (Vec<u8>, u32) {
        let mut a = Assembler::new();
        for s in &script.statements {
            a.token(&s.token);
        }
        (a.w.into_inner(), a.mem)
    }

    pub fn u8(&mut self, v: u8) {
        self.mem += 1;
        self.w.u8(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.mem += 2;
        self.w.u16(v);
    }
    pub fn i32(&mut self, v: i32) {
        self.mem += 4;
        self.w.i32(v);
    }
    pub fn f32(&mut self, v: f32) {
        self.mem += 4;
        self.w.f32(v);
    }
    pub fn name(&mut self, n: FName) {
        self.mem += 8;
        self.w.fname(n);
    }
    pub fn ptr(&mut self, p: PackageIndex) {
        self.mem += 8;
        self.w.index(p);
    }

    fn args(&mut self, a: &Args) {
        for t in &a.args {
            self.token(t);
        }
        self.u8(op::END_FUNCTION_PARMS);
        if let Some(d) = &a.debug {
            self.token(d);
        }
    }

    fn opt(&mut self, d: &Option<Box<Token>>) {
        if let Some(d) = d {
            self.token(d);
        }
    }

    pub fn token(&mut self, t: &Token) {
        match t {
            Token::LocalVariable(p) => {
                self.u8(op::LOCAL_VARIABLE);
                self.ptr(*p);
            }
            Token::InstanceVariable(p) => {
                self.u8(op::INSTANCE_VARIABLE);
                self.ptr(*p);
            }
            Token::DefaultVariable(p) => {
                self.u8(op::DEFAULT_VARIABLE);
                self.ptr(*p);
            }
            Token::StateVariable(p) => {
                self.u8(op::STATE_VARIABLE);
                self.ptr(*p);
            }
            Token::LocalOutVariable(p) => {
                self.u8(op::LOCAL_OUT_VARIABLE);
                self.ptr(*p);
            }
            Token::Return(e) => {
                self.u8(op::RETURN);
                self.token(e);
            }
            Token::ReturnNothing(p) => {
                self.u8(op::RETURN_NOTHING);
                self.ptr(*p);
            }
            Token::EatReturnValue(p) => {
                self.u8(op::EAT_RETURN_VALUE);
                self.ptr(*p);
            }
            Token::Switch { prop, size, expr } => {
                self.u8(op::SWITCH);
                self.ptr(*prop);
                self.u8(*size);
                self.token(expr);
            }
            Token::Jump { target } => {
                self.u8(op::JUMP);
                self.u16(*target);
            }
            Token::JumpIfNot { target, cond } => {
                self.u8(op::JUMP_IF_NOT);
                self.u16(*target);
                self.token(cond);
            }
            Token::Stop => self.u8(op::STOP),
            Token::Assert { line, debug, expr } => {
                self.u8(op::ASSERT);
                self.u16(*line);
                self.u8(*debug);
                self.token(expr);
            }
            Token::Case { target, expr } => {
                self.u8(op::CASE);
                self.u16(*target);
                if let Some(e) = expr {
                    self.token(e);
                }
            }
            Token::Nothing => self.u8(op::NOTHING),
            Token::LabelTable(entries) => {
                self.u8(op::LABEL_TABLE);
                for e in entries {
                    self.name(e.name);
                    self.i32(e.offset as i32);
                }
            }
            Token::GotoLabel(e) => {
                self.u8(op::GOTO_LABEL);
                self.token(e);
            }
            Token::Let(a, b) => {
                self.u8(op::LET);
                self.token(a);
                self.token(b);
            }
            Token::LetBool(a, b) => {
                self.u8(op::LET_BOOL);
                self.token(a);
                self.token(b);
            }
            Token::LetDelegate(a, b) => {
                self.u8(op::LET_DELEGATE);
                self.token(a);
                self.token(b);
            }
            Token::DynArrayElement { index, base } => {
                self.u8(op::DYN_ARRAY_ELEMENT);
                self.token(index);
                self.token(base);
            }
            Token::ArrayElement { index, base } => {
                self.u8(op::ARRAY_ELEMENT);
                self.token(index);
                self.token(base);
            }
            Token::New { outer, name, flags, class, template } => {
                self.u8(op::NEW);
                self.token(outer);
                self.token(name);
                self.token(flags);
                self.token(class);
                self.token(template);
            }
            Token::Context { kind, object, skip, prop, size, member } => {
                self.u8(match kind {
                    ContextKind::Class => op::CLASS_CONTEXT,
                    ContextKind::Object => op::CONTEXT,
                    ContextKind::SafeObject => op::SAFE_OBJECT_CONTEXT,
                });
                self.token(object);
                self.u16(*skip);
                self.ptr(*prop);
                self.u8(*size);
                self.token(member);
            }
            Token::MetaCast { class, expr } => {
                self.u8(op::META_CAST);
                self.ptr(*class);
                self.token(expr);
            }
            Token::DynamicCast { class, expr } => {
                self.u8(op::DYNAMIC_CAST);
                self.ptr(*class);
                self.token(expr);
            }
            Token::InterfaceCast { class, expr } => {
                self.u8(op::INTERFACE_CAST);
                self.ptr(*class);
                self.token(expr);
            }
            Token::EndParmValue => self.u8(op::END_PARM_VALUE),
            Token::EndFunctionParms => self.u8(op::END_FUNCTION_PARMS),
            Token::SelfRef => self.u8(op::SELF),
            Token::SelfSafeObject => self.u8(op::SELF_SAFE_OBJECT),
            Token::Skip { size, expr } => {
                self.u8(op::SKIP);
                self.u16(*size);
                self.token(expr);
            }
            Token::VirtualFunction { name, args } => {
                self.u8(op::VIRTUAL_FUNCTION);
                self.name(*name);
                self.args(args);
            }
            Token::GlobalFunction { name, args } => {
                self.u8(op::GLOBAL_FUNCTION);
                self.name(*name);
                self.args(args);
            }
            Token::FinalFunction { func, args } => {
                self.u8(op::FINAL_FUNCTION);
                self.ptr(*func);
                self.args(args);
            }
            Token::NativeFunction { index, args } => {
                if *index >= op::FIRST_NATIVE as u16 && *index < 256 {
                    self.u8(*index as u8);
                } else {
                    self.u8(op::EXTENDED_NATIVE + (index >> 8) as u8);
                    self.u8((index & 0xFF) as u8);
                }
                self.args(args);
            }
            Token::IntConst(v) => {
                self.u8(op::INT_CONST);
                self.i32(*v);
            }
            Token::FloatConst(v) => {
                self.u8(op::FLOAT_CONST);
                self.f32(*v);
            }
            Token::StringConst(bytes) => {
                self.u8(op::STRING_CONST);
                for b in bytes {
                    self.u8(*b);
                }
                self.u8(0);
            }
            Token::UnicodeStringConst(units) => {
                self.u8(op::UNICODE_STRING_CONST);
                for u in units {
                    self.u16(*u);
                }
                self.u16(0);
            }
            Token::ObjectConst(p) => {
                self.u8(op::OBJECT_CONST);
                self.ptr(*p);
            }
            Token::NameConst(n) => {
                self.u8(op::NAME_CONST);
                self.name(*n);
            }
            Token::RotationConst(a, b, c) => {
                self.u8(op::ROTATION_CONST);
                self.i32(*a);
                self.i32(*b);
                self.i32(*c);
            }
            Token::VectorConst(a, b, c) => {
                self.u8(op::VECTOR_CONST);
                self.f32(*a);
                self.f32(*b);
                self.f32(*c);
            }
            Token::ByteConst(v) => {
                self.u8(op::BYTE_CONST);
                self.u8(*v);
            }
            Token::IntConstByte(v) => {
                self.u8(op::INT_CONST_BYTE);
                self.u8(*v);
            }
            Token::IntZero => self.u8(op::INT_ZERO),
            Token::IntOne => self.u8(op::INT_ONE),
            Token::True => self.u8(op::TRUE),
            Token::False => self.u8(op::FALSE),
            Token::NativeParm(p) => {
                self.u8(op::NATIVE_PARM);
                self.ptr(*p);
            }
            Token::NoObject => self.u8(op::NO_OBJECT),
            Token::BoolVariable(e) => {
                self.u8(op::BOOL_VARIABLE);
                self.token(e);
            }
            Token::Iterator { expr, end } => {
                self.u8(op::ITERATOR);
                self.token(expr);
                self.u16(*end);
            }
            Token::IteratorPop => self.u8(op::ITERATOR_POP),
            Token::IteratorNext => self.u8(op::ITERATOR_NEXT),
            Token::StructCmpEq { strct, a, b } => {
                self.u8(op::STRUCT_CMP_EQ);
                self.ptr(*strct);
                self.token(a);
                self.token(b);
            }
            Token::StructCmpNe { strct, a, b } => {
                self.u8(op::STRUCT_CMP_NE);
                self.ptr(*strct);
                self.token(a);
                self.token(b);
            }
            Token::StructMember { prop, strct, copy, modify, expr } => {
                self.u8(op::STRUCT_MEMBER);
                self.ptr(*prop);
                self.ptr(*strct);
                self.u8(*copy);
                self.u8(*modify);
                self.token(expr);
            }
            Token::DynArrayLength(e) => {
                self.u8(op::DYN_ARRAY_LENGTH);
                self.token(e);
            }
            Token::PrimitiveCast { cast, expr } => {
                self.u8(op::PRIMITIVE_CAST);
                self.u8(*cast);
                self.token(expr);
            }
            Token::DynArrayInsert { base, index, count, debug } => {
                self.u8(op::DYN_ARRAY_INSERT);
                self.token(base);
                self.token(index);
                self.token(count);
                self.u8(op::END_FUNCTION_PARMS);
                self.opt(debug);
            }
            Token::DynArrayRemove { base, index, count, debug } => {
                self.u8(op::DYN_ARRAY_REMOVE);
                self.token(base);
                self.token(index);
                self.token(count);
                self.u8(op::END_FUNCTION_PARMS);
                self.opt(debug);
            }
            Token::DynArrayAdd { base, count, debug } => {
                self.u8(op::DYN_ARRAY_ADD);
                self.token(base);
                self.token(count);
                self.u8(op::END_FUNCTION_PARMS);
                self.opt(debug);
            }
            Token::DynArrayAddItem { base, skip, item, debug } => {
                self.u8(op::DYN_ARRAY_ADD_ITEM);
                self.token(base);
                self.u16(*skip);
                self.token(item);
                self.u8(op::END_FUNCTION_PARMS);
                self.opt(debug);
            }
            Token::DynArrayRemoveItem { base, skip, item, debug } => {
                self.u8(op::DYN_ARRAY_REMOVE_ITEM);
                self.token(base);
                self.u16(*skip);
                self.token(item);
                self.u8(op::END_FUNCTION_PARMS);
                self.opt(debug);
            }
            Token::DynArrayInsertItem { base, skip, index, item, debug } => {
                self.u8(op::DYN_ARRAY_INSERT_ITEM);
                self.token(base);
                self.u16(*skip);
                self.token(index);
                self.token(item);
                self.u8(op::END_FUNCTION_PARMS);
                self.opt(debug);
            }
            Token::DynArrayFind { base, skip, item, debug } => {
                self.u8(op::DYN_ARRAY_FIND);
                self.token(base);
                self.u16(*skip);
                self.token(item);
                self.u8(op::END_FUNCTION_PARMS);
                self.opt(debug);
            }
            Token::DynArrayFindStruct { base, skip, member, item, debug } => {
                self.u8(op::DYN_ARRAY_FIND_STRUCT);
                self.token(base);
                self.u16(*skip);
                self.token(member);
                self.token(item);
                self.u8(op::END_FUNCTION_PARMS);
                self.opt(debug);
            }
            Token::DynArraySort { base, skip, delegate, debug } => {
                self.u8(op::DYN_ARRAY_SORT);
                self.token(base);
                self.u16(*skip);
                self.token(delegate);
                self.u8(op::END_FUNCTION_PARMS);
                self.opt(debug);
            }
            Token::DynArrayIterator { array, item, has_index, index, end } => {
                self.u8(op::DYN_ARRAY_ITERATOR);
                self.token(array);
                self.token(item);
                self.u8(*has_index);
                self.token(index);
                self.u16(*end);
            }
            Token::DelegateCmp { op: o, args } => {
                self.u8(*o);
                self.args(args);
            }
            Token::EmptyDelegate => self.u8(op::EMPTY_DELEGATE),
            Token::DebugInfo { version, line, pos, opcode } => {
                self.u8(op::DEBUG_INFO);
                self.i32(*version);
                self.i32(*line);
                self.i32(*pos);
                self.u8(*opcode);
            }
            Token::DelegateFunction { local, prop, name, args } => {
                self.u8(op::DELEGATE_FUNCTION);
                self.u8(*local);
                self.ptr(*prop);
                self.name(*name);
                self.args(args);
            }
            Token::DelegateProperty { name, prop } => {
                self.u8(op::DELEGATE_PROPERTY);
                self.name(*name);
                self.ptr(*prop);
            }
            Token::InstanceDelegate(n) => {
                self.u8(op::INSTANCE_DELEGATE);
                self.name(*n);
            }
            Token::Conditional { cond, skip_a, a, skip_b, b } => {
                self.u8(op::CONDITIONAL);
                self.token(cond);
                self.u16(*skip_a);
                self.token(a);
                self.u16(*skip_b);
                self.token(b);
            }
            Token::DefaultParmValue { skip, debug_before, expr, debug_after, end } => {
                self.u8(op::DEFAULT_PARM_VALUE);
                self.u16(*skip);
                self.opt(debug_before);
                self.token(expr);
                self.opt(debug_after);
                self.u8(*end);
            }
            Token::EmptyParmValue => self.u8(op::EMPTY_PARM_VALUE),
            Token::InterfaceContext(e) => {
                self.u8(op::INTERFACE_CONTEXT);
                self.token(e);
            }
            Token::EndOfScript => self.u8(op::END_OF_SCRIPT),
            Token::JumpIfFilterEditorOnly { target } => {
                self.u8(op::JUMP_IF_FILTER_EDITOR_ONLY);
                self.u16(*target);
            }
        }
    }
}

/// Memory size of a token tree (what the VM's skip counts are measured in).
pub fn mem_size(t: &Token) -> u32 {
    let mut a = Assembler::new();
    a.token(t);
    a.mem
}

// --------------------------------------------------------------------------
// Reference mapping (memory-form handles → package indices)
// --------------------------------------------------------------------------

impl Token {
    /// Apply `f` to every object reference and `g` to every name in the tree.
    pub fn map_refs(&mut self, f: &mut dyn FnMut(PackageIndex) -> PackageIndex, g: &mut dyn FnMut(FName) -> FName) {
        fn args(a: &mut Args, f: &mut dyn FnMut(PackageIndex) -> PackageIndex, g: &mut dyn FnMut(FName) -> FName) {
            for t in &mut a.args {
                t.map_refs(f, g);
            }
            if let Some(d) = &mut a.debug {
                d.map_refs(f, g);
            }
        }
        fn opt(d: &mut Option<Box<Token>>, f: &mut dyn FnMut(PackageIndex) -> PackageIndex, g: &mut dyn FnMut(FName) -> FName) {
            if let Some(d) = d {
                d.map_refs(f, g);
            }
        }
        match self {
            Token::LocalVariable(p) | Token::InstanceVariable(p) | Token::DefaultVariable(p) | Token::StateVariable(p) | Token::LocalOutVariable(p) => *p = f(*p),
            Token::Return(e) => e.map_refs(f, g),
            Token::ReturnNothing(p) | Token::EatReturnValue(p) | Token::NativeParm(p) | Token::ObjectConst(p) => *p = f(*p),
            Token::Switch { prop, expr, .. } => {
                *prop = f(*prop);
                expr.map_refs(f, g);
            }
            Token::Jump { .. } | Token::Stop | Token::Nothing | Token::EndParmValue | Token::EndFunctionParms | Token::SelfRef | Token::SelfSafeObject => {}
            Token::JumpIfNot { cond, .. } => cond.map_refs(f, g),
            Token::Assert { expr, .. } => expr.map_refs(f, g),
            Token::Case { expr, .. } => opt(expr, f, g),
            Token::LabelTable(entries) => {
                for e in entries {
                    e.name = g(e.name);
                }
            }
            Token::GotoLabel(e) => e.map_refs(f, g),
            Token::Let(a, b) | Token::LetBool(a, b) | Token::LetDelegate(a, b) => {
                a.map_refs(f, g);
                b.map_refs(f, g);
            }
            Token::DynArrayElement { index, base } | Token::ArrayElement { index, base } => {
                index.map_refs(f, g);
                base.map_refs(f, g);
            }
            Token::New { outer, name, flags, class, template } => {
                outer.map_refs(f, g);
                name.map_refs(f, g);
                flags.map_refs(f, g);
                class.map_refs(f, g);
                template.map_refs(f, g);
            }
            Token::Context { object, prop, member, .. } => {
                object.map_refs(f, g);
                *prop = f(*prop);
                member.map_refs(f, g);
            }
            Token::MetaCast { class, expr } | Token::DynamicCast { class, expr } | Token::InterfaceCast { class, expr } => {
                *class = f(*class);
                expr.map_refs(f, g);
            }
            Token::Skip { expr, .. } => expr.map_refs(f, g),
            Token::VirtualFunction { name, args: a } | Token::GlobalFunction { name, args: a } => {
                *name = g(*name);
                args(a, f, g);
            }
            Token::FinalFunction { func, args: a } => {
                *func = f(*func);
                args(a, f, g);
            }
            Token::NativeFunction { args: a, .. } => args(a, f, g),
            Token::IntConst(_) | Token::FloatConst(_) | Token::StringConst(_) | Token::UnicodeStringConst(_) | Token::RotationConst(..) | Token::VectorConst(..) => {}
            Token::ByteConst(_) | Token::IntConstByte(_) | Token::IntZero | Token::IntOne | Token::True | Token::False | Token::NoObject => {}
            Token::NameConst(n) | Token::InstanceDelegate(n) => *n = g(*n),
            Token::BoolVariable(e) | Token::DynArrayLength(e) | Token::InterfaceContext(e) => e.map_refs(f, g),
            Token::Iterator { expr, .. } => expr.map_refs(f, g),
            Token::IteratorPop | Token::IteratorNext | Token::EmptyDelegate | Token::EmptyParmValue | Token::EndOfScript | Token::JumpIfFilterEditorOnly { .. } => {}
            Token::StructCmpEq { strct, a, b } | Token::StructCmpNe { strct, a, b } => {
                *strct = f(*strct);
                a.map_refs(f, g);
                b.map_refs(f, g);
            }
            Token::StructMember { prop, strct, expr, .. } => {
                *prop = f(*prop);
                *strct = f(*strct);
                expr.map_refs(f, g);
            }
            Token::PrimitiveCast { expr, .. } => expr.map_refs(f, g),
            Token::DynArrayInsert { base, index, count, debug } | Token::DynArrayRemove { base, index, count, debug } => {
                base.map_refs(f, g);
                index.map_refs(f, g);
                count.map_refs(f, g);
                opt(debug, f, g);
            }
            Token::DynArrayAdd { base, count, debug } => {
                base.map_refs(f, g);
                count.map_refs(f, g);
                opt(debug, f, g);
            }
            Token::DynArrayAddItem { base, item, debug, .. } | Token::DynArrayRemoveItem { base, item, debug, .. } | Token::DynArrayFind { base, item, debug, .. } => {
                base.map_refs(f, g);
                item.map_refs(f, g);
                opt(debug, f, g);
            }
            Token::DynArrayInsertItem { base, index, item, debug, .. } => {
                base.map_refs(f, g);
                index.map_refs(f, g);
                item.map_refs(f, g);
                opt(debug, f, g);
            }
            Token::DynArrayFindStruct { base, member, item, debug, .. } => {
                base.map_refs(f, g);
                member.map_refs(f, g);
                item.map_refs(f, g);
                opt(debug, f, g);
            }
            Token::DynArraySort { base, delegate, debug, .. } => {
                base.map_refs(f, g);
                delegate.map_refs(f, g);
                opt(debug, f, g);
            }
            Token::DynArrayIterator { array, item, index, .. } => {
                array.map_refs(f, g);
                item.map_refs(f, g);
                index.map_refs(f, g);
            }
            Token::DelegateCmp { args: a, .. } => args(a, f, g),
            Token::DebugInfo { .. } => {}
            Token::DelegateFunction { prop, name, args: a, .. } => {
                *prop = f(*prop);
                *name = g(*name);
                args(a, f, g);
            }
            Token::DelegateProperty { name, prop } => {
                *name = g(*name);
                *prop = f(*prop);
            }
            Token::Conditional { cond, a, b, .. } => {
                cond.map_refs(f, g);
                a.map_refs(f, g);
                b.map_refs(f, g);
            }
            Token::DefaultParmValue { debug_before, expr, debug_after, .. } => {
                opt(debug_before, f, g);
                expr.map_refs(f, g);
                opt(debug_after, f, g);
            }
        }
    }
}
