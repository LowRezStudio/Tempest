//! The inspector: the selected export's properties (edited in place, array
//! elements added, duplicated, removed, properties reset or added), its
//! references and what refers to it, and for script objects the owning
//! class's decompiled source with Compile.

use gpui::{div, prelude::*, px, AnyElement, Context, Entity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Editor, EditorState, Input};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{actions, Cmd, Studio, Tone};
use crate::model::{self, Deps, Edit, ListRole, Ref, Tables};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Properties,
    References,
    Source,
    /// A movie's editor.
    Movie,
}

/// The Source tab: the class the selected script object belongs to.
pub struct Source {
    pub export: usize,
    pub class: String,
    /// Text not yet in an editor (the editor needs a window to be made).
    pub text: Option<String>,
    pub editor: Option<Entity<EditorState>>,
    pub error: Option<String>,
}

/// Exports whose class makes them script objects (the Source tab).
pub fn is_script(class: &str) -> bool {
    let c = class.to_ascii_lowercase();
    matches!(c.as_str(), "class" | "function" | "state" | "scriptstruct" | "enum" | "const") || c.ends_with("property")
}

impl Studio {
    /// The export changed (selection) or the tab did: load what the tab shows.
    pub(super) fn load_tab(&mut self, cx: &mut Context<Self>) {
        let (Some(t), Some(i)) = (&self.open, self.selected_export) else { return };
        let path = t.path.clone();
        match self.tab {
            Tab::Properties | Tab::Movie => {}
            Tab::References => {
                if self.deps.as_ref().is_some_and(|(k, _)| *k == i) {
                    return;
                }
                let call = self.backend.call("object.deps", json!({ "path": path, "index": i }));
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| {
                        if this.selected_export != Some(i) {
                            return;
                        }
                        match r {
                            Ok(r) => this.deps = Some((i, model::deps(&r.json))),
                            Err(e) => this.set_status(e, cx),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            Tab::Source => {
                if self.source.as_ref().is_some_and(|s| s.export == i) || !is_script(&t.exports[i].class) {
                    return;
                }
                self.set_status("Decompiling…", cx);
                let call = self.backend.call("script.source", json!({ "path": path, "index": i }));
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| {
                        if this.selected_export != Some(i) {
                            return;
                        }
                        match r {
                            Ok(r) => {
                                let class = r.json["class"].as_str().unwrap_or_default().to_string();
                                this.set_status(format!("class {class}"), cx);
                                this.source = Some(Source { export: i, class, text: Some(r.json["source"].as_str().unwrap_or_default().to_string()), editor: None, error: None });
                            }
                            Err(e) => this.set_status(format!("Source: {e}"), cx),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.load_tab(cx);
        cx.notify();
    }

    /// An `object.*` reply that is the object as it is now: show it, mark the package.
    pub(super) fn apply_object(&mut self, path: &str, i: usize, json: &J, cx: &mut Context<Self>) {
        self.dirty.insert(path.to_string());
        if self.selected_export == Some(i) && self.open.as_ref().is_some_and(|t| t.path == path) {
            self.inspected = Some(model::inspected(json));
            self.deps = None;
        }
        cx.notify();
    }

    /// `object.list`: add / duplicate / remove an array element, or reset a property.
    fn list_op(&mut self, prop: String, op: &'static str, at: Option<usize>, cx: &mut Context<Self>) {
        let (Some(t), Some(i)) = (&self.open, self.selected_export) else { return };
        let path = t.path.clone();
        let call = self.backend.call("object.list", json!({ "path": path, "index": i, "prop": prop, "op": op, "at": at }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.apply_object(&path, i, &r.json, cx);
                    let what = match op {
                        "add" => format!("Added an element to {prop}"),
                        "duplicate" => format!("Duplicated {prop}[{}]", at.unwrap_or(0)),
                        "remove" => format!("Removed {prop}[{}]", at.unwrap_or(0)),
                        _ => format!("Reset {prop} to its default"),
                    };
                    this.set_status(what, cx);
                }
                Err(e) => this.toast(Tone::Error, format!("{prop}: {e}"), cx),
            });
        })
        .detach();
    }

    /// `object.addProperty` (from the palette's `+` list).
    pub(super) fn add_property(&mut self, name: String, cx: &mut Context<Self>) {
        let (Some(t), Some(i)) = (&self.open, self.selected_export) else { return };
        let path = t.path.clone();
        let call = self.backend.call("object.addProperty", json!({ "path": path, "index": i, "name": name }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.apply_object(&path, i, &r.json, cx);
                    this.set_status(format!("Added {name}"), cx);
                }
                Err(e) => this.toast(Tone::Error, format!("{name}: {e}"), cx),
            });
        })
        .detach();
    }

    /// Go to a referenced object: an export of this package, or an import
    /// found in its own package through the search index.
    fn follow(&mut self, r: Ref, cx: &mut Context<Self>) {
        if r.index > 0 {
            let i = (r.index - 1) as usize;
            self.reveal(i, cx);
            self.user_select_export(i, cx);
            return;
        }
        self.open_object_path(r.path, Some(r.class), cx);
    }

    /// Compile the edited class: the package is rebuilt in memory, Save writes it.
    pub(super) fn compile(&mut self, cx: &mut Context<Self>) {
        let (Some(t), Some(src)) = (&self.open, &self.source) else { return };
        let Some(editor) = &src.editor else { return };
        let path = t.path.clone();
        let class = src.class.clone();
        let text = editor.read(cx).value().to_string();
        self.set_status(format!("Compiling {class}…"), cx);
        let call = self.backend.call("script.compile", json!({ "path": path, "class": class, "source": text }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let Ok(tables) = this.update(cx, |this, cx| match &r {
                Ok(_) => {
                    this.dirty.insert(path.clone());
                    if let Some(s) = &mut this.source {
                        s.error = None;
                    }
                    this.set_status(format!("Compiled {class}: the package is rebuilt (unsaved)"), cx);
                    Some(this.backend.call("package.tables", json!({ "path": path })))
                }
                Err(e) => {
                    let msg = e.strip_prefix("compile failed: ").and_then(|j| serde_json::from_str::<J>(j).ok()).map(|v| match (v["line"].as_u64(), v["detail"].as_str()) {
                        (Some(l), Some(d)) => format!("Line {l}: {d}"),
                        _ => v["message"].as_str().unwrap_or_default().to_string(),
                    });
                    let msg = msg.unwrap_or_else(|| e.clone());
                    if let Some(s) = &mut this.source {
                        s.error = Some(msg.clone());
                    }
                    this.set_status(format!("{class} did not compile"), cx);
                    None
                }
            }) else { return };
            let Some(tables) = tables else { return };
            if let Ok(t) = tables.await {
                let _ = this.update(cx, |this, cx| {
                    this.open = Some(Tables::from_json(&path, &t.json));
                    this.refresh_rows(cx);
                });
            }
        })
        .detach();
    }

    // ----- rendering -----------------------------------------------------------------

    pub(super) fn render_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(ins) = &self.inspected else {
            let hint = if self.open.is_some() { "Select an object to see and edit it" } else { "" };
            return v_flex().size_full().items_center().justify_center().text_sm().text_color(theme.muted_foreground).child(hint).into_any_element();
        };
        let title: AnyElement = Input::new(&self.value_input).small().into_any_element();
        let script = is_script(&ins.class);
        let tab_button = |id: &'static str, label: &'static str, tab: Tab, me: &Studio, cx: &mut Context<Self>| {
            let b = Button::new(id).xsmall().label(label).on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)));
            if me.tab == tab { b.primary() } else { b.ghost() }
        };
        // Two everyday actions in view, the rest in the ⋯ menu.
        let tool = |id: &'static str, icon: IconName, label: &'static str, cmd: Cmd, cx: &mut Context<Self>| {
            let info = cmd.info();
            div().relative().child(Button::new(id).xsmall().icon(Icon::new(icon)).label(label).tooltip(if info.detail.is_empty() { info.title } else { info.detail }).on_click(cx.listener(move |this, _, window, cx| this.run(cmd, window, cx)))).child(crate::driver::probe(id))
        };
        let me = cx.entity().downgrade();
        let more = div()
            .relative()
            .child(Button::new("more-actions").xsmall().ghost().icon(Icon::new(IconName::Ellipsis)).tooltip("More: rename, duplicate, copy, delete…").dropdown_menu(move |menu, _, _| {
                let item = |label: &str, cmd: Cmd| {
                    let me = me.clone();
                    let keys = super::commands::key_hint(cmd.info().keys);
                    let label = if keys.is_empty() { label.to_string() } else { format!("{label}    {keys}") };
                    PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let _ = me.update(cx, |this, cx| this.run(cmd, window, cx));
                    })
                };
                menu.item(item("Rename", Cmd::Rename))
                    .item(item("Duplicate", Cmd::Duplicate))
                    .item(item("Copy to another package…", Cmd::CopyTo))
                    .item(item("Replace with an object of another package…", Cmd::ReplaceWith))
                    .item(item("Copy the object path", Cmd::CopyPath))
                    .separator()
                    .item(item("Delete…", Cmd::Delete))
            }))
            .child(crate::driver::probe("more-actions"));
        let tools = h_flex().gap_1().child(tool("export", IconName::Download, "Export", Cmd::ExportObject, cx)).child(tool("replace", IconName::Upload, "Replace", Cmd::ReplaceObject, cx)).child(more);
        let movie = super::swf::is_movie(&ins.class);
        let tabs = h_flex()
            .gap_1()
            .when(movie, |d| d.child(tab_button("tab-movie", "Movie", Tab::Movie, self, cx)))
            .child(tab_button("tab-props", "Properties", Tab::Properties, self, cx))
            .child(tab_button("tab-refs", "References", Tab::References, self, cx))
            .when(script, |d| d.child(tab_button("tab-src", "Source", Tab::Source, self, cx)));
        let (group, name) = match ins.path.rsplit_once('.') {
            Some((g, n)) => (g.to_string(), n.to_string()),
            None => (String::new(), ins.path.clone()),
        };
        let title: AnyElement = if self.editing.as_deref() == Some(actions::RENAME) {
            title
        } else {
            div()
                .id("ins-title")
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .cursor_pointer()
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new("Double-click to rename").build(window, cx))
                .on_click(cx.listener(|this, e: &gpui::ClickEvent, window, cx| {
                    if e.click_count() > 1 {
                        this.run(Cmd::Rename, window, cx);
                    }
                }))
                .child(name)
                .into_any_element()
        };
        let subtitle = if group.is_empty() { super::pretty_class(&ins.class) } else { format!("{} in {group}", super::pretty_class(&ins.class)) };
        let header = v_flex()
            .gap_2()
            .px_3()
            .pt_3()
            .pb_2()
            .border_b_1()
            .border_color(theme.border)
            .child(h_flex().gap_3().child(div().p_1p5().rounded_md().bg(theme.primary.opacity(0.15)).text_color(theme.primary).child(Icon::new(super::class_icon(&ins.class)))).child(v_flex().flex_1().min_w_0().child(title).child(div().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(subtitle))).child(tools))
            .child(tabs);
        let tab = if (self.tab == Tab::Source && !script) || (self.tab == Tab::Movie && !movie) { Tab::Properties } else { self.tab };
        let body: AnyElement = match tab {
            Tab::Properties => self.render_properties(window, cx),
            Tab::References => self.render_references(cx),
            Tab::Source => self.render_source(window, cx),
            Tab::Movie => match self.movie_editor(window, cx) {
                Some(m) => m.into_any_element(),
                None => div().into_any_element(),
            },
        };
        v_flex().size_full().child(header).child(div().flex_1().min_h_0().child(body)).into_any_element()
    }

    fn render_properties(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let previews: Vec<AnyElement> = [self.render_mesh(cx), self.render_slots(cx), self.render_material(cx), self.render_sound(cx), self.render_wwise(cx), self.render_anim(cx), self.render_particles(cx)].into_iter().flatten().collect();
        let texture = self.render_texture(cx);
        let Some(ins) = &self.inspected else { return div().into_any_element() };
        let mut body = v_flex().id("props").size_full().overflow_y_scroll().px_2().pb_2().gap_2();
        body = body.children(previews);
        if let Some(t) = texture {
            body = body.child(t);
        }
        if !ins.rows.is_empty() {
            body = body.child(div().pt_1().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("PROPERTIES"));
        }
        let rows = ins.rows.iter().enumerate().map(|(k, r)| {
            let value: AnyElement = if self.editing.as_deref() == Some(r.path.as_str()) {
                div().flex_1().child(Input::new(&self.value_input).xsmall()).into_any_element()
            } else {
                let cell = div().id(("val", k)).flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().px_1().rounded_sm().child(r.value.clone());
                match r.edit.clone() {
                    Edit::None => cell.into_any_element(),
                    Edit::Bool(b) => {
                        let path = r.path.clone();
                        cell.cursor_pointer().hover(|s| s.bg(theme.list_hover)).on_click(cx.listener(move |this, _, _, cx| this.set_property(path.clone(), if b { "False" } else { "True" }.into(), cx))).into_any_element()
                    }
                    Edit::Text(draft) => {
                        let path = r.path.clone();
                        cell.cursor_text().hover(|s| s.bg(theme.list_hover)).on_click(cx.listener(move |this, _, window, cx| this.start_edit(path.clone(), draft.clone(), window, cx))).into_any_element()
                    }
                }
            };
            // Row operations, shown when the row is hovered.
            let group = format!("row{k}");
            let mut ops = h_flex().gap_0p5().invisible().group_hover(group.clone(), |s| s.visible());
            match &r.list {
                ListRole::Array => {
                    let p = r.path.clone();
                    ops = ops.child(Button::new(("add", k)).xsmall().ghost().label("+").tooltip("Add an element").on_click(cx.listener(move |this, _, _, cx| this.list_op(p.clone(), "add", None, cx))));
                }
                ListRole::Element { array, at } => {
                    let (a1, a2, at) = (array.clone(), array.clone(), *at);
                    ops = ops
                        .child(Button::new(("dup", k)).xsmall().ghost().label("⧉").tooltip("Duplicate the element").on_click(cx.listener(move |this, _, _, cx| this.list_op(a1.clone(), "duplicate", Some(at), cx))))
                        .child(Button::new(("rm", k)).xsmall().ghost().label("−").tooltip("Remove the element").on_click(cx.listener(move |this, _, _, cx| this.list_op(a2.clone(), "remove", Some(at), cx))));
                }
                ListRole::None => {}
            }
            if r.resettable {
                let p = r.path.clone();
                ops = ops.child(Button::new(("reset", k)).xsmall().ghost().label("↺").tooltip("Back to the default (stop storing it)").on_click(cx.listener(move |this, _, _, cx| this.list_op(p.clone(), "removeProperty", None, cx))));
            }
            h_flex()
                .group(group)
                .pl(px(r.depth as f32 * 14.0))
                .gap_2()
                .text_sm()
                .child(div().w(px(200.0)).flex_none().overflow_hidden().text_ellipsis().whitespace_nowrap().child(r.name.clone()))
                .child(value)
                .child(ops)
                .child(div().flex_none().pr_2().text_xs().text_color(theme.muted_foreground).child(r.kind.clone()))
        });
        body = body.child(v_flex().children(rows));
        if ins.kind == "object" {
            body = body.child(h_flex().child(Button::new("add-prop").xsmall().ghost().label("Add a property…").on_click(cx.listener(|this, _, window, cx| this.open_palette("+", window, cx)))));
        }
        // What the engine stores outside the properties: for the curious, folded away.
        if !ins.native.is_empty() {
            let open = self.show_native;
            body = body.child(h_flex().child(Button::new("native-toggle").xsmall().ghost().label(if open { "Hide the technical details" } else { "Technical details" }).on_click(cx.listener(|this, _, _, cx| {
                this.show_native = !this.show_native;
                cx.notify();
            }))));
            if open {
                body = body.child(div().text_xs().font_family("Menlo").text_color(theme.muted_foreground).whitespace_normal().child(ins.native.clone()));
            }
        }
        body.into_any_element()
    }

    fn render_references(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some((_, deps)) = &self.deps else {
            return div().p_2().text_sm().text_color(theme.muted_foreground).child("Loading…").into_any_element();
        };
        let Deps { links, references, referrers } = deps;
        let mut k = 0usize;
        let mut row = |label: String, r: &Ref, cx: &mut Context<Self>| {
            k += 1;
            let target = r.clone();
            let place = if r.index > 0 { "here" } else { "import" };
            h_flex()
                .id(("ref", k))
                .gap_2()
                .px_1()
                .text_sm()
                .rounded_sm()
                .cursor_pointer()
                .hover(|s| s.bg(theme.list_hover))
                .on_click(cx.listener(move |this, _, _, cx| this.follow(target.clone(), cx)))
                .child(div().w(px(90.0)).flex_none().text_xs().text_color(theme.muted_foreground).child(label))
                .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(r.path.clone()))
                .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(format!("{} · {place}", r.class)))
        };
        let mut body = v_flex().id("refs").size_full().overflow_y_scroll().px_2().pb_2().gap_1();
        for (name, r) in links.clone() {
            body = body.child(row(name.to_string(), &r, cx));
        }
        body = body.child(div().pt_2().text_xs().text_color(theme.muted_foreground).child(format!("References ({})", references.len())));
        for r in references.clone() {
            body = body.child(row(String::new(), &r, cx));
        }
        body = body.child(div().pt_2().text_xs().text_color(theme.muted_foreground).child(format!("Used by ({}, in this package)", referrers.len())));
        for r in referrers.clone() {
            body = body.child(row(String::new(), &r, cx));
        }
        body.into_any_element()
    }

    fn render_source(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(src) = &mut self.source else {
            return div().p_2().text_sm().text_color(theme.muted_foreground).child("Decompiling…").into_any_element();
        };
        if let Some(text) = src.text.take() {
            // UnrealScript reads like Java to a highlighter: comments, strings, numbers, most keywords.
            let editor = cx.new(|cx| EditorState::new(window, cx).language("java").line_number(true));
            editor.update(cx, |e, cx| e.set_value(text, window, cx));
            src.editor = Some(editor);
        }
        let Some(editor) = src.editor.clone() else { return div().into_any_element() };
        let error = src.error.clone();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_2()
                    .pb_1()
                    .gap_2()
                    .child(div().text_sm().child(format!("class {}", src.class)))
                    .child(Button::new("compile").xsmall().primary().label("Compile").on_click(cx.listener(|this, _, _, cx| this.compile(cx))))
                    .when_some(error, |d, e| d.child(div().text_xs().text_color(theme.danger).child(e))),
            )
            .child(div().flex_1().min_h_0().font_family("Menlo").text_sm().child(Editor::new(&editor).size_full()))
            .into_any_element()
    }
}
