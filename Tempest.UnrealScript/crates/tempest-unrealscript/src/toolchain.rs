//! Whole-package operations shared by the `uscript` CLI and the Studio
//! backend (`upk serve`): decompile every class of a package to source,
//! compile a source directory (optionally conformed to the package it
//! replaces), and discover the script packages of a cooked directory with
//! their build order.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use tempest_upk::flags::pkg as pkgf;
use tempest_upk::{Package, PackageIndex};

use crate::compiler::driver::{compile_package, default_dependencies, load_dependencies, CompileRequest, CompiledPackage};
use crate::compiler::output::OutputOptions;
use crate::context::{Context, ObjRef};
use crate::decompile::plan::plan_dependson;
use crate::decompile::Decompiler;
use crate::objects::ObjectBody;
use crate::par;

// ---------------------------------------------------------------------------
// Decompilation
// ---------------------------------------------------------------------------

pub struct ClassSourceText {
    pub name: String,
    /// Export index of the class in the package.
    pub export: usize,
    pub source: String,
}

pub struct DecompiledPackage {
    pub name: String,
    pub path: PathBuf,
    pub classes: Vec<ClassSourceText>,
}

impl DecompiledPackage {
    pub fn class(&self, name: &str) -> Option<&ClassSourceText> {
        self.classes.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Write `<root>/<Pkg>/Classes/<Class>.uc` for every class.
    pub fn write_to(&self, root: &Path) -> std::io::Result<PathBuf> {
        let dir = root.join(&self.name).join("Classes");
        std::fs::create_dir_all(&dir)?;
        for c in &self.classes {
            std::fs::write(dir.join(format!("{}.uc", c.name)), &c.source)?;
        }
        Ok(root.join(&self.name))
    }
}

/// Decompile every class of `path` (imports resolved from `search_paths`,
/// the package's own directory included). Classes render concurrently
/// (`par::jobs`); the `dependson` lists are planned for the whole package so
/// the compiler can parse the result in a valid order.
pub fn decompile_package(path: &Path, search_paths: &[PathBuf]) -> Result<DecompiledPackage, String> {
    let mut ctx = Context::new();
    for p in search_paths {
        ctx.add_search_path(p);
    }
    if let Some(dir) = path.parent() {
        ctx.add_search_path(dir);
    }
    let slot = ctx.load_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    ctx.preload_imports(slot);
    let ctx = &ctx;
    let pkg_name = ctx.packages[slot].name.clone();
    let n = ctx.packages[slot].pkg.exports.len();
    let mut all: Vec<ObjRef> = Vec::new();
    for e in 0..n {
        let r = ObjRef { pkg: slot, export: e };
        if ctx.class_name(r) != "Class" {
            continue;
        }
        if ctx.packages[slot].pkg.exports[e].serial_size == 0 && ctx.name(r).eq_ignore_ascii_case("None") {
            continue;
        }
        all.push(r);
    }
    let by_name: HashMap<String, ObjRef> = all.iter().map(|&r| (ctx.name(r).to_ascii_lowercase(), r)).collect();
    // First render: which same-package classes each one's declarations use.
    let refs: HashMap<ObjRef, BTreeSet<ObjRef>> = par::map(&all, |&r| {
        let d = Decompiler::new(ctx, r);
        let _ = d.class_source();
        let set: BTreeSet<ObjRef> = d.depends_on.borrow().iter().filter_map(|n| by_name.get(&n.to_ascii_lowercase()).copied()).collect();
        (r, set)
    })
    .into_iter()
    .collect();
    let plan = plan_dependson(ctx, &all, &refs);
    let classes: Vec<ClassSourceText> = par::map(&all, |&r| {
        let depends: Vec<String> = plan.depends.get(&r).map(|v| v.iter().map(|o| ctx.name(*o)).collect()).unwrap_or_default();
        ClassSourceText { name: ctx.name(r), export: r.export, source: Decompiler::with_depends(ctx, r, depends).class_source() }
    });
    Ok(DecompiledPackage { name: pkg_name, path: path.to_path_buf(), classes })
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CompileOptions {
    pub output: OutputOptions,
    /// Cooked output semantics (`--cooked`): also implies no source text.
    pub cooked: bool,
    pub allow_config_defaults: bool,
    /// Package the result replaces (`--conform`): names, imports, export order,
    /// NetIndex, component maps, tags, positions and content exports are taken
    /// from it so the output is a drop-in replacement.
    pub conform: Option<PathBuf>,
    /// Dependency packages to load (`-P`); empty means the built-in defaults
    /// plus every `.u` of the search paths; `["None"]` means none.
    pub deps: Vec<String>,
    pub verbose: bool,
}

impl CompileOptions {
    /// What `uscript project build` uses for a package of a cooked game
    /// directory: a cooked original gets cooked, source-stripped output.
    pub fn for_replacement(original: &Path, cooked: bool, deps: Vec<String>) -> CompileOptions {
        let mut o = CompileOptions { conform: Some(original.to_path_buf()), allow_config_defaults: true, deps, ..Default::default() };
        if cooked {
            o.cooked = true;
            o.output.cooked = true;
            o.output.keep_source = false;
        }
        if o.deps.is_empty() {
            o.deps.push("None".into());
        }
        o
    }
}

/// Compile `<src>/Classes/*.uc` (the package is named after `src`) against
/// the packages found in `search_paths`.
pub fn compile_package_dir(src: &Path, search_paths: &[PathBuf], opts: CompileOptions) -> Result<CompiledPackage, String> {
    let mut ctx = Context::new();
    for p in search_paths {
        ctx.add_search_path(p);
    }
    let package_name = src.file_name().map(|s| s.to_string_lossy().to_string()).ok_or("bad source directory")?;
    let mut deps = opts.deps.clone();
    if deps.is_empty() {
        deps = default_dependencies();
        for dir in &ctx.search_paths {
            if let Ok(rd) = std::fs::read_dir(dir) {
                let mut names: Vec<String> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case("u")).unwrap_or(false)).filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string())).collect();
                names.sort();
                for n in names {
                    if !deps.iter().any(|d| d.eq_ignore_ascii_case(&n)) && !n.eq_ignore_ascii_case(&package_name) {
                        deps.push(n);
                    }
                }
            }
        }
    }
    let mut out_opts = opts.output.clone();
    let mut seed_names: Vec<String> = Vec::new();
    let mut phantom_imports: Vec<(String, String)> = Vec::new();
    let mut phantom_member_imports: Vec<(String, Vec<String>)> = Vec::new();
    if let Some(p) = &opts.conform {
        // Conform to the package being replaced: NetIndex per object path and the
        // component map entries the cooker left as None.
        let mut probe = Context::new();
        for dir in &ctx.search_paths {
            probe.add_search_path(dir);
        }
        let slot = probe.load_file(p).map_err(|e| format!("{}: {e}", p.display()))?;
        probe.preload_imports(slot);
        let mut map = HashMap::new();
        let mut comps: HashMap<String, Vec<(String, bool)>> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        let mut tags: HashMap<String, Vec<(String, String, i32, bool)>> = HashMap::new();
        let mut pos: HashMap<String, (i32, i32)> = HashMap::new();
        let n = probe.packages[slot].pkg.exports.len();
        for e in 0..n {
            let r = ObjRef { pkg: slot, export: e };
            let ex = &probe.packages[slot].pkg.exports[e];
            let is_null = ex.class.is_null() && ex.serial_size == 0 && probe.name(r).eq_ignore_ascii_case("None");
            order.push(if is_null { String::new() } else { probe.path(r).to_ascii_lowercase() });
            if let Some(o) = probe.object(r) {
                map.insert(probe.path(r).to_ascii_lowercase(), o.header.net_index);
                let sd = match &o.body {
                    ObjectBody::Class { s, .. } | ObjectBody::State { s, .. } | ObjectBody::Function { s, .. } | ObjectBody::ScriptStruct { s, .. } => Some((s.line, s.text_pos)),
                    _ => None,
                };
                if let Some(lt) = sd {
                    pos.insert(probe.path(r).to_ascii_lowercase(), lt);
                }
                let lp = &probe.packages[slot];
                let list: Vec<(String, String, i32, bool)> = o
                    .header
                    .properties
                    .iter()
                    .map(|tp| {
                        let ty = lp.name_of(tp.type_name);
                        let is_null = matches!(ty.as_str(), "ObjectProperty" | "ComponentProperty" | "ClassProperty" | "InterfaceProperty") && tp.raw.len() == 4 && tp.raw.iter().all(|&b| b == 0);
                        (lp.name_of(tp.name), ty, tp.array_index, is_null)
                    })
                    .collect();
                if list.iter().any(|t| t.3) {
                    tags.insert(probe.path(r).to_ascii_lowercase(), list);
                }
                if let ObjectBody::Class { class, .. } = &o.body {
                    let list: Vec<(String, bool)> = class.component_name_to_default_object.iter().map(|(n, i)| (lp.name_of(*n), i.is_null())).collect();
                    comps.insert(probe.name(r).to_ascii_lowercase(), list);
                }
            }
        }
        out_opts.net_index_map = Some(map);
        out_opts.conform_components = Some(comps);
        out_opts.conform_exports = Some(order);
        out_opts.conform_tags = Some(tags);
        out_opts.conform_pos = Some(pos);
        let rp = &probe.packages[slot].pkg;
        seed_names = rp.names.iter().map(|n| n.name.clone()).collect();
        out_opts.conform_name_flags = Some(rp.names.iter().map(|n| n.flags).collect());
        out_opts.conform_imports = Some(rp.imports.iter().map(|im| (rp.fname_to_string(im.class_package), rp.fname_to_string(im.class_name), im.outer.0, rp.fname_to_string(im.object_name))).collect());
        out_opts.conform_raw = Some((0..rp.exports.len()).map(|e| Some((rp.exports[e].clone(), rp.export_data[e].clone()))).collect());
        out_opts.conform_summary = Some(rp.summary.clone());
        for imp in rp.imports.iter() {
            let cn = rp.fname_to_string(imp.class_name);
            if cn == "Class" {
                if let Some(outer) = imp.outer.import_index().and_then(|oi| rp.imports.get(oi)) {
                    if rp.fname_to_string(outer.class_name) == "Package" {
                        phantom_imports.push((rp.fname_to_string(imp.object_name), rp.fname_to_string(outer.object_name)));
                    }
                }
            }
            // Full path of the import (package first).
            let mut segs: Vec<String> = vec![rp.fname_to_string(imp.object_name)];
            let mut cur = imp.outer;
            let mut guard = 0;
            while let Some(oi) = cur.import_index() {
                let Some(o) = rp.imports.get(oi) else { break };
                segs.push(rp.fname_to_string(o.object_name));
                cur = o.outer;
                guard += 1;
                if guard > 32 {
                    break;
                }
            }
            segs.reverse();
            phantom_member_imports.push((cn, segs));
        }
    }
    let missing = load_dependencies(&mut ctx, &deps);
    if let Some(p) = &opts.conform {
        // The reference package itself, under an alias: resolves object literals to
        // its forced-export content and supplies native-data samples.
        let pkg = Package::load(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let alias = format!("{package_name}__conform");
        let slot = ctx.add_package_shadow(alias, p.clone(), pkg);
        // Not `preload_imports`: a cooked package imports its own missing native
        // classes, which would load it a second time under its real name.
        ctx.reparse_components(slot);
    }
    if opts.verbose {
        for m in &missing {
            eprintln!("note: dependency package {m} not found");
        }
        eprintln!("loaded {} packages", ctx.packages.len());
    }
    let req = CompileRequest {
        package_name,
        package_dir: src.to_path_buf(),
        output: out_opts,
        verbose: opts.verbose,
        global_includes: deps,
        allow_config_defaults: opts.allow_config_defaults,
        cooked: opts.cooked,
        phantom_imports,
        phantom_member_imports,
        seed_names,
    };
    compile_package(&mut ctx, &req).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Script packages of a cooked directory
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ScriptPackage {
    pub name: String,
    pub ext: String,
    /// Classes of its own (top-level `Class` exports).
    pub classes: usize,
    /// Script packages it imports from (direct).
    pub deps: BTreeSet<String>,
    pub cooked: bool,
}

/// Script packages of a directory: those with at least one class of their own
/// (seek-free content packages only carry copies of classes under a `Package`
/// export). Fails when a package whose classes are imported is missing, since
/// a missing parent package would silently drop `extends`.
pub fn script_packages(dir: &Path) -> Result<BTreeMap<String, ScriptPackage>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| matches!(x.to_string_lossy().to_ascii_lowercase().as_str(), "upk" | "u")).unwrap_or(false))
        .collect();
    files.sort();
    struct Scan {
        pkg: ScriptPackage,
        class_deps: BTreeSet<String>,
    }
    let scanned: Vec<Result<Option<Scan>, String>> = par::map(&files, |p| {
        let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = p.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let pkg = match Package::load_tables(p) {
            Ok(pkg) => pkg,
            Err(e) => return Err(format!("skipping {}: {e}", p.display())),
        };
        let own_classes = pkg.exports.iter().enumerate().filter(|(i, e)| e.outer.is_null() && e.serial_size > 0 && pkg.class_name_of(PackageIndex::export(*i)) == "Class").count();
        if own_classes == 0 {
            return Ok(None);
        }
        let mut class_deps: BTreeSet<String> = BTreeSet::new();
        let mut deps: BTreeSet<String> = BTreeSet::new();
        for (i, _) in pkg.imports.iter().enumerate() {
            if pkg.class_name_of(PackageIndex::import(i)) == "Class" {
                let mut idx = PackageIndex::import(i);
                while let Some(ii) = idx.import_index() {
                    if pkg.imports[ii].outer.is_null() {
                        let n = pkg.fname_to_string(pkg.imports[ii].object_name);
                        if !n.eq_ignore_ascii_case(&name) {
                            class_deps.insert(n);
                        }
                        break;
                    }
                    idx = pkg.imports[ii].outer;
                }
            }
            let mut idx = PackageIndex::import(i);
            let mut guard = 0;
            while let Some(ii) = idx.import_index() {
                let imp = &pkg.imports[ii];
                if imp.outer.is_null() {
                    let n = pkg.fname_to_string(imp.object_name);
                    if !n.eq_ignore_ascii_case(&name) {
                        deps.insert(n);
                    }
                    break;
                }
                idx = imp.outer;
                guard += 1;
                if guard > 32 {
                    break;
                }
            }
        }
        let cooked = pkg.summary.package_flags & pkgf::COOKED != 0;
        Ok(Some(Scan { pkg: ScriptPackage { name, ext, classes: own_classes, deps, cooked }, class_deps }))
    });
    let mut found: BTreeMap<String, ScriptPackage> = BTreeMap::new();
    let mut class_deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for r in scanned {
        let scan = match r {
            Ok(Some(scan)) => scan,
            Ok(None) => continue,
            Err(msg) => {
                eprintln!("{msg}");
                continue;
            }
        };
        for d in scan.class_deps {
            class_deps.entry(d).or_default().insert(scan.pkg.name.clone());
        }
        found.insert(scan.pkg.name.clone(), scan.pkg);
    }
    let names: BTreeSet<String> = found.keys().map(|k| k.to_ascii_lowercase()).collect();
    let missing: Vec<String> = class_deps.iter().filter(|(d, _)| !names.contains(&d.to_ascii_lowercase())).map(|(d, by)| format!("{d} (classes used by {})", by.iter().cloned().collect::<Vec<_>>().join(", "))).collect();
    if !missing.is_empty() {
        return Err(format!("script packages missing from {}: {}", dir.display(), missing.join("; ")));
    }
    for p in found.values_mut() {
        p.deps.retain(|d| names.contains(&d.to_ascii_lowercase()));
    }
    Ok(found)
}

/// One package in build order with its transitive dependencies (in build order).
#[derive(Clone, Debug)]
pub struct OrderedPackage {
    pub name: String,
    pub ext: String,
    pub deps: Vec<String>,
    pub cooked: bool,
}

/// Dependency order (Kahn), ties by name.
pub fn build_order(found: &BTreeMap<String, ScriptPackage>) -> Result<Vec<OrderedPackage>, String> {
    let canon = |n: &str| found.keys().find(|k| k.eq_ignore_ascii_case(n)).cloned().unwrap_or_else(|| n.to_string());
    let mut done: Vec<String> = Vec::new();
    let mut closure: BTreeMap<String, Vec<String>> = BTreeMap::new();
    while done.len() < found.len() {
        let next = found.iter().find(|(n, p)| !done.contains(n) && p.deps.iter().all(|d| done.iter().any(|x| x.eq_ignore_ascii_case(d)))).map(|(n, _)| n.clone());
        let Some(n) = next else {
            let left: Vec<&String> = found.keys().filter(|k| !done.contains(k)).collect();
            return Err(format!("dependency cycle among {left:?}"));
        };
        let mut all: Vec<String> = Vec::new();
        for d in &found[&n].deps {
            let d = canon(d);
            for x in closure.get(&d).cloned().unwrap_or_default().into_iter().chain(std::iter::once(d.clone())) {
                if !all.iter().any(|y| y.eq_ignore_ascii_case(&x)) {
                    all.push(x);
                }
            }
        }
        all.sort_by_key(|x| done.iter().position(|d| d.eq_ignore_ascii_case(x)).unwrap_or(usize::MAX));
        closure.insert(n.clone(), all);
        done.push(n);
    }
    Ok(done.into_iter().map(|n| OrderedPackage { name: n.clone(), ext: found[&n].ext.clone(), deps: closure[&n].clone(), cooked: found[&n].cooked }).collect())
}
