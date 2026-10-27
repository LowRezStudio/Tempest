//! Package deltas: what a modded package changed relative to the game's own
//! file, as a `.tdelta` the launcher applies over whatever the file is when
//! the mod is enabled. Mods then ship a few kilobytes instead of whole
//! packages, and two mods editing different objects of the same package
//! compose.
//!
//! An object is addressed by its path (`Outer.Inner.Name`), never by its
//! export index: indices shift when two mods add exports. The delta is a
//! small package of its own (the "mini package"): its exports are the added
//! and changed objects with their payloads, its imports are every object
//! those payloads refer to, both the real imports of the edited package and
//! the exports of the base package the delta does not carry (resolved by
//! path at apply time). A JSON header describes the operations, the kind of
//! every reference and the hashes the applier checks for drift. Payloads
//! are remapped through [`crate::visit`], the same walk the editor's
//! cross-package copy uses, so names and references survive the move.
//!
//! What is compared is a *canonical* form of each export, independent of
//! the package's tables: names as text, references as `class|path`
//! descriptors, in order of first appearance, plus the written payload.
//! A recompiled script package therefore only differs where its sources did.
//!
//! File layout: `TDLT`, u32 version, u32 json length, json, u32 image
//! length, image (the mini package, uncompressed).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use md5::{Digest as _, Md5};
use serde_json::{json, Value};
use tempest_unrealscript::objects::Layout;
use tempest_upk::io::{Error, FName, PackageIndex, Reader, Result, Writer};
use tempest_upk::{ObjectExport, Package, WriteOptions};

use crate::edit::Editor;
use crate::object::{is_script_class, none_name, package_layout, parse_export, write_parsed, Parsed};
use crate::reflect::ClassDb;
use crate::visit::{visit_parsed, Visitor};

pub const MAGIC: &[u8; 4] = b"TDLT";
pub const VERSION: u32 = 1;

/// A delta: its JSON header and the mini package image.
#[derive(Clone, Debug)]
pub struct Delta {
    pub header: Value,
    pub image: Vec<u8>,
}

/// One operation of the header, as the launcher and the Studio see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op {
    /// `add`, `set` or `delete`.
    pub op: String,
    pub path: String,
    pub class: String,
    /// Canonical hash of the base object a `set` replaces (hex), when known.
    pub base: Option<String>,
    /// Export of the mini package carrying the payload (`add` / `set`).
    pub export: Option<usize>,
}

impl Delta {
    pub fn kind(&self) -> &str {
        self.header["kind"].as_str().unwrap_or("")
    }

    /// Game-relative path of the file the delta applies to.
    pub fn target(&self) -> &str {
        self.header["target"].as_str().unwrap_or("")
    }

    pub fn ops(&self) -> Vec<Op> {
        self.header["ops"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|o| Op {
                        op: o["op"].as_str().unwrap_or("").to_string(),
                        path: o["path"].as_str().unwrap_or("").to_string(),
                        class: o["class"].as_str().unwrap_or("").to_string(),
                        base: o["base"].as_str().map(str::to_string),
                        export: o["export"].as_u64().map(|v| v as usize),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn layout(&self) -> Layout {
        Layout { editor_data: self.header["layout"]["editor_data"].as_bool().unwrap_or(true) }
    }

    /// Classes the delta adds that the applier's class database may not know.
    pub fn added_classes(&self) -> Vec<String> {
        self.ops().into_iter().filter(|o| o.op == "add" && o.class.eq_ignore_ascii_case("Class")).map(|o| o.path.rsplit('.').next().unwrap_or("").to_string()).collect()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let json = serde_json::to_vec(&self.header).unwrap_or_default();
        let mut w = Writer::new();
        w.bytes(MAGIC);
        w.u32(VERSION);
        w.u32(json.len() as u32);
        w.bytes(&json);
        w.u32(self.image.len() as u32);
        w.bytes(&self.image);
        w.into_inner()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Delta> {
        let mut r = Reader::new(bytes);
        if r.take(4)? != MAGIC {
            return Err(Error::Format("not a Tempest delta (bad magic)".into()));
        }
        let version = r.u32()?;
        if version != VERSION {
            return Err(Error::Format(format!("delta version {version} is not supported (this build reads {VERSION})")));
        }
        let n = r.u32()? as usize;
        let header: Value = serde_json::from_slice(r.take(n)?).map_err(|e| Error::Format(format!("delta header: {e}")))?;
        let n = r.u32()? as usize;
        let image = r.take(n)?.to_vec();
        Ok(Delta { header, image })
    }

    pub fn read(path: impl AsRef<Path>) -> Result<Delta> {
        Delta::from_bytes(&std::fs::read(path)?)
    }

    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        std::fs::write(path, self.to_bytes())?;
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// Canonical form
// ----------------------------------------------------------------------------

/// Table-independent description of a reference: `I|class package|class|path`
/// for an import, `E|class|path` for an export, empty for `None`.
fn describe(pkg: &Package, idx: PackageIndex) -> String {
    if idx.is_null() {
        return String::new();
    }
    let s = if let Some(i) = idx.import_index() {
        let imp = &pkg.imports[i];
        format!("I|{}|{}|{}", pkg.fname_to_string(imp.class_package), pkg.fname_to_string(imp.class_name), pkg.object_path(idx))
    } else {
        format!("E|{}|{}", pkg.class_name_of(idx), pkg.object_path(idx))
    };
    s.to_ascii_lowercase()
}

/// The cooker's zero-size, class-less placeholder (also what `obj delete` leaves).
fn is_placeholder(pkg: &Package, i: usize) -> bool {
    let e = &pkg.exports[i];
    e.class.is_null() && pkg.export_data.get(i).is_none_or(|d| d.is_empty()) && pkg.fname_to_string(e.object_name).eq_ignore_ascii_case("None")
}

/// Renames every name and reference of an object to the order of its first
/// appearance, recording what they were; the written result then depends
/// only on the object.
struct Canon<'a> {
    pkg: &'a Package,
    names: Vec<String>,
    refs: Vec<String>,
    name_map: HashMap<u32, u32>,
    ref_map: HashMap<PackageIndex, PackageIndex>,
}

impl<'a> Canon<'a> {
    fn new(pkg: &'a Package) -> Canon<'a> {
        let none = none_name(pkg);
        Canon { pkg, names: vec!["None".to_string()], refs: Vec::new(), name_map: HashMap::from([(none.index, 0)]), ref_map: HashMap::new() }
    }
}

impl Visitor for Canon<'_> {
    fn name(&mut self, n: &mut FName) {
        let next = self.names.len() as u32;
        let idx = *self.name_map.entry(n.index).or_insert_with(|| {
            self.names.push(self.pkg.name_str(n.index).to_ascii_lowercase());
            next
        });
        n.index = idx;
    }
    fn index(&mut self, i: &mut PackageIndex) {
        if i.is_null() {
            return;
        }
        let next = PackageIndex::export(self.refs.len());
        let m = *self.ref_map.entry(*i).or_insert_with(|| {
            self.refs.push(describe(self.pkg, *i));
            next
        });
        *i = m;
    }
}

/// Canonical hash of export `i`: `None` for a placeholder.
pub fn canonical(db: &ClassDb, pkg: &Package, layout: Layout, i: usize) -> Option<[u8; 16]> {
    if is_placeholder(pkg, i) {
        return None;
    }
    let e = &pkg.exports[i];
    let mut h = Md5::new();
    // A null class means `UClass`; the compiler writes the same thing as an
    // explicit `Core.Class` import.
    let class_desc = describe(pkg, e.class);
    h.update(if e.class.is_null() || class_desc == "i|core|class|core.class" { "class" } else { class_desc.as_str() });
    h.update(b"\0");
    for r in [e.super_, e.archetype] {
        h.update(describe(pkg, r));
        h.update(b"\0");
    }
    h.update(e.object_flags.to_le_bytes());
    h.update(e.export_flags.to_le_bytes());
    h.update(e.package_flags.to_le_bytes());
    for c in &e.generation_net_object_count {
        h.update(c.to_le_bytes());
    }
    let data = pkg.export_data.get(i).map(|d| d.as_slice()).unwrap_or(&[]);
    let mut p = parse_export(db, pkg, layout, i, data);
    if let Parsed::Raw { data, .. } = &p {
        h.update(b"raw\0");
        h.update(data);
        return Some(h.finalize().into());
    }
    let mut c = Canon::new(pkg);
    let none = none_name(pkg);
    let problems = visit_parsed(&mut p, Some(none.index), &mut c);
    let bytes = write_parsed(&p, layout, FName::new(0));
    for n in &c.names {
        h.update(n.as_bytes());
        h.update(b"\0");
    }
    h.update(b"\x01");
    for r in &c.refs {
        h.update(r.as_bytes());
        h.update(b"\0");
    }
    h.update(b"\x02");
    for p in &problems {
        h.update(p.as_bytes());
    }
    h.update(&bytes);
    if let Ok(want) = std::env::var("TDELTA_DEBUG") {
        let path = pkg.object_path(PackageIndex::export(i));
        if path.eq_ignore_ascii_case(&want) {
            eprintln!("[tdelta] {path}: class={} super={} arch={} flags={:#x}/{:#x}/{:#x} netcounts={:?}", describe(pkg, e.class), describe(pkg, e.super_), describe(pkg, e.archetype), e.object_flags, e.export_flags, e.package_flags, e.generation_net_object_count);
            eprintln!("[tdelta]   names={:?}", c.names);
            eprintln!("[tdelta]   refs={:?}", c.refs);
            eprintln!("[tdelta]   problems={:?} payload={} bytes md5={}", problems, bytes.len(), hex(&Md5::digest(&bytes)));
        }
    }
    Some(h.finalize().into())
}

fn hex(h: &[u8]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// Lower-case path → export, for every export that is not a placeholder.
fn path_index(pkg: &Package) -> HashMap<String, usize> {
    let mut m = HashMap::with_capacity(pkg.exports.len());
    for i in 0..pkg.exports.len() {
        if !is_placeholder(pkg, i) {
            m.insert(pkg.object_path(PackageIndex::export(i)).to_ascii_lowercase(), i);
        }
    }
    m
}

/// The edited package's tables extend the base's (the editor appends; a
/// deleted export becomes a placeholder), so equal payload bytes mean equal
/// objects. A rebuilt package (script recompile) fails this and every
/// export is compared canonically.
fn tables_compatible(base: &Package, edited: &Package) -> bool {
    if edited.names.len() < base.names.len() || edited.imports.len() < base.imports.len() || edited.exports.len() < base.exports.len() {
        return false;
    }
    if base.names.iter().zip(&edited.names).any(|(a, b)| a.name != b.name) {
        return false;
    }
    if base.imports.iter().zip(&edited.imports).any(|(a, b)| a != b) {
        return false;
    }
    for (i, (a, b)) in base.exports.iter().zip(&edited.exports).enumerate() {
        if is_placeholder(edited, i) {
            continue;
        }
        if a.object_name != b.object_name || a.outer != b.outer {
            return false;
        }
    }
    true
}

// ----------------------------------------------------------------------------
// Digest (the Studio's record of a package before it was first saved)
// ----------------------------------------------------------------------------

/// Canonical hash and class of every object of a package: enough to create
/// a delta later without the original file.
pub fn digest(db: &ClassDb, pkg: &Package) -> Value {
    let layout = package_layout(pkg);
    let mut exports = serde_json::Map::new();
    for i in 0..pkg.exports.len() {
        if let Some(h) = canonical(db, pkg, layout, i) {
            let idx = PackageIndex::export(i);
            exports.insert(pkg.object_path(idx).to_ascii_lowercase(), json!([pkg.class_name_of(idx), hex(&h)]));
        }
    }
    json!({ "format": "tdelta-digest", "version": VERSION, "package": pkg.package_name(), "exports": exports })
}

/// What a delta is created against: the base package itself, or its digest.
pub enum Base<'a> {
    Package(&'a Package),
    Digest(&'a Value),
}

// ----------------------------------------------------------------------------
// Create
// ----------------------------------------------------------------------------

enum Change {
    Add(usize),
    Set(usize, String),
}

/// Compare `edited` with its base and build the delta for `target` (the
/// game-relative path of the file). Returns the delta and warnings about
/// payloads whose references could not be remapped.
pub fn create(db: &ClassDb, base: Base, edited: &Package, target: &str) -> Result<(Delta, Vec<String>)> {
    let layout = package_layout(edited);
    let mut warnings = Vec::new();
    let edited_paths = path_index(edited);

    // What changed, by path.
    let mut changes: Vec<(String, Change)> = Vec::new();
    let mut deletes: Vec<(String, String)> = Vec::new();
    match base {
        Base::Package(base) => {
            let base_layout = package_layout(base);
            let base_paths = path_index(base);
            // Equal bytes only mean equal objects while every export they can
            // point at still means the same thing: a deleted export (now a
            // placeholder) changes what an untouched reference to it means.
            let deleted_any = base_paths.keys().any(|p| !edited_paths.contains_key(p));
            let fast = !deleted_any && tables_compatible(base, edited);
            for (path, &i) in &edited_paths {
                match base_paths.get(path) {
                    None => changes.push((path.clone(), Change::Add(i))),
                    Some(&j) => {
                        let (a, b) = (&base.exports[j], &edited.exports[i]);
                        let same_entry = a.class == b.class && a.super_ == b.super_ && a.archetype == b.archetype && a.object_flags == b.object_flags && a.export_flags == b.export_flags;
                        if fast && same_entry && base.export_data.get(j) == edited.export_data.get(i) {
                            continue;
                        }
                        let hb = canonical(db, base, base_layout, j);
                        let he = canonical(db, edited, layout, i);
                        if hb != he {
                            changes.push((path.clone(), Change::Set(i, hb.map(|h| hex(&h)).unwrap_or_default())));
                        }
                    }
                }
            }
            for (path, &j) in &base_paths {
                if !edited_paths.contains_key(path) {
                    deletes.push((path.clone(), base.class_name_of(PackageIndex::export(j))));
                }
            }
        }
        Base::Digest(d) => {
            let empty = serde_json::Map::new();
            let exports = d["exports"].as_object().unwrap_or(&empty);
            for (path, &i) in &edited_paths {
                match exports.get(path) {
                    None => changes.push((path.clone(), Change::Add(i))),
                    Some(entry) => {
                        let hb = entry[1].as_str().unwrap_or("");
                        let he = canonical(db, edited, layout, i).map(|h| hex(&h)).unwrap_or_default();
                        if hb != he {
                            changes.push((path.clone(), Change::Set(i, hb.to_string())));
                        }
                    }
                }
            }
            for (path, entry) in exports {
                if !edited_paths.contains_key(path) {
                    deletes.push((path.clone(), entry[0].as_str().unwrap_or("").to_string()));
                }
            }
        }
    }
    // Outers before their children, then a stable order.
    changes.sort_by(|a, b| (a.0.matches('.').count(), &a.0).cmp(&(b.0.matches('.').count(), &b.0)));
    deletes.sort();

    // The mini package.
    let mut mini = Package::default();
    mini.summary = edited.summary.clone();
    mini.summary.compressed_chunks.clear();
    mini.summary.compression_flags = 0;
    mini.summary.texture_allocations.clear();
    mini.summary.additional_packages_to_cook.clear();
    mini.summary.generations = vec![Default::default()];
    mini.intern_name("None");
    let op_of: HashMap<usize, usize> = changes.iter().enumerate().map(|(k, (_, c))| (match c { Change::Add(i) | Change::Set(i, _) => *i }, k)).collect();
    // Reserve the mini exports so references between them resolve to their index.
    for (_, c) in &changes {
        let i = match c { Change::Add(i) | Change::Set(i, _) => *i };
        let e = &edited.exports[i];
        let name = edited.object_name(PackageIndex::export(i));
        let export = ObjectExport { object_name: mini.intern_fname(&name), object_flags: e.object_flags, export_flags: e.export_flags, generation_net_object_count: e.generation_net_object_count.clone(), package_guid: e.package_guid, package_flags: e.package_flags, ..Default::default() };
        mini.add_export(export, Vec::new());
    }

    let mut refs: Vec<Value> = Vec::new();
    let mut ref_memo: HashMap<PackageIndex, PackageIndex> = HashMap::new();
    // A reference of the edited package in mini-package coordinates: one of
    // the delta's exports, or an import describing where to find it.
    fn map_ref(edited: &Package, mini: &mut Package, idx: PackageIndex, op_of: &HashMap<usize, usize>, refs: &mut Vec<Value>, memo: &mut HashMap<PackageIndex, PackageIndex>) -> PackageIndex {
        if idx.is_null() {
            return PackageIndex::NONE;
        }
        if let Some(&m) = memo.get(&idx) {
            return m;
        }
        if let Some(i) = idx.export_index() {
            if let Some(&k) = op_of.get(&i) {
                let m = PackageIndex::export(k);
                memo.insert(idx, m);
                return m;
            }
        }
        let outer = map_ref(edited, mini, edited.object_outer(idx), op_of, refs, memo);
        let name = edited.object_name(idx);
        let (class_package, class_name, desc) = if let Some(i) = idx.import_index() {
            let imp = &edited.imports[i];
            (edited.fname_to_string(imp.class_package), edited.fname_to_string(imp.class_name), json!({ "k": "import" }))
        } else {
            let e = &edited.exports[idx.export_index().unwrap()];
            let class_name = edited.class_name_of(idx);
            let class_package = if e.class.is_null() { "Core".to_string() } else { edited.outermost_name(e.class) };
            (class_package, class_name.clone(), json!({ "k": "export", "path": edited.object_path(idx), "class": class_name }))
        };
        let before = mini.imports.len();
        let m = mini.add_import(&class_package, &class_name, outer, &name);
        if mini.imports.len() > before {
            refs.push(desc);
        }
        memo.insert(idx, m);
        m
    }

    let none = none_name(edited);
    let mut ops: Vec<Value> = Vec::new();
    let mut unknown_classes: HashSet<String> = HashSet::new();
    for (k, (path, c)) in changes.iter().enumerate() {
        let (i, op, base_hash) = match c {
            Change::Add(i) => (*i, "add", None),
            Change::Set(i, h) => (*i, "set", Some(h.clone())),
        };
        let e = edited.exports[i].clone();
        let table: Vec<PackageIndex> = [e.class, e.super_, e.outer, e.archetype].into_iter().map(|r| map_ref(edited, &mut mini, r, &op_of, &mut refs, &mut ref_memo)).collect();
        {
            let me = &mut mini.exports[k];
            me.class = table[0];
            me.super_ = table[1];
            me.outer = table[2];
            me.archetype = table[3];
        }
        let data = edited.export_data.get(i).map(|d| d.as_slice()).unwrap_or(&[]);
        let mut p = parse_export(db, edited, layout, i, data);
        // An object whose class the database does not know (or with values it
        // could not type) keeps raw bytes, and references inside raw bytes
        // cannot be remapped: the delta would carry this package's indices.
        if let Parsed::Object(o) = &p {
            if o.class.is_none() && !o.properties.is_empty() {
                let class = edited.class_name_of(PackageIndex::export(i));
                if !unknown_classes.contains(&class) {
                    unknown_classes.insert(class.clone());
                    warnings.push(format!("class {class} is not in the class database (wrong -I folder?): references inside its objects are copied as they are"));
                }
            } else if o.has_raw_values() {
                warnings.push(format!("{}: some values could not be typed; references inside them are copied as they are", edited.object_path(PackageIndex::export(i))));
            }
        }
        struct Remap<'s> {
            edited: &'s Package,
            mini: &'s mut Package,
            op_of: &'s HashMap<usize, usize>,
            refs: &'s mut Vec<Value>,
            memo: &'s mut HashMap<PackageIndex, PackageIndex>,
            names: HashMap<u32, u32>,
        }
        impl Visitor for Remap<'_> {
            fn name(&mut self, n: &mut FName) {
                let idx = *self.names.entry(n.index).or_insert_with(|| self.mini.intern_name(self.edited.name_str(n.index)));
                n.index = idx;
            }
            fn index(&mut self, i: &mut PackageIndex) {
                *i = map_ref(self.edited, self.mini, *i, self.op_of, self.refs, self.memo);
            }
        }
        let mut remap = Remap { edited, mini: &mut mini, op_of: &op_of, refs: &mut refs, memo: &mut ref_memo, names: HashMap::new() };
        for problem in visit_parsed(&mut p, Some(none.index), &mut remap) {
            warnings.push(format!("{}: {problem}", edited.object_path(PackageIndex::export(i))));
        }
        let mini_none = none_name(&mini);
        let bytes = write_parsed(&p, layout, mini_none);
        mini.set_export_data(k, bytes);
        let class = edited.class_name_of(PackageIndex::export(i));
        ops.push(json!({ "op": op, "path": path, "class": class, "base": base_hash, "export": k }));
    }
    for (path, class) in &deletes {
        ops.push(json!({ "op": "delete", "path": path, "class": class }));
    }

    let image = mini.to_bytes(&WriteOptions { compress: false, obscure: false });
    let header = json!({
        "format": "tdelta",
        "version": VERSION,
        "kind": "upk",
        "target": target,
        "package": Path::new(target).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        "layout": { "editor_data": layout.editor_data },
        "refs": refs,
        "ops": ops,
    });
    Ok((Delta { header, image }, warnings))
}

// ----------------------------------------------------------------------------
// Apply
// ----------------------------------------------------------------------------

/// Which operations an [`apply`] pass handles. A delta adding classes is
/// applied in two passes: the script objects first, then, once the class
/// database knows the new classes, the objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    All,
    Script,
    Objects,
}

#[derive(Clone, Debug, Default)]
pub struct Applied {
    pub added: usize,
    pub replaced: usize,
    pub deleted: usize,
    /// Objects a `set` replaced whose base no longer matched what the delta
    /// was made against (another mod or a game update changed them first).
    pub drift: Vec<String>,
    pub warnings: Vec<String>,
}

/// Apply `delta` to the open package `ed`.
pub fn apply(db: &ClassDb, ed: &mut Editor, delta: &Delta, phase: Phase) -> Result<Applied> {
    let mut out = Applied::default();
    let mini = Package::parse(&delta.image)?;
    let mini_layout = delta.layout();
    let mini_none = none_name(&mini);
    let refs: Vec<Value> = delta.header["refs"].as_array().cloned().unwrap_or_default();
    let ops = delta.ops();
    let in_phase = |class: &str| match phase {
        Phase::All => true,
        Phase::Script => is_script_class(class),
        Phase::Objects => !is_script_class(class),
    };

    ed.flush();
    let mut paths = path_index(&ed.pkg);
    let target_layout = ed.layout();

    // Drift is judged against the package as it was before this delta: once
    // the deletes below run, objects that referred to a deleted one hash
    // differently even though nobody else changed them.
    let mut drifted: HashSet<String> = HashSet::new();
    for op in ops.iter().filter(|o| o.op == "set" && in_phase(&o.class)) {
        let (Some(base), Some(&t)) = (&op.base, paths.get(&op.path.to_ascii_lowercase())) else { continue };
        if let Some(h) = canonical(db, &ed.pkg, target_layout, t) {
            if hex(&h) != *base {
                drifted.insert(op.path.to_ascii_lowercase());
            }
        }
    }

    // Deletes first, so an object deleted and re-created elsewhere does not
    // clash with its old self.
    if phase != Phase::Objects {
        for op in ops.iter().filter(|o| o.op == "delete") {
            if let Some(&t) = paths.get(&op.path.to_ascii_lowercase()) {
                ed.delete(t)?;
                paths.remove(&op.path.to_ascii_lowercase());
                out.deleted += 1;
            } else {
                out.warnings.push(format!("{}: not in the package, nothing to delete", op.path));
            }
        }
    }

    // Pass 1: every changed object gets its export (existing, or appended),
    // whatever the phase, so a class applied now can point at the default
    // object whose payload only arrives in the next phase.
    let mut target_of: HashMap<usize, usize> = HashMap::new();
    for op in ops.iter().filter(|o| o.op != "delete") {
        let Some(k) = op.export else { continue };
        let key = op.path.to_ascii_lowercase();
        let t = match paths.get(&key) {
            Some(&t) => {
                if in_phase(&op.class) && op.op == "set" {
                    if drifted.contains(&key) {
                        out.drift.push(op.path.clone());
                    }
                    out.replaced += 1;
                }
                t
            }
            None => {
                let me = &mini.exports[k];
                let name = mini.object_name(PackageIndex::export(k));
                let export = ObjectExport { object_name: ed.pkg.intern_fname(&name), object_flags: me.object_flags, export_flags: me.export_flags, generation_net_object_count: me.generation_net_object_count.clone(), package_guid: me.package_guid, package_flags: me.package_flags, ..Default::default() };
                let t = ed.add_export(export, Vec::new());
                paths.insert(key, t);
                out.added += 1;
                t
            }
        };
        target_of.insert(k, t);
    }

    // References of the mini package in the target's coordinates.
    struct Resolver<'s> {
        mini: &'s Package,
        refs: &'s [Value],
        target_of: &'s HashMap<usize, usize>,
        memo: HashMap<PackageIndex, PackageIndex>,
        warnings: Vec<String>,
    }
    impl Resolver<'_> {
        fn resolve(&mut self, ed: &mut Editor, paths: &mut HashMap<String, usize>, idx: PackageIndex) -> PackageIndex {
            if idx.is_null() {
                return PackageIndex::NONE;
            }
            if let Some(&m) = self.memo.get(&idx) {
                return m;
            }
            let m = if let Some(k) = idx.export_index() {
                match self.target_of.get(&k) {
                    Some(&t) => PackageIndex::export(t),
                    // An object of another pass: already in the package by now.
                    None => paths.get(&self.mini.object_path(idx).to_ascii_lowercase()).map(|&t| PackageIndex::export(t)).unwrap_or(PackageIndex::NONE),
                }
            } else {
                let k = idx.import_index().unwrap();
                let desc = self.refs.get(k).cloned().unwrap_or(Value::Null);
                if desc["k"].as_str() == Some("export") {
                    let path = desc["path"].as_str().unwrap_or("");
                    let class = desc["class"].as_str().unwrap_or("");
                    match paths.get(&path.to_ascii_lowercase()) {
                        Some(&t) => PackageIndex::export(t),
                        None if class.eq_ignore_ascii_case("Package") => {
                            // A group the base lacks: recreate it under its outer.
                            let outer = self.resolve(ed, paths, self.mini.imports[k].outer);
                            let core = ed.pkg.add_import("Core", "Package", PackageIndex::NONE, "Core");
                            let class = ed.pkg.add_import("Core", "Class", core, "Package");
                            let flags = (0..ed.pkg.exports.len()).find(|&i| ed.pkg.is_class(PackageIndex::export(i), "Package")).map(|i| ed.pkg.exports[i].object_flags).unwrap_or(0);
                            let name = self.mini.object_name(idx);
                            let export = ObjectExport { object_name: ed.pkg.intern_fname(&name), class, outer, object_flags: flags, ..Default::default() };
                            let t = ed.add_export(export, Vec::new());
                            paths.insert(path.to_ascii_lowercase(), t);
                            PackageIndex::export(t)
                        }
                        None => {
                            self.warnings.push(format!("{path} ({class}) is not in the package; the reference was cleared"));
                            PackageIndex::NONE
                        }
                    }
                } else {
                    ed.ensure_import(self.mini, idx)
                }
            };
            self.memo.insert(idx, m);
            m
        }
    }
    let mut res = Resolver { mini: &mini, refs: &refs, target_of: &target_of, memo: HashMap::new(), warnings: Vec::new() };

    // Pass 2: table entries and payloads.
    let target_none = none_name(&ed.pkg);
    for op in ops.iter().filter(|o| o.op != "delete" && in_phase(&o.class)) {
        let Some(k) = op.export else { continue };
        let t = target_of[&k];
        let me = mini.exports[k].clone();
        let mut table: Vec<PackageIndex> = [me.class, me.super_, me.outer, me.archetype].into_iter().map(|r| res.resolve(ed, &mut paths, r)).collect();
        // The cooker stores a class export's class (UClass) as a null index.
        if describe(&ed.pkg, table[0]) == "i|core|class|core.class" {
            table[0] = PackageIndex::NONE;
        }
        {
            let e = ed.export_mut(t)?;
            e.class = table[0];
            e.super_ = table[1];
            e.outer = table[2];
            e.archetype = table[3];
            e.object_flags = me.object_flags;
            e.export_flags = me.export_flags;
            e.generation_net_object_count = me.generation_net_object_count.clone();
            e.package_guid = me.package_guid;
            e.package_flags = me.package_flags;
        }
        let data = mini.export_data.get(k).map(|d| d.as_slice()).unwrap_or(&[]);
        let mut p = parse_export(db, &mini, mini_layout, k, data);
        struct Remap<'s, 'r, 'e> {
            res: &'s mut Resolver<'r>,
            ed: &'s mut Editor<'e>,
            paths: &'s mut HashMap<String, usize>,
            names: HashMap<u32, u32>,
        }
        impl Visitor for Remap<'_, '_, '_> {
            fn name(&mut self, n: &mut FName) {
                let mini = self.res.mini;
                let idx = *self.names.entry(n.index).or_insert_with(|| self.ed.pkg.intern_name(mini.name_str(n.index)));
                n.index = idx;
            }
            fn index(&mut self, i: &mut PackageIndex) {
                *i = self.res.resolve(self.ed, self.paths, *i);
            }
        }
        let mut remap = Remap { res: &mut res, ed, paths: &mut paths, names: HashMap::new() };
        for problem in visit_parsed(&mut p, Some(mini_none.index), &mut remap) {
            out.warnings.push(format!("{}: {problem}", op.path));
        }
        let bytes = write_parsed(&p, target_layout, target_none);
        ed.set_raw(t, bytes)?;
    }
    out.warnings.append(&mut res.warnings);
    Ok(out)
}

/// Objects two deltas both touch (lower-case paths): what the launcher
/// reports as a conflict.
pub fn overlap(a: &Delta, b: &Delta) -> Vec<String> {
    let keys: HashSet<String> = a.ops().into_iter().map(|o| o.path.to_ascii_lowercase()).collect();
    let mut out: Vec<String> = b.ops().into_iter().map(|o| o.path.to_ascii_lowercase()).filter(|p| keys.contains(p)).collect();
    out.sort();
    out.dedup();
    out
}

/// Every object path of `result` whose canonical form differs from
/// `expected`'s, plus objects one has and the other lacks: the round-trip
/// check (`apply(create(base, edited), base)` must equal `edited`).
pub fn compare(db: &ClassDb, expected: &Package, result: &Package) -> Vec<String> {
    let (le, lr) = (package_layout(expected), package_layout(result));
    let pe = path_index(expected);
    let pr = path_index(result);
    let mut diffs = Vec::new();
    for (path, &i) in &pe {
        match pr.get(path) {
            None => diffs.push(format!("{path}: missing from the result")),
            Some(&j) => {
                if canonical(db, expected, le, i) != canonical(db, result, lr, j) {
                    diffs.push(format!("{path}: differs"));
                }
            }
        }
    }
    for path in pr.keys() {
        if !pe.contains_key(path) {
            diffs.push(format!("{path}: unexpected in the result"));
        }
    }
    diffs.sort();
    diffs
}
