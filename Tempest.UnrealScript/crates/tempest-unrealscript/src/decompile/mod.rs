//! Bytecode → UnrealScript source.
//!
//! The output is meant to recompile (with this crate's compiler) to the same
//! bytecode and object layout, so casts and parentheses are emitted
//! conservatively rather than minimally.

pub mod decl;
pub mod plan;
pub mod defaults;
pub mod expr;
pub mod flow;

use crate::context::{Context, ObjRef};

/// Shared state for decompiling one class.
pub struct Decompiler<'a> {
    pub ctx: &'a Context,
    /// Package slot the class lives in.
    pub pkg: usize,
    /// The class being decompiled.
    pub class: ObjRef,
    /// Same-package classes whose enums/structs this class's declarations use and
    /// that the compiler would parse later: they need `dependson(...)`.
    pub depends_on: std::cell::RefCell<std::collections::BTreeSet<String>>,
    /// Lazily built count of struct/enum names across loaded packages.
    pub type_counts: std::cell::RefCell<Option<std::collections::HashMap<String, usize>>>,
    /// `dependson` list decided for the whole package (see `plan`); when set it
    /// replaces the per-class collection in the header.
    pub forced_depends: Option<Vec<String>>,
    /// Whether the values being rendered belong to a class default object or a
    /// struct's defaults (`Ar.IsSerializingDefaults()`: transient members of binary
    /// structs are present only then).
    pub bin_defaults: std::cell::Cell<bool>,
    /// Editor-only variables the cooker removed but bytecode still refers to (as null):
    /// `(type, stand-in name)`, declared `editoronly` so the compiler drops them again.
    pub stripped_refs: std::cell::RefCell<Vec<(String, String)>>,
}

impl<'a> Decompiler<'a> {
    pub fn new(ctx: &'a Context, class: ObjRef) -> Self {
        Decompiler { ctx, pkg: class.pkg, class, depends_on: Default::default(), type_counts: Default::default(), forced_depends: None, bin_defaults: std::cell::Cell::new(true), stripped_refs: Default::default() }
    }

    pub fn with_depends(ctx: &'a Context, class: ObjRef, depends: Vec<String>) -> Self {
        let mut d = Self::new(ctx, class);
        d.forced_depends = Some(depends);
        d
    }

    /// The `dependson` names to write in the class header.
    pub fn header_depends(&self) -> Vec<String> {
        match &self.forced_depends {
            Some(v) => v.clone(),
            None => self.depends_on.borrow().iter().cloned().collect(),
        }
    }

    /// Full class source.
    pub fn class_source(&self) -> String {
        decl::class_source(self)
    }
}

/// Escape a string for an UnrealScript string literal.
pub fn escape_string(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() + 2);
    s.push('"');
    for &b in bytes {
        match b {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            0x20..=0x7e => s.push(b as char),
            _ => s.push(b as char), // Latin-1 passthrough
        }
    }
    s.push('"');
    s
}

pub fn escape_utf16(units: &[u16]) -> String {
    let text = String::from_utf16_lossy(units);
    let mut s = String::with_capacity(text.len() + 2);
    s.push('"');
    for c in text.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            _ => s.push(c),
        }
    }
    s.push('"');
    s
}

/// Shortest float text that reads back to the same `f32`, always with a
/// decimal point so the lexer sees a float.
pub fn format_float(v: f32) -> String {
    if v.is_nan() {
        return "0.0".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "1e38".to_string() } else { "-1e38".to_string() };
    }
    let mut s = format!("{v}");
    if !s.contains('.') && !s.contains('e') && !s.contains("inf") && !s.contains("NaN") {
        s.push_str(".0");
    }
    s
}

/// Quote a name literal.
pub fn name_literal(s: &str) -> String {
    format!("'{s}'")
}

pub fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
