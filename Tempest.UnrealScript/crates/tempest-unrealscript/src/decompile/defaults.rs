//! Decoding and printing of tagged default-property values
//! (`defaultproperties` / `structdefaultproperties`).

use tempest_upk::flags::{cpf, pkg as pkgf, rf, strct};
use tempest_upk::io::{FName, PackageIndex, Reader};

use super::{escape_string, format_float, is_identifier, Decompiler};
use crate::context::ObjRef;
use crate::objects::{ObjectBody, PropertyKind, TaggedProperty};

/// Lines of `Name=Value` text for one property list.
pub struct DefaultLines {
    pub lines: Vec<String>,
    /// Subobject exports referenced (printed as `Begin Object` blocks by the caller).
    pub subobjects: Vec<ObjRef>,
}

impl<'a> Decompiler<'a> {
    pub fn pkgname_in(&self, pkg: usize, n: FName) -> String {
        self.ctx.packages[pkg].name_of(n)
    }

    /// Decode every tagged property of `owner` (the struct/class defining the
    /// properties) into `Name=Value` lines.
    pub fn default_lines(&self, props: &[TaggedProperty], owner: Option<ObjRef>, pkg: usize, cdo: Option<ObjRef>) -> DefaultLines {
        self.default_lines_of(props, owner, pkg, cdo, None)
    }

    /// Length of `prop_name`'s array in the archetype chain of `obj` (0 when unset).
    fn archetype_array_len(&self, obj: ObjRef, prop_name: &str) -> usize {
        let mut cur = {
            let a = self.ctx.packages[obj.pkg].pkg.exports[obj.export].archetype;
            self.ctx.resolve(obj.pkg, a).or_else(|| self.ctx.class_of(obj).and_then(|c| self.ctx.class_default_object(c)))
        };
        let mut guard = 0;
        while let Some(a) = cur {
            if let Some(o) = self.ctx.object(a) {
                for tp in &o.header.properties {
                    if self.ctx.packages[a.pkg].name_of(tp.name).eq_ignore_ascii_case(prop_name) && tp.array_index == 0 {
                        let mut r = Reader::new(&tp.raw);
                        return r.i32().unwrap_or(0).max(0) as usize;
                    }
                }
            }
            let next = self.ctx.packages[a.pkg].pkg.exports[a.export].archetype;
            cur = self.ctx.resolve(a.pkg, next);
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        0
    }

    /// `obj`: the object whose properties these are (its archetype decides whether an
    /// array must be emptied before its elements are listed).
    pub fn default_lines_of(&self, props: &[TaggedProperty], owner: Option<ObjRef>, pkg: usize, cdo: Option<ObjRef>, obj: Option<ObjRef>) -> DefaultLines {
        if let Some(o) = obj {
            self.bin_defaults.set(self.ctx.object_flags(o) & rf::CLASS_DEFAULT_OBJECT != 0);
        }
        let mut out = DefaultLines { lines: Vec::new(), subobjects: Vec::new() };
        for tp in props {
            let pname = self.pkgname_in(pkg, tp.name);
            let pdef = owner.and_then(|o| self.ctx.find_field(o, &pname));
            let (kind, _) = match pdef.and_then(|p| self.ctx.property(p)) {
                Some(p) => p,
                None => {
                    // Fall back on the tag type.
                    let ty = self.pkgname_in(pkg, tp.type_name);
                    match PropertyKind::from_class_name(&ty) {
                        Some(k) => (k, &DUMMY_PROP),
                        None => {
                            out.lines.push(format!("// unknown property {pname}"));
                            continue;
                        }
                    }
                }
            };
            let key = if tp.array_index > 0 || pdef.map(|p| self.ctx.property(p).map(|(_, d)| d.array_dim > 1).unwrap_or(false)).unwrap_or(false) {
                format!("{pname}({})", tp.array_index)
            } else {
                pname.clone()
            };
            match kind {
                PropertyKind::Array => {
                    let mut r = Reader::new(&tp.raw);
                    let Ok(n) = r.i32() else { continue };
                    let inner = pdef.and_then(|p| self.ctx.property(p).and_then(|(_, d)| d.ref1)).and_then(|i| self.ctx.resolve(pdef.unwrap().pkg, i));
                    if n == 0 {
                        out.lines.push(format!("{pname}.Empty"));
                        continue;
                    }
                    // Elements overlay the inherited array; a shorter one must be emptied first.
                    if let Some(o) = obj {
                        if (n as usize) < self.archetype_array_len(o, &pname) {
                            out.lines.push(format!("{pname}.Empty"));
                        }
                    }
                    for i in 0..n {
                        match inner {
                            Some(ip) => {
                                let v = self.value_text(&mut r, ip, pkg, cdo, &mut out.subobjects, tp);
                                out.lines.push(format!("{pname}({i})={v}"));
                            }
                            None => {
                                out.lines.push(format!("// {pname}({i}): unresolved array element type"));
                                break;
                            }
                        }
                    }
                }
                _ => {
                    let mut r = Reader::new(&tp.raw);
                    let v = match pdef {
                        Some(p) => self.value_text(&mut r, p, pkg, cdo, &mut out.subobjects, tp),
                        None => self.value_text_kind(&mut r, kind, None, pkg, cdo, &mut out.subobjects, tp),
                    };
                    out.lines.push(format!("{key}={v}"));
                }
            }
        }
        out
    }

    /// Print one value of property `p` read from `r`.
    fn value_text(&self, r: &mut Reader, p: ObjRef, pkg: usize, cdo: Option<ObjRef>, subs: &mut Vec<ObjRef>, tag: &TaggedProperty) -> String {
        let Some((kind, _)) = self.ctx.property(p) else { return "/*?*/".to_string() };
        self.value_text_kind(r, kind, Some(p), pkg, cdo, subs, tag)
    }

    fn value_text_kind(&self, r: &mut Reader, kind: PropertyKind, p: Option<ObjRef>, pkg: usize, cdo: Option<ObjRef>, subs: &mut Vec<ObjRef>, tag: &TaggedProperty) -> String {
        match kind {
            PropertyKind::Int => r.i32().map(|v| v.to_string()).unwrap_or_default(),
            PropertyKind::Float => r.f32().map(format_float).unwrap_or_default(),
            PropertyKind::Bool => {
                if tag.bool_val.map(|b| b != 0).unwrap_or(false) { "True".into() } else { "False".into() }
            }
            PropertyKind::Byte => {
                let has_enum = p.and_then(|p| self.ctx.enum_of_property(p)).is_some()
                    || tag.enum_name.map(|e| !self.ctx.packages[pkg].pkg.name_str(e.index).eq_ignore_ascii_case("None")).unwrap_or(false);
                if has_enum && r.remaining() >= 8 {
                    let n = r.fname().map(|n| self.pkgname_in(pkg, n)).unwrap_or_default();
                    // A value past the enum's last tag is saved as `None`; any such number
                    // compiles back to the same thing.
                    if n.eq_ignore_ascii_case("None") { "255".into() } else { n }
                } else {
                    r.u8().map(|v| v.to_string()).unwrap_or_default()
                }
            }
            PropertyKind::Name => {
                let n = r.fname().map(|n| self.pkgname_in(pkg, n)).unwrap_or_default();
                // `ReadToken` only knows double-quoted strings for names with other characters.
                if is_identifier(&n) { n } else { format!("\"{n}\"") }
            }
            PropertyKind::Str => {
                let s = r.fstring().unwrap_or_default();
                escape_string(s.as_bytes())
            }
            PropertyKind::Object | PropertyKind::Component | PropertyKind::SafeObject | PropertyKind::Class | PropertyKind::Interface => {
                let idx = r.index().unwrap_or_default();
                self.object_value(idx, pkg, cdo, subs)
            }
            PropertyKind::Delegate => {
                let _obj = r.index().unwrap_or_default();
                let n = r.fname().map(|n| self.pkgname_in(pkg, n)).unwrap_or_default();
                if n.eq_ignore_ascii_case("None") { "None".into() } else { n }
            }
            PropertyKind::Struct => {
                let sdef = p.and_then(|p| self.ctx.property(p).and_then(|(_, d)| d.ref1)).and_then(|s| self.ctx.resolve(p.unwrap().pkg, s));
                let Some(sdef) = sdef else {
                    let rest = r.take(r.remaining()).unwrap_or(&[]);
                    return format!("/* struct {} bytes */", rest.len());
                };
                let flags = match &self.ctx.object(sdef).map(|o| &o.body) {
                    Some(ObjectBody::ScriptStruct { data, .. }) => data.struct_flags,
                    _ => 0,
                };
                let binary = flags & (strct::IMMUTABLE | strct::IMMUTABLE_WHEN_COOKED) != 0;
                let mut parts = Vec::new();
                if binary {
                    let cooked = self.ctx.packages[pkg].pkg.summary.package_flags & pkgf::COOKED != 0;
                    for m in self.ctx.properties_in_serialize_order(sdef) {
                        let Some((mk, md)) = self.ctx.property(m) else { continue };
                        // `ShouldSerializeValue`: native never, transient only in defaults,
                        // deprecated only until the cooker re-saves.
                        let f = md.property_flags;
                        if f & cpf::NATIVE != 0 || (f & cpf::TRANSIENT != 0 && !self.bin_defaults.get()) || (f & cpf::DEPRECATED != 0 && cooked) {
                            continue;
                        }
                        let mname = self.ctx.name(m);
                        for ai in 0..md.array_dim.max(1) {
                            let key = if md.array_dim > 1 { format!("{mname}[{ai}]") } else { mname.clone() };
                            let v = self.binary_value(r, m, mk, pkg, cdo, subs);
                            parts.push(format!("{key}={v}"));
                        }
                    }
                } else {
                    let none = self.ctx.packages[pkg].none_name;
                    let inner = crate::objects::read_tagged_properties(r, &NameShim { pkg: &self.ctx.packages[pkg].pkg, none })
                        .unwrap_or_default();
                    let dl = self.default_lines(&inner, Some(sdef), pkg, cdo);
                    subs.extend(dl.subobjects);
                    // `X.Empty` is only valid as a statement; inside an inline struct literal an
                    // empty array is simply left out (it is the zero value anyway).
                    parts = dl.lines.into_iter().filter(|l| !l.ends_with(".Empty") && !l.starts_with("//")).collect();
                }
                format!("({})", parts.join(","))
            }
            PropertyKind::Array => {
                let inner = p.and_then(|p| self.ctx.property(p).and_then(|(_, d)| d.ref1)).and_then(|i| self.ctx.resolve(p.unwrap().pkg, i));
                let n = r.i32().unwrap_or(0);
                let mut items = Vec::new();
                for _ in 0..n {
                    match inner {
                        Some(ip) => items.push(self.value_text(r, ip, pkg, cdo, subs, tag)),
                        None => break,
                    }
                }
                format!("({})", items.join(","))
            }
            PropertyKind::Map => "/* map */".to_string(),
        }
    }

    /// Binary (`SerializeBin`) layout of a single property value.
    fn binary_value(&self, r: &mut Reader, p: ObjRef, kind: PropertyKind, pkg: usize, cdo: Option<ObjRef>, subs: &mut Vec<ObjRef>) -> String {
        match kind {
            PropertyKind::Bool => {
                if r.u8().unwrap_or(0) != 0 { "True".into() } else { "False".into() }
            }
            PropertyKind::Byte => {
                // `UByteProperty::SerializeItem`: an enum value is written as its tag name.
                if self.ctx.enum_of_property(p).is_some() {
                    return r.fname().map(|n| self.pkgname_in(pkg, n)).unwrap_or_else(|_| "0".into());
                }
                r.u8().unwrap_or(0).to_string()
            }
            PropertyKind::Struct => {
                let dummy = TaggedProperty { name: FName::default(), type_name: FName::default(), array_index: 0, struct_name: None, bool_val: None, enum_name: None, size: 0, raw: vec![] };
                self.value_text_kind(r, kind, Some(p), pkg, cdo, subs, &dummy)
            }
            _ => {
                let dummy = TaggedProperty { name: FName::default(), type_name: FName::default(), array_index: 0, struct_name: None, bool_val: None, enum_name: None, size: 0, raw: vec![] };
                self.value_text_kind(r, kind, Some(p), pkg, cdo, subs, &dummy)
            }
        }
    }

    /// Object reference inside defaults: subobjects of the CDO by name,
    /// everything else as a `Class'Path'` literal.
    pub fn object_value(&self, idx: PackageIndex, pkg: usize, cdo: Option<ObjRef>, subs: &mut Vec<ObjRef>) -> String {
        if idx.is_null() {
            return "None".to_string();
        }
        if let Some(r) = self.ctx.resolve(pkg, idx) {
            if let Some(cdo) = cdo {
                let mut cur = self.ctx.outer_of(r);
                let mut guard = 0;
                while let Some(o) = cur {
                    if o == cdo {
                        if !subs.contains(&r) {
                            subs.push(r);
                        }
                        return self.ctx.name(r);
                    }
                    cur = self.ctx.outer_of(o);
                    guard += 1;
                    if guard > 16 {
                        break;
                    }
                }
            }
        }
        let p = &self.ctx.packages[pkg].pkg;
        let class = p.class_name_of(idx);
        let forced = idx.export_index().map(|e| self.ctx.is_forced_export(ObjRef { pkg, export: e })).unwrap_or(false);
        let path = if idx.is_import() || forced { p.object_path(idx) } else { format!("{}.{}", self.ctx.packages[pkg].name, p.object_path(idx)) };
        if class == "Class" {
            format!("class'{path}'")
        } else {
            format!("{class}'{path}'")
        }
    }
}

static DUMMY_PROP: crate::objects::PropertyData = crate::objects::PropertyData {
    array_dim: 1,
    property_flags: 0,
    category: None,
    array_size_enum: None,
    rep_offset: None,
    ref1: None,
    ref2: None,
};

struct NameShim<'p> {
    pkg: &'p tempest_upk::Package,
    none: u32,
}

impl<'p> crate::objects::NameTable for NameShim<'p> {
    fn name(&self, index: u32) -> &str {
        self.pkg.name_str(index)
    }
    fn is_none(&self, n: FName) -> bool {
        n.index == self.none && n.number == 0
    }
}
