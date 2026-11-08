//! Materials: the preview of a material or instance (a lit sphere with its
//! base colour, the parent chain, every texture with a thumbnail, the
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
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use serde_json::{json, Value as J};

use super::mesh::{viewport, MeshState};
use super::palette::Pick;
use super::{file_name, Studio, Tone};
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
                    Err(e) => this.set_status(format!("Material preview: {e}"), cx),
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
        self.material = Some(MaterialState { export: i, info: r.json.clone(), thumbs, sphere: None });
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
            self.material_call("material.setParameter", json!({ "name": name, "vector": v }), format!("Set {name} (unsaved)"), cx);
        } else {
            let Ok(f) = value.trim().parse::<f64>() else {
                self.toast(Tone::Warning, format!("{name} takes a number"), cx);
                return;
            };
            self.material_call("material.setParameter", json!({ "name": name, "scalar": f }), format!("Set {name} to {f} (unsaved)"), cx);
        }
    }

    pub(super) fn set_texture(&mut self, name: String, tpath: String, ti: usize, cx: &mut Context<Self>) {
        self.material_call("material.setTexture", json!({ "name": name.clone(), "texture": { "path": tpath, "index": ti } }), format!("{name} now uses the picked texture (unsaved)"), cx);
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
        let sphere = m.sphere.clone().map(|s| viewport("material-view", s, 260.0, None, theme.muted, theme.muted_foreground)).unwrap_or_else(|| div().h(px(260.0)).w_full().rounded_md().bg(theme.muted).into_any_element());
        let section = |title: &str| div().pt_2().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(title.to_uppercase());
        let mut panel = v_flex().gap_1();
        panel = panel.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(if instance { "Changes here become overrides of this instance. A material shared by several meshes changes for all of them: give a mesh its own materials from its Materials list first." } else { "This is a base material: changes here change its defaults, for every instance and mesh using it. To change one mesh only, make an instance of it." }),
        );
        panel = panel.child(h_flex().gap_1().child(Button::new("new-instance").xsmall().label("New instance of this material").tooltip("A MaterialInstanceConstant whose parent is this material, with no overrides of its own").on_click(cx.listener(|this, _, _, cx| this.new_instance(cx)))));
        // Parent chain.
        panel = panel.child(section("Parent chain"));
        for (k, c) in info["chain"].as_array().into_iter().flatten().enumerate() {
            let (pkg, obj, class) = (c["package"].as_str().unwrap_or_default().to_string(), c["object"].as_str().unwrap_or_default().to_string(), c["class"].as_str().unwrap_or_default().to_string());
            let full = format!("{pkg}.{obj}");
            let target = obj.clone();
            panel = panel.child(
                div()
                    .id(("chain", k))
                    .pl(px(k as f32 * 12.0))
                    .text_sm()
                    .text_color(theme.link)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .when(k > 0, |d| d.on_click(cx.listener(move |this, _, _, cx| this.open_object_path(target.clone(), Some(class.clone()), cx))))
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
            let open_path = tpath.clone();
            panel = panel.child(
                h_flex()
                    .gap_2()
                    .child(thumb)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(h_flex().gap_1().text_sm().child(param.clone()).child(div().text_xs().text_color(theme.muted_foreground).child(t["slot"].as_str().unwrap_or_default().to_string())))
                            .child(div().id(("tex", k)).text_xs().text_color(theme.link).cursor_pointer().overflow_hidden().text_ellipsis().whitespace_nowrap().on_click(cx.listener(move |this, _, _, cx| this.open_object_path(open_path.clone(), None, cx))).child(tpath.clone()))
                            .when_some(t["error"].as_str(), |d, e| d.child(div().text_xs().text_color(theme.danger).child(e.to_string()))),
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
                div().flex_1().child(Input::new(&self.value_input).xsmall()).into_any_element()
            } else {
                div()
                    .id((kind, k))
                    .flex_1()
                    .px_1()
                    .rounded_sm()
                    .cursor_text()
                    .hover(|s| s.bg(theme.list_hover))
                    .font_family("Menlo")
                    .text_xs()
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
        let mut list = v_flex().gap_1().child(
            h_flex()
                .gap_1()
                .child(div().flex_1().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("MATERIALS"))
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
            let open = mpath.clone();
            let name: AnyElement = if mref == 0 {
                div().text_xs().text_color(theme.muted_foreground).child("none (map props take theirs from where they are placed)").into_any_element()
            } else {
                div()
                    .id(("slot-mat", k))
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(theme.link)
                    .cursor_pointer()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
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
                    .when(mref != 0, |d| d.child(div().text_xs().text_color(theme.muted_foreground).max_w(px(220.0)).overflow_hidden().text_ellipsis().whitespace_nowrap().child(mpath.rsplit_once('.').map(|(g, _)| format!("in {g}")).unwrap_or_default())))
                    .when(mref != 0, |d| d.child(badge(if s["instance"].as_bool().unwrap_or(false) { "instance" } else { "base material" })))
                    .when(s["external"].as_bool().unwrap_or(false), |d| d.child(badge("other package")))
                    .child(Button::new(("slot-change", k)).xsmall().ghost().label("Change…").tooltip("Draw this slot with another material or instance").on_click(cx.listener(move |this, _, window, cx| this.open_picker(Pick::SlotMaterial(slot), window, cx))))
                    .when(mref != 0, |d| d.child(Button::new(("slot-own", k)).xsmall().ghost().label("Make it this mesh's own").tooltip("A copy of the instance (or a new instance of a base material) next to the mesh, used by this slot only").on_click(cx.listener(move |this, _, _, cx| this.own_slot(slot, cx)))))
                    .when(skeletal && n > 1, |d| d.child(Button::new(("slot-rm", k)).xsmall().ghost().label("Remove").disabled(sections > 0).tooltip(if sections > 0 { "Sections draw with it: import a mesh that does not use it first" } else { "Remove this slot (later slots move down)" }).on_click(cx.listener(move |this, _, _, cx| this.slot_op("mesh.removeSlot", Some(slot), cx))))),
            );
        }
        Some(list.into_any_element())
    }
}
