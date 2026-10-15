//! `FPropertyBase` port: the compiler's notion of a value type, plus the
//! conversion table (`GetConversion`) and `ConversionCost`.

use tempest_upk::flags::cpf;

use super::model::FieldRef;
use crate::bytecode::cast;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cpt {
    None,
    Byte,
    Int,
    Bool,
    Float,
    Object,
    Name,
    Delegate,
    Interface,
    Range,
    Struct,
    Vector,
    Rotation,
    String,
    Map,
}

impl Cpt {
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn tag_name(self) -> &'static str {
        match self {
            Cpt::Byte => "ByteProperty",
            Cpt::Int => "IntProperty",
            Cpt::Bool => "BoolProperty",
            Cpt::Float => "FloatProperty",
            Cpt::Object => "ObjectProperty",
            Cpt::Name => "NameProperty",
            Cpt::Delegate => "DelegateProperty",
            Cpt::Interface => "InterfaceProperty",
            Cpt::Struct => "StructProperty",
            Cpt::String => "StrProperty",
            Cpt::Map => "MapProperty",
            _ => "None",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefType {
    None,
    AssignValue,
    SimpleReference,
    AssignmentReference,
    DualReference,
}

/// The compiler's value type (`FPropertyBase`).
#[derive(Clone, Debug)]
pub struct PropType {
    pub cpt: Cpt,
    /// 1 = scalar, >1 static array, 0 dynamic array, -1 map.
    pub array_dim: i32,
    pub flags: u64,
    /// Enum (byte), class (object/interface), struct (struct), function (delegate).
    pub inner: Option<FieldRef>,
    /// `MetaClass` for class references.
    pub meta_class: Option<FieldRef>,
    pub array_index_enum: Option<FieldRef>,
    pub delegate_name: Option<String>,
    pub ref_type: RefType,
    /// Interface flag carried on object-typed tokens after conversions.
    pub interface: bool,
    /// Hi-Rez safe object reference (`USafeObjectProperty`).
    pub safe: bool,
}

impl PropType {
    pub fn new(cpt: Cpt) -> PropType {
        PropType {
            cpt,
            array_dim: 1,
            flags: 0,
            inner: None,
            meta_class: None,
            array_index_enum: None,
            delegate_name: None,
            ref_type: RefType::None,
            interface: false,
            safe: false,
        }
    }
    pub fn with_ref(cpt: Cpt, r: RefType) -> PropType {
        let mut p = PropType::new(cpt);
        p.ref_type = r;
        p
    }
    pub fn none() -> PropType {
        PropType::new(Cpt::None)
    }
    pub fn object(class: FieldRef, is_interface: bool) -> PropType {
        let mut p = PropType::new(if is_interface { Cpt::Interface } else { Cpt::Object });
        p.inner = Some(class);
        p
    }
    pub fn class_ref(meta: Option<FieldRef>, class_class: FieldRef) -> PropType {
        let mut p = PropType::new(Cpt::Object);
        p.inner = Some(class_class);
        p.meta_class = meta;
        p
    }
    pub fn enum_byte(e: Option<FieldRef>) -> PropType {
        let mut p = PropType::new(Cpt::Byte);
        p.inner = e;
        p
    }
    pub fn strct(s: FieldRef) -> PropType {
        let mut p = PropType::new(Cpt::Struct);
        p.inner = Some(s);
        p
    }

    pub fn is_object(&self) -> bool {
        matches!(self.cpt, Cpt::Object | Cpt::Interface)
    }
    pub fn is_dynamic_array(&self) -> bool {
        self.array_dim == 0
    }
    pub fn is_enum_variable(&self) -> bool {
        self.cpt == Cpt::Byte && self.inner.is_some()
    }
}

pub const AUTOCONVERT: u32 = 0x100;
pub const TRUNCATE: u32 = 0x200;
pub const CONVERT_MASK: u32 = !(AUTOCONVERT | TRUNCATE);
pub const CST_MAX: u32 = 0xFF;

const AC: u32 = AUTOCONVERT;
const TAC: u32 = TRUNCATE | AUTOCONVERT;

/// `GetConversion(Dest, Src)` table indexed `[dest][src]` by `Cpt` order.
#[rustfmt::skip]
const CONVERSIONS: [[u32; 15]; 15] = {
    use cast::*;
    const M: u32 = CST_MAX;
    [
    /* None     */ [M, M, M, M, M, M, M, M, M, M, M, M, M, M, M],
    /* Byte     */ [M, M, INT_TO_BYTE as u32 | TAC, BOOL_TO_BYTE as u32, FLOAT_TO_BYTE as u32 | TAC, M, M, M, M, M, M, M, M, STRING_TO_BYTE as u32, M],
    /* Int      */ [M, BYTE_TO_INT as u32 | AC, M, BOOL_TO_INT as u32, FLOAT_TO_INT as u32 | TAC, M, M, M, M, M, M, M, M, STRING_TO_INT as u32, M],
    /* Bool     */ [M, BYTE_TO_BOOL as u32, INT_TO_BOOL as u32, M, FLOAT_TO_BOOL as u32, OBJECT_TO_BOOL as u32, NAME_TO_BOOL as u32, M, INTERFACE_TO_BOOL as u32, M, M, VECTOR_TO_BOOL as u32, ROTATOR_TO_BOOL as u32, STRING_TO_BOOL as u32, M],
    /* Float    */ [M, BYTE_TO_FLOAT as u32 | AC, INT_TO_FLOAT as u32 | AC, BOOL_TO_FLOAT as u32, M, M, M, M, M, M, M, M, M, STRING_TO_FLOAT as u32, M],
    /* Object   */ [M, M, M, M, M, M, M, M, INTERFACE_TO_OBJECT as u32 | AC, M, M, M, M, M, M],
    /* Name     */ [M, M, M, M, M, M, M, M, M, M, M, M, M, STRING_TO_NAME as u32, M],
    /* Delegate */ [M, M, M, M, M, M, M, M, M, M, M, M, M, M, M],
    /* Interface*/ [M, M, M, M, M, AC | OBJECT_TO_INTERFACE as u32, M, M, M, M, M, M, M, M, M],
    /* Range    */ [M, M, M, M, M, M, M, M, M, M, M, M, M, M, M],
    /* Struct   */ [M, M, M, M, M, M, M, M, M, M, M, M, M, M, M],
    /* Vector   */ [M, M, M, M, M, M, M, M, M, M, M, M, ROTATOR_TO_VECTOR as u32, STRING_TO_VECTOR as u32, M],
    /* Rotator  */ [M, M, M, M, M, M, M, M, M, M, M, VECTOR_TO_ROTATOR as u32, M, STRING_TO_ROTATOR as u32, M],
    /* String   */ [M, BYTE_TO_STRING as u32, INT_TO_STRING as u32, BOOL_TO_STRING as u32, FLOAT_TO_STRING as u32, OBJECT_TO_STRING as u32, NAME_TO_STRING as u32, DELEGATE_TO_STRING as u32, INTERFACE_TO_STRING as u32, M, M, VECTOR_TO_STRING as u32, ROTATOR_TO_STRING as u32, M, M],
    /* Map      */ [M, M, M, M, M, M, M, M, M, M, M, M, M, M, M],
    ]
};

pub const CONVERSION_EXPANSION: i32 = 101;
pub const CONVERSION_INT_TO_FLOAT: i32 = 103;
pub const CONVERSION_TRUNCATION: i32 = 104;
pub const MAXINT: i32 = i32::MAX;

/// Hi-Rez additions: safe object <-> object/interface/bool/string.
pub fn safe_object_conversion(dest: &PropType, src: &PropType, dest_is_safe: bool, src_is_safe: bool) -> Option<u32> {
    if src_is_safe && !dest_is_safe {
        return Some(match dest.cpt {
            Cpt::Object => cast::SAFE_OBJECT_TO_OBJECT as u32 | AC,
            Cpt::Interface => cast::SAFE_OBJECT_TO_INTERFACE as u32 | AC,
            Cpt::Bool => cast::SAFE_OBJECT_TO_BOOL as u32,
            Cpt::String => cast::SAFE_OBJECT_TO_STRING as u32,
            _ => CST_MAX,
        });
    }
    if dest_is_safe && !src_is_safe {
        return Some(match src.cpt {
            Cpt::Object => cast::OBJECT_TO_SAFE_OBJECT as u32 | AC,
            Cpt::Interface => cast::INTERFACE_TO_SAFE_OBJECT as u32 | AC,
            _ => CST_MAX,
        });
    }
    None
}

pub fn conversion_index(t: &PropType, is_vector: bool, is_rotator: bool) -> usize {
    if is_vector {
        Cpt::Vector.index()
    } else if is_rotator {
        Cpt::Rotation.index()
    } else {
        t.cpt.index()
    }
}

pub fn get_conversion_raw(dest_idx: usize, src_idx: usize) -> u32 {
    CONVERSIONS[dest_idx][src_idx]
}

pub const CPF_PARM_FLAGS: u64 = cpf::PARM_FLAGS;
