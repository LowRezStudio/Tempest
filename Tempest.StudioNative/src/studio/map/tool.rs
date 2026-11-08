//! What the MCP tool `studio_map` and the test driver's `map …` steps do:
//! the Electron tool's actions with the same arguments, run on the editor
//! the window shows. An action starts its work and says what to read once
//! nothing is in flight (`{"$read": …}`, answered by `read`), asks to be run
//! again once a list it needs is read (`{"$retry": true}`), or names a
//! backend call to answer with (`{"$backend": …}`).

use gpui::{Context, Window};
use serde_json::{json, Value as J};

use super::matinee::KeyRef;
use super::{MapEditor, Mode, Tool};
use crate::map_render::Shading;

fn read(what: &str) -> J {
    json!({ "$read": what })
}

impl MapEditor {
    /// The view's state (`studio_state.map`, the `levels` action).
    pub fn summary(&self, cx: &gpui::App) -> J {
        let (loading, loaded, failed) = self.view.with(|s| s.mesh_counts()).unwrap_or_default();
        let cam = self.view.cam();
        let (ms, (w, h)) = self.view.last_frame();
        let frame = json!({ "ms": ms, "width": w, "height": h });
        json!({
            "map": self.path,
            "view": self.mode.key(),
            "loading": self.loading,
            "error": self.error,
            "levels": self.levels.iter().map(|l| {
                let mut counts = serde_json::Map::new();
                for a in &l.actors {
                    let n = counts.get(&a.category).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
                    counts.insert(a.category.clone(), json!(n));
                }
                json!({ "path": l.path, "name": l.name, "actors": l.actors.len(), "counts": counts, "unsaved": self.is_dirty(&l.path, cx), "hidden": self.hidden_levels.contains(&l.path) })
            }).collect::<Vec<_>>(),
            "missing": self.missing,
            "selected": self.selected.as_ref().map(|(l, e)| self.actor_at(l, *e).map(|a| a.brief(l)).unwrap_or_else(|| json!({ "level": l, "export": e }))),
            "hiddenCategories": self.hidden_categories.iter().collect::<Vec<_>>(),
            "shading": self.shading.key(),
            "tool": self.tool.key(),
            "heavyWalls": self.heavy,
            "meshes": { "loading": loading + self.mesh_queue.len(), "loaded": loaded, "failed": failed },
            "camera": cam.map(|c| json!({ "eye": [c.eye.x, c.eye.y, c.eye.z], "target": [c.target.x, c.target.y, c.target.z] })),
            "gizmo": self.gizmo_on_screen(),
            "frame": frame,
            "collision": self.collision.enabled,
            "navmesh": self.nav.show,
            "kismet": self.kismet.level.as_ref().map(|l| json!({ "level": l, "sequence": self.kismet.sequence, "nodes": self.kismet.nodes().len(), "selected": self.kismet.selected })),
            "matinee": self.matinee.at.as_ref().map(|(l, i)| json!({ "curveKeys": self.matinee.curve.as_ref().map(|_| self.curve_on_screen()), "level": l, "index": i, "time": self.matinee.time, "playing": self.matinee.playing, "key": self.matinee.key.as_ref().map(|k| json!({ "track": k.track, "curve": k.curve, "key": k.key })), "track": self.matinee.track, "curve": self.matinee.curve.as_ref().map(|c| json!({ "track": c.0, "path": c.1 })), "poses": self.matinee.preview.poses.len() })),
            "palette": self.palette.open,
            "properties": self.props.read(cx).rows().into_iter().take(40).map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
            "unsavedLevels": self.dirty_levels(cx),
            "canUndo": self.can_undo(),
            "errors": self.errors,
        })
    }

    fn level_arg(&self, a: &J) -> Result<String, String> {
        let name = a["level"].as_str().map(str::to_string).or_else(|| self.path.clone()).ok_or("no map is open")?;
        self.level_named(&name).map(|l| l.path.clone()).ok_or_else(|| format!("no level {name} in this map"))
    }

    fn key_ref(a: &J) -> Result<KeyRef, String> {
        Ok(KeyRef { track: a["track"].as_u64().ok_or("give track")? as usize, curve: a["curve"].as_str().ok_or("give curve (a curve's path)")?.to_string(), key: a["key"].as_u64().ok_or("give key")? as usize })
    }

    /// Start an action (`studio_map`); see the module comment for the answer's forms.
    pub fn mcp_act(&mut self, a: &J, cx: &mut Context<Self>) -> Result<J, String> {
        let action = a["action"].as_str().unwrap_or("levels");
        let num = |k: &str| a[k].as_f64();
        let export = || a["export"].as_u64().map(|e| e as usize).ok_or("give export");
        if self.path.is_none() && !matches!(action, "list" | "open") {
            return Err("no map is open (open one with action open)".into());
        }
        // What is drawn in 3D shows the 3D view (the window follows the agent).
        if self.mode == Mode::Kismet && matches!(action, "select" | "move" | "show" | "addActor" | "heavyWalls" | "canBaseOn" | "collision" | "navmesh" | "setBrush") {
            self.mode = Mode::Scene;
        }
        match action {
            "list" => {
                if self.list.is_none() {
                    self.load_list(cx);
                }
                Ok(read("list"))
            }
            "open" => {
                let p = a["path"].as_str().ok_or("give path")?.to_string();
                let path = self.list.iter().flatten().find(|m| m.path == p || m.name.eq_ignore_ascii_case(&p) || m.name.trim_end_matches(".upk").eq_ignore_ascii_case(&p)).map(|m| m.path.clone()).unwrap_or(p);
                self.open(path, None, true, None, cx);
                Ok(read("levels"))
            }
            "levels" => Ok(read("levels")),
            "actors" => {
                let q = a["filter"].as_str().unwrap_or("").to_ascii_lowercase();
                let lv = a["level"].as_str().and_then(|l| self.level_named(l)).map(|l| l.path.clone());
                let mut rows = Vec::new();
                for l in &self.levels {
                    if lv.as_ref().is_some_and(|p| *p != l.path) {
                        continue;
                    }
                    for x in &l.actors {
                        if a["category"].as_str().is_some_and(|c| c != x.category) || (!q.is_empty() && !x.name.to_ascii_lowercase().contains(&q) && !x.class.to_ascii_lowercase().contains(&q)) {
                            continue;
                        }
                        rows.push(x.brief(&l.path));
                    }
                }
                Ok(json!({ "total": rows.len(), "actors": rows.into_iter().take(500).collect::<Vec<_>>() }))
            }
            "select" => {
                let l = self.level_arg(a)?;
                self.select(Some((l, export()?)), true, cx);
                self.frame_selection();
                Ok(read("levels"))
            }
            "move" => {
                let l = self.level_arg(a)?;
                let v3 = |k: &str| a[k].as_array().filter(|x| x.len() == 3).map(|x| [x[0].as_f64().unwrap_or(0.0), x[1].as_f64().unwrap_or(0.0), x[2].as_f64().unwrap_or(0.0)]);
                let e = export()?;
                if self.actor_at(&l, e).is_none() {
                    return Err("no such actor".into());
                }
                self.set_location(l, e, v3("location"), v3("rotation"), cx);
                Ok(read("selected"))
            }
            "show" => {
                if let Some(c) = a["categories"].as_object() {
                    for (k, v) in c {
                        let on = v.as_bool() == Some(true);
                        if on {
                            self.hidden_categories.remove(k);
                        } else {
                            self.hidden_categories.insert(k.clone());
                        }
                    }
                }
                if let Some(ls) = a["levels"].as_object() {
                    for (k, v) in ls {
                        if let Some(p) = self.level_named(k).map(|l| l.path.clone()) {
                            self.set_level_shown(&p, v.as_bool() == Some(true), cx);
                        }
                    }
                }
                if let Some(s) = a["shading"].as_str().and_then(Shading::parse) {
                    self.shading = s;
                }
                self.apply_look();
                cx.notify();
                Ok(read("levels"))
            }
            "kismet" => {
                let l = self.level_arg(a)?;
                self.open_kismet(l, a["sequence"].as_u64().map(|s| s as usize), true, cx);
                Ok(read("kismet"))
            }
            "link" | "unlink" => {
                let mut p = json!({ "from": a["from"], "to": a["to"] });
                for k in ["output", "input", "variable"] {
                    if !a[k].is_null() {
                        p[k] = a[k].clone();
                    }
                }
                if action == "unlink" {
                    p["remove"] = json!(true);
                }
                self.link(p, cx);
                Ok(read("graph"))
            }
            "palette" => {
                self.load_kismet_palette(cx);
                Ok(read("palette"))
            }
            "addNode" => {
                let Some(pal) = &self.kismet.palette else {
                    self.load_kismet_palette(cx);
                    return Ok(json!({ "$retry": true }));
                };
                let want = a["nodeClass"].as_str().ok_or("give nodeClass")?.to_ascii_lowercase();
                let c = pal.iter().find(|c| c["class"].as_str() == Some(want.as_str())).cloned().ok_or_else(|| format!("no example of {want} in this folder (see palette)"))?;
                let kind = c["kind"].as_str().unwrap_or("").to_string();
                self.add_node(c["example"].clone(), &kind, (num("x").unwrap_or(0.0) as f32, num("y").unwrap_or(0.0) as f32), (a["originator"].as_u64().map(|x| x as usize), a["object"].as_u64().map(|x| x as usize)), cx);
                Ok(read("added-node"))
            }
            "removeNode" => {
                self.remove_node(a["node"].as_u64().ok_or("give node")? as usize, cx);
                Ok(read("graph"))
            }
            "actorPalette" => {
                let l = self.level_arg(a)?;
                self.load_actor_palette(l, cx);
                Ok(read(if a["classes"] == true { "classes" } else { "actorPalette" }))
            }
            "addActor" => {
                let l = self.level_arg(a)?;
                let at = if a["location"].is_array() { None } else { self.view_target() };
                let mut p = json!({ "path": l, "at": at, "brush": a["brush"], "mesh": a["mesh"] });
                if let Some(c) = a["newClass"].as_str() {
                    p["class"] = json!(c);
                } else if let Some(e) = a["export"].as_u64() {
                    let x = self.actor_at(&l, e as usize).ok_or("no such actor in that level")?;
                    p["source"] = json!({ "path": l, "index": e });
                    if x.component {
                        p["parent"] = json!(x.parent);
                        self.pending_parent = x.parent;
                    }
                } else {
                    if self.palette.classes.is_none() || self.palette.loaded_for.as_ref() != Some(&l) {
                        self.load_actor_palette(l, cx);
                        return Ok(json!({ "$retry": true }));
                    }
                    let want = a["actorClass"].as_str().ok_or("give actorClass, export or newClass")?.to_ascii_lowercase();
                    let c = self.palette.classes.iter().flatten().find(|c| c["class"].as_str() == Some(want.as_str())).ok_or_else(|| format!("no example of {want} in this folder (see actorPalette)"))?;
                    p["source"] = c["example"].clone();
                }
                self.pending_location = a["location"].as_array().filter(|x| x.len() == 3).map(|x| [x[0].as_f64().unwrap_or(0.0), x[1].as_f64().unwrap_or(0.0), x[2].as_f64().unwrap_or(0.0)]);
                self.add_actor_call(l, p, "Could not add the actor".into(), cx);
                Ok(read("added"))
            }
            "removeActor" => {
                let l = self.level_arg(a)?;
                let x = self.actor_at(&l, export()?).cloned().ok_or("no such actor")?;
                self.remove_actor(l, x, cx);
                Ok(read("levels"))
            }
            "heavyWalls" => {
                if let Some(on) = a["on"].as_bool() {
                    self.heavy = on;
                    self.apply_look();
                }
                let rows: Vec<J> = self.levels.iter().flat_map(|l| l.actors.iter().filter(|x| !x.can_base_on).map(move |x| x.brief(&l.path))).collect();
                Ok(json!({ "on": self.heavy, "count": rows.len(), "actors": rows.into_iter().take(500).collect::<Vec<_>>() }))
            }
            "canBaseOn" => {
                let l = self.level_arg(a)?;
                self.select(Some((l, export()?)), false, cx);
                self.set_can_base_on(a["value"].as_bool().unwrap_or(false), cx);
                Ok(read("selected"))
            }
            "collision" => {
                if let Some(o) = a["only"].as_bool() {
                    self.collision.only = o;
                }
                self.toggle_collision(a["on"].as_bool().unwrap_or(true), cx);
                Ok(read("collision"))
            }
            "brush" => {
                let l = self.level_arg(a)?;
                Ok(json!({ "$backend": { "method": "map.brush", "params": { "path": l, "index": export()? } } }))
            }
            "setBrush" => {
                let l = self.level_arg(a)?;
                if !a["brush"].is_object() {
                    return Err("give brush".into());
                }
                let e = export()?;
                self.set_brush(l.clone(), e, a["brush"].clone(), cx);
                Ok(json!({ "$then": { "method": "map.brush", "params": { "path": l, "index": e } } }))
            }
            "export" => {
                let out = a["out"].as_str().ok_or("give out (.glb or .fbx)")?.to_string();
                let levels = a["levels"].as_object().map(|o| o.iter().filter(|(_, v)| v.as_bool() != Some(false)).filter_map(|(k, _)| self.level_named(k).map(|l| l.path.clone())).collect());
                let skip = a["skip"].as_array().map(|s| s.iter().filter_map(|x| x.as_str().map(str::to_string)).collect());
                self.export.volumes = a["volumes"] == true;
                self.export.textures = a["textures"] != false;
                self.export.size = a["textureSize"].as_u64().unwrap_or(1024) as u32;
                self.export_to(out, levels, skip, cx);
                Ok(read("export"))
            }
            "matinee" => {
                let l = self.level_arg(a)?;
                self.open_matinee(l, export()?, true, cx);
                Ok(read("matinee"))
            }
            "matinees" => {
                let l = self.level_arg(a)?;
                self.load_matinee_list(l, false, cx);
                Ok(read("matinees"))
            }
            "newMatinee" => {
                let l = self.level_arg(a)?;
                self.create_matinee(l, a["actor"].as_u64().map(|x| x as usize), num("length"), a["sequence"].as_u64().map(|x| x as usize), cx);
                Ok(read("matinee"))
            }
            "setKey" | "setInterpMode" | "setTangents" => {
                let r = Self::key_ref(a)?;
                let mut change = json!({});
                for k in ["time", "value", "mode", "arrive", "leave", "fields", "values"] {
                    if !a[k].is_null() {
                        change[k] = a[k].clone();
                    }
                }
                self.set_key(r, change, cx);
                Ok(read("timeline"))
            }
            "addKey" => {
                let mut extra = json!({});
                for k in ["value", "mode", "values", "fields"] {
                    if !a[k].is_null() {
                        extra[k] = a[k].clone();
                    }
                }
                self.add_key(a["track"].as_u64().ok_or("give track")? as usize, a["curve"].as_str().ok_or("give curve")?.to_string(), num("time").ok_or("give time")?, extra, cx);
                Ok(read("timeline"))
            }
            "removeKey" => {
                self.remove_key(Self::key_ref(a)?, cx);
                Ok(read("matinee"))
            }
            "keyAtPlayhead" => {
                let t = a["track"].as_u64().ok_or("give track")? as usize;
                self.matinee.track = Some(t);
                self.key_at_playhead(t, cx);
                Ok(read("timeline"))
            }
            "addGroup" => {
                self.add_group(a["name"].as_str().unwrap_or("NewGroup").to_string(), a["director"] == true, cx);
                Ok(read("matinee"))
            }
            "removeGroup" => {
                self.remove_group(a["group"].as_u64().ok_or("give group")? as usize, cx);
                Ok(read("matinee"))
            }
            "setGroup" => {
                let mut p = json!({ "group": a["group"].as_u64().ok_or("give group")? });
                if !a["name"].is_null() {
                    p["name"] = a["name"].clone();
                }
                if !a["color"].is_null() {
                    p["color"] = a["color"].clone();
                }
                self.matinee_edit("matinee.setGroup", p, None, cx);
                Ok(read("matinee"))
            }
            "addTrack" => {
                self.add_track(a["group"].as_u64().ok_or("give group")? as usize, a["trackClass"].as_str().ok_or("give trackClass")?.to_string(), a["property"].as_str().map(str::to_string), cx);
                Ok(read("matinee"))
            }
            "removeTrack" => {
                self.remove_track(a["track"].as_u64().ok_or("give track")? as usize, cx);
                Ok(read("matinee"))
            }
            "setProps" => {
                if !a["props"].is_object() {
                    return Err("give props".into());
                }
                self.matinee_edit("matinee.set", json!({ "object": a["object"], "values": a["props"] }), None, cx);
                Ok(read("matinee"))
            }
            "connect" => {
                self.connect(a["group"].as_u64().ok_or("give group")? as usize, export()?, a["remove"] == true, cx);
                Ok(read("matinee"))
            }
            "matineeTime" => {
                self.mode = Mode::Matinee;
                self.set_matinee_time(num("time").unwrap_or(0.0), cx);
                let p = &self.matinee.preview;
                Ok(json!({ "time": self.matinee.time, "poses": p.poses.iter().map(|(k, m)| (k.to_string(), json!(m.to_cols_array()))).collect::<serde_json::Map<_, _>>(), "hidden": p.hidden.iter().collect::<Vec<_>>(), "tints": p.tints.iter().map(|(k, (c, b))| (k.to_string(), json!({ "color": c, "brightness": b }))).collect::<serde_json::Map<_, _>>() }))
            }
            "navmesh" => {
                let op = a["navOp"].as_str().unwrap_or("status");
                match op {
                    "show" => self.nav.show = true,
                    "hide" => self.nav.show = false,
                    _ => {}
                }
                if let Some(c) = a["colour"].as_str() {
                    self.nav.by_cluster = c == "cluster";
                }
                if self.nav.show || op != "status" {
                    self.load_nav(false, cx);
                }
                let faces: Vec<u32> = a["faces"].as_array().into_iter().flatten().filter_map(|f| f.as_u64().map(|f| f as u32)).collect();
                match op {
                    "pick" => {
                        self.nav.pick = super::navmesh::Pick::Faces;
                        self.nav.selected = faces.into_iter().collect();
                    }
                    "deleteFaces" | "cutRegion" => {
                        if faces.is_empty() {
                            return Err("give faces".into());
                        }
                        if self.nav.actors.is_empty() {
                            return Ok(json!({ "$retry": true }));
                        }
                        self.nav_edit(if op == "cutRegion" { "deleteRegion" } else { "deleteFaces" }, json!({ "faces": faces }), cx);
                    }
                    "moveVertex" => {
                        let v = a["vertex"].as_u64().ok_or("give vertex")?;
                        let l = a["location"].as_array().filter(|x| x.len() == 3).ok_or("give location")?;
                        if self.nav.actors.is_empty() {
                            return Ok(json!({ "$retry": true }));
                        }
                        let n = |i: usize| l[i].as_f64().unwrap_or(0.0) / 100.0;
                        self.nav_edit("moveVertices", json!({ "moves": [[v, n(0), n(2), n(1)]] }), cx);
                    }
                    _ => {}
                }
                self.rebuild_nav();
                Ok(read("navmesh"))
            }
            "undo" => {
                self.undo(cx);
                Ok(read("levels"))
            }
            "bake" => {
                let scope = a["scope"].as_str().unwrap_or("edited");
                let targets = if scope == "selected" {
                    let l = self.level_arg(a).ok().or_else(|| self.selected.as_ref().map(|s| s.0.clone())).ok_or("give level and export (or select an actor)")?;
                    let e = a["export"].as_u64().map(|e| e as usize).or(self.selected.as_ref().map(|s| s.1)).ok_or("give level and export (or select an actor)")?;
                    Some(json!([{ "path": l, "index": e }]))
                } else {
                    None
                };
                let levels = a["levels"].as_object().map(|o| o.iter().filter(|(_, v)| v.as_bool() == Some(true)).filter_map(|(k, _)| self.level_named(k).map(|l| l.path.clone())).collect());
                let mut extra = json!({});
                for k in ["shadowSamples", "skySamples", "indirectRays", "albedo", "environmentScale", "compare"] {
                    extra[k] = a[k].clone();
                }
                self.bake_now(scope, targets, levels, extra, cx);
                Ok(read("bake"))
            }
            "lightInfo" => {
                let l = self.level_arg(a).ok().or_else(|| self.selected.as_ref().map(|s| s.0.clone())).ok_or("no such level")?;
                let mut e = a["export"].as_u64().map(|e| e as usize).or(self.selected.as_ref().map(|s| s.1)).ok_or("give export")?;
                if let Some(p) = self.actor_at(&l, e).and_then(|x| x.parts.iter().find(|p| p["kind"] == "mesh")).and_then(|p| p["export"].as_u64()) {
                    e = p as usize;
                }
                Ok(json!({ "$backend": { "method": "light.info", "params": { "path": l, "index": e } } }))
            }
            a => Err(format!("unknown action {a}")),
        }
    }

    /// The answer of an action once its work is done.
    pub fn mcp_read(&self, what: &str, cx: &gpui::App) -> J {
        match what {
            "list" => json!(self.list.iter().flatten().map(|m| json!({ "path": m.path, "name": m.name, "persistent": m.persistent, "kismet": m.kismet })).collect::<Vec<_>>()),
            "selected" => json!({ "selected": self.selected.as_ref().and_then(|(l, e)| self.actor_at(l, *e).map(|a| a.brief(l))), "error": self.error }),
            "kismet" => json!({ "level": self.kismet.level, "sequences": self.kismet.sequences, "graph": self.kismet.graph }),
            "graph" => json!({ "graph": self.kismet.graph }),
            "added-node" => json!({ "added": self.kismet.selected, "node": self.kismet.selected.and_then(|e| self.kismet.node(e)) }),
            "palette" => json!(self.kismet.palette.iter().flatten().map(|c| json!({ "class": c["class"], "kind": c["kind"], "count": c["count"] })).collect::<Vec<_>>()),
            "actorPalette" => json!(self.palette.classes.iter().flatten().map(|c| json!({ "class": c["class"], "category": c["category"], "count": c["count"], "inMap": c["inMap"] })).collect::<Vec<_>>()),
            "classes" => json!(self.palette.placeable),
            "added" => json!({ "errors": self.errors.last(), "added": self.selected.as_ref().map(|s| s.1), "warnings": self.last_added_warnings, "actor": self.selected.as_ref().and_then(|(l, e)| self.actor_at(l, *e).map(|a| a.brief(l))) }),
            "collision" => {
                let mut volumes = Vec::new();
                let mut none = Vec::new();
                if let Some(d) = &self.collision.data {
                    for l in d["levels"].as_array().into_iter().flatten() {
                        for it in l["items"].as_array().into_iter().flatten() {
                            if it["kind"] == "volume" {
                                volumes.push(json!({ "level": l["path"], "export": it["export"], "name": it["name"], "class": it["class"], "blocks": it["flags"]["blocks"], "hulls": it["geometry"]["convexCount"].as_u64().unwrap_or(0) }));
                            } else {
                                let m = &d["meshes"][it["meshKey"].as_str().unwrap_or("")];
                                if !m.is_null() && it["flags"]["blocks"] != "none" && m["pawns"] == "none" {
                                    none.push(json!({ "level": l["path"], "export": it["export"], "name": it["name"] }));
                                }
                            }
                        }
                    }
                }
                json!({ "on": self.collision.enabled, "only": self.collision.only, "counts": self.collision.summary(), "volumes": volumes.into_iter().take(500).collect::<Vec<_>>(), "noBodySetup": none.into_iter().take(200).collect::<Vec<_>>() })
            }
            "export" => self.last_export.clone().unwrap_or(J::Null),
            "matinee" => self.matinee.tl.clone().unwrap_or(J::Null),
            "timeline" => json!({ "key": self.matinee.key.as_ref().map(|k| json!({ "track": k.track, "curve": k.curve, "key": k.key })), "timeline": self.matinee.tl }),
            "matinees" => json!(self.matinee.list),
            "navmesh" => json!({ "show": self.nav.show, "level": self.nav.level, "error": self.nav.error, "colour": if self.nav.by_cluster { "cluster" } else { "region" }, "selected": self.nav.selected.iter().take(500).collect::<Vec<_>>(), "actors": self.nav.actors.iter().map(|a| a.info.clone()).collect::<Vec<_>>() }),
            "bake" => {
                let Some(r) = &self.bake.last else { return J::Null };
                let mut r = r.clone();
                for k in ["baked", "comparisons"] {
                    if let Some(a) = r[k].as_array_mut() {
                        a.truncate(200);
                    }
                }
                r
            }
            _ => self.summary(cx),
        }
    }

    /// The map's commands (`Cmd::Map*`, the palette and the shortcut list).
    pub fn run_command(&mut self, cmd: crate::studio::Cmd, window: &mut Window, cx: &mut Context<Self>) {
        use crate::studio::Cmd;
        if self.path.is_none() {
            self.toast(crate::studio::Tone::Info, "Open a map first", cx);
            return;
        }
        match cmd {
            Cmd::MapAddActor => {
                if !self.palette.open {
                    self.toggle_palette(window, cx);
                }
            }
            Cmd::MapCollision => {
                let on = !self.collision.enabled;
                self.toggle_collision(on, cx);
            }
            Cmd::MapBakeLighting => self.open_bake(cx),
            Cmd::MapNewMatinee => {
                let level = self.target_level().unwrap_or_default();
                let actor = self.actor().map(|a| a.owner());
                self.create_matinee(level, actor, None, None, cx);
            }
            Cmd::MapOpenMatinee => {
                let level = self.target_level().unwrap_or_default();
                self.load_matinee_list(level, true, cx);
            }
            Cmd::MapMatineeKey => match self.matinee.track.filter(|_| self.mode == Mode::Matinee) {
                Some(t) => self.key_at_playhead(t, cx),
                None => self.toast(crate::studio::Tone::Info, "Select a track of the Matinee timeline first", cx),
            },
            _ => {}
        }
        cx.notify();
    }

    /// Delete: the selected Matinee key in the Matinee view, else the selected actor (asking first).
    pub fn delete_key_or_actor(&mut self, cx: &mut Context<Self>) {
        if self.mode == Mode::Matinee {
            if let Some(k) = self.matinee.key.clone() {
                self.remove_key(k, cx);
                return;
            }
        }
        match self.mode {
            Mode::Kismet => {}
            _ => self.ask_remove(cx),
        }
    }

    /// A driver step (`map …`).
    pub fn drive(&mut self, arg: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let words: Vec<&str> = arg.split_whitespace().collect();
        let rest = |n: usize| words.get(n..).map(|w| w.join(" ")).unwrap_or_default();
        let num = |n: usize| words.get(n).and_then(|w| w.parse::<f64>().ok()).ok_or_else(|| format!("{arg}: give a number"));
        match words.first().copied().unwrap_or("") {
            "open" => {
                let name = rest(1);
                let path = self.list.iter().flatten().find(|m| m.name.eq_ignore_ascii_case(&name) || m.name.trim_end_matches(".upk").eq_ignore_ascii_case(&name) || m.path == name).map(|m| m.path.clone()).ok_or_else(|| format!("no map {name}"))?;
                self.open(path, None, true, None, cx);
            }
            "select" => {
                let (l, e) = match (words.get(1), words.get(2).and_then(|w| w.parse::<usize>().ok())) {
                    (Some(l), Some(e)) => (self.level_named(l).map(|l| l.path.clone()).ok_or("no such level")?, e),
                    (Some(name), None) => self.levels.iter().find_map(|l| l.actors.iter().find(|a| a.name.eq_ignore_ascii_case(name)).map(|a| (l.path.clone(), a.export))).ok_or_else(|| format!("no actor {name}"))?,
                    _ => return Err("select <level> <export> | <actor name>".into()),
                };
                self.select(Some((l, e)), true, cx);
                self.frame_selection();
            }
            "deselect" => self.select(None, true, cx),
            "click" => self.click_at(num(1)? as f32, num(2)? as f32, cx),
            "tool" => {
                self.tool = match words.get(1).copied() {
                    Some("select") => Tool::Select,
                    Some("rotate") => Tool::Rotate,
                    Some("scale") => Tool::Scale,
                    _ => Tool::Translate,
                };
                self.view.dirty();
            }
            "shading" => {
                self.shading = Shading::parse(words.get(1).copied().unwrap_or("")).ok_or("lit|unlit|untextured|wireframe")?;
                self.apply_look();
            }
            "heavy" => {
                self.heavy = !self.heavy;
                self.apply_look();
            }
            "xray" => {
                self.xray = !self.xray;
                self.apply_look();
            }
            "category" => self.set_category(words.get(1).ok_or("category <id> on|off")?, words.get(2) != Some(&"off"), cx),
            "level" => {
                let p = self.level_named(words.get(1).ok_or("level <name> on|off")?).map(|l| l.path.clone()).ok_or("no such level")?;
                self.set_level_shown(&p, words.get(2) != Some(&"off"), cx);
            }
            "frame" => self.frame_selection(),
            "frame-all" => self.frame_all(),
            "mode" => {
                self.mode = match words.get(1).copied() {
                    Some("kismet") => Mode::Kismet,
                    Some("matinee") => Mode::Matinee,
                    _ => Mode::Scene,
                };
                self.apply_matinee(cx);
            }
            "kismet" => {
                let l = match words.get(1) {
                    Some(n) => self.level_named(n).map(|l| l.path.clone()).ok_or("no such level")?,
                    None => self.path.clone().unwrap_or_default(),
                };
                self.open_kismet(l, words.get(2).and_then(|s| s.parse().ok()), true, cx);
            }
            "node" => self.select_node(words.get(1).and_then(|s| s.parse().ok()), cx),
            "palette" => self.toggle_palette(window, cx),
            "node-palette" => self.open_node_palette(None, window, cx),
            "matinee" => {
                let l = self.level_named(words.get(1).ok_or("matinee <level> <export>")?).map(|l| l.path.clone()).ok_or("no such level")?;
                self.open_matinee(l, words.get(2).and_then(|s| s.parse().ok()).ok_or("matinee <level> <export>")?, true, cx);
            }
            "time" => {
                self.mode = Mode::Matinee;
                self.set_matinee_time(num(1)?, cx);
            }
            "play" => self.toggle_play(cx),
            "track" => self.matinee.track = words.get(1).and_then(|s| s.parse().ok()),
            "curve" => {
                let t = words.get(1).and_then(|s| s.parse().ok()).ok_or("curve <track> <path>")?;
                self.matinee.curve = Some((t, rest(2)));
            }
            "collision" => self.toggle_collision(words.get(1) != Some(&"off"), cx),
            "collision-only" => {
                self.collision.only = words.get(1) != Some(&"off");
                self.toggle_collision(true, cx);
            }
            "nav" => {
                if self.nav.show != (words.get(1) != Some(&"off")) {
                    self.toggle_nav(cx);
                }
            }
            "nav-pick" => {
                self.nav.pick = match words.get(1).copied() {
                    Some("faces") => super::navmesh::Pick::Faces,
                    Some("vertex") => super::navmesh::Pick::Vertex,
                    _ => super::navmesh::Pick::Off,
                };
            }
            "export-dialog" => self.open_export(cx),
            "bake-dialog" => self.open_bake(cx),
            "close" => {
                self.export.open = false;
                self.bake.open = false;
                self.palette.open = false;
                self.confirm = None;
            }
            "undo" => self.undo(cx),
            "orbit" => {
                let (dx, dy) = (num(1)? as f32, num(2)? as f32);
                self.view.with(|s| {
                    let off = s.cam.eye - s.cam.target;
                    let q = glam::Quat::from_rotation_y(-dx * 0.01) * glam::Quat::from_axis_angle(s.cam.axes().1, -dy * 0.01);
                    s.cam.eye = s.cam.target + q * off;
                });
                self.view.dirty();
            }
            "zoom" => {
                let f = num(1)? as f32;
                self.view.with(|s| s.cam.eye = s.cam.target + (s.cam.eye - s.cam.target) / f.max(0.01));
                self.view.dirty();
            }
            s => return Err(format!("unknown map step {s}")),
        }
        cx.notify();
        Ok(())
    }
}
