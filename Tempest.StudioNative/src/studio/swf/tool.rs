//! The movie editor for agents: the MCP tool `studio_swf`, the same actions
//! as the Electron Studio's. It drives the editor the user sees (the movie is
//! selected, so the window follows), edits stay pending until `apply`. A call
//! runs in phases (`crate::mcp`): `studio_swf_open` selects the movie and
//! lets it load, `studio_swf` goes to the place and acts, then, once nothing
//! is in flight, the answer is read: the editor's state (`studio_swf_read`),
//! a backend call or a PNG rendered by Ruffle, as `studio_swf` asked.

use gpui::{Context, Entity, Window};
use serde_json::{json, Value as J};

use super::geom::{compose, decompose, M};
use super::{is_movie, MovieEditor, Node};
use crate::studio::Studio;

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// A page of `items`, filtered by `query` (anywhere in the JSON).
fn page(items: Vec<J>, a: &J, limit: usize) -> J {
    let q = a["query"].as_str().map(|q| q.to_ascii_lowercase());
    let hits: Vec<J> = items.into_iter().filter(|x| q.as_ref().is_none_or(|q| x.to_string().to_ascii_lowercase().contains(q))).collect();
    let offset = a["offset"].as_u64().unwrap_or(0) as usize;
    let n = a["limit"].as_u64().map(|n| n as usize).unwrap_or(limit);
    json!({ "total": hits.len(), "offset": offset, "items": hits.into_iter().skip(offset).take(n).collect::<Vec<_>>() })
}

impl MovieEditor {
    fn entry_json(&self, e: &J) -> J {
        let d = decompose(&M::from_json(&e["matrix"]));
        let id = e["character"].as_u64().unwrap_or(0);
        let depth = e["depth"].as_u64().unwrap_or(0);
        json!({
            "depth": depth, "character": id, "label": self.label(id), "kind": self.char(id).map(|c| c["kind"].clone()),
            "name": e["name"], "className": e["className"],
            "x": r3(d.x), "y": r3(d.y), "scaleX": r3(d.sx), "scaleY": r3(d.sy), "rotation": r3(d.rot), "skew": r3(d.skew),
            "alpha": r3(e["cxform"]["mul"][3].as_f64().unwrap_or(1.0) * 100.0), "cxform": e["cxform"], "rect": e["rect"],
            "clipDepth": e["clipDepth"], "since": e["since"], "extras": e["extras"],
            "tags": { "placedBy": e["placedBy"], "matrixBy": e["matrixBy"], "changedBy": e["changedBy"] },
            "edited": self.entry_edited(e), "hidden": self.hidden().contains(&depth),
        })
    }

    fn tool_status(&self) -> J {
        let info = &self.info;
        let n = |k: &str| info[k].as_array().map(|a| a.len()).unwrap_or(0);
        json!({
            "movie": {
                "path": self.path, "index": self.index, "object": self.name,
                "size": { "width": info["width"], "height": info["height"] }, "frameRate": info["frameRate"], "frames": info["frames"], "version": info["version"],
                "characters": self.chars.len(), "texts": n("texts"), "scripts": n("scripts"), "images": n("images"),
            },
            "place": {
                "node": self.node.key(), "sprite": self.sprite(),
                "trail": self.trail().iter().map(|(id, l)| format!("{l} ({id})")).collect::<Vec<_>>().join(" › "),
                "character": self.character().map(|c| json!({ "id": c["id"], "kind": c["kind"], "label": self.label(c["id"].as_u64().unwrap_or(0)) })),
                "frame": self.frame, "frames": self.sprite().map(|_| self.frames()), "objects": self.entries().len(),
                "selected": self.selected_entry().map(|e| self.entry_json(&e)),
            },
            "pending": self.pending(),
            "hiddenLayers": self.hidden.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<String, J>>(),
        })
    }

    fn entry_at(&self, depth: Option<u64>) -> Result<J, String> {
        let sprite = self.sprite().ok_or("open a sprite or the root timeline first (sprite)")?;
        let depth = depth.ok_or("give depth (action \"timeline\" lists them)")?;
        let list = self.entries();
        list.iter().find(|e| e["depth"].as_u64() == Some(depth)).cloned().ok_or_else(|| {
            let label = if sprite == 0 { "Root".to_string() } else { self.label(sprite) };
            format!("nothing at depth {depth} on frame {} of {label}; depths: {}", self.frame, list.iter().map(|e| e["depth"].to_string()).collect::<Vec<_>>().join(", "))
        })
    }

    /// The character an action edits, with its defining tag.
    fn defined(&self, a: &J, want: &str) -> Result<(u64, u64), String> {
        let id = a["character"].as_u64().ok_or("give character (action tree lists them)")?;
        let c = self.char(id).ok_or_else(|| format!("no character {id}"))?;
        if c["kind"] != want {
            return Err(format!("character {id} is a {}, not a {want}", c["kind"].as_str().unwrap_or("?")));
        }
        let tag = c["tag"].as_u64().ok_or_else(|| format!("character {id} is imported from another movie"))?;
        Ok((id, tag))
    }

    /// Go to `sprite` / `character` / `node`, then `frame` (and `depth` for `open`).
    fn tool_go(&mut self, a: &J, cx: &mut Context<Self>) -> Result<(), String> {
        let action = a["action"].as_str().unwrap_or("");
        if let Some(s) = a["sprite"].as_u64() {
            if s != 0 && self.kind_of(s) != "sprite" {
                return Err(format!("character {s} is not a sprite"));
            }
            self.open(if s == 0 { Node::Frames } else { Node::Char(s) }, cx);
        } else if let (Some(c), false) = (a["character"].as_u64(), matches!(action, "character" | "export" | "shapeStyle" | "shapeSvg" | "editText" | "staticText" | "addGlyphs" | "replaceImage" | "exportImage")) {
            if self.char(c).is_none() {
                return Err(format!("no character {c}"));
            }
            self.open(Node::Char(c), cx);
        } else if let Some(n) = a["node"].as_str() {
            self.open(Node::from_key(n).ok_or_else(|| format!("unknown node {n}"))?, cx);
        }
        if let Some(f) = a["frame"].as_u64() {
            self.frame = f as usize;
        }
        if action == "open" {
            if let Some(d) = a["depth"].as_u64() {
                self.depth = Some(d);
            }
        }
        cx.notify();
        Ok(())
    }

    /// Go and act; the answer is final, or says what to read once the edits settled.
    pub fn tool_act(&mut self, a: &J, window: &mut Window, cx: &mut Context<Self>) -> Result<J, String> {
        let action = a["action"].as_str().unwrap_or("");
        self.tool_go(a, cx)?;
        let read = |what: &str| Ok(json!({ "$read": what }));
        let at = json!({ "path": self.path, "index": self.index });
        match action {
            "open" | "status" => read("status"),
            "tree" => read("tree"),
            "timeline" => read("timeline"),
            "info" => read("info"),
            "script" | "setScript" => {
                let scripts = self.info["scripts"].as_array().cloned().unwrap_or_default();
                let sc = match (a["key"].as_str(), a["tag"].as_u64()) {
                    (Some(k), _) => scripts.iter().find(|s| s["key"] == k),
                    (None, Some(t)) => scripts.iter().find(|s| s["owner"].as_u64() == Some(0) && s["tag"].as_u64() == Some(t) && s["loc"]["event"].is_null()),
                    _ => None,
                }
                .cloned()
                .ok_or("give key (info section scripts lists them) or a root tag")?;
                self.open_script(sc["key"].as_str().unwrap_or("").to_string(), cx);
                if action == "setScript" {
                    // `crate::mcp` compiled it already.
                    let (text, pcode) = match (a["pcode"].as_str(), a["source"].as_str()) {
                        (Some(p), _) => (p.to_string(), true),
                        (None, Some(s)) => (s.to_string(), false),
                        _ => return Err("give source (ActionScript) or pcode".into()),
                    };
                    self.set_script(&sc["loc"], text, pcode, cx);
                    return Ok(json!({ "$read": "pending", "key": sc["key"], "bytes": a["$bytes"] }));
                }
                let mut params = at.clone();
                params["loc"] = sc["loc"].clone();
                params["edits"] = self.pending();
                Ok(json!({ "$backend": { "method": "swf.script", "params": params }, "$extra": { "key": sc["key"], "kind": sc["kind"], "staged": self.script_edit(&sc["loc"]).is_some() } }))
            }
            "edit" => {
                let e = self.entry_at(a["depth"].as_u64())?;
                let sprite = self.sprite().unwrap_or(0);
                self.depth = e["depth"].as_u64();
                let has = |k: &str| a[k].is_number();
                if a["matrix"].is_object() {
                    let mut m = e["matrix"].clone();
                    for (k, v) in a["matrix"].as_object().into_iter().flatten() {
                        m[k] = v.clone();
                    }
                    self.set_matrix(&e, M::from_json(&m), cx);
                } else if ["x", "y", "dx", "dy", "scaleX", "scaleY", "rotation", "skew"].iter().any(|k| has(k)) {
                    let mut d = decompose(&M::from_json(&e["matrix"]));
                    let f = |k: &str| a[k].as_f64();
                    d.x = f("x").unwrap_or(d.x) + f("dx").unwrap_or(0.0);
                    d.y = f("y").unwrap_or(d.y) + f("dy").unwrap_or(0.0);
                    d.sx = f("scaleX").unwrap_or(d.sx);
                    d.sy = f("scaleY").unwrap_or(d.sy);
                    d.rot = f("rotation").unwrap_or(d.rot);
                    d.skew = f("skew").unwrap_or(d.skew);
                    self.set_matrix(&e, compose(&d), cx);
                }
                if let Some(al) = a["alpha"].as_f64() {
                    self.set_alpha(&e, al, cx);
                }
                if a.get("cxform").is_some() {
                    let tag = e["changedBy"].as_u64().unwrap_or(0);
                    self.edit_place(sprite, tag, json!({ "cxform": a["cxform"] }), cx);
                }
                if let Some(n) = a["name"].as_str() {
                    let tag = e["placedBy"].as_u64().unwrap_or(0);
                    let n = n.trim();
                    self.edit_place(sprite, tag, json!({ "name": if n.is_empty() { J::Null } else { J::from(n) } }), cx);
                }
                Ok(json!({ "$read": "object", "depth": e["depth"] }))
            }
            "remove" => {
                let e = self.entry_at(a["depth"].as_u64())?;
                self.remove_entry(&e, cx);
                read("status")
            }
            "revert" => {
                let e = self.entry_at(a["depth"].as_u64())?;
                self.revert_entry(&e, cx);
                Ok(json!({ "$read": "object", "depth": e["depth"] }))
            }
            "text" => {
                let kind = a["kind"].as_str().unwrap_or("text");
                if !matches!(kind, "text" | "string" | "image") {
                    return Err("kind is text (edit-text field), string (script string) or image (external image file name)".into());
                }
                let (Some(tag), Some(text)) = (a["tag"].as_u64(), a["text"].as_str()) else { return Err("give tag and text".into()) };
                if kind == "string" && a["index"].is_null() {
                    return Err("a script string needs its index".into());
                }
                let mut e = json!({ "kind": kind, "tag": tag, "text": text });
                if kind == "string" {
                    e["index"] = a["index"].clone();
                }
                self.set_edit(e, cx);
                read("pending")
            }
            "layers" => {
                self.sprite().ok_or("open a sprite or the root timeline (sprite)")?;
                let list = |k: &str| a[k].as_array().map(|v| v.iter().filter_map(|x| x.as_u64()).collect::<Vec<_>>());
                if a["showAll"].as_bool() == Some(true) {
                    let h: Vec<u64> = self.hidden().into_iter().collect();
                    self.set_hidden(&h, false, cx);
                }
                if let Some(solo) = list("solo") {
                    let others: Vec<u64> = self.entries().iter().filter_map(|e| e["depth"].as_u64()).filter(|d| !solo.contains(d)).collect();
                    self.set_hidden(&others, true, cx);
                    self.set_hidden(&solo, false, cx);
                }
                if let Some(h) = list("hide") {
                    self.set_hidden(&h, true, cx);
                }
                if let Some(s) = list("show") {
                    self.set_hidden(&s, false, cx);
                }
                Ok(json!({ "sprite": self.sprite(), "hidden": self.hidden().into_iter().collect::<Vec<_>>() }))
            }
            "apply" => {
                let n = self.edits.len();
                if n == 0 {
                    return Ok(json!({ "applied": 0, "note": "nothing is pending" }));
                }
                self.apply(cx);
                Ok(json!({ "$read": "applied", "applied": n }))
            }
            "discard" => {
                self.discard(cx);
                read("status")
            }
            "addImage" => {
                let file = a["file"].as_str().ok_or("give file (a PNG or DDS)")?.to_string();
                self.open_add_image(window, cx);
                if let Some(ai) = &mut self.adding_image {
                    ai.file = Some(file);
                    ai.shape = a["shape"].as_bool().unwrap_or(true);
                    ai.place = a["place"].as_bool().unwrap_or(true);
                    let set = |i: &gpui::Entity<gpui_component::input::InputState>, v: String, window: &mut Window, cx: &mut Context<Self>| i.update(cx, |s, cx| s.set_value(v, window, cx));
                    let (name, inst, x, y) = (ai.name.clone(), ai.instance.clone(), ai.x.clone(), ai.y.clone());
                    set(&name, a["texture"].as_str().unwrap_or("").to_string(), window, cx);
                    set(&inst, a["name"].as_str().unwrap_or("").to_string(), window, cx);
                    set(&x, a["x"].as_f64().unwrap_or(0.0).to_string(), window, cx);
                    set(&y, a["y"].as_f64().unwrap_or(0.0).to_string(), window, cx);
                }
                self.add_image(cx);
                Ok(json!({ "$read": "status", "note": "the package is changed in memory: save it (studio_run_command file.save) or build the mod" }))
            }
            "replaceImage" | "exportImage" => {
                let image = match (a["tag"].as_u64(), a["character"].as_u64()) {
                    (Some(t), _) => self.info["images"].as_array().into_iter().flatten().find(|i| i["tag"].as_u64() == Some(t)).cloned(),
                    (None, Some(c)) => self.image_of(c),
                    _ => None,
                }
                .ok_or("give the image's character or tag (info section images)")?;
                let file_name = self.value("image", image["tag"].as_u64().unwrap_or(0), image["fileName"].as_str().unwrap_or(""));
                let texture = self.texture_for(&file_name, cx).ok_or_else(|| format!("no Texture2D for {file_name} in the package"))?;
                let file = a["file"].as_str().ok_or("give file")?;
                let mut params = at.clone();
                params["index"] = json!(texture);
                if action == "exportImage" {
                    params["out"] = json!(file);
                    return Ok(json!({ "$backend": { "method": "texture.export", "params": params } }));
                }
                params["file"] = json!(file);
                params["keepAlpha"] = json!(true);
                params["padBlocks"] = json!(true);
                self.thumbs.clear();
                self.stage.invalidate();
                self.mark_dirty(false, cx);
                Ok(json!({ "$backend": { "method": "texture.import", "params": params }, "$extra": { "drawnAt": { "width": image["width"], "height": image["height"] } } }))
            }
            "character" => {
                let id = a["character"].as_u64().ok_or("give character (action tree lists them)")?;
                self.open(Node::Char(id), cx);
                let mut params = at.clone();
                params["id"] = json!(id);
                params["edits"] = self.pending();
                Ok(json!({ "$backend": { "method": "swf.character", "params": params }, "$extra": { "id": id, "label": self.label(id) } }))
            }
            "shapeStyle" => {
                let (_, tag) = self.defined(a, "shape")?;
                let (Some(k), Some(v)) = (a["styleKey"].as_str(), a["value"].as_str()) else { return Err("give styleKey and value (action character lists the keys)".into()) };
                self.set_shape_style(tag, k, v.to_string(), cx);
                read("pending")
            }
            "shapeSvg" => {
                let (_, tag) = self.defined(a, "shape")?;
                let svg = match (a["svg"].as_str(), a["file"].as_str()) {
                    (Some(s), _) => s.to_string(),
                    (None, Some(f)) => std::fs::read_to_string(f).map_err(|e| e.to_string())?,
                    _ => return Err("give svg (the document) or file".into()),
                };
                self.replace_shape(tag, svg, a["fit"].as_bool().unwrap_or(true), cx);
                read("pending")
            }
            "editText" => {
                let (_, tag) = self.defined(a, "edittext")?;
                if !a["props"].is_object() {
                    return Err("give props (action character lists them)".into());
                }
                self.set_edit_text(tag, a["props"].clone(), cx);
                read("pending")
            }
            "staticText" => {
                let (_, tag) = self.defined(a, "text")?;
                let record = a["record"].as_u64().ok_or("give record and text and/or color")?;
                let mut change = json!({});
                if let Some(t) = a["text"].as_str() {
                    change["text"] = json!(t);
                }
                if let Some(c) = a["color"].as_str() {
                    change["color"] = json!(c);
                }
                if change.as_object().is_some_and(|o| o.is_empty()) {
                    return Err("give record and text and/or color".into());
                }
                self.set_static_text(tag, record, change, cx);
                read("pending")
            }
            "addGlyphs" => {
                let (_, tag) = self.defined(a, "font")?;
                let (Some(file), Some(chars)) = (a["file"].as_str(), a["chars"].as_str()) else { return Err("give file (TrueType/OpenType) and chars".into()) };
                self.set_edit(json!({ "kind": "fontGlyphs", "tag": tag, "file": file, "chars": chars }), cx);
                read("pending")
            }
            "export" => {
                let file = a["file"].as_str().ok_or("give file (where to write)")?;
                let format = a["format"].as_str().unwrap_or("svg");
                let mut params = at.clone();
                params["edits"] = self.pending();
                if matches!(format, "ttf" | "sound" | "binary") {
                    let id = a["character"].as_u64().ok_or_else(|| format!("{format} needs character"))?;
                    params["format"] = json!(format);
                    params["id"] = json!(id);
                    params["out"] = json!(file);
                    return Ok(json!({ "$backend": { "method": "swf.export", "params": params } }));
                }
                let frame = a["frame"].as_u64().unwrap_or(0);
                let target = match (a["character"].as_u64(), a["sprite"].as_u64()) {
                    (Some(c), _) => json!({ "kind": "character", "id": c }),
                    (None, Some(s)) if s != 0 => json!({ "kind": "frame", "id": s, "frame": frame }),
                    _ => json!({ "kind": "root", "frame": frame }),
                };
                params["target"] = target;
                if format == "svg" {
                    params["format"] = json!("svg");
                    params["out"] = json!(file);
                    return Ok(json!({ "$backend": { "method": "swf.export", "params": params } }));
                }
                Ok(json!({ "$png": { "params": params, "scale": a["scale"].as_u64().unwrap_or(1), "out": file } }))
            }
            a => Err(format!("unknown action {a}")),
        }
    }

    /// The answer `tool_act` asked for, read once the edits settled.
    pub fn tool_read(&self, what: &J, a: &J) -> Result<J, String> {
        Ok(match what["$read"].as_str().unwrap_or("status") {
            "status" => {
                let mut s = self.tool_status();
                if let Some(n) = what.get("note") {
                    s["note"] = n.clone();
                }
                s
            }
            "pending" => json!({ "pending": self.pending(), "key": what["key"], "bytes": what["bytes"] }),
            "object" => json!({ "object": self.entry_at(what["depth"].as_u64()).map(|e| self.entry_json(&e)).ok(), "pending": self.pending().as_array().map(|p| p.len()) }),
            "applied" => json!({ "applied": what["applied"], "pending": self.pending().as_array().map(|p| p.len()), "note": "the package is changed in memory: save it (studio_run_command file.save) or build the mod" }),
            "tree" => {
                let kind = a["kind"].as_str();
                let chars: Vec<J> = self.tree["characters"].as_array().into_iter().flatten().filter(|c| kind.is_none_or(|k| c["kind"] == k)).map(|c| json!({ "id": c["id"], "kind": c["kind"], "names": c["names"], "tag": c["tag"], "rect": c["rect"], "frames": c["frames"], "deps": c["deps"], "usedBy": c["usedBy"], "import": c["import"] })).collect();
                let mut out = json!({ "stage": self.tree["stage"], "rootFrames": self.tree["rootFrames"], "labels": self.tree["labels"] });
                for (k, v) in page(chars, a, 200).as_object().into_iter().flatten() {
                    out[k] = v.clone();
                }
                out
            }
            "timeline" => {
                let t = self.timeline.as_ref().filter(|_| self.sprite().is_some()).ok_or("open a sprite or the root timeline (sprite)")?;
                let s = self.sprite().unwrap_or(0);
                json!({ "sprite": s, "label": if s == 0 { "Root".to_string() } else { self.label(s) }, "frames": self.frames(), "frame": self.frame, "labels": t["labels"], "actions": t["actions"], "objects": self.entries().iter().map(|e| self.entry_json(e)).collect::<Vec<_>>() })
            }
            "info" => {
                let info = &self.info;
                let list = |k: &str| info[k].as_array().cloned().unwrap_or_default();
                match a["section"].as_str().unwrap_or("texts") {
                    "texts" => page(list("texts").into_iter().map(|mut t| {
                        let tag = t["tag"].as_u64().unwrap_or(0);
                        t["text"] = json!(self.value("text", tag, t["text"].as_str().unwrap_or("")));
                        t["edited"] = json!(self.is_edited(&format!("text|{tag}|0")));
                        t
                    }).collect(), a, 100),
                    "scripts" => page(list("scripts"), a, 30),
                    "images" => page(list("images").into_iter().map(|mut im| {
                        let tag = im["tag"].as_u64().unwrap_or(0);
                        im["fileName"] = json!(self.value("image", tag, im["fileName"].as_str().unwrap_or("")));
                        im
                    }).collect(), a, 100),
                    "linkage" => json!({ "exports": info["exports"], "imports": info["imports"] }),
                    "tags" => page(list("tags"), a, 200),
                    s => return Err(format!("unknown section {s}")),
                }
            }
            w => return Err(format!("unknown read {w}")),
        })
    }
}

impl Studio {
    /// The movie an action is about (`index` / `object` of the open package, else the selected export), selected.
    pub(crate) fn swf_tool_open(&mut self, a: &J, window: &mut Window, cx: &mut Context<Self>) -> Result<J, String> {
        if a["action"] == "movies" {
            let here: Vec<J> = self.open.as_ref().map(|t| (0..t.exports.len()).filter(|&i| is_movie(&t.exports[i].class)).map(|i| json!({ "package": t.path, "index": i, "path": t.object_path(i), "class": t.exports[i].class })).collect()).unwrap_or_default();
            return Ok(json!({ "openPackage": here }));
        }
        let t = self.open.as_ref().ok_or("no package is open (open one with studio_open_package)")?;
        if let Some(p) = a["path"].as_str() {
            if p != t.path {
                return Err(format!("{p} is not the open package: open it with studio_open_package first"));
            }
        }
        let index = match (a["index"].as_u64(), a["object"].as_str()) {
            (Some(i), _) => i as usize,
            (None, Some(o)) => {
                let hits: Vec<usize> = (0..t.exports.len()).filter(|&i| is_movie(&t.exports[i].class) && {
                    // With or without the package's name in front (`home.home`, `LobbyUI_SF.home.home`).
                    let full = t.object_path(i);
                    let inner = full.split_once('.').map(|(_, r)| r.to_string()).unwrap_or_default();
                    full.eq_ignore_ascii_case(o) || inner.eq_ignore_ascii_case(o) || t.exports[i].name.eq_ignore_ascii_case(o)
                }).collect();
                match hits.as_slice() {
                    [i] => *i,
                    [] => return Err(format!("no movie {o} in {} (action \"movies\" lists them)", t.name)),
                    many => return Err(format!("{o} names several movies: {}", many.iter().map(|i| format!("{i} {}", t.object_path(*i))).collect::<Vec<_>>().join(", "))),
                }
            }
            _ => self.selected_export.filter(|&i| t.exports.get(i).is_some_and(|e| is_movie(&e.class))).ok_or("give index or object (action \"movies\" lists the movies)")?,
        };
        let e = t.exports.get(index).ok_or_else(|| format!("export {index} does not exist"))?;
        if !is_movie(&e.class) {
            return Err(format!("export {index} is not a movie ({})", e.class));
        }
        self.view = super::super::View::Packages;
        if self.selected_export != Some(index) {
            self.reveal(index, cx);
            self.select_export(index, cx);
        }
        self.tab = super::super::inspector::Tab::Movie;
        self.movie_editor(window, cx).ok_or("the movie editor did not open")?;
        Ok(json!({ "index": index }))
    }

    fn swf_editor(&self) -> Result<Entity<MovieEditor>, String> {
        self.shown_movie().ok_or_else(|| "no movie is shown".to_string())
    }

    pub(crate) fn swf_tool_act(&mut self, a: &J, window: &mut Window, cx: &mut Context<Self>) -> Result<J, String> {
        let m = self.swf_editor()?;
        if let Some(e) = m.read(cx).error.clone() {
            return Err(e);
        }
        m.update(cx, |m, cx| m.tool_act(a, window, cx))
    }

    pub(crate) fn swf_tool_read(&mut self, what: &J, a: &J, cx: &mut Context<Self>) -> Result<J, String> {
        let m = self.swf_editor()?;
        m.read(cx).tool_read(what, a)
    }
}
