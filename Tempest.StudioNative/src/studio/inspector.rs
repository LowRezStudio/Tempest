//! The inspector: the selected export's properties (edited in place, array
//! elements added, duplicated, removed, properties reset or added), its
//! references and what refers to it, and for script objects the owning
//! class's decompiled source with Compile.

use gpui::{div, prelude::*, px, AnyElement, Context, Entity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Editor, EditorState, Input};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable, Selectable};
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
    /// The object as text (`object.text`).
    Dump,
    /// A movie's editor.
    Movie,
}

/// The Dump tab: the object written out as text.
pub struct Dump {
    pub export: usize,
    /// Text not yet in an editor (the editor needs a window to be made).
    pub text: Option<String>,
    pub editor: Option<Entity<EditorState>>,
}

/// The Source tab: the class the selected script object belongs to.
pub struct Source {
    pub export: usize,
    pub class: String,
    /// The selected export when it is a member of the class (a function, a state…): its kind and name.
    pub member: Option<(String, String)>,
    /// The decompiled text (Revert goes back to it).
    pub original: String,
    /// Text not yet in an editor (the editor needs a window to be made).
    pub text: Option<String>,
    pub editor: Option<Entity<EditorState>>,
    pub error: Option<String>,
    /// The line of the error, to go to once the editor is drawn.
    pub go_to: Option<u32>,
    pub warnings: Vec<String>,
    /// The text differs from the decompiled one (it is kept as a draft while the window lives).
    pub modified: bool,
    pub _sub: Option<gpui::Subscription>,
}

/// The line (0-based) where `member` is declared in a class's source.
fn member_line(text: &str, member: &str) -> Option<u32> {
    const KINDS: &[&str] = &["function", "event", "state", "operator", "preoperator", "postoperator", "delegate", "struct", "enum", "const"];
    let word = |line: &str, w: &str| {
        let l = line.to_ascii_lowercase();
        let w = w.to_ascii_lowercase();
        l.match_indices(&w).any(|(i, _)| {
            let before = l[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric() && c != '_');
            let after = l[i + w.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '_');
            before && after
        })
    };
    text.lines().position(|line| {
        let code = line.split("//").next().unwrap_or("");
        KINDS.iter().any(|k| word(code, k)) && word(code, member)
    }).map(|l| l as u32)
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
            Tab::Dump => {
                if self.dump.as_ref().is_some_and(|d| d.export == i) {
                    return;
                }
                let call = self.backend.call("object.text", json!({ "path": path, "index": i }));
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| {
                        if this.selected_export != Some(i) {
                            return;
                        }
                        let text = match r {
                            Ok(r) => r.json.as_str().map(str::to_string).unwrap_or_else(|| r.json.to_string()),
                            Err(e) => format!("Could not write the object out: {}", super::ui::explain_error(&e)),
                        };
                        this.dump = Some(Dump { export: i, text: Some(text), editor: None });
                        cx.notify();
                    });
                })
                .detach();
            }
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
                            Err(e) => this.toast(Tone::Error, e, cx),
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
                                let original = r.json["source"].as_str().unwrap_or_default().to_string();
                                // An edit left when another object was selected comes back.
                                let draft = this.drafts.get(&(path.clone(), class.clone())).cloned();
                                let member = r.json["member"].as_str().map(|m| (r.json["kind"].as_str().unwrap_or_default().to_string(), m.to_string()));
                                let text = draft.clone().unwrap_or_else(|| original.clone());
                                let go_to = member.as_ref().and_then(|(_, m)| member_line(&text, m));
                                this.source = Some(Source { export: i, class, member, original, text: Some(text), editor: None, error: None, go_to, warnings: Vec::new(), modified: draft.is_some(), _sub: None });
                            }
                            Err(e) => this.toast(Tone::Error, format!("Source: {e}"), cx),
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
    /// Pick a field to add to the tagged struct at `at` (the palette's `+` list, for that struct).
    fn add_field_palette(&mut self, at: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prop_open.insert(at.clone(), true);
        self.open_palette_at("+", at, window, cx);
    }

    /// Add a property the class declares and the object does not store; `at`: a field of that tagged struct.
    pub(super) fn add_property(&mut self, name: String, at: Option<String>, cx: &mut Context<Self>) {
        let (Some(t), Some(i)) = (&self.open, self.selected_export) else { return };
        let path = t.path.clone();
        let mut params = json!({ "path": path, "index": i, "name": name });
        if let Some(at) = at {
            params["at"] = json!(at);
        }
        let call = self.backend.call("object.addProperty", params);
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
                Ok(r) => {
                    this.dirty.insert(path.clone());
                    let warnings: Vec<String> = r.json["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_string)).collect();
                    let note = if warnings.is_empty() {
                        String::new()
                    } else {
                        let w = warnings.len();
                        format!(", {w} warning{}", if w == 1 { "" } else { "s" })
                    };
                    if let Some(s) = &mut this.source {
                        s.error = None;
                        s.warnings = warnings;
                        // What was compiled is the class's text now.
                        s.original = text.clone();
                        s.modified = false;
                    }
                    this.drafts.remove(&(path.clone(), class.clone()));
                    let updated = r.json["exports"]["names"].as_array().map(|n| n.len()).unwrap_or(0);
                    this.toast(Tone::Success, format!("Compiled {class} ({updated} object{} updated in your mod{note}).", if updated == 1 { "" } else { "s" }), cx);
                    Some(this.backend.call("package.tables", json!({ "path": path })))
                }
                Err(e) => {
                    let msg = e.strip_prefix("compile failed: ").and_then(|j| serde_json::from_str::<J>(j).ok()).map(|v| match (v["line"].as_u64(), v["detail"].as_str()) {
                        (Some(l), Some(d)) => format!("Line {l}: {d}"),
                        _ => v["message"].as_str().unwrap_or_default().to_string(),
                    });
                    let msg = msg.unwrap_or_else(|| super::ui::explain_error(e));
                    // The error's line, when it is in this class.
                    let at = e.strip_prefix("compile failed: ").and_then(|j| serde_json::from_str::<J>(j).ok()).filter(|v| v["class"].as_str().is_none_or(|c| c.eq_ignore_ascii_case(&class))).and_then(|v| v["line"].as_u64());
                    if let Some(s) = &mut this.source {
                        s.error = Some(msg);
                        s.warnings.clear();
                        s.go_to = at.map(|l| l.saturating_sub(1) as u32);
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
            if self.open.is_none() {
                return div().size_full().into_any_element();
            }
            return super::ui::empty(IconName::MousePointer2, "Nothing selected", "Select an object in the list to preview it and change it: textures, meshes, materials, sounds, animations, UI movies…", &theme).into_any_element();
        };
        let title: AnyElement = Input::new(&self.value_input).small().into_any_element();
        let script = is_script(&ins.class);
        let tab_button = |id: &'static str, label: &'static str, tab: Tab, me: &Studio, cx: &mut Context<Self>| {
            let b = Button::new(id).xsmall().label(label).on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)));
            b.ghost().selected(me.tab == tab)
        };
        // Two everyday actions in view, the rest in the ⋯ menu.
        let tool = |id: &'static str, icon: IconName, label: &'static str, cmd: Cmd, cx: &mut Context<Self>| {
            let info = cmd.info();
            div().relative().child(Button::new(id).xsmall().icon(Icon::new(icon)).label(label).tooltip(if info.detail.is_empty() { info.title } else { info.detail }).on_click(cx.listener(move |this, _, window, cx| this.run(cmd, window, cx)))).child(crate::driver::probe(id))
        };
        let me = cx.entity().downgrade();
        let is_dump = self.tab == Tab::Dump;
        let more = div()
            .relative()
            .child(Button::new("more-actions").xsmall().ghost().icon(Icon::new(IconName::Ellipsis)).tooltip("More: rename, duplicate, copy, raw bytes, delete…").dropdown_menu(move |menu, _, _| {
                let item = |label: &str, cmd: Cmd| {
                    let me = me.clone();
                    let keys = super::commands::key_hint(cmd.info().keys);
                    let label = if keys.is_empty() { label.to_string() } else { format!("{label}    {keys}") };
                    PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let _ = me.update(cx, |this, cx| this.run(cmd, window, cx));
                    })
                };
                let me_dump = me.clone();
                menu.item(item("Rename", Cmd::Rename))
                    .item(item("Duplicate", Cmd::Duplicate))
                    .item(item("Copy to another package…", Cmd::CopyTo))
                    .item(item("Replace with an object of another package…", Cmd::ReplaceWith))
                    .item(item("Copy the object path", Cmd::CopyPath))
                    .item(item("Extract the raw data…", Cmd::ExtractRaw))
                    .separator()
                    .item(PopupMenuItem::new("Inspect raw bytes").checked(is_dump).on_click(move |_, _, cx| {
                        let _ = me_dump.update(cx, |this, cx| this.set_tab(Tab::Dump, cx));
                    }))
                    .separator()
                    .item(item("Delete…", Cmd::Delete))
            }))
            .child(crate::driver::probe("more-actions"));
        let tools = h_flex().gap_1().child(tool("export", IconName::Download, "Export", Cmd::ExportObject, cx)).child(tool("replace", IconName::Upload, "Replace", Cmd::ReplaceObject, cx)).child(more);
        let movie = super::swf::is_movie(&ins.class);
        let can_back = self.history_at > 0;
        let tabs = h_flex()
            .items_center()
            .gap_1()
            .when(can_back, |d| {
                d.child(
                    Button::new("ins-back")
                        .xsmall()
                        .ghost()
                        .icon(Icon::new(IconName::ArrowLeft))
                        .label("Back")
                        .tooltip(format!("Back ({})", super::commands::key_hint("⌥←")))
                        .on_click(cx.listener(|this, _, window, cx| this.back(&super::Back, window, cx))),
                )
                .child(div().w(px(1.0)).h(px(14.0)).mx_0p5().bg(theme.border))
            })
            .when(movie, |d| d.child(tab_button("tab-movie", "Movie", Tab::Movie, self, cx)))
            .child(tab_button("tab-props", "Properties", Tab::Properties, self, cx))
            .child(tab_button("tab-refs", "References", Tab::References, self, cx))
            .when(script, |d| d.child(tab_button("tab-src", "Source", Tab::Source, self, cx)))
            .when(self.tab == Tab::Dump, |d| d.child(tab_button("tab-dump", "Raw data", Tab::Dump, self, cx)));
        let (group, name) = match ins.path.rsplit_once('.') {
            Some((g, n)) => (g.to_string(), n.to_string()),
            None => (String::new(), ins.path.clone()),
        };
        let ring = theme.ring;
        let title: AnyElement = if self.editing.as_deref() == Some(actions::RENAME) {
            title
        } else {
            div()
                .id("ins-title")
                .focusable()
                .tab_index(0)
                .px_0p5()
                .rounded_sm()
                .border_1()
                .border_color(gpui::transparent_black())
                .focus_visible(move |s| s.border_color(ring))
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .cursor_pointer()
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new("Double-click or press Enter to rename").build(window, cx))
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                    if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.run(Cmd::Rename, window, cx);
                    }
                }))
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
            Tab::Dump => self.render_dump(window, cx),
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
        if let Some((error, text)) = &ins.note {
            let shown = if *error { super::ui::explain_error(text) } else { text.clone() };
            body = body.child(div().pt_2().text_sm().font_family(theme.mono_font_family.clone()).whitespace_normal().text_color(if *error { theme.danger } else { theme.muted_foreground }).child(shown));
        }
        if let Some(t) = texture {
            body = body.child(t);
        }
        if !ins.rows.is_empty() {
            body = body.child(super::ui::section("Properties", &theme));
        }
        // Structs and arrays fold: nested ones and long arrays start folded.
        let ring = theme.ring;
        let hover_bg = theme.list_hover;
        let shown = model::unfolded(&ins.rows, |r| self.prop_open.get(&r.path).copied().unwrap_or_else(|| r.starts_open()));
        let rows = shown.into_iter().map(|k| (k, &ins.rows[k])).map(|(k, r)| {
            let value: AnyElement = if self.editing.as_deref() == Some(r.path.as_str()) {
                div().flex_1().min_w_0().child(Input::new(&self.value_input).xsmall()).into_any_element()
            } else {
                let cell = div().id(("val", k)).flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().px_1().rounded_sm().child(r.value.clone());
                match r.edit.clone() {
                    Edit::None => cell.into_any_element(),
                    Edit::Bool(b) => {
                        let (path, path_k) = (r.path.clone(), r.path.clone());
                        cell.focusable()
                            .tab_index(0)
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .focus_visible(move |s| s.border_color(ring))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                                if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.set_property(path_k.clone(), if b { "False" } else { "True" }.into(), cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| this.set_property(path.clone(), if b { "False" } else { "True" }.into(), cx)))
                            .into_any_element()
                    }
                    Edit::Text(draft) => {
                        let (path, path_k) = (r.path.clone(), r.path.clone());
                        let draft_k = draft.clone();
                        cell.focusable()
                            .tab_index(0)
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .focus_visible(move |s| s.border_color(ring))
                            .cursor_text()
                            .hover(move |s| s.bg(hover_bg))
                            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                                if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.start_edit(path_k.clone(), draft_k.clone(), window, cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| this.start_edit(path.clone(), draft.clone(), window, cx)))
                            .into_any_element()
                    }
                }
            };
            // Row operations, kept subtly visible so keyboard and mouse users alike can reach them.
            let mut ops = h_flex().gap_0p5();
            match &r.list {
                ListRole::Array => {
                    let p = r.path.clone();
                    ops = ops.child(Button::new(("add", k)).xsmall().ghost().text_color(theme.muted_foreground).icon(Icon::new(IconName::Plus)).tooltip("Add an element").on_click(cx.listener(move |this, _, _, cx| this.list_op(p.clone(), "add", None, cx))));
                }
                ListRole::Element { array, at } => {
                    let (a1, a2, at) = (array.clone(), array.clone(), *at);
                    ops = ops
                        .child(Button::new(("dup", k)).xsmall().ghost().text_color(theme.muted_foreground).icon(Icon::new(IconName::Copy)).tooltip("Duplicate the element").on_click(cx.listener(move |this, _, _, cx| this.list_op(a1.clone(), "duplicate", Some(at), cx))))
                        .child(Button::new(("rm", k)).xsmall().ghost().text_color(theme.muted_foreground).icon(Icon::new(IconName::Trash)).tooltip("Remove the element").on_click(cx.listener(move |this, _, _, cx| this.list_op(a2.clone(), "remove", Some(at), cx))));
                }
                ListRole::None => {}
            }
            if r.is_struct {
                let p = r.path.clone();
                ops = ops.child(Button::new(("field", k)).xsmall().ghost().text_color(theme.muted_foreground).icon(Icon::new(IconName::Plus)).tooltip("Add a field (it starts at its default)").on_click(cx.listener(move |this, _, window, cx| this.add_field_palette(p.clone(), window, cx))));
            }
            if r.resettable {
                let p = r.path.clone();
                ops = ops.child(Button::new(("reset", k)).xsmall().ghost().text_color(theme.muted_foreground).icon(Icon::new(IconName::RotateCcw)).tooltip("Back to the default (stop storing it)").on_click(cx.listener(move |this, _, _, cx| this.list_op(p.clone(), "removeProperty", None, cx))));
            }
            let foldable = r.children > 0;
            let open = self.prop_open.get(&r.path).copied().unwrap_or_else(|| r.starts_open());
            let (fold_path, fold_path_k) = (r.path.clone(), r.path.clone());
            let tip = if r.kind.is_empty() { r.path.clone() } else { format!("{} ({})", r.path, r.kind) };
            let name = h_flex()
                .id(("prop-name", k))
                .w(gpui::rems(12.5))
                .min_w(px(160.0))
                .max_w(px(260.0))
                .flex_none()
                .gap_1()
                .rounded_sm()
                .border_1()
                .border_color(gpui::transparent_black())
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                .when(foldable, |d| {
                    d.focusable()
                        .tab_index(0)
                        .focus_visible(move |s| s.border_color(ring))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                            if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.prop_open.insert(fold_path_k.clone(), !open);
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.prop_open.insert(fold_path.clone(), !open);
                            cx.notify();
                        }))
                })
                .child(div().w(px(12.0)).flex_none().when(foldable, |d| d.child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall().text_color(theme.muted_foreground))))
                .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(r.name.clone()))
                .when(foldable && !open && r.value.is_empty(), |d| {
                    let noun = if r.children == 1 { "field" } else { "fields" };
                    d.child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(format!("{} {noun}", r.children)))
                });
            h_flex()
                .min_h(px(22.0))
                .py_0p5()
                .pl(px(r.depth as f32 * 14.0))
                .gap_2()
                .text_sm()
                .child(name)
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
                body = body.child(div().text_xs().font_family(crate::studio::ui::mono()).text_color(theme.muted_foreground).whitespace_normal().child(ins.native.clone()));
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
        let ring = theme.ring;
        let hover_bg = theme.list_hover;
        let mut k = 0usize;
        let mut row = |label: String, r: &Ref, cx: &mut Context<Self>| {
            k += 1;
            let (target, target_k) = (r.clone(), r.clone());
            let place = if r.index > 0 { "here" } else { "import" };
            h_flex()
                .id(("ref", k))
                .focusable()
                .tab_index(0)
                .min_h(px(22.0))
                .py_0p5()
                .gap_2()
                .px_1()
                .text_sm()
                .rounded_sm()
                .border_1()
                .border_color(gpui::transparent_black())
                .focus_visible(move |s| s.border_color(ring))
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                    if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.follow(target_k.clone(), cx);
                    }
                }))
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

    /// The object as text, in a read-only editor (selectable, searchable, long dumps scroll).
    fn render_dump(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(dump) = &mut self.dump else {
            return div().p_2().text_sm().text_color(theme.muted_foreground).child("Loading…").into_any_element();
        };
        if let Some(text) = dump.text.take() {
            let editor = cx.new(|cx| EditorState::new(window, cx).line_number(false));
            editor.update(cx, |e, cx| e.set_value(text, window, cx));
            dump.editor = Some(editor);
        }
        let Some(editor) = dump.editor.clone() else { return div().into_any_element() };
        div().size_full().font_family(theme.mono_font_family.clone()).text_sm().child(Editor::new(&editor).readonly(true).size_full()).into_any_element()
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
            // An edit is a draft of the class: it survives selecting another object.
            src._sub = Some(cx.subscribe(&editor, |this, editor, e: &gpui_component::input::InputEvent, cx| {
                if !matches!(e, gpui_component::input::InputEvent::Change) {
                    return;
                }
                let text = editor.read(cx).value().to_string();
                let Some(path) = this.open.as_ref().map(|t| t.path.clone()) else { return };
                let Some(src) = &mut this.source else { return };
                let modified = text != src.original;
                let key = (path, src.class.clone());
                if modified {
                    this.drafts.insert(key, text);
                } else {
                    this.drafts.remove(&key);
                }
                if src.modified != modified {
                    src.modified = modified;
                    cx.notify();
                }
            }));
            src.editor = Some(editor);
        }
        let Some(editor) = src.editor.clone() else { return div().into_any_element() };
        if let Some(line) = src.go_to.take() {
            editor.update(cx, |e, cx| e.set_cursor_position(gpui_component::input::Position::new(line, 0), window, cx));
        }
        let error = src.error.clone();
        let (modified, warnings) = (src.modified, src.warnings.clone());
        let member = src.member.as_ref().map(|(kind, name)| format!("· {kind} {name}"));
        let revert = editor.clone();
        let original = src.original.clone();
        v_flex()
            .size_full()
            .key_context("SourceEdit")
            .on_action(cx.listener(|this, _: &super::CompileSource, _, cx| this.compile(cx)))
            .child(
                h_flex()
                    .px_2()
                    .pb_1()
                    .gap_2()
                    .child(div().text_sm().font_family(theme.mono_font_family.clone()).font_weight(gpui::FontWeight::SEMIBOLD).child(format!("{}.uc", src.class)))
                    .when_some(member, |d, m| d.child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(m)))
                    .when(modified, |d| d.child(super::ui::badge(Some(IconName::Pencil), "modified", theme.warning, &theme)))
                    .child(div().flex_1())
                    .child(Button::new("revert-src").xsmall().ghost().label("Revert").disabled(!modified).tooltip(if modified { "Back to the decompiled text" } else { "Make an edit first to revert" }).on_click(move |_, window, cx| {
                        let text = original.clone();
                        revert.update(cx, |e, cx| e.replace_all(text, window, cx));
                    }))
                    .child(Button::new("compile").xsmall().label("Compile").tooltip(format!("Recompile this class in your mod ({})", super::commands::key_hint("⌘↩"))).on_click(cx.listener(|this, _, _, cx| this.compile(cx)))),
            )
            .when_some(error, |d, e| d.child(div().px_2().pb_1().text_xs().whitespace_normal().text_color(theme.danger).child(e)))
            .when(!warnings.is_empty(), |d| {
                let mut list = v_flex().id("src-warnings").px_2().pb_1().max_h(px(72.0)).overflow_y_scroll().text_xs().text_color(theme.warning);
                for w in &warnings {
                    list = list.child(div().whitespace_normal().child(w.clone()));
                }
                d.child(list)
            })
            .child(div().flex_1().min_h_0().font_family(theme.mono_font_family.clone()).text_sm().child(Editor::new(&editor).size_full()))
            .into_any_element()
    }
}
