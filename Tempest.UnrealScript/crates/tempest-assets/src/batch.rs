//! Bulk operations: find & replace over a package's property values and
//! object names, and importing a folder of `<package>.<object>.<ext>` files
//! into their packages in one go.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tempest_upk::io::{Error, PackageIndex, Result};
use tempest_upk::{Package, PackageFile};

use crate::edit::Editor;
use crate::media;
use crate::mesh_import;
use crate::object::{Native, Parsed};
use crate::reflect::ClassDb;
use crate::texture_ops::{self, ImportOptions, ImportSource};
use crate::value::{Property, StructValue, Value};

// ----------------------------------------------------------------------------
// Find & replace
// ----------------------------------------------------------------------------

/// One value (or object name) that contains the query.
#[derive(Clone, Debug)]
pub struct Match {
    pub index: usize,
    /// Object path of the export.
    pub object: String,
    /// Property path (`Outer.Inner[2].Leaf`); `None` for the object's own name.
    pub prop: Option<String>,
    /// `str`, `name`, `enum`, `object` or `objectname`.
    pub kind: &'static str,
    pub current: String,
    pub replacement: String,
}

/// Case-insensitive substring replacement (every occurrence).
pub fn replace_ci(text: &str, query: &str, replacement: &str) -> String {
    if query.is_empty() {
        return text.to_string();
    }
    let lower = text.to_ascii_lowercase();
    let q = query.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(pos) = lower[i..].find(&q) {
        out.push_str(&text[i..i + pos]);
        out.push_str(replacement);
        i += pos + q.len();
    }
    out.push_str(&text[i..]);
    out
}

fn contains_ci(text: &str, query: &str) -> bool {
    text.to_ascii_lowercase().contains(&query.to_ascii_lowercase())
}

fn scalar_text(pkg: &Package, v: &Value) -> Option<(&'static str, String)> {
    match v {
        Value::Str(s) => Some(("str", s.clone())),
        Value::Name(n) => Some(("name", pkg.fname_to_string(*n))),
        Value::Enum(n) => Some(("enum", pkg.fname_to_string(*n))),
        Value::Object(i) if !i.is_null() => Some(("object", pkg.object_path(*i))),
        _ => None,
    }
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() { key.to_string() } else { format!("{prefix}.{key}") }
}

/// Visit every editable scalar with its property path.
fn walk(pkg: &Package, props: &[Property], prefix: &str, f: &mut dyn FnMut(String, &Value)) {
    for p in props {
        let name = pkg.fname_to_string(p.name);
        let key = if p.array_index > 0 { format!("{name}[{}]", p.array_index) } else { name };
        let path = join(prefix, &key);
        match &p.value {
            Value::Struct(StructValue::Tagged(ps)) => walk(pkg, ps, &path, f),
            Value::Struct(StructValue::Binary(ms)) => {
                for m in ms {
                    for (j, e) in m.elems.iter().enumerate() {
                        let mpath = if m.elems.len() == 1 { join(&path, &m.name) } else { format!("{}.{}[{j}]", path, m.name) };
                        if scalar_text(pkg, e).is_some() {
                            f(mpath, e);
                        }
                    }
                }
            }
            Value::Array(items) => {
                for (j, it) in items.iter().enumerate() {
                    let ipath = format!("{path}[{j}]");
                    match it {
                        Value::Struct(StructValue::Tagged(ps)) => walk(pkg, ps, &ipath, f),
                        v if scalar_text(pkg, v).is_some() => f(ipath, v),
                        _ => {}
                    }
                }
            }
            v => {
                if scalar_text(pkg, v).is_some() {
                    f(path, v);
                }
            }
        }
    }
}

/// Every value (and, with `names`, object name) of the package containing
/// `query`, with the text it would become.
pub fn find(ed: &mut Editor, query: &str, replacement: &str, names: bool) -> Vec<Match> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let n = ed.pkg.exports.len();
    for i in 0..n {
        let e = &ed.pkg.exports[i];
        if e.class.is_null() && e.serial_size == 0 {
            continue;
        }
        let object = ed.pkg.object_path(PackageIndex::export(i));
        if names {
            let name = ed.pkg.object_name(PackageIndex::export(i));
            if contains_ci(&name, query) {
                out.push(Match { index: i, object: object.clone(), prop: None, kind: "objectname", current: name.clone(), replacement: replace_ci(&name, query, replacement) });
            }
        }
        let Ok(Parsed::Object(obj)) = ed.parsed(i) else { continue };
        let obj = obj.clone();
        let pkg = &ed.pkg;
        walk(pkg, &obj.properties, "", &mut |path, v| {
            if let Some((kind, text)) = scalar_text(pkg, v) {
                if contains_ci(&text, query) {
                    out.push(Match { index: i, object: object.clone(), prop: Some(path), kind, current: text.clone(), replacement: replace_ci(&text, query, replacement) });
                }
            }
        });
    }
    out
}

/// Apply matches; returns the failures with their reasons.
pub fn replace_all(ed: &mut Editor, matches: &[Match]) -> Vec<(Match, String)> {
    let mut errors = Vec::new();
    // Renames first: object-reference matches are re-resolved from their new
    // path, which only exists once the renamed objects carry it.
    for m in matches.iter().filter(|m| m.prop.is_none()) {
        match ed.rename(m.index, &m.replacement) {
            Ok(()) => ed.mark_dirty(m.index),
            Err(e) => errors.push((m.clone(), e.to_string())),
        }
    }
    for m in matches.iter().filter(|m| m.prop.is_some()) {
        let path = m.prop.as_deref().unwrap();
        if m.kind == "object" {
            // A reference whose target was just renamed already points at it.
            if let (Some(a), Some(b)) = (ed.pkg.find_export(&m.replacement, None), ed.pkg.find_export(&m.current, None)) {
                if a == b {
                    continue;
                }
            }
            if ed.pkg.find_export(&m.replacement, None).is_none() && ed.pkg.find_import(&m.replacement, None).is_none() {
                errors.push((m.clone(), format!("no object {} in this package (copy it in first)", m.replacement)));
                continue;
            }
        }
        if let Err(e) = ed.set(m.index, path, &m.replacement) {
            errors.push((m.clone(), e.to_string()));
        }
    }
    errors
}

// ----------------------------------------------------------------------------
// Importing one file into one export
// ----------------------------------------------------------------------------

/// What a file extension imports as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportKind {
    Texture,
    Mesh,
    Sound,
    Swf,
    Raw,
}

impl ImportKind {
    pub fn from_extension(ext: &str) -> Option<ImportKind> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "png" | "dds" => ImportKind::Texture,
            "glb" | "gltf" | "obj" | "fbx" => ImportKind::Mesh,
            "ogg" => ImportKind::Sound,
            "gfx" | "swf" => ImportKind::Swf,
            "bin" | "raw" => ImportKind::Raw,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            ImportKind::Texture => "texture",
            ImportKind::Mesh => "mesh",
            ImportKind::Sound => "sound",
            ImportKind::Swf => "movie",
            ImportKind::Raw => "raw payload",
        }
    }

    /// Does an export of `class` accept this kind?
    pub fn accepts(self, db: &ClassDb, class: &str) -> bool {
        match self {
            ImportKind::Texture => db.class_is_a(class, "Texture2D"),
            ImportKind::Mesh => db.class_is_a(class, "StaticMesh") || db.class_is_a(class, "SkeletalMesh"),
            ImportKind::Sound => db.class_is_a(class, "SoundNodeWave"),
            ImportKind::Swf => db.class_is_a(class, "SwfMovie") || db.class_is_a(class, "GFxMovieInfo"),
            ImportKind::Raw => true,
        }
    }
}

/// Replace export `i` from `file` according to its extension. Returns a
/// one-line description of what was done.
pub fn import_file(db: &ClassDb, ed: &mut Editor, i: usize, file: &Path) -> Result<String> {
    let ext = file.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    let kind = ImportKind::from_extension(&ext).ok_or_else(|| Error::Format(format!("unsupported file type .{ext}")))?;
    let bytes = std::fs::read(file).map_err(|e| Error::Format(format!("{}: {e}", file.display())))?;
    if kind == ImportKind::Raw {
        let n = bytes.len();
        ed.set_raw(i, bytes)?;
        return Ok(format!("raw payload replaced ({n} bytes)"));
    }
    let mut o = ed.object(i)?.clone();
    let text = match kind {
        ImportKind::Texture => {
            if !matches!(o.native, Native::Texture2D(_)) {
                return Err(Error::Format("not a decoded Texture2D".into()));
            }
            let source = if ext.eq_ignore_ascii_case("dds") {
                let d = crate::dds::read_dds(&bytes)?;
                ImportSource::Encoded { format: d.format, width: d.width, height: d.height, mips: d.mips }
            } else {
                let (w, h, rgba) = crate::dds::read_png(&bytes)?;
                ImportSource::Rgba { width: w, height: h, rgba }
            };
            texture_ops::import(db, &mut ed.pkg, i, &mut o, source, &ImportOptions { mips: None, format: None })?;
            "texture replaced".to_string()
        }
        ImportKind::Mesh => {
            let r = mesh_import::import(&ed.pkg, &mut o, file, &bytes, 0)?;
            format!("mesh replaced ({} vertices, {} triangles{})", r.vertices, r.triangles, if r.warnings.is_empty() { String::new() } else { format!("; {}", r.warnings.join("; ")) })
        }
        ImportKind::Sound => {
            media::sound_import(&mut ed.pkg, &mut o, &bytes)?;
            "sound replaced".to_string()
        }
        ImportKind::Swf => {
            media::swf_import(&mut ed.pkg, &mut o, &bytes)?;
            "movie replaced".to_string()
        }
        ImportKind::Raw => unreachable!(),
    };
    ed.replace_object(i, o)?;
    ed.mark_dirty(i);
    Ok(text)
}

// ----------------------------------------------------------------------------
// Batch import of a folder
// ----------------------------------------------------------------------------

/// One file of the batch and what happens to it.
#[derive(Clone, Debug)]
pub struct BatchEntry {
    pub file: PathBuf,
    /// Package name parsed from the file name.
    pub package: String,
    /// Package file resolved in the workspace.
    pub package_path: Option<PathBuf>,
    /// Export index and object path once resolved.
    pub export: Option<(usize, String)>,
    pub kind: Option<ImportKind>,
    /// `true` once the import (or the dry run resolution) succeeded.
    pub ok: bool,
    pub message: String,
}

/// Files of `dir` (optionally recursive) whose names look like
/// `<package>.<object>.<ext>`, sorted.
pub fn collect_files(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if recursive {
                    stack.push(p);
                }
                continue;
            }
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with('.') || name.matches('.').count() < 2 {
                continue;
            }
            out.push(p);
        }
    }
    out.sort();
    out
}

/// Packages of the workspace that contain an export whose full object path
/// is `path` (case-insensitive): the fallback when a file's first name part is
/// not a package file but the top-level group of the object (cooked seekfree
/// packages keep their content under such groups, e.g.
/// `loadingart_zhin.loadingart_zhin_conqueror` inside `ZHIN_TRANS_ASSETS_SF`).
pub type PathLookup<'a> = dyn FnMut(&str) -> Vec<(String, PathBuf)> + 'a;

/// A [`PathLookup`] that opens every workspace package once and scans its
/// export table (what the CLI uses; the Studio answers from its search index).
pub fn scan_lookup(packages: &[(String, PathBuf)]) -> impl FnMut(&str) -> Vec<(String, PathBuf)> + '_ {
    let mut paths: Vec<Option<Vec<String>>> = Vec::new();
    move |path: &str| {
        if paths.is_empty() {
            paths = packages
                .iter()
                .map(|(_, p)| {
                    PackageFile::open(p).ok().map(|pf| {
                        let pkg = pf.tables();
                        (0..pkg.exports.len()).filter(|&i| pkg.exports[i].serial_size != 0).map(|i| pkg.object_path(PackageIndex::export(i)).to_ascii_lowercase()).collect()
                    })
                })
                .collect();
        }
        let want = path.to_ascii_lowercase();
        packages.iter().zip(&paths).filter(|(_, ps)| ps.as_ref().is_some_and(|ps| ps.contains(&want))).map(|(p, _)| p.clone()).collect()
    }
}

/// Resolve the files against the workspace packages (`(name, path)` pairs)
/// without changing anything. A file `<package>.<object>.<ext>` goes to the
/// package file called `<package>` when there is one and the object resolves
/// in it; otherwise the whole `<package>.<object>` is taken as an object path
/// and the file goes to every package `lookup` finds it in.
pub fn plan(db: &ClassDb, packages: &[(String, PathBuf)], files: &[PathBuf], lookup: &mut PathLookup) -> Vec<BatchEntry> {
    let mut tables: BTreeMap<String, Option<PackageFile>> = BTreeMap::new();
    let mut out = Vec::new();
    for file in files {
        let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let ext = file.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        let stem = &name[..name.len() - ext.len() - 1];
        let Some((pkg_name, object)) = stem.split_once('.') else {
            out.push(BatchEntry { file: file.clone(), package: String::new(), package_path: None, export: None, kind: None, ok: false, message: "file name is not <package>.<object>.<ext>".into() });
            continue;
        };
        let kind = ImportKind::from_extension(&ext);
        let Some(kind) = kind else {
            out.push(BatchEntry { file: file.clone(), package: pkg_name.to_string(), package_path: None, export: None, kind, ok: false, message: format!("unsupported file type .{ext}") });
            continue;
        };
        let named = packages.iter().find(|(n, _)| n.eq_ignore_ascii_case(pkg_name));
        let direct = named.map(|(pname, ppath)| resolve_in(db, &mut tables, pname, ppath, object, kind, file));
        if direct.as_ref().is_some_and(|e| e.ok) {
            out.push(direct.unwrap());
            continue;
        }
        let holders = lookup(stem);
        if holders.is_empty() {
            out.push(direct.unwrap_or_else(|| BatchEntry { file: file.clone(), package: pkg_name.to_string(), package_path: None, export: None, kind: Some(kind), ok: false, message: format!("no package {pkg_name} in the workspace, and no object {stem} in any package") }));
            continue;
        }
        for (pname, ppath) in &holders {
            out.push(resolve_in(db, &mut tables, pname, ppath, stem, kind, file));
        }
    }
    out
}

/// The entry of `file` for `object` inside the package `pname` at `ppath`.
fn resolve_in(db: &ClassDb, tables: &mut BTreeMap<String, Option<PackageFile>>, pname: &str, ppath: &Path, object: &str, kind: ImportKind, file: &Path) -> BatchEntry {
    let mut entry = BatchEntry { file: file.to_path_buf(), package: pname.to_string(), package_path: Some(ppath.to_path_buf()), export: None, kind: Some(kind), ok: false, message: String::new() };
    let pf = tables.entry(pname.to_ascii_lowercase()).or_insert_with(|| PackageFile::open(ppath).ok());
    let Some(pf) = pf else {
        entry.message = format!("cannot open {}", ppath.display());
        return entry;
    };
    match resolve_object(db, pf.tables(), object, kind) {
        Ok((i, path)) => {
            entry.message = format!("{} → {}", kind.label(), path);
            entry.export = Some((i, path));
            entry.ok = true;
        }
        Err(e) => entry.message = e.to_string(),
    }
    entry
}

/// The export `object` names: a full object path, or a unique object name
/// among exports that accept `kind`.
fn resolve_object(db: &ClassDb, pkg: &Package, object: &str, kind: ImportKind) -> Result<(usize, String)> {
    if let Some(i) = pkg.find_export(object, None) {
        let class = pkg.class_name_of(PackageIndex::export(i));
        if !kind.accepts(db, &class) {
            return Err(Error::Format(format!("{object} is a {class}, not something a .{} file can replace", kind.label())));
        }
        return Ok((i, pkg.object_path(PackageIndex::export(i))));
    }
    let mut candidates: Vec<usize> = Vec::new();
    for i in 0..pkg.exports.len() {
        let e = &pkg.exports[i];
        if e.serial_size == 0 || !pkg.object_name(PackageIndex::export(i)).eq_ignore_ascii_case(object) {
            continue;
        }
        if kind.accepts(db, &pkg.class_name_of(PackageIndex::export(i))) {
            candidates.push(i);
        }
    }
    match candidates.len() {
        0 => Err(Error::Format(format!("no {} named {object} in the package", kind.label()))),
        1 => Ok((candidates[0], pkg.object_path(PackageIndex::export(candidates[0])))),
        _ => Err(Error::Format(format!("{object} is ambiguous; use the full path: {}", candidates.iter().map(|&i| pkg.object_path(PackageIndex::export(i))).collect::<Vec<_>>().join(", ")))),
    }
}

/// Import every resolvable file, package by package, and save each touched
/// package in place or into `out_dir`. `progress(done, total, file)` is
/// called before each file.
/// `only`: when given, the package files (lowercased paths) to write; entries
/// of other packages are reported as not selected and left alone.
#[allow(clippy::too_many_arguments)]
pub fn run(db: &ClassDb, packages: &[(String, PathBuf)], files: &[PathBuf], lookup: &mut PathLookup, out_dir: Option<&Path>, only: Option<&std::collections::HashSet<String>>, progress: &mut dyn FnMut(usize, usize, &str)) -> Vec<BatchEntry> {
    let mut entries = plan(db, packages, files, lookup);
    if let Some(only) = only {
        for e in entries.iter_mut() {
            if e.ok && !e.package_path.as_ref().is_some_and(|p| only.contains(&p.to_string_lossy().to_ascii_lowercase())) {
                e.ok = false;
                e.message = "package not selected".to_string();
            }
        }
    }
    let total = entries.len();
    let mut done = 0usize;
    // Group by package, preserving file order.
    let mut by_package: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (k, e) in entries.iter().enumerate() {
        if e.ok {
            by_package.entry(e.package.to_ascii_lowercase()).or_default().push(k);
        } else {
            done += 1;
        }
    }
    for (_, idxs) in by_package {
        let path = entries[idxs[0]].package_path.clone().unwrap();
        let mut ed = match Editor::open(db, &path) {
            Ok(ed) => ed,
            Err(e) => {
                for k in idxs {
                    entries[k].ok = false;
                    entries[k].message = format!("cannot open {}: {e}", path.display());
                }
                continue;
            }
        };
        let mut touched = false;
        for &k in &idxs {
            done += 1;
            progress(done, total, &entries[k].file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            let (i, _) = entries[k].export.clone().unwrap();
            match import_file(db, &mut ed, i, &entries[k].file) {
                Ok(msg) => {
                    entries[k].message = format!("{}: {msg}", entries[k].export.as_ref().unwrap().1);
                    touched = true;
                }
                Err(e) => {
                    entries[k].ok = false;
                    entries[k].message = format!("{}: {e}", entries[k].export.as_ref().unwrap().1);
                }
            }
        }
        if !touched {
            continue;
        }
        let out = match out_dir {
            Some(d) => d.join(path.file_name().unwrap()),
            None => path.clone(),
        };
        if let Some(d) = out_dir {
            if let Err(e) = std::fs::create_dir_all(d) {
                for k in &idxs {
                    entries[*k].ok = false;
                    entries[*k].message = format!("cannot create {}: {e}", d.display());
                }
                continue;
            }
        }
        if let Err(e) = ed.save(&out) {
            for k in &idxs {
                entries[*k].ok = false;
                entries[*k].message = format!("save failed for {}: {e}", out.display());
            }
        } else {
            for k in &idxs {
                if entries[*k].ok {
                    entries[*k].message.push_str(&format!(" → {}", out.display()));
                }
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_is_case_insensitive_and_global() {
        assert_eq!(replace_ci("Skin00A_and_skin00a", "skin00a", "skin01a"), "skin01a_and_skin01a");
        assert_eq!(replace_ci("nothing", "zzz", "y"), "nothing");
        assert_eq!(replace_ci("abc", "", "y"), "abc");
    }

    #[test]
    fn import_kinds_follow_extensions() {
        assert_eq!(ImportKind::from_extension("DDS"), Some(ImportKind::Texture));
        assert_eq!(ImportKind::from_extension("glb"), Some(ImportKind::Mesh));
        assert_eq!(ImportKind::from_extension("txt"), None);
    }
}
