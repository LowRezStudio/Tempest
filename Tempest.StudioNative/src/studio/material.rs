//! Materials: the preview of a material or instance (a sphere drawn by the
//! material's own game shaders when its package has them compiled, else lit
//! with its base colour; see `game_material`), the parent chain, every texture with a thumbnail, the
//! scalar, vector and switch parameters, nearest override first) where
//! textures, scalars and vectors are edited; and a mesh's material slots,
//! where a slot is pointed at another material or given one of the mesh's
//! own, one at a time or every slot at once ("Own materials"): instances
//! are copied, base materials get a new instance, all in one group named
//! after the mesh, so editing them never changes another mesh.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{div, img, prelude::*, px, AnyElement, Context, Image, ImageFormat, ObjectFit, PathPromptOptions, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable, Selectable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::mesh::{viewport, MeshState};
use super::palette::Pick;
use super::{file_name, Studio, Tone};
use crate::game_material::{GameMaterial, GamePass, Params};
use crate::mesh_view::MeshView;

/// `Studio::editing` for a parameter: this prefix, `s:` or `v:`, the name.
pub const PARAM: &str = "\u{0}param:";

pub fn is_material(class: &str) -> bool {
    let c = class.to_ascii_lowercase();
    c == "material" || c.starts_with("materialinstance")
}

pub struct MaterialState {
    pub export: usize,
    info: J,
    thumbs: HashMap<String, Arc<Image>>,
    sphere: Option<Rc<RefCell<MeshState>>>,
    /// The sphere drawn by the material's game shaders, and the textures it was built with.
    game: Option<(GameView, Vec<String>)>,
    /// Why there is no game shader view (`None` while it is asked for or built).
    game_error: Option<String>,
}

type GameView = Rc<RefCell<MeshState<GameMaterial>>>;

/// The texture paths of a `material.preview` answer: the game view is rebuilt when they change.
fn texture_paths(j: &J) -> Vec<String> {
    j["textures"].as_array().into_iter().flatten().map(|t| t["path"].as_str().unwrap_or_default().to_string()).collect()
}

/// A mesh's material slots (`mesh.materials`).
pub struct Slots {
    pub export: usize,
    info: J,
}

impl Studio {
    pub(super) fn load_material(&mut self, path: String, i: usize, cx: &mut Context<Self>) {
        let call = self.backend.call("material.preview", json!({ "path": path, "index": i, "textureSize": 512 }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.selected_export != Some(i) {
                    return;
                }
                match r {
                    Ok(r) => this.show_material(i, &r, cx),
                    Err(e) => this.toast(Tone::Error, format!("Material preview: {e}"), cx),
                }
            });
        })
        .detach();
    }

    fn show_material(&mut self, i: usize, r: &crate::backend::Reply, cx: &mut Context<Self>) {
        let mut thumbs = HashMap::new();
        let mut base: Option<Vec<u8>> = None;
        for t in r.json["textures"].as_array().into_iter().flatten() {
            if let Some(png) = r.bin(&t["png"]) {
                thumbs.insert(t["path"].as_str().unwrap_or_default().to_string(), Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec())));
                if base.is_none() && t["slot"] == "basecolor" {
                    base = Some(png.to_vec());
                }
            }
        }
        let keep = self.material.as_ref().filter(|m| m.export == i).and_then(|m| m.sphere.as_ref().map(|s| s.borrow().view.camera));
        // The game view of the same material stays: new parameters apply at once, new textures rebuild it.
        let paths = texture_paths(&r.json);
        let old = self.material.take().filter(|m| m.export == i);
        let (game, game_error) = match old {
            Some(m) => (m.game, m.game_error),
            None => (None, None),
        };
        // A material without game shaders is not asked again for a parameter edit.
        let rebuild = game.as_ref().map_or(game_error.is_none(), |(_, p)| *p != paths);
        if let Some((g, _)) = &game {
            let mut st = g.borrow_mut();
            let time = st.view.pass.params.time;
            st.view.pass.params = Params { time, ..Params::from_json(&r.json) };
            st.dirty = true;
        }
        self.material = Some(MaterialState { export: i, info: r.json.clone(), thumbs, sphere: None, game, game_error });
        if rebuild {
            self.load_game_material(i, paths, cx);
        }
        cx.notify();
        let busy = crate::backend::Busy::new();
        cx.spawn(async move |this, cx| {
            let built = cx.background_executor().spawn(async move { MeshView::sphere(base.and_then(|png| image::load_from_memory(&png).ok()).map(|i| i.to_rgba8()).as_ref()) }).await;
            let _ = this.update(cx, |this, cx| {
                drop(busy);
                let Some(m) = this.material.as_mut().filter(|m| m.export == i) else { return };
                if let Ok(mut view) = built {
                    if let Some(c) = keep {
                        view.camera = c;
                    }
                    m.sphere = Some(Rc::new(RefCell::new(MeshState::new(i, 0, view, "base colour only; the game's shader does more".into()))));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask for the material's game shaders and build their view (`paths`: the textures it is built with).
    fn load_game_material(&mut self, i: usize, paths: Vec<String>, cx: &mut Context<Self>) {
        let Some(path) = self.open.as_ref().map(|t| t.path.clone()) else { return };
        let call = self.backend.call("material.gameShader", json!({ "path": path, "index": i, "textureSize": 512, "platform": crate::game_material::platform() }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let built = match r {
                Ok(r) => {
                    let busy = crate::backend::Busy::new();
                    let b = cx.background_executor().spawn(async move { GamePass::new(&r.json, &|v| r.bin(v).map(<[u8]>::to_vec)).map(GameMaterial::new) }).await;
                    drop(busy);
                    b
                }
                Err(e) => Err(e.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                let Some(m) = this.material.as_mut().filter(|m| m.export == i) else { return };
                match built {
                    Ok(mut view) => {
                        if let Some((old, _)) = &m.game {
                            let old = old.borrow();
                            (view.camera, view.pass.params.time) = (old.view.camera, old.view.pass.params.time);
                        }
                        let summary = format!("the game's shaders: {}{}", view.pass.pair, if view.pass.warnings.is_empty() { String::new() } else { format!(" ({} missing: white)", view.pass.warnings.join("; ")) });
                        let animated = view.pass.animated;
                        let state = Rc::new(RefCell::new(MeshState::new(i, 0, view, summary)));
                        m.game = Some((state.clone(), paths));
                        m.game_error = None;
                        if animated {
                            this.animate_game_material(state, cx);
                        }
                    }
                    Err(e) => {
                        m.game = None;
                        m.game_error = Some(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Redraw a game view whose material reads the clock, while it is the one shown.
    fn animate_game_material(&mut self, state: GameView, cx: &mut Context<Self>) {
        let start = std::time::Instant::now();
        let base = state.borrow().view.pass.params.time;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_millis(33)).await;
            let shown = this.update(cx, |this, cx| {
                let current = this.material.as_ref().and_then(|m| m.game.as_ref()).is_some_and(|(g, _)| Rc::ptr_eq(g, &state));
                if current && this.game_shaders {
                    let mut st = state.borrow_mut();
                    st.view.pass.params.time = base + start.elapsed().as_secs_f32();
                    st.dirty = true;
                    cx.notify();
                }
                current
            });
            if !matches!(shown, Ok(true)) {
                break;
            }
        })
        .detach();
    }

    pub(super) fn load_slots(&mut self, path: String, i: usize, cx: &mut Context<Self>) {
        let call = self.backend.call("mesh.materials", json!({ "path": path, "index": i }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.selected_export == Some(i) {
                    if let Ok(r) = r {
                        this.slots = Some(Slots { export: i, info: r.json });
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// A mesh slot change answered with the new slots: show them, redraw the mesh.
    fn slots_changed(&mut self, path: String, i: usize, r: J, what: String, cx: &mut Context<Self>) {
        self.slots = Some(Slots { export: i, info: r });
        self.dirty.insert(path.clone());
        self.toast(Tone::Success, what, cx);
        self.load_mesh(path.clone(), i, self.mesh.as_ref().map(|m| m.borrow().lod).unwrap_or(0), cx);
        // New materials are new exports.
        let call = self.backend.call("package.tables", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            if let Ok(t) = call.await {
                let _ = this.update(cx, |this, cx| {
                    if this.open.as_ref().is_some_and(|o| o.path == path) {
                        this.open = Some(crate::model::Tables::from_json(&path, &t.json));
                        this.refresh_rows(cx);
                    }
                });
            }
        })
        .detach();
    }

    fn mesh_call(&mut self, method: &'static str, params: J, done: impl FnOnce(&J) -> String + 'static, cx: &mut Context<Self>) {
        let (Some(path), Some(i)) = (self.open.as_ref().map(|t| t.path.clone()), self.selected_export) else { return };
        let mut params = params;
        params["path"] = J::from(path.clone());
        params["index"] = J::from(i);
        let call = self.backend.call(method, params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    let what = done(&r.json);
                    for s in r.json["skipped"].as_array().into_iter().flatten().filter(|s| s["reason"] != "already the mesh's own") {
                        this.toast(Tone::Warning, format!("Slot {}: {}", s["slot"], s["reason"].as_str().unwrap_or_default()), cx);
                    }
                    this.slots_changed(path, i, r.json, what, cx);
                }
                Err(e) => this.toast(Tone::Error, e, cx),
            });
        })
        .detach();
    }

    pub(super) fn set_slot_material(&mut self, slot: usize, mpath: String, mi: usize, cx: &mut Context<Self>) {
        self.mesh_call("mesh.setMaterial", json!({ "slot": slot, "material": { "path": mpath, "index": mi } }), move |r| format!("Slot {slot} now draws with {}", r["slots"].as_array().and_then(|a| a.iter().find(|s| s["slot"] == slot)).and_then(|s| s["material"]["path"].as_str()).unwrap_or_default()), cx);
    }

    fn own_slot(&mut self, slot: usize, cx: &mut Context<Self>) {
        self.mesh_call("mesh.ownMaterial", json!({ "slot": slot }), |r| format!("{} is this mesh's own: open it to change its textures and colours", r["material"]["path"].as_str().unwrap_or_default()), cx);
    }

    pub(super) fn own_materials(&mut self, cx: &mut Context<Self>) {
        self.mesh_call(
            "mesh.ownMaterials",
            json!({}),
            |r| {
                let n = r["made"].as_array().map(|a| a.len()).unwrap_or(0);
                if n == 0 {
                    "Every slot already has a material of this mesh's own".into()
                } else {
                    format!("{n} material{} of this mesh's own in {}: open one to change its textures", if n == 1 { "" } else { "s" }, r["group"]["path"].as_str().unwrap_or_default())
                }
            },
            cx,
        );
    }

    fn slot_op(&mut self, method: &'static str, slot: Option<usize>, cx: &mut Context<Self>) {
        let what = if method == "mesh.addSlot" { "Added a slot: sections use it once a mesh with that many materials is imported".to_string() } else { format!("Removed slot {}", slot.unwrap_or(0)) };
        self.mesh_call(method, json!({ "slot": slot }), move |_| what, cx);
    }

    // ----- material edits ---------------------------------------------------------

    fn material_call(&mut self, method: &'static str, params: J, what: String, cx: &mut Context<Self>) {
        let (Some(path), Some(i)) = (self.open.as_ref().map(|t| t.path.clone()), self.material.as_ref().map(|m| m.export)) else { return };
        let mut params = params;
        params["path"] = J::from(path.clone());
        params["index"] = J::from(i);
        let call = self.backend.call(method, params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.dirty.insert(path.clone());
                    this.set_status(what, cx);
                    if this.selected_export == Some(i) {
                        this.show_material(i, &r, cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, e, cx),
            });
        })
        .detach();
    }

    pub(super) fn commit_parameter(&mut self, key: String, value: String, cx: &mut Context<Self>) {
        let (kind, name) = key.split_once(':').unwrap_or(("s", &key));
        let name = name.to_string();
        if kind == "v" {
            let c: Vec<f64> = value.split(|ch: char| ch == ',' || ch.is_whitespace()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
            if c.len() < 3 {
                self.toast(Tone::Warning, "A colour is three or four numbers: R G B [A]", cx);
                return;
            }
            let v = [c[0], c[1], c[2], c.get(3).copied().unwrap_or(1.0)];
            self.material_call("material.setParameter", json!({ "name": name, "vector": v }), format!("Set {name}"), cx);
        } else {
            let Ok(f) = value.trim().parse::<f64>() else {
                self.toast(Tone::Warning, format!("{name} takes a number"), cx);
                return;
            };
            self.material_call("material.setParameter", json!({ "name": name, "scalar": f }), format!("Set {name} to {f}"), cx);
        }
    }

    pub(super) fn set_texture(&mut self, name: String, tpath: String, ti: usize, cx: &mut Context<Self>) {
        self.material_call("material.setTexture", json!({ "name": name.clone(), "texture": { "path": tpath, "index": ti } }), format!("{name} now uses the picked texture"), cx);
    }

    /// A new texture from an image, made like the parameter's current one
    /// and put next to the material, then used by the parameter.
    fn texture_from_file(&mut self, t: J, cx: &mut Context<Self>) {
        let Some(m) = &self.material else { return };
        let Some(tables) = &self.open else { return };
        let (path, i) = (tables.path.clone(), m.export);
        let param = t["parameter"].as_str().unwrap_or_default().to_string();
        let Some(from) = self.packages.iter().find(|p| t["package"].as_str().is_some_and(|n| p.name.eq_ignore_ascii_case(n))).map(|p| p.path.clone()) else {
            self.toast(Tone::Warning, format!("{} is not in the game folder", t["package"].as_str().unwrap_or_default()), cx);
            return;
        };
        let name = format!("{}_{}", tables.exports[i].name, param.replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "_"));
        let outer = tables.parent[i].map(J::from).unwrap_or_else(|| J::from("top"));
        let export = t["export"].as_u64().unwrap_or(0);
        let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(format!("Image for {param} (PNG or DDS)").into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut picked))) = files.await else { return };
            let Some(file) = picked.pop().map(|f| f.to_string_lossy().to_string()) else { return };
            let Ok(call) = this.update(cx, |this, cx| {
                this.set_status(format!("Encoding {}…", file_name(&file)), cx);
                this.backend.call("object.add", json!({ "path": path, "file": file, "template": { "path": from, "index": export }, "name": name, "outer": outer }))
            }) else { return };
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    let ti = r.json["index"].as_u64().unwrap_or(0) as usize;
                    this.toast(Tone::Success, format!("Added {} for {param}", r.json["path"].as_str().unwrap_or_default()), cx);
                    this.set_texture(param.clone(), path.clone(), ti, cx);
                    this.refresh_tables_only(path.clone(), cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not add the texture: {e}"), cx),
            });
        })
        .detach();
    }

    fn refresh_tables_only(&mut self, path: String, cx: &mut Context<Self>) {
        let call = self.backend.call("package.tables", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            if let Ok(t) = call.await {
                let _ = this.update(cx, |this, cx| {
                    if this.open.as_ref().is_some_and(|o| o.path == path) {
                        this.open = Some(crate::model::Tables::from_json(&path, &t.json));
                        this.refresh_rows(cx);
                    }
                });
            }
        })
        .detach();
    }

    /// A new instance of the selected material, next to it.
    pub(super) fn new_instance(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(i)) = (self.open.as_ref().map(|t| t.path.clone()), self.selected_export) else { return };
        self.new_instance_of(path, i, cx);
    }

    /// A new instance of material `mi` of package `mpath`, in the open
    /// package (next to it when it is there, else in the selected group).
    pub(super) fn new_instance_of(&mut self, mpath: String, mi: usize, cx: &mut Context<Self>) {
        let Some(t) = &self.open else { return };
        let path = t.path.clone();
        let (outer, base) = if mpath == path { (t.parent[mi], t.exports[mi].name.clone()) } else { (self.selected_export.and_then(|s| if t.exports[s].class.eq_ignore_ascii_case("package") { Some(s) } else { t.parent[s] }), "mi_new".to_string()) };
        let call = self.backend.call("material.newInstance", json!({ "path": path, "parent": { "path": mpath, "index": mi }, "name": format!("{base}_inst"), "outer": outer, "unique": true }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => this.added(&path, r.json["index"].as_u64().map(|i| i as usize), format!("{} draws like its parent until you change its textures or parameters; point a mesh slot at it with Change", r.json["path"].as_str().unwrap_or_default()), cx),
                Err(e) => this.toast(Tone::Error, format!("Could not make the instance: {e}"), cx),
            });
        })
        .detach();
    }

    /// Draw previews with the game's shaders, or with the base colour.
    pub(super) fn toggle_game_shaders(&mut self, cx: &mut Context<Self>) {
        self.game_shaders = !self.game_shaders;
        if let Some(m) = &self.mesh {
            let mut st = m.borrow_mut();
            st.view.use_game = self.game_shaders;
            st.dirty = true;
        }
        if let Some(g) = self.material.as_ref().and_then(|m| m.game.as_ref()) {
            g.0.borrow_mut().dirty = true;
        }
        cx.notify();
    }

    /// Game shaders from the other cache (shader model 5 ↔ 3): every preview asks again.
    pub(super) fn toggle_shader_model(&mut self, cx: &mut Context<Self>) {
        self.prefs.sm3 = !self.prefs.sm3;
        self.prefs.save();
        crate::game_material::set_sm3(self.prefs.sm3);
        self.toast(Tone::Info, if self.prefs.sm3 { "Game shaders: shader model 3 (Direct3D 9)" } else { "Game shaders: shader model 5 (Direct3D 11)" }, cx);
        if let Some(m) = self.material.as_mut() {
            m.game = None;
            m.game_error = None;
            let (i, paths) = (m.export, texture_paths(&m.info));
            self.load_game_material(i, paths, cx);
        }
        self.reload_mesh(cx);
        if let Some(map) = self.map.clone() {
            map.update(cx, |m, cx| m.reload_game(cx));
        }
        cx.notify();
    }

    /// The material preview for `studio_state`: what draws it and why.
    pub(super) fn material_state_json(&self) -> Option<J> {
        let m = self.material.as_ref()?;
        let game = m.game.as_ref().map(|(g, _)| {
            let st = g.borrow();
            json!({ "pair": st.view.pass.pair, "animated": st.view.pass.animated, "warnings": st.view.pass.warnings, "time": st.view.pass.params.time, "camera": super::mesh::camera_json(&st.view.camera) })
        });
        Some(json!({ "export": m.export, "drawnWithGameShader": self.game_shaders && game.is_some(), "platform": crate::game_material::platform(), "game": game, "gameError": m.game_error }))
    }

    /// Dev: `material <step>` of the test driver.
    pub(super) fn drive_material(&mut self, arg: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let (verb, rest) = arg.split_once(' ').unwrap_or((arg, ""));
        match verb {
            "own-all" => self.own_materials(cx),
            "own" => self.own_slot(rest.trim().parse().map_err(|_| "own <slot>")?, cx),
            "scalar" => {
                let (n, v) = rest.rsplit_once(' ').ok_or("scalar <name> <value>")?;
                self.commit_parameter(format!("s:{n}"), v.to_string(), cx);
            }
            "pick-texture" => self.open_picker(Pick::Texture(rest.trim().to_string()), window, cx),
            // `material game on|off`: draw with the game's shaders or the base colour.
            "game" => {
                if self.game_shaders != (rest.trim() != "off") {
                    self.toggle_game_shaders(cx);
                }
            }
            _ => return Err(format!("unknown material step {verb}")),
        }
        Ok(())
    }

    // ----- rendering -----------------------------------------------------------------

    pub(super) fn render_material(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let m = self.material.as_ref()?;
        let info = &m.info;
        let instance = self.selection_class().is_some_and(|c| c.to_ascii_lowercase().starts_with("materialinstance"));
        // The game's shaders when there are, else the base colour; a toggle between the two.
        let game = m.game.as_ref().map(|(g, _)| g.clone());
        let use_game = self.game_shaders && game.is_some();
        let (tip, available): (String, bool) = match (&game, &m.game_error) {
            (Some(_), _) => ("Draw with the material's own compiled shaders, like the game, or with its base colour only".into(), true),
            (None, Some(e)) => (format!("No game shader: {e}"), false),
            (None, None) => ("Building the game's shaders…".into(), false),
        };
        let toggle = Button::new("game-shader").xsmall().label("Game shader").tooltip(tip).disabled(!available).on_click(cx.listener(|this, _, _, cx| this.toggle_game_shaders(cx)));
        let bar = h_flex().gap_1().child(div().relative().child(toggle.ghost().selected(use_game)).child(crate::driver::probe("game-shader"))).into_any_element();
        let sphere = match (use_game, game, m.sphere.clone()) {
            (true, Some(g), _) => viewport("material-view", g, 260.0, Some(bar), theme.muted, theme.muted_foreground),
            (_, _, Some(s)) => {
                if let Some(e) = &m.game_error {
                    let mut st = s.borrow_mut();
                    let note = format!("base colour only: {e}");
                    if st.summary != note {
                        st.summary = note;
                    }
                }
                viewport("material-view", s, 260.0, Some(bar), theme.muted, theme.muted_foreground)
            }
            _ => div().h(px(260.0)).w_full().rounded_md().bg(theme.muted).into_any_element(),
        };
        let section = |title: &str| div().pt_2().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(title.to_string());
        let mut panel = v_flex().gap_1();
        panel = panel.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(if instance { "Changes here become overrides of this instance. A material shared by several meshes changes for all of them: give a mesh its own materials from its Materials list first." } else { "This is a base material: changes here change its defaults, for every instance and mesh using it. To change one mesh only, make an instance of it." }),
        );
        panel = panel.child(h_flex().gap_1().child(Button::new("new-instance").xsmall().label("New instance of this material").tooltip("A MaterialInstanceConstant whose parent is this material, with no overrides of its own").on_click(cx.listener(|this, _, _, cx| this.new_instance(cx)))));
        // Parent chain.
        let ring = theme.ring;
        let hover_bg = theme.list_hover;
        panel = panel.child(section("Parent chain"));
        for (k, c) in info["chain"].as_array().into_iter().flatten().enumerate() {
            let (pkg, obj, class) = (c["package"].as_str().unwrap_or_default().to_string(), c["object"].as_str().unwrap_or_default().to_string(), c["class"].as_str().unwrap_or_default().to_string());
            let full = format!("{pkg}.{obj}");
            let (target, target_k) = (obj.clone(), obj.clone());
            let class_k = class.clone();
            panel = panel.child(
                div()
                    .id(("chain", k))
                    .pl(px(k as f32 * 12.0))
                    .pr_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .text_sm()
                    .when(k == 0, |d| d.text_color(theme.muted_foreground))
                    .when(k > 0, |d| {
                        d.focusable()
                            .tab_index(0)
                            .focus_visible(move |s| s.border_color(ring))
                            .text_color(theme.link)
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                                if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.open_object_path(target_k.clone(), Some(class_k.clone()), cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| this.open_object_path(target.clone(), Some(class.clone()), cx)))
                    })
                    .child(if k == 0 { format!("{obj} (this)") } else { full }),
            );
        }
        // Textures.
        panel = panel.child(section("Textures"));
        let textures = info["textures"].as_array().cloned().unwrap_or_default();
        if textures.is_empty() {
            panel = panel.child(div().text_xs().text_color(theme.muted_foreground).child("No texture parameter"));
        }
        for (k, t) in textures.into_iter().enumerate() {
            let param = t["parameter"].as_str().unwrap_or_default().to_string();
            let tpath = t["path"].as_str().unwrap_or_default().to_string();
            let thumb: AnyElement = match m.thumbs.get(&tpath) {
                Some(i) => div().size(px(44.0)).flex_none().rounded_sm().overflow_hidden().bg(theme.muted).child(img(i.clone()).size_full().object_fit(ObjectFit::Cover)).into_any_element(),
                None => div().size(px(44.0)).flex_none().rounded_sm().bg(theme.muted).into_any_element(),
            };
            let (p1, p2, tj) = (param.clone(), param.clone(), t.clone());
            let (open_path, open_path_k) = (tpath.clone(), tpath.clone());
            panel = panel.child(
                h_flex()
                    .gap_2()
                    .child(thumb)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(h_flex().gap_1().text_sm().child(param.clone()).child(div().text_xs().text_color(theme.muted_foreground).child(t["slot"].as_str().unwrap_or_default().to_string())))
                            .child(
                                div()
                                    .id(("tex", k))
                                    .focusable()
                                    .tab_index(0)
                                    .px_0p5()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(gpui::transparent_black())
                                    .focus_visible(move |s| s.border_color(ring))
                                    .text_xs()
                                    .text_color(theme.link)
                                    .cursor_pointer()
                                    .hover(|s| s.underline())
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                                        if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                            window.prevent_default();
                                            cx.stop_propagation();
                                            this.open_object_path(open_path_k.clone(), None, cx);
                                        }
                                    }))
                                    .on_click(cx.listener(move |this, _, _, cx| this.open_object_path(open_path.clone(), None, cx)))
                                    .child(tpath.clone()),
                            )
                            .when_some(t["error"].as_str(), |d, e| d.child(div().text_xs().text_color(theme.danger).child(crate::studio::ui::explain_error(e)))),
                    )
                    .child(Button::new(("tex-change", k)).xsmall().ghost().label("Change…").tooltip("Use another texture of any package").on_click(cx.listener(move |this, _, window, cx| this.open_picker(Pick::Texture(p1.clone()), window, cx))))
                    .child(Button::new(("tex-file", k)).xsmall().ghost().label("From a file…").tooltip("A new texture from a PNG or DDS, made like the current one and put next to the material").on_click(cx.listener(move |this, _, _, cx| {
                        let _ = &p2;
                        this.texture_from_file(tj.clone(), cx)
                    }))),
            );
        }
        // Scalars and vectors, edited in place.
        let editing = self.editing.clone();
        let param_row = |k: usize, kind: &'static str, name: String, shown: String, draft: String, cx: &mut Context<Self>| {
            let key = format!("{PARAM}{kind}:{name}");
            let value: AnyElement = if editing.as_deref() == Some(key.as_str()) {
                div().flex_1().min_w_0().child(Input::new(&self.value_input).xsmall()).into_any_element()
            } else {
                let (key_k, draft_k) = (key.clone(), draft.clone());
                div()
                    .id((kind, k))
                    .flex_1()
                    .px_1()
                    .rounded_sm()
                    .focusable()
                    .tab_index(0)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(move |s| s.border_color(ring))
                    .cursor_text()
                    .hover(move |s| s.bg(hover_bg))
                    .font_family(crate::studio::ui::mono())
                    .text_xs()
                    .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                        if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.start_edit(key_k.clone(), draft_k.clone(), window, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| this.start_edit(key.clone(), draft.clone(), window, cx)))
                    .child(shown)
                    .into_any_element()
            };
            h_flex().gap_2().text_sm().child(div().w(px(180.0)).flex_none().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name)).child(value)
        };
        let scalars = info["scalars"].as_array().cloned().unwrap_or_default();
        if !scalars.is_empty() {
            panel = panel.child(section("Scalars"));
        }
        for (k, s) in scalars.into_iter().enumerate() {
            let v = s["value"].as_f64().unwrap_or(0.0);
            let shown = format!("{}", (v * 10000.0).round() / 10000.0);
            panel = panel.child(param_row(k, "s", s["name"].as_str().unwrap_or_default().to_string(), shown.clone(), shown, cx));
        }
        let vectors = info["vectors"].as_array().cloned().unwrap_or_default();
        if !vectors.is_empty() {
            panel = panel.child(section("Vectors (R G B A)"));
        }
        for (k, s) in vectors.into_iter().enumerate() {
            let c: Vec<f64> = s["value"].as_array().into_iter().flatten().map(|x| (x.as_f64().unwrap_or(0.0) * 1000.0).round() / 1000.0).collect();
            let text = c.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ");
            let swatch = gpui::Rgba { r: c.first().copied().unwrap_or(0.0).clamp(0.0, 1.0) as f32, g: c.get(1).copied().unwrap_or(0.0).clamp(0.0, 1.0) as f32, b: c.get(2).copied().unwrap_or(0.0).clamp(0.0, 1.0) as f32, a: 1.0 };
            panel = panel.child(h_flex().gap_1().child(div().size(px(12.0)).rounded_sm().bg(swatch)).child(param_row(k, "v", s["name"].as_str().unwrap_or_default().to_string(), text.clone(), text, cx)));
        }
        let switches = info["switches"].as_array().cloned().unwrap_or_default();
        if !switches.is_empty() {
            panel = panel.child(section("Static switches (compiled in, not editable)"));
            for s in switches {
                panel = panel.child(h_flex().gap_2().text_sm().child(div().w(px(180.0)).child(s["name"].as_str().unwrap_or_default().to_string())).child(div().text_xs().text_color(theme.muted_foreground).child(if s["value"].as_bool().unwrap_or(false) { "on" } else { "off" })));
            }
        }
        Some(v_flex().gap_2().child(sphere).child(panel).into_any_element())
    }

    pub(super) fn render_slots(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let s = self.slots.as_ref().filter(|s| Some(s.export) == self.selected_export)?;
        let slots = s.info["slots"].as_array().cloned().unwrap_or_default();
        if slots.is_empty() {
            return None;
        }
        let skeletal = s.info["skeletal"].as_bool().unwrap_or(false);
        let n = slots.len();
        let previewed = self.mesh_preview.as_ref().filter(|p| p.export == s.export).map(|p| p.slots.clone()).unwrap_or_default();
        let mut list = v_flex().gap_1().child(
            h_flex()
                .gap_1()
                .child(div().flex_1().min_w_0().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("Materials"))
                .child(div().relative().child(Button::new("own-all").xsmall().label("Own materials for every slot").tooltip("Give every slot a material of this mesh's own, gathered in one group named after the mesh: then change their textures without touching other meshes").on_click(cx.listener(|this, _, _, cx| this.own_materials(cx)))).child(crate::driver::probe("own-all")))
                .when(skeletal, |d| d.child(Button::new("add-slot").xsmall().ghost().label("Add slot").tooltip("A new slot drawn with the last slot's material; sections use it once a mesh with that many materials is imported").on_click(cx.listener(|this, _, _, cx| this.slot_op("mesh.addSlot", None, cx))))),
        );
        for (k, s) in slots.into_iter().enumerate() {
            let slot = s["slot"].as_u64().unwrap_or(0) as usize;
            let mref = s["material"]["ref"].as_i64().unwrap_or(0);
            let mpath = s["material"]["path"].as_str().unwrap_or_default().to_string();
            let class = s["material"]["class"].as_str().map(|c| c.to_string());
            let sections = s["sections"].as_u64().unwrap_or(0);
            let badge = |t: &str| div().px_1().rounded_sm().text_xs().bg(theme.secondary).text_color(theme.muted_foreground).child(t.to_string());
            let (open, open_k) = (mpath.clone(), mpath.clone());
            let class_k = class.clone();
            let ring = theme.ring;
            let name: AnyElement = if mref == 0 {
                div().text_xs().text_color(theme.muted_foreground).child("none (map props take theirs from where they are placed)").into_any_element()
            } else {
                div()
                    .id(("slot-mat", k))
                    .flex_1()
                    .min_w(px(120.0))
                    .px_0p5()
                    .rounded_sm()
                    .focusable()
                    .tab_index(0)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(move |s| s.border_color(ring))
                    .text_sm()
                    .text_color(theme.link)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                        if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                            window.prevent_default();
                            cx.stop_propagation();
                            if mref > 0 {
                                let i = (mref - 1) as usize;
                                this.reveal(i, cx);
                                this.user_select_export(i, cx);
                            } else {
                                this.open_object_path(open_k.clone(), class_k.clone(), cx);
                            }
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if mref > 0 {
                            let i = (mref - 1) as usize;
                            this.reveal(i, cx);
                            this.user_select_export(i, cx);
                        } else {
                            this.open_object_path(open.clone(), class.clone(), cx);
                        }
                    }))
                    .child(mpath.rsplit('.').next().unwrap_or_default().to_string())
                    .into_any_element()
            };
            list = list.child(
                h_flex()
                    .gap_2()
                    .child(div().w(px(44.0)).flex_none().text_xs().text_color(theme.muted_foreground).child(format!("slot {slot}")))
                    .child(name)
                    .when(mref != 0, |d| d.child(div().text_xs().text_color(theme.muted_foreground).min_w_0().max_w(px(220.0)).overflow_hidden().text_ellipsis().whitespace_nowrap().child(mpath.rsplit_once('.').map(|(g, _)| format!("in {g}")).unwrap_or_default())))
                    .when(mref != 0, |d| d.child(badge(if s["instance"].as_bool().unwrap_or(false) { "instance" } else { "base material" })))
                    .when(s["external"].as_bool().unwrap_or(false), |d| d.child(badge("other package")))
                    .child(Button::new(("slot-change", k)).xsmall().ghost().label("Change…").tooltip("Draw this slot with another material or instance").on_click(cx.listener(move |this, _, window, cx| this.open_picker(Pick::SlotMaterial(slot), window, cx))))
                    .child(div().relative().child(Button::new(("slot-preview", k)).xsmall().ghost().label("Preview…").tooltip("See the mesh drawn with another material, without changing it").on_click(cx.listener(move |this, _, window, cx| this.open_picker(Pick::PreviewMaterial(slot), window, cx)))).when(k == 0, |d| d.child(crate::driver::probe("slot-preview"))))
                    .when(mref != 0, |d| d.child(Button::new(("slot-own", k)).xsmall().ghost().label("Make it this mesh's own").tooltip("A copy of the instance (or a new instance of a base material) next to the mesh, used by this slot only").on_click(cx.listener(move |this, _, _, cx| this.own_slot(slot, cx)))))
                    .when(skeletal && n > 1, |d| d.child(Button::new(("slot-rm", k)).xsmall().ghost().label("Remove").disabled(sections > 0).tooltip(if sections > 0 { "Sections draw with it: import a mesh that does not use it first" } else { "Remove this slot (later slots move down)" }).on_click(cx.listener(move |this, _, _, cx| this.slot_op("mesh.removeSlot", Some(slot), cx))))),
            );
            if let Some(m) = previewed.get(&slot) {
                list = list.child(
                    h_flex()
                        .gap_2()
                        .pl(px(52.0))
                        .text_xs()
                        .child(Icon::new(IconName::Eye).xsmall().text_color(theme.info))
                        .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(theme.info).child(format!("Previewed with {} (in {}); the mesh is unchanged", m.object.rsplit('.').next().unwrap_or_default(), m.package)))
                        .child(Button::new(("slot-keep", k)).xsmall().ghost().label("Keep").tooltip("Make the slot draw with it (a change to the mesh)").on_click(cx.listener(move |this, _, _, cx| this.keep_material_preview(slot, cx))))
                        .child(Button::new(("slot-unpreview", k)).xsmall().ghost().label("Stop").tooltip("Draw the slot with its own material again").on_click(cx.listener(move |this, _, _, cx| this.stop_material_preview(Some(slot), cx)))),
                );
            }
        }
        Some(list.into_any_element())
    }
}
