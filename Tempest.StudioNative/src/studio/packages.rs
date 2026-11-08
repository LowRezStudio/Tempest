//! The Packages view: the game folder's packages, the open package's
//! exports as a tree (outers as parents), and the inspector. Both lists take
//! the arrow keys, open with a click, in a new tab with Cmd/Ctrl-click or a
//! middle click, and have a context menu with what can be done to the row.

use std::ops::Range;
use std::sync::Arc;

use gpui::{div, prelude::*, px, uniform_list, Context, Image, ImageFormat, ScrollStrategy, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Selectable, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{file_name, layout, mesh, Cmd, Place, Studio, Tone, ROW};
use crate::model::{self, Tables};

impl Studio {
    pub(super) fn open_package(&mut self, path: String, cx: &mut Context<Self>) {
        if self.selected_package.as_deref() == Some(&path) {
            if let Some(i) = self.pending_export.take() {
                self.reveal(i, cx);
                self.select_export(i, cx);
            }
            return;
        }
        self.selected_package = Some(path.clone());
        if let Some(k) = self.shown_packages.iter().position(|&i| self.packages[i].path == path) {
            self.package_scroll.scroll_to_item(k, ScrollStrategy::Center);
        }
        self.set_status(format!("Opening {}…", file_name(&path)), cx);
        let t = std::time::Instant::now();
        let call = self.backend.call("package.open", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                // Another package was picked meanwhile.
                if this.selected_package.as_deref() != Some(&path) {
                    return;
                }
                match r {
                    Ok(r) => {
                        let tables = Tables::from_json(&path, &r.json);
                        let n = tables.exports.len();
                        this.open = Some(tables);
                        this.expanded.clear();
                        this.clear_selection();
                        // Small packages open with their groups expanded.
                        if n <= 400 {
                            if let Some(t) = &this.open {
                                this.expanded.extend((0..n).filter(|&i| t.has_children(i) && t.parent[i].is_none()));
                            }
                        }
                        this.refresh_rows(cx);
                        this.export_scroll.scroll_to_item(0, ScrollStrategy::Top);
                        if let Some(i) = this.pending_export.take().filter(|&i| i < n) {
                            this.reveal(i, cx);
                            this.select_export(i, cx);
                        }
                        this.set_status(format!("{}: {n} objects ({} ms)", file_name(&path), t.elapsed().as_millis()), cx);
                    }
                    Err(e) => this.toast(Tone::Error, format!("{}: {e}", file_name(&path)), cx),
                }
            });
        })
        .detach();
    }

    pub(super) fn clear_selection(&mut self) {
        self.selected_export = None;
        self.inspected = None;
        self.preview = None;
        self.mesh = None;
        self.material = None;
        self.slots = None;
        self.sound = None;
        self.wwise = None;
        self.anim = None;
        self.particles = None;
        self.deps = None;
        self.source = None;
        self.player.stop();
    }

    /// New tables of package `path` (after an edit): keep the selection when
    /// it still exists, else select `select`.
    pub(super) fn reload_tables(&mut self, path: &str, json: &J, select: Option<usize>, cx: &mut Context<Self>) {
        if self.open.as_ref().map(|o| o.path.as_str()) != Some(path) {
            return;
        }
        self.open = Some(Tables::from_json(path, json));
        self.refresh_rows(cx);
        let n = self.open.as_ref().map(|o| o.exports.len()).unwrap_or(0);
        match select.or(self.selected_export).filter(|&i| i < n) {
            Some(i) => {
                self.reveal(i, cx);
                self.mesh = None;
                self.material = None;
                self.slots = None;
                self.sound = None;
                self.particles = None;
                self.deps = None;
                // The package changed under its movie editors: they read their movies again.
                for ((p, _), m) in self.movies.clone() {
                    if p == path {
                        m.update(cx, |m, cx| m.reload(cx));
                    }
                }
                self.source = None;
                self.select_export(i, cx);
            }
            None => {
                self.clear_selection();
                cx.notify();
            }
        }
    }

    /// Read package `path`'s tables again (after an edit made elsewhere).
    pub(crate) fn refresh_package(&mut self, path: String, select: Option<usize>, cx: &mut Context<Self>) {
        let call = self.backend.call("package.tables", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            if let Ok(t) = call.await {
                let _ = this.update(cx, |this, cx| this.reload_tables(&path, &t.json, select, cx));
            }
        })
        .detach();
    }

    pub(super) fn refresh_rows(&mut self, cx: &mut Context<Self>) {
        let filter = self.export_filter.read(cx).value().to_string();
        self.rows = self.open.as_ref().map(|t| t.visible(&self.expanded, &filter)).unwrap_or_default();
        cx.notify();
    }

    /// Expand the parents of export `i` and scroll it into view.
    pub(super) fn reveal(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(t) = &self.open else { return };
        let mut p = t.parent.get(i).copied().flatten();
        while let Some(q) = p {
            self.expanded.insert(q);
            p = t.parent[q];
        }
        self.refresh_rows(cx);
        if let Some(k) = self.rows.iter().position(|r| r.0 == i) {
            self.export_scroll.scroll_to_item(k, ScrollStrategy::Center);
        }
    }

    pub(super) fn toggle(&mut self, i: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(&i) {
            self.expanded.insert(i);
        }
        self.refresh_rows(cx);
    }

    pub(super) fn select_export(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(t) = &self.open else { return };
        if self.selected_export != Some(i) {
            let keep_mesh = self.mesh.as_ref().is_some_and(|m| m.borrow().export == i);
            self.deps = None;
            self.source = None;
            if !keep_mesh {
                self.mesh = None;
            }
            self.material = None;
            self.slots = None;
            self.sound = None;
            self.wwise = None;
            self.anim = None;
            self.player.stop();
        }
        self.selected_export = Some(i);
        let path = t.path.clone();
        let class = t.exports[i].class.to_ascii_lowercase();
        let get = self.backend.call("object.get", json!({ "path": path, "index": i }));
        // Textures of every kind preview as a PNG.
        let preview = (class.contains("texture") || class.contains("lightmap")).then(|| self.backend.call("texture.preview", json!({ "path": path, "index": i })));
        if preview.is_some() && self.tex.as_ref().is_none_or(|t| t.export != i) {
            self.load_texture_info(path.clone(), i, cx);
        }
        if mesh::is_mesh(&class) {
            if self.mesh.as_ref().is_none_or(|m| m.borrow().export != i) {
                self.load_mesh(path.clone(), i, 0, cx);
            }
            self.load_slots(path.clone(), i, cx);
        }
        if super::material::is_material(&class) {
            self.load_material(path.clone(), i, cx);
        }
        if super::anim::is_anim(&class) {
            self.load_anim(path.clone(), i, None, cx);
        }
        // Any export of the system shown keeps the preview running.
        if super::particle::is_particle(&class) {
            self.load_particles(path.clone(), i, cx);
        } else {
            self.particles = None;
        }
        // A movie opens on its editor; other objects on their properties.
        if super::swf::is_movie(&class) {
            if self.tab == super::inspector::Tab::Properties {
                self.tab = super::inspector::Tab::Movie;
            }
        } else if self.tab == super::inspector::Tab::Movie {
            self.tab = super::inspector::Tab::Properties;
        }
        if super::wwise::is_wwise(&class) {
            self.load_wwise(path.clone(), i, cx);
        }
        if class == "soundnodewave" {
            self.load_sound(path.clone(), i, cx);
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = get.await;
            let _ = this.update(cx, |this, cx| {
                if this.selected_export != Some(i) {
                    return;
                }
                match r {
                    Ok(r) => this.inspected = Some(model::inspected(&r.json)),
                    Err(e) => this.set_status(e, cx),
                }
                this.preview = None;
                this.load_tab(cx);
                cx.notify();
            });
            if let Some(preview) = preview {
                let r = preview.await;
                let _ = this.update(cx, |this, cx| {
                    if this.selected_export != Some(i) {
                        return;
                    }
                    match r {
                        Ok(r) => this.preview = r.bin(&r.json["png"]).map(|png| Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec()))),
                        Err(e) => this.set_status(format!("Preview: {e}"), cx),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    // ----- keyboard ---------------------------------------------------------------

    /// Up/Down/First/Last in the package list: the package there opens.
    fn package_step(&mut self, to: impl Fn(Option<usize>, usize) -> usize, cx: &mut Context<Self>) {
        let n = self.shown_packages.len();
        if n == 0 {
            return;
        }
        let at = self.shown_packages.iter().position(|&i| Some(self.packages[i].path.as_str()) == self.selected_package.as_deref());
        let k = to(at, n).min(n - 1);
        let path = self.packages[self.shown_packages[k]].path.clone();
        self.package_scroll.scroll_to_item(k, ScrollStrategy::Nearest);
        self.user_open_package(path, cx);
    }

    fn export_step(&mut self, to: impl Fn(Option<usize>, usize) -> usize, cx: &mut Context<Self>) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let at = self.rows.iter().position(|r| Some(r.0) == self.selected_export);
        let k = to(at, n).min(n - 1);
        self.export_scroll.scroll_to_item(k, ScrollStrategy::Nearest);
        self.user_select_export(self.rows[k].0, cx);
    }

    pub(super) fn list_step(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let f = move |at: Option<usize>, n: usize| -> usize {
            match step {
                isize::MIN => 0,
                isize::MAX => n - 1,
                s => at.map(|a| (a as isize + s).clamp(0, n as isize - 1) as usize).unwrap_or(0),
            }
        };
        if self.packages_focus.is_focused(window) {
            self.package_step(f, cx);
        } else {
            self.export_step(f, cx);
        }
    }

    /// Right: expand, or go to the first child of an expanded row.
    pub(super) fn on_right(&mut self, cx: &mut Context<Self>) {
        let (Some(t), Some(i)) = (&self.open, self.selected_export) else { return };
        if !t.has_children(i) {
            return;
        }
        if self.expanded.contains(&i) {
            let first = t.children[i + 1][0];
            let k = self.rows.iter().position(|r| r.0 == first).unwrap_or(0);
            self.export_scroll.scroll_to_item(k, ScrollStrategy::Nearest);
            self.user_select_export(first, cx);
        } else {
            self.toggle(i, cx);
        }
    }

    /// Left: collapse, or go to the parent.
    pub(super) fn on_left(&mut self, cx: &mut Context<Self>) {
        let (Some(t), Some(i)) = (&self.open, self.selected_export) else { return };
        if self.expanded.contains(&i) {
            self.toggle(i, cx);
        } else if let Some(p) = t.parent[i] {
            if let Some(k) = self.rows.iter().position(|r| r.0 == p) {
                self.export_scroll.scroll_to_item(k, ScrollStrategy::Nearest);
            }
            self.user_select_export(p, cx);
        }
    }

    /// What a click or a key does: open/select, and record the place.
    pub(crate) fn user_open_package(&mut self, path: String, cx: &mut Context<Self>) {
        self.view = super::View::Packages;
        layout::update(&self.panes, |p| p.overlay = false);
        self.record(Place::package(path.clone(), None));
        self.open_package(path, cx);
    }

    pub(crate) fn user_select_export(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(t) = &self.open {
            self.record(Place::package(t.path.clone(), Some(i)));
        }
        self.select_export(i, cx);
    }

    /// Open an object of any package by its path (`pkg.group.name`, found
    /// through the search index).
    pub(crate) fn open_object_path(&mut self, object: String, class: Option<String>, cx: &mut Context<Self>) {
        let call = self.backend.call("workspace.search", json!({ "query": object, "limit": 50 }));
        let here = self.open.as_ref().map(|t| t.path.clone()).unwrap_or_default();
        self.set_status(format!("Looking for {object}…"), cx);
        cx.spawn(async move |this, cx| {
            let found = call.await.ok().and_then(|res| {
                res.json["results"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|h| h["object"].as_str().is_some_and(|o| o.eq_ignore_ascii_case(&object)))
                    // The real object, not an empty placeholder in some package: prefer its class.
                    .max_by_key(|h| (class.as_ref().is_some_and(|c| h["class"].as_str().is_some_and(|hc| hc.eq_ignore_ascii_case(c))), h["path"].as_str() != Some(here.as_str())))
                    .map(|h| Place::package(h["path"].as_str().unwrap_or_default(), h["index"].as_u64().map(|i| i as usize)))
            });
            let _ = this.update(cx, |this, cx| match found {
                Some(place) => {
                    this.record(place.clone());
                    if let Place::Package { package, export, .. } = place {
                        this.view = super::View::Packages;
                        this.pending_export = export;
                        this.open_package(package, cx);
                    }
                }
                None => this.toast(Tone::Warning, format!("{object} is in no package of this folder (a script object, or a package that is not here)"), cx),
            });
        })
        .detach();
    }

    // ----- views -----------------------------------------------------------------

    fn package_menu(&self, path: String, menu: PopupMenu, cx: &mut Context<Self>) -> PopupMenu {
        let me = cx.entity().downgrade();
        let dirty = self.dirty.contains(&path);
        type Action = dyn Fn(&mut Studio, &mut Window, &mut Context<Studio>);
        let item = |label: &str, f: Box<Action>| {
            let me = me.clone();
            let f: std::rc::Rc<Action> = f.into();
            PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                let f = f.clone();
                let _ = me.update(cx, |this, cx| f(this, window, cx));
            })
        };
        let (p1, p2, p3, p4, p5) = (path.clone(), path.clone(), path.clone(), path.clone(), path.clone());
        let (p6, p7, p8, p9) = (path.clone(), path.clone(), path.clone(), path.clone());
        let pinned = self.is_pinned(&path);
        menu.item(item("Open in a new tab", Box::new(move |s, _, cx| s.new_tab(Some(Place::package(p1.clone(), None)), cx))))
            .item(item(if pinned { "Unpin" } else { "Pin to the top of the list" }, Box::new(move |s, _, cx| s.toggle_pin(p6.clone(), cx))))
            .separator()
            .item(item("Copy the file path", Box::new(move |s, _, cx| s.copy_text(p7.clone(), "file path", cx))))
            .item(item(if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in the file manager" }, Box::new(move |s, _, cx| s.reveal_file(&p8, cx))))
            .separator()
            .item(item("Save into the game folder…", Box::new(move |s, w, cx| s.save_packages(vec![p2.clone()], w, cx))).disabled(!dirty))
            .item(item("Export a copy…", Box::new(move |s, _, cx| s.save_copy(p3.clone(), cx))))
            .item(item("Discard the changes…", Box::new(move |s, w, cx| s.discard(p4.clone(), w, cx))).disabled(!dirty))
            .separator()
            .item(item("Check the package (verify)", Box::new(move |s, w, cx| s.verify(p5.clone(), w, cx))))
            .item(item("Package info…", Box::new(move |s, w, cx| s.package_info(p9.clone(), w, cx))))
    }

    fn export_menu(&self, i: usize, menu: PopupMenu, cx: &mut Context<Self>) -> PopupMenu {
        let me = cx.entity().downgrade();
        let item = |label: &str, cmd: Cmd| {
            let me = me.clone();
            PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                let _ = me.update(cx, |this, cx| {
                    if this.selected_export != Some(i) {
                        this.user_select_export(i, cx);
                    }
                    this.run(cmd, window, cx);
                });
            })
        };
        let group = self.open.as_ref().is_some_and(|t| t.exports[i].class.eq_ignore_ascii_case("package"));
        let me2 = me.clone();
        let path = self.open.as_ref().map(|t| t.path.clone()).unwrap_or_default();
        let mut menu = menu
            .item(PopupMenuItem::new("Open in a new tab").on_click(move |_, _, cx| {
                let _ = me2.update(cx, |this, cx| this.new_tab(Some(Place::package(path.clone(), Some(i))), cx));
            }))
            .item(item("Copy the object path", Cmd::CopyPath))
            .separator()
            .item(item("Rename", Cmd::Rename))
            .item(item("Duplicate", Cmd::Duplicate))
            .item(item("Copy to another package…", Cmd::CopyTo))
            .separator()
            .item(item("Export to a file…", Cmd::ExportObject))
            .item(item("Replace from a file…", Cmd::ReplaceObject))
            .item(item("Replace with an object of another package…", Cmd::ReplaceWith));
        if group {
            menu = menu.separator().item(item("Add an object here…", Cmd::AddObject)).item(item("Add a group here…", Cmd::AddGroup));
        }
        menu.separator().item(item("Delete…", Cmd::Delete))
    }

    fn render_packages(&mut self, range: Range<usize>, _window: &mut Window, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let theme = cx.theme().clone();
        let pinned = self.pinned();
        range
            .filter_map(|k| self.shown_packages.get(k).map(|&i| (k, i)))
            .map(|(k, i)| {
                let p = &self.packages[i];
                let selected = self.selected_package.as_deref() == Some(p.path.as_str());
                let path = p.path.clone();
                let badge = if p.map { "map" } else if p.script { "script" } else { "" };
                let menu_path = path.clone();
                let me = cx.entity();
                h_flex()
                    .id(("pkg", k))
                    .w_full()
                    .h(px(ROW))
                    .px_2()
                    .gap_2()
                    .text_sm()
                    .when(selected, |d| d.bg(theme.list_active))
                    .hover(|s| s.bg(theme.list_hover))
                    .on_click({
                        let path = path.clone();
                        cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
                            this.packages_focus.focus(window, cx);
                            if ev.modifiers().secondary(cx) {
                                this.new_tab(Some(Place::package(path.clone(), None)), cx);
                            } else {
                                this.user_open_package(path.clone(), cx);
                            }
                        })
                    })
                    .on_mouse_down(gpui::MouseButton::Middle, cx.listener(move |this, _, _, cx| this.new_tab(Some(Place::package(path.clone(), None)), cx)))
                    .when(pinned.contains(&p.path), |d| d.child(Icon::new(IconName::Pin).xsmall().text_color(theme.muted_foreground)))
                    .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(p.name.clone()))
                    .when(self.dirty.contains(&p.path), |d| d.child(div().size(px(6.0)).rounded_full().bg(theme.warning)))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(badge))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(model::human_size(p.size)))
                    .child(crate::driver::probe(format!("pkg-{k}")))
                    .context_menu(move |menu, _, cx| me.update(cx, |s, cx| s.package_menu(menu_path.clone(), menu, cx)))
                    .into_any_element()
            })
            .collect()
    }

    fn render_exports(&mut self, range: Range<usize>, _window: &mut Window, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let theme = cx.theme().clone();
        let Some(t) = &self.open else { return Vec::new() };
        range
            .filter_map(|k| self.rows.get(k).map(|&r| (k, r)))
            .map(|(k, (i, depth))| {
                let e = &t.exports[i];
                let selected = self.selected_export == Some(i);
                let parent = t.has_children(i);
                let open = self.expanded.contains(&i);
                let me = cx.entity();
                h_flex()
                    .id(("exp", k))
                    .w_full()
                    .h(px(ROW))
                    .pl(px(8.0 + depth as f32 * 14.0))
                    .pr_2()
                    .gap_1()
                    .text_sm()
                    .when(selected, |d| d.bg(theme.list_active))
                    .hover(|s| s.bg(theme.list_hover))
                    .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
                        this.exports_focus.focus(window, cx);
                        if ev.modifiers().secondary(cx) {
                            let package = this.open.as_ref().map(|t| t.path.clone()).unwrap_or_default();
                            this.new_tab(Some(Place::package(package, Some(i))), cx);
                            return;
                        }
                        // A group opens with a click (and closes with the next); other objects with children on a double click.
                        let group = this.open.as_ref().is_some_and(|t| t.exports[i].class.eq_ignore_ascii_case("package"));
                        let toggle = if group { ev.click_count() == 1 && (!this.expanded.contains(&i) || this.selected_export == Some(i)) } else { ev.click_count() > 1 };
                        if parent && toggle {
                            this.toggle(i, cx);
                        }
                        this.user_select_export(i, cx);
                    }))
                    .on_mouse_down(gpui::MouseButton::Middle, cx.listener(move |this, _, _, cx| {
                        let package = this.open.as_ref().map(|t| t.path.clone()).unwrap_or_default();
                        this.new_tab(Some(Place::package(package, Some(i))), cx);
                    }))
                    .child(
                        div()
                            .id(("twist", k))
                            .w(px(14.0))
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .when(parent, |d| {
                                d.child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall()).on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.toggle(i, cx);
                                }))
                            }),
                    )
                    .child(Icon::new(super::class_icon(&e.class)).xsmall().text_color(if e.class.eq_ignore_ascii_case("package") { theme.warning.opacity(0.8) } else { theme.muted_foreground }))
                    .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(e.name.clone()))
                    .when(!e.class.eq_ignore_ascii_case("package"), |d| d.child(div().text_xs().text_color(theme.muted_foreground).whitespace_nowrap().child(super::pretty_class(&e.class))))
                    .child(crate::driver::probe(format!("exp-{k}")))
                    .context_menu(move |menu, _, cx| me.update(cx, |s, cx| s.export_menu(i, menu, cx)))
                    .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_packages_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let pane_title = |title: &str, tools: gpui::AnyElement| h_flex().h(px(28.0)).px_2().gap_1().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(div().flex_1().child(title.to_uppercase())).child(tools);
        let packages = v_flex()
            .size_full()
            .child(pane_title(
                "Packages",
                h_flex()
                    .child(
                        Button::new("unsaved-only")
                            .xsmall()
                            .ghost()
                            .selected(self.unsaved_only)
                            .label(if self.dirty.is_empty() { "Unsaved".to_string() } else { format!("Unsaved {}", self.dirty.len()) })
                            .tooltip("Show only the packages with unsaved edits")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.unsaved_only = !this.unsaved_only;
                                this.refilter_packages(cx);
                            })),
                    )
                    .child(Button::new("new-package").xsmall().ghost().icon(Icon::new(IconName::FilePlus)).tooltip("New package…").on_click(cx.listener(|this, _, window, cx| this.run(Cmd::NewPackage, window, cx))))
                    .into_any_element(),
            ))
            .child(div().px_1().pb_1().child(Input::new(&self.package_filter).small()))
            .child(div().key_context("Packages").track_focus(&self.packages_focus).flex_1().min_h_0().child(uniform_list("packages", self.shown_packages.len(), cx.processor(Self::render_packages)).track_scroll(&self.package_scroll).size_full()));
        let title = self.open.as_ref().map(|t| t.name.clone()).unwrap_or_else(|| "Objects".into());
        let has_open = self.open.is_some();
        let exports_body: gpui::AnyElement = if has_open {
            uniform_list("exports", self.rows.len(), cx.processor(Self::render_exports)).track_scroll(&self.export_scroll).size_full().into_any_element()
        } else {
            v_flex().size_full().items_center().justify_center().gap_1().text_sm().text_color(theme.muted_foreground).child("Pick a package on the left").child(div().text_xs().child("or press ⌘P to find one by name")).into_any_element()
        };
        let exports = v_flex()
            .size_full()
            .child(pane_title(
                &title,
                h_flex()
                    .child(Button::new("add-object").xsmall().ghost().icon(Icon::new(IconName::Plus)).tooltip("Add to the package: an object from a file, a copy, a group…").disabled(!has_open).on_click(cx.listener(|this, _, window, cx| this.run(Cmd::AddObject, window, cx))))
                    .child(Button::new("find-replace").xsmall().ghost().icon(Icon::new(IconName::Replace)).tooltip("Find and replace in the package…").disabled(!has_open).on_click(cx.listener(|this, _, window, cx| this.run(Cmd::FindReplace, window, cx))))
                    .into_any_element(),
            ))
            .child(div().px_1().pb_1().child(Input::new(&self.export_filter).small()))
            .child(div().key_context("Exports").track_focus(&self.exports_focus).flex_1().min_h_0().child(exports_body));
        // Focus (a movie's stage) hides both lists.
        let mut panes = self.panes.get();
        if self.focus_mode {
            panes = layout::Panes { side_hidden: true, second_hidden: true, ..Default::default() };
        }
        let sizes = self.pane_sizes("main", &[260.0, 400.0]);
        let me = cx.entity().downgrade();
        let inspector = self.render_inspector(window, cx).into_any_element();
        layout::group(
            "main",
            panes,
            vec![layout::Part(layout::Role::Side, Some(sizes[0]), packages.into_any_element()), layout::Part(layout::Role::Second, Some(sizes[1]), exports.into_any_element()), layout::Part(layout::Role::Main, None, inspector)],
            Self::remember_panes(me.clone(), "main"),
            move |_, cx| {
                let _ = me.update(cx, |s, cx| s.close_overlay(cx));
            },
            cx,
        )
    }

    /// The unsaved files, each with Save and Discard.
    pub(super) fn show_unsaved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt;
        let mut files: Vec<String> = self.dirty.iter().cloned().collect();
        files.sort();
        let me = cx.entity().downgrade();
        let assembly = self.assembly.as_ref().map(|a| a.read(cx).unsaved_files()).unwrap_or_default();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let mut list = v_flex().gap_1();
            for f in &files {
                let (me1, me2, me3, f1, f2, f3) = (me.clone(), me.clone(), me.clone(), f.clone(), f.clone(), f.clone());
                list = list.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(div().flex_1().child(file_name(f)))
                        .child(Button::new(format!("open-{f}")).xsmall().ghost().label("Open").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let _ = me1.update(cx, |this, cx| this.user_open_package(f1.clone(), cx));
                        }))
                        .child(Button::new(format!("save-{f}")).xsmall().label("Save…").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let _ = me2.update(cx, |this, cx| this.save_packages(vec![f2.clone()], window, cx));
                        }))
                        .child(Button::new(format!("discard-{f}")).xsmall().ghost().label("Discard…").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let _ = me3.update(cx, |this, cx| this.discard(f3.clone(), window, cx));
                        })),
                );
            }
            for f in &assembly {
                list = list.child(h_flex().gap_2().text_sm().child(div().flex_1().child(file_name(f))).child(div().text_xs().text_color(theme.muted_foreground).child("assembly · save it from the Assembly view")));
            }
            dialog.title("Unsaved files").w(px(560.0)).child(list)
        });
    }

    /// The package's header in words: versions, compression, counts, flags.
    pub(super) fn package_info(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt;
        let call = self.backend.call("package.open", json!({ "path": path }));
        let me = cx.entity().downgrade();
        let handle = window.window_handle();
        cx.spawn(async move |_, cx| {
            let Ok(r) = call.await else { return };
            let s = r.json["summary"].clone();
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let _ = cx.update_window(handle, |_, window, cx| {
                let me = me.clone();
                window.open_dialog(cx, move |dialog, _, cx| {
                    let theme = cx.theme().clone();
                    let yes = |b: &J| if b.as_bool() == Some(true) { "yes" } else { "no" };
                    let rows: Vec<(&str, String)> = vec![
                        ("File", format!("{} · {}", file_name(&path), model::human_size(size))),
                        ("Objects", format!("{} exports, {} imports, {} names", s["exports"], s["imports"], s["names"])),
                        ("Versions", format!("file {} · licensee {} · engine {} · cooked content {}", s["fileVersion"], s["licenseeVersion"], s["engineVersion"], s["cookedContentVersion"])),
                        ("Stored", format!("{}{} · {} chunks", if s["compressed"].as_bool() == Some(true) { "LZO-compressed" } else { "uncompressed" }, if s["obscured"].as_bool() == Some(true) { ", obscured" } else { "" }, s["chunks"])),
                        ("Cooked", yes(&s["cooked"]).to_string()),
                        ("Flags", s["packageFlagsText"].as_str().unwrap_or("").to_string()),
                        ("GUID", s["guid"].as_str().unwrap_or("").to_string()),
                        ("Texture allocations", s["textureAllocations"].to_string()),
                    ];
                    let mut list = v_flex().gap_1p5();
                    for (k, v) in rows {
                        list = list.child(h_flex().gap_3().items_start().child(div().w(px(140.0)).flex_none().text_xs().text_color(theme.muted_foreground).child(k)).child(div().flex_1().min_w_0().text_sm().child(v)));
                    }
                    let (m1, m2, p1, p2) = (me.clone(), me.clone(), path.clone(), path.clone());
                    dialog.title(format!("{} info", file_name(&path))).w(px(600.0)).child(list).footer(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(Button::new("pi-copy").ghost().label("Copy the path").on_click(move |_, _, cx| {
                                let _ = m1.update(cx, |s, cx| s.copy_text(p1.clone(), "file path", cx));
                            }))
                            .child(Button::new("pi-reveal").ghost().label(if cfg!(target_os = "macos") { "Show in Finder" } else { "Show the file" }).on_click(move |_, _, cx| {
                                let _ = m2.update(cx, |s, cx| s.reveal_file(&p2, cx));
                            }))
                            .child(Button::new("pi-close").primary().label("Close").on_click(|_, window, cx| window.close_dialog(cx))),
                    )
                });
            });
        })
        .detach();
    }
}
