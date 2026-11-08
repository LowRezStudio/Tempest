//! Collision in the map view (`map.collision`, `map.brush`, `map.setBrush`):
//! what pawns and shots hit, drawn over (or instead of) the render geometry
//! as the scene's layer 1, and the shape of volumes. Volumes are their
//! convex hulls, meshes their simple collision (boxes, spheres, capsules,
//! hulls) or, when they collide with their render triangles ("complex"),
//! the mesh itself loaded untextured. The inspector's Collision section
//! shows an actor's switches as toggles and, for volumes, a Shape panel
//! that rebuilds the brush with UnrealEd's builders, from a mesh, or from
//! edited vertices.

use std::collections::HashMap;

use glam::{Mat4, Vec3};
use gpui::{div, prelude::*, px, AnyElement, Context, Entity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Selectable, Sizable};
use serde_json::{json, Value as J};

use super::{hsla, Actor, MapEditor};
use crate::map_render::{rgba, solid, Node, Part};
use crate::studio::Tone;

/// Colours by what a primitive blocks; volumes that block nothing by kind.
pub const BLOCKS: &[(&str, u32, &str)] = &[("all", 0xf38ba8, "blocks pawns and shots"), ("pawns", 0xfab387, "blocks pawns only"), ("shots", 0x89dceb, "blocks shots only"), ("none", 0x7f849c, "no collision")];

fn block_color(b: &str) -> u32 {
    BLOCKS.iter().find(|x| x.0 == b).map(|x| x.1).unwrap_or(0x7f849c)
}

fn block_label(b: &str) -> &'static str {
    BLOCKS.iter().find(|x| x.0 == b).map(|x| x.2).unwrap_or("")
}

fn item_color(it: &J) -> u32 {
    let blocks = it["flags"]["blocks"].as_str().unwrap_or("none");
    if it["kind"] == "volume" && blocks == "none" {
        return match it["category"].as_str().unwrap_or("") {
            "trigger" => 0xa6e3a1,
            "blocking" => 0xfab387,
            "navigation" => 0x94e2d5,
            _ => 0xcba6f7,
        };
    }
    block_color(blocks)
}

/// UnrealEd's brush builders and their defaults (Unreal units).
/// A builder: its kind, label and fields (key, label, default).
type Builder = (&'static str, &'static str, &'static [(&'static str, &'static str, f64)]);

pub const BUILDERS: &[Builder] = &[
    ("box", "Box", &[("x", "X", 256.0), ("y", "Y", 256.0), ("z", "Z", 256.0), ("wallThickness", "Wall", 16.0)]),
    ("cylinder", "Cylinder", &[("z", "Height", 256.0), ("outerRadius", "Radius", 512.0), ("innerRadius", "Inner radius", 384.0), ("sides", "Sides", 8.0)]),
    ("cone", "Cone", &[("z", "Height", 256.0), ("capZ", "Cap height", 256.0), ("outerRadius", "Radius", 512.0), ("innerRadius", "Inner radius", 384.0), ("sides", "Sides", 8.0)]),
    ("sphere", "Sphere", &[("radius", "Radius", 256.0), ("sphereExtrapolation", "Subdivisions (1–5)", 2.0)]),
];

/// Collision switches with what they mean.
const FLAGS: &[(&str, &str)] = &[
    ("bCollideActors", "Actor: takes part in collision at all"),
    ("bBlockActors", "Actor: blocks other actors' movement"),
    ("CollideActors", "Component: takes part in collision"),
    ("BlockActors", "Component: blocks actors (pawns)"),
    ("BlockNonZeroExtent", "Component: blocks box sweeps (pawn movement)"),
    ("BlockZeroExtent", "Component: blocks line traces (shots, hitscan)"),
    ("BlockRigidBody", "Component: blocks physics bodies"),
];

pub struct Collision {
    pub enabled: bool,
    /// Hide the render geometry while collision shows.
    pub only: bool,
    /// Leave out what collides with nothing.
    pub hide_none: bool,
    pub loading: bool,
    pub data: Option<J>,
    // The Shape panel.
    pub kind: String,
    pub spec: HashMap<String, f64>,
    pub hollow: bool,
    pub align: bool,
    pub mesh_object: String,
    pub bounds: bool,
    pub info: Option<J>,
    vertices: Option<Entity<TextareaState>>,
    /// The current brush's corners, one "x y z" per line (Current fills it).
    vertices_text: Option<String>,
    pub busy: bool,
}

impl Collision {
    pub fn new(_window: &mut Window, _cx: &mut Context<MapEditor>) -> Collision {
        let mut c = Collision { enabled: false, only: false, hide_none: true, loading: false, data: None, kind: "box".into(), spec: HashMap::new(), hollow: false, align: true, mesh_object: String::new(), bounds: false, info: None, vertices: None, vertices_text: None, busy: false };
        c.pick("box");
        c
    }

    fn pick(&mut self, kind: &str) {
        self.kind = kind.to_string();
        if let Some((_, _, fields)) = BUILDERS.iter().find(|b| b.0 == kind) {
            self.spec = fields.iter().map(|(k, _, v)| (k.to_string(), *v)).collect();
            self.hollow = false;
            self.align = true;
        }
    }

    /// Counts per kind and `blocks` (MCP).
    pub fn summary(&self) -> J {
        let mut out = serde_json::Map::new();
        for l in self.data.as_ref().and_then(|d| d["levels"].as_array()).into_iter().flatten() {
            for it in l["items"].as_array().into_iter().flatten() {
                let k = format!("{}:{}", it["kind"].as_str().unwrap_or(""), it["flags"]["blocks"].as_str().unwrap_or(""));
                let n = out.get(&k).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
                out.insert(k, json!(n));
            }
        }
        J::Object(out)
    }

    /// The collision items of an actor (a volume, its mesh components, or one collection mesh).
    pub fn items_of(&self, level: &str, export: usize) -> Vec<J> {
        let Some(l) = self.data.as_ref().and_then(|d| d["levels"].as_array()).and_then(|a| a.iter().find(|l| l["path"] == level)) else { return Vec::new() };
        l["items"].as_array().into_iter().flatten().filter(|it| it["export"].as_u64() == Some(export as u64) || (it["kind"] == "mesh" && it["parent"].as_u64() == Some(export as u64))).cloned().collect()
    }
}

/// Indexed geometry of a box of full lengths `s` centred on the origin.
fn box_geo(s: Vec3) -> (Vec<Vec3>, Vec<u32>) {
    let h = s * 0.5;
    let c = |k: u32| Vec3::new(if k & 1 == 0 { -h.x } else { h.x }, if k & 2 == 0 { -h.y } else { h.y }, if k & 4 == 0 { -h.z } else { h.z });
    ((0..8).map(c).collect(), vec![0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5])
}

/// A UV sphere, or a capsule along Y when `len` > 0 (three's CapsuleGeometry).
fn capsule_geo(r: f32, len: f32, segs: u32, rings: u32) -> (Vec<Vec3>, Vec<u32>) {
    let mut pos = Vec::new();
    for i in 0..=rings {
        let phi = i as f32 / rings as f32 * std::f32::consts::PI;
        let shift = if phi < std::f32::consts::FRAC_PI_2 { len / 2.0 } else { -len / 2.0 };
        for k in 0..=segs {
            let th = k as f32 / segs as f32 * std::f32::consts::TAU;
            pos.push(Vec3::new(r * phi.sin() * th.cos(), r * phi.cos() + shift, r * phi.sin() * th.sin()));
        }
    }
    let mut idx = Vec::new();
    for i in 0..rings {
        for k in 0..segs {
            let a = i * (segs + 1) + k;
            let b = a + segs + 1;
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    (pos, idx)
}

impl MapEditor {
    /// Read the map's collision (again with `force`, after an edit).
    pub(super) fn load_collision(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else { return };
        if !force && self.collision.data.is_some() {
            self.rebuild_collision();
            return;
        }
        self.collision.loading = true;
        let call = self.call("map.collision", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.collision.loading = false;
                match r {
                    Ok(r) => {
                        this.collision.data = Some(r.json);
                        this.rebuild_collision();
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not read the collision: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn toggle_collision(&mut self, on: bool, cx: &mut Context<Self>) {
        self.collision.enabled = on;
        if on {
            self.load_collision(false, cx);
        }
        self.apply_collision_layers();
        cx.notify();
    }

    fn apply_collision_layers(&mut self) {
        let (on, only) = (self.collision.enabled, self.collision.only);
        self.view.with(|s| {
            s.shown[1] = on;
            s.shown[0] = !(on && only);
            s.invalidate();
        });
        self.view.dirty();
    }

    /// Aggregate geometry: hulls (view space already), boxes, spheres and capsules (Unreal units under their matrices).
    fn agg_parts(geom: &J, color: u32, out: &mut Vec<Part>) {
        let (fill, edge) = (rgba(color, 0.32), rgba(color, 0.8));
        let mut add = |pos: Vec<Vec3>, idx: Vec<u32>, m: Mat4| {
            let pos: Vec<Vec3> = pos.into_iter().map(|p| m.transform_point3(p)).collect();
            let (f, l) = solid(&pos, &idx, fill, edge);
            out.push(Part::Fill { verts: f, brush: false });
            out.push(Part::Lines { verts: l, pick: false });
        };
        let nums = |v: &J| -> Vec<f32> { v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default() };
        let p = nums(&geom["positions"]);
        let idx: Vec<u32> = geom["indices"].as_array().map(|a| a.iter().map(|x| x.as_u64().unwrap_or(0) as u32).collect()).unwrap_or_default();
        if !idx.is_empty() {
            add(p.as_chunks::<3>().0.iter().map(|c| Vec3::new(c[0], c[1], c[2])).collect(), idx, Mat4::IDENTITY);
        }
        let s = 0.01;
        for b in geom["boxes"].as_array().into_iter().flatten() {
            // View space swaps Y and Z.
            let size = Vec3::new(b["x"].as_f64().unwrap_or(0.0) as f32, b["z"].as_f64().unwrap_or(0.0) as f32, b["y"].as_f64().unwrap_or(0.0) as f32) * s;
            let (pos, i) = box_geo(size);
            add(pos, i, super::mat(&b["matrix"]));
        }
        for sp in geom["spheres"].as_array().into_iter().flatten() {
            let (pos, i) = capsule_geo(sp["radius"].as_f64().unwrap_or(0.0) as f32 * s, 0.0, 16, 12);
            add(pos, i, super::mat(&sp["matrix"]));
        }
        for c in geom["sphyls"].as_array().into_iter().flatten() {
            let (pos, i) = capsule_geo(c["radius"].as_f64().unwrap_or(0.0) as f32 * s, c["length"].as_f64().unwrap_or(0.0) as f32 * s, 12, 12);
            add(pos, i, super::mat(&c["matrix"]));
        }
    }

    /// The collision layer for the whole map: per item its world matrix, then what it collides with.
    pub(super) fn rebuild_collision(&mut self) {
        let Some(data) = self.collision.data.clone() else { return };
        let hide_none = self.collision.hide_none;
        let mut nodes = Vec::new();
        let mut wanted = Vec::new();
        for level in data["levels"].as_array().into_iter().flatten() {
            if self.hidden_levels.contains(level["path"].as_str().unwrap_or("")) {
                continue;
            }
            for it in level["items"].as_array().into_iter().flatten() {
                let blocks = it["flags"]["blocks"].as_str().unwrap_or("none");
                let color = item_color(it);
                let mut parts = Vec::new();
                if it["kind"] == "volume" {
                    if hide_none && blocks == "none" && it["category"] != "trigger" {
                        continue;
                    }
                    Self::agg_parts(&it["geometry"], color, &mut parts);
                } else {
                    if hide_none && blocks == "none" {
                        continue;
                    }
                    let key = it["meshKey"].as_str().unwrap_or("");
                    let mc = &data["meshes"][key];
                    if mc.is_null() {
                        continue;
                    }
                    let how = if blocks == "shots" { &mc["shots"] } else { &mc["pawns"] };
                    if how == "simple" && !mc["simple"].is_null() {
                        Self::agg_parts(&mc["simple"], color, &mut parts);
                    } else if how == "complex" {
                        // The render triangles, untextured.
                        let k = format!("complex|{key}");
                        let mut req = json!({ "path": mc["path"], "lod": 0, "textures": false, "skin": false });
                        match mc["index"].as_u64() {
                            Some(i) => req["index"] = json!(i),
                            None => req["object"] = mc["object"].clone(),
                        }
                        wanted.push((k.clone(), mc["path"].is_string().then_some(req)));
                        parts.push(Part::Mesh { key: k, local: Mat4::IDENTITY, color: rgba(color, 0.32), flat: true, edges: Some(rgba(color, 0.8)) });
                    } else {
                        continue;
                    }
                }
                nodes.push(Node { matrix: super::mat(&it["matrix"]), visible: true, rooftop: false, tint: None, parts });
            }
        }
        for (k, req) in wanted {
            self.want_mesh(&k, req);
        }
        self.view.with(|s| {
            s.layers[1] = nodes;
            s.invalidate();
        });
        self.apply_collision_layers();
    }

    /// A mesh arrived: complex collision drawn with it needs its edges (the layer is rebuilt on the next frame anyway).
    pub(super) fn collision_mesh_loaded(&mut self, key: &str) {
        if key.starts_with("complex|") {
            self.view.with(|s| s.invalidate());
        }
    }

    /// Write a collision switch (`CollideActors`, `BlockActors`…) on an actor or component.
    fn set_flag(&mut self, level: String, index: usize, prop: &'static str, value: bool, cx: &mut Context<Self>) {
        let call = self.call("object.set", json!({ "path": level, "index": index, "prop": prop, "value": if value { "True" } else { "False" } }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(_) => {
                    this.note_edit(&level, cx);
                    this.reload_level(level.clone(), cx);
                    this.load_collision(true, cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not change {prop}: {e}"), cx),
            });
        })
        .detach();
    }

    /// The volume's current brush (Shape panel's Current).
    pub(super) fn read_brush(&mut self, level: String, index: usize, cx: &mut Context<Self>) {
        let call = self.call("map.brush", json!({ "path": level, "index": index }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        // Every distinct corner of its polys, for "Edit vertices".
                        let mut seen = std::collections::HashSet::new();
                        let mut text = String::new();
                        for poly in r.json["polys"].as_array().into_iter().flatten() {
                            for v in poly.as_array().into_iter().flatten() {
                                let c: Vec<f64> = v.as_array().into_iter().flatten().map(|x| x.as_f64().unwrap_or(0.0)).collect();
                                let k = c.iter().map(|x| format!("{x:.2}")).collect::<Vec<_>>().join(",");
                                if c.len() == 3 && seen.insert(k) {
                                    text.push_str(&format!("{} {} {}\n", (c[0] * 100.0).round() / 100.0, (c[1] * 100.0).round() / 100.0, (c[2] * 100.0).round() / 100.0));
                                }
                            }
                        }
                        this.collision.info = Some(r.json);
                        this.collision.vertices = None;
                        this.collision.vertices_text = Some(text);
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not read the volume's shape: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reshape a volume, then redraw its level (and the collision when shown).
    pub(super) fn set_brush(&mut self, level: String, index: usize, brush: J, cx: &mut Context<Self>) {
        self.collision.busy = true;
        let call = self.call("map.setBrush", json!({ "path": level, "index": index, "brush": brush }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.collision.busy = false;
                match r {
                    Ok(r) => {
                        this.note_edit(&level, cx);
                        this.reload_level(level.clone(), cx);
                        if this.collision.enabled {
                            this.load_collision(true, cx);
                        }
                        let hulls = r.json["hulls"].as_u64().unwrap_or(0);
                        this.toast(Tone::Success, format!("Shape rebuilt: {} polys, {hulls} convex hull{}", r.json["polys"], if hulls == 1 { "" } else { "s" }), cx);
                        this.read_brush(level.clone(), index, cx);
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not rebuild the shape: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn apply_shape(&mut self, level: String, index: usize, cx: &mut Context<Self>) {
        let c = &self.collision;
        let brush = match c.kind.as_str() {
            "mesh" => json!({ "kind": "mesh", "mesh": { "object": c.mesh_object.trim() }, "fit": if c.bounds { "bounds" } else { "hull" } }),
            "vertices" => {
                let text = self.collision.vertices.as_ref().map(|t| t.read(cx).value().to_string()).or_else(|| self.collision.vertices_text.clone()).unwrap_or_default();
                let points: Vec<[f64; 3]> = text
                    .lines()
                    .filter_map(|l| {
                        let n: Vec<f64> = l.split([' ', ',', '\t']).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
                        (n.len() == 3).then(|| [n[0], n[1], n[2]])
                    })
                    .collect();
                if points.len() < 4 {
                    self.toast(Tone::Warning, "A hull needs at least 4 points: one \"x y z\" per line", cx);
                    return;
                }
                json!({ "kind": "hull", "points": points })
            }
            kind => {
                let mut b = json!({ "kind": kind });
                for (k, v) in &c.spec {
                    if !k.starts_with('$') {
                        b[k] = json!(if ["sides", "sphereExtrapolation"].contains(&k.as_str()) { v.round() } else { *v });
                    }
                }
                if kind != "sphere" {
                    b["hollow"] = json!(c.hollow);
                }
                if kind == "cylinder" || kind == "cone" {
                    b["alignToSide"] = json!(c.align);
                }
                b
            }
        };
        self.set_brush(level, index, brush, cx);
    }

    pub(super) fn commit_collision_field(&mut self, key: &str, value: String, cx: &mut Context<Self>) {
        if key == "mesh" {
            self.collision.mesh_object = value.trim().to_string();
        } else if let Ok(n) = value.trim().parse::<f64>() {
            self.collision.spec.insert(key.to_string(), n);
        }
        cx.notify();
    }

    // ----- views ----------------------------------------------------------------------

    pub(super) fn render_collision_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let c = &self.collision;
        let mut row = h_flex().gap_1().ml_2().child(
            Button::new("mv-collision")
                .xsmall()
                .label(if c.loading { "Collision…" } else { "Collision" })
                .selected(c.enabled)
                .tooltip("Draw collision: volumes' convex hulls and meshes' simple collision (or their triangles when they use complex collision)")
                .on_click(cx.listener(|this, _, _, cx| {
                    let on = !this.collision.enabled;
                    this.toggle_collision(on, cx);
                })),
        );
        if c.enabled {
            row = row
                .child(Checkbox::new("mv-col-only").label("Only").checked(c.only).tooltip("Hide the render geometry").on_click(cx.listener(|this, v: &bool, _, cx| {
                    this.collision.only = *v;
                    this.apply_collision_layers();
                    cx.notify();
                })))
                .child(Checkbox::new("mv-col-blocking").label("Blocking only").checked(c.hide_none).tooltip("Leave out meshes and volumes that collide with nothing (triggers stay)").on_click(cx.listener(|this, v: &bool, _, cx| {
                    this.collision.hide_none = *v;
                    this.rebuild_collision();
                    cx.notify();
                })))
                .children(BLOCKS.iter().map(|(_, color, label)| div().id(gpui::SharedString::from(*label)).size(px(10.0)).rounded_sm().bg(hsla(*color)).tooltip(move |w, cx| gpui_component::tooltip::Tooltip::new(*label).build(w, cx))));
        }
        let _ = theme;
        row.into_any_element()
    }

    pub(super) fn render_collision_panel(&mut self, level: &str, actor: &Actor, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let is_volume = actor.parts.iter().any(|p| p["kind"] == "brush");
        let items = self.collision.items_of(level, actor.export);
        let mut col = v_flex().p_2().gap_2().border_b_1().border_color(theme.border).text_xs();
        col = col.child(h_flex().gap_2().child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Collision")).when(self.collision.data.is_none(), |d| {
            d.child(Button::new("mi-col-read").xsmall().ghost().label(if self.collision.loading { "Reading…" } else { "Read collision" }).on_click(cx.listener(|this, _, _, cx| this.load_collision(false, cx))))
        }));
        for (n, it) in items.iter().enumerate() {
            let blocks = it["flags"]["blocks"].as_str().unwrap_or("none").to_string();
            let mesh = it["kind"] == "mesh";
            let name = if mesh { it["name"].as_str().unwrap_or("").to_string() } else { actor.name.clone() };
            let mut block = v_flex().gap_1().child(h_flex().gap_2().child(div().size(px(10.0)).rounded_sm().bg(hsla(block_color(&blocks)))).child(div().font_family("Menlo").child(name)).child(div().text_color(theme.muted_foreground).child(block_label(&blocks))));
            if mesh {
                let key = it["meshKey"].as_str().unwrap_or("");
                if let Some(mc) = self.collision.data.as_ref().map(|d| &d["meshes"][key]).filter(|m| !m.is_null()) {
                    let badge = |t: String| div().px_1().rounded_sm().bg(theme.secondary).child(t);
                    block = block.child(h_flex().gap_1().child(badge(format!("pawns: {}", mc["pawns"].as_str().unwrap_or("")))).child(badge(format!("shots: {}", mc["shots"].as_str().unwrap_or("")))).when(mc["bodySetup"] == false, |d| d.child(div().px_1().rounded_sm().bg(theme.warning).text_color(theme.warning_foreground).child("no body setup"))));
                }
            }
            let mut grid = div().grid().grid_cols(2).gap_x_2().gap_y_0p5();
            for (fk, (flag, help)) in FLAGS.iter().enumerate() {
                let Some(v) = it["flags"][*flag].as_bool() else { continue };
                // Actor switches (b…) live on the actor or the collection; the others on the component.
                let index = if flag.starts_with('b') { if mesh { it["parent"].as_u64() } else { it["export"].as_u64() } } else if mesh { it["export"].as_u64() } else { it["component"].as_u64() }.unwrap_or(0) as usize;
                let lvl = level.to_string();
                grid = grid.child(h_flex().gap_1().child(Switch::new(("mi-flag", n * 16 + fk)).xsmall().checked(v).tooltip(*help).on_click(cx.listener(move |this, v: &bool, _, cx| this.set_flag(lvl.clone(), index, flag, *v, cx)))).child(div().font_family("Menlo").child(flag.to_string())));
            }
            col = col.child(block.child(grid));
        }
        if is_volume {
            col = col.child(self.render_shape_panel(level, actor.export, window, cx));
        }
        col.into_any_element()
    }

    fn render_shape_panel(&mut self, level: &str, export: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let kind = self.collision.kind.clone();
        let label = match kind.as_str() {
            "mesh" => "From a mesh",
            "vertices" => "Edit vertices",
            k => BUILDERS.iter().find(|b| b.0 == k).map(|b| b.1).unwrap_or("Box"),
        };
        let me = cx.entity().downgrade();
        let menu = Button::new("mi-shape-kind").xsmall().label(label).dropdown_menu(move |menu, _, _| {
            let mut menu = menu;
            for (k, l) in [("box", "Box"), ("cylinder", "Cylinder"), ("cone", "Cone"), ("sphere", "Sphere"), ("mesh", "From a mesh"), ("vertices", "Edit vertices")] {
                let me = me.clone();
                menu = menu.item(PopupMenuItem::new(l).on_click(move |_, _, cx| {
                    let _ = me.update(cx, |this, cx| {
                        this.collision.pick(k);
                        cx.notify();
                    });
                }));
            }
            menu
        });
        let (l1, l2) = (level.to_string(), level.to_string());
        let info = self.collision.info.as_ref().map(|i| {
            let hulls = i["hulls"].as_array().map(|a| a.len()).unwrap_or(0);
            format!("{} polys, {hulls} hull{}{}", i["polys"].as_array().map(|a| a.len()).unwrap_or(0), if hulls == 1 { "" } else { "s" }, if i["fromBrush"] == false { " (cooked: no brush)" } else { "" })
        });
        let mut panel = v_flex().gap_1().child(
            h_flex()
                .gap_2()
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Shape"))
                .child(menu)
                .child(Button::new("mi-shape-current").xsmall().ghost().label("Current").tooltip("Read the current brush").on_click(cx.listener(move |this, _, _, cx| this.read_brush(l1.clone(), export, cx))))
                .children(info.map(|i| div().text_color(theme.muted_foreground).child(i))),
        );
        if let Some((_, _, fields)) = BUILDERS.iter().find(|b| b.0 == kind) {
            let mut grid = div().grid().grid_cols(2).gap_1();
            for (k, label, _) in fields.iter() {
                let v = self.collision.spec.get(*k).copied().unwrap_or(0.0);
                grid = grid.child(h_flex().gap_1().child(div().w(px(80.0)).text_color(theme.muted_foreground).child(label.to_string())).child(self.field(format!("col:{k}"), format!("{v}"), 70.0, cx)));
            }
            panel = panel.child(grid).child(
                h_flex()
                    .gap_3()
                    .when(kind != "sphere", |d| {
                        d.child(Checkbox::new("mi-hollow").label("Hollow").checked(self.collision.hollow).on_click(cx.listener(|this, v: &bool, _, cx| {
                            this.collision.hollow = *v;
                            cx.notify();
                        })))
                    })
                    .when(kind == "cylinder" || kind == "cone", |d| {
                        d.child(Checkbox::new("mi-align").label("Align to side").checked(self.collision.align).on_click(cx.listener(|this, v: &bool, _, cx| {
                            this.collision.align = *v;
                            cx.notify();
                        })))
                    }),
            );
        } else if kind == "mesh" {
            panel = panel.child(h_flex().gap_1().child(div().text_color(theme.muted_foreground).child("Static mesh")).child(self.field("col:mesh", self.collision.mesh_object.clone(), 220.0, cx))).child(Checkbox::new("mi-bounds").label("Bounding box instead of the convex hull").checked(self.collision.bounds).on_click(cx.listener(|this, v: &bool, _, cx| {
                this.collision.bounds = *v;
                cx.notify();
            })));
        } else {
            if self.collision.vertices.is_none() {
                let text = self.collision.vertices_text.clone().unwrap_or_default();
                self.collision.vertices = Some(cx.new(|cx| {
                    let mut s = TextareaState::new(window, cx).rows(8);
                    s.set_value(text, window, cx);
                    s
                }));
            }
            let t = self.collision.vertices.clone().expect("made above");
            panel = panel.child(div().text_color(theme.muted_foreground).child("The volume becomes the convex hull of these points: one \"x y z\" per line, actor space, Unreal units. Read the current shape first.")).child(Textarea::new(&t).h(px(140.0)).font_family("Menlo"));
        }
        let disabled = self.collision.busy || (kind == "mesh" && self.collision.mesh_object.trim().is_empty());
        panel.child(h_flex().child(Button::new("mi-shape-apply").xsmall().primary().label(if self.collision.busy { "Building…" } else { "Apply shape" }).disabled(disabled).on_click(cx.listener(move |this, _, _, cx| this.apply_shape(l2.clone(), export, cx))))).into_any_element()
    }
}
