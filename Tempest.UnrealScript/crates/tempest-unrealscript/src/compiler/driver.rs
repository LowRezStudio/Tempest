//! Package compilation driver: reads the `.uc` sources of a package,
//! runs the two compiler passes in dependency order, imports defaults and
//! produces a package.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tempest_upk::flags::{class as cf, rf};
use tempest_upk::Package;

use super::engine::{CompileError, Compiler, Pass};
use super::model::{FieldRef, Node, NodeId, NodeKind};
use super::output::{Emitter, OutputOptions};
use super::preprocess::{factory_prepass, ClassSource, MacroProcessor};
use crate::context::Context;

pub struct CompileRequest {
    pub package_name: String,
    /// Directory holding `Classes/*.uc` (and optionally `Globals.uci`).
    pub package_dir: PathBuf,
    pub output: OutputOptions,
    pub verbose: bool,
    /// Packages whose `Globals.uci` are loaded first (in order), like UDK's EditPackages.
    pub global_includes: Vec<String>,
    /// Allow config/localized values in defaultproperties (cooked-package round trips).
    pub allow_config_defaults: bool,
    /// Cooked output semantics (immutable-when-cooked structs are binary).
    pub cooked: bool,
    /// Extra `(class, package)` imports to provide stand-in classes for (from the
    /// package being conformed to, which may import native classes of itself).
    pub phantom_imports: Vec<(String, String)>,
    /// Every import of the conform package as `(class name, path segments)`, to
    /// give phantom classes their members.
    pub phantom_member_imports: Vec<(String, Vec<String>)>,
    /// Names interned first (a conform package's name table, so indices line up).
    pub seed_names: Vec<String>,
}

#[derive(Debug)]
pub struct DriverError(pub String);
impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for DriverError {}

impl From<CompileError> for DriverError {
    fn from(e: CompileError) -> Self {
        DriverError(e.to_string())
    }
}

pub struct CompiledPackage {
    pub package: Package,
    pub warnings: Vec<String>,
}

/// Read and preprocess every class file of a package.
pub fn load_sources(req: &CompileRequest) -> Result<Vec<ClassSource>, DriverError> {
    let classes_dir = req.package_dir.join("Classes");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&classes_dir)
        .map_err(|e| DriverError(format!("cannot read {}: {e}", classes_dir.display())))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case("uc")).unwrap_or(false))
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default());
    let source_path = req.package_dir.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let mut pre = MacroProcessor::new(&source_path, &req.package_name, tempest_upk::flags::ENGINE_VERSION);
    for g in &req.global_includes {
        if !g.eq_ignore_ascii_case(&req.package_name) {
            pre.process_global_include_named(g).map_err(|e| DriverError(format!("{g}/Globals.uci: {e}")))?;
        }
    }
    pre.process_global_include().map_err(|e| DriverError(format!("Globals.uci: {e}")))?;
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read(&f).map_err(|e| DriverError(format!("cannot read {}: {e}", f.display())))?;
        let text = decode_text(&text);
        let class_name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        pre.class_name = class_name.clone();
        let processed = if text.contains('`') {
            pre.process(&text, &format!("{class_name}.uc")).map_err(|e| DriverError(format!("{class_name}: {e}")))?
        } else {
            text
        };
        let cs = factory_prepass(&processed, &class_name).map_err(|e| DriverError(format!("{class_name}: {e}")))?;
        out.push(cs);
    }
    Ok(out)
}

/// Decode script text the way the engine does: UTF-16 with a BOM, otherwise
/// one `TCHAR` per byte (Latin-1 here; the Windows-1252 remap happens on output).
fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let u: Vec<u16> = bytes[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&u).chars().map(|c| if (c as u32) < 0x100 { c } else { '?' }).collect();
    }
    bytes.iter().map(|&b| b as char).collect()
}

/// Compile a package from sources against the packages loaded in `ctx`.
pub fn compile_package(ctx: &mut Context, req: &CompileRequest) -> Result<CompiledPackage, DriverError> {
    let sources = load_sources(req)?;
    let mut compiler = Compiler::new(ctx, &req.package_name, &req.seed_names);
    compiler.add_phantom_classes_from(&req.phantom_imports);
    compiler.add_phantom_members(&req.phantom_member_imports);
    compiler.allow_config_defaults = req.allow_config_defaults;
    compiler.cooked = req.cooked;
    // Create class nodes.
    let mut ids: Vec<NodeId> = Vec::new();
    for cs in &sources {
        let mut n = Node::new(NodeKind::Class, &cs.class_name, None);
        n.object_flags = rf::PUBLIC | rf::STANDALONE;
        if cs.is_interface {
            n.class.class_flags |= cf::INTERFACE;
        }
        n.class.source_file = format!("{}.uc", cs.class_name);
        n.class.default_props_text = Some((cs.default_prop_text.clone(), 0));
        if !cs.cpp_text.is_empty() {
            n.strct.cpp_text = Some(cs.cpp_text.clone());
        }
        n.strct.line = -1;
        n.strct.text_pos = -1;
        n.class.depends_on = cs.depends_on.clone();
        let id = compiler.model.add(n);
        compiler.model.classes.push(id);
        compiler.sources.insert(id, cs.script_text.clone());
        ids.push(id);
    }
    // Resolve super classes and within.
    let object_class = compiler.res().find_class("Object");
    for (i, cs) in sources.iter().enumerate() {
        let id = ids[i];
        if cs.class_name.eq_ignore_ascii_case("Object") {
            continue;
        }
        let Some(sup) = compiler.res().find_class(&cs.base_class_name) else {
            return Err(DriverError(format!("Superclass {} of class {} not found", cs.base_class_name, cs.class_name)));
        };
        if sup == FieldRef::Local(id) {
            return Err(DriverError(format!("Class is extending itself '{}'", cs.class_name)));
        }
        compiler.model.node_mut(id).strct.super_struct = Some(sup);
    }
    for &id in &ids {
        let sup = compiler.model.node(id).strct.super_struct;
        let within = sup.and_then(|s| compiler.res().class_within(s)).or(object_class);
        let config = sup.map(|s| compiler.res().class_config_name(s)).unwrap_or_else(|| "None".into());
        let n = compiler.model.node_mut(id);
        n.class.class_within = within;
        n.class.config_name = config;
    }
    // Parse pass: class-tree order, DependsOn classes pulled forward (`ParseScripts`).
    let order: Vec<NodeId> = class_tree_order(&compiler, &ids);
    let mut parsed: HashSet<NodeId> = HashSet::new();
    for &id in &order {
        parse_class(&mut compiler, id, &order, &mut parsed, &mut Vec::new(), req.verbose)?;
    }
    // Compile pass: pure class-tree order (`CompileScripts`).
    for &id in &order {
        if req.verbose {
            eprintln!("Compiling {}", compiler.model.node(id).name);
        }
        compiler.compile_script(id, Pass::Compile)?;
    }
    // Native header export marks native, non-noexport classes as exported.
    for &id in &order {
        let f = compiler.model.node(id).class.class_flags;
        if f & cf::NATIVE != 0 && f & cf::NO_EXPORT == 0 {
            compiler.model.node_mut(id).class.class_flags |= cf::EXPORTED;
        }
    }
    // Defaults.
    super::defaults::compile_defaults(&mut compiler, &order)?;
    let warnings = compiler.warnings.clone();
    let package = Emitter::new(&compiler, req.output.clone()).build(&req.package_name);
    Ok(CompiledPackage { package, warnings })
}

/// `FClassTree` walk order: pre-order from `Object`, siblings sorted alphabetically
/// (`FClassTree::AddChildNode`). `ParseScripts`/`CompileScripts` visit classes in this
/// order; only local classes are returned.
fn class_tree_order(c: &Compiler, ids: &[NodeId]) -> Vec<NodeId> {
    use std::collections::HashMap;
    let r = c.res();
    let mut children: HashMap<Option<FieldRef>, Vec<FieldRef>> = HashMap::new();
    let mut seen: HashSet<FieldRef> = HashSet::new();
    for &id in ids {
        let mut cur = FieldRef::Local(id);
        loop {
            if !seen.insert(cur) {
                break;
            }
            let sup = r.inheritance_super(cur);
            children.entry(sup).or_default().push(cur);
            match sup {
                Some(s) => cur = s,
                None => break,
            }
        }
    }
    for v in children.values_mut() {
        v.sort_by(|a, b| r.name_of(*a).to_ascii_lowercase().cmp(&r.name_of(*b).to_ascii_lowercase()));
    }
    let mut out = Vec::new();
    let mut stack: Vec<FieldRef> = children.get(&None).cloned().unwrap_or_default().into_iter().rev().collect();
    while let Some(f) = stack.pop() {
        if let FieldRef::Local(id) = f {
            out.push(id);
        }
        if let Some(ch) = children.get(&Some(f)) {
            stack.extend(ch.iter().rev().copied());
        }
    }
    out
}

/// `ParseScripts` for one class: DependsOn classes first (with their subclasses, as
/// the engine does), then the class itself. `order` is the class-tree order.
fn parse_class(c: &mut Compiler, id: NodeId, order: &[NodeId], parsed: &mut HashSet<NodeId>, stack: &mut Vec<NodeId>, verbose: bool) -> Result<(), DriverError> {
    if parsed.contains(&id) {
        return Ok(());
    }
    // `ParseScripts` may re-enter a class whose DependsOn handling is in progress
    // (a dependency's subclass depending back on it); only a cycle in the DependsOn
    // lists themselves is an error (`IsDependentOn`).
    if stack.len() > 4096 {
        return Err(DriverError(format!("Class {} DependsOn recursion too deep.", c.model.node(id).name)));
    }
    stack.push(id);
    if let Some(FieldRef::Local(s)) = c.model.node(id).strct.super_struct {
        parse_class(c, s, order, parsed, stack, verbose)?;
    }
    for d in c.model.node(id).class.depends_on.clone() {
        match c.res().find_class(&d) {
            Some(FieldRef::Local(dn)) => {
                // Already parsed (or no script): nothing to pull forward.
                if parsed.contains(&dn) {
                    continue;
                }
                if c.res().is_child_of(FieldRef::Local(id), FieldRef::Local(dn)) {
                    return Err(DriverError(format!("{} is derived from {} - please remove the DependsOn", c.model.node(id).name, d)));
                }
                if depends_on_transitively(c, dn, id) {
                    return Err(DriverError(format!("Class {} DependsOn({}) is a circular dependency.", c.model.node(dn).name, c.model.node(id).name)));
                }
                // The dependency and its subclasses (`ParseScripts(..., MakeSubclasses=TRUE)`).
                parse_class(c, dn, order, parsed, stack, verbose)?;
                let subtree: Vec<NodeId> = order.iter().copied().filter(|&x| x != dn && c.res().is_child_of(FieldRef::Local(x), FieldRef::Local(dn))).collect();
                for x in subtree {
                    parse_class(c, x, order, parsed, stack, verbose)?;
                }
            }
            Some(FieldRef::External(_)) => {}
            None => return Err(DriverError(format!("Failed to find DependsOn/Implements class '{}' while parsing '{}'", d, c.model.node(id).name))),
        }
    }
    stack.pop();
    if parsed.contains(&id) {
        // Parsed re-entrantly while handling a dependency.
        return Ok(());
    }
    if verbose {
        eprintln!("Parsing {}", c.model.node(id).name);
    }
    parsed.insert(id);
    c.compile_script(id, Pass::Parse)?;
    Ok(())
}

/// `FScriptCompiler::IsDependentOn`: does `class`'s DependsOn list reach `target`?
fn depends_on_transitively(c: &Compiler, class: NodeId, target: NodeId) -> bool {
    let mut seen: HashSet<NodeId> = HashSet::new();
    let mut todo = vec![class];
    while let Some(x) = todo.pop() {
        if !seen.insert(x) {
            continue;
        }
        for d in &c.model.node(x).class.depends_on {
            if let Some(FieldRef::Local(dn)) = c.res().find_class(d) {
                if dn == target {
                    return true;
                }
                todo.push(dn);
            }
        }
    }
    false
}

/// Convenience: load the standard dependency packages from search paths.
pub fn load_dependencies(ctx: &mut Context, names: &[String]) -> Vec<String> {
    let mut missing = Vec::new();
    for n in names {
        match ctx.package_by_name(n) {
            Some(slot) => ctx.preload_imports(slot),
            None => missing.push(n.clone()),
        }
    }
    missing
}

pub fn default_dependencies() -> Vec<String> {
    ["Core", "Engine", "GameFramework", "IpDrv", "GFxUI", "OnlineSubsystemPC", "TgGame", "TgClient", "UDKBase", "UTGame"].iter().map(|s| s.to_string()).collect()
}

pub fn package_dir_for(src_root: &Path, name: &str) -> PathBuf {
    src_root.join(name)
}
