//! Particle systems: a ParticleSystem, emitter, LOD level or module export
//! previews the whole system (`particle.system`) in the 3D view, simulated
//! on the CPU (`crate::particles`, drawn by `crate::particle_view`): play,
//! pause, restart, speed, loop, grid, fit. Below it the emitters (shown or
//! hidden, particle counts, the modules the preview does not simulate),
//! the modules of the chosen emitter, and the chosen module's baked
//! distributions as curves: drag a point, type a value, the number of
//! points, the time scale and start; the preview follows the draft, Apply
//! writes the table (`particle.setTable`, one undo step, saved with the package).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use gpui::{canvas, div, fill, point, prelude::*, px, size, AnyElement, Bounds, Context, Hsla, MouseButton, PathBuilder, Pixels};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Sizable};
use serde_json::{json, Value as J};

use super::mesh::{viewport, MeshState};
use super::{Studio, Tone};
use crate::particle_view::{Blend, Material, ParticleScene};
use crate::particles::{self, build_emitter, Dist, Drafts};

/// `editing` keys of the curve fields (the shared value input).
pub const FIELD: &str = "particle:";

pub fn is_particle(class: &str) -> bool {
    let c = class.to_ascii_lowercase();
    c.starts_with("particle") && c != "particlesystemcomponent"
}

/// A point being dragged: which curve, which value, and the range frozen at the start.
#[derive(Clone)]
struct Drag {
    key: (usize, String),
    entry: usize,
    k: usize,
    lo: f32,
    hi: f32,
}

/// Where each curve was drawn, by (module, property): mouse events find their points with it.
type Graphs = Rc<RefCell<HashMap<(usize, String), Bounds<Pixels>>>>;

pub struct ParticleState {
    path: String,
    /// The system as `particle.system` returned it (module JSON replaced as edits apply).
    system: J,
    /// Every export of the system: selecting one of them keeps the preview.
    members: HashSet<usize>,
    view: Option<Rc<RefCell<MeshState<ParticleScene>>>>,
    note: String,
    playing: bool,
    speed: f32,
    looping: bool,
    /// The loop driving playback: a new one replaces the old.
    generation: u64,
    /// Simulated time since the last restart, to frame the effect once it has started.
    sim_time: f32,
    fitted: bool,
    emitter: Option<usize>,
    module: Option<usize>,
    pub drafts: Drafts,
    /// The curve whose fields are shown, and its chosen point (entry, value).
    focus: Option<(usize, String)>,
    point: Option<(usize, usize)>,
    drag: Option<Drag>,
    graphs: Graphs,
}

impl ParticleState {
    fn emitters(&self) -> &[J] {
        self.system["emitters"].as_array().map(|a| a.as_slice()).unwrap_or_default()
    }

    fn emitter_json(&self) -> Option<&J> {
        self.emitters().iter().find(|e| e["index"].as_u64().map(|i| i as usize) == self.emitter)
    }

    fn module_json(&self, index: usize) -> Option<&J> {
        self.emitters().iter().flat_map(particles::modules).map(|(m, _)| m).find(|m| m["index"].as_u64() == Some(index as u64))
    }

    /// The stored distribution of a curve (what Revert goes back to).
    fn stored(&self, key: &(usize, String)) -> Option<Dist> {
        let m = self.module_json(key.0)?;
        particles::prop(m, &key.1).filter(|p| particles::is_dist(p)).map(particles::decode)
    }

    /// The curve as shown: the draft, else what is stored.
    fn dist(&self, key: &(usize, String)) -> Option<Dist> {
        self.drafts.get(key).cloned().or_else(|| self.stored(key))
    }

    /// Point the panel at export `i`: its emitter, and its module when it is one.
    fn select(&mut self, i: usize) {
        let mut emitter = None;
        let mut module = None;
        for e in self.emitters() {
            let ei = e["index"].as_u64().unwrap_or(0) as usize;
            let mods = particles::modules(e);
            if ei == i || e["lods"][0]["index"].as_u64() == Some(i as u64) {
                emitter = Some(ei);
            }
            if mods.iter().any(|(m, _)| m["index"].as_u64() == Some(i as u64)) {
                emitter = Some(ei);
                module = Some(i);
            }
        }
        self.emitter = emitter.or_else(|| self.emitters().first().and_then(|e| e["index"].as_u64()).map(|i| i as usize));
        if module.is_some() || emitter.is_some() {
            self.module = module;
        }
    }

    /// Give the simulation the models as the system and the drafts say now.
    fn rebuild(&mut self) {
        let Some(v) = &self.view else { return };
        let mut v = v.borrow_mut();
        let emitters = self.system["emitters"].as_array().cloned().unwrap_or_default();
        for (r, e) in v.view.runtimes.iter_mut().zip(&emitters) {
            r.sim.model = build_emitter(e, &self.drafts);
        }
        v.dirty = true;
    }

    fn summary(&self) -> String {
        let n = self.view.as_ref().map(|v| v.borrow().view.count()).unwrap_or(0);
        format!("{n} particles · {} emitters · approximate preview · drag to orbit, wheel to zoom, double-click to fit", self.emitters().len())
    }
}

/// The value range a curve is drawn over (padded; a flat curve gets room around it).
fn range(d: &Dist) -> (f32, f32) {
    let (lo, hi) = d.entries.iter().flatten().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    if !lo.is_finite() {
        return (0.0, 1.0);
    }
    if hi - lo < 1e-6 {
        let pad = (hi.abs() * 0.5).max(1.0);
        return (lo - pad, hi + pad);
    }
    let pad = (hi - lo) * 0.15;
    (lo - pad, hi + pad)
}

fn fmt(v: f32) -> String {
    if v.abs() >= 100.0 {
        format!("{v:.0}")
    } else if v.abs() >= 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.3}")
    }
}

/// Where entry `i` of `n` and value `v` sit in a graph `b` drawn over `lo..hi`.
fn graph_x(b: &Bounds<Pixels>, i: usize, n: usize) -> f32 {
    let w = f32::from(b.size.width);
    f32::from(b.origin.x) + 28.0 + (w - 36.0) * if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 }
}

fn graph_y(b: &Bounds<Pixels>, v: f32, lo: f32, hi: f32) -> f32 {
    let h = f32::from(b.size.height);
    f32::from(b.origin.y) + h - 10.0 - (h - 20.0) * (v - lo) / (hi - lo)
}

fn graph_value(b: &Bounds<Pixels>, y: f32, lo: f32, hi: f32) -> f32 {
    let h = f32::from(b.size.height);
    let v = lo + (f32::from(b.origin.y) + h - 10.0 - y) / (h - 20.0) * (hi - lo);
    (v * 1000.0).round() / 1000.0
}

fn short_class(c: &str) -> String {
    let l = c.to_ascii_lowercase();
    let s = l.strip_prefix("particlemodule").unwrap_or(&l);
    let s = s.strip_prefix("typedata").map(|t| format!("type: {t}")).unwrap_or_else(|| s.to_string());
    if s.is_empty() { c.to_string() } else { s }
}

impl Studio {
    /// Show the particle system holding export `i` (kept when `i` is part of the one shown).
    pub(super) fn load_particles(&mut self, path: String, i: usize, cx: &mut Context<Self>) {
        if let Some(p) = self.particles.as_mut().filter(|p| p.path == path && p.members.contains(&i)) {
            p.select(i);
            cx.notify();
            return;
        }
        let call = self.backend.call("particle.system", json!({ "path": path, "index": i, "textureSize": 256 }));
        self.particles = Some(ParticleState {
            path: path.clone(),
            system: J::Null,
            members: HashSet::from([i]),
            view: None,
            note: "Reading the particle system…".into(),
            playing: true,
            speed: 1.0,
            looping: true,
            generation: 0,
            sim_time: 0.0,
            fitted: false,
            emitter: None,
            module: None,
            drafts: Drafts::new(),
            focus: None,
            point: None,
            drag: None,
            graphs: Default::default(),
        });
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let r = match call.await {
                Ok(r) => r,
                Err(e) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(p) = this.particles.as_mut().filter(|p| p.members.contains(&i)) {
                            p.note = format!("Particle system: {e}");
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            let system = r.json.clone();
            let materials: HashMap<String, Material> = system["materials"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), Material { blend: Blend::of(v["blendMode"].as_str().unwrap_or_default()), png: r.bin(&v["png"]).map(|b| b.to_vec()) })).collect()).unwrap_or_default();
            // Meshes of mesh emitters, wherever they live.
            let mut meshes = HashMap::new();
            for (mpath, at) in system["meshes"].as_object().cloned().unwrap_or_default() {
                let (Some(file), Some(index)) = (at["path"].as_str().filter(|f| !f.is_empty()), at["index"].as_u64()) else { continue };
                if let Ok(m) = backend.call("mesh.preview", json!({ "path": file, "index": index, "textureSize": 512 })).await {
                    if let Some(glb) = m.bin(&m.json["glb"]) {
                        meshes.insert(mpath, glb.to_vec());
                    }
                }
            }
            let models: Vec<_> = system["emitters"].as_array().map(|a| a.iter().map(|e| build_emitter(e, &Drafts::new())).collect()).unwrap_or_default();
            let busy = crate::backend::Busy::new();
            let built = cx.background_executor().spawn(async move { ParticleScene::new(models, &materials, &meshes) }).await;
            let _ = this.update(cx, |this, cx| {
                drop(busy);
                let Some(p) = this.particles.as_mut().filter(|p| p.path == path && p.members.contains(&i)) else { return };
                let mut members: HashSet<usize> = system["root"].as_u64().map(|r| r as usize).into_iter().collect();
                for e in system["emitters"].as_array().into_iter().flatten() {
                    members.extend(e["index"].as_u64().map(|i| i as usize));
                    for lod in e["lods"].as_array().into_iter().flatten() {
                        members.extend(lod["index"].as_u64().map(|i| i as usize));
                        for k in ["required", "spawn", "typeData"] {
                            members.extend(lod[k]["index"].as_u64().map(|i| i as usize));
                        }
                        members.extend(lod["modules"].as_array().into_iter().flatten().filter_map(|m| m["index"].as_u64()).map(|i| i as usize));
                    }
                }
                p.members = members;
                p.system = system;
                p.select(i);
                match built {
                    Ok(scene) => {
                        p.view = Some(Rc::new(RefCell::new(MeshState::new(i, 0, scene, String::new()))));
                        p.note.clear();
                    }
                    Err(e) => p.note = e,
                }
                this.particles_play(true, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Start (or stop) the playback loop: a step every frame at the chosen speed.
    fn particles_play(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(p) = &mut self.particles else { return };
        p.playing = on;
        p.generation += 1;
        if !on {
            cx.notify();
            return;
        }
        let generation = p.generation;
        cx.spawn(async move |this, cx| {
            let mut last = Instant::now();
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                let go = this.update(cx, |this, cx| {
                    let Some(p) = this.particles.as_mut().filter(|p| p.generation == generation && p.playing) else { return false };
                    let dt = last.elapsed().as_secs_f32().min(0.05) * p.speed;
                    last = Instant::now();
                    this.particles_step(dt);
                    cx.notify();
                    true
                });
                if !matches!(go, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Advance the simulation by `dt` seconds (restart one-shot effects, frame the effect once).
    fn particles_step(&mut self, dt: f32) {
        let Some(p) = &mut self.particles else { return };
        let Some(v) = &p.view else { return };
        {
            let mut v = v.borrow_mut();
            v.view.step(dt);
            p.sim_time += dt;
            if p.looping && v.view.finished() {
                v.view.restart();
            }
            if !p.fitted && p.sim_time >= 0.6 {
                v.fit();
                p.fitted = true;
            }
            v.dirty = true;
        }
        let summary = p.summary();
        v.borrow_mut().summary = summary;
    }

    fn particles_restart(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = self.particles.as_ref().and_then(|p| p.view.clone()) {
            let mut v = v.borrow_mut();
            v.view.restart();
            v.dirty = true;
        }
        cx.notify();
    }

    /// Change a curve's draft (the preview follows it).
    fn particles_draft(&mut self, key: (usize, String), d: Dist, cx: &mut Context<Self>) {
        let Some(p) = &mut self.particles else { return };
        if p.stored(&key).as_ref() == Some(&d) {
            p.drafts.remove(&key);
        } else {
            p.drafts.insert(key, d);
        }
        p.rebuild();
        cx.notify();
    }

    /// Write a curve's draft into its module (`particle.setTable`).
    fn particles_apply(&mut self, key: (usize, String), cx: &mut Context<Self>) {
        let Some(p) = &self.particles else { return };
        let Some(d) = p.drafts.get(&key).cloned() else { return };
        let path = p.path.clone();
        let call = self.backend.call("particle.setTable", json!({ "path": path, "index": key.0, "prop": d.prop, "table": d.table(), "timeScale": d.time_scale, "startTime": d.start_time }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.dirty.insert(path.clone());
                    if let Some(p) = this.particles.as_mut().filter(|p| p.path == path) {
                        // The module as stored now replaces its JSON; the draft is done.
                        if let Some(emitters) = p.system["emitters"].as_array_mut() {
                            for e in emitters {
                                for lod in e["lods"].as_array_mut().into_iter().flatten() {
                                    for k in ["required", "spawn", "typeData"] {
                                        if lod[k]["index"].as_u64() == Some(key.0 as u64) {
                                            lod[k] = r.json.clone();
                                        }
                                    }
                                    for m in lod["modules"].as_array_mut().into_iter().flatten() {
                                        if m["index"].as_u64() == Some(key.0 as u64) {
                                            *m = r.json.clone();
                                        }
                                    }
                                }
                            }
                        }
                        p.drafts.remove(&key);
                        p.rebuild();
                    }
                    this.set_status(format!("{} updated (unsaved): save the package to keep it", d.prop), cx);
                    // The inspector shows the module's properties: read them again.
                    if this.selected_export == Some(key.0) {
                        this.select_export(key.0, cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("{} not applied: {e}", d.prop), cx),
            });
        })
        .detach();
    }

    /// A typed curve field (`particle:value|points|timescale|start`).
    pub(super) fn commit_particle_field(&mut self, field: &str, value: String, cx: &mut Context<Self>) {
        let Some(p) = &self.particles else { return };
        let Some(key) = p.focus.clone() else { return };
        let Some(mut d) = p.dist(&key) else { return };
        let Ok(v) = value.trim().parse::<f32>() else {
            self.toast(Tone::Warning, "That takes a number", cx);
            return;
        };
        match field {
            "value" => {
                let Some((e, k)) = p.point else { return };
                if let Some(x) = d.entries.get_mut(e).and_then(|entry| entry.get_mut(k)) {
                    *x = v;
                }
            }
            "points" => d.resample(v.round().max(2.0) as usize),
            "timescale" => d.time_scale = v,
            "start" => d.start_time = v,
            _ => return,
        }
        if let Some(p) = &mut self.particles {
            if field == "points" {
                p.point = None;
            }
        }
        self.particles_draft(key, d, cx);
    }

    pub(super) fn render_particles(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let p = self.particles.as_ref().filter(|p| self.selected_export.is_some_and(|i| p.members.contains(&i)))?;
        let view = match &p.view {
            Some(v) => {
                let (playing, looping, grid, speed) = (p.playing, p.looping, v.borrow().view.show_grid, p.speed);
                let toggle = |id: &'static str, label: &'static str, on: bool| {
                    let b = Button::new(id).xsmall().label(label);
                    if on { b.primary() } else { b.ghost() }
                };
                let (v1, v2, me) = (v.clone(), v.clone(), cx.entity().downgrade());
                let bar = h_flex()
                    .gap_1()
                    .child(Button::new("pt-play").xsmall().primary().label(if playing { "Pause" } else { "Play" }).on_click(cx.listener(move |this, _, _, cx| this.particles_play(!playing, cx))))
                    .child(Button::new("pt-restart").xsmall().ghost().label("Restart").tooltip("Every emitter from the start").on_click(cx.listener(|this, _, _, cx| this.particles_restart(cx))))
                    .child(Button::new("pt-speed").xsmall().ghost().label(format!("{speed}×")).dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        for s in [0.1f32, 0.25, 0.5, 1.0, 2.0] {
                            let label = format!("{s}×");
                            let me = me.clone();
                            menu = menu.item(PopupMenuItem::new(label).checked(s == speed).on_click(move |_, _, cx| {
                                let _ = me.update(cx, |this, cx| {
                                    if let Some(p) = &mut this.particles {
                                        p.speed = s;
                                    }
                                    cx.notify();
                                });
                            }));
                        }
                        menu
                    }))
                    .child(toggle("pt-loop", "Loop", looping).tooltip("Start one-shot effects again when they finish").on_click(cx.listener(|this, _, _, cx| {
                        if let Some(p) = &mut this.particles {
                            p.looping = !p.looping;
                        }
                        cx.notify();
                    })))
                    .child(toggle("pt-grid", "Grid", grid).on_click(move |_, window, _| {
                        let mut v = v1.borrow_mut();
                        v.view.show_grid = !v.view.show_grid;
                        v.dirty = true;
                        window.refresh();
                    }))
                    .child(Button::new("pt-fit").xsmall().ghost().label("Fit").tooltip("Frame the particles").on_click(move |_, window, _| {
                        v2.borrow_mut().fit();
                        window.refresh();
                    }));
                viewport("particle-view", v.clone(), 420.0, Some(bar.into_any_element()), theme.muted, theme.muted_foreground)
            }
            None => div().h(px(420.0)).w_full().rounded_md().bg(theme.muted).flex().items_center().justify_center().text_sm().text_color(theme.muted_foreground).child(p.note.clone()).into_any_element(),
        };
        let section = |t: &'static str| div().pt_1().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(t);
        let mut out = v_flex().gap_1().child(view);
        if p.view.is_none() {
            return Some(out.into_any_element());
        }
        // Emitters, in a list of their own so the modules and curves stay in view.
        let shown = p.view.as_ref().map(|v| v.borrow().view.runtimes.iter().filter(|r| r.visible).count()).unwrap_or(0);
        out = out.child(section("EMITTERS"));
        let mut emitters = v_flex().id("pt-emitters").max_h(px(200.0)).overflow_y_scroll();
        let counts: HashMap<usize, (usize, bool)> = p.view.as_ref().map(|v| v.borrow().view.runtimes.iter().map(|r| (r.sim.model.index, (r.sim.particles.len(), r.visible))).collect()).unwrap_or_default();
        for (k, e) in p.emitters().iter().enumerate() {
            let ei = e["index"].as_u64().unwrap_or(0) as usize;
            let model = p.view.as_ref().and_then(|v| v.borrow().view.runtimes.get(k).map(|r| r.sim.model.clone()));
            let (count, visible) = counts.get(&ei).copied().unwrap_or((0, true));
            let selected = p.emitter == Some(ei);
            let name = model.as_ref().map(|m| m.name.clone()).unwrap_or_default();
            let kind = match &model {
                Some(m) if m.mesh.is_some() => "mesh",
                _ => "sprite",
            };
            let off = model.as_ref().is_some_and(|m| !m.enabled);
            let row = h_flex()
                .id(("pt-emitter", k))
                .gap_2()
                .px_1()
                .rounded_sm()
                .text_sm()
                .when(selected, |d| d.bg(theme.primary.opacity(0.15)))
                .hover(|d| d.bg(theme.secondary))
                .child(Checkbox::new(("pt-show", k)).checked(visible).on_click(cx.listener(move |this, v: &bool, _, cx| {
                    if let Some(view) = this.particles.as_ref().and_then(|p| p.view.clone()) {
                        let mut view = view.borrow_mut();
                        if let Some(r) = view.view.runtimes.get_mut(k) {
                            r.visible = *v;
                        }
                        view.dirty = true;
                    }
                    cx.notify();
                })))
                .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name))
                .children(model.as_ref().filter(|m| !m.unsupported.is_empty()).map(|m| {
                    let text = format!("Not simulated here: {}", m.unsupported.iter().map(|c| short_class(c)).collect::<Vec<_>>().join(", "));
                    div().id(("pt-unsupported", k)).px_1().rounded_sm().text_xs().text_color(theme.warning).bg(theme.warning.opacity(0.12)).child("partial").tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx))
                }))
                .child(div().text_xs().text_color(theme.muted_foreground).child(if off { format!("{kind} · off") } else { kind.to_string() }))
                .child(div().w(px(48.0)).text_right().text_xs().font_family("Menlo").text_color(theme.muted_foreground).child(count.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(p) = &mut this.particles {
                        p.emitter = Some(ei);
                        p.module = None;
                    }
                    cx.notify();
                }));
            emitters = emitters.child(row);
        }
        out = out.child(emitters).child(div().text_xs().text_color(theme.muted_foreground).child(format!("{shown} of {} shown · the box hides an emitter from the preview only", p.emitters().len())));
        // The chosen emitter's modules.
        if let Some(e) = p.emitter_json() {
            out = out.child(section("MODULES"));
            let mut list = h_flex().flex_wrap().gap_1();
            for (m, role) in particles::modules(e) {
                let mi = m["index"].as_u64().unwrap_or(0) as usize;
                let class = short_class(m["class"].as_str().unwrap_or_default());
                let label = if role.is_empty() || role == class { class } else { format!("{class} ({role})") };
                let edited = p.drafts.keys().any(|(k, _)| *k == mi);
                let b = Button::new(("pt-module", mi)).xsmall().label(if edited { format!("{label} •") } else { label }).tooltip("Double-click: show it in the package");
                let b = if p.module == Some(mi) { b.primary() } else { b.ghost() };
                list = list.child(b.on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                    if e.click_count() == 2 {
                        this.reveal(mi, cx);
                        this.select_export(mi, cx);
                        return;
                    }
                    if let Some(p) = &mut this.particles {
                        p.module = if p.module == Some(mi) { None } else { Some(mi) };
                    }
                    cx.notify();
                })));
            }
            out = out.child(list);
        }
        // The chosen module's curves.
        if let Some(m) = p.module.and_then(|mi| p.module_json(mi)) {
            let mi = m["index"].as_u64().unwrap_or(0) as usize;
            let dists = particles::module_dists(m, &p.drafts);
            if dists.is_empty() {
                out = out.child(div().text_xs().text_color(theme.muted_foreground).child("No distribution on this module; its other properties are edited in the package (double-click the module)."));
            }
            for d in dists {
                let key = (mi, d.prop.clone());
                out = out.child(self.render_curve(key, d, cx));
            }
        }
        Some(out.into_any_element())
    }

    fn render_curve(&self, key: (usize, String), d: Dist, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let p = self.particles.as_ref().expect("particles shown");
        let dirty = p.drafts.contains_key(&key);
        let focused = p.focus.as_ref() == Some(&key);
        let colours: [Hsla; 3] = [gpui::rgb(0xf38ba8).into(), gpui::rgb(0xa6e3a1).into(), gpui::rgb(0x89b4fa).into()];
        let kind = if d.vector { "vector" } else { "float" };
        let op = match d.op {
            1 => " · random between A and B",
            2 => " · A or B",
            _ => "",
        };
        let shape = if d.is_curve() { format!(" · {} points over {:.2} s from {}", d.entries.len(), d.span(), d.start_time) } else { " · constant".to_string() };
        let (lo, hi) = match &p.drag {
            Some(g) if g.key == key => (g.lo, g.hi),
            _ => range(&d),
        };
        let chosen = if focused { p.point } else { None };
        let (k1, k2, k3) = (key.clone(), key.clone(), key.clone());
        let header = h_flex()
            .gap_2()
            .child(div().text_sm().font_family("Menlo").font_weight(gpui::FontWeight::SEMIBOLD).child(d.prop.clone()))
            .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().text_color(theme.muted_foreground).child(format!("{kind}{op}{shape}")))
            .when(dirty, |h| {
                h.child(Button::new(("pt-revert", key.0 * 64 + key.1.len())).xsmall().ghost().label("Revert").tooltip("Back to the stored values").on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(p) = &mut this.particles {
                        p.drafts.remove(&k1);
                        p.point = None;
                        p.rebuild();
                    }
                    cx.notify();
                })))
                .child(Button::new(("pt-apply", key.0 * 64 + key.1.len())).xsmall().primary().label("Apply").tooltip("Write it into the module (undoable, saved with the package)").on_click(cx.listener(move |this, _, _, cx| this.particles_apply(k2.clone(), cx))))
            });
        // The graph: a line per value, a dot per entry; drag a dot to change it.
        let graphs = p.graphs.clone();
        let (gk, draw) = (key.clone(), d.clone());
        let (grid_c, zero_c, focus_c) = (theme.border, theme.muted_foreground.opacity(0.6), theme.foreground);
        let graph = canvas(
            move |b, _, _| {
                graphs.borrow_mut().insert(gk.clone(), b);
                b
            },
            move |_, b, window, _| {
                let n = draw.entries.len();
                let y = |v: f32| graph_y(&b, v, lo, hi);
                let (l, r) = (f32::from(b.origin.x) + 28.0, f32::from(b.origin.x) + f32::from(b.size.width) - 8.0);
                for v in [lo, (lo + hi) / 2.0, hi] {
                    window.paint_quad(fill(Bounds::new(point(px(l), px(y(v))), size(px(r - l), px(1.0))), grid_c));
                }
                if lo < 0.0 && hi > 0.0 {
                    window.paint_quad(fill(Bounds::new(point(px(l), px(y(0.0))), size(px(r - l), px(1.0))), zero_c));
                }
                let values = if draw.ranged() { 2 } else { 1 };
                for v in 0..values {
                    for c in 0..draw.comps {
                        let k = v * draw.comps + c;
                        let colour = colours[c % 3].opacity(if v == 1 { 0.55 } else { 1.0 });
                        if n > 1 {
                            let mut path = PathBuilder::stroke(px(1.5));
                            for i in 0..n {
                                let pt = point(px(graph_x(&b, i, n)), px(y(draw.entries[i][k])));
                                if i == 0 { path.move_to(pt) } else { path.line_to(pt) }
                            }
                            if let Ok(path) = path.build() {
                                window.paint_path(path, colour);
                            }
                        }
                        for i in 0..n {
                            let (x, yy) = (graph_x(&b, i, n), y(draw.entries[i][k]));
                            let r = if chosen == Some((i, k)) { 4.5 } else { 3.0 };
                            if chosen == Some((i, k)) {
                                window.paint_quad(fill(Bounds::new(point(px(x - r - 1.5), px(yy - r - 1.5)), size(px(2.0 * r + 3.0), px(2.0 * r + 3.0))), focus_c).corner_radii(px(r + 1.5)));
                            }
                            window.paint_quad(fill(Bounds::new(point(px(x - r), px(yy - r)), size(px(2.0 * r), px(2.0 * r))), colour).corner_radii(px(r)));
                        }
                    }
                }
            },
        )
        .size_full();
        let (dk, mk) = (key.clone(), key.clone());
        let graph_box = div()
            .id(("pt-graph", key.0 * 64 + key.1.len()))
            .relative()
            .h(px(120.0))
            .w_full()
            .rounded_md()
            .bg(theme.muted)
            .cursor_crosshair()
            .child(graph)
            .child(div().absolute().top(px(2.0)).left(px(4.0)).text_xs().text_color(theme.muted_foreground).child(fmt(hi)))
            .child(div().absolute().bottom(px(2.0)).left(px(4.0)).text_xs().text_color(theme.muted_foreground).child(fmt(lo)))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                let Some(p) = &mut this.particles else { return };
                let Some(b) = p.graphs.borrow().get(&dk).copied() else { return };
                let Some(d) = p.dist(&dk) else { return };
                let (lo, hi) = range(&d);
                let n = d.entries.len();
                let (mx, my) = (f32::from(e.position.x), f32::from(e.position.y));
                let mut best: Option<(usize, usize, f32)> = None;
                for i in 0..n {
                    for k in 0..d.chunk {
                        let dist = (graph_x(&b, i, n) - mx).hypot(graph_y(&b, d.entries[i][k], lo, hi) - my);
                        if dist < 9.0 && best.is_none_or(|(_, _, bd)| dist < bd) {
                            best = Some((i, k, dist));
                        }
                    }
                }
                p.focus = Some(dk.clone());
                p.point = best.map(|(i, k, _)| (i, k));
                p.drag = best.map(|(entry, k, _)| Drag { key: dk.clone(), entry, k, lo, hi });
                cx.notify();
            }))
            .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
                let Some(p) = &this.particles else { return };
                let Some(g) = p.drag.clone().filter(|g| g.key == mk) else { return };
                if !e.dragging() {
                    if let Some(p) = &mut this.particles {
                        p.drag = None;
                    }
                    return;
                }
                let Some(b) = p.graphs.borrow().get(&mk).copied() else { return };
                let Some(mut d) = p.dist(&mk) else { return };
                let v = graph_value(&b, f32::from(e.position.y), g.lo, g.hi);
                if let Some(x) = d.entries.get_mut(g.entry).and_then(|en| en.get_mut(g.k)) {
                    *x = v;
                }
                this.particles_draft(mk.clone(), d, cx);
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if let Some(p) = &mut this.particles {
                    p.drag = None;
                }
                cx.notify();
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| {
                if let Some(p) = &mut this.particles {
                    p.drag = None;
                }
            }));
        let mut card = v_flex().gap_1().p_2().rounded_md().border_1().border_color(if focused { theme.primary.opacity(0.6) } else { theme.border }).child(header).child(graph_box);
        // The fields of the curve last touched: the chosen point, points, time scale, start.
        if focused {
            let field = |id: &'static str, label: String, value: String, editable: bool, me: &Studio, cx: &mut Context<Studio>| -> AnyElement {
                let ekey = format!("{FIELD}{id}");
                let shown: AnyElement = if me.editing.as_deref() == Some(ekey.as_str()) {
                    div().w(px(80.0)).child(Input::new(&me.value_input).xsmall()).into_any_element()
                } else {
                    let v = value.clone();
                    div()
                        .id(id)
                        .px_1()
                        .rounded_sm()
                        .font_family("Menlo")
                        .when(editable, |d| d.bg(theme.secondary).cursor_text())
                        .child(value)
                        .when(editable, |d| d.on_click(cx.listener(move |this, _, window, cx| this.start_edit(ekey.clone(), v.clone(), window, cx))))
                        .into_any_element()
                };
                h_flex().gap_1().text_xs().child(div().text_color(theme.muted_foreground).child(label)).child(shown).into_any_element()
            };
            let mut fields = h_flex().flex_wrap().gap_3();
            if let Some((i, k)) = chosen.filter(|(i, k)| d.entries.get(*i).is_some_and(|e| *k < e.len())) {
                fields = fields.child(field("value", format!("#{i} {}", d.label(k)), d.entries[i][k].to_string(), true, self, cx));
            } else {
                fields = fields.child(div().text_xs().text_color(theme.muted_foreground).child("Click a point to type its value"));
            }
            fields = fields
                .child(field("points", "points".into(), d.entries.len().to_string(), true, self, cx))
                .child(field("timescale", "time scale".into(), d.time_scale.to_string(), true, self, cx))
                .child(field("start", "start".into(), d.start_time.to_string(), true, self, cx));
            card = card.child(fields).child(div().text_xs().text_color(theme.muted_foreground).child("Rotations: 1 = 360°. Sizes and positions in Unreal units. Entries are spread over the time span; life-relative curves run from 0 to 1."));
        } else {
            card = card.child(div().id(("pt-focus", k3.0 * 64 + k3.1.len())).text_xs().text_color(theme.muted_foreground).cursor_pointer().child("Click the graph to edit values, points and timing").on_click(cx.listener(move |this, _, _, cx| {
                if let Some(p) = &mut this.particles {
                    p.focus = Some(k3.clone());
                    p.point = None;
                }
                cx.notify();
            })));
        }
        card.into_any_element()
    }

    /// Dev: `particles <step>` of the test driver.
    pub(super) fn drive_particles(&mut self, arg: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let (verb, rest) = arg.split_once(' ').unwrap_or((arg, ""));
        let words: Vec<&str> = rest.split_whitespace().collect();
        let n = |k: usize| words.get(k).and_then(|w| w.parse::<f32>().ok());
        match verb {
            "pause" => self.particles_play(false, cx),
            "play" => self.particles_play(true, cx),
            "restart" => self.particles_restart(cx),
            // Advance a stopped simulation by fixed steps (repeatable runs).
            "step" => {
                self.particles_play(false, cx);
                let s = n(0).ok_or("step <seconds>")?;
                for _ in 0..(s * 60.0).round().max(1.0) as usize {
                    self.particles_step(1.0 / 60.0);
                }
            }
            "fit" => {
                if let Some(v) = self.particles.as_ref().and_then(|p| p.view.clone()) {
                    v.borrow_mut().fit();
                }
            }
            "emitter" => {
                let k = n(0).ok_or("emitter <n>")? as usize;
                let p = self.particles.as_mut().ok_or("no particle system")?;
                p.emitter = Some(p.emitters().get(k).and_then(|e| e["index"].as_u64()).ok_or("no such emitter")? as usize);
                p.module = None;
            }
            "module" => {
                let k = n(0).ok_or("module <n>")? as usize;
                let p = self.particles.as_mut().ok_or("no particle system")?;
                let m = p.emitter_json().map(particles::modules).and_then(|m| m.get(k).and_then(|(m, _)| m["index"].as_u64())).ok_or("no such module")?;
                p.module = Some(m as usize);
            }
            // `set <curve> <entry> <value index> <v>`: a draft, as a drag would make it.
            "set" => {
                let p = self.particles.as_mut().ok_or("no particle system")?;
                let mi = p.module.ok_or("no module chosen")?;
                let m = p.module_json(mi).ok_or("no module")?;
                let d = particles::module_dists(m, &p.drafts).into_iter().nth(n(0).ok_or("set <curve> <entry> <k> <value>")? as usize).ok_or("no such curve")?;
                let key = (mi, d.prop.clone());
                let mut d = d;
                let (e, k, v) = (n(1).ok_or("entry")? as usize, n(2).ok_or("k")? as usize, n(3).ok_or("value")?);
                *d.entries.get_mut(e).and_then(|en| en.get_mut(k)).ok_or("no such value")? = v;
                p.focus = Some(key.clone());
                p.point = Some((e, k));
                self.particles_draft(key, d, cx);
            }
            "apply" => {
                let p = self.particles.as_ref().ok_or("no particle system")?;
                let key = p.focus.clone().ok_or("no curve edited")?;
                self.particles_apply(key, cx);
            }
            _ => return Err(format!("unknown particles step {verb}")),
        }
        cx.notify();
        Ok(())
    }

    /// What the MCP state reports about the particle preview.
    pub(super) fn particles_state(&self) -> J {
        let Some(p) = &self.particles else { return J::Null };
        let runtimes = p.view.as_ref().map(|v| {
            v.borrow()
                .view
                .runtimes
                .iter()
                .map(|r| json!({ "name": r.sim.model.name, "index": r.sim.model.index, "particles": r.sim.particles.len(), "visible": r.visible, "mesh": r.sim.model.mesh, "unsupported": r.sim.model.unsupported }))
                .collect::<Vec<_>>()
        });
        json!({
            "note": p.note,
            "playing": p.playing,
            "emitters": runtimes,
            "emitter": p.emitter,
            "module": p.module,
            "curves": p.module.and_then(|mi| p.module_json(mi)).map(|m| particles::module_dists(m, &p.drafts).iter().map(|d| json!({ "prop": d.prop, "entries": d.entries.len(), "timeScale": d.time_scale, "edited": p.drafts.contains_key(&(m["index"].as_u64().unwrap_or(0) as usize, d.prop.clone())) })).collect::<Vec<_>>()),
            "drafts": p.drafts.len(),
            // Where the focused curve is drawn (window coordinates), and its points there.
            "graph": p.focus.as_ref().and_then(|k| {
                let b = p.graphs.borrow().get(k).copied()?;
                let d = p.dist(k)?;
                let (lo, hi) = range(&d);
                let n = d.entries.len();
                let pts: Vec<_> = (0..n).map(|i| [graph_x(&b, i, n), graph_y(&b, d.entries[i][0], lo, hi), d.entries[i][0]]).collect();
                Some(json!({ "prop": k.1, "points": pts }))
            }),
        })
    }
}
