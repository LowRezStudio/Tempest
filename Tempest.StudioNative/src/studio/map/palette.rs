//! Adding actors (toolbar Actor, ⇧A; `map.palette` / `map.addActor`): a copy
//! of an actor a level places (from this map when it has one), a new actor
//! of any placeable class built from its defaults, or a StaticMeshActor
//! showing a mesh of any package; placed where the view looks. And the
//! export of the map (the levels and kinds shown, by default) as one GLB or
//! FBX (`map.export`).

use std::collections::HashSet;

use gpui::{div, prelude::*, px, AnyElement, Context, Entity, MouseButton, ScrollHandle, Subscription, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Selectable, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{category_color, MapEditor, CATEGORIES};
use crate::studio::Tone;

/// A dialog box over the map view.
pub fn modal(title: &str, width: f32, body: AnyElement, cx: &mut Context<MapEditor>) -> AnyElement {
    let theme = cx.theme().clone();
    div()
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.overlay)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            v_flex()
                .id(gpui::SharedString::from(format!("modal-{title}")))
                .w(px(width))
                .max_w(gpui::relative(0.94))
                .max_h(gpui::relative(0.9))
                .overflow_y_scroll()
                .p_4()
                .gap_3()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(title.to_string()))
                .child(body),
        )
        .into_any_element()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AddMode {
    Copy,
    Class,
    Mesh,
}

pub struct ActorPalette {
    pub open: bool,
    pub mode: AddMode,
    pub level: Option<String>,
    pub selected_row: usize,
    filter: Option<Entity<InputState>>,
    scroll: ScrollHandle,
    pub classes: Option<Vec<J>>,
    pub placeable: Option<Vec<J>>,
    pub loaded_for: Option<String>,
    meshes: Vec<J>,
    mesh_seq: u64,
    _sub: Option<Subscription>,
}

impl ActorPalette {
    pub fn new(_window: &mut Window, _cx: &mut Context<MapEditor>) -> ActorPalette {
        ActorPalette {
            open: false,
            mode: AddMode::Copy,
            level: None,
            selected_row: 0,
            filter: None,
            scroll: ScrollHandle::new(),
            classes: None,
            placeable: None,
            loaded_for: None,
            meshes: Vec::new(),
            mesh_seq: 0,
            _sub: None,
        }
    }

    pub fn reset(&mut self) {
        self.open = false;
        self.selected_row = 0;
        self.classes = None;
        self.placeable = None;
        self.loaded_for = None;
    }
}

#[derive(Default)]
pub struct Export {
    pub open: bool,
    pub fbx: bool,
    pub levels: HashSet<String>,
    pub skip: HashSet<String>,
    pub show_skip: Option<bool>,
    pub volumes: bool,
    pub textures: bool,
    pub size: u32,
    pub busy: bool,
}

impl MapEditor {
    pub(super) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.open {
            self.palette.open = false;
            if let Some(f) = &self.view.focus {
                f.focus(window, cx);
            }
            cx.notify();
            return;
        }
        self.palette.open = true;
        self.palette.selected_row = 0;
        self.palette.scroll.scroll_to_item(0);
        self.bake.open = false;
        self.export.open = false;
        if self.palette.level.as_ref().is_none_or(|l| self.level(l).is_none()) {
            self.palette.level = self.target_level();
        }
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |s, cx| s.focus(window, cx));
        self.palette._sub = Some(cx.subscribe(&input, |this, input, e: &InputEvent, cx| match e {
            InputEvent::Change => {
                this.palette.selected_row = 0;
                this.palette.scroll.scroll_to_item(0);
                if this.palette.mode == AddMode::Mesh {
                    let q = input.read(cx).value().to_string();
                    this.search_meshes(q, cx);
                }
                cx.notify();
            }
            InputEvent::PressEnter { .. } => this.palette_enter(cx),
            _ => {}
        }));
        self.palette.filter = Some(input);
        if self.palette.mode == AddMode::Mesh {
            self.palette.meshes.clear();
        }
        if let Some(l) = self.palette.level.clone() {
            self.load_actor_palette(l, cx);
        }
        cx.notify();
    }

    pub(crate) fn load_actor_palette(&mut self, level: String, cx: &mut Context<Self>) {
        if self.palette.classes.is_some() && self.palette.loaded_for.as_ref() == Some(&level) {
            return;
        }
        let levels: Vec<String> = self.levels.iter().map(|l| l.path.clone()).collect();
        let call = self.call("map.palette", json!({ "path": level, "levels": levels }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        this.palette.classes = r.json["classes"].as_array().cloned();
                        this.palette.placeable = r.json["placeable"].as_array().cloned();
                        this.palette.loaded_for = Some(level.clone());
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not list the actors: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn search_meshes(&mut self, q: String, cx: &mut Context<Self>) {
        self.palette.mesh_seq += 1;
        let seq = self.palette.mesh_seq;
        if q.trim().len() < 2 {
            self.palette.meshes.clear();
            self.palette.selected_row = 0;
            return;
        }
        let call = self.call("workspace.search", json!({ "query": q, "limit": 100, "isA": "StaticMesh" }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.palette.mesh_seq == seq {
                    this.palette.meshes = r.map(|r| r.json["results"].as_array().cloned().unwrap_or_default()).unwrap_or_default();
                    this.palette.selected_row = this.palette.selected_row.min(this.palette.meshes.len().saturating_sub(1));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn palette_query(&self, cx: &gpui::App) -> String {
        self.palette.filter.as_ref().map(|f| f.read(cx).value().trim().to_ascii_lowercase()).unwrap_or_default()
    }

    fn copies(&self, q: &str) -> Vec<J> {
        let mut v: Vec<J> = self.palette.classes.iter().flatten().filter(|c| q.is_empty() || c["class"].as_str().is_some_and(|s| s.contains(q)) || c["category"].as_str().is_some_and(|s| s.contains(q))).cloned().collect();
        v.sort_by(|a, b| (b["inMap"] == true).cmp(&(a["inMap"] == true)).then(b["count"].as_u64().cmp(&a["count"].as_u64())));
        v
    }

    fn new_classes(&self, q: &str) -> Vec<J> {
        self.palette.placeable.iter().flatten().filter(|c| q.is_empty() || [&c["class"], &c["category"], &c["package"]].iter().any(|v| v.as_str().is_some_and(|s| s.to_ascii_lowercase().contains(q)))).cloned().collect()
    }

    fn palette_count(&self, q: &str) -> usize {
        if self.palette.classes.is_none() && self.palette.mode != AddMode::Mesh {
            return 0;
        }
        match self.palette.mode {
            AddMode::Copy => self.copies(q).len().min(300),
            AddMode::Class => self.new_classes(q).len().min(400),
            AddMode::Mesh => self.palette.meshes.len(),
        }
    }

    fn step_palette(&mut self, delta: isize, cx: &mut Context<Self>) {
        let q = self.palette_query(cx);
        let count = self.palette_count(&q);
        if count == 0 {
            self.palette.selected_row = 0;
            return;
        }
        let cur = self.palette.selected_row.min(count - 1);
        let next = (cur as isize + delta).clamp(0, count as isize - 1) as usize;
        self.palette.selected_row = next;
        self.palette.scroll.scroll_to_item(next);
        cx.notify();
    }

    /// Where the view looks (the orbit target): new actors go there.
    pub(crate) fn view_target(&self) -> Option<[f32; 3]> {
        self.view.cam().map(|c| c.target.into())
    }

    /// Add a copy of `example` (an actor of any level package) to `level`, where the view looks.
    pub(crate) fn add_copy(&mut self, example: &J, level: String, cx: &mut Context<Self>) {
        let at = self.view_target();
        self.add_actor_call(level.clone(), json!({ "path": level, "source": { "path": example["path"], "index": example["index"] }, "at": at }), "Could not add the actor".into(), cx);
    }

    /// Add a new actor of `class` built from its defaults (with `mesh` on its mesh component).
    pub(crate) fn add_new(&mut self, class: String, mesh: Option<J>, level: String, cx: &mut Context<Self>) {
        let at = self.view_target();
        self.add_actor_call(level.clone(), json!({ "path": level, "class": class.clone(), "mesh": mesh, "at": at }), format!("Could not add a {class}"), cx);
    }

    /// Add a `StaticMeshActor` showing `mesh` (`{"path": …, "index": …}`) to `level`, where the view looks.
    pub(crate) fn add_static_mesh(&mut self, mesh: J, level: String, cx: &mut Context<Self>) {
        self.add_new("StaticMeshActor".into(), Some(mesh), level, cx);
    }

    fn palette_enter(&mut self, cx: &mut Context<Self>) {
        if !self.palette.open {
            return;
        }
        let q = self.palette_query(cx);
        let Some(level) = self.palette.level.clone() else { return };
        let count = self.palette_count(&q);
        if count == 0 {
            return;
        }
        let idx = self.palette.selected_row.min(count - 1);
        match self.palette.mode {
            AddMode::Copy => {
                if let Some(c) = self.copies(&q).get(idx).cloned() {
                    self.palette.open = false;
                    self.add_copy(&c["example"], level, cx);
                }
            }
            AddMode::Class => {
                if let Some(c) = self.new_classes(&q).get(idx).and_then(|c| c["class"].as_str()).map(str::to_string) {
                    self.palette.open = false;
                    self.add_new(c, None, level, cx);
                }
            }
            AddMode::Mesh => {
                if let Some(h) = self.palette.meshes.get(idx) {
                    let mesh = json!({ "path": h["path"], "index": h["index"] });
                    self.palette.open = false;
                    self.add_static_mesh(mesh, level, cx);
                }
            }
        }
    }

    pub(super) fn render_palette(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.palette.open {
            return None;
        }
        let theme = cx.theme().clone();
        let q = self.palette_query(cx);
        let mode = self.palette.mode;
        let count = self.palette_count(&q);
        let selected_row = self.palette.selected_row.min(count.saturating_sub(1));
        self.palette.selected_row = selected_row;
        let mut modes = h_flex().gap_0p5();
        for (m, id, label, tip) in [(AddMode::Copy, "pal-copy", "Copy", "A copy of an actor a level places, with its settings"), (AddMode::Class, "pal-class", "New", "A new actor of a class, with the class defaults"), (AddMode::Mesh, "pal-mesh", "Mesh", "A StaticMeshActor showing a mesh of any package")] {
            modes = modes.child(Button::new(id).xsmall().label(label).tooltip(tip).selected(mode == m).on_click(cx.listener(move |this, _, window, cx| {
                this.palette.mode = m;
                this.palette.selected_row = 0;
                this.palette.scroll.scroll_to_item(0);
                if m == AddMode::Mesh {
                    let q = this.palette.filter.as_ref().map(|f| f.read(cx).value().to_string()).unwrap_or_default();
                    this.search_meshes(q, cx);
                }
                if let Some(f) = &this.palette.filter {
                    f.update(cx, |s, cx| s.focus(window, cx));
                }
                cx.notify();
            })));
        }
        let me = cx.entity().downgrade();
        let levels: Vec<(String, String)> = self.levels.iter().map(|l| (l.path.clone(), l.name.clone())).collect();
        let current = self.palette.level.as_ref().and_then(|p| self.level(p)).map(|l| l.name.clone()).unwrap_or_default();
        let level_menu = Button::new("pal-level").xsmall().flex_1().label(current).dropdown_caret(true).tooltip("Into this level").dropdown_menu(move |menu, _, _| {
            let mut menu = menu;
            for (p, n) in &levels {
                let (me, p) = (me.clone(), p.clone());
                menu = menu.item(PopupMenuItem::new(n.clone()).on_click(move |_, _, cx| {
                    let _ = me.update(cx, |this, cx| {
                        this.palette.level = Some(p.clone());
                        this.palette.selected_row = 0;
                        this.palette.scroll.scroll_to_item(0);
                        this.load_actor_palette(p.clone(), cx);
                        cx.notify();
                    });
                }));
            }
            menu
        });
        let note = match mode {
            AddMode::Copy => "A copy of an actor a level places, with its settings. From another map its meshes, materials and textures come along.",
            AddMode::Class => "A new actor with the class defaults: set what it needs (mesh, target, team…) in the inspector. Volumes get a 256 cube: reshape it in the inspector.",
            AddMode::Mesh => "A new StaticMeshActor showing the mesh; a mesh from another package is copied in with its materials and textures. It has no baked lighting. Type part of a mesh's name or path.",
        };
        let level = self.palette.level.clone().unwrap_or_default();
        let (ring, hover_bg, active_bg, sec_bg, muted_fg) = (theme.ring, theme.list_hover, theme.list_active, theme.secondary, theme.muted_foreground);
        let row = move |id: (&'static str, usize), sel: bool, color: gpui::Hsla, name: String, right: String, badge: Option<&'static str>| {
            h_flex()
                .id(id)
                .px_2()
                .py_0p5()
                .gap_2()
                .rounded_sm()
                .cursor_pointer()
                .focusable()
                .tab_index(0)
                .border_1()
                .border_color(gpui::transparent_black())
                .focus_visible(move |s| s.border_color(ring))
                .when(sel, |s| s.bg(active_bg))
                .when(!sel, |s| s.hover(move |s| s.bg(hover_bg)))
                .child(div().size(px(8.0)).flex_none().rounded_full().bg(color))
                .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family(crate::studio::ui::mono()).child(name))
                .children(badge.map(|b| div().px_1().rounded_sm().bg(sec_bg).text_xs().child(b)))
                .child(div().text_color(muted_fg).child(right))
        };
        let list: Vec<AnyElement> = if self.palette.classes.is_none() && mode != AddMode::Mesh {
            vec![div().p_2().text_color(theme.muted_foreground).child("Listing the actors…").into_any_element()]
        } else {
            match mode {
                AddMode::Copy => self
                    .copies(&q)
                    .into_iter()
                    .take(300)
                    .enumerate()
                    .map(|(k, c)| {
                        let (l, l_key) = (level.clone(), level.clone());
                        let ex_key = c["example"].clone();
                        row(("pal-row", k), k == selected_row, category_color(c["category"].as_str().unwrap_or(""), &theme), c["class"].as_str().unwrap_or("").to_string(), c["count"].to_string(), (c["inMap"] != true).then_some("other map"))
                            .child(crate::driver::probe(format!("pal-{}", c["class"].as_str().unwrap_or(""))))
                            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                                if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.palette.open = false;
                                    this.add_copy(&ex_key, l_key.clone(), cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.palette.open = false;
                                this.add_copy(&c["example"], l.clone(), cx);
                            }))
                            .into_any_element()
                    })
                    .collect(),
                AddMode::Class => self
                    .new_classes(&q)
                    .into_iter()
                    .take(400)
                    .enumerate()
                    .map(|(k, c)| {
                        let (l, l_key) = (level.clone(), level.clone());
                        let cls = c["class"].as_str().unwrap_or("").to_string();
                        let cls_key = cls.clone();
                        row(("pal-new", k), k == selected_row, category_color(c["category"].as_str().unwrap_or(""), &theme), cls.clone(), c["package"].as_str().unwrap_or("").to_string(), c["volume"].as_bool().filter(|v| *v).map(|_| "volume"))
                            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                                if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.palette.open = false;
                                    this.add_new(cls_key.clone(), None, l_key.clone(), cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.palette.open = false;
                                this.add_new(cls.clone(), None, l.clone(), cx);
                            }))
                            .into_any_element()
                    })
                    .collect(),
                AddMode::Mesh => self
                    .palette
                    .meshes
                    .clone()
                    .into_iter()
                    .enumerate()
                    .map(|(k, h)| {
                        let (l, l_key) = (level.clone(), level.clone());
                        let mesh = json!({ "path": h["path"], "index": h["index"] });
                        let mesh_key = mesh.clone();
                        row(("pal-meshes", k), k == selected_row, theme.muted_foreground, h["object"].as_str().unwrap_or("").to_string(), h["package"].as_str().unwrap_or("").to_string(), None)
                            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                                if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.palette.open = false;
                                    this.add_static_mesh(mesh_key.clone(), l_key.clone(), cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.palette.open = false;
                                this.add_static_mesh(mesh.clone(), l.clone(), cx);
                            }))
                            .into_any_element()
                    })
                    .collect(),
            }
        };
        Some(
            v_flex()
                .id("actor-palette")
                .absolute()
                .top(px(4.0))
                .right(px(8.0))
                .w(px(384.0))
                .max_w(gpui::relative(0.94))
                .max_h(px(460.0))
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_xs()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_scroll(|_, _, cx| cx.stop_propagation())
                .capture_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                    if !e.keystroke.modifiers.modified() {
                        match e.keystroke.key.as_str() {
                            "up" => {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.step_palette(-1, cx);
                            }
                            "down" => {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.step_palette(1, cx);
                            }
                            _ => {}
                        }
                    }
                }))
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                    if e.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.palette.open = false;
                        if let Some(f) = &this.view.focus {
                            f.focus(window, cx);
                        }
                        cx.notify();
                    }
                }))
                .child(h_flex().p_1p5().gap_1p5().child(modes).child(level_menu).child(Button::new("pal-close").xsmall().ghost().label("Close").on_click(cx.listener(|this, _, window, cx| {
                    this.palette.open = false;
                    if let Some(f) = &this.view.focus {
                        f.focus(window, cx);
                    }
                    cx.notify();
                }))))
                .children(self.palette.filter.as_ref().map(|f| div().px_1p5().pb_1().child(Input::new(f).xsmall().cleanable(true))))
                .child(div().px_2().pb_1().text_xs().text_color(theme.muted_foreground).child(format!("{note} Placed where the view looks; navigation paths are not rebuilt.")))
                .child(v_flex().id("pal-list").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.palette.scroll).pb_1().children(list))
                .into_any_element(),
        )
    }

    // ----- export -------------------------------------------------------------------------

    pub(super) fn open_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Start from what the view shows.
        self.focus.focus(window, cx);
        self.focus_modal = true;
        self.bake.open = false;
        self.palette.open = false;
        self.export.open = true;
        self.export.levels = self.levels.iter().map(|l| l.path.clone()).filter(|p| !self.hidden_levels.contains(p)).collect();
        self.export.skip = self.hidden_categories.clone();
        self.export.show_skip = None;
        if self.export.size == 0 {
            self.export.size = 1024;
            self.export.textures = true;
        }
        cx.notify();
    }

    pub(super) fn commit_export_field(&mut self, _key: &str, _value: String, _cx: &mut Context<Self>) {}

    /// `map.export` into `out` (.glb or .fbx by its extension), with the dialog's volumes, textures and size.
    pub(crate) fn export_to(&mut self, out: String, levels: Option<Vec<String>>, skip: Option<Vec<String>>, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else { return };
        let (volumes, textures, size) = (self.export.volumes, self.export.textures, self.export.size);
        self.export.busy = true;
        let call = self.call("map.export", json!({ "path": path, "out": out, "levels": levels, "skip": skip, "volumes": volumes, "textures": textures, "textureSize": size }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.export.busy = false;
                this.progress = None;
                match r {
                    Ok(r) => {
                        let j = &r.json;
                        let warnings = j["warningCount"].as_u64().unwrap_or(0);
                        let text = format!(
                            "Map exported: {}: {} placements of {} meshes, {}{}",
                            super::file_name(j["out"].as_str().unwrap_or("")),
                            j["placements"],
                            j["meshes"],
                            crate::model::human_size(j["bytes"].as_u64().unwrap_or(0)),
                            if warnings > 0 {
                                format!(
                                    "; {warnings} {}: {}",
                                    if warnings == 1 { "warning" } else { "warnings" },
                                    j["warnings"].as_array().into_iter().flatten().take(3).filter_map(|w| w.as_str()).collect::<Vec<_>>().join("; ")
                                )
                            } else {
                                String::new()
                            }
                        );
                        this.toast(if warnings > 0 { Tone::Warning } else { Tone::Success }, text, cx);
                        this.export.open = false;
                        this.last_export = Some(r.json);
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not export the map: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn run_export(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else { return };
        let name = super::file_name(&path).trim_end_matches(".upk").trim_end_matches(".UPK").to_string();
        let ext = if self.export.fbx { "fbx" } else { "glb" };
        let dir = std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        let target = cx.prompt_for_new_path(&dir, Some(&format!("{name}.{ext}")));
        let (levels, skip) = (self.export.levels.iter().cloned().collect::<Vec<_>>(), self.export.skip.iter().cloned().collect::<Vec<_>>());
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(out))) = target.await else { return };
            let _ = this.update(cx, |this, cx| this.export_to(out.to_string_lossy().to_string(), Some(levels), Some(skip), cx));
        })
        .detach();
    }

    pub(super) fn render_export(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.export.open {
            return None;
        }
        let theme = cx.theme().clone();
        let mut body = v_flex().gap_3().text_xs();
        body = body.child(div().text_color(theme.muted_foreground).child("One file with every placed mesh, its materials and textures. Each mesh is stored once and placed by a node per placement, grouped by level; positions in metres (GLB) or centimetres (FBX), Y up."));
        body = body.child(
            h_flex()
                .gap_1()
                .child(Button::new("exp-glb").small().label("GLB (glTF)").selected(!self.export.fbx).on_click(cx.listener(|this, _, _, cx| {
                    this.export.fbx = false;
                    cx.notify();
                })))
                .child(Button::new("exp-fbx").small().label("FBX").selected(self.export.fbx).on_click(cx.listener(|this, _, _, cx| {
                    this.export.fbx = true;
                    cx.notify();
                }))),
        );
        let mut levels = div().grid().grid_cols(2).gap_x_3();
        for (k, l) in self.levels.iter().enumerate() {
            let p = l.path.clone();
            levels = levels.child(Checkbox::new(("exp-lvl", k)).label(l.name.clone()).checked(self.export.levels.contains(&l.path)).on_click(cx.listener(move |this, v: &bool, _, cx| {
                if *v {
                    this.export.levels.insert(p.clone());
                } else {
                    this.export.levels.remove(&p);
                }
                cx.notify();
            })));
        }
        body = body.child(v_flex().gap_1().child(div().text_color(theme.muted_foreground).child("Levels")).child(levels));
        let skip_count = CATEGORIES.iter().filter(|(id, _)| self.export.skip.contains(*id)).count();
        let show_skip = self.export.show_skip.unwrap_or(skip_count > 0);
        let skip_summary = if skip_count == 0 {
            "Leave out actor types (none)".to_string()
        } else {
            format!("Leave out actor types ({skip_count} left out)")
        };
        let ring = theme.ring;
        let hover_bg = theme.list_hover;
        let skip_toggle = h_flex()
            .id("exp-skip-toggle")
            .min_h(px(24.0))
            .px_1()
            .py_0p5()
            .gap_1()
            .items_center()
            .rounded_sm()
            .cursor_pointer()
            .focusable()
            .tab_index(0)
            .border_1()
            .border_color(gpui::transparent_black())
            .focus_visible(move |s| s.border_color(ring))
            .hover(move |s| s.bg(hover_bg))
            .child(Icon::new(if show_skip { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall().text_color(theme.muted_foreground))
            .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().font_weight(gpui::FontWeight::MEDIUM).text_color(if skip_count > 0 { theme.foreground } else { theme.muted_foreground }).child(skip_summary))
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                    window.prevent_default();
                    cx.stop_propagation();
                    let cur = this.export.show_skip.unwrap_or_else(|| CATEGORIES.iter().any(|(id, _)| this.export.skip.contains(*id)));
                    this.export.show_skip = Some(!cur);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                let cur = this.export.show_skip.unwrap_or_else(|| CATEGORIES.iter().any(|(id, _)| this.export.skip.contains(*id)));
                this.export.show_skip = Some(!cur);
                cx.notify();
            }));
        let mut skip = div().grid().grid_cols(3).gap_x_3().px_1();
        for (k, (id, label)) in CATEGORIES.iter().enumerate() {
            skip = skip.child(Checkbox::new(("exp-skip", k)).label(*label).checked(self.export.skip.contains(*id)).on_click(cx.listener(move |this, v: &bool, _, cx| {
                this.export.show_skip = Some(true);
                if *v {
                    this.export.skip.insert(id.to_string());
                } else {
                    this.export.skip.remove(*id);
                }
                cx.notify();
            })));
        }
        body = body.child(v_flex().gap_1().child(skip_toggle).when(show_skip, |d| d.child(skip)));
        body = body.child(Checkbox::new("exp-volumes").label("Volumes as meshes (blocking, triggers… of the kinds kept)").checked(self.export.volumes).on_click(cx.listener(|this, v: &bool, _, cx| {
            this.export.volumes = *v;
            cx.notify();
        })));
        let me = cx.entity().downgrade();
        let size = self.export.size;
        let tex_on = self.export.textures;
        body = body.child(
            h_flex()
                .gap_2()
                .child(Checkbox::new("exp-textures").label("Textures").checked(tex_on).on_click(cx.listener(|this, v: &bool, _, cx| {
                    this.export.textures = *v;
                    cx.notify();
                })))
                .child(
                    Button::new("exp-size")
                        .xsmall()
                        .disabled(!tex_on)
                        .when(!tex_on, |b| b.tooltip("Turn on Textures on the left to choose a texture size"))
                        .label(if size >= 8192 { "full size".to_string() } else { format!("up to {size}²") })
                        .dropdown_menu(move |menu, _, _| {
                            let mut menu = menu;
                            for (s, l) in [(512u32, "up to 512²"), (1024, "up to 1024²"), (2048, "up to 2048²"), (8192, "full size")] {
                                let me = me.clone();
                                menu = menu.item(PopupMenuItem::new(l).on_click(move |_, _, cx| {
                                    let _ = me.update(cx, |this, cx| {
                                        this.export.size = s;
                                        cx.notify();
                                    });
                                }));
                            }
                            menu
                        }),
                ),
        );
        let progress = self.progress.as_ref().filter(|p| p.0 == "map.progress").map(|p| format!(" {}%", p.1 * 100 / p.2.max(1))).unwrap_or_else(|| "…".into());
        let busy = self.export.busy;
        let no_levels = self.export.levels.is_empty();
        body = body.child(
            h_flex()
                .gap_2()
                .justify_end()
                .when(busy, |d| d.child(div().mr_auto().text_color(theme.muted_foreground).child(format!("Exporting{progress}"))))
                .child(
                    Button::new("exp-cancel")
                        .small()
                        .ghost()
                        .label("Cancel")
                        .disabled(busy)
                        .when(busy, |b| b.tooltip("Wait for the map export to finish"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.export.open = false;
                            this.focus.focus(window, cx);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("exp-run")
                        .small()
                        .label(if busy { "Exporting…" } else { "Export…" })
                        .disabled(busy || no_levels)
                        .when(busy || no_levels, |b| b.tooltip(if busy { "Wait for the map export to finish" } else { "Select at least one level above to export" }))
                        .on_click(cx.listener(|this, _, _, cx| this.run_export(cx))),
                ),
        );
        Some(modal("Export the map", 520.0, body.into_any_element(), cx))
    }
}
