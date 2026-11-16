//! The map's actors as scene nodes (`crate::map_render`): what each draws
//! (meshes loaded once per mesh and material set through `mesh.preview`,
//! two requests in flight, brushes, cylinders, lights and markers), which
//! are shown (levels, categories, heavy walls), how they are shaded, and
//! framing the playable area or the selection.

use glam::{Mat4, Vec3};
use gpui::{AppContext as _, Context};
use serde_json::{json, Value as J};

use std::collections::{HashSet, VecDeque};

use super::{Actor, MapEditor};
use crate::map_render::Shading;

/// A game shading request.
pub enum GameStep {
    /// Which material a section of a loaded mesh draws with.
    Resolve { key: String, section: usize, params: J },
    /// A material's shaders (asked through one of its sections).
    Load { material: String, params: J },
}

/// Game shading requests: queued, asked for, materials met, failures.
#[derive(Default)]
pub struct GameLoad {
    pub queue: VecDeque<GameStep>,
    /// Requests in flight (the backend answers one at a time; the GPU pipelines build in parallel).
    pub running: usize,
    /// When the view was last redrawn for arriving materials.
    pub shown: Option<std::time::Instant>,
    pub asked: HashSet<(String, usize)>,
    pub materials: HashSet<String>,
    pub failed: usize,
    pub last_error: Option<String>,
    /// The clock loop runs ([`MapEditor::animate_game`]).
    pub animating: bool,
}
use crate::map_render::{self, arrow_lines, cylinder_lines, rgba, solid, Cv, Heavy, Node, Part, Shape};

/// The `mesh.preview` key of a mesh part: one load per mesh and material set.
pub fn mesh_key(part: &J) -> String {
    let m = &part["mesh"];
    let id = m["index"].as_u64().map(|i| i.to_string()).or_else(|| m["object"].as_str().map(str::to_string)).unwrap_or_default();
    let mats: Vec<String> = part["materials"].as_array().into_iter().flatten().map(|v| v.as_str().unwrap_or("").to_string()).collect();
    format!("{}|{id}|{}", m["path"].as_str().unwrap_or(""), mats.join(","))
}

fn mesh_request(part: &J, textures: bool) -> Option<J> {
    let m = &part["mesh"];
    let path = m["path"].as_str()?;
    let mut p = json!({ "path": path, "lod": 0, "textures": textures, "skin": false, "textureSize": 512 });
    match m["index"].as_u64() {
        Some(i) => p["index"] = json!(i),
        None => p["object"] = m["object"].clone(),
    }
    if part["materials"].as_array().is_some_and(|a| !a.is_empty()) {
        p["materials"] = part["materials"].clone();
    }
    Some(p)
}

fn floats(v: &J) -> Vec<f32> {
    v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default()
}

/// A brush part's triangles as a see-through fill and its edges.
pub fn brush_parts(part: &J, color: u32) -> (Part, Part) {
    let p = floats(&part["positions"]);
    let positions: Vec<Vec3> = p.as_chunks::<3>().0.iter().map(|c| Vec3::new(c[0], c[1], c[2])).collect();
    let indices: Vec<u32> = part["indices"].as_array().map(|a| a.iter().map(|x| x.as_u64().unwrap_or(0) as u32).collect()).unwrap_or_default();
    let (fill, lines) = solid(&positions, &indices, rgba(color, 0.04), rgba(color, 1.0));
    (Part::Fill { verts: fill, brush: true }, Part::Lines { verts: lines, pick: true })
}

impl MapEditor {
    /// What an actor draws, asking for the meshes the scene does not have.
    fn node_of_actor(&mut self, a: &Actor) -> Node {
        let color = a.color();
        let mut parts = Vec::new();
        let mut draws = false;
        for p in &a.parts {
            match p["kind"].as_str().unwrap_or("") {
                "mesh" => {
                    if p["hidden"].as_bool() == Some(true) {
                        continue;
                    }
                    draws = true;
                    let key = mesh_key(p);
                    self.want_mesh(&key, mesh_request(p, true));
                    parts.push(Part::Mesh { key, local: super::mat(&p["matrix"]), color: [1.0; 4], flat: false, edges: None });
                }
                "brush" => {
                    draws = true;
                    let (fill, lines) = brush_parts(p, color);
                    parts.push(fill);
                    parts.push(lines);
                }
                "cylinder" => {
                    let mut verts = Vec::new();
                    let r = p["radius"].as_f64().unwrap_or(0.0) as f32 / 100.0;
                    let h = p["height"].as_f64().unwrap_or(0.0) as f32 / 100.0;
                    cylinder_lines(r, h, rgba(color, 0.5), &mut verts);
                    parts.push(Part::Lines { verts, pick: false });
                }
                "light" => {
                    draws = true;
                    let c = floats(&p["color"]);
                    let lin = |v: f32| {
                        let v = v / 255.0;
                        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
                    };
                    let col = [lin(*c.first().unwrap_or(&255.0)), lin(*c.get(1).unwrap_or(&255.0)), lin(*c.get(2).unwrap_or(&255.0)), 1.0];
                    parts.push(Part::Marker { shape: Shape::Sphere, local: super::mat(&p["matrix"]) * Mat4::from_scale(Vec3::splat(0.2)), color: col });
                }
                "particles" => {
                    draws = true;
                    parts.push(Part::Marker { shape: Shape::Tetrahedron, local: Mat4::from_scale(Vec3::splat(0.3)), color: rgba(color, 1.0) });
                }
                _ => {}
            }
        }
        if !draws {
            // A marker standing for an actor that draws nothing itself, the same size whatever its DrawScale.
            let s = a.draw_scale.filter(|d| *d > 0.0).map(|d| 1.0 / d as f32).unwrap_or(1.0);
            parts.push(Part::Marker { shape: Shape::Octahedron, local: Mat4::from_scale(Vec3::splat(0.35 * s)), color: rgba(color, 1.0) });
            if ["playerstart", "spawn", "botfactory", "gameplay"].contains(&a.category.as_str()) {
                // Unreal +X is where the actor faces.
                let mut verts = Vec::new();
                arrow_lines(Vec3::ZERO, Vec3::X, 1.2 * s, 0.4 * s, rgba(color, 1.0), &mut verts);
                parts.push(Part::Lines { verts, pick: false });
            }
        }
        Node { matrix: a.matrix, visible: true, rooftop: !a.can_base_on, tint: None, parts }
    }

    /// Queue a mesh load (or mark it failed when it is in no package).
    pub(super) fn want_mesh(&mut self, key: &str, req: Option<J>) {
        if let Some(r) = &req {
            self.mesh_params.entry(key.to_string()).or_insert_with(|| r.clone());
        }
        let known = self.view.with(|s| s.has_mesh(key)).unwrap_or(true);
        if known {
            return;
        }
        match req {
            Some(r) => {
                self.view.with(|s| s.mark_loading(key.to_string()));
                self.mesh_queue.push_back((key.to_string(), r));
            }
            None => {
                self.view.with(|s| s.mark_failed(key.to_string()));
            }
        }
    }

    /// Start mesh loads, two at a time (the backend answers one at a time anyway).
    pub(super) fn pump_meshes(&mut self, cx: &mut Context<Self>) {
        while self.mesh_running < 2 {
            let Some((key, params)) = self.mesh_queue.pop_front() else { break };
            // Already here: a load asked for by the last map answered after this one queued it again.
            if !self.view.with(|s| s.is_loading(&key)).unwrap_or(false) {
                continue;
            }
            self.mesh_running += 1;
            let epoch = self.mesh_epoch;
            let call = self.call("mesh.preview", params);
            cx.spawn(async move |this, cx| {
                let parsed = match call.await {
                    Ok(r) => match r.bin(&r.json["glb"]).map(|b| b.to_vec()) {
                        Some(glb) => cx.background_spawn(async move { map_render::parse_glb(&glb) }).await,
                        None => Err("no GLB".to_string()),
                    },
                    Err(e) => Err(e),
                };
                let _ = this.update(cx, |this, cx| {
                    this.mesh_running -= 1;
                    // Asked for by a map since replaced, and not wanted by this one: dropped.
                    if this.mesh_epoch != epoch && !this.view.with(|s| s.is_loading(&key)).unwrap_or(false) {
                        cx.notify();
                        return;
                    }
                    this.view.with(|s| match &parsed {
                        Ok(m) => s.add_mesh(key.clone(), m),
                        Err(_) => s.mark_failed(key.clone()),
                    });
                    if this.shading == Shading::Game && parsed.is_ok() {
                        this.want_game_for(&key);
                        this.pump_game(cx);
                    }
                    this.collision_mesh_loaded(&key);
                    this.view.dirty();
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Build every level's nodes again (meshes already loaded are kept).
    pub(super) fn rebuild(&mut self, cx: &mut Context<Self>) {
        let mut nodes = Vec::new();
        let mut index = Vec::new();
        self.node_of.clear();
        let levels = std::mem::take(&mut self.levels);
        for (li, l) in levels.iter().enumerate() {
            for (ai, a) in l.actors.iter().enumerate() {
                self.node_of.insert((l.path.clone(), a.export), nodes.len());
                index.push((li, ai));
                nodes.push(self.node_of_actor(a));
            }
        }
        self.levels = levels;
        self.nodes = index;
        self.view.with(|s| s.layers[0] = nodes);
        self.apply_visibility();
        self.apply_look();
        self.apply_matinee(cx);
        self.view.dirty();
        cx.notify();
    }

    /// Levels and categories shown; the heavy-wall view shows flagged volumes
    /// even when volumes are hidden; the selection always shows.
    pub(super) fn apply_visibility(&mut self) {
        let sel = self.selected.as_ref().and_then(|s| self.node_of.get(s)).copied();
        let flags: Vec<bool> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(k, &(li, ai))| {
                let l = &self.levels[li];
                let a = &l.actors[ai];
                let shown = !self.hidden_levels.contains(&l.path) && (!self.hidden_categories.contains(&a.category) || (self.heavy && !a.can_base_on));
                shown || sel == Some(k)
            })
            .collect();
        // A Matinee toggle or visibility track switches actors off at the playhead.
        let hidden_by_matinee: Vec<bool> = self.nodes.iter().map(|&(li, ai)| self.matinee.hides(&self.levels[li].path, self.levels[li].actors[ai].export)).collect();
        self.view.with(|s| {
            for (k, n) in s.layers[0].iter_mut().enumerate() {
                n.visible = flags[k] && !hidden_by_matinee[k];
            }
            s.invalidate();
        });
        self.view.dirty();
    }

    /// Shading and the heavy-wall colours.
    /// Change the shading; game shading asks for the materials' shaders of every mesh loaded.
    pub(super) fn set_shading(&mut self, s: Shading, cx: &mut Context<Self>) {
        self.shading = s;
        self.apply_look();
        if s == Shading::Game {
            let keys: Vec<String> = self.mesh_params.keys().cloned().collect();
            for k in keys {
                self.want_game_for(&k);
            }
            self.pump_game(cx);
            self.animate_game(cx);
        }
        cx.notify();
    }

    /// Redraw the level 30 times a second while game shading draws a material
    /// that reads the clock (the viewport keeps one frame in flight: slow frames coalesce).
    pub(super) fn animate_game(&mut self, cx: &mut Context<Self>) {
        if self.game.animating {
            return;
        }
        self.game.animating = true;
        let start = std::time::Instant::now();
        cx.spawn(async move |this, cx| loop {
            let next = this.update(cx, |this, cx| {
                let t = start.elapsed().as_secs_f32();
                let animated = this.shading == Shading::Game && this.view.with(|s| s.set_game_time(t)).unwrap_or(false);
                if !animated {
                    this.game.animating = false;
                    return None;
                }
                this.view.dirty();
                cx.notify();
                Some(std::time::Duration::from_millis(33))
            });
            match next {
                Ok(Some(wait)) => cx.background_executor().timer(wait).await,
                _ => break,
            }
        })
        .detach();
    }

    /// The shader model changed: drop the game passes and ask again (when shading with them).
    pub(crate) fn reload_game(&mut self, cx: &mut Context<Self>) {
        self.game.queue.clear();
        self.game.asked.clear();
        self.game.materials.clear();
        self.game.failed = 0;
        self.mesh_epoch += 1;
        self.view.with(|s| s.clear_game());
        if self.shading == Shading::Game {
            self.set_shading(Shading::Game, cx);
        }
    }

    /// Ask for the material of each section of the mesh loaded as `key` (once).
    pub(super) fn want_game_for(&mut self, key: &str) {
        let Some(params) = self.mesh_params.get(key).cloned() else { return };
        let sections = self.view.with(|s| s.mesh_sections(key)).unwrap_or_default();
        for section in sections {
            if self.game.asked.insert((key.to_string(), section)) {
                self.game.queue.push_back(GameStep::Resolve { key: key.to_string(), section, params: params.clone() });
            }
        }
    }

    /// Run the game shader requests, one at a time: which material each
    /// section draws with (cheap), then each material's shaders once
    /// (`material.gameShader`, textures at 256: a level has hundreds).
    pub(super) fn pump_game(&mut self, cx: &mut Context<Self>) {
        while self.game.running < 3 {
            let Some(step) = self.game.queue.pop_front() else { break };
            self.game.running += 1;
            self.run_game_step(step, cx);
        }
        // Materials show as they come, the view redrawn at most four times a second (and when all are in).
        let due = self.game.shown.is_none_or(|t| t.elapsed() >= std::time::Duration::from_millis(250));
        if due || self.game.running == 0 {
            self.game.shown = Some(std::time::Instant::now());
            self.view.dirty();
            cx.notify();
        }
    }

    fn run_game_step(&mut self, step: GameStep, cx: &mut Context<Self>) {
        let epoch = self.mesh_epoch;
        match step {
            GameStep::Resolve { key, section, params } => {
                let mut p = params.clone();
                p["section"] = json!(section);
                p["resolveOnly"] = json!(true);
                let call = self.call("material.gameShader", p);
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| {
                        this.game.running -= 1;
                        if this.mesh_epoch == epoch {
                            match r.ok().and_then(|r| r.json["materialPath"].as_str().map(str::to_string)) {
                                Some(m) => {
                                    this.view.with(|s| s.set_game_section(key.clone(), section, m.clone()));
                                    if this.game.materials.insert(m.clone()) {
                                        let mut p = params.clone();
                                        p["section"] = json!(section);
                                        p["textureSize"] = json!(256);
                                        p["platform"] = json!(crate::game_material::platform());
                                        // Materials first: each draws many sections.
                                        this.game.queue.push_front(GameStep::Load { material: m, params: p });
                                    }
                                }
                                None => this.game.failed += 1,
                            }
                        }
                        this.pump_game(cx);
                    });
                })
                .detach();
            }
            GameStep::Load { material, params } => {
                let call = self.call("material.gameShader", params);
                cx.spawn(async move |this, cx| {
                    let built = match call.await {
                        Ok(r) => {
                            let busy = crate::backend::Busy::new();
                            let options = crate::game_material::PassOptions { reversed_depth: true, cull: false };
                            let b = cx.background_executor().spawn(async move { crate::game_material::GamePass::with_options(&r.json, &|v| r.bin(v).map(<[u8]>::to_vec), options) }).await;
                            drop(busy);
                            b
                        }
                        Err(e) => Err(e.to_string()),
                    };
                    let _ = this.update(cx, |this, cx| {
                        this.game.running -= 1;
                        if this.mesh_epoch == epoch {
                            match built {
                                Ok(pass) => {
                                    let animated = pass.animated;
                                    this.view.with(|s| s.add_game_material(material, pass));
                                    if animated {
                                        this.animate_game(cx);
                                    }
                                }
                                Err(e) => {
                                    this.game.failed += 1;
                                    this.game.last_error = Some(format!("{material}: {e}"));
                                }
                            }
                        }
                        this.pump_game(cx);
                    });
                })
                .detach();
            }
        }
    }

    pub(super) fn apply_look(&mut self) {
        let (shading, heavy) = (self.shading, self.heavy.then_some(if self.xray { Heavy::Xray } else { Heavy::Slope }));
        self.view.with(|s| {
            s.shading = shading;
            s.heavy = heavy;
            s.invalidate();
        });
        self.apply_visibility();
    }

    /// Frame the playable area: player starts and gameplay actors (skyboxes are far bigger), else everything.
    pub(super) fn frame_all(&mut self) {
        let play = ["playerstart", "spawn", "botfactory", "gameplay"];
        let mut pts: Vec<Vec3> = Vec::new();
        for only in [true, false] {
            for l in &self.levels {
                for a in &l.actors {
                    if a.category == "info" || (only && !play.contains(&a.category.as_str())) {
                        continue;
                    }
                    pts.push(a.matrix.w_axis.truncate());
                }
            }
            if !pts.is_empty() {
                break;
            }
        }
        if pts.is_empty() {
            return;
        }
        let (lo, hi) = pts.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(a, b), p| (a.min(*p), b.max(*p)));
        let center = (lo + hi) * 0.5;
        let size = hi - lo;
        let radius = size.x.max(size.z).max(10.0) * 0.6;
        self.view.with(|s| {
            s.cam.target = center;
            s.cam.eye = center + Vec3::new(radius * 0.6, radius * 0.8, radius * 0.6);
        });
        self.view.dirty();
    }

    /// Frame the selected actor (F).
    pub(super) fn frame_selection(&mut self) {
        let Some(k) = self.selected.as_ref().and_then(|s| self.node_of.get(s)).copied() else { return };
        self.view.with(|s| {
            let n = &s.layers[0][k];
            let (center, radius) = match s.bounds(0, k) {
                Some((lo, hi)) => ((lo + hi) * 0.5, (hi - lo).length().max(6.0)),
                None => (n.matrix.w_axis.truncate(), 6.0),
            };
            let dir = (s.cam.eye - s.cam.target).normalize_or(Vec3::new(0.6, 0.8, 0.6).normalize());
            s.cam.target = center;
            s.cam.eye = center + dir * radius * 1.5;
        });
        self.view.dirty();
    }

    /// The selected actor's node index.
    pub(super) fn selected_node(&self) -> Option<usize> {
        self.selected.as_ref().and_then(|s| self.node_of.get(s)).copied()
    }

    /// The actor of a node of layer 0.
    pub(super) fn node_actor(&self, k: usize) -> Option<(&str, &Actor)> {
        let &(li, ai) = self.nodes.get(k)?;
        let l = self.levels.get(li)?;
        Some((l.path.as_str(), l.actors.get(ai)?))
    }

    /// The light reach of the selected actor's lights (wire spheres), drawn while it is selected.
    pub(super) fn light_reach(&self, out: &mut Vec<Cv>) {
        let Some(a) = self.actor() else { return };
        for p in &a.parts {
            if p["kind"] != "light" {
                continue;
            }
            let r = p["radius"].as_f64().unwrap_or(0.0) as f32 / 100.0;
            if r <= 0.0 {
                continue;
            }
            let m = a.matrix * super::mat(&p["matrix"]);
            let c = floats(&p["color"]);
            let col = [c.first().copied().unwrap_or(255.0) / 255.0, c.get(1).copied().unwrap_or(255.0) / 255.0, c.get(2).copied().unwrap_or(255.0) / 255.0, 0.25];
            let mut v = Vec::new();
            map_render::sphere_lines(Vec3::ZERO, r, col, &mut v);
            out.extend(v.into_iter().map(|x| Cv::new(m.transform_point3(Vec3::from(x.pos)), x.color)));
        }
    }
}
