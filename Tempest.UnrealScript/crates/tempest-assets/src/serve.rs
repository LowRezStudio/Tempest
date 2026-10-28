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

use std::collections::{HashMap, HashSet};
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
use crate::object::{package_layout, parse_export, Native, Object, Parsed};
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
    /// `mod.preview` results of items read from disk, by what they depend on
    /// (source and game file sizes and times, mode, base record).
    preview_cache: HashMap<String, J>,
    /// The cooker's finished items (encoded), by `item_stamp` and sharing:
    /// building again after a small change only redoes the files that changed.
    cook_cache: HashMap<String, Item>,
    /// Large objects each item holds (the cooker's first pass), by `item_stamp`.
    holds_cache: HashMap<String, Vec<String>>,
    /// Compressed shared object files, by hash and compression.
    packed_cache: HashMap<String, Vec<u8>>,
    /// How the mod being cooked is compressed (`mod.cook {compression}`).
    compression: crate::delta::Compression,
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
    /// The last movie parsed (`path#export`), valid until a call outside the
    /// SWF editor or `swf.apply` (the only ways a movie can change).
    swf_cache: Option<(String, crate::swf::Movie)>,
    /// External images converted for Ruffle, by package and tag contents.
    swf_embeds: HashMap<u64, SwfEmbed>,
    /// Where the libraries movies import live, by URL stem (`None`: no
    /// workspace package has it). Found ones are checked on use; misses are
    /// forgotten with `swf_embeds`.
    swf_libs: HashMap<String, Option<(PathBuf, usize, String)>>,
    /// Library subsets by library and imported names: the subset's URL and
    /// every subset it brings (`swf_subsets`); dropped with `swf_embeds` and
    /// whenever a movie changes.
    swf_subset_cache: HashMap<String, LibrarySubset>,
}

/// A library subset's URL, and every subset it brings as `(url, movie)`.
type LibrarySubset = (String, Vec<(String, Vec<u8>)>);

/// What an external image (and its sub-images) became in a player movie.
#[derive(Clone)]
struct SwfEmbed {
    tags: Vec<crate::swf::Tag>,
    consumed: Vec<u16>,
    missing: Vec<String>,
    /// Keep the original tag (the texture was not found).
    keep: bool,
}

/// An external image to convert, with its texture (`Err`: why there is none).
struct ImageJob {
    key: u64,
    img: crate::swf::ExternalImage,
    image_id: u16,
    subs: Vec<crate::swf::SubImage>,
    texture: Result<(usize, Object), String>,
}

/// Decode an external image's texture into `DefineBitsLossless2` bitmaps: the
/// whole image, or one per sub-image crop (Scaleform atlases).
fn convert_image(db: &ClassDb, pkg: &Package, pkg_name: &str, src: &TextureSource, j: &ImageJob) -> SwfEmbed {
    let mut e = SwfEmbed { tags: Vec::new(), consumed: Vec::new(), missing: Vec::new(), keep: false };
    let img = &j.img;
    let (tw, th, rgba) = match j.texture.as_ref().map_err(String::clone).and_then(|(i, o)| crate::texture_ops::largest_rgba(db, pkg, pkg_name, src, *i, o).map_err(|err| format!("{}: {err}", img.file_name))) {
        Ok(v) => v,
        Err(err) => {
            e.missing.push(err);
            e.keep = true;
            return e;
        }
    };
    if tw == 0 || th == 0 {
        e.keep = true;
        return e;
    }
    if !j.subs.is_empty() {
        // The atlas rect is in target pixels; DXT textures can be up to 3 px
        // larger (block padding), so scale into the decoded texture and crop there.
        let scale_x = |v: u16| ((v as u64 * tw as u64) / (img.width.max(1) as u64)) as u16;
        let scale_y = |v: u16| ((v as u64 * th as u64) / (img.height.max(1) as u64)) as u16;
        for sub in &j.subs {
            let Some((w, h, pixels)) = crop_rgba(&rgba, tw, th, scale_x(sub.x1), scale_y(sub.y1), scale_x(sub.x2), scale_y(sub.y2)) else {
                e.missing.push(format!("{}: empty crop {}x{}", img.file_name, sub.x1, sub.y1));
                continue;
            };
            e.tags.push(crate::swf::Tag { code: 36, data: define_bits_lossless2(sub.id, w, h, &pixels), long: true });
            e.consumed.push(sub.id);
        }
    } else if img.width > 0 && img.height > 0 {
        let (w, h, pixels) = fit_rgba(&rgba, tw, th, img.width as u32, img.height as u32);
        e.tags.push(crate::swf::Tag { code: 36, data: define_bits_lossless2(j.image_id, w, h, &pixels), long: true });
    } else {
        e.keep = true;
    }
    e
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
        Server { dir: None, db: None, src: None, packages: Vec::new(), editors: HashMap::new(), index: HashMap::new(), index_job: None, scripts: HashMap::new(), order: None, png_cache: HashMap::new(),
            preview_cache: HashMap::new(),
            cook_cache: HashMap::new(),
            holds_cache: HashMap::new(),
            packed_cache: HashMap::new(),
            compression: crate::delta::Compression::Small, out: std::io::stdout(), wwise: None, ak_names: None, mcts: None, bins: BinSink::default(),
            swf_cache: None, swf_embeds: HashMap::new(), swf_libs: HashMap::new(), swf_subset_cache: HashMap::new() }
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
        // Anything outside the SWF editor may change a movie or its textures,
        // except these reads (the inspector's tabs, texture thumbnails).
        const READS: &[&str] = &["ping", "package.names", "package.export", "package.tables", "package.undoList", "object.get", "object.text", "object.deps", "texture.info", "texture.preview", "texture.export"];
        if READS.contains(&method) {
        } else if !method.starts_with("swf.") || method == "swf.apply" || method == "swf.import" || method == "swf.addImage" {
            self.swf_cache = None;
            self.swf_subset_cache.clear();
            if !method.starts_with("swf.") {
                self.swf_embeds.clear();
                self.swf_libs.retain(|_, v| v.is_some());
            }
        }
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
            "anim.info" => self.anim_info(p),
            "anim.preview" => self.anim_preview(p),
            "anim.export" => self.anim_export(p),
            "anim.import" => self.anim_import(p),
            "anim.files" => self.anim_files(p),
            "anim.edit" => self.anim_edit(p),
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
                let r = self.mcts_mut()?.save(p)?;
                if p.get("out").and_then(|v| v.as_str()).is_some() {
                    let id = p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly");
                    let original = self.mcts()?.doc(id)?.path.clone();
                    if let Some(out) = r["out"].as_str() {
                        crate::copies::add(&original, Path::new(out));
                    }
                }
                Ok(r)
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
            "mod.preview" => self.mod_preview(p),
            "mod.match" => self.mod_match(p),
            "mod.forgetCopy" => self.mod_forget_copy(p),
            "mod.addCopy" => {
                crate::copies::add(Path::new(s(p, "target")?), Path::new(s(p, "path")?));
                Ok(J::Bool(true))
            }
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
            "swf.compile" => self.swf_compile(p),
            "swf.addImage" => self.swf_add_image(p),
            "swf.character" => self.swf_character(p),
            "swf.export" => self.swf_export(p),
            "swf.apply" => self.swf_apply(p),
            "swf.tree" => self.swf_tree(p),
            "swf.timeline" => self.swf_timeline(p),
            "swf.preview" => self.swf_preview(p),
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
        self.swf_libs.clear();
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
        if !in_place {
            // Saved elsewhere ("Save as"): a copy; the package's own file is
            // unchanged, so its edits stay unsaved. The copy is remembered as
            // a modified version of `path` for the mod cooker.
            ed.save_copy(&out)?;
            crate::copies::add(Path::new(path), Path::new(&out));
            return Ok(json!({ "out": out, "reopened": false, "dirty": ed.is_dirty(), "undo": ed.undo_labels() }));
        }
        ed.save(&out)?;
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
        // `maxSize`: the largest mip that fits (thumbnails).
        let png = match opt_u(p, "maxSize") {
            Some(max) => texture_ops::export_png_preview(db, &ed.pkg, &ed.name, src, i, &o, max as u32)?,
            None => texture_ops::export_png(db, &ed.pkg, &ed.name, src, i, &o, mip)?,
        };
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
        let mut opts = ImportOptions { mips: opt_b(p, "mips"), format: None, keep_alpha: opt_b(p, "keepAlpha").unwrap_or(false), pad_blocks: opt_b(p, "padBlocks").unwrap_or(false) };
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
        let (sw, sh) = match &source {
            ImportSource::Encoded { width, height, .. } | ImportSource::Rgba { width, height, .. } => (*width, *height),
        };
        let ed = self.editor(path)?;
        ed.begin_undo(format!("replace texture {}", ed.pkg.object_name(PackageIndex::export(i))));
        texture_ops::import(db, &mut ed.pkg, i, &mut o, source, &opts)?;
        ed.replace_object(i, o)?;
        ed.mark_dirty(i);
        let mut info = self.texture_info(p)?;
        // The file's own size: the texture's differs when it was padded or scaled to whole blocks.
        info["source"] = json!({ "width": sw, "height": sh });
        Ok(info)
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
        let sound = kind == "sound";
        let bytes = self.editor(path)?.with_object(i, |pkg, o| if sound { media::sound_export(pkg, o) } else { media::swf_export(pkg, o) })??;
        match kind {
            "sound" => {
                let ogg = bytes;
                let info = media::ogg_info(&ogg).ok();
                let len = ogg.len();
                Ok(json!({ "kind": "sound", "ogg": self.bins.add(ogg), "bytes": len, "channels": info.as_ref().map(|i| i.channels), "sampleRate": info.as_ref().map(|i| i.sample_rate), "duration": info.as_ref().map(|i| i.duration()) }))
            }
            _ => {
                let gfx = bytes;
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
        let sound = kind == "sound";
        let bytes = self.editor(path)?.with_object(i, |pkg, o| if sound { media::sound_export(pkg, o) } else { media::swf_export(pkg, o) })??;
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
        let k = format!("{}#{i}", key(&path));
        if let Some((ck, m)) = &self.swf_cache {
            if *ck == k {
                return Ok((path, i, m.clone()));
            }
        }
        let ed = self.editor(&path)?;
        let bytes = ed.with_object(i, media::swf_export)??;
        let m = crate::swf::Movie::parse(&bytes)?;
        self.swf_cache = Some((k, m.clone()));
        Ok((path, i, m))
    }

    /// The movie with the Studio's pending edits (`edits`, as `swf.apply`
    /// takes them) applied, without writing anything.
    fn swf_edited(&mut self, p: &J) -> Res<(String, usize, crate::swf::Movie)> {
        let (path, i, mut m) = self.swf_movie(p)?;
        if let Some(edits) = p.get("edits").and_then(|v| v.as_array()) {
            if !edits.is_empty() {
                crate::swf::apply(&mut m, edits)?;
            }
        }
        Ok((path, i, m))
    }

    /// `swf.info`: tags, texts, scripts, external images, linkage of a movie.
    fn swf_info(&mut self, p: &J) -> Res<J> {
        let (_, _, m) = self.swf_edited(p)?;
        Ok(crate::swf::info(&m))
    }

    /// `swf.bytes`: the movie as the game's GFx loader receives it, for the
    /// Studio's Ruffle player.
    fn swf_bytes(&mut self, p: &J) -> Res<J> {
        let (_, _, m) = self.swf_movie(p)?;
        let bytes = m.write();
        let len = bytes.len();
        Ok(json!({ "swf": self.bins.add(bytes), "bytes": len }))
    }

    /// `swf.player`: the movie with its GFx external images embedded as
    /// standard `DefineBitsLossless2` bitmaps (`embed_images`), pending
    /// `edits` applied.
    fn swf_player(&mut self, p: &J) -> Res<J> {
        let (path, _, mut m) = self.swf_edited(p)?;
        let (images, missing) = self.embed_images(&path, &mut m)?;
        crate::swf::merge_imports(&mut m.tags, |_| true);
        uncompressed(&mut m);
        let bytes = m.write();
        let len = bytes.len();
        Ok(json!({ "swf": self.bins.add(bytes), "bytes": len, "images": images, "missing": missing }))
    }

    /// Replace the GFx external images of a movie by standard bitmaps. The
    /// engine resolves a `DefineExternalImage2` (`Foo.tga`) to a `Texture2D`
    /// of the same package; Ruffle has no engine assets, so the texture is
    /// decoded and written into the movie instead (preview bytes only, the
    /// package is untouched). `DefineSubImage` crops (Scaleform atlases)
    /// become bitmaps of their own, because Ruffle has no sub-image tag.
    /// Conversions are cached by package and tag contents: decoding and
    /// compressing a 2048² atlas is the slow part of every preview.
    fn embed_images(&mut self, path: &str, m: &mut crate::swf::Movie) -> Res<(usize, Vec<String>)> {
        use std::hash::{Hash, Hasher};
        let mut missing: Vec<String> = Vec::new();
        if self.src.is_none() {
            return Ok((0, missing));
        }
        let pkg_key = key(path);
        let tags = std::mem::take(&mut m.tags);
        // Sub-images are crops of an external image; the shapes reference
        // the sub-image id, not the atlas.
        let mut sub_by_image: HashMap<u16, Vec<(crate::swf::SubImage, Vec<u8>)>> = HashMap::new();
        for tag in &tags {
            if tag.code == 1008 {
                if let Some(sub) = crate::swf::parse_sub_image(&tag.data) {
                    sub_by_image.entry(sub.image_id).or_default().push((sub, tag.data.clone()));
                }
            }
        }
        // Each external image with the key its conversion is cached under.
        let mut items: Vec<(crate::swf::Tag, Option<u64>)> = Vec::with_capacity(tags.len());
        let mut jobs: Vec<ImageJob> = Vec::new();
        for tag in tags {
            let img = if tag.code == 1001 || tag.code == 1009 { crate::swf::parse_external_image(tag.code, &tag.data) } else { None };
            let Some(img) = img else {
                items.push((tag, None));
                continue;
            };
            // GFx external images use a 32-bit id whose low 16 bits are the
            // character id SWF tags refer to (sub-images, bitmap fills); the
            // high half is the image namespace (`0x0009_0000` in these movies).
            let image_id = (img.id & 0xFFFF) as u16;
            let subs = sub_by_image.remove(&image_id).unwrap_or_default();
            let mut h = std::collections::hash_map::DefaultHasher::new();
            pkg_key.hash(&mut h);
            tag.data.hash(&mut h);
            for (_, d) in &subs {
                d.hash(&mut h);
            }
            let ck = h.finish();
            if !self.swf_embeds.contains_key(&ck) && !jobs.iter().any(|j| j.key == ck) {
                let texture = self.image_texture(&pkg_key, &img)?;
                jobs.push(ImageJob { key: ck, image_id, subs: subs.iter().map(|(s, _)| *s).collect(), img, texture });
            }
            items.push((tag, Some(ck)));
        }
        if !jobs.is_empty() {
            // Decoding a texture and compressing its bitmap is the slow part
            // of a first preview (a header with 184 images took 250 ms): the
            // images convert in parallel.
            let db = self.db()?;
            let src = self.src.as_ref().ok_or("no workspace")?;
            let ed = self.editors.get(&pkg_key).ok_or("package is not open")?;
            let done = tempest_unrealscript::par::map(&jobs, |j| convert_image(db, &ed.pkg, &ed.name, src, j));
            for (j, e) in jobs.iter().zip(done) {
                self.swf_embeds.insert(j.key, e);
            }
        }
        let mut images = 0usize;
        let mut consumed: std::collections::HashSet<u16> = std::collections::HashSet::new();
        let mut out = Vec::with_capacity(items.len());
        for (tag, ck) in items {
            let Some(embed) = ck.and_then(|ck| self.swf_embeds.get(&ck)) else {
                out.push(tag);
                continue;
            };
            let embed = embed.clone();
            missing.extend(embed.missing.iter().cloned());
            images += embed.tags.len();
            consumed.extend(embed.consumed.iter().copied());
            out.extend(embed.tags);
            if embed.keep {
                out.push(tag);
            }
            // An atlas with sub-images is no longer needed: only its
            // sub-images are referenced.
        }
        // Drop the sub-image tags whose cropped bitmap was emitted.
        out.retain(|t| t.code != 1008 || !crate::swf::parse_sub_image(&t.data).is_some_and(|s| consumed.contains(&s.id)));
        m.tags = out;
        Ok((images, missing))
    }

    /// The `Texture2D` an external image names (`Foo.tga` → `Foo`) and its
    /// parsed object, or why there is none.
    fn image_texture(&mut self, pkg_key: &str, img: &crate::swf::ExternalImage) -> Res<Result<(usize, Object), String>> {
        let ed = self.editors.get_mut(pkg_key).ok_or("package is not open")?;
        let file = img.file_name.rsplit('/').next().unwrap_or(&img.file_name);
        let stem = file.rsplit_once('.').map(|(name, _)| name).unwrap_or(file);
        let texture = (0..ed.pkg.exports.len()).find(|&i| {
            let idx = PackageIndex::export(i);
            ed.pkg.object_name(idx).eq_ignore_ascii_case(stem) && ed.pkg.class_name_of(idx).eq_ignore_ascii_case("Texture2D")
        });
        let Some(texture) = texture else {
            return Ok(Err(format!("{}: no Texture2D named {stem}", img.file_name)));
        };
        Ok(match ed.object(texture) {
            Ok(o) => Ok((texture, o.clone())),
            Err(err) => Err(format!("{}: {err}", img.file_name)),
        })
    }

    /// `swf.tree`: the character dictionary (kinds, linkage names, bounds,
    /// dependencies, attached tags) and the root timeline's size.
    fn swf_tree(&mut self, p: &J) -> Res<J> {
        let (_, _, m) = self.swf_edited(p)?;
        let dict = crate::swf::dict::Dict::build(&m);
        let (x0, x1, y0, y1) = m.frame();
        let root = crate::swf::display::timeline(&m.tags, 100_000);
        Ok(json!({
            "characters": dict.json(&m),
            "stage": { "x": x0, "y": y0, "w": x1 - x0, "h": y1 - y0 },
            "frames": m.frame_count(),
            "rootFrames": root.frames.len(),
            "labels": root.labels.iter().map(|(f, l)| json!({ "frame": f, "name": l })).collect::<Vec<_>>(),
        }))
    }

    /// `swf.timeline {sprite}`: the display list of a sprite (0: the root)
    /// after every frame, with the tag that placed, moved and last changed
    /// each object and its bounds in the sprite's coordinates.
    fn swf_timeline(&mut self, p: &J) -> Res<J> {
        let (_, _, m) = self.swf_edited(p)?;
        let sprite = opt_u(p, "sprite").unwrap_or(0) as u16;
        let limit = opt_u(p, "limit").unwrap_or(2000);
        let dict = crate::swf::dict::Dict::build(&m);
        let body;
        let tags: &[crate::swf::Tag] = if sprite == 0 {
            &m.tags
        } else {
            let c = dict.chars.get(&sprite).filter(|c| c.kind == "sprite").ok_or(format!("no sprite {sprite}"))?;
            body = crate::swf::display::sprite_body(&m.tags[c.tag.ok_or("imported sprite")?]);
            &body
        };
        let tl = crate::swf::display::timeline(tags, limit);
        let codes: Vec<u16> = tags.iter().map(|t| t.code).collect();
        let frames: Vec<J> = tl
            .frames
            .iter()
            .map(|list| {
                J::Array(
                    list.iter()
                        .map(|e| {
                            let rect = dict.bounds(&m, e.character).map(|r| crate::swf::dict::rect_json(e.matrix.apply_rect(r)));
                            let extras = crate::swf::display::Place::parse(codes[e.changed_by], &tags[e.changed_by].data).is_some_and(|p| p.has_extras());
                            json!({
                                "depth": e.depth,
                                "character": e.character,
                                "name": e.name,
                                "className": e.class_name,
                                "matrix": e.matrix.json(),
                                "cxform": (!e.cxform.is_identity()).then(|| e.cxform.json()),
                                "ratio": e.ratio,
                                "clipDepth": e.clip_depth,
                                "placedBy": e.placed_by,
                                "matrixBy": e.matrix_by,
                                "changedBy": e.changed_by,
                                "placeCode": codes[e.placed_by],
                                "since": e.since,
                                "extras": extras,
                                "rect": rect,
                            })
                        })
                        .collect(),
                )
            })
            .collect();
        Ok(json!({
            "sprite": sprite,
            "frames": frames,
            "labels": tl.labels.iter().map(|(f, l)| json!({ "frame": f, "name": l })).collect::<Vec<_>>(),
            "actions": tl.actions.iter().map(|(f, t)| json!({ "frame": f, "tag": t })).collect::<Vec<_>>(),
            "tags": tags.len(),
        }))
    }

    /// `swf.preview {target: {kind: root|frame|character, id, frame, play}, exclude?, rect?}`:
    /// a movie showing one character or one frame of a timeline, cut to its
    /// bounds (or `rect`, pixels), without the depths in `exclude`, with
    /// pending `edits` applied and external images embedded.
    fn swf_preview(&mut self, p: &J) -> Res<J> {
        use crate::swf::preview::{build, Target};
        let (path, _, m) = self.swf_edited(p)?;
        let t = p.get("target").cloned().unwrap_or(J::Null);
        let id = t.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
        let frame = t.get("frame").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let target = match t.get("kind").and_then(|v| v.as_str()).unwrap_or("root") {
            "root" => Target::Root(frame),
            "frame" => Target::Frame(id, frame),
            "character" => Target::Character(id, t.get("play").and_then(|v| v.as_bool()).unwrap_or(false)),
            k => return Err(format!("unknown preview target {k}").into()),
        };
        let dict = crate::swf::dict::Dict::build(&m);
        let exclude: Vec<u16> = p.get("exclude").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64()).map(|x| x as u16).collect()).unwrap_or_default();
        let view = p.get("rect").filter(|r| r.is_object()).map(|r| {
            let f = |k: &str| (r.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) * 20.0).round() as i32;
            [f("x"), f("y"), f("x") + f("w"), f("y") + f("h")]
        });
        let play = matches!(target, Target::Character(_, true));
        let mut preview = build(&m, &dict, target, &crate::swf::preview::Options { exclude, view });
        let (images, missing) = self.embed_images(&path, &mut preview.movie)?;
        let imports = preview.movie.tags.iter().filter(|t| t.code == 57 || t.code == 71).count();
        // A layout gets its own cut of each library; a playing target uses
        // the whole libraries (`swf.libraries`), its scripts may want any symbol.
        let mut subsets = Vec::new();
        if !play {
            if self.swf_subset_cache.len() > 256 {
                self.swf_subset_cache.clear();
            }
            self.swf_subsets(&mut preview.movie.tags, 0, &mut subsets)?;
        }
        let libraries: Vec<J> = subsets.into_iter().map(|(url, bytes)| json!({ "url": url, "swf": self.bins.add(bytes) })).collect();
        let bytes = preview.movie.write();
        let len = bytes.len();
        let mut out = json!({ "swf": self.bins.add(bytes), "bytes": len, "rect": crate::swf::dict::rect_json(preview.rect), "images": images, "missing": missing, "imports": imports });
        if !play {
            out["libraries"] = J::Array(libraries);
        }
        Ok(out)
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
            let stems: Vec<&str> = wave.iter().map(|(_, stem)| stem.as_str()).collect();
            let found = self.swf_lib_locations(&stems);
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
                let mut bytes = self.editor(&path_text)?.with_object(*i, media::swf_export)??;
                if let Ok(mut shared) = crate::swf::Movie::parse(&bytes) {
                    pending.extend(crate::swf::import_urls(&shared));
                    let merged = crate::swf::merge_imports(&mut shared.tags, |_| true);
                    let inflated = uncompressed(&mut shared);
                    if merged || inflated {
                        bytes = shared.write();
                    }
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

    /// Where the libraries named by import URL stems live. A library is
    /// looked up by the name its importer refers to (`SharedAssets`), not by
    /// the package it lives in; scanning every package's exports takes ~30 ms,
    /// so locations are remembered (`swf_libs`).
    fn swf_lib_locations(&mut self, stems: &[&str]) -> HashMap<String, (PathBuf, usize, String)> {
        let mut found: HashMap<String, (PathBuf, usize, String)> = HashMap::new();
        let mut unknown: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for &stem in stems {
            match self.swf_libs.get(stem).cloned() {
                Some(Some(loc)) if self.swf_lib_valid(&loc, stem) => {
                    found.insert(stem.to_string(), loc);
                }
                Some(None) => {}
                _ => {
                    unknown.insert(stem);
                }
            }
        }
        if unknown.is_empty() {
            return found;
        }
        self.index_all();
        let mut hits: HashMap<String, (PathBuf, usize, String)> = HashMap::new();
        'packages: for w in &self.packages {
            let k = key(&w.path.to_string_lossy());
            let Some(idx) = self.index.get(&k) else { continue };
            for (i, (cid, object_path)) in idx.exports.iter().enumerate() {
                if !idx.classes[*cid as usize].eq_ignore_ascii_case("SwfMovie") {
                    continue;
                }
                let name = object_path.rsplit('.').next().unwrap_or(object_path).to_ascii_lowercase();
                if unknown.contains(name.as_str()) && !hits.contains_key(&name) {
                    hits.insert(name.clone(), (w.path.clone(), i, object_path.clone()));
                    if hits.len() == unknown.len() {
                        break 'packages;
                    }
                }
            }
        }
        for stem in unknown {
            self.swf_libs.insert(stem.to_string(), hits.get(stem).cloned());
        }
        found.extend(hits);
        found
    }

    /// Point a layout's imports at subsets of their libraries
    /// (`preview::library_subset`), recursively for the libraries' own
    /// imports, each served under a URL made of its contents; `out` gets
    /// every subset. Imports of libraries the workspace lacks are left alone.
    fn swf_subsets(&mut self, tags: &mut [crate::swf::Tag], depth: usize, out: &mut Vec<(String, Vec<u8>)>) -> Res<()> {
        use std::hash::{Hash, Hasher};
        for t in tags.iter_mut().filter(|t| t.code == 57 || t.code == 71) {
            let Some((url, names)) = crate::swf::import_entries(t) else { continue };
            let stem = swf_stem(&url);
            let Some((path, i, _)) = self.swf_lib_locations(&[stem.as_str()]).remove(&stem) else { continue };
            let path_text = path.to_string_lossy().to_string();
            let mut sorted = names.clone();
            sorted.sort_unstable();
            let ck = format!("{}#{i}|{}", key(&path_text), sorted.join("\u{1}"));
            let subset = match self.swf_subset_cache.get(&ck) {
                Some(s) => s.clone(),
                None => {
                    let bytes = self.editor(&path_text)?.with_object(i, media::swf_export)??;
                    let Some(mut sub) = crate::swf::Movie::parse(&bytes).ok().and_then(|lib| crate::swf::preview::library_subset(&lib, &names)) else { continue };
                    let mut all = Vec::new();
                    if depth < 4 {
                        self.swf_subsets(&mut sub.tags, depth + 1, &mut all)?;
                    }
                    // The library's images come from its own package.
                    self.embed_images(&path_text, &mut sub)?;
                    let bytes = sub.write();
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    bytes.hash(&mut h);
                    let url = format!("swf-lib/{:016x}.swf", h.finish());
                    all.push((url.clone(), bytes));
                    self.swf_subset_cache.insert(ck, (url.clone(), all.clone()));
                    (url, all)
                }
            };
            let (sub_url, all) = subset;
            for (u, b) in all {
                if !out.iter().any(|(x, _)| *x == u) {
                    out.push((u, b));
                }
            }
            crate::swf::set_import_url(t, &sub_url);
        }
        Ok(())
    }

    /// A remembered library location still names a `SwfMovie` of that name
    /// (exports can be renamed or deleted since).
    fn swf_lib_valid(&mut self, (path, i, _): &(PathBuf, usize, String), stem: &str) -> bool {
        let Ok(ed) = self.editor(&path.to_string_lossy()) else { return false };
        if *i >= ed.pkg.exports.len() {
            return false;
        }
        let idx = PackageIndex::export(*i);
        ed.pkg.class_name_of(idx).eq_ignore_ascii_case("SwfMovie") && ed.pkg.object_name(idx).eq_ignore_ascii_case(stem)
    }

    /// `swf.script {loc | tag, edits?}`: one script as P-code and decompiled
    /// ActionScript (with the pending edits applied), and what the decompiler
    /// had to keep as P-code.
    fn swf_script(&mut self, p: &J) -> Res<J> {
        use crate::swf::as2;
        let (_, _, m) = self.swf_edited(p)?;
        let loc = match p.get("loc") {
            Some(l) => as2::Loc::from_json(l)?,
            None => as2::Loc::Tag { sprite: 0, tag: opt_u(p, "tag").ok_or("missing parameter loc")? },
        };
        let script = as2::action::Script::decode(&as2::script_bytes(&m, &loc)?)?;
        let pcode = as2::pcode::print(&script);
        Ok(json!({ "listing": pcode, "pcode": pcode, "source": as2::decompile::source(&script), "problems": as2::decompile::problems(&script) }))
    }

    /// `swf.compile {source | pcode}`: compile ActionScript or assemble P-code
    /// without writing it (errors name the line); the result as P-code.
    fn swf_compile(&mut self, p: &J) -> Res<J> {
        use crate::swf::as2;
        let script = match opt_s(p, "source") {
            Some(src) => as2::compile::compile(&src)?,
            None => as2::pcode::parse(s(p, "pcode")?)?,
        };
        let bytes = script.encode()?;
        Ok(json!({ "bytes": bytes.len(), "pcode": as2::pcode::print(&script) }))
    }

    /// Bitmaps of a movie for SVG export, by character id: external images
    /// decoded from their textures (as the preview embeds them) and the
    /// movie's own lossless bitmaps, as PNG at the size the movie draws them.
    fn swf_bitmaps(&mut self, path: &str, m: &crate::swf::Movie) -> HashMap<u16, (u32, u32, Vec<u8>)> {
        let mut copy = m.clone();
        let _ = self.embed_images(path, &mut copy);
        let mut out = HashMap::new();
        for t in &copy.tags {
            if let Some((id, w, h, rgba)) = crate::swf::decode_lossless(t) {
                if let Ok(png) = crate::dds::write_png(w, h, &rgba) {
                    out.insert(id, (w, h, png));
                }
            }
        }
        out
    }

    /// `swf.character {id}`: what the editor shows and edits of one character
    /// (pending `edits` applied): a shape's colours and line widths, a font's
    /// glyphs, a static text's records, an edit-text field's properties and
    /// the fonts it can use, a sound's format.
    fn swf_character(&mut self, p: &J) -> Res<J> {
        use crate::swf::{dict::Dict, font::Font, shape::Shape, text};
        let (_, _, m) = self.swf_edited(p)?;
        let id = opt_u(p, "id").ok_or("missing parameter id")? as u16;
        let dict = Dict::build(&m);
        let c = dict.chars.get(&id).ok_or(format!("no character {id}"))?;
        let Some(tag) = c.tag else { return Ok(json!({ "kind": c.kind })) };
        let t = &m.tags[tag];
        let fonts = |fid: u16| dict.chars.get(&fid).and_then(|c| c.tag).and_then(|ft| Font::parse(&m, ft).ok());
        Ok(match c.kind {
            "shape" => {
                let s = Shape::parse(t)?;
                let lines: Vec<J> = s
                    .groups
                    .iter()
                    .enumerate()
                    .flat_map(|(g, st)| st.lines.iter().enumerate().map(move |(i, l)| json!({ "key": format!("width:{g}:{i}"), "colorKey": format!("line:{g}:{i}"), "width": l.width as f64 / 20.0, "color": l.color.hex() })))
                    .collect();
                let bitmaps = s.groups.iter().flat_map(|g| g.fills.iter()).filter(|f| matches!(f, crate::swf::shape::Fill::Bitmap { .. })).count();
                json!({
                    "kind": "shape",
                    "tagName": crate::swf::tag_name(t.code),
                    "alpha": s.has_alpha(),
                    "colors": s.colors().into_iter().filter(|(k, _)| !k.starts_with("line:")).map(|(k, c)| json!({ "key": k, "color": c.hex() })).collect::<Vec<_>>(),
                    "lines": lines,
                    "bitmapFills": bitmaps,
                    "edges": s.edges.len(),
                })
            }
            "font" => {
                let f = Font::parse(&m, tag)?;
                let chars: String = f.codes.iter().filter_map(|&c| char::from_u32(c as u32)).collect();
                json!({
                    "kind": "font",
                    "tagName": crate::swf::tag_name(t.code),
                    "name": f.name(),
                    "glyphs": f.glyphs.len(),
                    "chars": chars,
                    "bold": f.bold(),
                    "italic": f.italic(),
                    "layout": f.layout.is_some(),
                    "alignZones": m.tags.iter().any(|x| x.code == 73 && crate::swf::u16_at_pub(&x.data, 0) == Some(id)),
                    "editable": t.code != 10,
                })
            }
            "text" => {
                let st = text::StaticText::parse(t).ok_or("bad DefineText")?;
                json!({ "kind": "text", "tagName": crate::swf::tag_name(t.code), "records": st.json(&fonts) })
            }
            "edittext" => {
                let et = text::EditText::parse(&t.data).ok_or("bad DefineEditText")?;
                let available: Vec<J> = dict
                    .chars
                    .values()
                    .filter(|c| c.kind == "font" || (c.kind == "import" && c.import.is_some()))
                    .map(|c| {
                        let name = if c.kind == "font" { fonts(c.id).map(|f| f.name()).unwrap_or_default() } else { c.names.first().cloned().unwrap_or_default() };
                        json!({ "id": c.id, "name": name, "imported": c.kind == "import" })
                    })
                    .collect();
                json!({ "kind": "edittext", "props": et.json(), "text": et.text, "fonts": available })
            }
            "sound" => {
                let b = t.data.get(2).copied().unwrap_or(0);
                let format = ["uncompressed", "ADPCM", "MP3", "uncompressed", "Nellymoser 16 kHz", "Nellymoser 8 kHz", "Nellymoser"].get((b >> 4) as usize).copied().unwrap_or("Speex");
                let ext = text::sound_file(t).map(|(e, _)| e).ok();
                const RATES: [u32; 4] = [5512, 11025, 22050, 44100];
                json!({ "kind": "sound", "format": format, "rate": RATES[((b >> 2) & 3) as usize], "channels": 1 + (b & 1), "samples": crate::swf::u32_at_pub(&t.data, 3), "ext": ext })
            }
            "binary" => json!({ "kind": "binary", "bytes": t.data.len().saturating_sub(6) }),
            k => json!({ "kind": k, "tagName": crate::swf::tag_name(t.code) }),
        })
    }

    /// `swf.export {target | id, format: svg|ttf|sound|binary, out}`: a
    /// character, sprite frame or root frame as SVG, a font as TrueType, a
    /// sound as MP3/WAV, binary data as is (pending `edits` applied).
    fn swf_export(&mut self, p: &J) -> Res<J> {
        use crate::swf::{dict::Dict, font::Font, svg, text};
        let (path, _, m) = self.swf_edited(p)?;
        let out = s(p, "out")?.to_string();
        let format = s(p, "format")?;
        let tag_of = |m: &crate::swf::Movie, id: u16| -> Res<usize> { Dict::build(m).chars.get(&id).and_then(|c| c.tag).ok_or_else(|| format!("no character {id} defined in this movie").into()) };
        let id = opt_u(p, "id").map(|v| v as u16);
        let mut warnings: Vec<String> = Vec::new();
        let bytes = match format {
            "svg" => {
                let t = p.get("target").cloned().unwrap_or_else(|| json!({ "kind": "character", "id": id }));
                let frame = t.get("frame").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let tid = t.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                let target = match t.get("kind").and_then(|v| v.as_str()).unwrap_or("character") {
                    "root" => svg::Target::Root { frame },
                    "frame" => svg::Target::Frame { sprite: tid, frame },
                    _ => svg::Target::Character { id: tid },
                };
                let bitmaps = self.swf_bitmaps(&path, &m);
                let (doc, w) = svg::export(&m, target, &mut |b| bitmaps.get(&b).cloned())?;
                warnings = w;
                doc.into_bytes()
            }
            "ttf" => Font::parse(&m, tag_of(&m, id.ok_or("missing parameter id")?)?)?.to_ttf()?,
            "sound" => text::sound_file(&m.tags[tag_of(&m, id.ok_or("missing parameter id")?)?])?.1,
            "binary" => text::binary_data(&m.tags[tag_of(&m, id.ok_or("missing parameter id")?)?])?,
            f => return Err(format!("unknown export format {f}").into()),
        };
        std::fs::write(&out, &bytes)?;
        Ok(json!({ "out": out, "bytes": bytes.len(), "warnings": warnings }))
    }

    /// `swf.addImage {file, name?, shape?, place?: {sprite, frame, x, y, name?}, format?, mips?, edits?}`:
    /// a new external image in the movie (pending `edits` applied first, as
    /// `swf.apply` does): a Texture2D made from the PNG/DDS `file` with the
    /// settings of a texture the movie already uses (same group, so the
    /// engine finds `<name>.tga` next to the others) and listed in the
    /// movie's `References`; a `DefineExternalImage2` naming it; with `shape`
    /// (default) a shape that draws it, and with `place` that shape on a
    /// frame of a timeline. One undo step.
    fn swf_add_image(&mut self, p: &J) -> Res<J> {
        use crate::swf::add;
        let (path, i, mut m) = self.swf_edited(p)?;
        let file = s(p, "file")?;
        let bytes = std::fs::read(file)?;
        let (width, height, source) = if file.to_ascii_lowercase().ends_with(".dds") {
            let d = crate::dds::read_dds(&bytes)?;
            (d.width, d.height, ImportSource::Encoded { format: d.format, width: d.width, height: d.height, mips: d.mips })
        } else {
            let (w, h, rgba) = crate::dds::read_png(&bytes)?;
            (w, h, ImportSource::Rgba { width: w, height: h, rgba })
        };
        let mut opts = ImportOptions { mips: opt_b(p, "mips"), format: None, keep_alpha: true, pad_blocks: true };
        if let Some(f) = opt_s(p, "format").filter(|f| !f.is_empty()) {
            opts.format = Some(crate::dds::PixelFormat::from_name(&f).ok_or_else(|| format!("unknown pixel format {f}"))?);
        }
        let db = self.db()?;

        // The texture to copy: one the movie draws, preferably in the movie's group.
        let style = add::ImageStyle::of(&m);
        let stems: Vec<String> = m
            .tags
            .iter()
            .filter_map(|t| crate::swf::parse_external_image(t.code, &t.data))
            .map(|img| {
                let file = img.file_name.rsplit('/').next().unwrap_or(&img.file_name).to_ascii_lowercase();
                file.rsplit_once('.').map(|(n, _)| n.to_string()).unwrap_or(file)
            })
            .collect();
        let ed = self.editor(&path)?;
        let movie_outer = ed.pkg.exports[i].outer;
        let is_texture = |ed: &Editor, j: usize| ed.pkg.class_name_of(PackageIndex::export(j)).eq_ignore_ascii_case("Texture2D");
        let template = (0..ed.pkg.exports.len())
            .filter(|&j| is_texture(ed, j) && stems.contains(&ed.pkg.object_name(PackageIndex::export(j)).to_ascii_lowercase()))
            .min_by_key(|&j| ed.pkg.exports[j].outer != movie_outer)
            .or_else(|| (0..ed.pkg.exports.len()).find(|&j| is_texture(ed, j) && ed.pkg.exports[j].outer == movie_outer))
            .ok_or("the movie uses no texture of this package to copy the settings of a new one from")?;

        // A name no other object of the package has, so `<name>.tga` is unambiguous.
        let wanted = opt_s(p, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| {
            let stem = std::path::Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or("Image");
            format!("{}_{stem}", ed.pkg.object_name(PackageIndex::export(i)))
        });
        let mut base: String = wanted.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
        if base.is_empty() {
            base = "Image".into();
        }
        let taken = |ed: &Editor, n: &str| (0..ed.pkg.exports.len()).any(|j| ed.pkg.object_name(PackageIndex::export(j)).eq_ignore_ascii_case(n));
        let mut name = base.clone();
        let mut k = 2;
        while taken(ed, &name) {
            name = format!("{base}_{k}");
            k += 1;
        }
        let file_name = format!("{name}{}", style.extension);

        // The movie.
        let image = add::next_id(&m)?;
        let mut defs = vec![add::external_image(&style, image, width, height, &file_name)?];
        let shape = if opt_b(p, "shape").unwrap_or(true) {
            if image >= 0xFFFE {
                return Err("the movie has no free character id for the shape".into());
            }
            defs.push(add::bitmap_shape(image + 1, image, width, height));
            Some(image + 1)
        } else {
            None
        };
        add::insert_definitions(&mut m, defs);
        let placed = match p.get("place").filter(|v| v.is_object()) {
            Some(pl) => {
                let shape = shape.ok_or("only a shape can be placed: add the image with its shape")?;
                let sprite = pl.get("sprite").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                let frame = pl.get("frame").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let x = pl.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let y = pl.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let depth = add::place(&mut m, sprite, frame, shape, x, y, pl.get("name").and_then(|v| v.as_str()))?;
                Some(json!({ "sprite": sprite, "frame": frame, "depth": depth }))
            }
            None => None,
        };
        let movie_bytes = m.write();

        // The package, as one undo step (rolled back if a step fails).
        let ed = self.editor(&path)?;
        ed.begin_undo(format!("add image {name} to {}", ed.pkg.object_name(PackageIndex::export(i))));
        let result = (|| -> Res<usize> {
            let t = ed.duplicate(template)?;
            ed.rename(t, &name)?;
            let mut o = ed.object(t)?.clone();
            o.net_index = -1;
            texture_ops::import(db, &mut ed.pkg, t, &mut o, source, &opts)?;
            ed.replace_object(t, o)?;
            ed.mark_dirty(t);
            let mut movie = ed.object(i)?.clone();
            media::swf_import(&mut ed.pkg, &mut movie, &movie_bytes)?;
            let reference = crate::value::Value::Object(PackageIndex::export(t));
            match movie.properties.iter_mut().find(|q| q.array_index == 0 && ed.pkg.fname_to_string(q.name).eq_ignore_ascii_case("References")) {
                Some(q) => match &mut q.value {
                    crate::value::Value::Array(items) => items.push(reference),
                    _ => return Err("the movie's References is not an array".into()),
                },
                None => media::set_property(&mut ed.pkg, &mut movie, "References", tempest_unrealscript::objects::PropertyKind::Array, crate::value::Value::Array(vec![reference])),
            }
            ed.replace_object(i, movie)?;
            ed.mark_dirty(i);
            Ok(t)
        })();
        let texture = match result {
            Ok(t) => t,
            Err(e) => {
                ed.undo();
                return Err(e);
            }
        };
        let undo = ed.undo_labels();
        let texture_path = ed.pkg.object_path(PackageIndex::export(texture));
        self.swf_cache = Some((format!("{}#{i}", key(&path)), m));
        Ok(json!({
            "texture": { "index": texture, "path": texture_path, "name": name },
            "fileName": file_name,
            "image": image,
            "shape": shape,
            "width": width,
            "height": height,
            "placed": placed,
            "bytes": movie_bytes.len(),
            "undo": undo,
        }))
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
        let undo = ed.undo_labels();
        self.swf_cache = Some((format!("{}#{i}", key(&path)), m));
        Ok(json!({ "edits": n, "bytes": bytes.len(), "undo": undo }))
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
                    let (d, warnings) = self.make_delta(path, &digest, &rel, false)?;
                    let ops = d.ops().len() + d.header["tables"].as_array().map(|a| a.iter().map(|t| t["ops"].as_array().map(|o| o.len()).unwrap_or(0)).sum::<usize>()).unwrap_or(0) + d.header["header"].as_object().map(|h| h.len()).unwrap_or(0);
                    if ops == 0 {
                        entries.push(json!({ "file": rel, "mode": "unchanged", "ops": 0, "bytes": 0, "warnings": warnings }));
                        continue;
                    }
                    let out = dir.join("deltas").join(format!("{rel}.tdelta"));
                    if let Some(parent) = out.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    d.write_shared(&out, dir.join("shared"))?;
                    added.push(format!("deltas/{rel}.tdelta"));
                    entries.push(json!({ "file": rel, "mode": "delta", "ops": ops, "bytes": d.to_bytes_external().len(), "warnings": warnings }));
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

    /// What a package or marshal of the cooker is compared against: the
    /// Studio's record, else the launcher's pristine backup; `None` when
    /// neither exists (the user confirms the game file or picks the original).
    fn base_of(target: &Path) -> J {
        if crate::basestore::read(target).is_some() {
            return json!({ "kind": "recorded", "recorded": crate::basestore::status(target)["recorded"] });
        }
        match crate::basestore::launcher_backup(target) {
            Some(b) => json!({ "kind": "backup", "path": b.to_string_lossy() }),
            // The game folder's own file is taken as the original.
            None => json!({ "kind": "game" }),
        }
    }

    /// `mod.changes`: everything a mod made from this workspace could hold,
    /// one entry per source of edited content: unsaved edits (in memory),
    /// game files saved in place since the Studio recorded their original,
    /// and exported copies ("Save as") of game files. Each names the game
    /// file it changes (`target`, `rel`) and what it is compared with (`base`).
    fn mod_changes(&mut self) -> Res<J> {
        let dir = self.dir()?.to_path_buf();
        let root = crate::twrk::game_root(&dir);
        let mut out: Vec<J> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let entry = |source: &Path, target: &Path, kind: &str, origin: &str, script: bool, extra: J| -> J {
            let mut e = json!({
                "id": format!("{origin}|{}", key(&source.to_string_lossy())),
                "source": source.to_string_lossy(), "target": target.to_string_lossy(),
                "rel": crate::basestore::game_relative(target), "kind": kind, "origin": origin,
                "script": script, "base": Self::base_of(target),
            });
            if let (Some(m), Some(x)) = (e.as_object_mut(), extra.as_object()) {
                m.extend(x.clone());
            }
            e
        };
        for (k, ed) in &self.editors {
            if !ed.is_dirty() {
                continue;
            }
            let path = PathBuf::from(k);
            seen.insert(key(k));
            let is_script = ed.pkg.summary.package_flags & pkgf::CONTAINS_SCRIPT != 0;
            out.push(entry(&path, &path, "upk", "unsaved", is_script, json!({})));
        }
        if let Some(ws) = &self.mcts {
            for (id, doc) in &ws.docs {
                if doc.dirty {
                    seen.insert(key(&doc.path.to_string_lossy()));
                    out.push(entry(&doc.path, &doc.path, "mcts", "unsaved", false, json!({ "doc": id })));
                }
            }
        }
        for (file, info) in crate::basestore::all() {
            let k = key(&file.to_string_lossy());
            if seen.contains(&k) || !file.exists() || crate::twrk::relative(&root, &file).is_none() {
                continue;
            }
            let kind = if file.to_string_lossy().to_ascii_lowercase().ends_with(".dat") { "mcts" } else { "upk" };
            out.push(entry(&file, &file, kind, "saved", false, json!({ "when": info["recorded"] })));
        }
        for (original, copy, saved) in crate::copies::all() {
            if crate::twrk::relative(&root, &original).is_none() {
                continue;
            }
            let kind = if copy.to_string_lossy().to_ascii_lowercase().ends_with(".dat") { "mcts" } else { "upk" };
            out.push(entry(&copy, &original, kind, "copy", false, json!({ "when": saved })));
        }
        out.sort_by(|a, b| (a["rel"].as_str(), a["origin"].as_str()).cmp(&(b["rel"].as_str(), b["origin"].as_str())));
        Ok(json!({ "files": out }))
    }

    /// One cooker entry as a delta (or the whole file): `it` = `{source,
    /// target, kind, mode}`. Unsaved edits are taken from memory. Returns the
    /// summary and, when something changed, the folder (`deltas`/`files`),
    /// path and bytes to write. Errors with `needs-original:` when no
    /// original is known for a delta.
    fn mod_item(&mut self, it: &J, sharing: Sharing, preview: bool) -> Res<Item> {
        let source = PathBuf::from(it["source"].as_str().unwrap_or_default());
        let target = it["target"].as_str().map(PathBuf::from).unwrap_or_else(|| source.clone());
        let rel = crate::basestore::game_relative(&target);
        let whole = it["mode"].as_str() == Some("whole");
        let sk = key(&source.to_string_lossy());
        let missing = || format!("{rel}: the game file is not in the game folder, so there is nothing to compare with; ship the whole file instead");
        if it["kind"] == "mcts" {
            let (tokens, edited) = match self.mcts.as_ref().and_then(|ws| ws.docs.iter().find(|(_, d)| key(&d.path.to_string_lossy()) == sk).map(|(_, d)| (ws.tokens.clone(), d.marshal.clone()))) {
                Some(x) => x,
                None => {
                    let tokens = mcts::Tokens::for_dir(target.parent().unwrap_or(Path::new(".")));
                    let m = mcts::parse_obscured(&std::fs::read(&source)?, &tokens)?;
                    (tokens, m)
                }
            };
            if whole {
                let bytes = mcts::write_obscured(&edited, &tokens);
                return Ok((json!({ "rel": rel, "mode": "whole", "bytes": bytes.len() }), Some(("files", rel, Payload::Bytes(bytes))), Shared::new()));
            }
            let d = match crate::basestore::read(&target) {
                Some(digest) => mcts::delta::create(&tokens, mcts::delta::Base::Digest(&digest), &edited, &rel)?,
                None => match crate::basestore::launcher_backup(&target) {
                    Some(b) => {
                        let base = mcts::parse_obscured(&std::fs::read(b)?, &tokens)?;
                        mcts::delta::create(&tokens, mcts::delta::Base::Marshal(&base), &edited, &rel)?
                    }
                    // The game folder's file is the original by default.
                    None => {
                        let bytes = std::fs::read(&target).map_err(|_| missing())?;
                        let base = mcts::parse_obscured(&bytes, &tokens)?;
                        mcts::delta::create(&tokens, mcts::delta::Base::Marshal(&base), &edited, &rel)?
                    }
                },
            };
            let mut changes: Vec<J> = Vec::new();
            let (mut added, mut changed, mut deleted) = (0, 0, 0);
            for t in d.header["tables"].as_array().cloned().unwrap_or_default() {
                for o in t["ops"].as_array().cloned().unwrap_or_default() {
                    match o["op"].as_str() {
                        Some("add") => added += 1,
                        Some("delete") => deleted += 1,
                        _ => changed += 1,
                    }
                    if changes.len() < 300 {
                        let label = o["row"]["REFERENCE_NAME"].as_str().or(o["row"]["NAME"].as_str()).unwrap_or("");
                        changes.push(json!({ "op": o["op"], "what": format!("{} {} {}", t["table"].as_str().unwrap_or("").trim_start_matches("DATA_SET_"), o["key"].as_str().unwrap_or(""), label).trim().to_string() }));
                    }
                }
            }
            let header = d.header["header"].as_object().map(|h| h.len()).unwrap_or(0);
            changed += header;
            let summary = json!({ "rel": rel, "mode": "delta", "added": added, "changed": changed, "deleted": deleted, "changes": changes });
            if added + changed + deleted == 0 {
                return Ok((summary, None, Shared::new()));
            }
            return Ok((summary, Some(("deltas", format!("{rel}.tdelta"), Payload::Bytes(d.to_bytes()))), Shared::new()));
        }
        // Large payloads go to shared object files: an object the mod changes
        // the same way in several packages is shipped once.
        let share = share_options(sharing, preview, self.compression);
        let db = self.db()?;
        let edited_bytes = match self.editors.get_mut(&sk) {
            Some(ed) if ed.is_dirty() => ed.bytes(),
            _ => std::fs::read(&source)?,
        };
        if whole {
            let n = edited_bytes.len();
            return Ok((json!({ "rel": rel, "mode": "whole", "bytes": n }), Some(("files", rel, Payload::Bytes(edited_bytes))), Shared::new()));
        }
        package_item(db, &edited_bytes, &target, &rel, share, preview).map_err(|e| e.into())
    }

    /// What a cooker item's result depends on, for the caches: `None` when it
    /// has unsaved edits in memory (always made again).
    fn item_stamp(&self, it: &J) -> Option<String> {
        let source = it["source"].as_str().unwrap_or_default().to_string();
        let target = it["target"].as_str().unwrap_or_default().to_string();
        let sk = key(&source);
        if self.editors.get(&sk).is_some_and(|e| e.is_dirty()) || self.mcts.as_ref().is_some_and(|ws| ws.docs.iter().any(|(_, d)| d.dirty && key(&d.path.to_string_lossy()) == sk)) {
            return None;
        }
        let stamp = |p: &Path| std::fs::metadata(p).ok().map(|m| format!("{}:{:?}", m.len(), m.modified().ok()));
        let t = Path::new(&target);
        Some(format!("{source}|{target}|{}|{}|{:?}|{:?}|{:?}|{:?}", it["mode"], it["kind"], stamp(Path::new(&source)), stamp(t), crate::basestore::launcher_backup(t).as_deref().and_then(stamp), crate::basestore::read(t).map(|d| d.to_string().len())))
    }

    /// [`Server::mod_item`] for several items, the package comparisons (the
    /// slow part: parsing and diffing whole packages) on every core.
    fn mod_items(&mut self, items: &[J], sharing: &dyn Fn(usize) -> Sharing, preview: bool) -> Vec<Res<Item>> {
        let mut out: Vec<Option<Res<Item>>> = (0..items.len()).map(|_| None).collect();
        struct Job {
            k: usize,
            bytes: Vec<u8>,
            target: PathBuf,
            rel: String,
            share: crate::delta::CreateOptions,
        }
        let mut jobs: Vec<Job> = Vec::new();
        for (k, it) in items.iter().enumerate() {
            let source = PathBuf::from(it["source"].as_str().unwrap_or_default());
            if it["kind"] == "mcts" || it["mode"].as_str() == Some("whole") {
                out[k] = Some(self.mod_item(it, sharing(k), preview));
                continue;
            }
            let target = it["target"].as_str().map(PathBuf::from).unwrap_or_else(|| source.clone());
            let sk = key(&source.to_string_lossy());
            let bytes = match self.editors.get_mut(&sk) {
                Some(ed) if ed.is_dirty() => Ok(ed.bytes()),
                _ => std::fs::read(&source),
            };
            match bytes {
                Ok(bytes) => jobs.push(Job { k, bytes, rel: crate::basestore::game_relative(&target), target, share: share_options(sharing(k), preview, self.compression) }),
                Err(e) => out[k] = Some(Err(e.into())),
            }
        }
        if !jobs.is_empty() {
            match self.db() {
                Ok(db) => {
                    let done = crate::delta::parallel_map(&jobs, |j| package_item(db, &j.bytes, &j.target, &j.rel, j.share.clone(), preview));
                    for (j, r) in jobs.iter().zip(done) {
                        out[j.k] = Some(r.map_err(|e| e.into()));
                    }
                }
                Err(e) => {
                    let msg = e.to_string();
                    for j in &jobs {
                        out[j.k] = Some(Err(msg.clone().into()));
                    }
                }
            }
        }
        out.into_iter().map(|r| r.unwrap_or_else(|| Err("item not made".into()))).collect()
    }

    /// `mod.preview {items}`: what each cooker entry would put in the mod,
    /// without writing anything (`error` set on entries that cannot be made).
    fn mod_preview(&mut self, p: &J) -> Res<J> {
        let mut out: Vec<J> = Vec::new();
        let mut todo: Vec<(usize, J, Option<String>)> = Vec::new();
        for it in p.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
            // Files on disk that did not change since the last preview: the same answer.
            let cache_key = self.item_stamp(&it);
            if let Some(hit) = cache_key.as_ref().and_then(|k| self.preview_cache.get(k)) {
                let mut summary = hit.clone();
                summary["id"] = it["id"].clone();
                out.push(summary);
                continue;
            }
            todo.push((out.len(), it, cache_key));
            out.push(J::Null);
        }
        let items: Vec<J> = todo.iter().map(|(_, it, _)| it.clone()).collect();
        let made = self.mod_items(&items, &|_| Sharing::No, true);
        for ((at, it, cache_key), r) in todo.into_iter().zip(made) {
            out[at] = match r {
                Ok((mut summary, _, _)) => {
                    if let Some(k) = cache_key {
                        self.preview_cache.insert(k, summary.clone());
                    }
                    summary["id"] = it["id"].clone();
                    summary
                }
                Err(e) => json!({ "id": it["id"], "error": e.to_string() }),
            };
        }
        Ok(json!({ "items": out }))
    }

    /// `mod.cook {dir, id, name, version, authors, readme, dependencies, icon,
    /// items: [{source, target, kind, mode}], extras: [{src, dest}]}`: write
    /// a launcher mod folder (what `tempest mod pack` packages):
    /// `manifest.toml` (the launcher cooker's format), `README.md`,
    /// `icon.png`, every item as `deltas/<game path>.tdelta` or
    /// `files/<game path>` (unsaved edits taken from memory), and the extra
    /// files at their destination inside the mod.
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

        let mut entries: Vec<J> = Vec::new();
        // Shared object files, written once at `shared/<hash>.tobj` however
        // many deltas use them.
        // Only objects two or more deltas use are shared: a delta whose large
        // objects are its own is made again without sharing, so it stays a
        // version 1 file that every launcher reads.
        let items = p.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        // Keep the caches from growing without bound (a few builds of big mods).
        if self.cook_cache.values().map(|m| m.1.as_ref().map(|f| f.2.len()).unwrap_or(0)).sum::<usize>() + self.packed_cache.values().map(|b| b.len()).sum::<usize>() > (1 << 30) {
            self.cook_cache.clear();
            self.packed_cache.clear();
        }
        self.compression = crate::delta::Compression::parse(&opt_s(p, "compression").unwrap_or_default());
        let mode = self.compression;
        let stamps: Vec<Option<String>> = items.iter().map(|it| self.item_stamp(it)).collect();
        // Which large objects several deltas contain: found without compressing
        // (remembered for files that did not change).
        let mut holds: Vec<Vec<String>> = stamps.iter().map(|st| st.as_ref().and_then(|k| self.holds_cache.get(k)).cloned().unwrap_or_default()).collect();
        let unknown: Vec<usize> = (0..items.len()).filter(|&k| stamps[k].as_ref().is_none_or(|st| !self.holds_cache.contains_key(st))).collect();
        let found = self.mod_items(&unknown.iter().map(|&k| items[k].clone()).collect::<Vec<_>>(), &|_| Sharing::All, true);
        for (&k, r) in unknown.iter().zip(found) {
            holds[k] = r?.2.into_keys().collect();
            if let Some(st) = &stamps[k] {
                self.holds_cache.insert(st.clone(), holds[k].clone());
            }
        }
        let mut users: HashMap<&str, usize> = HashMap::new();
        for h in holds.iter().flatten() {
            *users.entry(h.as_str()).or_default() += 1;
        }
        let multi: std::sync::Arc<HashSet<String>> = std::sync::Arc::new(users.into_iter().filter(|(_, n)| *n >= 2).map(|(h, _)| h.to_string()).collect());
        // Those objects are stored once, as shared object files; everything
        // else stays in each delta, where like objects compress together
        // (sharing every large object made Cassie Bow 80.8 MB, sharing none
        // 74.6 MB, sharing those 63.7 MB).
        let sharing_key = |k: usize| {
            let mut mine: Vec<&String> = holds[k].iter().filter(|h| multi.contains(*h)).collect();
            mine.sort();
            mine.iter().map(|h| &h[..16]).collect::<Vec<_>>().join(",")
        };
        let cache_keys: Vec<Option<String>> = (0..items.len()).map(|k| stamps[k].as_ref().map(|st| format!("{st}|{}|{mode:?}", sharing_key(k)))).collect();
        let mut made: Vec<Option<Item>> = cache_keys.iter().map(|ck| ck.as_ref().and_then(|c| self.cook_cache.get(c)).map(cached_copy)).collect();
        let todo: Vec<usize> = (0..items.len()).filter(|&k| made[k].is_none()).collect();
        let todo_items: Vec<J> = todo.iter().map(|&k| items[k].clone()).collect();
        let holds_ref = &holds;
        let fresh = self.mod_items(&todo_items, &|t| if holds_ref[todo[t]].iter().any(|h| multi.contains(h)) { Sharing::Only(multi.clone()) } else { Sharing::No }, false);
        for (&k, r) in todo.iter().zip(fresh) {
            made[k] = Some(r?);
        }
        let mut made: Vec<Item> = made.into_iter().map(|m| m.expect("every item made")).collect();
        // Shared object files not compressed yet go through the same pool as the deltas.
        let mut unique: Vec<(String, Vec<u8>)> = Vec::new();
        for m in &made {
            for (h, b) in &m.2 {
                if !b.is_empty() && !self.packed_cache.contains_key(&format!("{h}|{mode:?}")) && !unique.iter().any(|(x, _)| x == h) {
                    unique.push((h.clone(), b.clone()));
                }
            }
        }
        let extra: Vec<(&[u8], crate::delta::Compression)> = unique.iter().map(|(_, b)| (b.as_slice(), mode)).collect();
        let packed_list = {
            let mut all: Vec<&mut Item> = made.iter_mut().collect();
            encode_pending(&mut all, &extra)
        };
        for ((h, _), p) in unique.iter().zip(packed_list) {
            self.packed_cache.insert(format!("{h}|{mode:?}"), p);
        }
        drop(extra);
        for (k, ck) in cache_keys.iter().enumerate() {
            if let (Some(c), true) = (ck, todo.contains(&k)) {
                self.cook_cache.insert(c.clone(), cached_copy(&made[k]));
            }
        }
        let packed_objects: HashMap<String, Vec<u8>> = made.iter().flat_map(|m| m.2.keys()).filter_map(|h| Some((h.clone(), self.packed_cache.get(&format!("{h}|{mode:?}"))?.clone()))).collect();
        let mut shared_written: HashMap<String, usize> = HashMap::new();
        let mut shared_uses = 0usize;
        let mut shared_saved = 0usize;
        for (mut summary, file, shared) in made {
            for (hash, bytes) in shared {
                shared_uses += 1;
                if let Some(&n) = shared_written.get(&hash) {
                    shared_saved += n;
                    continue;
                }
                let out = dir.join("shared").join(format!("{hash}.{}", crate::delta::SHARED_EXT));
                std::fs::create_dir_all(dir.join("shared"))?;
                let packed = packed_objects.get(&hash).cloned().unwrap_or_else(|| crate::delta::pack_with(&bytes, mode));
                std::fs::write(&out, &packed)?;
                shared_written.insert(hash, packed.len());
            }
            match file {
                Some((folder, name, Payload::Bytes(bytes))) => {
                    let out = dir.join(folder).join(&name);
                    if let Some(parent) = out.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&out, bytes)?;
                    summary["entry"] = J::from(format!("{folder}/{name}"));
                }
                Some((_, _, Payload::Delta(_))) => unreachable!("encoded above"),
                None => summary["mode"] = J::from("unchanged"),
            }
            summary.as_object_mut().map(|o| o.remove("changes"));
            entries.push(summary);
        }
        for x in p.get("extras").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
            let src = PathBuf::from(x["src"].as_str().unwrap_or_default());
            let dest = x["dest"].as_str().unwrap_or_default().trim_start_matches('/').replace('\\', "/");
            // A folder with no destination is laid out like the mod itself
            // (a project's `mod-files/`: scripts/, dlls/, files/…).
            if dest.is_empty() && src.is_dir() {
                let mut n = 0;
                for e in std::fs::read_dir(&src)? {
                    let e = e?;
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') || name.eq_ignore_ascii_case("manifest.toml") || name.eq_ignore_ascii_case(".tempest") {
                        continue;
                    }
                    if e.file_type()?.is_dir() {
                        copy_dir(&e.path(), &dir.join(&name))?;
                    } else {
                        std::fs::copy(e.path(), dir.join(&name))?;
                    }
                    n += 1;
                }
                if n > 0 {
                    entries.push(json!({ "rel": src.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(), "entry": ".", "mode": "extra" }));
                }
                continue;
            }
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
            entries.push(json!({ "rel": dest, "entry": dest, "mode": "extra" }));
        }
        let shared_bytes: usize = shared_written.values().sum();
        Ok(json!({ "dir": dir.to_string_lossy(), "entries": entries, "shared": { "files": shared_written.len(), "uses": shared_uses, "bytes": shared_bytes, "saved": shared_saved } }))
    }

    /// `mod.match {path}`: the game file a package or `.dat` from anywhere is
    /// a modified copy of (same file name in the workspace's folders), for
    /// files added to the cooker by hand.
    fn mod_match(&mut self, p: &J) -> Res<J> {
        let path = PathBuf::from(s(p, "path")?);
        let file_name = path.file_name().unwrap_or_default().to_os_string();
        let name = file_name.to_string_lossy().to_ascii_lowercase();
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let dir = self.dir()?.to_path_buf();
        let root = crate::twrk::game_root(&dir);
        // The workspace's own listing first: it has the file's real case.
        let mut found = self.packages.iter().find(|w| w.name.eq_ignore_ascii_case(&stem) && w.path.extension() == path.extension()).map(|w| w.path.clone());
        if found.is_none() {
            found = [dir.join(&file_name), root.join("ChaosGame").join("CookedPCConsole").join(&file_name)].into_iter().find(|c| c.exists());
        }
        if found.is_none() && name.starts_with("lang_") && name.ends_with(".dat") {
            let ext = name.trim_start_matches("lang_").trim_end_matches(".dat").to_ascii_uppercase();
            let c = root.join("ChaosGame").join("Localization").join(&ext).join(path.file_name().unwrap_or_default());
            if c.exists() {
                found = Some(c);
            }
        }
        Ok(json!({ "target": found.map(|f| f.to_string_lossy().to_string()) }))
    }

    /// `mod.forgetCopy {path}`: drop an exported copy from the cooker's list.
    fn mod_forget_copy(&mut self, p: &J) -> Res<J> {
        crate::copies::remove(Path::new(s(p, "path")?));
        Ok(J::Bool(true))
    }

    /// The delta of a file on disk against its recorded base: rows for a
    /// marshal, objects for a package (the open editor's unsaved edits are
    /// not part of it; the dialog asks for a save first).
    fn make_delta(&mut self, path: &Path, digest: &J, target: &str, share: bool) -> Res<(crate::delta::Delta, Vec<String>)> {
        let lower = path.to_string_lossy().to_ascii_lowercase();
        if lower.ends_with(".dat") {
            let tokens = mcts::Tokens::for_dir(path.parent().unwrap_or(Path::new(".")));
            let m = mcts::parse_obscured(&std::fs::read(path)?, &tokens)?;
            let d = mcts::delta::create(&tokens, mcts::delta::Base::Digest(digest), &m, target)?;
            return Ok((d, Vec::new()));
        }
        let db = self.db()?;
        let pkg = PackageFile::open(path)?.load()?;
        let opts = crate::delta::CreateOptions { share_min: share.then_some(crate::delta::SHARE_MIN), ..Default::default() };
        Ok(crate::delta::create_with(db, crate::delta::Base::Digest(digest), &pkg, target, opts)?)
    }

    /// `delta.record {path}`: take the file as it is now as the base (after
    /// restoring the original, or when a package was edited before the
    /// Studio kept records). Replaces any earlier record.
    fn delta_record(&mut self, p: &J) -> Res<J> {
        let path = PathBuf::from(s(p, "path")?);
        let t = std::time::Instant::now();
        // `from`: another file holding the original (a backup, a clean install).
        if let Some(from) = opt_s(p, "from") {
            let tokens = mcts::Tokens::for_dir(path.parent().unwrap_or(Path::new(".")));
            crate::basestore::record_from(self.db()?, Some(&tokens), &path, Path::new(&from))?;
            let mut st = crate::basestore::status(&path);
            st["ms"] = J::from(t.elapsed().as_millis() as u64);
            return Ok(st);
        }
        crate::basestore::remove(&path);
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
        let (d, warnings) = self.make_delta(&path, &digest, &target, false)?;
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

/// Write a movie for Ruffle without zlib (`CFX` → `GFX`, `CWS` → `FWS`):
/// deflating a 7 MB movie costs more than the bytes it saves over IPC, and
/// Ruffle inflates every library again on each load. Returns whether it was
/// compressed.
fn uncompressed(m: &mut crate::swf::Movie) -> bool {
    match &m.signature {
        b"CFX" => m.signature = *b"GFX",
        b"CWS" => m.signature = *b"FWS",
        _ => return false,
    }
    true
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
    // Only Ruffle in this process reads these: the fast level compresses
    // several times quicker for slightly larger bitmaps.
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
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

/// A file of a cooked mod: folder (`deltas`/`files`), path inside it, bytes.
type ModFile = (&'static str, String, Payload);

/// What a mod file holds: its bytes, or a delta still to encode (compressing
/// is the slow part; the cooker encodes them all at once, in parallel).
enum Payload {
    Bytes(Vec<u8>),
    Delta(Box<crate::delta::Delta>),
}

impl Payload {
    fn len(&self) -> usize {
        match self {
            Payload::Bytes(b) => b.len(),
            Payload::Delta(_) => 0,
        }
    }
}

/// Encode every pending delta of `made` (in parallel), and note its size.
fn encode_pending(made: &mut [&mut Item], extra: &[(&[u8], crate::delta::Compression)]) -> Vec<Vec<u8>> {
    let mut jobs: Vec<(usize, Box<crate::delta::Delta>)> = Vec::new();
    for (i, m) in made.iter_mut().enumerate() {
        if let Some(f) = m.1.as_mut() {
            if matches!(f.2, Payload::Delta(_)) {
                if let Payload::Delta(d) = std::mem::replace(&mut f.2, Payload::Bytes(Vec::new())) {
                    jobs.push((i, d));
                }
            }
        }
    }
    // Every part of every delta through one compression pool (biggest first,
    // within the memory budget), then each delta assembled.
    // (`extra`, the shared object files, go through the same pool.)
    let parts: Vec<Vec<&[u8]>> = jobs.iter().map(|(_, d)| d.parts(false)).collect();
    let flat: Vec<(&[u8], crate::delta::Compression)> = jobs.iter().zip(&parts).flat_map(|((_, d), p)| p.iter().map(|b| (*b, d.compression))).chain(extra.iter().copied()).collect();
    let mut packed = crate::delta::pack_all_with(&flat).into_iter();
    let encoded: Vec<Vec<u8>> = jobs.iter().zip(&parts).map(|((_, d), p)| d.assemble(packed.by_ref().take(p.len()).collect(), false)).collect();
    let extra_packed: Vec<Vec<u8>> = packed.collect();
    drop(parts);
    for ((i, _), bytes) in jobs.into_iter().zip(encoded) {
        made[i].0["bytes"] = J::from(bytes.len());
        if let Some(f) = made[i].1.as_mut() {
            f.2 = Payload::Bytes(bytes);
        }
    }
    extra_packed
}
/// The package part of a cooker item (`Server::mod_item`): parse the edited
/// package, compare it with its original, summarise. Needs no server state,
/// so the cooker runs it for several files at once.
fn package_item(db: &ClassDb, edited_bytes: &[u8], target: &Path, rel: &str, share: crate::delta::CreateOptions, preview: bool) -> Result<Item, String> {
    let run = || -> Res<Item> {
        let rel = rel.to_string();
        let missing = || format!("{rel}: the game file is not in the game folder, so there is nothing to compare with; ship the whole file instead");
            let edited = Package::parse(edited_bytes)?;
            let (mut d, warnings) = match crate::basestore::read(target) {
                Some(digest) => crate::delta::create_with(db, crate::delta::Base::Digest(&digest), &edited, &rel, share.clone())?,
                None => match crate::basestore::launcher_backup(target) {
                    Some(b) => {
                        let base = PackageFile::open(&b)?.load()?;
                        crate::delta::create_with(db, crate::delta::Base::Package(&base), &edited, &rel, share.clone())?
                    }
                    // The game folder's file is the original by default.
                    None => {
                        if !target.exists() {
                            return Err(missing().into());
                        }
                        let base = PackageFile::open(target)?.load()?;
                        crate::delta::create_with(db, crate::delta::Base::Package(&base), &edited, &rel, share.clone())?
                    }
                },
            };
            let ops = d.ops();
            let count = |k: &str| ops.iter().filter(|o| o.op == k).count();
            let changes: Vec<J> = ops.iter().take(300).map(|o| json!({ "op": o.op, "what": format!("{} ({})", o.path, o.class) })).collect();
            let shared: Vec<J> = d.shared.iter().map(|(h, b)| json!({ "hash": h, "bytes": b.len() })).collect();
            let summary = json!({ "rel": rel, "mode": "delta", "added": count("add"), "changed": count("set") + count("patch"), "deleted": count("delete"), "changes": changes, "warnings": warnings, "shared": shared });
            if ops.is_empty() {
                return Ok((summary, None, Shared::new()));
            }
            // Encoding (compressing) is left to the build (`encode_pending`); a preview never needs it.
            let objects = std::mem::take(&mut d.shared);
            Ok((summary, Some(("deltas", format!("{rel}.tdelta"), if preview { Payload::Bytes(Vec::new()) } else { Payload::Delta(Box::new(d)) })), objects))
    };
    run().map_err(|e| e.to_string())
}

/// A copy of a finished cooker item for the cache (or out of it): the encoded
/// file, and the shared objects' hashes (their files are in `packed_cache`).
fn cached_copy(m: &Item) -> Item {
    let file = m.1.as_ref().map(|(folder, name, payload)| {
        let bytes = match payload {
            Payload::Bytes(b) => b.clone(),
            Payload::Delta(_) => Vec::new(),
        };
        (*folder, name.clone(), Payload::Bytes(bytes))
    });
    (m.0.clone(), file, m.2.keys().map(|h| (h.clone(), Vec::new())).collect())
}

/// The delta options for a cooker item.
fn share_options(sharing: Sharing, preview: bool, compression: crate::delta::Compression) -> crate::delta::CreateOptions {
    crate::delta::CreateOptions {
        compression,
        share_min: (!matches!(sharing, Sharing::No)).then_some(crate::delta::SHARE_MIN),
        fast: preview,
        share_only: match sharing {
            Sharing::Only(set) => Some(set),
            _ => None,
        },
    }
}

/// How the cooker makes a delta: sharing no large object, every one (to find
/// which ones several deltas contain), or only these.
enum Sharing {
    No,
    All,
    Only(std::sync::Arc<HashSet<String>>),
}

/// Shared object files of a delta, by hash.
type Shared = std::collections::BTreeMap<String, Vec<u8>>;
/// A cooker item: its summary, the file it puts in the mod, its shared objects.
type Item = (J, Option<ModFile>, Shared);

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

// ----------------------------------------------------------------------------
// Animations (`anim.*`)
// ----------------------------------------------------------------------------

use crate::value::Value;

/// What `anim.*` needs about a sequence.
struct AnimContext {
    anim: crate::anim::Anim,
    header: crate::anim::Header,
    binding: crate::anim_ops::Binding,
    preview: Option<String>,
    set: usize,
}

impl Server {
    fn anim_context(&mut self, path: &str, i: usize) -> Res<AnimContext> {
        let ed = self.editor(path)?;
        ed.flush();
        let o = ed.object(i)?.clone();
        let header = crate::anim::header(&ed.pkg, &o);
        let stream = crate::anim::stream(&o).ok_or("not an AnimSequence")?;
        let anim = crate::anim::decode(&header, stream)?;
        let set = ed.pkg.exports[i].outer.export_index().ok_or("this sequence is not in an AnimSet")?;
        let set_obj = ed.object(set)?.clone();
        let (binding, preview) = crate::anim_ops::binding_of(&ed.pkg, &set_obj);
        Ok(AnimContext { anim, header, binding, preview, set })
    }

    /// Skeletal meshes the sequence can play on: the AnimSet's preview mesh
    /// (found in any package of the workspace), then the sequence's package's
    /// meshes by bones in common.
    fn anim_meshes(&mut self, path: &str, ctx: &AnimContext) -> Res<Vec<J>> {
        let db = self.db()?;
        let mut out = Vec::new();
        {
            let ed = self.editor(path)?;
            for (i, matched) in crate::anim_ops::mesh_candidates(db, &ed.pkg, &ctx.binding, ctx.preview.as_deref()) {
                let p = ed.pkg.object_path(PackageIndex::export(i));
                let is_preview = ctx.preview.as_deref().is_some_and(|pv| pv.eq_ignore_ascii_case(&p));
                out.push(json!({ "path": path, "index": i, "object": p, "matched": matched, "preview": is_preview }));
            }
        }
        if let Some(pv) = ctx.preview.clone() {
            if !out.iter().any(|m| m["preview"] == true) {
                self.index_all();
                let found = index_lookup(&self.index, &self.packages, &pv).and_then(|name| self.packages.iter().find(|w| w.name == name).map(|w| w.path.to_string_lossy().to_string()));
                if let Some(pkg_path) = found {
                    let ed = self.editor(&pkg_path)?;
                    if let Ok(i) = ed.find(&pv) {
                        let matched = crate::anim_ops::mesh_candidates(db, &ed.pkg, &ctx.binding, Some(&pv)).into_iter().find(|c| c.0 == i).map(|c| c.1).unwrap_or(0);
                        out.insert(0, json!({ "path": pkg_path, "index": i, "object": pv, "matched": matched, "preview": true }));
                    }
                }
            }
        }
        Ok(out)
    }

    /// The mesh to use: `mesh: {path, index}` or the best candidate.
    fn anim_mesh(&mut self, p: &J, path: &str, ctx: &AnimContext) -> Res<(String, usize)> {
        if let (Some(mp), Some(mi)) = (p["mesh"]["path"].as_str(), p["mesh"]["index"].as_u64()) {
            return Ok((mp.to_string(), mi as usize));
        }
        let m = self.anim_meshes(path, ctx)?;
        let first = m.first().ok_or("no skeletal mesh found for this animation's bones; pick one")?;
        Ok((first["path"].as_str().unwrap_or(path).to_string(), first["index"].as_u64().unwrap_or(0) as usize))
    }

    fn notifies_json(&mut self, path: &str, i: usize) -> Res<Vec<J>> {
        let ed = self.editor(path)?;
        let o = ed.object(i)?.clone();
        let pkg = &ed.pkg;
        let mut out = Vec::new();
        if let Some(Value::Array(items)) = o.property("Notifies", pkg).map(|p| &p.value) {
            for (k, v) in items.iter().enumerate() {
                let Value::Struct(crate::value::StructValue::Tagged(ps)) = v else { continue };
                let field = |n: &str| ps.iter().find(|p| pkg.fname_to_string(p.name).eq_ignore_ascii_case(n)).map(|p| &p.value);
                let time = if let Some(Value::Float(f)) = field("Time") { *f } else { 0.0 };
                let duration = if let Some(Value::Float(f)) = field("Duration") { *f } else { 0.0 };
                let notify = match field("Notify") {
                    Some(Value::Object(idx)) => json::reference(pkg, *idx),
                    _ => json::reference(pkg, PackageIndex::NONE),
                };
                let comment = if let Some(Value::Name(n)) = field("Comment") { pkg.fname_to_string(*n) } else { String::new() };
                out.push(json!({ "index": k, "time": time, "duration": duration, "notify": notify, "comment": comment }));
            }
        }
        Ok(out)
    }

    /// `anim.info {path, index}`.
    fn anim_info(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let i = self.anim_target(&path, p)?;
        let ctx = self.anim_context(&path, i)?;
        let notifies = self.notifies_json(&path, i)?;
        let meshes = self.anim_meshes(&path, &ctx)?;
        let mut sequences = Vec::new();
        for k in self.set_sequences(&path, ctx.set)? {
            sequences.push(json!({ "index": k, "name": self.sequence_name(&path, k)? }));
        }
        let ed = self.editor(&path)?;
        let o = ed.object(i)?.clone();
        let rate = match o.property("RateScale", &ed.pkg).map(|p| &p.value) {
            Some(Value::Float(f)) => *f,
            _ => 1.0,
        };
        let keys: usize = ctx.anim.tracks.iter().map(|t| t.rot.len() + t.trans.len()).sum();
        let a = &ctx.anim;
        Ok(json!({
            "index": i,
            "numFrames": a.num_frames,
            "length": a.length,
            "fps": if a.num_frames > 1 && a.length > 0.0 { (a.num_frames - 1) as f32 / a.length } else { 30.0 },
            "rateScale": rate,
            "keyFormat": format!("{:?}", ctx.header.key_format),
            "rotationFormat": format!("{:?}", ctx.header.rot_format),
            "translationFormat": format!("{:?}", ctx.header.trans_format),
            "tracks": a.tracks.len(),
            "keys": keys,
            "bytes": crate::anim::stream(&o).map(|s| s.len()).unwrap_or(0),
            "animSet": { "index": ctx.set, "path": ed.pkg.object_path(PackageIndex::export(ctx.set)), "preview": ctx.preview },
            "notifies": notifies,
            "meshes": meshes,
            "sequences": sequences,
        }))
    }

    /// The mesh's geometry (with textures for previews and exports) and the sequence as a glTF animation.
    /// The sequences of an AnimSet: its `Sequences` that are exports of the
    /// package, else the AnimSequences inside it.
    fn set_sequences(&mut self, path: &str, set: usize) -> Res<Vec<usize>> {
        let ed = self.editor(path)?;
        ed.flush();
        let o = ed.object(set)?.clone();
        let pkg = &ed.pkg;
        let is_seq = |i: usize| pkg.class_name_of(PackageIndex::export(i)).eq_ignore_ascii_case("AnimSequence");
        let mut out: Vec<usize> = match o.property("Sequences", pkg).map(|p| &p.value) {
            Some(Value::Array(items)) => items.iter().filter_map(|v| if let Value::Object(idx) = v { idx.export_index() } else { None }).filter(|&i| is_seq(i)).collect(),
            _ => Vec::new(),
        };
        if out.is_empty() {
            out = (0..pkg.exports.len()).filter(|&i| pkg.exports[i].outer.export_index() == Some(set) && is_seq(i)).collect();
        }
        Ok(out)
    }

    /// A sequence's `SequenceName` (what the game plays it by), else its object name.
    fn sequence_name(&mut self, path: &str, i: usize) -> Res<String> {
        let ed = self.editor(path)?;
        let o = ed.object(i)?.clone();
        Ok(match o.property("SequenceName", &ed.pkg).map(|p| &p.value) {
            Some(Value::Name(n)) if !ed.pkg.fname_to_string(*n).eq_ignore_ascii_case("None") => ed.pkg.fname_to_string(*n),
            _ => ed.pkg.object_name(PackageIndex::export(i)),
        })
    }

    /// The sequence an `anim.*` call works on: `index` itself, or the first
    /// sequence of `index` when it is an AnimSet.
    fn anim_target(&mut self, path: &str, p: &J) -> Res<usize> {
        let i = self.export_of(path, p)?;
        if self.editor(path)?.pkg.class_name_of(PackageIndex::export(i)).eq_ignore_ascii_case("AnimSet") {
            return self.set_sequences(path, i)?.first().copied().ok_or_else(|| "this AnimSet has no sequences".into());
        }
        Ok(i)
    }

    /// The mesh's geometry (with textures for previews and exports) and the
    /// sequence as a glTF animation; with `all` (or an AnimSet as `index`)
    /// every sequence of its AnimSet, named by `SequenceName`.
    fn anim_gltf(&mut self, p: &J, preview: bool) -> Res<(String, crate::mesh_ops::Geometry, Vec<crate::mesh_ops::GltfAnim>, J)> {
        let path = s(p, "path")?.to_string();
        let given = self.export_of(&path, p)?;
        let i = self.anim_target(&path, p)?;
        let ctx = self.anim_context(&path, i)?;
        let (mesh_path, mesh_i) = self.anim_mesh(p, &path, &ctx)?;
        let mut mp = json!({ "path": mesh_path, "index": mesh_i, "textures": opt_b(p, "textures").unwrap_or(true) });
        if let Some(ts) = p.get("textureSize") {
            mp["textureSize"] = ts.clone();
        }
        let (mesh_name, g, warnings) = self.mesh_geometry(&mp, preview)?;
        if g.bones.is_empty() {
            return Err("that mesh has no skeleton".into());
        }
        let all = given != i || opt_b(p, "all").unwrap_or(false);
        let name = self.editor(&path)?.pkg.object_name(PackageIndex::export(if all { ctx.set } else { i }));
        let map = crate::anim_ops::track_to_bone(&ctx.binding, &g.bones);
        let missing: Vec<&String> = ctx.binding.track_bones.iter().zip(&map).filter(|(_, m)| m.is_none()).map(|(n, _)| n).collect();
        let seqs = if all { self.set_sequences(&path, ctx.set)? } else { vec![i] };
        let mut anims = Vec::new();
        let mut names = Vec::new();
        for k in seqs {
            let anim = if k == i { ctx.anim.clone() } else { self.anim_context(&path, k)?.anim };
            let mut n = self.sequence_name(&path, k)?;
            // Animation names must be unique in a file.
            if names.contains(&n.to_ascii_lowercase()) {
                n = self.editor(&path)?.pkg.object_name(PackageIndex::export(k));
            }
            names.push(n.to_ascii_lowercase());
            anims.push(crate::anim_ops::gltf_anim(&n, &anim, &ctx.binding, &g.bones));
        }
        let info = json!({ "mesh": { "path": mesh_path, "index": mesh_i, "name": mesh_name }, "tracksWithoutBone": missing, "warnings": warnings, "animations": anims.len() });
        Ok((name, g, anims, info))
    }

    /// `anim.preview {path, index, mesh?, textureSize?}`: a GLB of the mesh playing the sequence.
    fn anim_preview(&mut self, p: &J) -> Res<J> {
        let (name, g, anims, mut info) = self.anim_gltf(p, true)?;
        let bytes = crate::mesh_ops::write_glb_anims(&name, &g, &anims)?;
        info["glb"] = self.bins.add(bytes);
        Ok(info)
    }

    /// `anim.export {path, index, out, mesh?, all?}`: `.glb` or `.fbx` with the
    /// mesh and the animation (every sequence of the AnimSet with `all`, or
    /// when `index` is the AnimSet).
    fn anim_export(&mut self, p: &J) -> Res<J> {
        let out = s(p, "out")?.to_string();
        let (name, g, anims, mut info) = self.anim_gltf(p, false)?;
        let bytes = if out.to_ascii_lowercase().ends_with(".fbx") { crate::mesh_fbx::write_fbx_anims(&name, &g, &anims)? } else { crate::mesh_ops::write_glb_anims(&name, &g, &anims)? };
        std::fs::write(&out, &bytes)?;
        info["out"] = J::from(out);
        info["bytes"] = J::from(bytes.len());
        Ok(info)
    }

    /// `anim.files {file}`: the animations in a `.glb` / `.gltf` / `.fbx`.
    fn anim_files(&mut self, p: &J) -> Res<J> {
        let file = s(p, "file")?;
        let bytes = std::fs::read(file)?;
        let names = if file.to_ascii_lowercase().ends_with(".fbx") { crate::mesh_fbx::fbx_animation_names(&bytes)? } else { crate::anim_ops::gltf_animation_names(Path::new(file), &bytes)? };
        Ok(json!({ "animations": names }))
    }

    /// Scale or shift every notify time: `t → (t - shift) * scale`, clamped to `[0, length]`.
    fn anim_move_notifies(&mut self, path: &str, i: usize, shift: f32, scale: f32, length: f32) -> Res<()> {
        let ed = self.editor(path)?;
        let pkg = ed.pkg.clone();
        let mut o = ed.object(i)?.clone();
        if let Some(prop) = o.properties.iter_mut().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case("Notifies")) {
            if let Value::Array(items) = &mut prop.value {
                for v in items.iter_mut() {
                    if let Value::Struct(crate::value::StructValue::Tagged(ps)) = v {
                        if let Some(t) = ps.iter_mut().find(|p| pkg.fname_to_string(p.name).eq_ignore_ascii_case("Time")) {
                            if let Value::Float(f) = &mut t.value {
                                *f = ((*f - shift) * scale).clamp(0.0, length.max(0.0));
                            }
                        }
                    }
                }
            }
        }
        ed.replace_object(i, o)?;
        Ok(())
    }

    /// Keep `Notifies` in time order (the engine walks them in order).
    fn anim_sort_notifies(&mut self, path: &str, i: usize) -> Res<()> {
        let ed = self.editor(path)?;
        let pkg = ed.pkg.clone();
        let mut o = ed.object(i)?.clone();
        let time_of = |v: &Value| -> f32 {
            if let Value::Struct(crate::value::StructValue::Tagged(ps)) = v {
                if let Some(Value::Float(f)) = ps.iter().find(|p| pkg.fname_to_string(p.name).eq_ignore_ascii_case("Time")).map(|p| &p.value) {
                    return *f;
                }
            }
            0.0
        };
        if let Some(prop) = o.properties.iter_mut().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case("Notifies")) {
            if let Value::Array(items) = &mut prop.value {
                items.sort_by(|a, b| time_of(a).partial_cmp(&time_of(b)).unwrap_or(std::cmp::Ordering::Equal));
            }
        }
        ed.replace_object(i, o)?;
        Ok(())
    }

    /// `anim.import {path, index, file, mesh?, animation?, fps?, all?}`: replace
    /// the sequence's motion from a glTF / FBX file (bones matched by name;
    /// tracks whose bone the mesh lacks keep their keys, time-scaled); notify
    /// times are scaled to the new length. With `all` (or an AnimSet as
    /// `index`) every animation of the file replaces the sequence of the set
    /// with its name (`SequenceName`, else the object name; a `Armature|`
    /// prefix is ignored), as one undo step; `unmatched` lists the file's
    /// other animations, `untouched` the set's other sequences.
    fn anim_import(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let file = s(p, "file")?.to_string();
        let given = self.export_of(&path, p)?;
        let i = self.anim_target(&path, p)?;
        let ctx = self.anim_context(&path, i)?;
        let (mesh_path, mesh_i) = self.anim_mesh(p, &path, &ctx)?;
        let (_, g, _) = self.mesh_geometry(&json!({ "path": mesh_path, "index": mesh_i, "textures": false }), true)?;
        if g.bones.is_empty() {
            return Err("that mesh has no skeleton".into());
        }
        let bytes = std::fs::read(&file)?;
        let fbx = file.to_ascii_lowercase().ends_with(".fbx");
        let all = given != i || opt_b(p, "all").unwrap_or(false);
        // Which file animation goes into which sequence.
        let mut plan: Vec<(usize, Option<usize>)> = Vec::new();
        let mut unmatched: Vec<String> = Vec::new();
        let mut untouched: Vec<String> = Vec::new();
        if all {
            let names = if fbx { crate::mesh_fbx::fbx_animation_names(&bytes)? } else { crate::anim_ops::gltf_animation_names(Path::new(&file), &bytes)? };
            let seqs = self.set_sequences(&path, ctx.set)?;
            let mut keys = Vec::new();
            for &k in &seqs {
                let seq = self.sequence_name(&path, k)?.to_ascii_lowercase();
                let obj = self.editor(&path)?.pkg.object_name(PackageIndex::export(k)).to_ascii_lowercase();
                keys.push((k, seq, obj));
            }
            // Blender and other tools may prefix the armature name (`Armature|emote`).
            let leaf = |n: &str| n.rsplit('|').next().unwrap_or(n).trim().to_ascii_lowercase();
            let mut used = Vec::new();
            for (a, n) in names.iter().enumerate() {
                let l = leaf(n);
                let hit = keys.iter().find(|(k, seq, _)| !used.contains(k) && *seq == l).or_else(|| keys.iter().find(|(k, _, obj)| !used.contains(k) && *obj == l));
                match hit {
                    Some((k, _, _)) => {
                        used.push(*k);
                        plan.push((*k, Some(a)));
                    }
                    None => unmatched.push(n.clone()),
                }
            }
            for (k, seq, _) in &keys {
                if !used.contains(k) {
                    untouched.push(seq.clone());
                }
            }
            if plan.is_empty() {
                return Err(format!("no animation of the file is named like a sequence of this AnimSet (file: {}; sequences: {})", names.join(", "), keys.iter().map(|k| k.1.as_str()).collect::<Vec<_>>().join(", ")).into());
            }
        } else {
            plan.push((i, opt_u(p, "animation")));
        }
        let set_name = self.editor(&path)?.pkg.object_name(PackageIndex::export(if all { ctx.set } else { i }));
        self.editor(&path)?.begin_undo(format!("import animation into {set_name}"));
        let mut replaced = Vec::new();
        let mut bones_missing: Vec<String> = Vec::new();
        let mut matched = 0;
        for (k, which) in plan {
            let old = if k == i { ctx.anim.clone() } else { self.anim_context(&path, k)?.anim };
            let fps = p["fps"].as_f64().map(|f| f as f32).unwrap_or(if old.num_frames > 1 && old.length > 0.0 { (old.num_frames - 1) as f32 / old.length } else { 30.0 });
            let fa = if fbx { crate::mesh_fbx::read_fbx_anim(&bytes, which, fps)? } else { crate::anim_ops::read_gltf(Path::new(&file), &bytes, which, fps)? };
            let (frames, missing) = crate::anim_ops::retarget(&fa, &g.bones);
            matched = g.bones.len() - missing.len();
            if matched == 0 {
                return Err(format!("none of the bones of animation {} are named like the mesh's", fa.name).into());
            }
            for m in missing {
                if !bones_missing.contains(&m) {
                    bones_missing.push(m);
                }
            }
            let tracks = crate::anim_ops::tracks_from_locals(&frames, &ctx.binding, &g.bones, Some(&old));
            let new = crate::anim::Anim { num_frames: frames.len(), length: fa.duration, tracks };
            crate::anim_ops::write_sequence(self.editor(&path)?, k, &new)?;
            if old.length > 0.0 && (old.length - new.length).abs() > 1e-4 {
                self.anim_move_notifies(&path, k, 0.0, new.length / old.length, new.length)?;
            }
            replaced.push(json!({ "index": k, "sequence": self.sequence_name(&path, k)?, "animation": fa.name, "numFrames": new.num_frames, "length": new.length }));
        }
        let first = &replaced[0];
        Ok(json!({
            "numFrames": first["numFrames"], "length": first["length"], "animation": first["animation"],
            "bonesMatched": matched, "bonesMissing": bones_missing,
            "replaced": replaced, "unmatched": unmatched, "untouched": untouched,
        }))
    }

    /// `anim.edit {path, index, op, ...}`: `trim {start, end}` (frames, both
    /// kept), `retime {length}` (seconds), `notifyMove {notify, time}`,
    /// `notifyRemove {notify}`, `notifyAdd {notify, time}` (a copy of notify
    /// `notify`, its object duplicated, at `time`).
    fn anim_edit(&mut self, p: &J) -> Res<J> {
        let path = s(p, "path")?.to_string();
        let i = self.export_of(&path, p)?;
        let op = s(p, "op")?.to_string();
        let name = self.editor(&path)?.pkg.object_name(PackageIndex::export(i));
        match op.as_str() {
            "trim" => {
                let ctx = self.anim_context(&path, i)?;
                let a = &ctx.anim;
                let last = a.num_frames.saturating_sub(1);
                let start = opt_u(p, "start").unwrap_or(0).min(last);
                let end = opt_u(p, "end").unwrap_or(last).clamp(start + 1, last.max(start + 1));
                let step = if a.num_frames > 1 { a.length / (a.num_frames - 1) as f32 } else { 0.0 };
                let tracks = a.tracks.iter().map(|t| {
                    let frames = start..=end;
                    let rot: Vec<[f32; 4]> = if t.rot.is_empty() { Vec::new() } else { frames.clone().map(|f| t.sample(f as f32).0.unwrap_or([0.0, 0.0, 0.0, 1.0])).collect() };
                    let trans: Vec<[f32; 3]> = if t.trans.is_empty() { Vec::new() } else { frames.map(|f| t.sample(f as f32).1.unwrap_or([0.0; 3])).collect() };
                    crate::anim::reduce(&rot, &trans, crate::anim::ROT_TOLERANCE, crate::anim::TRANS_TOLERANCE)
                }).collect();
                let new = crate::anim::Anim { num_frames: end - start + 1, length: (end - start) as f32 * step, tracks };
                let ed = self.editor(&path)?;
                ed.begin_undo(format!("trim {name}"));
                crate::anim_ops::write_sequence(ed, i, &new)?;
                self.anim_move_notifies(&path, i, start as f32 * step, 1.0, new.length)?;
            }
            "retime" => {
                let length = p["length"].as_f64().ok_or("missing length")? as f32;
                if length <= 0.0 {
                    return Err("the length must be positive".into());
                }
                let mut ctx = self.anim_context(&path, i)?;
                let old = ctx.anim.length;
                ctx.anim.length = length;
                let ed = self.editor(&path)?;
                ed.begin_undo(format!("retime {name}"));
                crate::anim_ops::write_sequence(ed, i, &ctx.anim)?;
                if old > 0.0 {
                    self.anim_move_notifies(&path, i, 0.0, length / old, length)?;
                }
            }
            "notifyMove" => {
                let k = opt_u(p, "notify").ok_or("missing notify")?;
                let t = p["time"].as_f64().ok_or("missing time")?;
                let ed = self.editor(&path)?;
                ed.begin_undo(format!("move notify of {name}"));
                ed.set(i, &format!("Notifies[{k}].Time"), &format!("{t}"))?;
                if let Some(d) = p["duration"].as_f64() {
                    ed.set(i, &format!("Notifies[{k}].Duration"), &format!("{d}"))?;
                }
                self.anim_sort_notifies(&path, i)?;
            }
            "notifyRemove" => {
                let k = opt_u(p, "notify").ok_or("missing notify")?;
                let ed = self.editor(&path)?;
                ed.begin_undo(format!("remove notify of {name}"));
                ed.edit_list(i, &format!("Notifies[{k}]"), crate::edit::ListOp::Remove(k))?;
            }
            "notifyAdd" => {
                let k = opt_u(p, "notify").ok_or("missing notify")?;
                let t = p["time"].as_f64().ok_or("missing time")? as f32;
                let ed = self.editor(&path)?;
                ed.begin_undo(format!("add notify to {name}"));
                let pkg = ed.pkg.clone();
                let o = ed.object(i)?.clone();
                let Some(Value::Array(items)) = o.property("Notifies", &pkg).map(|p| p.value.clone()) else { return Err("the sequence has no notifies to copy".into()) };
                let mut item = items.get(k).cloned().ok_or("no such notify")?;
                // A copy of the notify object, so the two can be edited apart.
                let mut copy_of: Option<usize> = None;
                if let Value::Struct(crate::value::StructValue::Tagged(ps)) = &item {
                    if let Some(Value::Object(idx)) = ps.iter().find(|p| pkg.fname_to_string(p.name).eq_ignore_ascii_case("Notify")).map(|p| &p.value) {
                        copy_of = idx.export_index();
                    }
                }
                let new_obj = match copy_of {
                    Some(src) => Some(ed.duplicate(src)?),
                    None => None,
                };
                let pkg = ed.pkg.clone();
                if let Value::Struct(crate::value::StructValue::Tagged(ps)) = &mut item {
                    for prop in ps.iter_mut() {
                        let n = pkg.fname_to_string(prop.name);
                        if n.eq_ignore_ascii_case("Time") {
                            prop.value = Value::Float(t);
                        } else if n.eq_ignore_ascii_case("Notify") {
                            if let Some(no) = new_obj {
                                prop.value = Value::Object(PackageIndex::export(no));
                            }
                        }
                    }
                }
                let mut o = ed.object(i)?.clone();
                if let Some(prop) = o.properties.iter_mut().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case("Notifies")) {
                    if let Value::Array(list) = &mut prop.value {
                        list.push(item);
                    }
                }
                ed.replace_object(i, o)?;
                self.anim_sort_notifies(&path, i)?;
            }
            other => return Err(format!("unknown animation edit {other}").into()),
        }
        self.editor(&path)?.mark_dirty(i);
        self.anim_info(p)
    }
}
