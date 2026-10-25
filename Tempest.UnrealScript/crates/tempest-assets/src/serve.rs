//! `upk serve`: a JSON-lines RPC server over stdin/stdout, the backend of
//! the Studio GUI.
//!
//! One request per line, `{"id": n, "method": "...", "params": {...}}`;
//! one response per line, `{"id": n, "ok": true, "result": ...}` or
//! `{"id": n, "ok": false, "error": "..."}`. Lines without an `id` are
//! events the server pushes on its own (`{"event": "...", ...}`), used for
//! progress. Binary results (previews) travel as base64.
//!
//! The class database and every opened package stay in memory between
//! calls, which is what makes a property inspector responsive: the class
//! database alone takes a second or two to load.

use std::collections::HashMap;
use std::error::Error;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value as J};
use tempest_unrealscript::toolchain::{build_order, compile_package_dir, decompile_package, script_packages, CompileOptions, DecompiledPackage, OrderedPackage};
use tempest_upk::flags::{compression, pkg as pkgf};
use tempest_upk::io::PackageIndex;
use tempest_upk::{Package, PackageFile, WriteOptions};

use crate::edit::{referrers, Editor};
use crate::json;
use crate::material_ops::PackageCache;
use crate::media;
use crate::mesh_ops;
use crate::object::{Native, Parsed};
use crate::reflect::ClassDb;
use crate::texcache::{MipLocation, TextureSource};
use crate::texture_ops::{self, ImportOptions, ImportSource};
use crate::verify::verify_package;
use crate::visit::references;

type Res<T> = Result<T, Box<dyn Error>>;

/// One package of the workspace directory.
struct WorkspacePackage {
    name: String,
    path: PathBuf,
    size: u64,
    flags: u32,
}

/// Searchable table of one package (built lazily by `workspace.search`).
struct SearchIndex {
    classes: Vec<String>,
    /// (class id, object path) per export.
    exports: Vec<(u16, String)>,
}

pub struct Server {
    dir: Option<PathBuf>,
    /// Leaked on purpose: editors borrow it for the server's whole life.
    db: Option<&'static ClassDb>,
    src: Option<TextureSource>,
    packages: Vec<WorkspacePackage>,
    editors: HashMap<String, Editor<'static>>,
    index: HashMap<String, SearchIndex>,
    /// Decompiled sources per package (keyed like `editors`).
    scripts: HashMap<String, DecompiledPackage>,
    /// Script packages of the workspace in build order (lazily computed).
    order: Option<Vec<OrderedPackage>>,
    /// Preview PNGs of material textures (`mesh.preview`), by path and size.
    png_cache: HashMap<String, Vec<u8>>,
    out: std::io::Stdout,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

fn key(path: &str) -> String {
    let p = Path::new(path);
    let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon.to_string_lossy().to_string()
}

fn s<'a>(p: &'a J, name: &str) -> Res<&'a str> {
    p.get(name).and_then(|v| v.as_str()).ok_or_else(|| format!("missing parameter {name}").into())
}

fn opt_s(p: &J, name: &str) -> Option<String> {
    p.get(name).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn opt_u(p: &J, name: &str) -> Option<usize> {
    p.get(name).and_then(|v| v.as_u64()).map(|v| v as usize)
}

fn opt_b(p: &J, name: &str) -> Option<bool> {
    p.get(name).and_then(|v| v.as_bool())
}

impl Server {
    pub fn new() -> Server {
        Server { dir: None, db: None, src: None, packages: Vec::new(), editors: HashMap::new(), index: HashMap::new(), scripts: HashMap::new(), order: None, png_cache: HashMap::new(), out: std::io::stdout() }
    }

    /// Serve stdin until EOF.
    pub fn run(&mut self) {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            match stdin.lock().read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = line.trim();
            if text.is_empty() {
                continue;
            }
            let req: J = match serde_json::from_str(text) {
                Ok(v) => v,
                Err(e) => {
                    self.emit(&json!({ "id": null, "ok": false, "error": format!("bad request: {e}") }));
                    continue;
                }
            };
            let id = req.get("id").cloned().unwrap_or(J::Null);
            let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("").to_string();
            let params = req.get("params").cloned().unwrap_or(J::Object(Map::new()));
            if method == "quit" {
                self.emit(&json!({ "id": id, "ok": true, "result": null }));
                break;
            }
            let t = std::time::Instant::now();
            let response = match self.call(&method, &params) {
                Ok(r) => json!({ "id": id, "ok": true, "result": r, "ms": t.elapsed().as_millis() as u64 }),
                Err(e) => json!({ "id": id, "ok": false, "error": e.to_string(), "ms": t.elapsed().as_millis() as u64 }),
            };
            self.emit(&response);
        }
    }

    fn emit(&mut self, v: &J) {
        let mut text = v.to_string();
        text.push('\n');
        let _ = self.out.write_all(text.as_bytes());
        let _ = self.out.flush();
    }

    fn event(&mut self, name: &str, mut fields: Map<String, J>) {
        fields.insert("event".into(), J::from(name));
        self.emit(&J::Object(fields));
    }

    fn db(&self) -> Res<&'static ClassDb> {
        self.db.ok_or_else(|| "no workspace open (call workspace.open first)".into())
    }

    fn src(&self) -> Res<&TextureSource> {
        self.src.as_ref().ok_or_else(|| "no workspace open".into())
    }

    fn dir(&self) -> Res<&Path> {
        self.dir.as_deref().ok_or_else(|| "no workspace open".into())
    }

    fn editor(&mut self, path: &str) -> Res<&mut Editor<'static>> {
        let k = key(path);
        if !self.editors.contains_key(&k) {
            let db = self.db()?;
            let ed = Editor::open(db, path)?;
            self.editors.insert(k.clone(), ed);
        }
        Ok(self.editors.get_mut(&k).unwrap())
    }

    /// The index of one export: a number, or an object path.
    fn export_of(&mut self, path: &str, p: &J) -> Res<usize> {
        let ed = self.editor(path)?;
        if let Some(i) = opt_u(p, "index") {
            if i >= ed.pkg.exports.len() {
                return Err(format!("export {i} out of range").into());
            }
            return Ok(i);
        }
        if let Some(name) = opt_s(p, "object") {
            return Ok(ed.find(&name)?);
        }
        Err("missing parameter index".into())
    }

    pub fn call(&mut self, method: &str, p: &J) -> Res<J> {
        match method {
            "ping" => Ok(json!({ "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id() })),
            "workspace.open" => self.workspace_open(s(p, "dir")?),
            "workspace.packages" => Ok(self.workspace_packages()),
            "workspace.search" => self.workspace_search(p),
            "workspace.summary" => self.workspace_summary(s(p, "path")?),
            "package.open" => self.package_open(s(p, "path")?),
            "package.tables" => {
                let ed = self.editor(s(p, "path")?)?;
                let mut v = json::tables(&ed.pkg);
                v["name"] = J::from(ed.name.clone());
                Ok(v)
            }
            "package.close" => {
                let k = key(s(p, "path")?);
                self.scripts.remove(&k);
                Ok(J::from(self.editors.remove(&k).is_some()))
            }
            "package.dirty" => {
                let ed = self.editor(s(p, "path")?)?;
                Ok(J::from(ed.is_dirty()))
            }
            "package.save" => self.package_save(p),
            "package.undo" => {
                let ed = self.editor(s(p, "path")?)?;
                let label = ed.undo();
                Ok(json!({ "label": label, "remaining": ed.undo_labels(), "dirty": ed.is_dirty() }))
            }
            "package.undoList" => {
                let ed = self.editor(s(p, "path")?)?;
                Ok(json!({ "labels": ed.undo_labels(), "dirty": ed.is_dirty() }))
            }
            "package.discard" => {
                // Drop the in-memory package: the next open reads the file again.
                let k = key(s(p, "path")?);
                self.scripts.remove(&k);
                Ok(J::from(self.editors.remove(&k).is_some()))
            }
            "package.rewrite" => self.package_rewrite(p),
            "package.verify" => self.package_verify(s(p, "path")?),
            "object.get" => self.object_get(p),
            "object.text" => self.object_text(p),
            "object.set" => self.object_set(p),
            "object.rename" => self.object_rename(p),
            "object.delete" => self.object_delete(p),
            "object.copy" => self.object_copy(p),
            "object.deps" => self.object_deps(p),
            "object.extract" => self.object_extract(p),
            "texture.info" => self.texture_info(p),
            "texture.preview" => self.texture_preview(p),
            "texture.export" => self.texture_export(p),
            "texture.exportAll" => self.texture_export_all(p),
            "texture.import" => self.texture_import(p),
            "mesh.info" => self.mesh_info(p),
            "mesh.preview" => self.mesh_glb(p, true),
            "mesh.export" => self.mesh_export(p),
            "mesh.import" => self.mesh_import(p),
            "object.replaceRaw" => self.object_replace_raw(p),
            "media.preview" => self.media_preview(p),
            "media.export" => self.media_export(p),
            "media.import" => self.media_import(p),
            "mod.dir" => self.mod_dir(p),
            "script.source" => self.script_source(p),
            "script.compile" => self.script_compile(p),
            "script.export" => self.script_export(p),
            "package.findReplace" => self.find_replace(p),
            "material.preview" => self.material_preview(p),
            "material.setParameter" => self.material_set_parameter(p),
            "batch.plan" => self.batch(p, false),
            "batch.run" => self.batch(p, true),
            _ => Err(format!("unknown method {method}").into()),
        }
    }

    // ----- workspace --------------------------------------------------------

    fn workspace_open(&mut self, dir: &str) -> Res<J> {
        let dir = PathBuf::from(dir);
        if !dir.is_dir() {
            return Err(format!("{} is not a directory", dir.display()).into());
        }
        let t = std::time::Instant::now();
        let scripts = ClassDb::script_packages_in(&dir)?;
        if scripts.is_empty() {
            return Err(format!("{} holds no script packages (Core.upk, Engine.upk, …); open the CookedPCConsole folder", dir.display()).into());
        }
        let db: &'static ClassDb = Box::leak(Box::new(ClassDb::load_dir(&dir)?));
        self.editors.clear();
        self.index.clear();
        self.scripts.clear();
        self.order = None;
        self.db = Some(db);
        self.src = Some(TextureSource::open(&dir)?);
        self.dir = Some(dir.clone());
        self.packages = list_packages(&dir);
        Ok(json!({
            "dir": dir.to_string_lossy(),
            "scriptPackages": db.package_names(),
            "textureIndex": self.src.as_ref().map(|s| s.index.is_some()).unwrap_or(false),
            "loadMs": t.elapsed().as_millis() as u64,
            "packages": self.workspace_packages(),
        }))
    }

    fn workspace_packages(&self) -> J {
        J::Array(
            self.packages
                .iter()
                .map(|p| {
                    json!({
                        "name": p.name,
                        "path": p.path.to_string_lossy(),
                        "size": p.size,
                        "script": p.flags & pkgf::CONTAINS_SCRIPT != 0,
                        "map": p.flags & pkgf::CONTAINS_MAP != 0,
                        "open": self.editors.contains_key(&key(&p.path.to_string_lossy())),
                    })
                })
                .collect(),
        )
    }

    /// Cheap facts about one package without opening it (summary only).
    fn workspace_summary(&self, path: &str) -> Res<J> {
        let pf = PackageFile::open(path)?;
        Ok(json::tables(pf.tables()))
    }

    fn workspace_search(&mut self, p: &J) -> Res<J> {
        let query = s(p, "query")?.to_ascii_lowercase();
        let class = opt_s(p, "class").map(|c| c.to_ascii_lowercase()).filter(|c| !c.is_empty());
        let limit = opt_u(p, "limit").unwrap_or(500);
        if query.len() < 2 && class.is_none() {
            return Err("query too short".into());
        }
        let total = self.packages.len();
        let paths: Vec<(String, PathBuf)> = self.packages.iter().map(|p| (p.name.clone(), p.path.clone())).collect();
        let mut results = Vec::new();
        let mut scanned = 0usize;
        let mut truncated = false;
        for (n, (name, path)) in paths.iter().enumerate() {
            let k = key(&path.to_string_lossy());
            if !self.index.contains_key(&k) {
                let idx = build_index(path);
                self.index.insert(k.clone(), idx);
                scanned += 1;
                if scanned.is_multiple_of(20) {
                    let mut f = Map::new();
                    f.insert("done".into(), J::from(n + 1));
                    f.insert("total".into(), J::from(total));
                    self.event("search.progress", f);
                }
            }
            let idx = &self.index[&k];
            for (i, (cid, path_text)) in idx.exports.iter().enumerate() {
                let cname = &idx.classes[*cid as usize];
                if let Some(c) = &class {
                    if !cname.eq_ignore_ascii_case(c) {
                        continue;
                    }
                }
                if !query.is_empty() && !path_text.to_ascii_lowercase().contains(&query) {
                    continue;
                }
                results.push(json!({ "package": name, "path": path.to_string_lossy(), "index": i, "object": path_text, "class": cname }));
                if results.len() >= limit {
                    truncated = true;
                    break;
                }
            }
            if truncated {
                break;
            }
        }
        Ok(json!({ "results": results, "truncated": truncated }))
    }

    // ----- package ----------------------------------------------------------

    fn package_open(&mut self, path: &str) -> Res<J> {
        let t = std::time::Instant::now();
        let ed = self.editor(path)?;
        let mut v = json::tables(&ed.pkg);
        v["name"] = J::from(ed.name.clone());
        v["path"] = J::from(path);
        v["dirty"] = J::from(ed.is_dirty());
        v["loadMs"] = J::from(t.elapsed().as_millis() as u64);
        Ok(v)
    }

    fn package_save(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let out = opt_s(p, "out").unwrap_or_else(|| path.to_string());
        let ed = self.editor(path)?;
        ed.save(&out)?;
        if key(&out) != key(path) {
            // Saved elsewhere: the open editor still describes `path`.
            return Ok(json!({ "out": out, "reopened": false }));
        }
        // Saving in place: the file changed under the memory-mapped original;
        // drop and reopen so later reads come from the new file.
        let k = key(path);
        let is_script = self.editors.get(&k).map(|e| e.pkg.summary.package_flags & pkgf::CONTAINS_SCRIPT != 0).unwrap_or(false);
        self.editors.remove(&k);
        self.index.remove(&k);
        self.scripts.remove(&k);
        // A changed script package changes reflection for every other package.
        let mut db_reloaded = false;
        if is_script {
            if let Some(dir) = self.dir.clone() {
                if let Ok(db) = ClassDb::load_dir(&dir) {
                    self.db = Some(Box::leak(Box::new(db)));
                    db_reloaded = true;
                }
            }
        }
        Ok(json!({ "out": out, "reopened": true, "dbReloaded": db_reloaded }))
    }

    fn package_rewrite(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let out = s(p, "out")?;
        let compress = opt_b(p, "compress").unwrap_or(true);
        let pf = PackageFile::open(path)?;
        let mut pkg = pf.load()?;
        let obscure = opt_b(p, "obscure").unwrap_or(compress && pkg.summary.compression_flags & compression::OBSCURED != 0);
        pkg.save(out, &WriteOptions { compress, obscure })?;
        Ok(json!({ "out": out, "compress": compress, "obscure": obscure }))
    }

    fn package_verify(&mut self, path: &str) -> Res<J> {
        let db = self.db()?;
        let r = verify_package(db, Path::new(path), false);
        Ok(json!({
            "name": r.name,
            "error": r.error,
            "exports": r.exports,
            "full": r.full,
            "raw": r.raw,
            "partialValues": r.partial_values,
            "withTail": r.with_tail,
            "mismatches": r.mismatches,
            "imageIdentical": r.image_identical,
            "mismatchSamples": r.mismatch_samples,
            "rawSamples": r.raw_samples,
            "ok": r.error.is_none() && r.mismatches == 0 && r.image_identical,
        }))
    }

    // ----- objects ----------------------------------------------------------

    fn object_get(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let ed = self.editor(path)?;
        let parsed = ed.parsed(i)?.clone();
        let mut v = json::parsed(&ed.pkg, i, &parsed);
        v["row"] = json::export_row(&ed.pkg, i);
        Ok(v)
    }

    fn object_text(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let ed = self.editor(path)?;
        let parsed = ed.parsed(i)?.clone();
        let d = crate::dump::Dumper::new(&ed.pkg);
        let mut out = d.export_line(i);
        out.push('\n');
        d.parsed(&parsed, &mut out);
        Ok(J::from(out))
    }

    fn object_set(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let prop = s(p, "prop")?;
        let value = s(p, "value")?;
        let ed = self.editor(path)?;
        ed.begin_undo(format!("set {prop}"));
        ed.set(i, prop, value)?;
        let parsed = ed.parsed(i)?.clone();
        let mut v = json::parsed(&ed.pkg, i, &parsed);
        v["row"] = json::export_row(&ed.pkg, i);
        v["dirty"] = J::from(true);
        Ok(v)
    }

    fn object_rename(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let name = s(p, "name")?;
        let ed = self.editor(path)?;
        let old = ed.pkg.object_name(PackageIndex::export(i));
        ed.begin_undo(format!("rename {old} → {name}"));
        ed.rename(i, name)?;
        ed.mark_dirty(i);
        Ok(json::export_row(&ed.pkg, i))
    }

    fn object_delete(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let indices: Vec<usize> = p.get("indices").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().map(|x| x as usize)).collect()).unwrap_or_default();
        if indices.is_empty() {
            return Err("missing parameter indices".into());
        }
        let ed = self.editor(path)?;
        ed.begin_undo(format!("delete {} object(s)", indices.len()));
        let mut cleared = 0;
        for i in indices {
            if i >= ed.pkg.exports.len() {
                return Err(format!("export {i} out of range").into());
            }
            cleared += ed.delete(i)?;
        }
        Ok(json!({ "cleared": cleared }))
    }

    fn object_copy(&mut self, p: &J) -> Res<J> {
        let src_path = s(p, "src")?;
        let dst_path = s(p, "dst")?;
        let i = self.export_of(src_path, p)?;
        let deep = opt_b(p, "deep").unwrap_or(true);
        let outer = match p.get("outer") {
            Some(J::Number(n)) => Some(PackageIndex::export(n.as_u64().unwrap_or(0) as usize)),
            Some(J::String(st)) if st == "top" => Some(PackageIndex::NONE),
            _ => None,
        };
        if key(src_path) == key(dst_path) {
            return Err("source and destination are the same package".into());
        }
        let db = self.db()?;
        // Two editors of the same map cannot be borrowed at once: take the
        // destination out, work, put it back.
        let dk = key(dst_path);
        self.editor(dst_path)?;
        let mut dst = self.editors.remove(&dk).unwrap();
        let src = match self.editor(src_path) {
            Ok(e) => e,
            Err(e) => {
                self.editors.insert(dk, dst);
                return Err(e);
            }
        };
        dst.begin_undo(format!("copy {}", src.pkg.object_name(PackageIndex::export(i))));
        let r = dst.copy_from(src, i, outer, deep);
        let _ = db;
        let result = match r {
            Ok((idx, warnings)) => Ok(json!({ "index": idx.export_index(), "path": dst.pkg.object_path(idx), "warnings": warnings })),
            Err(e) => Err(e.into()),
        };
        self.editors.insert(dk, dst);
        result
    }

    fn object_deps(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let ed = self.editor(path)?;
        let refs = match ed.parsed(i)? {
            Parsed::Object(o) => references(o),
            _ => Vec::new(),
        };
        let references: Vec<J> = refs.iter().map(|r| json::reference(&ed.pkg, *r)).collect();
        let back = referrers(ed, i);
        let referrers: Vec<J> = back.iter().map(|j| json::reference(&ed.pkg, PackageIndex::export(*j))).collect();
        let e = &ed.pkg.exports[i];
        Ok(json!({
            "class": json::reference(&ed.pkg, e.class),
            "outer": json::reference(&ed.pkg, e.outer),
            "archetype": json::reference(&ed.pkg, e.archetype),
            "super": json::reference(&ed.pkg, e.super_),
            "references": references,
            "referrers": referrers,
        }))
    }

    fn object_extract(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let out = s(p, "out")?;
        let i = self.export_of(path, p)?;
        let ed = self.editor(path)?;
        let data = &ed.pkg.export_data[i];
        std::fs::write(out, data)?;
        Ok(json!({ "out": out, "bytes": data.len() }))
    }

    // ----- textures ---------------------------------------------------------

    fn texture_object(&mut self, path: &str, i: usize) -> Res<crate::object::Object> {
        let ed = self.editor(path)?;
        let o = ed.object(i)?;
        if !matches!(o.native, Native::Texture2D(_)) {
            return Err(format!("{} is not a decoded Texture2D", ed.pkg.full_name(PackageIndex::export(i))).into());
        }
        Ok(o.clone())
    }

    fn texture_info(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let o = self.texture_object(path, i)?;
        let db = self.db()?;
        let src = self.src.as_ref().ok_or("no workspace")?;
        let ed = self.editors.get(&key(path)).unwrap();
        let info = texture_ops::texture_info(db, &ed.pkg, &ed.name, src, i, &o)?;
        Ok(json!({
            "path": info.path,
            "format": info.format_name,
            "supported": info.format.is_some(),
            "width": info.width,
            "height": info.height,
            "lightmap": info.lightmap,
            "mips": info.mips.iter().map(|m| {
                let loc = match &m.location {
                    MipLocation::Inline => json!({ "kind": "inline" }),
                    MipLocation::Unused => json!({ "kind": "unused" }),
                    MipLocation::Missing => json!({ "kind": "missing" }),
                    MipLocation::Cache { file, offset, size_on_disk, size } => json!({ "kind": "cache", "file": file, "offset": offset, "sizeOnDisk": size_on_disk, "size": size }),
                };
                json!({ "index": m.index, "width": m.width, "height": m.height, "location": loc, "storedBytes": m.stored_bytes })
            }).collect::<Vec<_>>(),
        }))
    }

    fn texture_preview(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let mip = opt_u(p, "mip").unwrap_or(0);
        let o = self.texture_object(path, i)?;
        let db = self.db()?;
        let src = self.src.as_ref().ok_or("no workspace")?;
        let ed = self.editors.get(&key(path)).unwrap();
        let png = texture_ops::export_png(db, &ed.pkg, &ed.name, src, i, &o, mip)?;
        Ok(json!({ "png": json::base64(&png), "bytes": png.len() }))
    }

    fn texture_export(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let out = s(p, "out")?;
        let i = self.export_of(path, p)?;
        let mip = opt_u(p, "mip").unwrap_or(0);
        let o = self.texture_object(path, i)?;
        let db = self.db()?;
        let src = self.src.as_ref().ok_or("no workspace")?;
        let ed = self.editors.get(&key(path)).unwrap();
        let bytes = if out.to_ascii_lowercase().ends_with(".png") { texture_ops::export_png(db, &ed.pkg, &ed.name, src, i, &o, mip)? } else { texture_ops::export_dds(db, &ed.pkg, &ed.name, src, i, &o)? };
        std::fs::write(out, &bytes)?;
        Ok(json!({ "out": out, "bytes": bytes.len() }))
    }

    fn texture_export_all(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let outdir = PathBuf::from(s(p, "outDir")?);
        let png = opt_b(p, "png").unwrap_or(true);
        let db = self.db()?;
        std::fs::create_dir_all(&outdir)?;
        // Parse every texture first (needs the editor mutably), then export
        // them against the texture caches.
        let textures: Vec<(usize, crate::object::Object)> = {
            let ed = self.editor(path)?;
            let mut v = Vec::new();
            for i in 0..ed.pkg.exports.len() {
                if !db.class_is_a(&ed.pkg.class_name_of(PackageIndex::export(i)), "Texture2D") {
                    continue;
                }
                if let Ok(o) = ed.object(i) {
                    if matches!(o.native, Native::Texture2D(_)) {
                        v.push((i, o.clone()));
                    }
                }
            }
            v
        };
        let src = self.src()?;
        let ed = self.editors.get(&key(path)).ok_or("package not open")?;
        let (mut ok, mut failed) = (0, 0);
        let mut errors = Vec::new();
        for (i, o) in &textures {
            let name = ed.pkg.object_path(PackageIndex::export(*i)).replace(['/', '\\', ':'], "_");
            let r = if png { texture_ops::export_png(db, &ed.pkg, &ed.name, src, *i, o, 0).map(|b| (b, format!("{name}.png"))) } else { texture_ops::export_dds(db, &ed.pkg, &ed.name, src, *i, o).map(|b| (b, format!("{name}.dds"))) };
            match r {
                Ok((bytes, file)) => {
                    std::fs::write(outdir.join(file), bytes)?;
                    ok += 1;
                }
                Err(e) => {
                    failed += 1;
                    if errors.len() < 20 {
                        errors.push(format!("{name}: {e}"));
                    }
                }
            }
        }
        Ok(json!({ "exported": ok, "failed": failed, "errors": errors, "outDir": outdir.to_string_lossy() }))
    }

    fn texture_import(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let file = s(p, "file")?;
        let i = self.export_of(path, p)?;
        let mut o = self.texture_object(path, i)?;
        let db = self.db()?;
        let mut opts = ImportOptions { mips: opt_b(p, "mips"), format: None };
        if let Some(f) = opt_s(p, "format").filter(|f| !f.is_empty()) {
            opts.format = Some(crate::dds::PixelFormat::from_name(&f).ok_or_else(|| format!("unknown pixel format {f}"))?);
        }
        let bytes = std::fs::read(file)?;
        let source = if file.to_ascii_lowercase().ends_with(".dds") {
            let d = crate::dds::read_dds(&bytes)?;
            ImportSource::Encoded { format: d.format, width: d.width, height: d.height, mips: d.mips }
        } else {
            let (w, h, rgba) = crate::dds::read_png(&bytes)?;
            ImportSource::Rgba { width: w, height: h, rgba }
        };
        let ed = self.editor(path)?;
        ed.begin_undo(format!("replace texture {}", ed.pkg.object_name(PackageIndex::export(i))));
        texture_ops::import(db, &mut ed.pkg, i, &mut o, source, &opts)?;
        ed.replace_object(i, o)?;
        ed.mark_dirty(i);
        self.texture_info(p)
    }

    // ----- meshes -----------------------------------------------------------

    fn mesh_object(&mut self, path: &str, i: usize) -> Res<crate::object::Object> {
        let ed = self.editor(path)?;
        let o = ed.object(i)?;
        if !matches!(o.native, Native::StaticMesh(_) | Native::SkeletalMesh(_)) {
            return Err(format!("{} is not a decoded static or skeletal mesh", ed.pkg.full_name(PackageIndex::export(i))).into());
        }
        Ok(o.clone())
    }

    fn mesh_info(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let o = self.mesh_object(path, i)?;
        let ed = self.editors.get(&key(path)).unwrap();
        let mut lods = Vec::new();
        for lod in 0..mesh_ops::lod_count(&o) {
            match mesh_ops::geometry(&ed.pkg, &o, lod) {
                Ok(g) => lods.push(json!({
                    "index": lod,
                    "vertices": g.positions.len(),
                    "triangles": g.indices.len() / 3,
                    "bones": g.bone_names.len(),
                    "sections": g.sections.iter().map(|sct| json!({ "material": json::reference(&ed.pkg, sct.material), "triangles": sct.num_triangles })).collect::<Vec<_>>(),
                })),
                Err(e) => lods.push(json!({ "index": lod, "error": e.to_string() })),
            }
        }
        Ok(json!({ "skeletal": matches!(o.native, Native::SkeletalMesh(_)), "lods": lods }))
    }

    /// GLB bytes of a mesh (`preview`: base64 in the response).
    fn mesh_glb(&mut self, p: &J, preview: bool) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let lod = opt_u(p, "lod").unwrap_or(0);
        let textures = opt_b(p, "textures").unwrap_or(true);
        let skin = opt_b(p, "skin").unwrap_or(true);
        let o = self.mesh_object(path, i)?;
        let db = self.db()?;
        let dir = self.dir()?.to_path_buf();
        // Field borrows (not accessor methods): the PNG cache is taken mutably below.
        self.editors.get_mut(&key(path)).ok_or("package not open")?.flush();
        let src = self.src.as_ref().ok_or("no workspace open")?;
        let ed = self.editors.get(&key(path)).ok_or("package not open")?;
        let name = ed.pkg.object_name(PackageIndex::export(i));
        let mut g = mesh_ops::geometry(&ed.pkg, &o, lod)?;
        if !skin {
            g.bones.clear();
        }
        let mut warnings = Vec::new();
        if textures {
            // Previews use ≤ 1024² textures (fast PNG, cached); exports the full mips.
            let max_size = if preview { Some(opt_u(p, "textureSize").unwrap_or(1024) as u32) } else { None };
            let mut cache = PackageCache::new(db, &dir);
            cache.insert(&ed.name, ed.pkg.clone());
            let mut png_cache = std::mem::take(&mut self.png_cache);
            warnings = mesh_ops::attach_materials(&mut g, &mut cache, src, &ed.name, max_size, &mut png_cache);
            if png_cache.len() > 256 {
                png_cache.clear();
            }
            self.png_cache = png_cache;
        }
        let bytes = mesh_ops::write_glb(&name, &g)?;
        let mut v = json!({ "bytes": bytes.len(), "vertices": g.positions.len(), "triangles": g.indices.len() / 3, "materials": g.materials.len(), "images": g.images.len(), "warnings": warnings });
        if preview {
            v["glb"] = J::from(json::base64(&bytes));
        } else {
            v["data"] = J::from(json::base64(&bytes));
        }
        Ok(v)
    }

    fn mesh_export(&mut self, p: &J) -> Res<J> {
        let out = s(p, "out")?.to_string();
        if out.to_ascii_lowercase().ends_with(".obj") {
            let path = s(p, "path")?;
            let i = self.export_of(path, p)?;
            let lod = opt_u(p, "lod").unwrap_or(0);
            let o = self.mesh_object(path, i)?;
            let ed = self.editors.get(&key(path)).unwrap();
            let name = ed.pkg.object_name(PackageIndex::export(i));
            let g = mesh_ops::geometry(&ed.pkg, &o, lod)?;
            let text = mesh_ops::write_obj(&name, &g);
            std::fs::write(&out, text.as_bytes())?;
            return Ok(json!({ "out": out, "bytes": text.len(), "vertices": g.positions.len(), "triangles": g.indices.len() / 3 }));
        }
        let mut v = self.mesh_glb(p, false)?;
        let data = v["data"].as_str().unwrap_or("").to_string();
        std::fs::write(&out, base64_decode(&data))?;
        v["data"] = J::Null;
        v["out"] = J::from(out);
        Ok(v)
    }

    fn mesh_import(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let file = s(p, "file")?;
        let i = self.export_of(path, p)?;
        let lod = opt_u(p, "lod").unwrap_or(0);
        let mut o = self.mesh_object(path, i)?;
        let bytes = std::fs::read(file)?;
        let ed = self.editor(path)?;
        ed.begin_undo(format!("replace mesh {}", ed.pkg.object_name(PackageIndex::export(i))));
        let report = crate::mesh_import::import(&ed.pkg, &mut o, Path::new(file), &bytes, lod)?;
        ed.replace_object(i, o)?;
        ed.mark_dirty(i);
        let mut v = self.mesh_info(p)?;
        v["vertices"] = J::from(report.vertices);
        v["triangles"] = J::from(report.triangles);
        v["sections"] = J::from(report.sections);
        v["chunks"] = J::from(report.chunks);
        v["warnings"] = J::from(report.warnings);
        Ok(v)
    }

    fn object_replace_raw(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let file = s(p, "file")?;
        let i = self.export_of(path, p)?;
        let bytes = std::fs::read(file)?;
        let n = bytes.len();
        let ed = self.editor(path)?;
        ed.begin_undo(format!("replace raw payload of {}", ed.pkg.object_name(PackageIndex::export(i))));
        ed.set_raw(i, bytes)?;
        // Report whether the new payload still parses.
        let parsed = ed.parsed(i)?.clone();
        let kind = match &parsed {
            Parsed::Object(_) => "object",
            Parsed::Script(_) => "script",
            Parsed::Empty => "empty",
            Parsed::Raw { .. } => "raw",
        };
        let error = match &parsed {
            Parsed::Raw { error, .. } => Some(error.clone()),
            _ => None,
        };
        Ok(json!({ "bytes": n, "kind": kind, "error": error }))
    }

    // ----- media ------------------------------------------------------------

    fn media_kind(&mut self, path: &str, i: usize) -> Res<&'static str> {
        let ed = self.editor(path)?;
        let class = ed.pkg.class_name_of(PackageIndex::export(i));
        let db = ed.db;
        if db.class_is_a(&class, "SoundNodeWave") {
            Ok("sound")
        } else if db.class_is_a(&class, "SwfMovie") || db.class_is_a(&class, "GFxMovieInfo") {
            Ok("swf")
        } else {
            Err(format!("{class} is neither a SoundNodeWave nor a SwfMovie").into())
        }
    }

    fn media_preview(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let kind = self.media_kind(path, i)?;
        let ed = self.editor(path)?;
        let o = ed.object(i)?.clone();
        match kind {
            "sound" => {
                let ogg = media::sound_export(&ed.pkg, &o)?;
                let info = media::ogg_info(&ogg).ok();
                Ok(json!({ "kind": "sound", "ogg": json::base64(&ogg), "bytes": ogg.len(), "channels": info.as_ref().map(|i| i.channels), "sampleRate": info.as_ref().map(|i| i.sample_rate), "duration": info.as_ref().map(|i| i.duration()) }))
            }
            _ => {
                let gfx = media::swf_export(&ed.pkg, &o)?;
                let head = String::from_utf8_lossy(&gfx[..gfx.len().min(3)]).to_string();
                Ok(json!({ "kind": "swf", "bytes": gfx.len(), "signature": head }))
            }
        }
    }

    fn media_export(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let out = s(p, "out")?;
        let i = self.export_of(path, p)?;
        let kind = self.media_kind(path, i)?;
        let ed = self.editor(path)?;
        let o = ed.object(i)?.clone();
        let bytes = if kind == "sound" { media::sound_export(&ed.pkg, &o)? } else { media::swf_export(&ed.pkg, &o)? };
        std::fs::write(out, &bytes)?;
        Ok(json!({ "out": out, "bytes": bytes.len(), "kind": kind }))
    }

    fn media_import(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let file = s(p, "file")?;
        let i = self.export_of(path, p)?;
        let kind = self.media_kind(path, i)?;
        let bytes = std::fs::read(file)?;
        let ed = self.editor(path)?;
        ed.begin_undo(format!("replace {kind} {}", ed.pkg.object_name(PackageIndex::export(i))));
        let mut o = ed.object(i)?.clone();
        if kind == "sound" {
            media::sound_import(&mut ed.pkg, &mut o, &bytes)?;
        } else {
            media::swf_import(&mut ed.pkg, &mut o, &bytes)?;
        }
        ed.replace_object(i, o)?;
        ed.mark_dirty(i);
        Ok(json!({ "kind": kind, "bytes": bytes.len() }))
    }

    // ----- mods -------------------------------------------------------------

    fn mod_dir(&mut self, p: &J) -> Res<J> {
        let dir = PathBuf::from(s(p, "dir")?);
        let id = s(p, "id")?;
        let name = opt_s(p, "name").unwrap_or_else(|| id.to_string());
        let version = opt_s(p, "version").unwrap_or_else(|| "1.0.0".into());
        let author = opt_s(p, "author").filter(|a| !a.is_empty());
        let files: Vec<String> = p.get("files").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        if files.is_empty() {
            return Err("no package files given".into());
        }
        std::fs::create_dir_all(dir.join("files"))?;
        let esc = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let mut manifest = format!("[mod]\nid = {}\nname = {}\nversion = {}\n", esc(id), esc(&name), esc(&version));
        if let Some(a) = author {
            manifest.push_str(&format!("\n[[mod.authors]]\nname = {}\n", esc(&a)));
        }
        std::fs::write(dir.join("manifest.toml"), manifest)?;
        let mut added = Vec::new();
        for f in files {
            let p = Path::new(&f);
            let fname = p.file_name().ok_or("bad file name")?;
            std::fs::copy(p, dir.join("files").join(fname))?;
            added.push(fname.to_string_lossy().to_string());
        }
        Ok(json!({ "dir": dir.to_string_lossy(), "files": added }))
    }
}

impl Server {
    // ----- scripts ----------------------------------------------------------

    /// Decompiled sources of a package (cached).
    fn decompiled(&mut self, path: &str) -> Res<&mut DecompiledPackage> {
        let k = key(path);
        if !self.scripts.contains_key(&k) {
            let mut search = Vec::new();
            if let Some(d) = &self.dir {
                search.push(d.clone());
            }
            let dp = decompile_package(Path::new(path), &search)?;
            self.scripts.insert(k.clone(), dp);
        }
        Ok(self.scripts.get_mut(&k).unwrap())
    }

    /// The class export that owns export `i` (itself when it is a class).
    fn owner_class(pkg: &Package, i: usize) -> Option<usize> {
        let mut cur = i;
        let mut guard = 0;
        loop {
            let e = &pkg.exports[cur];
            // Cooked packages leave a class export's class null; compiled ones
            // may point it at Core.Class. Either way it is a UClass with a body.
            let has_body = e.serial_size > 0 || pkg.export_data.get(cur).map(|d| !d.is_empty()).unwrap_or(false);
            let is_class = has_body && (e.class.is_null() || pkg.class_name_of(PackageIndex::export(cur)).eq_ignore_ascii_case("Class"));
            if is_class {
                return Some(cur);
            }
            cur = e.outer.export_index()?;
            guard += 1;
            if guard > 64 {
                return None;
            }
        }
    }

    fn script_source(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let (class_index, member, kind) = {
            let ed = self.editor(path)?;
            // A class default object (`Default__X`) sits under the package: map it to its class.
            let cdo_class = {
                let n = ed.pkg.object_name(PackageIndex::export(i));
                n.strip_prefix("Default__").or_else(|| n.strip_prefix("default__")).and_then(|c| ed.pkg.find_export(c, Some("Class")))
            };
            let Some(c) = Server::owner_class(&ed.pkg, i).or(cdo_class) else {
                return Err(format!("{} is not inside a class", ed.pkg.full_name(PackageIndex::export(i))).into());
            };
            let kind = ed.pkg.class_name_of(PackageIndex::export(i));
            let member = if c == i { None } else { Some(ed.pkg.object_name(PackageIndex::export(i))) };
            (c, member, kind)
        };
        let class_name = {
            let ed = self.editor(path)?;
            ed.pkg.object_name(PackageIndex::export(class_index))
        };
        let dp = self.decompiled(path)?;
        let Some(c) = dp.class(&class_name) else {
            return Err(format!("{class_name} is not a top-level class of this package (only those decompile)").into());
        };
        Ok(json!({ "class": c.name, "classIndex": class_index, "source": c.source, "member": member, "kind": kind, "classes": dp.classes.len() }))
    }

    /// The build-order entry of a workspace script package.
    fn ordered(&mut self, name: &str) -> Res<OrderedPackage> {
        if self.order.is_none() {
            let dir = self.dir()?.to_path_buf();
            let found = script_packages(&dir)?;
            self.order = Some(build_order(&found)?);
        }
        self.order.as_ref().unwrap().iter().find(|o| o.name.eq_ignore_ascii_case(name)).cloned().ok_or_else(|| format!("{name} is not a script package of the workspace (no classes of its own)").into())
    }

    fn script_compile(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let class = s(p, "class")?.to_string();
        let source = s(p, "source")?.to_string();
        let (name, cooked, dirty) = {
            let ed = self.editor(path)?;
            // A previous recompile is fine (it came from these sources); other
            // unsaved edits would be lost in the rebuild.
            (ed.name.clone(), ed.pkg.summary.is_cooked(), ed.has_object_edits())
        };
        if dirty {
            return Err("the package has unsaved property or asset edits; save (or discard) them before recompiling its scripts".into());
        }
        // The file may carry another name than the package (Save as…): the
        // build order is keyed by the package's own name.
        let internal = self.editor(path)?.pkg.package_name();
        let op = match self.ordered(&name) {
            Ok(op) => op,
            Err(e) => self.ordered(&internal).map_err(|_| e)?,
        };
        // Sources: the decompiled package with the edited class replaced.
        let dp = self.decompiled(path)?;
        let Some(slot) = dp.classes.iter_mut().find(|c| c.name.eq_ignore_ascii_case(&class)) else {
            return Err(format!("no class {class} in {name}").into());
        };
        let previous = std::mem::replace(&mut slot.source, source);
        let root = std::env::temp_dir().join("tempest-studio").join(format!("src-{}", std::process::id()));
        let src_dir = match dp.write_to(&root) {
            Ok(d) => d,
            Err(e) => {
                dp.classes.iter_mut().find(|c| c.name.eq_ignore_ascii_case(&class)).unwrap().source = previous;
                return Err(e.into());
            }
        };
        let mut search = vec![self.dir()?.to_path_buf()];
        if let Some(d) = Path::new(path).parent() {
            search.push(d.to_path_buf());
        }
        let t = std::time::Instant::now();
        let opts = CompileOptions::for_replacement(Path::new(path), cooked, op.deps.clone());
        let result = compile_package_dir(&src_dir, &search, opts);
        let _ = std::fs::remove_dir_all(&root);
        match result {
            Ok(r) => {
                let ed = self.editor(path)?;
                ed.begin_undo(format!("recompile {class}"));
                ed.replace_package(r.package);
                let mut v = json::tables(&ed.pkg);
                v["name"] = J::from(ed.name.clone());
                v["path"] = J::from(path);
                v["dirty"] = J::from(true);
                v["warnings"] = J::from(r.warnings);
                v["compileMs"] = J::from(t.elapsed().as_millis() as u64);
                Ok(v)
            }
            Err(e) => {
                // Keep the previous (compiling) text in the cache.
                if let Ok(dp) = self.decompiled(path) {
                    if let Some(c) = dp.classes.iter_mut().find(|c| c.name.eq_ignore_ascii_case(&class)) {
                        c.source = previous;
                    }
                }
                let mut v = json!({ "message": e });
                // "Error in X, Line N: msg"
                if let Some(rest) = e.strip_prefix("Error in ") {
                    if let Some((cls, tail)) = rest.split_once(", Line ") {
                        if let Some((line, msg)) = tail.split_once(": ") {
                            v["class"] = J::from(cls);
                            v["line"] = J::from(line.trim().parse::<u32>().unwrap_or(0));
                            v["detail"] = J::from(msg);
                        }
                    }
                }
                Err(format!("compile failed: {}", v).into())
            }
        }
    }

    fn script_export(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let out = PathBuf::from(s(p, "outDir")?);
        let dp = self.decompiled(path)?;
        let dir = dp.write_to(&out)?;
        Ok(json!({ "dir": dir.to_string_lossy(), "classes": dp.classes.len() }))
    }
}

impl Server {
    // ----- materials --------------------------------------------------------

    /// Resolved parameters of a material (instance) with preview PNGs of its textures.
    fn material_preview(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let max_size = opt_u(p, "textureSize").unwrap_or(512) as u32;
        let db = self.db()?;
        let dir = self.dir()?.to_path_buf();
        {
            let ed = self.editor(path)?;
            let class = ed.pkg.class_name_of(PackageIndex::export(i));
            if !db.class_is_a(&class, "MaterialInterface") {
                return Err(format!("{class} is not a material").into());
            }
        }
        // Unsaved edits live in the parsed objects: flush them so the cache's
        // copy of the package (parsed from export data) sees them.
        self.editors.get_mut(&key(path)).ok_or("package not open")?.flush();
        let src = self.src.as_ref().ok_or("no workspace open")?;
        let ed = self.editors.get(&key(path)).ok_or("package not open")?;
        let mut cache = PackageCache::new(db, &dir);
        cache.insert(&ed.name, ed.pkg.clone());
        let info = crate::material_ops::material_info(&mut cache, &ed.name, PackageIndex::export(i));
        let mut png_cache = std::mem::take(&mut self.png_cache);
        let mut textures = Vec::new();
        for t in &info.textures {
            let k = format!("{}@{}", t.path, max_size);
            let png = match png_cache.get(&k) {
                Some(b) => Ok(b.clone()),
                None => crate::material_ops::texture_png(&mut cache, src, &t.package, t.export, Some(max_size)),
            };
            let (png, error) = match png {
                Ok(b) => {
                    png_cache.insert(k, b.clone());
                    (Some(json::base64(&b)), None)
                }
                Err(e) => (None, Some(e.to_string())),
            };
            let slot = format!("{:?}", t.slot).to_ascii_lowercase();
            textures.push(json!({ "parameter": t.parameter, "slot": slot, "path": t.path, "package": t.package, "export": t.export, "png": png, "error": error }));
        }
        if png_cache.len() > 256 {
            png_cache.clear();
        }
        self.png_cache = png_cache;
        Ok(json!({
            "chain": info.chain.iter().map(|(p, o, c)| json!({ "package": p, "object": o, "class": c })).collect::<Vec<_>>(),
            "textures": textures,
            "scalars": info.scalars.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>(),
            "vectors": info.vectors.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>(),
            "switches": info.switches.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>(),
        }))
    }

    fn material_set_parameter(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let name = s(p, "name")?.to_string();
        let value = if let Some(v) = p.get("vector").and_then(|v| v.as_array()) {
            let mut c = [0f32; 4];
            for (k, x) in v.iter().take(4).enumerate() {
                c[k] = x.as_f64().unwrap_or(0.0) as f32;
            }
            crate::material_ops::ParamValue::Vector(c)
        } else if let Some(f) = p.get("scalar").and_then(|v| v.as_f64()) {
            crate::material_ops::ParamValue::Scalar(f as f32)
        } else {
            return Err("give scalar or vector".into());
        };
        let ed = self.editor(path)?;
        ed.begin_undo(format!("set parameter {name}"));
        let changed = crate::material_ops::set_parameter(ed, i, &name, value)?;
        let mut v = self.material_preview(p)?;
        v["changed"] = J::from(changed);
        Ok(v)
    }

    // ----- bulk operations --------------------------------------------------

    fn find_replace(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let query = s(p, "query")?.to_string();
        let replacement = opt_s(p, "replacement").unwrap_or_default();
        let names = opt_b(p, "names").unwrap_or(false);
        let apply = opt_b(p, "apply").unwrap_or(false);
        let ed = self.editor(path)?;
        let matches = crate::batch::find(ed, &query, &replacement, names);
        if apply {
            ed.begin_undo(format!("replace {query} → {replacement}"));
        }
        let errors = if apply { crate::batch::replace_all(ed, &matches) } else { Vec::new() };
        let failed: Vec<usize> = errors.iter().map(|(m, _)| m.index).collect();
        let _ = failed;
        let rows: Vec<J> = matches.iter().map(|m| json!({ "index": m.index, "object": m.object, "prop": m.prop, "kind": m.kind, "current": m.current, "replacement": m.replacement })).collect();
        let errs: Vec<J> = errors.iter().map(|(m, e)| json!({ "index": m.index, "object": m.object, "prop": m.prop, "error": e })).collect();
        Ok(json!({ "matches": rows, "applied": if apply { matches.len() - errors.len() } else { 0 }, "errors": errs, "dirty": ed.is_dirty() }))
    }

    /// `batch.plan` (dry run) and `batch.run`: a folder of `<package>.<object>.<ext>` files.
    fn batch(&mut self, p: &J, run: bool) -> Res<J> {
        let dir = PathBuf::from(s(p, "dir")?);
        let recursive = opt_b(p, "recursive").unwrap_or(false);
        let out_dir = opt_s(p, "outDir").filter(|d| !d.is_empty()).map(PathBuf::from);
        let db = self.db()?;
        let packages: Vec<(String, PathBuf)> = self.packages.iter().map(|w| (w.name.clone(), w.path.clone())).collect();
        let files = crate::batch::collect_files(&dir, recursive);
        let entries = if run {
            // Unsaved edits of the touched packages would be overwritten: refuse.
            let plan = crate::batch::plan(db, &packages, &files);
            for e in &plan {
                if let Some(pp) = &e.package_path {
                    if let Some(ed) = self.editors.get(&key(&pp.to_string_lossy())) {
                        if ed.is_dirty() {
                            return Err(format!("{} has unsaved edits in the Studio; save or close it before running the batch", e.package).into());
                        }
                    }
                }
            }
            let mut events: Vec<(usize, usize, String)> = Vec::new();
            let entries = crate::batch::run(db, &packages, &files, out_dir.as_deref(), &mut |done, total, file| events.push((done, total, file.to_string())));
            for (done, total, file) in events {
                let mut f = Map::new();
                f.insert("done".into(), J::from(done));
                f.insert("total".into(), J::from(total));
                f.insert("file".into(), J::from(file));
                self.event("batch.progress", f);
            }
            // Packages saved in place changed on disk: drop their open editors.
            if out_dir.is_none() {
                for e in &entries {
                    if let Some(pp) = &e.package_path {
                        let k = key(&pp.to_string_lossy());
                        self.editors.remove(&k);
                        self.index.remove(&k);
                        self.scripts.remove(&k);
                    }
                }
            } else {
                self.packages = list_packages(self.dir()?);
            }
            entries
        } else {
            crate::batch::plan(db, &packages, &files)
        };
        let rows: Vec<J> = entries
            .iter()
            .map(|e| {
                json!({
                    "file": e.file.to_string_lossy(),
                    "name": e.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    "package": e.package,
                    "packagePath": e.package_path.as_ref().map(|p| p.to_string_lossy().to_string()),
                    "object": e.export.as_ref().map(|x| x.1.clone()),
                    "index": e.export.as_ref().map(|x| x.0),
                    "kind": e.kind.map(|k| k.label()),
                    "ok": e.ok,
                    "message": e.message,
                })
            })
            .collect();
        let ok = entries.iter().filter(|e| e.ok).count();
        Ok(json!({ "entries": rows, "ok": ok, "failed": entries.len() - ok, "ran": run }))
    }
}

fn list_packages(dir: &Path) -> Vec<WorkspacePackage> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let path = e.path();
            let ext = path.extension().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if ext != "upk" && ext != "u" {
                continue;
            }
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            let flags = Package::read_summary(&path).map(|s| s.package_flags).unwrap_or(0);
            out.push(WorkspacePackage { name: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), path, size, flags });
        }
    }
    out.sort_by_key(|p| p.name.to_ascii_lowercase());
    out
}

fn build_index(path: &Path) -> SearchIndex {
    let mut idx = SearchIndex { classes: Vec::new(), exports: Vec::new() };
    let Ok(pf) = PackageFile::open(path) else { return idx };
    let pkg = pf.tables();
    let mut class_ids: HashMap<String, u16> = HashMap::new();
    for i in 0..pkg.exports.len() {
        let r = PackageIndex::export(i);
        let class = pkg.class_name_of(r);
        let id = match class_ids.get(&class) {
            Some(id) => *id,
            None => {
                let id = idx.classes.len() as u16;
                idx.classes.push(class.clone());
                class_ids.insert(class, id);
                id
            }
        };
        idx.exports.push((id, pkg.object_path(r)));
    }
    idx
}

fn base64_decode(text: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 26,
            b'0'..=b'9' => (c - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let bytes: Vec<u8> = text.bytes().filter(|b| *b != b'=' && !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (k, c) in chunk.iter().enumerate() {
            n |= val(*c).unwrap_or(0) << (18 - 6 * k);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    out
}

/// Entry point of `upk serve`.
pub fn main() {
    let mut server = Server::new();
    eprintln!("upk serve: ready (pid {})", std::process::id());
    server.run();
}
