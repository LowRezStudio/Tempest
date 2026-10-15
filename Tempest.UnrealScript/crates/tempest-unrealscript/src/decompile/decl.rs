//! Class, struct, enum, const, variable, function and state declarations.

use std::collections::BTreeMap;

use tempest_upk::flags::{class as cf, cpf, func, rf, state as sf, strct};
use tempest_upk::io::{FName, PackageIndex};

use super::expr::{Hint, Scope};
use super::flow::Flow;
use super::Decompiler;
use crate::bytecode::{Disassembler, Token};
use crate::context::ObjRef;
use crate::objects::{ObjectBody, PropertyKind};

fn join_nonempty(parts: Vec<String>) -> String {
    parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

impl<'a> Decompiler<'a> {
    fn lp(&self) -> &crate::context::LoadedPackage {
        &self.ctx.packages[self.pkg]
    }

    fn fname(&self, n: tempest_upk::io::FName) -> String {
        self.lp().name_of(n)
    }

    /// Type name for a property as written in a declaration.
    /// Name of an import that resolves to no loaded export (e.g. a class the cooker
    /// left out of every script package), so the declaration still names it.
    fn unresolved_import_name(&self, pkg: usize, idx: Option<PackageIndex>) -> Option<String> {
        let idx = idx?;
        if !idx.is_import() {
            return None;
        }
        Some(self.ctx.packages[pkg].pkg.object_name(idx))
    }

    pub fn type_name(&self, p: ObjRef, scope_class: ObjRef) -> String {
        let Some((kind, d)) = self.ctx.property(p) else { return "int".into() };
        let r1 = d.ref1.and_then(|r| self.ctx.resolve(p.pkg, r));
        let r2 = d.ref2.and_then(|r| self.ctx.resolve(p.pkg, r));
        let u1 = || self.unresolved_import_name(p.pkg, d.ref1);
        let u2 = || self.unresolved_import_name(p.pkg, d.ref2);
        match kind {
            PropertyKind::Int => "int".into(),
            PropertyKind::Float => "float".into(),
            PropertyKind::Bool => "bool".into(),
            PropertyKind::Name => "name".into(),
            PropertyKind::Str => "string".into(),
            PropertyKind::Byte => match r1 {
                Some(e) if self.type_through_component_class(p) => self.class_qualified_type(e),
                Some(e) => self.qualified_type(e, scope_class),
                None => u1().unwrap_or_else(|| "byte".into()),
            },
            PropertyKind::Object | PropertyKind::Component => match r1 {
                Some(c) => self.ctx.name(c),
                None => u1().unwrap_or_else(|| "Object".into()),
            },
            PropertyKind::SafeObject => match r1 {
                Some(c) => format!("safeobject<{}>", self.ctx.name(c)),
                None => format!("safeobject<{}>", u1().unwrap_or_else(|| "Object".into())),
            },
            PropertyKind::Interface => match r1 {
                Some(c) => self.ctx.name(c),
                None => u1().unwrap_or_else(|| "Interface".into()),
            },
            PropertyKind::Class => match r2 {
                Some(m) if !self.ctx.name(m).eq_ignore_ascii_case("Object") => format!("class<{}>", self.ctx.name(m)),
                Some(_) => "class".into(),
                None => match u2() {
                    Some(m) if !m.eq_ignore_ascii_case("Object") => format!("class<{m}>"),
                    _ => "class".into(),
                },
            },
            PropertyKind::Struct => match r1 {
                Some(s) if self.type_through_component_class(p) => self.class_qualified_type(s),
                Some(s) => self.qualified_type(s, scope_class),
                None => u1().unwrap_or_else(|| "struct".into()),
            },
            PropertyKind::Array => match r1 {
                Some(inner) => {
                    let t = self.type_name(inner, scope_class);
                    // `>>` would lex as a shift operator.
                    if t.ends_with('>') { format!("array<{t} >") } else { format!("array<{t}>") }
                }
                None => "array<int>".into(),
            },
            // The C++ key/value text of a native map is not serialized; it only feeds
            // header export, so any placeholder keeps the declaration compilable.
            PropertyKind::Map => "map{UNKNOWN_KEY,UNKNOWN_VALUE}".into(),
            PropertyKind::Delegate => match r1 {
                Some(f) => {
                    let owner = self.ctx.outer_of(f);
                    if owner.map(|o| self.ctx.derives_from(scope_class, o)).unwrap_or(true) {
                        format!("delegate<{}>", self.ctx.name(f))
                    } else {
                        format!("delegate<{}.{}>", owner.map(|o| self.ctx.name(o)).unwrap_or_default(), self.ctx.name(f))
                    }
                }
                None => "delegate".into(),
            },
        }
    }

    /// `var Class.Type X;` through a component class makes the compiler add
    /// CPF_Component|CPF_ExportObject to the property (a quirk of `GetVarType`): a
    /// byte/struct property carrying CPF_Component without a components struct was
    /// declared that way.
    fn type_through_component_class(&self, p: ObjRef) -> bool {
        let Some((kind, d)) = self.ctx.property(p) else { return false };
        if d.property_flags & cpf::COMPONENT == 0 || !matches!(kind, PropertyKind::Byte | PropertyKind::Struct) {
            return false;
        }
        let Some(t) = d.ref1.and_then(|r| self.ctx.resolve(p.pkg, r)) else { return false };
        if let Some(ObjectBody::ScriptStruct { data, .. }) = self.ctx.object(t).map(|o| &o.body) {
            if data.struct_flags & strct::HAS_COMPONENTS != 0 {
                return false;
            }
        }
        self.ctx.owner_class(t).map(|o| self.ctx.is_a(o, "Component")).unwrap_or(false)
    }

    /// `OwnerClass.Type`, always qualified.
    fn class_qualified_type(&self, t: ObjRef) -> String {
        let name = self.ctx.name(t);
        let Some(owner) = self.ctx.owner_class(t) else { return name };
        if owner.pkg == self.pkg {
            self.depends_on.borrow_mut().insert(self.ctx.name(owner));
        }
        format!("{}.{}", self.ctx.name(owner), name)
    }

    /// A struct/enum name, qualified with its class when it is not visible
    /// from `scope_class` (declared in an unrelated class).
    fn qualified_type(&self, t: ObjRef, scope_class: ObjRef) -> String {
        let name = self.ctx.name(t);
        let Some(owner) = self.ctx.owner_class(t) else { return name };
        if self.ctx.derives_from(scope_class, owner) {
            return name;
        }
        // DependsOn chains reorder parsing beyond the plain class-tree walk, so every
        // same-package owner that is not an ancestor is declared (extra ones are harmless).
        if owner.pkg == self.pkg {
            self.depends_on.borrow_mut().insert(self.ctx.name(owner));
        }
        // Unqualified names resolve anywhere (`FindObject(ANY_PACKAGE)`); qualifying
        // through a component class would also add component flags, so only qualify
        // when another struct/enum of that name exists.
        if self.type_name_count(&name) <= 1 {
            return name;
        }
        format!("{}.{}", self.ctx.name(owner), name)
    }

    /// A struct/enum name as `FindField` must see it: bare when declared in the
    /// scope class or an ancestor, otherwise qualified with its class.
    fn scoped_type(&self, t: ObjRef, scope_class: ObjRef) -> String {
        let name = self.ctx.name(t);
        let Some(owner) = self.ctx.owner_class(t) else { return name };
        if self.ctx.derives_from(scope_class, owner) {
            return name;
        }
        if owner.pkg == self.pkg {
            self.depends_on.borrow_mut().insert(self.ctx.name(owner));
        }
        format!("{}.{}", self.ctx.name(owner), name)
    }

    /// Number of script structs/enums named `name` across the loaded packages.
    fn type_name_count(&self, name: &str) -> usize {
        let mut cache = self.type_counts.borrow_mut();
        if cache.is_none() {
            let mut m: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for lp in &self.ctx.packages {
                for i in 0..lp.pkg.exports.len() {
                    let cn = lp.pkg.class_name_of(PackageIndex::export(i));
                    if cn == "ScriptStruct" || cn == "Enum" {
                        *m.entry(lp.export_name(i).to_ascii_lowercase()).or_insert(0) += 1;
                    }
                }
            }
            *cache = Some(m);
        }
        cache.as_ref().unwrap().get(&name.to_ascii_lowercase()).copied().unwrap_or(0)
    }

    /// Whether `a` precedes `b` in the compiler's class-tree walk (pre-order,
    /// siblings alphabetical): compare the first differing ancestors' names.
    fn parsed_before(&self, a: ObjRef, b: ObjRef) -> bool {
        let chain = |c: ObjRef| {
            let mut v = vec![c];
            let mut cur = c;
            while let Some(s) = self.ctx.super_of(cur) {
                v.push(s);
                cur = s;
            }
            v.reverse();
            v
        };
        let ca = chain(a);
        let cb = chain(b);
        for (x, y) in ca.iter().zip(cb.iter()) {
            if x != y {
                return self.ctx.name(*x).to_ascii_lowercase() < self.ctx.name(*y).to_ascii_lowercase();
            }
        }
        ca.len() <= cb.len()
    }

    fn access_modifier(&self, r: ObjRef) -> &'static str {
        let f = self.ctx.object_flags(r);
        if f & rf::PROTECTED != 0 {
            "protected"
        } else if f & rf::PUBLIC == 0 {
            "private"
        } else {
            ""
        }
    }

    fn var_modifiers(&self, p: ObjRef, in_struct: bool) -> String {
        let Some((kind, d)) = self.ctx.property(p) else { return String::new() };
        let f = d.property_flags;
        let mut m: Vec<&str> = Vec::new();
        let access = self.access_modifier(p);
        if !access.is_empty() {
            m.push(access);
        }
        let push = |m: &mut Vec<&str>, bit: u64, name: &'static str| {
            if f & bit != 0 {
                m.push(name);
            }
        };
        push(&mut m, cpf::CONST, "const");
        push(&mut m, cpf::INPUT, "input");
        push(&mut m, cpf::NATIVE, "native");
        push(&mut m, cpf::TRANSIENT, "transient");
        push(&mut m, cpf::DUPLICATE_TRANSIENT, "duplicatetransient");
        push(&mut m, cpf::NON_TRANSACTIONAL, "nontransactional");
        if f & cpf::GLOBAL_CONFIG != 0 {
            m.push("globalconfig");
        } else if f & cpf::CONFIG != 0 {
            m.push("config");
        }
        push(&mut m, cpf::LOCALIZED, "localized");
        push(&mut m, cpf::EDIT_CONST, "editconst");
        push(&mut m, cpf::EDIT_FIXED_SIZE, "editfixedsize");
        push(&mut m, cpf::EDIT_INLINE_USE, "editinlineuse");
        // Component references get EditInline, Component and ExportObject implicitly.
        let implied_component = kind == PropertyKind::Component || self.type_through_component_class(p);
        if f & cpf::EDIT_INLINE != 0 && f & cpf::EDIT_INLINE_USE == 0 && !implied_component {
            m.push("editinline");
        }
        if f & cpf::EXPORT_OBJECT != 0 && !implied_component {
            m.push("export");
        }
        push(&mut m, cpf::NO_EXPORT, "noexport");
        push(&mut m, cpf::NO_IMPORT, "noimport");
        push(&mut m, cpf::NO_CLEAR, "noclear");
        push(&mut m, cpf::DEPRECATED, "deprecated");
        push(&mut m, cpf::DATA_BINDING, "databinding");
        push(&mut m, cpf::SERIALIZE_TEXT, "serializetext");
        push(&mut m, cpf::REP_NOTIFY, "repnotify");
        push(&mut m, cpf::REP_RETRY, "repretry");
        push(&mut m, cpf::INTERP, "interp");
        push(&mut m, cpf::EDITOR_ONLY, "editoronly");
        push(&mut m, cpf::NOT_FOR_CONSOLE, "notforconsole");
        push(&mut m, cpf::PRIVATE_WRITE, "privatewrite");
        push(&mut m, cpf::PROTECTED_WRITE, "protectedwrite");
        push(&mut m, cpf::ARCHETYPE_PROPERTY, "archetype");
        push(&mut m, cpf::EDIT_HIDE, "edithide");
        push(&mut m, cpf::EDIT_TEXT_BOX, "edittextbox");
        push(&mut m, cpf::CROSS_LEVEL_PASSIVE, "crosslevelpassive");
        push(&mut m, cpf::CROSS_LEVEL_ACTIVE, "crosslevelactive");
        // Members of a transient struct get CPF_AlwaysInit from the compiler (after the
        // array inner is created, so the inner does not inherit it): never spell it out.
        let in_transient_struct = self
            .ctx
            .outer_of(p)
            .and_then(|o| match self.ctx.object(o).map(|x| &x.body) {
                Some(ObjectBody::ScriptStruct { data, .. }) => Some(data.struct_flags & strct::TRANSIENT != 0),
                _ => None,
            })
            .unwrap_or(false);
        if f & cpf::ALWAYS_INIT != 0 && !in_transient_struct {
            m.push("init");
        }
        if in_struct && f & cpf::COMPONENT != 0 && kind != PropertyKind::Component {
            // component flag is derived
        }
        m.join(" ")
    }

    fn var_decl(&self, p: ObjRef, scope_class: ObjRef, in_struct: bool) -> String {
        let Some((_, d)) = self.ctx.property(p) else { return String::new() };
        let cat = d.category.map(|c| self.fname(c)).filter(|c| !c.eq_ignore_ascii_case("None"));
        let edit = d.property_flags & cpf::EDIT != 0;
        let head = match (edit, cat) {
            (true, Some(c)) => {
                if c == self.ctx.name(scope_class) {
                    "var()".to_string()
                } else {
                    format!("var({c})")
                }
            }
            (true, None) => "var()".to_string(),
            _ => "var".to_string(),
        };
        let mods = self.var_modifiers(p, in_struct);
        let ty = self.type_name(p, scope_class);
        let mut name = self.ctx.name(p);
        if d.array_dim > 1 {
            match d.array_size_enum.and_then(|e| self.ctx.resolve(p.pkg, e)) {
                Some(e) => {
                    // `FindObject(ANY_PACKAGE)`: the enum's class must have been parsed already.
                    if let Some(owner) = self.ctx.owner_class(e) {
                        if owner.pkg == self.pkg && !self.ctx.derives_from(scope_class, owner) {
                            self.depends_on.borrow_mut().insert(self.ctx.name(owner));
                        }
                    }
                    name = format!("{name}[{}.EnumCount]", self.ctx.name(e));
                }
                None => name = format!("{name}[{}]", d.array_dim),
            }
        }
        format!("{};", join_nonempty(vec![head, mods, ty, name]))
    }

    fn param_decl(&self, p: ObjRef, scope_class: ObjRef, default: Option<&str>) -> String {
        let Some((_, d)) = self.ctx.property(p) else { return String::new() };
        let f = d.property_flags;
        let mut m: Vec<&str> = Vec::new();
        if f & cpf::OPTIONAL_PARM != 0 {
            m.push("optional");
        }
        if f & cpf::OUT_PARM != 0 {
            m.push("out");
        }
        if f & cpf::COERCE_PARM != 0 {
            m.push("coerce");
        }
        if f & cpf::SKIP_PARM != 0 {
            m.push("skip");
        }
        if f & cpf::CONST != 0 {
            m.push("const");
        }
        if f & cpf::ALWAYS_INIT != 0 {
            m.push("init");
        }
        let ty = self.type_name(p, scope_class);
        let mut name = self.ctx.name(p);
        if d.array_dim > 1 {
            name = format!("{name}[{}]", d.array_dim);
        }
        let mut s = join_nonempty(vec![m.join(" "), ty, name]);
        if let Some(dv) = default {
            s.push_str(" = ");
            s.push_str(dv);
        }
        s
    }

    // ----- class ---------------------------------------------------------

    pub fn class_header(&self) -> String {
        let c = self.class;
        let Some(ParsedClass { flags, within, config, interfaces, editor }) = self.parsed_class(c) else {
            return format!("class {};", self.ctx.name(c));
        };
        let name = self.ctx.name(c);
        let sup = self.ctx.super_of(c);
        let mut lines = Vec::new();
        if flags & cf::INTERFACE != 0 {
            // `CompileInterfaceDeclaration`: only extends/native/nativeonly/dependson apply.
            let mut head = format!("interface {name}");
            // The root `Interface` class itself extends Object implicitly.
            if let Some(s) = sup.filter(|s| !self.ctx.name(*s).eq_ignore_ascii_case("Interface") && !name.eq_ignore_ascii_case("Interface")) {
                head.push_str(&format!(" extends {}", self.ctx.name(s)));
            }
            lines.push(head);
            if flags & cf::NATIVE != 0 {
                let kw = if flags & cf::NATIVE_ONLY != 0 { "nativeonly" } else { "native" };
                match editor.as_ref().map(|e| e.class_header_filename.clone()).filter(|s| !s.is_empty()) {
                    Some(h) => lines.push(format!("{kw}({h})")),
                    None => lines.push(kw.into()),
                }
            }
            for d in self.header_depends() {
                lines.push(format!("dependson({d})"));
            }
            let mut s = lines[0].clone();
            for l in &lines[1..] {
                s.push_str("\n    ");
                s.push_str(l);
            }
            s.push(';');
            return s;
        }
        let mut head = format!("class {name}");
        if let Some(s) = sup {
            head.push_str(&format!(" extends {}", self.ctx.name(s)));
        } else if let Some(raw) = self.ctx.struct_data(c).map(|d| d.super_struct).filter(|s| !s.is_null()) {
            // The parent lives in a package that is not loaded: keep the qualified name so
            // the compile fails on the missing package instead of a bad class definition.
            let qualified = self.ctx.packages[c.pkg].pkg.object_path(raw);
            eprintln!("warning: {name}: superclass {qualified} is in a package that could not be loaded");
            head.push_str(&format!(" extends {qualified}"));
        }
        let parent_flags = sup.and_then(|s| self.parsed_class(s)).map(|p| p.flags).unwrap_or(0);
        lines.push(head);
        if let Some(w) = within {
            let wn = self.ctx.name(w);
            let parent_within = sup.and_then(|s| self.parsed_class(s)).and_then(|p| p.within).map(|w| self.ctx.name(w));
            if parent_within.as_deref() != Some(&wn) || parent_within.is_none() && !wn.eq_ignore_ascii_case("Object") {
                lines.push(format!("within {wn}"));
            }
        }
        let has = |b: u32| flags & b != 0;
        let parent_has = |b: u32| parent_flags & b != 0;
        if has(cf::NATIVE) {
            match editor.as_ref().map(|e| e.class_header_filename.clone()).filter(|s| !s.is_empty()) {
                Some(h) => lines.push(format!("native({h})")),
                None => lines.push("native".into()),
            }
        }
        if has(cf::NATIVE_REPLICATION) {
            lines.push("nativereplication".into());
        }
        for d in self.header_depends() {
            lines.push(format!("dependson({d})"));
        }
        if has(cf::NATIVE_ONLY) {
            lines.push("nativeonly".into());
        }
        if has(cf::NO_EXPORT) {
            lines.push("noexport".into());
        }
        if has(cf::ABSTRACT) {
            lines.push("abstract".into());
        }
        if has(cf::DEPRECATED) {
            lines.push("deprecated".into());
        }
        if has(cf::TRANSIENT) && !parent_has(cf::TRANSIENT) {
            lines.push("transient".into());
        } else if !has(cf::TRANSIENT) && parent_has(cf::TRANSIENT) {
            lines.push("nontransient".into());
        }
        {
            // The compiler clears the config name unless the class declares one.
            let cn = config.clone().unwrap_or_default();
            if !cn.is_empty() && !cn.eq_ignore_ascii_case("None") {
                lines.push(format!("config({cn})"));
            }
        }
        if has(cf::PER_OBJECT_CONFIG) && !parent_has(cf::PER_OBJECT_CONFIG) {
            lines.push("perobjectconfig".into());
        }
        if has(cf::PER_OBJECT_LOCALIZED) && !parent_has(cf::PER_OBJECT_LOCALIZED) {
            lines.push("perobjectlocalized".into());
        }
        if has(cf::PLACEABLE) && !parent_has(cf::PLACEABLE) {
            lines.push("placeable".into());
        } else if !has(cf::PLACEABLE) && parent_has(cf::PLACEABLE) {
            lines.push("notplaceable".into());
        }
        if has(cf::EDIT_INLINE_NEW) && !parent_has(cf::EDIT_INLINE_NEW) {
            lines.push("editinlinenew".into());
        } else if !has(cf::EDIT_INLINE_NEW) && parent_has(cf::EDIT_INLINE_NEW) {
            lines.push("noteditinlinenew".into());
        }
        if has(cf::COLLAPSE_CATEGORIES) && !parent_has(cf::COLLAPSE_CATEGORIES) {
            lines.push("collapsecategories".into());
        } else if !has(cf::COLLAPSE_CATEGORIES) && parent_has(cf::COLLAPSE_CATEGORIES) {
            lines.push("dontcollapsecategories".into());
        }
        if has(cf::HIDDEN) {
            lines.push("hidden".into());
        }
        if has(cf::HIDE_DROP_DOWN) {
            lines.push("hidedropdown".into());
        }
        if let Some(e) = &editor {
            let pe = sup.and_then(|s| self.parsed_class(s)).and_then(|p| p.editor);
            // Category lists are appended to (duplicates included), so compare counts.
            let diff = |mine: &Vec<String>, theirs: Option<&Vec<String>>| -> (Vec<String>, Vec<String>) {
                let t: Vec<String> = theirs.cloned().unwrap_or_default();
                let count = |v: &Vec<String>, x: &str| v.iter().filter(|y| y.eq_ignore_ascii_case(x)).count();
                let mut added: Vec<String> = Vec::new();
                for x in mine {
                    let extra = count(mine, x) as isize - count(&t, x) as isize - count(&added, x) as isize;
                    if extra > 0 {
                        added.push(x.clone());
                    }
                }
                let mut removed: Vec<String> = Vec::new();
                for x in &t {
                    let extra = count(&t, x) as isize - count(mine, x) as isize - count(&removed, x) as isize;
                    if extra > 0 {
                        removed.push(x.clone());
                    }
                }
                (added, removed)
            };
            let (add, rem) = diff(&e.hide_categories, pe.as_ref().map(|p| &p.hide_categories));
            if !add.is_empty() {
                lines.push(format!("hidecategories({})", add.join(",")));
            }
            if !rem.is_empty() {
                lines.push(format!("showcategories({})", rem.join(",")));
            }
            let (add, _) = diff(&e.auto_expand_categories, pe.as_ref().map(|p| &p.auto_expand_categories));
            if !add.is_empty() {
                lines.push(format!("autoexpandcategories({})", add.join(",")));
            }
            let (add, _) = diff(&e.auto_collapse_categories, pe.as_ref().map(|p| &p.auto_collapse_categories));
            if !add.is_empty() {
                lines.push(format!("autocollapsecategories({})", add.join(",")));
            }
            let (add, _) = diff(&e.dont_sort_categories, pe.as_ref().map(|p| &p.dont_sort_categories));
            if !add.is_empty() {
                lines.push(format!("dontsortcategories({})", add.join(",")));
            }
            // `classgroup` is not inherited from the parent class.
            if !e.class_group_names.is_empty() {
                lines.push(format!("classgroup({})", e.class_group_names.join(",")));
            }
            if e.force_script_order != 0 {
                lines.push("forcescriptorder(true)".into());
            }
        }
        // `inherits(...)`: VfTable_X properties not explained by implemented native interfaces.
        {
            let iface_names: Vec<String> = interfaces.iter().map(|i| format!("I{}", self.ctx.name(*i))).collect();
            let mut inherits: Vec<String> = Vec::new();
            for ch in self.ctx.children(self.class) {
                let n = self.ctx.name(ch);
                if let Some(x) = n.strip_prefix("VfTable_") {
                    if self.ctx.property_kind(ch).is_some() && !iface_names.iter().any(|i| i.eq_ignore_ascii_case(x)) {
                        inherits.push(x.to_string());
                    }
                }
            }
            if !inherits.is_empty() {
                lines.push(format!("inherits({})", inherits.join(",")));
            }
        }
        if !interfaces.is_empty() {
            let names: Vec<String> = interfaces.iter().map(|i| self.ctx.name(*i)).collect();
            lines.push(format!("implements({})", names.join(",")));
        }
        if let Some(d) = self.dll_bind_name() {
            lines.push(format!("DLLBind({d})"));
        }
        let mut s = lines[0].clone();
        for l in &lines[1..] {
            s.push_str("\n    ");
            s.push_str(l);
        }
        s.push(';');
        s
    }

    fn dll_bind_name(&self) -> Option<String> {
        match &self.ctx.object(self.class)?.body {
            ObjectBody::Class { class, .. } => {
                let n = self.fname(class.dll_bind_name);
                if n.eq_ignore_ascii_case("None") { None } else { Some(n) }
            }
            _ => None,
        }
    }

    fn parsed_class(&self, c: ObjRef) -> Option<ParsedClass> {
        match &self.ctx.object(c)?.body {
            ObjectBody::Class { class, .. } => {
                let lp = &self.ctx.packages[c.pkg];
                let names = |v: &Vec<tempest_upk::io::FName>| v.iter().map(|n| lp.name_of(*n)).collect::<Vec<_>>();
                Some(ParsedClass {
                    flags: class.class_flags,
                    within: self.ctx.resolve(c.pkg, class.class_within).filter(|w| !self.ctx.name(*w).eq_ignore_ascii_case("Object")),
                    config: Some(lp.name_of(class.class_config_name)).filter(|n| !n.eq_ignore_ascii_case("None")),
                    interfaces: class.interfaces.iter().filter_map(|(i, _)| self.ctx.resolve(c.pkg, *i)).collect(),
                    editor: class.editor.as_ref().map(|e| ParsedEditor {
                        hide_categories: names(&e.hide_categories),
                        auto_expand_categories: names(&e.auto_expand_categories),
                        auto_collapse_categories: names(&e.auto_collapse_categories),
                        dont_sort_categories: names(&e.dont_sort_categories),
                        class_group_names: names(&e.class_group_names),
                        force_script_order: e.force_script_order,
                        class_header_filename: e.class_header_filename.clone(),
                    }),
                })
            }
            _ => None,
        }
    }

    // ----- members -------------------------------------------------------

    fn const_decl(&self, c: ObjRef) -> String {
        match &self.ctx.object(c).map(|o| &o.body) {
            Some(ObjectBody::Const(d)) => format!("const {} = {};", self.ctx.name(c), d.value),
            _ => String::new(),
        }
    }

    fn enum_decl(&self, e: ObjRef) -> String {
        let names = self.ctx.enum_names(e);
        let ename = self.ctx.name(e);
        let mut body: Vec<String> = names.clone();
        // The compiler appends `<Prefix>_MAX` (prefix of the tags, else the enum name):
        // only that one is implied; any other trailing `_MAX` tag was spelled out.
        if body.len() > 1 {
            let mut prefix = crate::compiler::decl::generate_enum_prefix(&body[..body.len() - 1]);
            if prefix.is_empty() {
                prefix = ename.clone();
            }
            if body.last().map(|l| l.eq_ignore_ascii_case(&format!("{prefix}_MAX"))).unwrap_or(false) {
                body.pop();
            }
        }
        let mut s = format!("enum {ename}\n{{\n");
        for n in body {
            s.push_str(&format!("    {n},\n"));
        }
        s.push_str("};");
        s
    }

    /// Properties of this struct type carry CPF_NeedCtorLink although no member needs
    /// construction: the cooker removed an editor-only member that did (string, array...).
    fn struct_lost_ctor_member(&self, st: ObjRef, members: &[ObjRef]) -> bool {
        let has_ctor = members.iter().any(|&m| match self.ctx.property(m) {
            Some((k, d)) => matches!(k, PropertyKind::Array | PropertyKind::Str | PropertyKind::Delegate | PropertyKind::Map) || d.property_flags & cpf::NEED_CTOR_LINK != 0,
            None => false,
        });
        if has_ctor {
            return false;
        }
        let lp = &self.ctx.packages[st.pkg];
        let st_idx = PackageIndex::export(st.export);
        (0..lp.pkg.exports.len()).any(|e| {
            let r = ObjRef { pkg: st.pkg, export: e };
            match self.ctx.property(r) {
                Some((PropertyKind::Struct, d)) => d.ref1 == Some(st_idx) && d.property_flags & cpf::NEED_CTOR_LINK != 0,
                _ => false,
            }
        })
    }

    fn struct_decl(&self, st: ObjRef, scope_class: ObjRef, indent: usize) -> String {
        let pad = "    ".repeat(indent);
        let Some(ObjectBody::ScriptStruct { data, .. }) = self.ctx.object(st).map(|o| &o.body) else { return String::new() };
        let f = data.struct_flags;
        let parent = self.ctx.super_of(st);
        let parent_flags = parent
            .and_then(|p| match self.ctx.object(p).map(|o| &o.body) {
                Some(ObjectBody::ScriptStruct { data, .. }) => Some(data.struct_flags),
                _ => None,
            })
            .unwrap_or(0);
        let mut mods: Vec<&str> = Vec::new();
        if f & strct::NATIVE != 0 {
            mods.push("native");
        }
        if f & strct::EXPORT != 0 {
            mods.push("export");
        }
        if f & strct::TRANSIENT != 0 {
            mods.push("transient");
        }
        if f & strct::ATOMIC != 0 && parent_flags & strct::ATOMIC == 0 {
            mods.push("atomic");
        }
        if f & strct::IMMUTABLE != 0 {
            mods.push("immutable");
        }
        if f & strct::IMMUTABLE_WHEN_COOKED != 0 {
            mods.push("immutablewhencooked");
        }
        if f & strct::STRICT_CONFIG != 0 && parent_flags & strct::STRICT_CONFIG == 0 {
            mods.push("strictconfig");
        }
        let mut head = "struct".to_string();
        if !mods.is_empty() {
            head.push(' ');
            head.push_str(&mods.join(" "));
        }
        head.push(' ');
        head.push_str(&self.ctx.name(st));
        if let Some(p) = parent {
            // `extends` is resolved with `FindField` (scope chain only), so a parent
            // struct from an unrelated class must be qualified.
            head.push_str(&format!(" extends {}", self.scoped_type(p, scope_class)));
        }
        let mut s = format!("{pad}{head}\n{pad}{{\n");
        let children = self.ctx.children(st);
        let (before, vars, after) = declaration_order(self, &children, &|_| false);
        let ctor_stub = self.struct_lost_ctor_member(st, &children);
        let emit = |s: &mut String, ch: ObjRef| {
            let cn = self.ctx.class_name(ch);
            if cn == "ScriptStruct" {
                s.push_str(&self.struct_decl(ch, scope_class, indent + 1));
                s.push('\n');
            } else if cn == "Enum" {
                for l in self.enum_decl(ch).lines() {
                    s.push_str(&format!("{pad}    {l}\n"));
                }
            } else if cn == "Const" {
                s.push_str(&format!("{pad}    {}\n", self.const_decl(ch)));
            } else if self.ctx.property_kind(ch).is_some() {
                s.push_str(&format!("{pad}    {}\n", self.var_decl(ch, scope_class, true)));
            }
        };
        for ch in before.iter().chain(vars.iter()).chain(after.iter()) {
            emit(&mut s, *ch);
        }
        if ctor_stub {
            s.push_str(&format!("{pad}    var editoronly string {STRIPPED_STUB};\n"));
        }
        if !data.defaults.is_empty() {
            self.bin_defaults.set(true);
            let dl = self.default_lines(&data.defaults, Some(st), st.pkg, None);
            s.push_str(&format!("\n{pad}    structdefaultproperties\n{pad}    {{\n"));
            for l in dl.lines {
                s.push_str(&format!("{pad}        {l}\n"));
            }
            s.push_str(&format!("{pad}    }}\n"));
        }
        s.push_str(&format!("{pad}}};"));
        s
    }

    fn function_modifiers(&self, _f: ObjRef, flags: u32, native_index: u16) -> Vec<String> {
        let mut m: Vec<String> = Vec::new();
        if flags & func::PRIVATE != 0 {
            m.push("private".into());
        } else if flags & func::PROTECTED != 0 {
            m.push("protected".into());
        }
        if flags & func::NATIVE != 0 {
            if native_index != 0 {
                m.push(format!("native({native_index})"));
            } else {
                m.push("native".into());
            }
        }
        if flags & func::STATIC != 0 {
            m.push("static".into());
        }
        // `private` implies final (and interfaces reject an explicit `final`).
        if flags & func::FINAL != 0 && flags & func::PRIVATE == 0 {
            m.push("final".into());
        }
        if flags & func::SIMULATED != 0 {
            m.push("simulated".into());
        }
        if flags & func::EXEC != 0 {
            m.push("exec".into());
        }
        if flags & func::ITERATOR != 0 {
            m.push("iterator".into());
        }
        if flags & func::LATENT != 0 {
            m.push("latent".into());
        }
        if flags & func::SINGULAR != 0 {
            m.push("singular".into());
        }
        if flags & func::NET != 0 {
            m.push(if flags & func::NET_RELIABLE != 0 { "reliable".into() } else { "unreliable".into() });
            if flags & func::NET_SERVER != 0 {
                m.push("server".into());
            }
            if flags & func::NET_CLIENT != 0 {
                m.push("client".into());
            }
            if flags & (func::NET_SERVER | func::NET_CLIENT) == 0 {
                m.push("demorecording".into());
            }
        }
        if flags & func::DLL_IMPORT != 0 {
            m.push("dllimport".into());
        }
        if flags & func::PCOM_NET_APPROVED != 0 {
            m.push("pcomnetapproved".into());
        }
        if flags & func::PCOM_SERVER_CMD != 0 {
            m.push("pcomservercmd".into());
        }
        m
    }

    /// Full function declaration + body.
    pub fn function_decl(&self, f: ObjRef, state: Option<ObjRef>, indent: usize) -> String {
        let pad = "    ".repeat(indent);
        let Some(ObjectBody::Function { s: sdata, function }) = self.ctx.object(f).map(|o| &o.body) else { return String::new() };
        let flags = function.function_flags;
        let mut head = self.function_modifiers(f, flags, function.native_index);
        let kind = if flags & func::DELEGATE != 0 {
            "delegate".to_string()
        } else if flags & func::OPERATOR != 0 {
            if flags & func::PRE_OPERATOR != 0 {
                "preoperator".to_string()
            } else if self.ctx.function_params(f).len() == 1 {
                "postoperator".to_string()
            } else {
                format!("operator({})", function.oper_precedence)
            }
        } else if flags & func::EVENT != 0 {
            "event".to_string()
        } else {
            "function".to_string()
        };
        head.push(kind);
        let scope_class = self.class;
        if let Some(ret) = self.ctx.function_return(f) {
            if self.ctx.property(ret).map(|(_, d)| d.property_flags & cpf::COERCE_PARM != 0).unwrap_or(false) {
                head.push("coerce".into());
            }
            head.push(self.type_name(ret, scope_class));
        }
        let fname = self.fname(function.friendly_name);
        let name_text = if fname.is_empty() || fname.eq_ignore_ascii_case("None") { self.ctx.name(f) } else { fname };

        // Default parameter values live in the bytecode prologue.
        let none = self.ctx.packages[f.pkg].none_name;
        let script = if sdata.script.is_empty() { None } else { Disassembler::disassemble(&sdata.script, Some(none)).ok() };
        let scope = Scope { class: scope_class, state, function: Some(f) };
        // The prologue holds one entry per optional parameter: a DefaultParmValue or a Nothing.
        let mut defaults: Vec<Option<String>> = Vec::new();
        if let Some(sc) = &script {
            for st in &sc.statements {
                match &st.token {
                    Token::DefaultParmValue { expr, .. } => defaults.push(Some(self.expr_text(self.strip_autocast(expr), Hint::default(), scope))),
                    Token::Nothing => defaults.push(None),
                    _ => break,
                }
            }
        }
        let params = self.ctx.function_params(f);
        let mut di = 0;
        let mut ptexts = Vec::new();
        for p in &params {
            let is_opt = self.ctx.property(*p).map(|(_, d)| d.property_flags & cpf::OPTIONAL_PARM != 0).unwrap_or(false);
            let dv = if is_opt && di < defaults.len() {
                let v = defaults[di].clone();
                di += 1;
                v
            } else {
                None
            };
            // Re-hint enum defaults.
            let dv = dv.map(|v| {
                if let Some(e) = self.ctx.enum_of_property(*p) {
                    if let Ok(n) = v.parse::<u8>() {
                        if let Some(tag) = self.ctx.enum_names(e).get(n as usize) {
                            return tag.clone();
                        }
                    }
                }
                v
            });
            ptexts.push(self.param_decl(*p, scope_class, dv.as_deref()));
        }
        let mut sig = format!("{} {}({})", head.join(" "), name_text, ptexts.join(", "));
        if flags & func::CONST != 0 {
            sig.push_str(" const");
        }
        // Native functions and bodiless prototypes (`function X();`, not FUNC_Defined).
        if script.is_none() || flags & func::NATIVE != 0 || flags & func::DEFINED == 0 {
            return format!("{pad}{sig};");
        }
        let mut s = format!("{pad}{sig}\n{pad}{{\n");
        let locals = self.ctx.function_locals(f);
        for l in &locals {
            let Some((_, d)) = self.ctx.property(*l) else { continue };
            let mut name = self.ctx.name(*l);
            if d.array_dim > 1 {
                name = format!("{name}[{}]", d.array_dim);
            }
            s.push_str(&format!("{pad}    local {} {};\n", self.type_name(*l, scope_class), name));
        }
        if !locals.is_empty() {
            s.push('\n');
        }
        let ret_hint = Hint { enum_: self.ctx.function_return(f).and_then(|r| self.ctx.enum_of_property(r)), bool_ctx: false };
        let body = Flow::new(self, scope, script.as_ref().unwrap(), BTreeMap::new(), ret_hint).render(indent + 1);
        if body.trim().is_empty() && locals.is_empty() && flags & func::DEFINED != 0 {
            // A body that was not `{}` in the source (FUNC_Defined): keep a statement so it stays defined.
            s.push_str(&format!("{pad}    ;\n"));
        }
        s.push_str(&body);
        s.push_str(&format!("{pad}}}"));
        s
    }

    pub fn state_decl(&self, st: ObjRef) -> String {
        let Some(ObjectBody::State { s: sdata, state }) = self.ctx.object(st).map(|o| &o.body) else { return String::new() };
        let mut head: Vec<String> = Vec::new();
        if state.state_flags & sf::AUTO != 0 {
            head.push("auto".into());
        }
        if state.state_flags & sf::SIMULATED != 0 {
            head.push("simulated".into());
        }
        head.push(if state.state_flags & sf::EDITABLE != 0 { "state()".into() } else { "state".into() });
        head.push(self.ctx.name(st));
        if let Some(sup) = self.ctx.super_of(st) {
            // A state overriding a parent class' state of the same name gets its super
            // implicitly; `extends` is only written for a differently named base state.
            if !self.ctx.name(sup).eq_ignore_ascii_case(&self.ctx.name(st)) {
                head.push(format!("extends {}", self.ctx.name(sup)));
            }
        }
        let mut s = format!("{}\n{{\n", head.join(" "));
        // State variables (`var` inside a state) come first; the chain keeps their order.
        let state_vars: Vec<ObjRef> = self.ctx.children(st).into_iter().filter(|c| self.ctx.property_kind(*c).is_some()).collect();
        for v in &state_vars {
            s.push_str(&format!("    {}\n", self.var_decl(*v, self.class, false)));
        }
        if !state_vars.is_empty() {
            s.push('\n');
        }
        let mut children: Vec<ObjRef> = self.ctx.children(st).into_iter().filter(|c| self.ctx.class_name(*c) == "Function").collect();
        children.reverse();
        let mut pending_ignores: Vec<String> = Vec::new();
        for ch in &children {
            let is_ignore = match self.ctx.object(*ch).map(|o| &o.body) {
                // `ignores` stubs: never `FUNC_Defined`, and their super is the ignored
                // function (a bodiless prototype `function X();` has neither).
                Some(ObjectBody::Function { function, s: fs }) => {
                    // (`ignores` binds through the class chain only; a super inside a state
                    // means a bodiless prototype overriding the super state's function.)
                    let class_level_super = self
                        .ctx
                        .resolve(ch.pkg, fs.super_struct)
                        .and_then(|sup| self.ctx.outer_of(sup))
                        .map(|o| self.ctx.class_name(o) == "Class")
                        .unwrap_or(false);
                    function.function_flags & (func::DEFINED | func::NATIVE) == 0
                        && class_level_super
                        && function.function_flags & !(func::FUNC_OVERRIDE_MATCH | func::HAS_OPTIONAL_PARMS | func::HAS_OUT_PARMS) == 0
                }
                _ => false,
            };
            if is_ignore {
                pending_ignores.push(self.ctx.name(*ch));
                continue;
            }
            if !pending_ignores.is_empty() {
                s.push_str(&format!("    ignores {};\n\n", pending_ignores.join(", ")));
                pending_ignores.clear();
            }
            s.push_str(&self.function_decl(*ch, Some(st), 1));
            s.push_str("\n\n");
        }
        if !pending_ignores.is_empty() {
            s.push_str(&format!("    ignores {};\n\n", pending_ignores.join(", ")));
        }
        if !sdata.script.is_empty() {
            let none = self.ctx.packages[st.pkg].none_name;
            if let Ok(script) = Disassembler::disassemble(&sdata.script, Some(none)) {
                let mut labels = BTreeMap::new();
                for stmt in &script.statements {
                    if let Token::LabelTable(entries) = &stmt.token {
                        for e in entries {
                            let n = self.ctx.packages[st.pkg].name_of(e.name);
                            if !n.eq_ignore_ascii_case("None") {
                                labels.insert(e.offset, n);
                            }
                        }
                    }
                }
                let scope = Scope { class: self.class, state: Some(st), function: None };
                let body = Flow::new(self, scope, &script, labels, Hint::default()).render(1);
                s.push_str(&body);
            }
        }
        s.push('}');
        s
    }

    fn replication_block(&self) -> String {
        let Some(sdata) = self.ctx.struct_data(self.class) else { return String::new() };
        if sdata.script.is_empty() {
            return String::new();
        }
        let none = self.ctx.packages[self.pkg].none_name;
        let Ok(script) = Disassembler::disassemble(&sdata.script, Some(none)) else { return String::new() };
        let mut groups: BTreeMap<u16, Vec<String>> = BTreeMap::new();
        for ch in self.ctx.children(self.class) {
            if let Some((_, d)) = self.ctx.property(ch) {
                if d.property_flags & cpf::NET != 0 {
                    if let Some(off) = d.rep_offset {
                        groups.entry(off).or_default().push(self.ctx.name(ch));
                    }
                }
            }
        }
        if groups.is_empty() {
            return String::new();
        }
        let scope = Scope { class: self.class, state: None, function: None };
        let mut s = String::from("replication\n{\n");
        for (off, names) in groups {
            let cond = script
                .statements
                .iter()
                .find(|st| st.mem_offset == off as u32)
                .map(|st| self.expr_text(&st.token, Hint { enum_: None, bool_ctx: true }, scope))
                .unwrap_or_else(|| "true".into());
            s.push_str(&format!("    if ({cond})\n        {};\n", names.join(", ")));
        }
        s.push('}');
        s
    }

    fn defaultproperties_block(&self) -> String {
        let Some(ObjectBody::Class { class, .. }) = self.ctx.object(self.class).map(|o| &o.body) else { return String::new() };
        let Some(cdo) = self.ctx.resolve(self.pkg, class.class_default_object) else { return String::new() };
        let Some(cdo_obj) = self.ctx.object(cdo) else { return String::new() };
        let dl = self.default_lines_of(&cdo_obj.header.properties, Some(self.class), cdo.pkg, Some(cdo), Some(cdo));
        let mut s = String::from("defaultproperties\n{\n");
        // Subobjects: every export whose outer is the CDO, in export order.
        let mut subs: Vec<ObjRef> = Vec::new();
        let lp = &self.ctx.packages[cdo.pkg];
        for (i, e) in lp.pkg.exports.iter().enumerate() {
            if e.outer == PackageIndex::export(cdo.export) {
                subs.push(ObjRef { pkg: cdo.pkg, export: i });
            }
        }
        // Components the compiler duplicates from the parent class on its own
        // (`CopyInheritedComponents`): not part of the source.
        let parent_cdo = self.ctx.super_of(self.class).and_then(|p| self.ctx.class_default_object(p));
        let mut skipped: Vec<String> = Vec::new();
        let mut blocks: Vec<(String, String)> = Vec::new();
        for sub in &subs {
            match self.subobject_block_or_inherited(*sub, cdo, parent_cdo, 1) {
                Some(block) => blocks.push((self.ctx.name(*sub), block)),
                None => skipped.push(self.ctx.name(*sub)),
            }
        }
        // A sub-object named inside another block's values must be declared first.
        for block in order_subobject_blocks(blocks) {
            s.push_str(&block);
        }
        // Inherited components no longer in this class's map were taken out with
        // `Components.Remove`.
        for name in self.removed_component_names(&class.component_name_to_default_object, parent_cdo) {
            s.push_str(&format!("    Components.Remove({name})\n"));
        }
        let ancestor_lines = if skipped.is_empty() { Vec::new() } else { self.ancestor_default_lines() };
        for l in dl.lines {
            if !skipped.is_empty() {
                if let Some((_, value)) = l.split_once('=') {
                    if skipped.iter().any(|n| n.eq_ignore_ascii_case(value)) && ancestor_lines.iter().any(|a| a.eq_ignore_ascii_case(&l)) {
                        continue;
                    }
                }
            }
            s.push_str(&format!("    {l}\n"));
        }
        s.push('}');
        s
    }

    /// Names in the parent default object's `Components` array that this class's
    /// component map no longer has.
    fn removed_component_names(&self, map: &[(FName, PackageIndex)], parent_cdo: Option<ObjRef>) -> Vec<String> {
        let Some(pc) = parent_cdo else { return Vec::new() };
        let Some(po) = self.ctx.object(pc) else { return Vec::new() };
        let lines = self.default_lines_of(&po.header.properties, self.ctx.class_of(pc), pc.pkg, Some(pc), Some(pc)).lines;
        let map_names: Vec<String> = map.iter().map(|(n, _)| self.pkgname_in(self.pkg, *n)).collect();
        let mut out: Vec<String> = Vec::new();
        for l in lines {
            let Some(rest) = l.strip_prefix("Components(") else { continue };
            let Some((_, v)) = rest.split_once('=') else { continue };
            let v = v.trim();
            if v.eq_ignore_ascii_case("None") || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            if !map_names.iter().any(|m| m.eq_ignore_ascii_case(v)) && !out.iter().any(|o| o.eq_ignore_ascii_case(v)) {
                out.push(v.to_string());
            }
        }
        out
    }

    /// `Name=Value` lines of every ancestor class default object.
    fn ancestor_default_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = self.ctx.super_of(self.class);
        let mut guard = 0;
        while let Some(c) = cur {
            if let Some(cdo) = self.ctx.class_default_object(c) {
                if let Some(o) = self.ctx.object(cdo) {
                    out.extend(self.default_lines_of(&o.header.properties, Some(c), cdo.pkg, Some(cdo), Some(cdo)).lines);
                }
            }
            cur = self.ctx.super_of(c);
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        out
    }

    fn archetype_of(&self, r: ObjRef) -> Option<ObjRef> {
        let a = self.ctx.packages[r.pkg].pkg.exports[r.export].archetype;
        self.ctx.resolve(r.pkg, a)
    }

    /// A component the class inherited from its parent (same name, archetype is the
    /// parent CDO's component, no template owner).
    fn is_inherited_component(&self, sub: ObjRef, from: Option<ObjRef>) -> bool {
        let Some(pc) = from else { return false };
        let Some(a) = self.archetype_of(sub) else { return false };
        if self.ctx.outer_of(a) != Some(pc) || !self.ctx.name(a).eq_ignore_ascii_case(&self.ctx.name(sub)) {
            return false;
        }
        match self.ctx.object(sub).and_then(|o| o.header.component.as_ref()) {
            Some(c) => c.template_owner_class.is_null(),
            None => true,
        }
    }

    /// Lines of `sub` that the source must have set: everything except cross-level
    /// references the compiler always writes and whose value matches the archetype.
    fn subobject_source_lines(&self, sub: ObjRef, cdo: ObjRef) -> Vec<String> {
        let Some(o) = self.ctx.object(sub) else { return Vec::new() };
        let owner = self.ctx.class_of(sub);
        let lines = self.default_lines_of(&o.header.properties, owner, sub.pkg, Some(cdo), Some(sub)).lines;
        let Some(a) = self.archetype_of(sub) else { return lines };
        let arch_lines: Vec<String> = match self.ctx.object(a) {
            Some(ao) => self.default_lines_of(&ao.header.properties, self.ctx.class_of(a), a.pkg, None, Some(a)).lines,
            None => Vec::new(),
        };
        lines
            .into_iter()
            .filter(|l| {
                let Some((key, value)) = l.split_once('=') else { return true };
                let pname = key.split('(').next().unwrap_or(key);
                let crosslevel = owner
                    .and_then(|c| self.ctx.find_field(c, pname))
                    .and_then(|p| self.ctx.property(p))
                    .map(|(_, d)| d.property_flags & (cpf::CROSS_LEVEL_ACTIVE | cpf::CROSS_LEVEL_PASSIVE) != 0)
                    .unwrap_or(false);
                if !crosslevel {
                    return true;
                }
                let same = arch_lines.iter().any(|x| x.eq_ignore_ascii_case(l));
                let arch_has = arch_lines.iter().any(|x| x.split_once('=').map(|(k, _)| k.eq_ignore_ascii_case(key)).unwrap_or(false));
                !(same || (!arch_has && value.eq_ignore_ascii_case("None")))
            })
            .collect()
    }

    /// `Begin Object` block for a CDO subobject, or `None` when it is a pure copy
    /// of an inherited component that the compiler recreates by itself.
    fn subobject_block_or_inherited(&self, sub: ObjRef, cdo: ObjRef, parent_cdo: Option<ObjRef>, indent: usize) -> Option<String> {
        if !self.is_inherited_component(sub, parent_cdo) {
            return Some(self.subobject_block(sub, cdo, indent));
        }
        let pad = "    ".repeat(indent);
        let lines = self.subobject_source_lines(sub, cdo);
        let lp = &self.ctx.packages[sub.pkg];
        let nested: Vec<ObjRef> = lp.pkg.exports.iter().enumerate().filter(|(_, e)| e.outer == PackageIndex::export(sub.export)).map(|(i, _)| ObjRef { pkg: sub.pkg, export: i }).collect();
        if lines.is_empty() && nested.is_empty() {
            return None;
        }
        let template = self.archetype_of(sub);
        let mut blocks = String::new();
        let mut skipped: Vec<String> = Vec::new();
        for n in nested {
            match self.subobject_block_or_inherited(n, cdo, template, indent + 1) {
                Some(b) => blocks.push_str(&b),
                None => skipped.push(self.ctx.name(n)),
            }
        }
        let template_lines: Vec<String> = match template.and_then(|t| self.ctx.object(t)) {
            Some(to) if !skipped.is_empty() => self.default_lines_of(&to.header.properties, template.and_then(|t| self.ctx.class_of(t)), template.unwrap().pkg, template, template).lines,
            _ => Vec::new(),
        };
        let lines: Vec<String> = lines
            .into_iter()
            .filter(|l| match l.split_once('=') {
                Some((_, value)) => !(skipped.iter().any(|n| n.eq_ignore_ascii_case(value)) && template_lines.iter().any(|a| a.eq_ignore_ascii_case(l))),
                None => true,
            })
            .collect();
        if lines.is_empty() && blocks.is_empty() {
            return None;
        }
        let mut s = format!("{pad}Begin Object Name={}\n", self.ctx.name(sub));
        s.push_str(&blocks);
        for l in lines {
            s.push_str(&format!("{pad}    {l}\n"));
        }
        s.push_str(&format!("{pad}End Object\n"));
        Some(s)
    }

    fn subobject_block(&self, sub: ObjRef, cdo: ObjRef, indent: usize) -> String {
        let pad = "    ".repeat(indent);
        let class = self.ctx.class_of(sub).map(|c| self.ctx.name(c)).unwrap_or_else(|| self.ctx.class_name(sub));
        let mut s = format!("{pad}Begin Object Class={class} Name={}", self.ctx.name(sub));
        // Instanced from another class's template (not its class defaults): say so.
        if let Some(a) = self.archetype_of(sub) {
            if self.ctx.object_flags(a) & rf::CLASS_DEFAULT_OBJECT == 0 {
                let ac = self.ctx.class_of(a).map(|c| self.ctx.name(c)).unwrap_or_else(|| self.ctx.class_name(a));
                s.push_str(&format!(" Archetype={ac}'{}'", self.ctx.full_path(a)));
            }
        }
        s.push('\n');
        if let Some(o) = self.ctx.object(sub) {
            let owner = self.ctx.class_of(sub);
            let dl = self.default_lines_of(&o.header.properties, owner, sub.pkg, Some(cdo), Some(sub));
            // Nested sub-objects: those instanced from the template (its own sub-objects)
            // are recreated by the compiler and only written when changed.
            let template = self.archetype_of(sub).or_else(|| owner.and_then(|c| self.ctx.class_default_object(c)));
            let lp = &self.ctx.packages[sub.pkg];
            let mut skipped: Vec<String> = Vec::new();
            for (i, e) in lp.pkg.exports.iter().enumerate() {
                if e.outer == PackageIndex::export(sub.export) {
                    let n = ObjRef { pkg: sub.pkg, export: i };
                    match self.subobject_block_or_inherited(n, cdo, template, indent + 1) {
                        Some(block) => s.push_str(&block),
                        None => skipped.push(self.ctx.name(n)),
                    }
                }
            }
            let template_lines: Vec<String> = match template.and_then(|t| self.ctx.object(t)) {
                Some(to) if !skipped.is_empty() => self.default_lines_of(&to.header.properties, template.and_then(|t| self.ctx.class_of(t)), template.unwrap().pkg, template, template).lines,
                _ => Vec::new(),
            };
            for l in dl.lines {
                if let Some((_, value)) = l.split_once('=') {
                    if skipped.iter().any(|n| n.eq_ignore_ascii_case(value)) && template_lines.iter().any(|a| a.eq_ignore_ascii_case(&l)) {
                        continue;
                    }
                }
                s.push_str(&format!("{pad}    {l}\n"));
            }
            if let ObjectBody::Other { rest } = &o.body {
                if !rest.is_empty() {
                    s.push_str(&format!("{pad}    // {} bytes of native data\n", rest.len()));
                }
            }
        }
        s.push_str(&format!("{pad}End Object\n"));
        s
    }
}

pub struct ParsedEditor {
    pub hide_categories: Vec<String>,
    pub auto_expand_categories: Vec<String>,
    pub auto_collapse_categories: Vec<String>,
    pub dont_sort_categories: Vec<String>,
    pub class_group_names: Vec<String>,
    pub force_script_order: u32,
    pub class_header_filename: String,
}

pub struct ParsedClass {
    pub flags: u32,
    pub within: Option<ObjRef>,
    pub config: Option<String>,
    pub interfaces: Vec<ObjRef>,
    pub editor: Option<ParsedEditor>,
}

/// Reconstruct a declaration order that the compiler turns back into the
/// same children list: non-property fields are prepended as declared, the
/// first property is prepended too (no property to append after yet) and
/// later properties are appended after the last property.
///
/// Returns `(before_vars, vars, after_vars)`.
/// Name of the editor-only stand-in property the cooker is assumed to have stripped.
pub const STRIPPED_STUB: &str = "EditorOnlyStripped";

pub fn declaration_order(d: &Decompiler, children: &[ObjRef], is_generated: &dyn Fn(ObjRef) -> bool) -> (Vec<ObjRef>, Vec<ObjRef>, Vec<ObjRef>) {
    declaration_order_ex(d, children, is_generated).0
}

/// `declaration_order` plus the index in `before` at which an editor-only stand-in
/// property must be declared: the cooked package lost the real first property, and
/// types the remaining properties use were declared after it (a chain only
/// reproducible with a property in that spot).
pub fn declaration_order_ex(d: &Decompiler, children: &[ObjRef], is_generated: &dyn Fn(ObjRef) -> bool) -> ((Vec<ObjRef>, Vec<ObjRef>, Vec<ObjRef>), Option<usize>) {
    let is_prop = |c: ObjRef| d.ctx.property_kind(c).is_some();
    let is_code = |c: ObjRef| matches!(d.ctx.class_name(c).as_str(), "Function" | "State");
    let first_prop = children.iter().position(|c| is_prop(*c));
    let Some(p1) = first_prop else {
        let mut all: Vec<ObjRef> = children.to_vec();
        all.reverse();
        let split = all.iter().position(|c| is_code(*c)).unwrap_or(all.len());
        let after = all.split_off(split);
        return ((all, Vec::new(), after), None);
    };
    let last_prop = children.iter().rposition(|c| is_prop(*c)).unwrap_or(p1);
    let mut head: Vec<ObjRef> = children[..p1].iter().copied().filter(|c| !is_prop(*c)).collect();
    head.extend(children[p1..=last_prop].iter().copied().filter(|c| !is_prop(*c)));
    head.reverse();
    let mut tail: Vec<ObjRef> = children[last_prop + 1..].iter().copied().filter(|c| !is_prop(*c)).collect();
    tail.reverse();
    let props: Vec<ObjRef> = children.iter().copied().filter(|c| is_prop(*c) && !is_generated(*c)).collect();
    let tail_len = tail.len();
    let mut before: Vec<ObjRef> = tail;
    let mut vars: Vec<ObjRef> = Vec::new();
    let first_generated = is_generated(children[p1]);
    if !first_generated {
        before.push(children[p1]);
        vars.extend(props.iter().copied().filter(|c| *c != children[p1]));
    } else {
        vars.extend(props);
    }
    let split = head.iter().position(|c| is_code(*c)).unwrap_or(head.len());
    let mut after = head.split_off(split);
    before.extend(head);
    // Cooked packages lose editor-only properties: when the first remaining property
    // uses a type that was declared after the real first property, the exact
    // declaration layout cannot be reproduced; fall back to declaring types first.
    let type_refs = |p: ObjRef| -> Vec<ObjRef> {
        let mut out = Vec::new();
        let mut cur = Some(p);
        while let Some(x) = cur {
            let Some((k, data)) = d.ctx.property(x) else { break };
            match k {
                PropertyKind::Array => cur = data.ref1.and_then(|r| d.ctx.resolve(x.pkg, r)),
                PropertyKind::Struct | PropertyKind::Byte => {
                    if let Some(t) = data.ref1.and_then(|r| d.ctx.resolve(x.pkg, r)) {
                        out.push(t);
                    }
                    cur = None;
                }
                _ => cur = None,
            }
        }
        out
    };
    let mut stub_at: Option<usize> = None;
    if !first_generated {
        let p1 = children[p1];
        if let Some(pos) = before.iter().position(|c| *c == p1) {
            let needs = type_refs(p1);
            let later = before[pos + 1..].iter().any(|c| needs.contains(c)) || after.iter().any(|c| needs.contains(c));
            if later {
                // The real first property was stripped: a stand-in takes its place so the
                // types declared after it still land ahead of it in the field chain.
                before.remove(pos);
                vars.insert(0, p1);
                stub_at = Some(tail_len);
            }
        }
    }
    let needed: Vec<ObjRef> = vars.iter().flat_map(|v| type_refs(*v)).collect();
    let (moved, rest): (Vec<ObjRef>, Vec<ObjRef>) = after.iter().partition(|c| !is_code(**c) && needed.contains(c));
    if !moved.is_empty() {
        before.extend(moved);
        after = rest;
    }
    ((before, vars, after), stub_at)
}

pub fn class_source(d: &Decompiler) -> String {
    let c = d.class;
    let mut s = String::new();
    let children = d.ctx.children(c);
    let is_generated = |ch: ObjRef| {
        let name = d.ctx.name(ch);
        (name.starts_with("__") && name.ends_with("__Delegate")) || name.starts_with("VfTable_")
    };
    let ((before, vars, after), stub_at) = declaration_order_ex(d, &children, &is_generated);
    let emit_field = |s: &mut String, f: ObjRef| match d.ctx.class_name(f).as_str() {
        "Const" => {
            s.push_str(&d.const_decl(f));
            s.push_str("\n\n");
        }
        "Enum" => {
            s.push_str(&d.enum_decl(f));
            s.push_str("\n\n");
        }
        "ScriptStruct" => {
            s.push_str(&d.struct_decl(f, c, 0));
            s.push_str("\n\n");
        }
        "Function" => {
            s.push_str(&d.function_decl(f, None, 0));
            s.push_str("\n\n");
        }
        "State" => {
            s.push_str(&d.state_decl(f));
            s.push_str("\n\n");
        }
        _ => {
            if d.ctx.property_kind(f).is_some() {
                s.push_str(&d.var_decl(f, c, false));
                s.push_str("\n\n");
            }
        }
    };
    for (i, f) in before.iter().enumerate() {
        if stub_at == Some(i) {
            s.push_str(&format!("var editoronly int {STRIPPED_STUB};\n\n"));
        }
        emit_field(&mut s, *f);
    }
    if stub_at == Some(before.len()) {
        s.push_str(&format!("var editoronly int {STRIPPED_STUB};\n\n"));
    }
    for v in &vars {
        s.push_str(&d.var_decl(*v, c, false));
        s.push('\n');
    }
    if !vars.is_empty() {
        s.push('\n');
    }
    let rep = d.replication_block();
    // Functions and states first (into their own buffer): rendering them discovers the
    // cooked-out variables their bytecode still refers to, declared with the vars.
    let mut tail = String::new();
    for f in &after {
        emit_field(&mut tail, *f);
    }
    tail.push_str(&d.defaultproperties_block());
    tail.push('\n');
    let stripped = d.stripped_refs.borrow();
    for (ty, name) in stripped.iter() {
        s.push_str(&format!("var editoronly {ty} {name};\n"));
    }
    if !stripped.is_empty() {
        s.push('\n');
    }
    drop(stripped);
    if !rep.is_empty() {
        s.push_str(&rep);
        s.push_str("\n\n");
    }
    s.push_str(&tail);
    let mut out = d.class_header();
    out.push_str("\n\n");
    out.push_str(&s);
    out
}

/// Sub-object blocks in dependency order: a block whose values name a sibling
/// comes after that sibling (stable otherwise; cycles keep the original order).
fn order_subobject_blocks(blocks: Vec<(String, String)>) -> Vec<String> {
    let names: Vec<String> = blocks.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
    let deps: Vec<Vec<usize>> = blocks
        .iter()
        .enumerate()
        .map(|(i, (_, text))| {
            let mut words: Vec<String> = Vec::new();
            for line in text.lines() {
                let t = line.trim_start();
                if t.starts_with("Begin Object") || t.starts_with("End Object") {
                    continue;
                }
                let Some((_, value)) = t.split_once('=') else { continue };
                let mut word = String::new();
                for c in value.chars() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        word.push(c.to_ascii_lowercase());
                    } else if !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                }
                if !word.is_empty() {
                    words.push(word);
                }
            }
            names.iter().enumerate().filter(|(j, n)| *j != i && words.iter().any(|w| w == *n)).map(|(j, _)| j).collect()
        })
        .collect();
    let mut out = Vec::new();
    let mut done = vec![false; blocks.len()];
    loop {
        let mut progressed = false;
        for i in 0..blocks.len() {
            if !done[i] && deps[i].iter().all(|&j| done[j]) {
                done[i] = true;
                out.push(blocks[i].1.clone());
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    for i in 0..blocks.len() {
        if !done[i] {
            out.push(blocks[i].1.clone());
        }
    }
    out
}
