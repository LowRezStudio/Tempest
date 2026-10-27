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
use crate::object::{package_layout, parse_export, Native, Parsed};
use crate::reflect::ClassDb;
use crate::texcache::{MipLocation, TextureSource};
use crate::texture_ops::{self, ImportOptions, ImportSource};
use crate::verify::verify_package;
use crate::visit::references;
use crate::wwise::{akpk, bnk, fnv, vorbis};
use crate::audio;
use crate::mcts;

type Res<T> = Result<T, Box<dyn Error>>;

/// Binary payloads of the response being built. With framed responses
/// (`protocol {frames: true}`) each payload is sent raw after the result
/// JSON and the JSON holds a `{"$bin": n}` marker in its place; otherwise it
/// is inlined as base64 (scripted sessions, tests).
#[derive(Default)]
pub struct BinSink {
    pub frames: bool,
    items: Vec<Vec<u8>>,
}

impl BinSink {
    pub fn add(&mut self, bytes: Vec<u8>) -> J {
        if self.frames {
            self.items.push(bytes);
            json!({ "$bin": self.items.len() - 1 })
        } else {
            J::from(json::base64(&bytes))
        }
    }

    fn take(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.items)
    }
}

/// One package of the workspace directory.
struct WorkspacePackage {
    name: String,
    path: PathBuf,
    size: u64,
    flags: u32,
}

/// Searchable table of one package (built lazily by `workspace.search`).
#[derive(Clone)]
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
    /// Search index of every workspace package, built in the background
    /// from `workspace.open` on; joined by `index_all`.
    index_job: Option<std::thread::JoinHandle<HashMap<String, SearchIndex>>>,
    /// Decompiled sources per package (keyed like `editors`).
    scripts: HashMap<String, DecompiledPackage>,
    /// Script packages of the workspace in build order (lazily computed).
    order: Option<Vec<OrderedPackage>>,
    /// Preview PNGs of material textures (`mesh.preview`), by path and size.
    png_cache: HashMap<String, Vec<u8>>,
    out: std::io::Stdout,
    /// The last Wwise package opened (`(canonical path, package)`).
    wwise: Option<(String, akpk::Package)>,
    /// Wwise object names from the AkEvent / AkBank exports of the workspace,
    /// by FNV id (built from the search index on first use).
    ak_names: Option<HashMap<u32, String>>,
    /// The assembly and language marshals of the workspace (`mcts.open`).
    mcts: Option<mcts::workspace::Workspace>,
    /// Binary payloads of the response being built.
    bins: BinSink,
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
        Server { dir: None, db: None, src: None, packages: Vec::new(), editors: HashMap::new(), index: HashMap::new(), index_job: None, scripts: HashMap::new(), order: None, png_cache: HashMap::new(), out: std::io::stdout(), wwise: None, ak_names: None, mcts: None, bins: BinSink::default() }
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
            if method == "protocol" {
                self.bins.frames = params.get("frames").and_then(|v| v.as_bool()).unwrap_or(false);
                let frames = self.bins.frames;
                self.emit(&json!({ "id": id, "ok": true, "result": { "frames": frames } }));
                continue;
            }
            let t = std::time::Instant::now();
            match self.call(&method, &params) {
                Ok(r) => {
                    let ms = t.elapsed().as_millis() as u64;
                    if self.bins.frames {
                        let bins = self.bins.take();
                        self.emit_framed(&json!({ "id": id, "ok": true, "ms": ms }), &r, bins);
                    } else {
                        self.emit(&json!({ "id": id, "ok": true, "result": r, "ms": ms }));
                    }
                }
                Err(e) => {
                    self.bins.take();
                    self.emit(&json!({ "id": id, "ok": false, "error": e.to_string(), "ms": t.elapsed().as_millis() as u64 }));
                }
            }
        }
    }

    fn emit(&mut self, v: &J) {
        let mut text = v.to_string();
        text.push('\n');
        let _ = self.out.write_all(text.as_bytes());
        let _ = self.out.flush();
    }

    /// A framed response: the header line (with `len`, the byte length of
    /// the result JSON, and `bin`, the lengths of the payloads), the result
    /// JSON, then the payloads.
    fn emit_framed(&mut self, header: &J, result: &J, bins: Vec<Vec<u8>>) {
        let body = serde_json::to_vec(result).unwrap_or_default();
        let mut h = header.clone();
        h["len"] = json!(body.len());
        h["bin"] = json!(bins.iter().map(|b| b.len()).collect::<Vec<_>>());
        let mut text = h.to_string();
        text.push('\n');
        let _ = self.out.write_all(text.as_bytes());
        let _ = self.out.write_all(&body);
        for b in &bins {
            let _ = self.out.write_all(b);
        }
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
            "package.names" => {
                let ed = self.editor(s(p, "path")?)?;
                Ok(J::Array(ed.pkg.names.iter().map(|n| J::from(n.name.as_str())).collect()))
            }
            "package.export" => {
                let path = s(p, "path")?;
                let i = self.export_of(path, p)?;
                let ed = self.editor(path)?;
                Ok(json::export_row(&ed.pkg, i))
            }
            "workspace.packages" => Ok(self.workspace_packages()),
            "workspace.search" => self.workspace_search(p),
            "workspace.summary" => self.workspace_summary(s(p, "path")?),
            "package.open" => self.package_open(s(p, "path")?),
            "package.tables" => {
                let path = s(p, "path")?;
                let ed = self.editor(path)?;
                let mut v = json::tables_compact(&ed.pkg);
                v["name"] = J::from(ed.name.clone());
                v["path"] = J::from(path);
                v["dirty"] = J::from(ed.is_dirty());
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
            "object.list" => self.object_list(p),
            "object.rename" => self.object_rename(p),
            "object.delete" => self.object_delete(p),
            "object.copy" => self.object_copy(p),
            "object.duplicate" => {
                let path = s(p, "path")?.to_string();
                let i = self.export_of(&path, p)?;
                let ed = self.editor(&path)?;
                let name = ed.pkg.object_name(PackageIndex::export(i));
                ed.begin_undo(format!("duplicate {name}"));
                let n = ed.duplicate(i)?;
                Ok(json!({ "index": n, "path": ed.pkg.object_path(PackageIndex::export(n)) }))
            }
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
            "wwise.packages" => self.wwise_packages(),
            "wwise.open" => self.wwise_open(p),
            "wwise.preview" => self.wwise_preview(p),
            "wwise.export" => self.wwise_export(p),
            "wwise.import" => self.wwise_import(p),
            "wwise.forObject" => self.wwise_for_object(p),
            "wwise.replacePcm" => self.wwise_replace_pcm(p),
            "sound.replacePcm" => self.sound_replace_pcm(p),
            "particle.system" => self.particle_system(p),
            "mcts.open" => self.mcts_open(p),
            "mcts.summary" => Ok(self.mcts()?.summary()?),
            "mcts.doc" => Ok(self.mcts()?.doc_json(s(p, "doc")?)?),
            "mcts.table" => Ok(self.mcts()?.table(p)?),
            "mcts.rows" => Ok(self.mcts()?.rows(p)?),
            "mcts.row" => Ok(self.mcts()?.row(p)?),
            "mcts.rowFull" => Ok(self.mcts()?.row_full(p)?),
            "mcts.set" => Ok(self.mcts_mut()?.set(p)?),
            "mcts.setHeader" => Ok(self.mcts_mut()?.set_header(p)?),
            "mcts.insert" => Ok(self.mcts_mut()?.insert(p)?),
            "mcts.addRow" => Ok(self.mcts_mut()?.append_row(p)?),
            "mcts.usages" => Ok(self.mcts()?.usages(p)?),
            "mcts.delete" => Ok(self.mcts_mut()?.delete(p)?),
            "mcts.undo" => Ok(self.mcts_mut()?.undo(p)?),
            "mcts.save" => {
                // Saving in place: remember the file as it was first, so a
                // mod delta can be made from it later.
                if p.get("out").and_then(|v| v.as_str()).is_none() {
                    let id = p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly");
                    let ws = self.mcts()?;
                    let path = ws.doc(id)?.path.clone();
                    if let Err(e) = crate::basestore::record_mcts(&ws.tokens, &path) {
                        eprintln!("basestore: {}: {e}", path.display());
                    }
                }
                Ok(self.mcts_mut()?.save(p)?)
            }
            "mcts.idRange" => Ok(self.mcts_mut()?.set_id_range(p)?),
            "delta.status" => {
                let path = PathBuf::from(s(p, "path")?);
                let mut st = crate::basestore::status(&path);
                st["target"] = J::from(crate::basestore::game_relative(&path));
                Ok(st)
            }
            "delta.record" => self.delta_record(p),
            "delta.reset" => Ok(J::from(crate::basestore::remove(Path::new(s(p, "path")?)))),
            "twrk.save" => self.twrk_save(p),
            "mod.changes" => self.mod_changes(),
            "mod.cook" => self.mod_cook(p),
            "twrk.read" => Ok(crate::twrk::summary(&crate::twrk::Reader::open(Path::new(s(p, "path")?))?.manifest)),
            "twrk.apply" => self.twrk_apply(p),
            "delta.info" => {
                let d = crate::delta::Delta::read(s(p, "file")?)?;
                Ok(json!({ "kind": d.kind(), "target": d.target(), "header": d.header }))
            }
            "delta.create" => self.delta_create(p),
            "mcts.discard" => Ok(self.mcts_mut()?.discard(p)?),
            "mcts.find" => Ok(self.mcts()?.find(p)?),
            "mcts.messages" => Ok(self.mcts_mut()?.messages(p)?),
            "mcts.setMessage" => Ok(self.mcts_mut()?.set_message(p)?),
            "mcts.validValues" => Ok(self.mcts_mut()?.valid_values()?),
            "swf.info" => self.swf_info(p),
            "swf.bytes" => self.swf_bytes(p),
            "swf.player" => self.swf_player(p),
            "swf.libraries" => self.swf_libraries(p),
            "swf.script" => self.swf_script(p),
            "swf.apply" => self.swf_apply(p),
            "mcts.nextIds" => Ok(self.mcts_mut()?.next_ids_json()?),
            "mcts.create" => Ok(self.mcts_mut()?.create(p)?),
            "mcts.serverRows" => Ok(self.mcts()?.server_rows(p)?),
            "mcts.vendor" => Ok(self.mcts()?.vendor()?),
            "mcts.vendorAdd" => Ok(self.mcts_mut()?.vendor_add(p)?),
            "mcts.scaffold" => Ok(self.mcts_mut()?.scaffold(p)?),
            "mcts.close" => {
                self.mcts = None;
                Ok(json!(true))
            }
            "particle.setTable" => self.particle_set_table(p),
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

    // ----- mcts (assembly.dat, Lang_*.dat) -----------------------------------

    fn mcts(&self) -> Res<&mcts::workspace::Workspace> {
        self.mcts.as_ref().ok_or_else(|| "no assembly open (mcts.open first)".into())
    }

    fn mcts_mut(&mut self) -> Res<&mut mcts::workspace::Workspace> {
        self.mcts.as_mut().ok_or_else(|| "no assembly open (mcts.open first)".into())
    }

    /// Open the assembly and language files of a cooked folder (the
    /// workspace folder by default).
    fn mcts_open(&mut self, p: &J) -> Res<J> {
        let dir = match p.get("dir").and_then(|v| v.as_str()) {
            Some(d) => PathBuf::from(d),
            None => self.dir.clone().ok_or("no workspace open")?,
        };
        // Already open from this folder: keep the edits (a renderer reload
        // asks again) unless a reload is wanted.
        let reload = opt_b(p, "reload").unwrap_or(false);
        let same = self.mcts.as_ref().map(|w| w.dir == dir).unwrap_or(false);
        if !same || reload {
            self.mcts = Some(mcts::workspace::Workspace::open(&dir)?);
        }
        Ok(self.mcts()?.summary()?)
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
        let class_db_ms = t.elapsed().as_millis() as u64;
        self.editors.clear();
        self.index.clear();
        self.wwise = None;
        self.ak_names = None;
        self.mcts = None;
        self.scripts.clear();
        self.order = None;
        self.db = Some(db);
        self.src = Some(TextureSource::open(&dir)?);
        let tex_ms = t.elapsed().as_millis() as u64 - class_db_ms;
        self.dir = Some(dir.clone());
        self.packages = list_packages(&dir);
        let list_ms = t.elapsed().as_millis() as u64 - class_db_ms - tex_ms;
        // Index every package now (≈2 s for a full cooked folder) so the
        // first search, batch plan or cross-package texture lookup is instant.
        let paths: Vec<PathBuf> = self.packages.iter().map(|p| p.path.clone()).collect();
        let cache_file = index_cache_file(&dir);
        self.index_job = Some(std::thread::spawn(move || build_indexes(&paths, cache_file.as_deref())));
        Ok(json!({
            "dir": dir.to_string_lossy(),
            "scriptPackages": db.package_names(),
            "textureIndex": self.src.as_ref().map(|s| s.index.is_some()).unwrap_or(false),
            "loadMs": t.elapsed().as_millis() as u64,
            "timing": { "classDb": class_db_ms, "textureIndex": tex_ms, "list": list_ms },
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
        self.index_all();
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
        let mut v = json::tables_compact(&ed.pkg);
        v["name"] = J::from(ed.name.clone());
        v["path"] = J::from(path);
        v["dirty"] = J::from(ed.is_dirty());
        v["loadMs"] = J::from(t.elapsed().as_millis() as u64);
        Ok(v)
    }

    fn package_save(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let out = opt_s(p, "out").unwrap_or_else(|| path.to_string());
        let in_place = key(&out) == key(path);
        if in_place {
            // The file as it is on disk now is the base a mod delta is made
            // against later; recorded once, before the first overwrite.
            let t = std::time::Instant::now();
            match crate::basestore::record_upk(self.db()?, Path::new(path)) {
                Ok(true) => eprintln!("basestore: recorded {} ({:.2}s)", path, t.elapsed().as_secs_f32()),
                Ok(false) => {}
                Err(e) => eprintln!("basestore: {path}: {e}"),
            }
        }
        let ed = self.editor(path)?;
        ed.save(&out)?;
        if !in_place {
            // Saved elsewhere: the open editor still describes `path`, and stays dirty.
            return Ok(json!({ "out": out, "reopened": false, "dirty": ed.is_dirty(), "undo": ed.undo_labels() }));
        }
        // Saved in place: the package stays open with its undo journal (the
        // in-memory copy is now what the file holds); disk-based caches go.
        ed.mark_saved();
        let labels = ed.undo_labels();
        let is_script = ed.pkg.summary.package_flags & pkgf::CONTAINS_SCRIPT != 0;
        let k = key(path);
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
        Ok(json!({ "out": out, "reopened": false, "dirty": false, "undo": labels, "dbReloaded": db_reloaded }))
    }

    /// `twrk.save {out, mode: "linked"|"portable", state, include?}`: write
    /// every unsaved package and marshal file of the session to a workspace
    /// file (see `twrk.rs`). `include` lists packages opened from outside the
    /// game folder that a portable workspace carries even when unchanged.
    fn twrk_save(&mut self, p: &J) -> Res<J> {
        use crate::twrk::{relative, Entry};
        let out = PathBuf::from(s(p, "out")?);
        let portable = opt_s(p, "mode").as_deref() == Some("portable");
        let dir = self.dir()?.to_path_buf();
        let root = crate::twrk::game_root(&dir);
        let db = self.db()?;
        let mut files: Vec<J> = Vec::new();
        let mut entries: Vec<Entry> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        let mut n = 0usize;
        let mut push = |files: &mut Vec<J>, entries: &mut Vec<Entry>, mut meta: J, bytes: Vec<u8>, base: Option<J>| {
            n += 1;
            let stored = meta["stored"].as_str().unwrap_or("full").to_string();
            let name = Path::new(meta["path"].as_str().unwrap_or("file")).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let entry = if stored == "delta" { format!("deltas/{n:04}-{name}.tdelta") } else { format!("files/{n:04}-{name}") };
            meta["entry"] = J::from(entry.clone());
            meta["bytes"] = J::from(bytes.len());
            entries.push(Entry { meta: meta.clone(), bytes });
            if let Some(b) = base {
                let be = format!("bases/{n:04}-{name}.json");
                meta["base"] = J::from(be.clone());
                entries.push(Entry { meta: json!({ "entry": be }), bytes: serde_json::to_vec(&b).unwrap_or_default() });
            }
            files.push(meta);
        };
        // Packages.
        let keys: Vec<String> = self.editors.iter().filter(|(_, e)| e.is_dirty()).map(|(k, _)| k.clone()).collect();
        for k in &keys {
            let path = PathBuf::from(k);
            let rel = relative(&root, &path);
            let ed = self.editors.get_mut(k).unwrap();
            let is_script = ed.pkg.summary.package_flags & pkgf::CONTAINS_SCRIPT != 0;
            let target = rel.clone().unwrap_or_else(|| crate::basestore::game_relative(&path));
            let meta = json!({ "kind": "upk", "path": path.to_string_lossy(), "rel": rel });
            let mut delta = None;
            if !portable && !is_script && path.exists() {
                // Compared as Save would write it: the in-memory package can
                // differ from its saved form in ways a delta would count.
                let edited = Package::parse(&ed.bytes());
                match tempest_upk::PackageFile::open(&path).and_then(|f| f.load()).and_then(|b| edited.map(|e| (b, e))) {
                    Ok((base, edited)) => match crate::delta::create(db, crate::delta::Base::Package(&base), &edited, &target) {
                        Ok((d, w)) if w.is_empty() => delta = Some(d.to_bytes()),
                        Ok((_, w)) => warnings.push(format!("{}: stored whole ({})", ed.name, w.join("; "))),
                        Err(e) => warnings.push(format!("{}: stored whole ({e})", ed.name)),
                    },
                    Err(e) => warnings.push(format!("{}: stored whole ({e})", ed.name)),
                }
            }
            let base = if portable { crate::basestore::read(&path) } else { None };
            match delta {
                Some(bytes) => push(&mut files, &mut entries, with_field(&meta, "stored", "delta"), bytes, base),
                None => {
                    let bytes = ed.bytes();
                    push(&mut files, &mut entries, with_field(&meta, "stored", "full"), bytes, base);
                }
            }
        }
        // Packages from outside the game folder, carried by portable workspaces.
        if portable {
            for f in p.get("include").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                let Some(f) = f.as_str() else { continue };
                if keys.contains(&key(f)) || relative(&root, Path::new(f)).is_some() {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(f) {
                    push(&mut files, &mut entries, json!({ "kind": "upk", "path": f, "rel": null, "stored": "full", "unchanged": true }), bytes, None);
                }
            }
        }
        // Marshal files.
        if let Some(ws) = self.mcts.as_mut() {
            let tokens = ws.tokens.clone();
            for (id, doc) in ws.docs.iter_mut() {
                if !doc.dirty {
                    continue;
                }
                let rel = relative(&root, &doc.path);
                let target = rel.clone().unwrap_or_else(|| crate::basestore::game_relative(&doc.path));
                let meta = json!({ "kind": "mcts", "doc": id, "path": doc.path.to_string_lossy(), "rel": rel });
                let mut delta = None;
                if !portable {
                    let base = std::fs::read(&doc.path).map_err(tempest_upk::io::Error::from).and_then(|b| mcts::parse_obscured(&b, &tokens));
                    match base {
                        Ok(base) => match mcts::delta::create(&tokens, mcts::delta::Base::Marshal(&base), &doc.marshal, &target) {
                            Ok(d) => delta = Some(d.to_bytes()),
                            Err(e) => warnings.push(format!("{id}: stored whole ({e})")),
                        },
                        Err(e) => warnings.push(format!("{id}: stored whole ({e})")),
                    }
                }
                let base = if portable { crate::basestore::read(&doc.path) } else { None };
                match delta {
                    Some(bytes) => push(&mut files, &mut entries, with_field(&meta, "stored", "delta"), bytes, base),
                    None => {
                        let bytes = mcts::write_obscured(&doc.marshal, &tokens);
                        push(&mut files, &mut entries, with_field(&meta, "stored", "full"), bytes, base);
                    }
                }
            }
        }
        let saved = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let manifest = json!({
            "format": crate::twrk::FORMAT,
            "mode": if portable { "portable" } else { "linked" },
            "gameDir": dir.to_string_lossy(),
            "saved": saved,
            "files": files,
            "state": p.get("state").cloned().unwrap_or(J::Null),
        });
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::twrk::write(&out, &manifest, &entries)?;
        let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
        Ok(json!({ "out": out.to_string_lossy(), "mode": manifest["mode"], "files": manifest["files"], "size": size, "warnings": warnings }))
    }

    /// `twrk.apply {path}`: put a workspace file's changes into the open
    /// workspace (which must be its game folder, or another copy of the game
    /// for a portable one): deltas are applied, whole files replace the
    /// in-memory package or marshal. Everything ends up unsaved, as it was.
    fn twrk_apply(&mut self, p: &J) -> Res<J> {
        let mut r = crate::twrk::Reader::open(Path::new(s(p, "path")?))?;
        let dir = self.dir()?.to_path_buf();
        let root = crate::twrk::game_root(&dir);
        let db = self.db()?;
        let files: Vec<J> = r.manifest["files"].as_array().cloned().unwrap_or_default();
        let mut packages: Vec<J> = Vec::new();
        let mut docs: Vec<String> = Vec::new();
        let mut extras: Vec<String> = Vec::new();
        let mut problems: Vec<String> = Vec::new();
        for meta in &files {
            let stored = meta["stored"].as_str().unwrap_or("full");
            let entry = meta["entry"].as_str().unwrap_or_default().to_string();
            let mut target = crate::twrk::resolve(&root, meta);
            let label = target.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let bytes = match r.bytes(&entry) {
                Ok(b) => b,
                Err(e) => {
                    problems.push(format!("{label}: {e}"));
                    continue;
                }
            };
            if let Some(be) = meta["base"].as_str() {
                if let Ok(b) = r.bytes(be) {
                    if let Ok(digest) = serde_json::from_slice::<J>(&b) {
                        let _ = crate::basestore::restore(&target, digest);
                    }
                }
            }
            if meta["kind"] == "mcts" {
                if self.mcts.is_none() {
                    self.mcts = Some(mcts::workspace::Workspace::open(&dir)?);
                }
                let ws = self.mcts.as_mut().unwrap();
                let id = meta["doc"].as_str().unwrap_or_default().to_string();
                let tokens = ws.tokens.clone();
                let doc = match ws.doc_mut(&id) {
                    Ok(d) => d,
                    Err(e) => {
                        problems.push(format!("{label}: {e}"));
                        continue;
                    }
                };
                let res: Res<()> = if stored == "delta" {
                    crate::delta::Delta::from_bytes(&bytes).map_err(Into::into).and_then(|d| {
                        let a = mcts::delta::apply(&tokens, &mut doc.marshal, &d)?;
                        if !a.drift.is_empty() {
                            problems.push(format!("{label}: {} rows changed on disk since the workspace was saved", a.drift.len()));
                        }
                        Ok(())
                    })
                } else {
                    mcts::parse_obscured(&bytes, &tokens).map(|m| doc.marshal = m).map_err(Into::into)
                };
                match res {
                    Ok(()) => {
                        doc.dirty = true;
                        docs.push(id);
                    }
                    Err(e) => problems.push(format!("{label}: {e}")),
                }
                continue;
            }
            // A package from outside the game folder that is not on this machine:
            // a portable workspace carries it; unpack it into the cache.
            if !target.exists() && stored == "full" && meta["rel"].is_null() {
                let Some(cache) = crate::basestore::cache_dir() else {
                    problems.push(format!("{label}: not found"));
                    continue;
                };
                let d = cache.join("twrk").join(format!("{:08x}", meta["path"].as_str().unwrap_or_default().bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))));
                std::fs::create_dir_all(&d)?;
                target = d.join(&label);
                std::fs::write(&target, &bytes)?;
            }
            if meta["unchanged"].as_bool().unwrap_or(false) {
                extras.push(target.to_string_lossy().to_string());
                continue;
            }
            if !target.exists() {
                problems.push(format!("{label}: {} not found", target.display()));
                continue;
            }
            let path = target.to_string_lossy().to_string();
            if crate::twrk::relative(&root, &target).is_none() {
                extras.push(path.clone());
            }
            let ed = match self.editor(&path) {
                Ok(e) => e,
                Err(e) => {
                    problems.push(format!("{label}: {e}"));
                    continue;
                }
            };
            ed.begin_undo("load workspace");
            let res: Res<usize> = if stored == "delta" {
                crate::delta::Delta::from_bytes(&bytes).map_err(Into::into).and_then(|d| {
                    let a = crate::delta::apply(db, ed, &d, crate::delta::Phase::All)?;
                    if !a.drift.is_empty() {
                        problems.push(format!("{label}: {} objects changed on disk since the workspace was saved", a.drift.len()));
                    }
                    Ok(a.added + a.replaced + a.deleted)
                })
            } else {
                Package::parse(&bytes).map(|pkg| {
                    ed.replace_package(pkg);
                    0
                }).map_err(Into::into)
            };
            ed.end_undo();
            match res {
                Ok(changes) => packages.push(json!({ "path": path, "stored": stored, "changes": changes, "dirty": ed.is_dirty() })),
                Err(e) => problems.push(format!("{label}: {e}")),
            }
        }
        Ok(json!({ "packages": packages, "docs": docs, "extras": extras, "problems": problems, "state": r.manifest["state"], "mode": r.manifest["mode"] }))
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

    /// `object.list {path, index, prop, op: add|duplicate|remove|removeProperty, at?}`.
    fn object_list(&mut self, p: &J) -> Res<J> {
        use crate::edit::ListOp;
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let prop = s(p, "prop")?;
        let at = p.get("at").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let op = match s(p, "op")? {
            "add" => ListOp::Add,
            "duplicate" => ListOp::Duplicate(at),
            "remove" => ListOp::Remove(at),
            "removeProperty" => ListOp::RemoveProperty,
            other => return Err(format!("unknown op {other}").into()),
        };
        let ed = self.editor(path)?;
        let label = match op {
            ListOp::Add => format!("add {prop} element"),
            ListOp::Duplicate(n) => format!("duplicate {prop}[{n}]"),
            ListOp::Remove(n) => format!("remove {prop}[{n}]"),
            ListOp::RemoveProperty => format!("reset {prop} to default"),
        };
        ed.begin_undo(label);
        ed.edit_list(i, prop, op)?;
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
        let bytes = png.len();
        Ok(json!({ "png": self.bins.add(png), "bytes": bytes }))
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
        let (bytes, mut v) = self.mesh_glb_bytes(p, preview)?;
        v["glb"] = self.bins.add(bytes);
        Ok(v)
    }

    /// The GLB bytes and the summary of a mesh.
    fn mesh_glb_bytes(&mut self, p: &J, preview: bool) -> Res<(Vec<u8>, J)> {
        let (name, g, warnings) = self.mesh_geometry(p, preview)?;
        let bytes = mesh_ops::write_glb(&name, &g)?;
        let v = json!({ "bytes": bytes.len(), "vertices": g.positions.len(), "triangles": g.indices.len() / 3, "materials": g.materials.len(), "images": g.images.len(), "warnings": warnings });
        Ok((bytes, v))
    }

    /// Geometry of a mesh with its textured materials and skeleton
    /// (`preview`: textures bounded to `textureSize`, 1024 by default).
    fn mesh_geometry(&mut self, p: &J, preview: bool) -> Res<(String, mesh_ops::Geometry, Vec<String>)> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let lod = opt_u(p, "lod").unwrap_or(0);
        let textures = opt_b(p, "textures").unwrap_or(true);
        let skin = opt_b(p, "skin").unwrap_or(true);
        let o = self.mesh_object(path, i)?;
        let db = self.db()?;
        let dir = self.dir()?.to_path_buf();
        if textures {
            self.index_all();
        }
        // Field borrows (not accessor methods): the PNG cache is taken mutably below.
        self.editors.get_mut(&key(path)).ok_or("package not open")?.flush();
        let src = self.src.as_ref().ok_or("no workspace open")?;
        let ed = self.editors.get(&key(path)).ok_or("package not open")?;
        let (index, pkgs) = (&self.index, &self.packages);
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
            cache.set_lookup(|p| index_lookup(index, pkgs, p));
            cache.insert(&ed.name, ed.pkg.clone());
            let mut png_cache = std::mem::take(&mut self.png_cache);
            warnings = mesh_ops::attach_materials(&mut g, &mut cache, src, &ed.name, PackageIndex::export(i), max_size, &mut png_cache);
            if png_cache.len() > 256 {
                png_cache.clear();
            }
            self.png_cache = png_cache;
        }
        Ok((name, g, warnings))
    }

    fn mesh_export(&mut self, p: &J) -> Res<J> {
        let out = s(p, "out")?.to_string();
        if out.to_ascii_lowercase().ends_with(".fbx") {
            let (name, g, warnings) = self.mesh_geometry(p, false)?;
            let bytes = crate::mesh_fbx::write_fbx(&name, &g)?;
            std::fs::write(&out, &bytes)?;
            return Ok(json!({ "out": out, "bytes": bytes.len(), "vertices": g.positions.len(), "triangles": g.indices.len() / 3, "materials": g.materials.len(), "images": g.images.len(), "warnings": warnings }));
        }
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
            return Ok(json!({ "out": out, "bytes": text.len(), "vertices": g.positions.len(), "triangles": g.indices.len() / 3, "warnings": [] }));
        }
        // Written straight from the bytes: the response must not carry the
        // payload (under framed responses it would be a `$bin` marker, not base64).
        let (bytes, mut v) = self.mesh_glb_bytes(p, false)?;
        std::fs::write(&out, &bytes)?;
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
                let len = ogg.len();
                Ok(json!({ "kind": "sound", "ogg": self.bins.add(ogg), "bytes": len, "channels": info.as_ref().map(|i| i.channels), "sampleRate": info.as_ref().map(|i| i.sample_rate), "duration": info.as_ref().map(|i| i.duration()) }))
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
        let mut bytes = std::fs::read(file)?;
        if kind == "sound" && !bytes.starts_with(b"OggS") {
            // WAV or Wwise Vorbis: re-encode as Ogg Vorbis.
            let quality = p.get("quality").and_then(|q| q.as_f64()).unwrap_or(0.7) as f32;
            bytes = audio::encode_ogg(&audio::decode_any(&bytes)?, quality)?;
        }
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

    // ----- Scaleform movies ------------------------------------------------------

    fn swf_movie(&mut self, p: &J) -> Res<(String, usize, crate::swf::Movie)> {
        let path = s(p, "path")?.to_string();
        let i = self.export_of(&path, p)?;
        let ed = self.editor(&path)?;
        let o = ed.object(i)?.clone();
        let bytes = media::swf_export(&ed.pkg, &o)?;
        Ok((path, i, crate::swf::Movie::parse(&bytes)?))
    }

    /// `swf.info`: tags, texts, scripts, external images, linkage of a movie.
    fn swf_info(&mut self, p: &J) -> Res<J> {
        let (_, _, m) = self.swf_movie(p)?;
        Ok(crate::swf::info(&m))
    }

    /// `swf.bytes`: the movie as the game's GFx loader receives it, for the
    /// Studio's Ruffle player.
    fn swf_bytes(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let i = self.export_of(&path, p)?;
        let bytes = {
            let ed = self.editor(&path)?;
            let o = ed.object(i)?.clone();
            media::swf_export(&ed.pkg, &o)?
        };
        let len = bytes.len();
        Ok(json!({ "swf": self.bins.add(bytes), "bytes": len }))
    }

    /// `swf.player`: the movie with its GFx external images embedded as
    /// standard `DefineBitsLossless2` bitmaps. The engine resolves a
    /// `DefineExternalImage2` (`Foo.tga`) to a `Texture2D` of the same package;
    /// Ruffle has no engine assets, so the texture is decoded and written into
    /// the movie instead (preview bytes only, the package is untouched).
    /// `DefineSubImage` crops (Scaleform atlases) become bitmaps of their own,
    /// because Ruffle has no sub-image tag.
    fn swf_player(&mut self, p: &J) -> Res<J> {
        let (path, _, mut m) = self.swf_movie(p)?;
        let mut images = 0usize;
        let mut missing: Vec<String> = Vec::new();
        if self.src.is_none() {
            let bytes = m.write();
            let len = bytes.len();
            return Ok(json!({ "swf": self.bins.add(bytes), "bytes": len, "images": 0, "missing": missing }));
        }
        {
            let db = self.db()?;
            let src = self.src.as_ref().unwrap();
            let ed = self.editors.get_mut(&key(&path)).ok_or("package is not open")?;
            let tags = std::mem::take(&mut m.tags);
            // Sub-images are crops of an external image; the shapes reference
            // the sub-image id, not the atlas.
            let mut sub_by_image: HashMap<u16, Vec<crate::swf::SubImage>> = HashMap::new();
            for tag in &tags {
                if tag.code == 1008 {
                    if let Some(sub) = crate::swf::parse_sub_image(&tag.data) {
                        sub_by_image.entry(sub.image_id).or_default().push(sub);
                    }
                }
            }
            let mut consumed: std::collections::HashSet<u16> = std::collections::HashSet::new();
            let mut out = Vec::with_capacity(tags.len());
            for tag in tags {
                if tag.code != 1001 && tag.code != 1009 {
                    out.push(tag);
                    continue;
                }
                let Some(img) = crate::swf::parse_external_image(tag.code, &tag.data) else {
                    out.push(tag);
                    continue;
                };
                // GFx external images use a 32-bit id whose low 16 bits are the
                // character id SWF tags refer to (sub-images, bitmap fills); the
                // high half is the image namespace (`0x0009_0000` in these movies).
                let image_id = (img.id & 0xFFFF) as u16;
                let file = img.file_name.rsplit('/').next().unwrap_or(&img.file_name);
                let stem = file.rsplit_once('.').map(|(name, _)| name).unwrap_or(file);
                let mut texture = None;
                for i in 0..ed.pkg.exports.len() {
                    let idx = PackageIndex::export(i);
                    if ed.pkg.object_name(idx).eq_ignore_ascii_case(stem) && ed.pkg.class_name_of(idx).eq_ignore_ascii_case("Texture2D") {
                        texture = Some(i);
                        break;
                    }
                }
                let Some(texture) = texture else {
                    missing.push(format!("{}: no Texture2D named {stem}", img.file_name));
                    out.push(tag);
                    continue;
                };
                let o = match ed.object(texture) {
                    Ok(o) => o.clone(),
                    Err(e) => {
                        missing.push(format!("{}: {e}", img.file_name));
                        out.push(tag);
                        continue;
                    }
                };
                let (tw, th, rgba) = match crate::texture_ops::largest_rgba(db, &ed.pkg, &ed.name, src, texture, &o) {
                    Ok(v) => v,
                    Err(e) => {
                        missing.push(format!("{}: {e}", img.file_name));
                        out.push(tag);
                        continue;
                    }
                };
                if tw == 0 || th == 0 {
                    out.push(tag);
                    continue;
                }
                if let Some(subs) = sub_by_image.remove(&image_id) {
                    // The atlas rect is in target pixels; DXT textures can be
                    // up to 3 px larger (block padding), so scale into the
                    // decoded texture and crop there.
                    let scale_x = |v: u16| ((v as u64 * tw as u64) / (img.width.max(1) as u64)) as u16;
                    let scale_y = |v: u16| ((v as u64 * th as u64) / (img.height.max(1) as u64)) as u16;
                    for sub in subs {
                        let Some((w, h, pixels)) = crop_rgba(&rgba, tw, th, scale_x(sub.x1), scale_y(sub.y1), scale_x(sub.x2), scale_y(sub.y2)) else {
                            missing.push(format!("{}: empty crop {}x{}", img.file_name, sub.x1, sub.y1));
                            continue;
                        };
                        out.push(crate::swf::Tag { code: 36, data: define_bits_lossless2(sub.id, w, h, &pixels), long: true });
                        consumed.insert(sub.id);
                        images += 1;
                    }
                    // The atlas tag itself is no longer needed: only its
                    // sub-images are referenced.
                } else if img.width > 0 && img.height > 0 {
                    let (w, h, pixels) = fit_rgba(&rgba, tw, th, img.width as u32, img.height as u32);
                    out.push(crate::swf::Tag { code: 36, data: define_bits_lossless2(image_id, w, h, &pixels), long: true });
                    images += 1;
                } else {
                    out.push(tag);
                }
            }
            // Drop the sub-image tags whose cropped bitmap was emitted.
            out.retain(|t| t.code != 1008 || !crate::swf::parse_sub_image(&t.data).is_some_and(|s| consumed.contains(&s.id)));
            m.tags = out;
        }
        let bytes = m.write();
        let len = bytes.len();
        Ok(json!({ "swf": self.bins.add(bytes), "bytes": len, "images": images, "missing": missing }))
    }

    /// `swf.libraries`: the movies this movie imports (`ImportAssets2` URLs such
    /// as `../SharedAssets/SharedAssets.swf`), found by export name among the
    /// workspace's `SwfMovie`s and returned with their bytes. The Ruffle player
    /// serves them beside the movie so the shared symbols (frames, fonts, image
    /// stacks) draw instead of staying blank. Imports of imports are followed
    /// too: the shared libraries themselves pull in the font library.
    fn swf_libraries(&mut self, p: &J) -> Res<J> {
        let (_, _, m) = self.swf_movie(p)?;
        let mut pending = crate::swf::import_urls(&m);
        if pending.is_empty() {
            return Ok(json!({ "libraries": [], "missing": [] }));
        }
        self.index_all();
        let mut looked_up: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut emitted: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut libraries = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        while !pending.is_empty() {
            let wave: Vec<(String, String)> = pending
                .drain(..)
                .filter_map(|url| {
                    let stem = swf_stem(&url);
                    (!stem.is_empty() && looked_up.insert(stem.clone())).then_some((url, stem))
                })
                .collect();
            if wave.is_empty() {
                break;
            }
            let wanted: std::collections::HashSet<&str> = wave.iter().map(|(_, stem)| stem.as_str()).collect();
            // A library is looked up by the name its importer refers to (the
            // URL's file stem), e.g. `SharedAssets`, not by the package it
            // lives in.
            let mut found: HashMap<String, (PathBuf, usize, String)> = HashMap::new();
            for w in &self.packages {
                if found.len() == wanted.len() {
                    break;
                }
                let k = key(&w.path.to_string_lossy());
                let Some(idx) = self.index.get(&k) else { continue };
                for (i, (cid, object_path)) in idx.exports.iter().enumerate() {
                    if !idx.classes[*cid as usize].eq_ignore_ascii_case("SwfMovie") {
                        continue;
                    }
                    let name = object_path.rsplit('.').next().unwrap_or(object_path).to_ascii_lowercase();
                    if wanted.contains(name.as_str()) && !found.contains_key(&name) {
                        found.insert(name.clone(), (w.path.clone(), i, object_path.clone()));
                        if found.len() == wanted.len() {
                            break;
                        }
                    }
                }
            }
            for (url, stem) in wave {
                let Some((path, i, object)) = found.get(&stem) else {
                    if !missing.contains(&url) {
                        missing.push(url);
                    }
                    continue;
                };
                let path_text = path.to_string_lossy().to_string();
                if !emitted.insert(format!("{path_text}#{i}")) {
                    continue;
                }
                let bytes = {
                    let ed = self.editor(&path_text)?;
                    let o = ed.object(*i)?.clone();
                    media::swf_export(&ed.pkg, &o)?
                };
                if let Ok(shared) = crate::swf::Movie::parse(&bytes) {
                    pending.extend(crate::swf::import_urls(&shared));
                }
                let len = bytes.len();
                libraries.push(json!({
                    "url": url,
                    "path": path_text,
                    "object": object,
                    "index": i,
                    "bytes": len,
                    "swf": self.bins.add(bytes),
                }));
            }
        }
        Ok(json!({ "libraries": libraries, "missing": missing }))
    }

    /// `swf.script`: the disassembly of one script tag.
    fn swf_script(&mut self, p: &J) -> Res<J> {
        let (_, _, m) = self.swf_movie(p)?;
        let tag = opt_u(p, "tag").ok_or("missing parameter tag")?;
        Ok(json!({ "listing": crate::swf::script_listing(&m, tag)? }))
    }

    /// `swf.apply`: edits (`{kind: text|string|image, tag, index?, text}`) written back into the package (undoable).
    fn swf_apply(&mut self, p: &J) -> Res<J> {
        let (path, i, mut m) = self.swf_movie(p)?;
        let edits: Vec<J> = p.get("edits").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let n = crate::swf::apply(&mut m, &edits)?;
        let bytes = m.write();
        let ed = self.editor(&path)?;
        ed.begin_undo(format!("edit movie {}", ed.pkg.object_name(PackageIndex::export(i))));
        let mut o = ed.object(i)?.clone();
        media::swf_import(&mut ed.pkg, &mut o, &bytes)?;
        ed.replace_object(i, o)?;
        ed.mark_dirty(i);
        Ok(json!({ "edits": n, "bytes": bytes.len(), "undo": ed.undo_labels() }))
    }

    // ----- sound editing ------------------------------------------------------

    /// Interleaved 16-bit PCM (base64) → planar float.
    fn pcm_param(p: &J) -> Res<audio::Pcm> {
        let data = base64_decode(s(p, "pcm")?);
        let channels = opt_u(p, "channels").unwrap_or(1).max(1);
        let rate = opt_u(p, "sampleRate").unwrap_or(44100) as u32;
        if data.is_empty() {
            return Err("empty PCM".into());
        }
        Ok(audio::Pcm::from_interleaved_i16(&data, channels, rate))
    }

    /// `sound.replacePcm`: edited PCM from the sound editor, encoded as Ogg
    /// Vorbis into a SoundNodeWave.
    fn sound_replace_pcm(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?;
        let i = self.export_of(path, p)?;
        let kind = self.media_kind(path, i)?;
        if kind != "sound" {
            return Err("not a SoundNodeWave".into());
        }
        let pcm = Self::pcm_param(p)?;
        let quality = p.get("quality").and_then(|q| q.as_f64()).unwrap_or(0.7) as f32;
        let ogg = audio::encode_ogg(&pcm, quality)?;
        let ed = self.editor(path)?;
        ed.begin_undo(format!("edit sound {}", ed.pkg.object_name(PackageIndex::export(i))));
        let mut o = ed.object(i)?.clone();
        media::sound_import(&mut ed.pkg, &mut o, &ogg)?;
        ed.replace_object(i, o)?;
        ed.mark_dirty(i);
        Ok(json!({ "bytes": ogg.len(), "duration": pcm.duration(), "channels": pcm.channels.len(), "sampleRate": pcm.sample_rate }))
    }

    // ----- particles -----------------------------------------------------------

    /// Raw-distribution properties of a class default object chain, as
    /// JSON property views (child class wins): what a module's missing
    /// members and properties default to, since cooked objects only store
    /// what differs from their class defaults.
    fn cdo_distributions(db: &ClassDb, class: Option<tempest_unrealscript::context::ObjRef>) -> Vec<J> {
        let mut out: Vec<J> = Vec::new();
        let mut cur = class;
        for _ in 0..32 {
            let Some(c) = cur else { break };
            if let Some(cdo) = db.class_default_object(c) {
                let lp = &db.ctx.packages[cdo.pkg];
                if let Some(data) = lp.pkg.export_data.get(cdo.export) {
                    let layout = package_layout(&lp.pkg);
                    if let Parsed::Object(o) = parse_export(db, &lp.pkg, layout, cdo.export, data) {
                        for p in &o.properties {
                            let is_dist = p.struct_name.map(|n| lp.pkg.fname_to_string(n).to_ascii_lowercase().starts_with("rawdistribution")).unwrap_or(false);
                            if !is_dist || p.array_index != 0 {
                                continue;
                            }
                            let name = lp.pkg.fname_to_string(p.name).to_ascii_lowercase();
                            if out.iter().any(|j| j["name"].as_str().map(|n| n.to_ascii_lowercase()) == Some(name.clone())) {
                                continue;
                            }
                            out.push(json::property(&lp.pkg, p));
                        }
                    }
                }
            }
            cur = db.super_class(c);
        }
        out
    }

    /// Complete a parsed module with its class defaults: distribution
    /// members the instance does not store, and distribution properties it
    /// lacks altogether.
    fn with_defaults(mut parsed: J, defaults: &[J]) -> J {
        let Some(props) = parsed.get_mut("properties").and_then(|p| p.as_array_mut()) else { return parsed };
        for d in defaults {
            let dname = d["name"].as_str().unwrap_or("").to_ascii_lowercase();
            match props.iter_mut().find(|p| p["name"].as_str().map(|n| n.eq_ignore_ascii_case(&dname)).unwrap_or(false) && p["index"].as_i64() == Some(0)) {
                Some(p) => {
                    if p["value"]["t"] != "struct" {
                        continue;
                    }
                    let (Some(members), Some(dm)) = (p["value"]["v"].as_array_mut(), d["value"]["v"].as_array()) else { continue };
                    for m in dm {
                        let mname = m["name"].as_str().unwrap_or("");
                        if !members.iter().any(|x| x["name"].as_str().map(|n| n.eq_ignore_ascii_case(mname)).unwrap_or(false)) {
                            members.push(m.clone());
                        }
                    }
                }
                None => props.push(d.clone()),
            }
        }
        parsed
    }

    /// `particle.system`: the whole system owning an export (system, emitter,
    /// LOD level or module), every object as its parsed view, plus the sprite
    /// materials (blend mode, base texture) and the meshes of mesh emitters.
    fn particle_system(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let i = self.export_of(&path, p)?;
        let db = self.db()?;
        let dir = self.dir()?.to_path_buf();
        self.index_all();
        let max_size = opt_u(p, "textureSize").unwrap_or(256) as u32;
        let (root, tree, material_refs, mesh_refs, pkg_name, pkg_clone) = {
            let ed = self.editor(&path)?;
            ed.flush();
            let root = crate::particle::system_root(db, &ed.pkg, i).ok_or("not inside a particle system")?;
            let mut material_refs: Vec<PackageIndex> = Vec::new();
            let mut mesh_refs: Vec<PackageIndex> = Vec::new();
            let mut default_cache: HashMap<String, Vec<J>> = HashMap::new();
            let mut parsed_of = |ed: &mut Editor<'static>, idx: usize| -> Res<J> {
                let parsed = ed.parsed(idx)?.clone();
                let mut j = json::parsed(&ed.pkg, idx, &parsed);
                if let Parsed::Object(o) = &parsed {
                    let class_name = ed.pkg.class_name_of(PackageIndex::export(idx)).to_ascii_lowercase();
                    if class_name.starts_with("particlemodule") {
                        let defaults = default_cache.entry(class_name).or_insert_with(|| Self::cdo_distributions(db, o.class));
                        j = Self::with_defaults(j, defaults);
                    }
                }
                Ok(j)
            };
            let system_obj = ed.object(root)?.clone();
            let system = parsed_of(ed, root)?;
            let mut emitters = Vec::new();
            for e in crate::particle::object_array(&ed.pkg, &system_obj, "Emitters") {
                let Ok(eo) = ed.object(e).map(|o| o.clone()) else { continue };
                let mut lods = Vec::new();
                for l in crate::particle::object_array(&ed.pkg, &eo, "LODLevels") {
                    let Ok(lo) = ed.object(l).map(|o| o.clone()) else { continue };
                    let mut one = |name: &str| -> Res<J> {
                        match crate::particle::object_ref(&ed.pkg, &lo, name).and_then(|r| r.export_index()) {
                            Some(m) => {
                                let mo = ed.object(m)?.clone();
                                if let Some(mat) = crate::particle::object_ref(&ed.pkg, &mo, "Material") {
                                    if !material_refs.contains(&mat) {
                                        material_refs.push(mat);
                                    }
                                }
                                if let Some(mesh) = crate::particle::object_ref(&ed.pkg, &mo, "Mesh") {
                                    if !mesh_refs.contains(&mesh) {
                                        mesh_refs.push(mesh);
                                    }
                                }
                                parsed_of(ed, m)
                            }
                            None => Ok(J::Null),
                        }
                    };
                    let required = one("RequiredModule")?;
                    let spawn = one("SpawnModule")?;
                    let type_data = one("TypeDataModule")?;
                    let mut modules = Vec::new();
                    for m in crate::particle::object_array(&ed.pkg, &lo, "Modules") {
                        modules.push(parsed_of(ed, m)?);
                    }
                    lods.push(json!({ "index": l, "props": parsed_of(ed, l)?, "required": required, "spawn": spawn, "typeData": type_data, "modules": modules }));
                }
                emitters.push(json!({ "index": e, "path": ed.pkg.object_path(PackageIndex::export(e)), "class": ed.pkg.class_name_of(PackageIndex::export(e)), "props": parsed_of(ed, e)?, "lods": lods }));
            }
            (root, json!({ "system": system, "emitters": emitters }), material_refs, mesh_refs, ed.name.clone(), ed.pkg.clone())
        };
        // Materials: blend mode and the base texture, through the material chain.
        let src = self.src.as_ref().ok_or("no workspace open")?;
        let (index, pkgs) = (&self.index, &self.packages);
        let mut cache = PackageCache::new(db, &dir);
        cache.set_lookup(|p| index_lookup(index, pkgs, p));
        cache.insert(&pkg_name, pkg_clone.clone());
        let mut png_cache = std::mem::take(&mut self.png_cache);
        let mut sink = std::mem::take(&mut self.bins);
        let mut materials = Map::new();
        for m in &material_refs {
            let mpath = pkg_clone.object_path(*m);
            let info = crate::material_ops::material_info(&mut cache, &pkg_name, *m);
            let base = info.textures.iter().find(|t| t.slot == crate::material_ops::TextureSlot::BaseColor).or_else(|| info.textures.first());
            let png = base.and_then(|t| {
                let k = format!("{}@{}", t.path, max_size);
                match png_cache.get(&k) {
                    Some(b) => Some(sink.add(b.clone())),
                    None => crate::material_ops::texture_png(&mut cache, src, &t.package, t.export, Some(max_size)).ok().map(|b| {
                        png_cache.insert(k, b.clone());
                        sink.add(b)
                    }),
                }
            });
            let color = info.vectors.iter().find(|(n, _)| n.eq_ignore_ascii_case("Color") || n.eq_ignore_ascii_case("Tint") || n.eq_ignore_ascii_case("ParticleColor")).map(|(_, v)| *v);
            materials.insert(mpath, json!({ "blendMode": info.blend_mode, "twoSided": info.two_sided, "png": png, "texture": base.map(|t| t.path.clone()), "color": color, "chain": info.chain.len() }));
        }
        if png_cache.len() > 256 {
            png_cache.clear();
        }
        self.png_cache = png_cache;
        self.bins = sink;
        // Meshes: where the mesh export lives (same package or another one).
        let mut meshes = Map::new();
        for m in &mesh_refs {
            let mpath = pkg_clone.object_path(*m);
            if let Some((pname, e)) = cache.resolve(&pkg_name, *m) {
                let file = if pname.eq_ignore_ascii_case(&pkg_name) { path.clone() } else { pkgs.iter().find(|w| w.name.eq_ignore_ascii_case(&pname)).map(|w| w.path.to_string_lossy().to_string()).unwrap_or_default() };
                meshes.insert(mpath, json!({ "path": file, "index": e }));
            }
        }
        let mut out = tree;
        out["root"] = J::from(root);
        out["materials"] = J::Object(materials);
        out["meshes"] = J::Object(meshes);
        Ok(out)
    }

    /// `particle.setTable`: replace a module's baked distribution table.
    fn particle_set_table(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let i = self.export_of(&path, p)?;
        let prop = s(p, "prop")?.to_string();
        let table: Vec<f32> = p.get("table").and_then(|t| t.as_array()).ok_or("missing parameter table")?.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect();
        let f = |name: &str| p.get(name).and_then(|v| v.as_f64()).map(|v| v as f32);
        let b = |name: &str| p.get(name).and_then(|v| v.as_u64()).map(|v| v as u8);
        let edit = crate::particle::TableEdit { table, time_scale: f("timeScale"), start_time: f("startTime"), op: b("op"), num_elements: b("numElements"), chunk_size: b("chunkSize") };
        let ed = self.editor(&path)?;
        ed.begin_undo(format!("edit {prop} of {}", ed.pkg.object_name(PackageIndex::export(i))));
        let mut o = ed.object(i)?.clone();
        crate::particle::set_distribution(&mut ed.pkg, &mut o, &prop, &edit)?;
        ed.replace_object(i, o)?;
        ed.mark_dirty(i);
        let parsed = ed.parsed(i)?.clone();
        Ok(json::parsed(&ed.pkg, i, &parsed))
    }

    // ----- Wwise -------------------------------------------------------------

    /// `wwise.packages`: the `.pck` files of the workspace folder.
    fn wwise_packages(&mut self) -> Res<J> {
        let dir = self.dir()?.to_path_buf();
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let path = e.path();
                if path.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case("pck")).unwrap_or(false) {
                    out.push(json!({ "name": path.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(), "path": path.to_string_lossy(), "size": e.metadata().map(|m| m.len()).unwrap_or(0) }));
                }
            }
        }
        out.sort_by(|a, b| a["name"].as_str().unwrap_or("").to_ascii_lowercase().cmp(&b["name"].as_str().unwrap_or("").to_ascii_lowercase()));
        Ok(json!({ "packages": out }))
    }

    /// Names of the AkEvent and AkBank exports of the workspace, by Wwise id.
    fn ak_names(&mut self) -> &HashMap<u32, String> {
        if self.ak_names.is_none() {
            self.index_all();
            let mut names: HashMap<u32, String> = HashMap::new();
            for idx in self.index.values() {
                for (cid, path) in &idx.exports {
                    let class = &idx.classes[*cid as usize];
                    if class.eq_ignore_ascii_case("AkEvent") || class.eq_ignore_ascii_case("AkBank") {
                        let leaf = path.rsplit('.').next().unwrap_or(path);
                        names.entry(fnv::name_id(leaf)).or_insert_with(|| leaf.to_string());
                    }
                }
            }
            self.ak_names = Some(names);
        }
        self.ak_names.as_ref().unwrap()
    }

    fn wwise_package(&mut self, path: &str) -> Res<&akpk::Package> {
        let k = key(path);
        if self.wwise.as_ref().map(|(p, _)| p != &k).unwrap_or(true) {
            let pkg = akpk::Package::parse(&std::fs::read(path)?)?;
            self.wwise = Some((k, pkg));
        }
        Ok(&self.wwise.as_ref().unwrap().1)
    }

    /// Media of a package: `(id, bytes, bank id or None)`.
    fn wwise_media(pkg: &akpk::Package, id: u32) -> Option<(Vec<u8>, Option<u64>)> {
        for e in &pkg.banks {
            if let Ok(b) = bnk::Bank::parse(&e.data) {
                if let Some((_, d)) = b.media().into_iter().find(|(i, _)| *i == id) {
                    return Some((d.to_vec(), Some(e.id)));
                }
            }
        }
        pkg.streams.iter().find(|s| s.id == id as u64).map(|s| (s.data.clone(), None))
    }

    fn media_json(id: u32, bytes: &[u8], bank: Option<u64>, streamed: bool) -> J {
        match vorbis::info(bytes) {
            Ok(i) => json!({ "id": id, "bytes": bytes.len(), "codec": i.codec, "channels": i.channels, "sampleRate": i.sample_rate, "duration": i.duration(), "streamed": streamed, "bank": bank, "hash": format!("{:08x}", i.setup_hash) }),
            Err(e) => json!({ "id": id, "bytes": bytes.len(), "codec": "unknown", "channels": 0, "sampleRate": 0, "duration": 0.0, "streamed": streamed, "bank": bank, "error": e.to_string() }),
        }
    }

    /// `wwise.open`: banks, media, events and streams of a `.pck`.
    fn wwise_open(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let names = self.ak_names().clone();
        let pkg = self.wwise_package(&path)?;
        let mut banks = Vec::new();
        for e in &pkg.banks {
            let b = match bnk::Bank::parse(&e.data) {
                Ok(b) => b,
                Err(err) => {
                    banks.push(json!({ "id": e.id, "error": err.to_string(), "media": [], "events": [] }));
                    continue;
                }
            };
            let objects = b.objects();
            let media: Vec<J> = b.media().into_iter().map(|(id, d)| Self::media_json(id, d, Some(e.id), false)).collect();
            let mut events: Vec<J> = objects
                .iter()
                .filter(|o| o.kind == bnk::EVENT)
                .map(|o| {
                    let (sources, _) = bnk::event_media(&objects, o.id);
                    json!({ "id": o.id, "name": names.get(&o.id), "media": sources.iter().map(|s| s.id).collect::<Vec<_>>() })
                })
                .collect();
            events.sort_by(|a, b| a["name"].as_str().unwrap_or("~").to_ascii_lowercase().cmp(&b["name"].as_str().unwrap_or("~").to_ascii_lowercase()));
            banks.push(json!({ "id": e.id, "name": names.get(&(e.id as u32)), "version": b.version, "objects": objects.len(), "media": media, "events": events }));
        }
        let streams: Vec<J> = pkg.streams.iter().map(|st| Self::media_json(st.id as u32, &st.data, None, true)).collect();
        Ok(json!({
            "path": path,
            "name": Path::new(&path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(),
            "languages": pkg.languages.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>(),
            "banks": banks,
            "streams": streams,
        }))
    }

    /// `wwise.preview`: a media file as Ogg Vorbis.
    fn wwise_preview(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let id = opt_u(p, "id").ok_or("missing parameter id")? as u32;
        let pkg = self.wwise_package(&path)?;
        let (bytes, bank) = Self::wwise_media(pkg, id).ok_or_else(|| format!("media {id} is not in {path}"))?;
        let (ogg, info) = vorbis::wem_to_ogg(&bytes)?;
        Ok(json!({ "id": id, "ogg": self.bins.add(ogg), "bytes": bytes.len(), "codec": info.codec, "channels": info.channels, "sampleRate": info.sample_rate, "duration": info.duration(), "bank": bank, "hash": format!("{:08x}", info.setup_hash) }))
    }

    /// `wwise.export`: a media file as `.ogg`, `.wav` or raw `.wem`.
    fn wwise_export(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let out = s(p, "out")?.to_string();
        let id = opt_u(p, "id").ok_or("missing parameter id")? as u32;
        let pkg = self.wwise_package(&path)?;
        let (bytes, _) = Self::wwise_media(pkg, id).ok_or_else(|| format!("media {id} is not in {path}"))?;
        let lower = out.to_ascii_lowercase();
        let data = if lower.ends_with(".wem") {
            bytes
        } else if lower.ends_with(".wav") {
            audio::encode_wav(&audio::decode_any(&bytes)?)
        } else {
            vorbis::wem_to_ogg(&bytes)?.0
        };
        std::fs::write(&out, &data)?;
        Ok(json!({ "out": out, "bytes": data.len() }))
    }

    /// Replace a media file in a package with Wwise Vorbis bytes and write
    /// the package to `out` (or in place).
    fn wwise_write(&mut self, path: &str, id: u32, wem: Vec<u8>, out: Option<String>) -> Res<J> {
        let mut pkg = self.wwise_package(path)?.clone();
        let mut done = false;
        for e in pkg.banks.iter_mut() {
            let mut b = bnk::Bank::parse(&e.data)?;
            if b.media().iter().any(|(i, _)| *i == id) {
                b.replace_media(id, &wem)?;
                e.data = b.to_bytes();
                done = true;
            }
        }
        for st in pkg.streams.iter_mut() {
            if st.id == id as u64 {
                st.data = wem.clone();
                done = true;
            }
        }
        if !done {
            return Err(format!("media {id} is not in {path}").into());
        }
        let out = out.unwrap_or_else(|| path.to_string());
        if let Some(parent) = Path::new(&out).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&out, pkg.to_bytes())?;
        self.wwise = None;
        Ok(json!({ "out": out, "bytes": wem.len() }))
    }

    /// Audio file bytes → Wwise Vorbis (`.wem` verbatim; Ogg/WAV re-encoded).
    fn to_wem(bytes: Vec<u8>, quality: f32) -> Res<Vec<u8>> {
        if bytes.starts_with(b"RIFF") && bytes.len() > 22 && u16::from_le_bytes([bytes[20], bytes[21]]) == 0xFFFF {
            return Ok(bytes);
        }
        let ogg = if bytes.starts_with(b"OggS") { bytes } else { audio::encode_ogg(&audio::decode_any(&bytes)?, quality)? };
        Ok(vorbis::ogg_to_wem(&ogg)?)
    }

    /// `wwise.import`: replace a media file from `.ogg`, `.wav` or `.wem`.
    fn wwise_import(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let file = s(p, "file")?.to_string();
        let id = opt_u(p, "id").ok_or("missing parameter id")? as u32;
        let quality = p.get("quality").and_then(|q| q.as_f64()).unwrap_or(0.7) as f32;
        let out = opt_s(p, "out").filter(|o| !o.is_empty());
        let wem = Self::to_wem(std::fs::read(&file)?, quality)?;
        self.wwise_write(&path, id, wem, out)
    }

    /// `wwise.replacePcm`: edited PCM from the sound editor into a media file.
    fn wwise_replace_pcm(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let id = opt_u(p, "id").ok_or("missing parameter id")? as u32;
        let quality = p.get("quality").and_then(|q| q.as_f64()).unwrap_or(0.7) as f32;
        let out = opt_s(p, "out").filter(|o| !o.is_empty());
        let pcm = Self::pcm_param(p)?;
        let wem = vorbis::ogg_to_wem(&audio::encode_ogg(&pcm, quality)?)?;
        self.wwise_write(&path, id, wem, out)
    }

    /// `wwise.forObject`: the package and media behind an AkEvent or AkBank
    /// export (bank name → `<name>.pck` in the workspace folder, event name →
    /// FNV id).
    fn wwise_for_object(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let i = self.export_of(&path, p)?;
        let dir = self.dir()?.to_path_buf();
        let (class, name, bank_name) = {
            let ed = self.editor(&path)?;
            let class = ed.pkg.class_name_of(PackageIndex::export(i));
            let name = ed.pkg.object_name(PackageIndex::export(i));
            let o = ed.object(i)?.clone();
            let bank_name = if class.eq_ignore_ascii_case("AkBank") {
                name.clone()
            } else {
                match o.property("RequiredBank", &ed.pkg).map(|p| &p.value) {
                    Some(crate::value::Value::Object(b)) if !b.is_null() => ed.pkg.object_name(*b),
                    _ => String::new(),
                }
            };
            (class, name, bank_name)
        };
        let is_event = class.eq_ignore_ascii_case("AkEvent");
        let pck = std::fs::read_dir(&dir).ok().and_then(|rd| rd.flatten().map(|e| e.path()).find(|f| f.file_stem().map(|x| x.to_string_lossy().eq_ignore_ascii_case(&bank_name)).unwrap_or(false) && f.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case("pck")).unwrap_or(false)));
        let event_id = if is_event { Some(fnv::name_id(&name)) } else { None };
        let bank_id = fnv::name_id(&bank_name);
        let Some(pck) = pck else {
            return Ok(json!({ "pck": null, "bank": bank_name, "bankId": bank_id, "eventId": event_id, "name": name, "media": [], "trail": [] }));
        };
        let pck_path = pck.to_string_lossy().to_string();
        let pkg = self.wwise_package(&pck_path)?;
        let mut media = Vec::new();
        let mut trail = Vec::new();
        let mut found = false;
        for e in &pkg.banks {
            let Ok(b) = bnk::Bank::parse(&e.data) else { continue };
            match event_id {
                Some(ev) => {
                    let objects = b.objects();
                    if !objects.iter().any(|o| o.id == ev && o.kind == bnk::EVENT) {
                        continue;
                    }
                    found = true;
                    let (sources, t) = bnk::event_media(&objects, ev);
                    trail = t.into_iter().map(|(k, id)| json!({ "kind": bnk::kind_name(k), "id": id })).collect();
                    for src in sources {
                        match Self::wwise_media(pkg, src.id) {
                            Some((bytes, bank)) => media.push(Self::media_json(src.id, &bytes, bank, src.stream_type == 2)),
                            None => media.push(json!({ "id": src.id, "bytes": 0, "codec": "missing", "channels": 0, "sampleRate": 0, "duration": 0.0, "streamed": src.stream_type == 2, "bank": null, "error": "not in this package" })),
                        }
                    }
                }
                None => {
                    found = true;
                    for (id, d) in b.media() {
                        media.push(Self::media_json(id, d, Some(e.id), false));
                    }
                }
            }
        }
        if event_id.is_none() {
            for st in &pkg.streams {
                media.push(Self::media_json(st.id as u32, &st.data, None, true));
            }
        }
        Ok(json!({ "pck": pck_path, "bank": bank_name, "bankId": bank_id, "eventId": event_id, "name": name, "found": found, "media": media, "trail": trail }))
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
        // A file whose base the Studio recorded ships as a delta
        // (`deltas/<game path>.tdelta`, applied by the launcher over the
        // game's file); the others are copied whole under `files/`.
        let use_deltas = opt_b(p, "deltas").unwrap_or(true);
        let mut added = Vec::new();
        let mut entries = Vec::new();
        for f in files {
            let path = Path::new(&f);
            let rel = crate::basestore::game_relative(path);
            let digest = if use_deltas { crate::basestore::read(path) } else { None };
            match digest {
                Some(digest) => {
                    let (d, warnings) = self.make_delta(path, &digest, &rel)?;
                    let ops = d.ops().len() + d.header["tables"].as_array().map(|a| a.iter().map(|t| t["ops"].as_array().map(|o| o.len()).unwrap_or(0)).sum::<usize>()).unwrap_or(0) + d.header["header"].as_object().map(|h| h.len()).unwrap_or(0);
                    if ops == 0 {
                        entries.push(json!({ "file": rel, "mode": "unchanged", "ops": 0, "bytes": 0, "warnings": warnings }));
                        continue;
                    }
                    let out = dir.join("deltas").join(format!("{rel}.tdelta"));
                    if let Some(parent) = out.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    d.write(&out)?;
                    added.push(format!("deltas/{rel}.tdelta"));
                    entries.push(json!({ "file": rel, "mode": "delta", "ops": ops, "bytes": d.to_bytes().len(), "warnings": warnings }));
                }
                None => {
                    let out = dir.join("files").join(&rel);
                    if let Some(parent) = out.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(path, &out)?;
                    added.push(format!("files/{rel}"));
                    entries.push(json!({ "file": rel, "mode": "copy", "ops": 0, "bytes": std::fs::metadata(path).map(|m| m.len()).unwrap_or(0), "warnings": [] }));
                }
            }
        }
        Ok(json!({ "dir": dir.to_string_lossy(), "files": added, "entries": entries }))
    }

    /// `mod.changes`: what a mod made from this workspace could hold: files
    /// with unsaved edits (packages, `.dat` files) and files saved in place
    /// since the Studio recorded their base. Each with whether a delta can be
    /// made (a base exists, or the unsaved edits can be compared with the
    /// file on disk).
    fn mod_changes(&mut self) -> Res<J> {
        let dir = self.dir()?.to_path_buf();
        let root = crate::twrk::game_root(&dir);
        let mut out: Vec<J> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (k, ed) in &self.editors {
            if !ed.is_dirty() {
                continue;
            }
            let path = PathBuf::from(k);
            seen.insert(key(k));
            let is_script = ed.pkg.summary.package_flags & pkgf::CONTAINS_SCRIPT != 0;
            out.push(json!({
                "path": k, "kind": "upk", "state": "unsaved",
                "rel": crate::basestore::game_relative(&path),
                "base": crate::basestore::read(&path).is_some(),
                "script": is_script,
            }));
        }
        if let Some(ws) = &self.mcts {
            for (id, doc) in &ws.docs {
                if !doc.dirty {
                    continue;
                }
                seen.insert(key(&doc.path.to_string_lossy()));
                out.push(json!({
                    "path": doc.path.to_string_lossy(), "kind": "mcts", "doc": id, "state": "unsaved",
                    "rel": crate::basestore::game_relative(&doc.path),
                    "base": crate::basestore::read(&doc.path).is_some(),
                    "script": false,
                }));
            }
        }
        for (file, info) in crate::basestore::all() {
            let k = key(&file.to_string_lossy());
            if seen.contains(&k) || !file.exists() || crate::twrk::relative(&root, &file).is_none() {
                continue;
            }
            let lower = file.to_string_lossy().to_ascii_lowercase();
            out.push(json!({
                "path": file.to_string_lossy(), "kind": if lower.ends_with(".dat") { "mcts" } else { "upk" },
                "state": "saved", "rel": crate::basestore::game_relative(&file), "base": true, "script": false,
                "recorded": info["recorded"],
            }));
        }
        out.sort_by(|a, b| a["rel"].as_str().cmp(&b["rel"].as_str()));
        Ok(json!({ "files": out }))
    }

    /// `mod.cook {dir, id, name, version, authors, readme, dependencies, icon,
    /// items: [{path, kind, doc?, mode: delta|whole}], extras: [{src, dest}]}`:
    /// write a launcher mod folder (what `tempest mod pack` packages):
    /// `manifest.toml` (the launcher cooker's format), `README.md`,
    /// `icon.png`, every item as `deltas/<game path>.tdelta` or
    /// `files/<game path>` (unsaved edits included, taken from memory), and
    /// the extra files at their destination inside the mod.
    fn mod_cook(&mut self, p: &J) -> Res<J> {
        let dir = PathBuf::from(s(p, "dir")?);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        let esc = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"));
        let id = s(p, "id")?.to_string();
        let mut m = format!("[mod]\nid = {}\nname = {}\nversion = {}\n", esc(&id), esc(&opt_s(p, "name").unwrap_or_else(|| id.clone())), esc(&opt_s(p, "version").unwrap_or_else(|| "1.0.0".into())));
        if let Some(icon) = opt_s(p, "icon").filter(|i| !i.is_empty()) {
            std::fs::copy(&icon, dir.join("icon.png"))?;
            m.push_str("icon = \"icon.png\"\n");
        }
        if let Some(readme) = opt_s(p, "readme").filter(|r| !r.trim().is_empty()) {
            std::fs::write(dir.join("README.md"), readme.trim())?;
            m.push_str("readme = \"README.md\"\n");
        }
        let deps: Vec<String> = p.get("dependencies").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
        if !deps.is_empty() {
            m.push_str("dependencies = [\n");
            for d in &deps {
                m.push_str(&format!("    {},\n", esc(d)));
            }
            m.push_str("]\n");
        }
        m.push('\n');
        for a in p.get("authors").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
            let name = a["name"].as_str().unwrap_or("").trim().to_string();
            if name.is_empty() {
                continue;
            }
            m.push_str(&format!("[[mod.authors]]\nname = {}\n", esc(&name)));
            if let Some(link) = a["link"].as_str().map(str::trim).filter(|l| !l.is_empty()) {
                m.push_str(&format!("link = {}\n", esc(link)));
            }
            m.push('\n');
        }
        std::fs::write(dir.join("manifest.toml"), m)?;

        let db = self.db()?;
        let mut entries: Vec<J> = Vec::new();
        let write = |rel: &str, bytes: &[u8], folder: &str| -> Res<String> {
            let name = if folder == "deltas" { format!("{rel}.tdelta") } else { rel.to_string() };
            let out = dir.join(folder).join(&name);
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, bytes)?;
            Ok(format!("{folder}/{name}"))
        };
        for it in p.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
            let path = PathBuf::from(it["path"].as_str().unwrap_or_default());
            let rel = crate::basestore::game_relative(&path);
            let whole = it["mode"].as_str() == Some("whole");
            let k = key(&path.to_string_lossy());
            let label = rel.clone();
            if it["kind"] == "mcts" {
                // The marshal as it is now: in memory when edited, else on disk.
                let (tokens, edited) = match self.mcts.as_ref().and_then(|ws| ws.docs.iter().find(|(_, d)| key(&d.path.to_string_lossy()) == k).map(|(_, d)| (ws.tokens.clone(), d.marshal.clone()))) {
                    Some(x) => x,
                    None => {
                        let tokens = mcts::Tokens::for_dir(path.parent().unwrap_or(Path::new(".")));
                        let m = mcts::parse_obscured(&std::fs::read(&path)?, &tokens)?;
                        (tokens, m)
                    }
                };
                if whole {
                    let f = write(&rel, &mcts::write_obscured(&edited, &tokens), "files")?;
                    entries.push(json!({ "file": label, "entry": f, "mode": "whole" }));
                    continue;
                }
                let d = match crate::basestore::read(&path) {
                    Some(digest) => mcts::delta::create(&tokens, mcts::delta::Base::Digest(&digest), &edited, &rel)?,
                    None => {
                        let base = mcts::parse_obscured(&std::fs::read(&path)?, &tokens)?;
                        mcts::delta::create(&tokens, mcts::delta::Base::Marshal(&base), &edited, &rel)?
                    }
                };
                let ops = d.header["tables"].as_array().map(|a| a.iter().map(|t| t["ops"].as_array().map(|o| o.len()).unwrap_or(0)).sum::<usize>()).unwrap_or(0) + d.header["header"].as_object().map(|h| h.len()).unwrap_or(0);
                if ops == 0 {
                    entries.push(json!({ "file": label, "mode": "unchanged" }));
                    continue;
                }
                let f = write(&rel, &d.to_bytes(), "deltas")?;
                entries.push(json!({ "file": label, "entry": f, "mode": "delta", "ops": ops }));
                continue;
            }
            // Packages: the bytes Save would write when edited, else the file.
            let edited_bytes = match self.editors.get_mut(&k) {
                Some(ed) if ed.is_dirty() => ed.bytes(),
                _ => std::fs::read(&path)?,
            };
            if whole {
                let f = write(&rel, &edited_bytes, "files")?;
                entries.push(json!({ "file": label, "entry": f, "mode": "whole" }));
                continue;
            }
            let edited = Package::parse(&edited_bytes)?;
            let (d, warnings) = match crate::basestore::read(&path) {
                Some(digest) => crate::delta::create(db, crate::delta::Base::Digest(&digest), &edited, &rel)?,
                None => {
                    let base = PackageFile::open(&path)?.load()?;
                    crate::delta::create(db, crate::delta::Base::Package(&base), &edited, &rel)?
                }
            };
            if d.ops().is_empty() {
                entries.push(json!({ "file": label, "mode": "unchanged" }));
                continue;
            }
            let f = write(&rel, &d.to_bytes(), "deltas")?;
            entries.push(json!({ "file": label, "entry": f, "mode": "delta", "ops": d.ops().len(), "warnings": warnings }));
        }
        for x in p.get("extras").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
            let src = PathBuf::from(x["src"].as_str().unwrap_or_default());
            let dest = x["dest"].as_str().unwrap_or_default().trim_start_matches('/').replace('\\', "/");
            if dest.is_empty() || dest.contains("..") || dest.starts_with(".tempest") {
                return Err(format!("bad destination for {}", src.display()).into());
            }
            let out = dir.join(&dest);
            if src.is_dir() {
                copy_dir(&src, &out)?;
            } else {
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&src, &out)?;
            }
            entries.push(json!({ "file": dest, "entry": dest, "mode": "extra" }));
        }
        Ok(json!({ "dir": dir.to_string_lossy(), "entries": entries }))
    }

    /// The delta of a file on disk against its recorded base: rows for a
    /// marshal, objects for a package (the open editor's unsaved edits are
    /// not part of it; the dialog asks for a save first).
    fn make_delta(&mut self, path: &Path, digest: &J, target: &str) -> Res<(crate::delta::Delta, Vec<String>)> {
        let lower = path.to_string_lossy().to_ascii_lowercase();
        if lower.ends_with(".dat") {
            let tokens = mcts::Tokens::for_dir(path.parent().unwrap_or(Path::new(".")));
            let m = mcts::parse_obscured(&std::fs::read(path)?, &tokens)?;
            let d = mcts::delta::create(&tokens, mcts::delta::Base::Digest(digest), &m, target)?;
            return Ok((d, Vec::new()));
        }
        let db = self.db()?;
        let pkg = PackageFile::open(path)?.load()?;
        Ok(crate::delta::create(db, crate::delta::Base::Digest(digest), &pkg, target)?)
    }

    /// `delta.record {path}`: take the file as it is now as the base (after
    /// restoring the original, or when a package was edited before the
    /// Studio kept records). Replaces any earlier record.
    fn delta_record(&mut self, p: &J) -> Res<J> {
        let path = PathBuf::from(s(p, "path")?);
        crate::basestore::remove(&path);
        let t = std::time::Instant::now();
        if path.to_string_lossy().to_ascii_lowercase().ends_with(".dat") {
            let tokens = mcts::Tokens::for_dir(path.parent().unwrap_or(Path::new(".")));
            crate::basestore::record_mcts(&tokens, &path)?;
        } else {
            crate::basestore::record_upk(self.db()?, &path)?;
        }
        let mut st = crate::basestore::status(&path);
        st["ms"] = J::from(t.elapsed().as_millis() as u64);
        Ok(st)
    }

    /// `delta.create {path, out, target?}`: one delta file from a game file and its record.
    fn delta_create(&mut self, p: &J) -> Res<J> {
        let path = PathBuf::from(s(p, "path")?);
        let out = s(p, "out")?;
        let target = opt_s(p, "target").unwrap_or_else(|| crate::basestore::game_relative(&path));
        let digest = crate::basestore::read(&path).ok_or("no base recorded for this file (save it in place once, or use delta.record)")?;
        let (d, warnings) = self.make_delta(&path, &digest, &target)?;
        d.write(out)?;
        let ops: Vec<J> = d.ops().into_iter().map(|o| json!({ "op": o.op, "path": o.path, "class": o.class })).collect();
        Ok(json!({ "out": out, "target": target, "kind": d.kind(), "ops": ops, "tables": d.header["tables"], "bytes": d.to_bytes().len(), "warnings": warnings }))
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
        self.index_all();
        self.editors.get_mut(&key(path)).ok_or("package not open")?.flush();
        let src = self.src.as_ref().ok_or("no workspace open")?;
        let ed = self.editors.get(&key(path)).ok_or("package not open")?;
        let (index, pkgs) = (&self.index, &self.packages);
        let mut cache = PackageCache::new(db, &dir);
        cache.set_lookup(|p| index_lookup(index, pkgs, p));
        cache.insert(&ed.name, ed.pkg.clone());
        let info = crate::material_ops::material_info(&mut cache, &ed.name, PackageIndex::export(i));
        let mut png_cache = std::mem::take(&mut self.png_cache);
        let mut sink = std::mem::take(&mut self.bins);
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
                    (Some(sink.add(b)), None)
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
        self.bins = sink;
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

    /// Build the search index of every workspace package that has none yet
    /// (`search.progress` events while it runs).
    fn index_all(&mut self) {
        if let Some(job) = self.index_job.take() {
            if let Ok(built) = job.join() {
                for (k, idx) in built {
                    self.index.entry(k).or_insert(idx);
                }
            }
        }
        let total = self.packages.len();
        let paths: Vec<PathBuf> = self.packages.iter().map(|p| p.path.clone()).collect();
        let mut scanned = 0usize;
        for (n, path) in paths.iter().enumerate() {
            let k = key(&path.to_string_lossy());
            if self.index.contains_key(&k) {
                continue;
            }
            self.index.insert(k, build_index(path));
            scanned += 1;
            if scanned.is_multiple_of(20) {
                let mut f = Map::new();
                f.insert("done".into(), J::from(n + 1));
                f.insert("total".into(), J::from(total));
                self.event("search.progress", f);
            }
        }
    }

    /// `batch.plan` (dry run) and `batch.run`: a folder of `<package>.<object>.<ext>` files.
    fn batch(&mut self, p: &J, run: bool) -> Res<J> {
        let dir = PathBuf::from(s(p, "dir")?);
        let recursive = opt_b(p, "recursive").unwrap_or(false);
        let out_dir = opt_s(p, "outDir").filter(|d| !d.is_empty()).map(PathBuf::from);
        let db = self.db()?;
        let packages: Vec<(String, PathBuf)> = self.packages.iter().map(|w| (w.name.clone(), w.path.clone())).collect();
        let files = crate::batch::collect_files(&dir, recursive);
        // Files whose first name part is a group rather than a package file are
        // located through the search index of every package.
        self.index_all();
        let index = &self.index;
        let mut lookup = |path: &str| -> Vec<(String, PathBuf)> {
            packages.iter().filter(|(_, p)| index.get(&key(&p.to_string_lossy())).is_some_and(|idx| idx.exports.iter().any(|(_, op)| op.eq_ignore_ascii_case(path)))).cloned().collect()
        };
        let entries = if run {
            // Unsaved edits of the touched packages would be overwritten: refuse.
            let plan = crate::batch::plan(db, &packages, &files, &mut lookup);
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
            // `packages`: the package files to write (every touched one when absent).
            let only: Option<std::collections::HashSet<String>> = p.get("packages").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| s.to_ascii_lowercase()).collect());
            let entries = crate::batch::run(db, &packages, &files, &mut lookup, out_dir.as_deref(), only.as_ref(), &mut |done, total, file| events.push((done, total, file.to_string())));
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
            crate::batch::plan(db, &packages, &files, &mut lookup)
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
    let mut files: Vec<(PathBuf, u64)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let path = e.path();
            let ext = path.extension().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if ext != "upk" && ext != "u" {
                continue;
            }
            files.push((path, e.metadata().map(|m| m.len()).unwrap_or(0)));
        }
    }
    // A thousand summaries to read: spread the file opens over the cores.
    let mut out: Vec<WorkspacePackage> = tempest_unrealscript::par::map(&files, |(path, size)| {
        let flags = Package::read_summary(path).map(|s| s.package_flags).unwrap_or(0);
        WorkspacePackage { name: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), path: path.clone(), size: *size, flags }
    });
    out.sort_by_key(|p| p.name.to_ascii_lowercase());
    out
}

/// The workspace package holding the object `path` (any case), from the
/// search index.
/// The name an import URL refers to (`../GFxFonts/gfxfontlib.swf` → `gfxfontlib`).
fn swf_stem(url: &str) -> String {
    let file = url.rsplit('/').next().unwrap_or(url);
    let stem = file.split('.').next().unwrap_or(file);
    stem.to_ascii_lowercase()
}

/// `DefineBitsLossless2` tag body for straight-alpha RGBA pixels: 32-bit ARGB
/// (alpha first) zlib-compressed, which is what Ruffle and Flash decode.
fn define_bits_lossless2(id: u16, width: u16, height: u16, rgba: &[u8]) -> Vec<u8> {
    let mut argb = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        // DefineBitsLossless2 stores premultiplied ARGB; the decoded texture
        // is straight RGBA, so premultiply here or anti-aliased edges fringe.
        let a = px[3] as u32;
        let channel = |c: u8| ((c as u32 * a + 127) / 255) as u8;
        argb.extend_from_slice(&[px[3], channel(px[0]), channel(px[1]), channel(px[2])]);
    }
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = enc.write_all(&argb);
    let compressed = enc.finish().unwrap_or_default();
    let mut body = Vec::with_capacity(7 + compressed.len());
    body.extend_from_slice(&id.to_le_bytes());
    body.push(5);
    body.extend_from_slice(&width.to_le_bytes());
    body.extend_from_slice(&height.to_le_bytes());
    body.extend_from_slice(&compressed);
    body
}

/// Fit decoded pixels to the size the movie declares. A texture larger than
/// the target is the same image with DXT block padding (up to 3 px) on the
/// right/bottom, so crop it; a smaller one is scaled up.
fn fit_rgba(rgba: &[u8], w: u32, h: u32, tw: u32, th: u32) -> (u16, u16, Vec<u8>) {
    if w >= tw && h >= th && tw > 0 && th > 0 {
        if let Some(fit) = crop_rgba(rgba, w, h, 0, 0, tw as u16, th as u16) {
            return fit;
        }
    }
    (tw as u16, th as u16, resample_rgba(rgba, w, h, tw, th))
}

/// Crop a rectangle of an RGBA8 image, clamped to its bounds.
fn crop_rgba(rgba: &[u8], w: u32, h: u32, x1: u16, y1: u16, x2: u16, y2: u16) -> Option<(u16, u16, Vec<u8>)> {
    let x1 = (x1 as u32).min(w);
    let y1 = (y1 as u32).min(h);
    let x2 = (x2 as u32).min(w);
    let y2 = (y2 as u32).min(h);
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    let cw = x2 - x1;
    let ch = y2 - y1;
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for y in y1..y2 {
        let start = ((y * w + x1) * 4) as usize;
        out.extend_from_slice(&rgba[start..start + (cw * 4) as usize]);
    }
    Some((cw as u16, ch as u16, out))
}

/// Nearest-neighbour scale to the size the movie declares for the image (the
/// engine's `DefineExternalImage2` dimensions); normally already equal.
fn resample_rgba(rgba: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    if w == tw && h == th {
        return rgba.to_vec();
    }
    let mut out = vec![0u8; tw as usize * th as usize * 4];
    for y in 0..th {
        let sy = ((y as u64 * h as u64) / th as u64).min(h as u64 - 1) as usize;
        for x in 0..tw {
            let sx = ((x as u64 * w as u64) / tw as u64).min(w as u64 - 1) as usize;
            let src = (sy * w as usize + sx) * 4;
            let dst = (y as usize * tw as usize + x as usize) * 4;
            out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
        }
    }
    out
}

fn index_lookup(index: &HashMap<String, SearchIndex>, packages: &[WorkspacePackage], path: &str) -> Option<String> {
    packages.iter().find(|w| index.get(&key(&w.path.to_string_lossy())).is_some_and(|idx| idx.exports.iter().any(|(_, op)| op.eq_ignore_ascii_case(path)))).map(|w| w.name.clone())
}

/// The on-disk search index of a cooked folder: `<cache dir>/tempest-studio/index/<hash>.idx`.
fn index_cache_file(dir: &Path) -> Option<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"))
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    }?;
    let mut h: u64 = 0xcbf29ce484222325;
    for b in key(&dir.to_string_lossy()).bytes() {
        h = h.wrapping_mul(0x100000001b3) ^ b as u64;
    }
    Some(base.join("tempest-studio").join("index").join(format!("{h:016x}.idx")))
}

/// Index every package, reusing the entries of the cache file whose file
/// size and modification time still match, and rewrite the cache when any
/// package was (re)indexed. Format: `TSIX` + u32 count, then per package
/// the key, mtime, size, class names and (class id, path) pairs.
fn build_indexes(paths: &[PathBuf], cache: Option<&Path>) -> HashMap<String, SearchIndex> {
    let stamp = |p: &Path| -> (u64, u64) {
        std::fs::metadata(p).map(|m| (m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0), m.len())).unwrap_or((0, 0))
    };
    let mut cached: HashMap<String, ((u64, u64), SearchIndex)> = cache
        .and_then(|c| std::fs::read(c).ok())
        .and_then(|z| {
            use std::io::Read;
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(&z[..]).read_to_end(&mut out).ok()?;
            Some(out)
        })
        .map(|b| read_index_cache(&b))
        .unwrap_or_default();
    let mut changed = false;
    let mut out = HashMap::new();
    let built: Vec<(String, (u64, u64), Option<SearchIndex>)> = tempest_unrealscript::par::map(paths, |p| {
        let k = key(&p.to_string_lossy());
        let st = stamp(p);
        let fresh = cached.get(&k).map(|(s, _)| *s == st).unwrap_or(false);
        (k, st, if fresh { None } else { Some(build_index(p)) })
    });
    for (k, st, idx) in built {
        match idx {
            Some(i) => {
                changed = true;
                out.insert(k.clone(), i.clone());
                cached.insert(k, (st, i));
            }
            None => {
                if let Some((_, i)) = cached.get(&k) {
                    out.insert(k, i.clone());
                }
            }
        }
    }
    if changed {
        if let Some(c) = cache {
            let keep: Vec<&String> = out.keys().collect();
            cached.retain(|k, _| keep.contains(&k));
            if let Some(parent) = c.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            // Object paths compress about ten to one.
            use std::io::Write;
            let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            let _ = enc.write_all(&write_index_cache(&cached));
            if let Ok(z) = enc.finish() {
                let _ = std::fs::write(c, z);
            }
        }
    }
    out
}

fn write_index_cache(entries: &HashMap<String, ((u64, u64), SearchIndex)>) -> Vec<u8> {
    let mut v = Vec::new();
    let put_str = |v: &mut Vec<u8>, s: &str| {
        v.extend_from_slice(&(s.len() as u32).to_le_bytes());
        v.extend_from_slice(s.as_bytes());
    };
    v.extend_from_slice(b"TSIX");
    v.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (k, ((mtime, size), idx)) in entries {
        put_str(&mut v, k);
        v.extend_from_slice(&mtime.to_le_bytes());
        v.extend_from_slice(&size.to_le_bytes());
        v.extend_from_slice(&(idx.classes.len() as u32).to_le_bytes());
        for c in &idx.classes {
            put_str(&mut v, c);
        }
        v.extend_from_slice(&(idx.exports.len() as u32).to_le_bytes());
        for (c, p) in &idx.exports {
            v.extend_from_slice(&c.to_le_bytes());
            put_str(&mut v, p);
        }
    }
    v
}

fn read_index_cache(b: &[u8]) -> HashMap<String, ((u64, u64), SearchIndex)> {
    let mut out = HashMap::new();
    let mut pos = 4;
    if b.len() < 8 || &b[..4] != b"TSIX" {
        return out;
    }
    let u32_at = |pos: &mut usize| -> Option<u32> {
        let v = u32::from_le_bytes(b.get(*pos..*pos + 4)?.try_into().ok()?);
        *pos += 4;
        Some(v)
    };
    let get_str = |pos: &mut usize| -> Option<String> {
        let n = u32::from_le_bytes(b.get(*pos..*pos + 4)?.try_into().ok()?) as usize;
        *pos += 4;
        let s = std::str::from_utf8(b.get(*pos..*pos + n)?).ok()?.to_string();
        *pos += n;
        Some(s)
    };
    let get_u64 = |pos: &mut usize| -> Option<u64> {
        let v = u64::from_le_bytes(b.get(*pos..*pos + 8)?.try_into().ok()?);
        *pos += 8;
        Some(v)
    };
    let Some(count) = u32_at(&mut pos) else { return out };
    for _ in 0..count {
        let (Some(k), Some(mtime), Some(size)) = (get_str(&mut pos), get_u64(&mut pos), get_u64(&mut pos)) else { break };
        let Some(nc) = u32_at(&mut pos) else { break };
        let mut classes = Vec::with_capacity(nc as usize);
        for _ in 0..nc {
            let Some(c) = get_str(&mut pos) else { return out };
            classes.push(c);
        }
        let Some(ne) = u32_at(&mut pos) else { break };
        let mut exports = Vec::with_capacity(ne as usize);
        for _ in 0..ne {
            let Some(c) = b.get(pos..pos + 2).map(|x| u16::from_le_bytes([x[0], x[1]])) else { return out };
            pos += 2;
            let Some(p) = get_str(&mut pos) else { return out };
            exports.push((c, p));
        }
        out.insert(k, ((mtime, size), SearchIndex { classes, exports }));
    }
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

pub fn base64_decode(text: &str) -> Vec<u8> {
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

/// A copy of a JSON object with one more field.
fn with_field(meta: &J, key: &str, value: &str) -> J {
    let mut m = meta.clone();
    m[key] = J::from(value);
    m
}

/// Copy a folder recursively.
fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &to)?;
        } else {
            std::fs::copy(e.path(), to)?;
        }
    }
    Ok(())
}
