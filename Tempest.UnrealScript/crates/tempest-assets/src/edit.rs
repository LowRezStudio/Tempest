//! Package editing: rename, delete and copy objects, read and write
//! properties by path, and save the result the way the cooker stored it.
//!
//! Objects are parsed on demand and written back only when touched. A
//! deleted export becomes the same zero-size, class-less placeholder the
//! cooker leaves behind (no index shifting), and every reference to it is
//! cleared. Copying brings an object's whole dependency closure over from
//! the source package (sub-objects, materials, textures…), reusing objects
//! the destination already has at the same path, and turns references to
//! classes and other script objects into imports.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use tempest_unrealscript::context::ObjRef;
use tempest_unrealscript::objects::{Layout, NameTable, PropertyKind};
use tempest_upk::flags::compression;
use tempest_upk::io::{Error, FName, PackageIndex, Result};
use tempest_upk::{ObjectExport, Package, PackageFile, WriteOptions};

use crate::dump::Dumper;
use crate::object::{none_name, package_layout, parse_export, write_parsed, Object, Parsed};
use crate::reflect::ClassDb;
use crate::value::{kind_from_type_name, tag_type_name, Property, StructValue, Value};
use crate::visit::{references, Visit, Visitor};

pub struct Editor<'a> {
    pub db: &'a ClassDb,
    pub name: String,
    pub pkg: Package,
    layout: Layout,
    objects: HashMap<usize, Parsed>,
    dirty: HashSet<usize>,
    was_compressed: bool,
    obscured: bool,
    /// The whole package was regenerated (script recompile) and not saved yet.
    rebuilt: bool,
    /// Undo journal: one entry per operation (`begin_undo`), newest last.
    undo: Vec<UndoEntry>,
    /// The entry capturing the operation in progress.
    current: Option<UndoEntry>,
}

const MAX_UNDO: usize = 50;

/// State of one export before an operation touched it.
struct Snapshot {
    index: usize,
    export: ObjectExport,
    data: Vec<u8>,
    parsed: Option<Parsed>,
    dirty: bool,
}

/// Everything needed to put the package back as it was before one operation.
pub struct UndoEntry {
    pub label: String,
    snapshots: Vec<Snapshot>,
    /// (exports, imports, names) counts before: anything appended is cut off.
    lens: (usize, usize, usize),
    /// Whole-package snapshot (script recompiles replace everything).
    full: Option<Box<FullSnapshot>>,
    /// The summary before (texture allocations change with texture imports).
    summary: tempest_upk::package::Summary,
    /// The operation's result has been written to disk: undoing it makes the
    /// package dirty again instead of restoring the pre-operation dirty flags.
    saved: bool,
}

/// (package, parsed objects, dirty set, rebuilt flag) before a whole-package replacement.
type FullSnapshot = (Package, HashMap<usize, Parsed>, HashSet<usize>, bool);

impl UndoEntry {
    /// Nothing was snapshotted and no table grew: not worth an entry.
    fn is_empty(&self, pkg: &Package) -> bool {
        self.snapshots.is_empty() && self.full.is_none() && self.lens == (pkg.exports.len(), pkg.imports.len(), pkg.names.len()) && self.summary == pkg.summary
    }
}

impl<'a> Editor<'a> {
    pub fn open(db: &'a ClassDb, path: impl AsRef<Path>) -> Result<Editor<'a>> {
        let path = path.as_ref();
        let pf = PackageFile::open(path)?;
        let pkg = pf.load()?;
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        Ok(Editor::from_package(db, name, pkg))
    }

    pub fn from_package(db: &'a ClassDb, name: String, pkg: Package) -> Editor<'a> {
        let layout = package_layout(&pkg);
        let obscured = pkg.summary.compression_flags & compression::OBSCURED != 0;
        Editor { db, name, was_compressed: pkg.was_compressed, obscured, pkg, layout, objects: HashMap::new(), dirty: HashSet::new(), rebuilt: false, undo: Vec::new(), current: None }
    }

    // ----- undo -------------------------------------------------------------

    /// Start recording an operation; the previous one is filed (if it
    /// changed anything). Every mutation below snapshots what it touches.
    pub fn begin_undo(&mut self, label: impl Into<String>) {
        self.end_undo();
        self.current = Some(UndoEntry { label: label.into(), snapshots: Vec::new(), lens: (self.pkg.exports.len(), self.pkg.imports.len(), self.pkg.names.len()), full: None, summary: self.pkg.summary.clone(), saved: false });
    }

    pub fn end_undo(&mut self) {
        if let Some(e) = self.current.take() {
            if !e.is_empty(&self.pkg) {
                self.undo.push(e);
                if self.undo.len() > MAX_UNDO {
                    self.undo.remove(0);
                }
            }
        }
    }

    /// Record export `i` before it changes (once per operation).
    fn snapshot(&mut self, i: usize) {
        let Some(cur) = &mut self.current else { return };
        if cur.full.is_some() || cur.snapshots.iter().any(|s| s.index == i) || i >= self.pkg.exports.len() {
            return;
        }
        // Exports appended by this operation are undone by truncation.
        if i >= cur.lens.0 {
            return;
        }
        cur.snapshots.push(Snapshot { index: i, export: self.pkg.exports[i].clone(), data: self.pkg.export_data.get(i).cloned().unwrap_or_default(), parsed: self.objects.get(&i).cloned(), dirty: self.dirty.contains(&i) });
    }

    /// Labels of what `undo` would revert, newest first.
    pub fn undo_labels(&self) -> Vec<String> {
        let mut v: Vec<String> = self.undo.iter().map(|e| e.label.clone()).collect();
        if let Some(c) = &self.current {
            if !c.is_empty(&self.pkg) {
                v.push(c.label.clone());
            }
        }
        v.reverse();
        v
    }

    /// Revert the last operation. Returns its label.
    pub fn undo(&mut self) -> Option<String> {
        self.end_undo();
        let e = self.undo.pop()?;
        if let Some(full) = e.full {
            let (pkg, objects, dirty, rebuilt) = *full;
            self.layout = package_layout(&pkg);
            self.pkg = pkg;
            self.objects = objects;
            self.dirty = dirty;
            // The whole package differs from the saved file again.
            self.rebuilt = rebuilt || e.saved;
            return Some(e.label);
        }
        let saved = e.saved;
        let tables_changed = e.lens != (self.pkg.exports.len(), self.pkg.imports.len(), self.pkg.names.len()) || e.summary != self.pkg.summary;
        for snap in e.snapshots.into_iter().rev() {
            let i = snap.index;
            if i < self.pkg.exports.len() {
                self.pkg.exports[i] = snap.export;
                while self.pkg.export_data.len() <= i {
                    self.pkg.export_data.push(Vec::new());
                }
                self.pkg.export_data[i] = snap.data;
            }
            match snap.parsed {
                Some(p) => {
                    self.objects.insert(i, p);
                }
                None => {
                    self.objects.remove(&i);
                }
            }
            if snap.dirty || saved {
                self.dirty.insert(i);
            } else {
                self.dirty.remove(&i);
            }
        }
        if saved && tables_changed {
            self.rebuilt = true;
        }
        let (ne, ni, nn) = e.lens;
        if self.pkg.exports.len() > ne {
            for j in ne..self.pkg.exports.len() {
                self.objects.remove(&j);
                self.dirty.remove(&j);
            }
            self.pkg.exports.truncate(ne);
            self.pkg.export_data.truncate(ne);
            self.pkg.depends.truncate(ne);
        }
        if self.pkg.imports.len() > ni {
            self.pkg.imports.truncate(ni);
        }
        if self.pkg.names.len() > nn {
            self.pkg.names.truncate(nn);
            self.pkg.rebuild_name_lookup();
        }
        self.pkg.summary = e.summary;
        Some(e.label)
    }

    /// Store a rewritten object for export `i` (recorded for undo, marked dirty).
    pub fn replace_object(&mut self, i: usize, obj: Object) -> Result<()> {
        if i >= self.pkg.exports.len() {
            return Err(Error::Format(format!("no export {i}")));
        }
        self.snapshot(i);
        self.objects.insert(i, Parsed::Object(obj));
        self.dirty.insert(i);
        Ok(())
    }

    /// Swap in a regenerated package (a recompiled script package): parsed
    /// objects are dropped, the storage flags of the original are kept so
    /// `save` writes it the same way.
    pub fn replace_package(&mut self, mut pkg: Package) {
        // Shown as "compressed" when it will be saved that way: the editor's
        // storage flags come from the file, not from the compiler's output.
        pkg.was_compressed = self.was_compressed;
        if self.obscured {
            pkg.summary.compression_flags |= compression::OBSCURED;
        } else {
            pkg.summary.compression_flags &= !compression::OBSCURED;
        }
        if let Some(cur) = &mut self.current {
            if cur.full.is_none() {
                cur.snapshots.clear();
                cur.full = Some(Box::new((self.pkg.clone(), self.objects.clone(), self.dirty.clone(), self.rebuilt)));
            }
        }
        self.layout = package_layout(&pkg);
        self.pkg = pkg;
        // A compiled package gets its serial sizes on save; the tables are
        // shown (and classes recognised) before that, so fill them in now.
        for i in 0..self.pkg.exports.len() {
            if let Some(d) = self.pkg.export_data.get(i) {
                self.pkg.exports[i].serial_size = d.len() as i32;
            }
        }
        self.objects.clear();
        self.dirty.clear();
        self.rebuilt = true;
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// The parsed form of export `i` (cached).
    pub fn parsed(&mut self, i: usize) -> Result<&mut Parsed> {
        if i >= self.pkg.exports.len() {
            return Err(Error::Format(format!("no export {i}")));
        }
        if !self.objects.contains_key(&i) {
            let p = parse_export(self.db, &self.pkg, self.layout, i, &self.pkg.export_data[i]);
            self.objects.insert(i, p);
        }
        Ok(self.objects.get_mut(&i).unwrap())
    }

    /// Export `i` as an editable object (error for script objects and raw payloads).
    pub fn object(&mut self, i: usize) -> Result<&mut Object> {
        let full = self.pkg.full_name(PackageIndex::export(i));
        match self.parsed(i)? {
            Parsed::Object(o) => Ok(o),
            Parsed::Script(_) => Err(Error::Format(format!("{full} is a script object; use uscript to edit it"))),
            Parsed::Empty => Err(Error::Format(format!("{full} is an empty placeholder"))),
            Parsed::Raw { error, .. } => Err(Error::Format(format!("{full} could not be parsed: {error}"))),
        }
    }

    pub fn mark_dirty(&mut self, i: usize) {
        self.dirty.insert(i);
    }

    /// Replace the raw serialized payload of export `i` (the inverse of
    /// extracting it): no parsing, written as given.
    pub fn set_raw(&mut self, i: usize, data: Vec<u8>) -> Result<()> {
        if i >= self.pkg.exports.len() {
            return Err(Error::Format(format!("no export {i}")));
        }
        self.snapshot(i);
        self.pkg.set_export_data(i, data);
        self.objects.remove(&i);
        self.dirty.insert(i);
        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        self.rebuilt || !self.dirty.is_empty()
    }

    /// Unsaved edits to individual objects (as opposed to a whole-package rebuild).
    pub fn has_object_edits(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// Resolve `<idx|path>` to an export index.
    pub fn find(&self, what: &str) -> Result<usize> {
        if let Ok(i) = what.parse::<usize>() {
            if i < self.pkg.exports.len() {
                return Ok(i);
            }
            return Err(Error::Format(format!("export {i} out of range (have {})", self.pkg.exports.len())));
        }
        self.pkg.find_export(what, None).ok_or_else(|| Error::Format(format!("no export named {what}")))
    }

    // ----- rename / delete ------------------------------------------------

    pub fn rename(&mut self, i: usize, new_name: &str) -> Result<()> {
        let outer = self.pkg.exports[i].outer;
        let clash = (0..self.pkg.exports.len()).any(|j| j != i && self.pkg.exports[j].outer == outer && self.pkg.object_name(PackageIndex::export(j)).eq_ignore_ascii_case(new_name));
        if clash {
            return Err(Error::Format(format!("an object called {new_name} already exists in the same outer")));
        }
        self.snapshot(i);
        let n = self.pkg.intern_fname(new_name);
        self.pkg.exports[i].object_name = n;
        Ok(())
    }

    /// Children (transitively) of export `i`.
    pub fn children(&self, i: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for j in 0..self.pkg.exports.len() {
            let mut cur = self.pkg.exports[j].outer;
            let mut guard = 0;
            while let Some(o) = cur.export_index() {
                if o == i {
                    out.push(j);
                    break;
                }
                cur = self.pkg.exports[o].outer;
                guard += 1;
                if guard > 64 {
                    break;
                }
            }
        }
        out
    }

    /// Turn export `i` (and its sub-objects) into cooker-style placeholders
    /// and clear every reference to them. Returns the number of references
    /// cleared.
    pub fn delete(&mut self, i: usize) -> Result<usize> {
        let mut victims = self.children(i);
        victims.push(i);
        let victim_set: HashSet<PackageIndex> = victims.iter().map(|&v| PackageIndex::export(v)).collect();
        let none = self.pkg.intern_fname("None");
        for &v in &victims {
            self.snapshot(v);
        }
        for &v in &victims {
            let e = &mut self.pkg.exports[v];
            e.class = PackageIndex::NONE;
            e.super_ = PackageIndex::NONE;
            e.outer = PackageIndex::NONE;
            e.archetype = PackageIndex::NONE;
            e.object_name = none;
            e.object_flags = 0;
            e.export_flags = 0;
            e.serial_size = 0;
            self.pkg.export_data[v] = Vec::new();
            self.objects.insert(v, Parsed::Empty);
            self.dirty.remove(&v);
        }
        let mut cleared = 0;
        for j in 0..self.pkg.exports.len() {
            let e = &self.pkg.exports[j];
            if ![e.class, e.super_, e.archetype].iter().any(|r| victim_set.contains(r)) {
                continue;
            }
            self.snapshot(j);
            let e = &mut self.pkg.exports[j];
            for r in [&mut e.class, &mut e.super_, &mut e.archetype] {
                if victim_set.contains(r) {
                    *r = PackageIndex::NONE;
                    cleared += 1;
                }
            }
        }
        struct Clear<'s> {
            victims: &'s HashSet<PackageIndex>,
            n: usize,
        }
        impl Visitor for Clear<'_> {
            fn name(&mut self, _: &mut FName) {}
            fn index(&mut self, i: &mut PackageIndex) {
                if self.victims.contains(i) {
                    *i = PackageIndex::NONE;
                    self.n += 1;
                }
            }
        }
        for j in 0..self.pkg.exports.len() {
            if victim_set.contains(&PackageIndex::export(j)) {
                continue;
            }
            let touches = match self.parsed(j) {
                Ok(Parsed::Object(o)) => references(o).iter().any(|r| victim_set.contains(r)),
                _ => false,
            };
            if !touches {
                continue;
            }
            self.snapshot(j);
            if let Ok(Parsed::Object(o)) = self.parsed(j) {
                let mut c = Clear { victims: &victim_set, n: 0 };
                o.visit(&mut c);
                if c.n > 0 {
                    cleared += c.n;
                    self.dirty.insert(j);
                }
            }
        }
        Ok(cleared)
    }

    // ----- copy -------------------------------------------------------------

    /// The table entry of export `i`, recorded for undo before it changes.
    pub fn export_mut(&mut self, i: usize) -> Result<&mut ObjectExport> {
        if i >= self.pkg.exports.len() {
            return Err(Error::Format(format!("no export {i}")));
        }
        self.snapshot(i);
        Ok(&mut self.pkg.exports[i])
    }

    /// Append an export (recorded for undo by truncation). Returns its index.
    /// A copy of export `i` next to it, named `<name>_copy` (or `_copy2`…):
    /// same class, outer, archetype and flags, the same serialized payload
    /// (references keep pointing where the original's do). Sub-objects are
    /// not copied. Returns the new export's index.
    pub fn duplicate(&mut self, i: usize) -> Result<usize> {
        self.flush();
        let src = self.pkg.exports.get(i).cloned().ok_or_else(|| Error::Format(format!("no export {i}")))?;
        let base = self.pkg.object_name(PackageIndex::export(i));
        let taken = |ed: &Self, n: &str| (0..ed.pkg.exports.len()).any(|j| ed.pkg.exports[j].outer == src.outer && ed.pkg.object_name(PackageIndex::export(j)).eq_ignore_ascii_case(n));
        let mut name = format!("{base}_copy");
        let mut k = 2;
        while taken(self, &name) {
            name = format!("{base}_copy{k}");
            k += 1;
        }
        let data = self.pkg.export_data.get(i).cloned().unwrap_or_default();
        let export = ObjectExport { object_name: self.pkg.intern_fname(&name), ..src };
        Ok(self.add_export(export, data))
    }

    pub fn add_export(&mut self, export: ObjectExport, data: Vec<u8>) -> usize {
        let idx = self.pkg.add_export(export, data);
        let i = idx.export_index().unwrap();
        self.dirty.insert(i);
        i
    }

    /// Ensure `src`'s import `idx` (with its outer chain) exists here.
    pub fn ensure_import(&mut self, src: &Package, idx: PackageIndex) -> PackageIndex {
        let Some(ii) = idx.import_index() else { return PackageIndex::NONE };
        let imp = src.imports[ii].clone();
        let outer = if imp.outer.is_import() { self.ensure_import(src, imp.outer) } else { PackageIndex::NONE };
        let class_package = src.fname_to_string(imp.class_package);
        let class_name = src.fname_to_string(imp.class_name);
        let name = src.fname_to_string(imp.object_name);
        self.pkg.add_import(&class_package, &class_name, outer, &name)
    }

    /// Import for a script object (class, function…) exported by `src`.
    fn import_for_export(&mut self, src: &Editor, j: usize) -> PackageIndex {
        let sp = &src.pkg;
        let e = &sp.exports[j];
        let outer = if let Some(o) = e.outer.export_index() {
            self.import_for_export(src, o)
        } else if e.outer.is_import() {
            self.ensure_import(sp, e.outer)
        } else {
            // Top level: the source package itself.
            self.pkg.add_import("Core", "Package", PackageIndex::NONE, &src.name)
        };
        let class_name = sp.class_name_of(PackageIndex::export(j));
        let class_package = if e.class.is_null() { "Core".to_string() } else { sp.outermost_name(e.class) };
        let name = sp.object_name(PackageIndex::export(j));
        self.pkg.add_import(&class_package, &class_name, outer, &name)
    }

    /// Copy export `src_i` of `src`. `dst_outer`: `Some` places the copy
    /// under that object (`None` = top level), `None` keeps the source's
    /// outer chain (package groups are recreated). `deep` also copies every
    /// referenced export of `src`, otherwise references to objects the
    /// destination lacks are cleared. Returns the new export and warnings.
    pub fn copy_from(&mut self, src: &mut Editor, src_i: usize, dst_outer: Option<PackageIndex>, deep: bool) -> Result<(PackageIndex, Vec<String>)> {
        let mut warnings = Vec::new();
        let mut map: HashMap<usize, PackageIndex> = HashMap::new();
        let mut queue: VecDeque<usize> = VecDeque::new();
        let reserve = |ed: &mut Editor, src: &Editor, j: usize, map: &mut HashMap<usize, PackageIndex>, queue: &mut VecDeque<usize>| -> PackageIndex {
            if let Some(&d) = map.get(&j) {
                return d;
            }
            let sp = &src.pkg;
            let e = &sp.exports[j];
            let name = sp.object_name(PackageIndex::export(j));
            let export = ObjectExport { object_name: ed.pkg.intern_fname(&name), object_flags: e.object_flags, export_flags: e.export_flags, generation_net_object_count: e.generation_net_object_count.clone(), package_guid: e.package_guid, package_flags: e.package_flags, ..Default::default() };
            let d = ed.pkg.add_export(export, Vec::new());
            map.insert(j, d);
            queue.push_back(j);
            d
        };
        let root = reserve(self, src, src_i, &mut map, &mut queue);
        while let Some(j) = queue.pop_front() {
            let d = map[&j].export_index().unwrap();
            let sp_class = src.pkg.exports[j].class;
            let sp_super = src.pkg.exports[j].super_;
            let sp_outer = src.pkg.exports[j].outer;
            let sp_arch = src.pkg.exports[j].archetype;
            // Table references.
            let mut table = Vec::new();
            for (k, r) in [sp_class, sp_super, sp_outer, sp_arch].into_iter().enumerate() {
                let mapped = match dst_outer {
                    Some(o) if k == 2 && j == src_i => o,
                    _ => self.map_ref(src, r, deep, &mut map, &mut queue, &mut warnings, k == 2),
                };
                table.push(mapped);
            }
            {
                let e = &mut self.pkg.exports[d];
                e.class = table[0];
                e.super_ = table[1];
                e.outer = table[2];
                e.archetype = table[3];
            }
            // Payload.
            let parsed = src.parsed(j)?.clone();
            let out = match parsed {
                Parsed::Object(mut o) => {
                    o.net_index = -1;
                    let mut names: Vec<(FName, String)> = Vec::new();
                    {
                        struct Collect<'s>(&'s Package, &'s mut Vec<(FName, String)>);
                        impl Visitor for Collect<'_> {
                            fn name(&mut self, n: &mut FName) {
                                self.1.push((*n, self.0.fname_to_string(*n)));
                            }
                            fn index(&mut self, _: &mut PackageIndex) {}
                        }
                        let mut c = Collect(&src.pkg, &mut names);
                        o.visit(&mut c);
                    }
                    let name_map: HashMap<FName, FName> = names.into_iter().map(|(n, text)| (n, self.pkg.intern_fname(&text))).collect();
                    let mut refs: Vec<PackageIndex> = Vec::new();
                    {
                        struct CollectRefs<'s>(&'s mut Vec<PackageIndex>);
                        impl Visitor for CollectRefs<'_> {
                            fn name(&mut self, _: &mut FName) {}
                            fn index(&mut self, i: &mut PackageIndex) {
                                self.0.push(*i);
                            }
                        }
                        o.visit(&mut CollectRefs(&mut refs));
                    }
                    let mut ref_map: HashMap<PackageIndex, PackageIndex> = HashMap::new();
                    for r in refs {
                        if let std::collections::hash_map::Entry::Vacant(e) = ref_map.entry(r) {
                            let m = self.map_ref(src, r, deep, &mut map, &mut queue, &mut warnings, false);
                            e.insert(m);
                        }
                    }
                    struct Remap<'s>(&'s HashMap<FName, FName>, &'s HashMap<PackageIndex, PackageIndex>);
                    impl Visitor for Remap<'_> {
                        fn name(&mut self, n: &mut FName) {
                            if let Some(m) = self.0.get(n) {
                                *n = *m;
                            }
                        }
                        fn index(&mut self, i: &mut PackageIndex) {
                            if let Some(m) = self.1.get(i) {
                                *i = *m;
                            }
                        }
                    }
                    o.visit(&mut Remap(&name_map, &ref_map));
                    Parsed::Object(o)
                }
                Parsed::Empty => Parsed::Empty,
                Parsed::Raw { data, error } => {
                    warnings.push(format!("{}: copied verbatim (references inside not remapped: {error})", src.pkg.full_name(PackageIndex::export(j))));
                    Parsed::Raw { data, error }
                }
                Parsed::Script(_) => return Err(Error::Format(format!("{} is a script object and cannot be copied", src.pkg.full_name(PackageIndex::export(j))))),
            };
            let none = none_name(&self.pkg);
            let data = write_parsed(&out, self.layout, none);
            self.pkg.set_export_data(d, data);
            self.objects.insert(d, out);
        }
        Ok((root, warnings))
    }

    #[allow(clippy::too_many_arguments)]
    fn map_ref(&mut self, src: &Editor, r: PackageIndex, deep: bool, map: &mut HashMap<usize, PackageIndex>, queue: &mut VecDeque<usize>, warnings: &mut Vec<String>, is_outer: bool) -> PackageIndex {
        if r.is_null() {
            return PackageIndex::NONE;
        }
        if r.is_import() {
            return self.ensure_import(&src.pkg, r);
        }
        let j = r.export_index().unwrap();
        if let Some(&d) = map.get(&j) {
            return d;
        }
        let sp = &src.pkg;
        let class_name = sp.class_name_of(PackageIndex::export(j));
        let script = sp.exports[j].class.is_null() || kind_from_type_name(&class_name).is_some() || ["Function", "State", "ScriptStruct", "Enum", "Const"].iter().any(|c| c.eq_ignore_ascii_case(&class_name));
        if script {
            return self.import_for_export(src, j);
        }
        // Already present here at the same path?
        let path = sp.object_path(PackageIndex::export(j));
        if let Some(existing) = self.pkg.find_export(&path, Some(&class_name)) {
            let d = PackageIndex::export(existing);
            map.insert(j, d);
            return d;
        }
        if deep || is_outer || class_name.eq_ignore_ascii_case("Package") {
            let name = sp.object_name(PackageIndex::export(j));
            let e = &sp.exports[j];
            let export = ObjectExport { object_name: self.pkg.intern_fname(&name), object_flags: e.object_flags, export_flags: e.export_flags, generation_net_object_count: e.generation_net_object_count.clone(), package_guid: e.package_guid, package_flags: e.package_flags, ..Default::default() };
            let d = self.pkg.add_export(export, Vec::new());
            map.insert(j, d);
            queue.push_back(j);
            d
        } else {
            warnings.push(format!("reference to {} cleared (not in destination; use deep copy)", sp.full_name(PackageIndex::export(j))));
            PackageIndex::NONE
        }
    }

    // ----- properties -------------------------------------------------------

    /// Render a property (or the whole object when `path` is empty).
    pub fn get(&mut self, i: usize, path: &str) -> Result<String> {
        let obj = self.object(i)?.clone();
        let d = Dumper::new(&self.pkg);
        let mut out = String::new();
        if path.is_empty() {
            d.object(&obj, &mut out);
            return Ok(out);
        }
        let segs = parse_path(path)?;
        let v = find_value(&obj, &self.pkg, &segs).ok_or_else(|| Error::Format(format!("no property {path}")))?;
        match v {
            Found::Property(p) => d.property(p, 0, &mut out),
            Found::Value(v) => out.push_str(&d.scalar(v)),
        }
        Ok(out.trim_end().to_string())
    }

    /// Set a property by path from its text form.
    pub fn set(&mut self, i: usize, path: &str, text: &str) -> Result<()> {
        let segs = parse_path(path)?;
        let class = {
            let o = self.object(i)?;
            o.class
        };
        let mut obj = self.object(i)?.clone();
        set_value(self.db, &mut self.pkg, &mut obj, class, &segs, text)?;
        self.replace_object(i, obj)
    }

    /// Add, duplicate or remove an array element, or remove a property (back
    /// to its class default), at a property path (`Arr`, `S.Arr`, `Arr[2]`).
    pub fn edit_list(&mut self, i: usize, path: &str, op: ListOp) -> Result<()> {
        let segs = parse_path(path)?;
        let mut obj = self.object(i)?.clone();
        edit_props(&self.pkg, &mut obj.properties, &segs, op)?;
        self.replace_object(i, obj)
    }

    // ----- save -------------------------------------------------------------

    /// Serialise every touched object into the package's export data (they
    /// stay dirty): for readers that work from `pkg` rather than the parsed
    /// objects, such as previews of unsaved edits.
    pub fn flush(&mut self) {
        let none = none_name(&self.pkg);
        let dirty: Vec<usize> = self.dirty.iter().copied().collect();
        for i in dirty {
            if let Some(p) = self.objects.get(&i) {
                let data = write_parsed(p, self.layout, none);
                self.pkg.set_export_data(i, data);
            }
        }
    }

    /// Write touched objects back and save, compressed like the original.
    pub fn save(&mut self, out: impl AsRef<Path>) -> Result<()> {
        self.flush();
        self.pkg.save(out, &WriteOptions { compress: self.was_compressed, obscure: self.obscured })?;
        self.dirty.clear();
        self.rebuilt = false;
        Ok(())
    }

    /// The bytes Save would write, without saving: the package stays dirty.
    pub fn bytes(&mut self) -> Vec<u8> {
        self.flush();
        self.pkg.to_bytes(&WriteOptions { compress: self.was_compressed, obscure: self.obscured })
    }

    /// The package was saved to its own file: everything journaled so far is
    /// on disk now (undoing it will dirty the package again).
    pub fn mark_saved(&mut self) {
        self.end_undo();
        for e in &mut self.undo {
            e.saved = true;
        }
    }
}

// ----------------------------------------------------------------------------
// Property paths
// ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub name: String,
    pub index: Option<usize>,
}

/// `Name`, `Name[3]`, `Outer.Inner[2].Leaf`.
pub fn parse_path(path: &str) -> Result<Vec<Segment>> {
    let mut out = Vec::new();
    for part in path.split('.') {
        let part = part.trim();
        if part.is_empty() {
            return Err(Error::Format(format!("bad property path {path}")));
        }
        if let Some(b) = part.find('[') {
            let idx = part[b + 1..].trim_end_matches(']').parse::<usize>().map_err(|_| Error::Format(format!("bad index in {part}")))?;
            out.push(Segment { name: part[..b].to_string(), index: Some(idx) });
        } else {
            out.push(Segment { name: part.to_string(), index: None });
        }
    }
    Ok(out)
}

pub enum Found<'v> {
    Property(&'v Property),
    Value(&'v Value),
}

fn find_in_list<'v>(props: &'v [Property], pkg: &Package, seg: &Segment, is_array_kind: impl Fn(&Property) -> bool) -> Option<Found<'v>> {
    let matches: Vec<&Property> = props.iter().filter(|p| pkg.fname_to_string(p.name).eq_ignore_ascii_case(&seg.name)).collect();
    let first = *matches.first()?;
    match seg.index {
        None => Some(Found::Property(first)),
        Some(ix) => {
            if is_array_kind(first) {
                match &first.value {
                    Value::Array(items) => items.get(ix).map(Found::Value),
                    _ => None,
                }
            } else {
                matches.into_iter().find(|p| p.array_index as usize == ix).map(Found::Property)
            }
        }
    }
}

pub fn find_value<'v>(obj: &'v Object, pkg: &Package, segs: &[Segment]) -> Option<Found<'v>> {
    let mut cur = find_in_list(&obj.properties, pkg, &segs[0], |p| p.kind == PropertyKind::Array)?;
    for seg in &segs[1..] {
        let value = match cur {
            Found::Property(p) => &p.value,
            Found::Value(v) => v,
        };
        cur = match value {
            Value::Struct(StructValue::Tagged(ps)) => find_in_list(ps, pkg, seg, |p| p.kind == PropertyKind::Array)?,
            Value::Struct(StructValue::Binary(ms)) => {
                let m = ms.iter().find(|m| m.name.eq_ignore_ascii_case(&seg.name))?;
                let ix = seg.index.unwrap_or(0);
                if m.kind == PropertyKind::Array {
                    match m.elems.first()? {
                        Value::Array(items) => Found::Value(items.get(ix)?),
                        _ => return None,
                    }
                } else {
                    Found::Value(m.elems.get(ix)?)
                }
            }
            _ => return None,
        };
    }
    Some(cur)
}

/// Parse a scalar from text according to the property kind.
fn parse_scalar(pkg: &mut Package, kind: PropertyKind, has_enum: bool, text: &str) -> Result<Value> {
    let t = text.trim();
    Ok(match kind {
        PropertyKind::Int => Value::Int(t.parse().map_err(|_| Error::Format(format!("not an integer: {t}")))?),
        PropertyKind::Float => Value::Float(t.parse().map_err(|_| Error::Format(format!("not a number: {t}")))?),
        PropertyKind::Bool => Value::Bool(matches!(t.to_ascii_lowercase().as_str(), "true" | "1" | "yes")),
        PropertyKind::Byte => {
            if has_enum || t.parse::<u8>().is_err() {
                Value::Enum(pkg.intern_fname(t))
            } else {
                Value::Byte(t.parse().unwrap())
            }
        }
        PropertyKind::Name => Value::Name(pkg.intern_fname(t.trim_matches('\''))),
        PropertyKind::Str => Value::Str(t.trim_matches('"').to_string()),
        PropertyKind::Object | PropertyKind::Component | PropertyKind::SafeObject | PropertyKind::Class | PropertyKind::Interface => {
            if t.eq_ignore_ascii_case("none") {
                Value::Object(PackageIndex::NONE)
            } else {
                // `Class'Path'` or `Path`.
                let (class, path) = match (t.find('\''), t.ends_with('\'')) {
                    (Some(q), true) => (Some(&t[..q]), &t[q + 1..t.len() - 1]),
                    _ => (None, t),
                };
                if let Some(e) = pkg.find_export(path, class) {
                    Value::Object(PackageIndex::export(e))
                } else if let Some(i) = pkg.find_import(path, class) {
                    Value::Object(PackageIndex::import(i))
                } else {
                    return Err(Error::Format(format!("no object {path} in this package (copy it in first)")));
                }
            }
        }
        PropertyKind::Delegate => Value::Delegate { object: PackageIndex::NONE, function: pkg.intern_fname(t) },
        PropertyKind::Struct | PropertyKind::Array | PropertyKind::Map => return Err(Error::Format("set a member or element, not the whole struct/array".into())),
    })
}

/// Create a tagged property for `name` declared in `scope`.
fn new_property(db: &ClassDb, pkg: &mut Package, scope: Option<ObjRef>, name: &str, array_index: i32) -> Result<(Property, Option<ObjRef>)> {
    let pdef = scope.and_then(|s| db.find_property(s, name)).ok_or_else(|| Error::Format(format!("unknown property {name} (not declared on this class)")))?;
    let kind = db.property_kind(pdef).unwrap();
    let ref1 = db.property_ref1(pdef);
    let struct_name = if kind == PropertyKind::Struct { ref1.map(|s| pkg.intern_fname(&db.name(s))) } else { None };
    let enum_name = if kind == PropertyKind::Byte { Some(match ref1 { Some(e) => pkg.intern_fname(&db.name(e)), None => pkg.intern_fname("None") }) } else { None };
    let type_name = tag_type_name(pkg, kind);
    let value = match kind {
        PropertyKind::Int => Value::Int(0),
        PropertyKind::Float => Value::Float(0.0),
        PropertyKind::Bool => Value::Bool(false),
        PropertyKind::Byte => if ref1.is_some() { Value::Enum(pkg.intern_fname("None")) } else { Value::Byte(0) },
        PropertyKind::Name => Value::Name(pkg.intern_fname("None")),
        PropertyKind::Str => Value::Str(String::new()),
        PropertyKind::Array => Value::Array(Vec::new()),
        PropertyKind::Struct => Value::Struct(StructValue::Tagged(Vec::new())),
        PropertyKind::Delegate => Value::Delegate { object: PackageIndex::NONE, function: pkg.intern_fname("None") },
        PropertyKind::Map => return Err(Error::Format("map properties cannot be created".into())),
        _ => Value::Object(PackageIndex::NONE),
    };
    Ok((Property { name: pkg.intern_fname(name), array_index, kind, type_name, struct_name, enum_name, value }, Some(pdef)))
}

fn set_in_list(db: &ClassDb, pkg: &mut Package, props: &mut Vec<Property>, scope: Option<ObjRef>, segs: &[Segment], text: &str) -> Result<()> {
    let seg = &segs[0];
    let pos = props.iter().position(|p| pkg.fname_to_string(p.name).eq_ignore_ascii_case(&seg.name) && (seg.index.is_none() || p.kind == PropertyKind::Array || p.array_index as usize == seg.index.unwrap_or(0)));
    let pdef = scope.and_then(|s| db.find_property(s, &seg.name));
    let pos = match pos {
        Some(p) => p,
        None => {
            let static_index = match (pdef.map(|p| db.property_kind(p)), seg.index) {
                (Some(Some(PropertyKind::Array)), _) | (_, None) => 0,
                (_, Some(ix)) => ix as i32,
            };
            let (p, _) = new_property(db, pkg, scope, &seg.name, static_index)?;
            props.push(p);
            props.len() - 1
        }
    };
    let prop = &mut props[pos];
    let kind = prop.kind;
    let has_enum = prop.enum_name.map(|e| !pkg.is_none(e)).unwrap_or(false) || pdef.and_then(|p| db.property_ref1(p)).is_some() && kind == PropertyKind::Byte;
    if segs.len() == 1 {
        if kind == PropertyKind::Array {
            let Some(ix) = seg.index else { return Err(Error::Format("give an element index to set an array element".into())) };
            let inner = pdef.and_then(|p| db.property_ref1(p));
            let ik = inner.and_then(|p| db.property_kind(p)).ok_or_else(|| Error::Format("array element type unknown".into()))?;
            let ienum = inner.and_then(|p| db.property_ref1(p)).is_some() && ik == PropertyKind::Byte;
            let v = parse_scalar(pkg, ik, ienum, text)?;
            if let Value::Array(items) = &mut prop.value {
                while items.len() <= ix {
                    items.push(v.clone());
                }
                items[ix] = v;
                return Ok(());
            }
            return Err(Error::Format("array value is raw; cannot edit".into()));
        }
        prop.value = parse_scalar(pkg, kind, has_enum, text)?;
        if let Value::Enum(_) = &prop.value {
            if prop.enum_name.map(|e| pkg.is_none(e)).unwrap_or(true) {
                if let Some(e) = pdef.and_then(|p| db.property_ref1(p)) {
                    prop.enum_name = Some(pkg.intern_fname(&db.name(e)));
                }
            }
        }
        return Ok(());
    }
    // Descend.
    let inner_scope = pdef.and_then(|p| db.property_ref1(p));
    match (&mut prop.value, kind) {
        (Value::Struct(StructValue::Tagged(ps)), PropertyKind::Struct) => set_in_list(db, pkg, ps, inner_scope, &segs[1..], text),
        (Value::Struct(StructValue::Binary(ms)), PropertyKind::Struct) => {
            let m = ms.iter_mut().find(|m| m.name.eq_ignore_ascii_case(&segs[1].name)).ok_or_else(|| Error::Format(format!("no member {}", segs[1].name)))?;
            if segs.len() != 2 {
                return Err(Error::Format("nested members inside binary structs are not editable yet".into()));
            }
            let mdef = inner_scope.and_then(|s| db.find_property(s, &m.name));
            let menum = mdef.and_then(|p| db.property_ref1(p)).is_some() && m.kind == PropertyKind::Byte;
            let ix = segs[1].index.unwrap_or(0);
            let v = parse_scalar(pkg, m.kind, menum, text)?;
            *m.elems.get_mut(ix).ok_or_else(|| Error::Format("index out of range".into()))? = v;
            Ok(())
        }
        (Value::Array(items), PropertyKind::Array) => {
            let ix = seg.index.ok_or_else(|| Error::Format("give an element index".into()))?;
            let inner = inner_scope.ok_or_else(|| Error::Format("array element type unknown".into()))?;
            let elem_scope = db.property_ref1(inner);
            let len = items.len();
            let it = items.get_mut(ix).ok_or_else(|| Error::Format(format!("array has {len} elements")))?;
            match it {
                Value::Struct(StructValue::Tagged(ps)) => set_in_list(db, pkg, ps, elem_scope, &segs[1..], text),
                _ => Err(Error::Format("element is not a struct".into())),
            }
        }
        _ => Err(Error::Format(format!("{} is not a struct or array", seg.name))),
    }
}

/// Structural edits of a property list (see `Editor::edit_list`).
#[derive(Clone, Copy, Debug)]
pub enum ListOp {
    /// Append a copy of the array's last element.
    Add,
    /// Insert a copy of element `n` after it.
    Duplicate(usize),
    /// Remove element `n`.
    Remove(usize),
    /// Remove the property itself: the object falls back to the class default.
    RemoveProperty,
}

fn edit_props(pkg: &Package, props: &mut Vec<Property>, segs: &[Segment], op: ListOp) -> Result<()> {
    let seg = &segs[0];
    let pos = props
        .iter()
        .position(|p| pkg.fname_to_string(p.name).eq_ignore_ascii_case(&seg.name) && (seg.index.is_none() || p.kind == PropertyKind::Array || p.array_index as usize == seg.index.unwrap_or(0)))
        .ok_or_else(|| Error::Format(format!("no property {} (it has its class default)", seg.name)))?;
    let last = segs.len() == 1;
    if last {
        if let ListOp::RemoveProperty = op {
            props.remove(pos);
            return Ok(());
        }
        let Value::Array(items) = &mut props[pos].value else {
            return Err(Error::Format(format!("{} is not an array", seg.name)));
        };
        match op {
            ListOp::Add => {
                let v = items.last().cloned().ok_or_else(|| Error::Format("the array is empty: set element [0] first".into()))?;
                items.push(v);
            }
            ListOp::Duplicate(n) => {
                let v = items.get(n).cloned().ok_or_else(|| Error::Format(format!("no element {n}")))?;
                items.insert(n + 1, v);
            }
            ListOp::Remove(n) => {
                if n >= items.len() {
                    return Err(Error::Format(format!("no element {n}")));
                }
                items.remove(n);
            }
            ListOp::RemoveProperty => unreachable!(),
        }
        return Ok(());
    }
    match &mut props[pos].value {
        Value::Struct(StructValue::Tagged(ps)) => edit_props(pkg, ps, &segs[1..], op),
        Value::Array(items) => {
            let ix = seg.index.ok_or_else(|| Error::Format("give an element index".into()))?;
            match items.get_mut(ix) {
                Some(Value::Struct(StructValue::Tagged(ps))) => edit_props(pkg, ps, &segs[1..], op),
                _ => Err(Error::Format("element is not a struct".into())),
            }
        }
        _ => Err(Error::Format(format!("{} is not a struct or array", seg.name))),
    }
}

pub fn set_value(db: &ClassDb, pkg: &mut Package, obj: &mut Object, class: Option<ObjRef>, segs: &[Segment], text: &str) -> Result<()> {
    set_in_list(db, pkg, &mut obj.properties, class, segs, text)
}

/// Objects referencing export `i` (scanning every parsed export).
pub fn referrers(ed: &mut Editor, i: usize) -> Vec<usize> {
    let target = PackageIndex::export(i);
    let mut out = Vec::new();
    for j in 0..ed.pkg.exports.len() {
        if j == i {
            continue;
        }
        let e = &ed.pkg.exports[j];
        if [e.class, e.super_, e.outer, e.archetype].contains(&target) {
            out.push(j);
            continue;
        }
        if let Ok(Parsed::Object(o)) = ed.parsed(j) {
            if references(o).contains(&target) {
                out.push(j);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_property_paths() {
        let p = parse_path("TextureParameterValues[3].ParameterValue").unwrap();
        assert_eq!(p, vec![Segment { name: "TextureParameterValues".into(), index: Some(3) }, Segment { name: "ParameterValue".into(), index: None }]);
        assert!(parse_path("a..b").is_err());
        assert!(parse_path("a[x]").is_err());
    }
}
