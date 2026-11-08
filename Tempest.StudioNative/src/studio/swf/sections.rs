//! The tag views: edit-text fields (initial text), scripts (every AS2
//! script: class definitions, frame scripts, clip events, button actions,
//! with the selected one decompiled to ActionScript or shown as P-code),
//! external images, linkage (exports and imports) and every tag. Edits go to
//! the pending list like the stage's.

use gpui::{div, prelude::*, px, AnyElement, Context, Entity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Editor, EditorState, Input, Position};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{MovieEditor, Node};

/// The script open in the Scripts view.
pub struct ScriptView {
    key: String,
    loc: J,
    pcode: bool,
    editor: Option<Entity<EditorState>>,
    /// Text to put in the editor once there is a window.
    pending: Option<String>,
    /// The text the editor started from: a modification is a difference from it.
    base: String,
    problems: Vec<String>,
    load_error: Option<String>,
    failure: Option<(Option<u32>, String)>,
    checked: Option<String>,
    busy: bool,
    /// Read the script again (after Apply, a discard or an undo).
    pub reload: bool,
}

const GROUPS: &[(&str, &str)] = &[("DoInitAction", "Classes"), ("DoAction", "Frame scripts"), ("ClipAction", "Clip events"), ("ButtonAction", "Buttons")];

/// The line number in a compiler message (`line 12`, `:12:`).
fn error_line(msg: &str) -> Option<u32> {
    let lower = msg.to_ascii_lowercase();
    if let Some(i) = lower.find("line ") {
        let n: String = lower[i + 5..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(v) = n.parse() {
            return Some(v);
        }
    }
    None
}

impl MovieEditor {
    /// A script's title in the list.
    fn script_title(&self, sc: &J) -> String {
        let owner = sc["owner"].as_u64().unwrap_or(0);
        let where_ = if owner == 0 { "root".to_string() } else { self.label(owner) };
        match sc["kind"].as_str().unwrap_or("") {
            "DoInitAction" => sc["sprite"].as_u64().and_then(|s| self.char(s)).and_then(|c| c["names"][0].as_str()).map(|n| n.trim_start_matches("__Packages.").to_string()).unwrap_or_else(|| format!("#initclip {}", sc["label"].as_str().unwrap_or(""))),
            "DoAction" => format!("{where_} · frame {}", sc["frame"].as_u64().unwrap_or(0) + 1),
            "ClipAction" => {
                let p = &sc["placed"];
                let target = p["name"].as_str().map(str::to_string).or_else(|| p["character"].as_u64().map(|c| self.label(c))).unwrap_or_else(|| format!("depth {}", p["depth"]));
                format!("onClipEvent({}) · {target} in {where_}", sc["label"].as_str().unwrap_or(""))
            }
            _ => format!("on({}) · button {}", sc["label"].as_str().unwrap_or(""), sc["tag"]),
        }
    }

    fn scripts(&self) -> Vec<J> {
        self.info["scripts"].as_array().cloned().unwrap_or_default()
    }

    /// Read the open script (pending edits included) in the current mode.
    fn load_script(&mut self, cx: &mut Context<Self>) {
        let Some(sv) = &mut self.script_view else { return };
        sv.load_error = None;
        sv.failure = None;
        sv.checked = None;
        sv.reload = false;
        let (key, loc, pcode) = (sv.key.clone(), sv.loc.clone(), sv.pcode);
        let call = self.backend.call("swf.script", json!({ "path": self.path, "index": self.index, "loc": loc, "edits": self.pending() }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                let own = this.script_edit(&loc).cloned();
                let Some(sv) = this.script_view.as_mut().filter(|s| s.key == key) else { return };
                match r {
                    Ok(r) => {
                        // The text staged in this view stays as written (comments, layout).
                        let own = own.and_then(|p| match (p["kind"].as_str(), pcode) {
                            (Some("script"), false) => p["source"].as_str().map(str::to_string),
                            (Some("pcode"), true) => p["pcode"].as_str().map(str::to_string),
                            _ => None,
                        });
                        let text = own.unwrap_or_else(|| r.json[if pcode { "pcode" } else { "source" }].as_str().unwrap_or("").to_string());
                        sv.problems = r.json["problems"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_string)).collect();
                        sv.base = text.clone();
                        sv.pending = Some(text);
                    }
                    Err(e) => sv.load_error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn select_script(&mut self, key: String, cx: &mut Context<Self>) {
        self.script = Some(key);
        cx.notify();
    }

    fn script_text(&self, cx: &gpui::App) -> Option<String> {
        Some(self.script_view.as_ref()?.editor.as_ref()?.read(cx).value().to_string())
    }

    /// Compile the editor's text (`then_stage`: keep it as the script's pending replacement).
    fn check_script(&mut self, then_stage: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.script_text(cx) else { return };
        let Some(sv) = &mut self.script_view else { return };
        if sv.busy {
            return;
        }
        sv.busy = true;
        sv.failure = None;
        let (pcode, loc) = (sv.pcode, sv.loc.clone());
        let call = self.backend.call("swf.compile", if pcode { json!({ "pcode": text }) } else { json!({ "source": text }) });
        let editor = sv.editor.clone();
        cx.spawn_in(window, async move |this, cx| {
            let r = call.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let Some(sv) = &mut this.script_view else { return };
                sv.busy = false;
                match r {
                    Ok(r) => {
                        sv.checked = Some(format!("{} bytes", r.json["bytes"]));
                        if then_stage {
                            sv.base = text.clone();
                            this.set_script(&loc, text, pcode, cx);
                        }
                    }
                    Err(e) => {
                        let line = error_line(&e);
                        sv.failure = Some((line, e));
                        if let (Some(l), Some(ed)) = (line, editor) {
                            ed.update(cx, |s, cx| s.set_cursor_position(Position { line: l.saturating_sub(1), character: 0 }, window, cx));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn switch_mode(&mut self, pcode: bool, window: &mut Window, cx: &mut Context<Self>) {
        let modified = self.script_view.as_ref().is_some_and(|s| s.pcode != pcode) && self.script_text(cx).is_some_and(|t| self.script_view.as_ref().is_some_and(|s| s.base != t));
        if modified {
            self.toast(super::Tone::Warning, "Stage the script first (or revert it): switching shows what the staged text compiles to", cx);
            return;
        }
        if let Some(sv) = &mut self.script_view {
            if sv.pcode != pcode {
                sv.pcode = pcode;
                sv.editor = None;
                self.load_script(cx);
            }
        }
        let _ = window;
    }

    fn revert_script(&mut self, cx: &mut Context<Self>) {
        let Some(loc) = self.script_view.as_ref().map(|s| s.loc.clone()) else { return };
        let key = format!("script|{}", super::script_key(&loc));
        if self.is_edited(&key) {
            self.drop_edit(&key, cx);
        }
        if let Some(sv) = &mut self.script_view {
            sv.editor = None;
            sv.reload = true;
        }
        cx.notify();
    }

    pub(super) fn render_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if self.info.is_null() {
            return div().p_3().text_xs().opacity(0.6).child("Reading the movie…").into_any_element();
        }
        if self.node == Node::Scripts {
            return self.render_scripts(window, cx);
        }
        let mut out = v_flex().id("mv-section").size_full().overflow_y_scroll().p_1().text_xs();
        match self.node {
            Node::Texts => {
                let texts = self.info["texts"].as_array().cloned().unwrap_or_default();
                if texts.is_empty() {
                    out = out.child(div().p_2().opacity(0.6).child("No edit-text fields; the UI's texts come from the language files (Assembly view) or from scripts."));
                }
                out = out.child(h_flex().gap_2().px_1().py_0p5().font_family("Menlo").text_color(theme.muted_foreground).child(div().w(px(50.0)).child("id")).child(div().w(px(160.0)).child("variable")).child(div().w(px(140.0)).child("font")).child("initial text"));
                for t in &texts {
                    let id = t["id"].as_u64().unwrap_or(0);
                    let tag = t["tag"].as_u64().unwrap_or(0);
                    let font = format!("{}{}{}", t["fontClass"].as_str().map(str::to_string).or_else(|| t["fontId"].as_u64().map(|f| f.to_string())).unwrap_or_default(), t["fontHeight"].as_f64().map(|h| format!(" {h}")).unwrap_or_default(), if t["html"].as_bool() == Some(true) { " html" } else { "" });
                    let value = self.value("text", tag, t["text"].as_str().unwrap_or(""));
                    let edited = self.is_edited(&format!("text|{tag}|0"));
                    out = out.child(
                        h_flex()
                            .gap_2()
                            .px_1()
                            .py_0p5()
                            .font_family("Menlo")
                            .child(div().id(("tx-id", tag as usize)).w(px(50.0)).text_color(theme.link).cursor_pointer().child(id.to_string()).on_click(cx.listener(move |this, _, _, cx| this.open(Node::Char(id), cx))))
                            .child(div().w(px(160.0)).overflow_hidden().text_ellipsis().child(t["variable"].as_str().unwrap_or("").to_string()))
                            .child(div().w(px(140.0)).opacity(0.6).overflow_hidden().text_ellipsis().child(font))
                            .child(div().flex_1().min_w_0().when(edited, |d| d.border_l_2().border_color(theme.warning).pl_1()).child(self.field(format!("text:{tag}"), value, 520.0, cx))),
                    );
                }
            }
            Node::Images => {
                out = out.child(h_flex().gap_2().items_start().px_1().pb_1().child(div().flex_1().opacity(0.6).child("External images: the engine loads each file name as a Texture2D of this package. Replace the picture (PNG or DDS) to change it; the movie keeps drawing it at the size listed here. Rename the reference to point at another texture.")).child(Button::new("im-add").xsmall().icon(Icon::new(IconName::Image)).label("Add image…").on_click(cx.listener(|this, _, window, cx| this.open_add_image(window, cx)))));
                out = out.child(h_flex().gap_2().px_1().font_family("Menlo").text_color(theme.muted_foreground).child(div().w(px(50.0)).child("id")).child(div().w(px(90.0)).child("size")).child(div().w(px(60.0)).child("format")).child(div().w(px(220.0)).child("file")).child("picture"));
                for im in self.info["images"].as_array().cloned().unwrap_or_default() {
                    let id = im["id"].as_u64().unwrap_or(0) & 0xffff;
                    let tag = im["tag"].as_u64().unwrap_or(0);
                    let file = self.value("image", tag, im["fileName"].as_str().unwrap_or(""));
                    let edited = self.is_edited(&format!("image|{tag}|0"));
                    let card = self.image_card(&im, true, cx);
                    out = out.child(
                        h_flex()
                            .gap_2()
                            .px_1()
                            .py_0p5()
                            .font_family("Menlo")
                            .child(div().id(("im-id", tag as usize)).w(px(50.0)).text_color(theme.link).cursor_pointer().child(id.to_string()).on_click(cx.listener(move |this, _, _, cx| this.open(Node::Char(id), cx))))
                            .child(div().w(px(90.0)).child(format!("{}×{}", im["width"], im["height"])))
                            .child(div().w(px(60.0)).opacity(0.6).child(im["format"].to_string()))
                            .child(div().w(px(220.0)).when(edited, |d| d.border_l_2().border_color(theme.warning).pl_1()).child(self.field(format!("image:{tag}"), file, 210.0, cx)))
                            .child(card),
                    );
                }
            }
            Node::Linkage => {
                let mut ex = v_flex().flex_1().gap_0p5().child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Exported symbols"));
                for (k, e) in self.info["exports"].as_array().cloned().unwrap_or_default().iter().enumerate() {
                    let id = e["id"].as_u64().unwrap_or(0);
                    ex = ex.child(h_flex().id(("lk-ex", k)).gap_1().font_family("Menlo").cursor_pointer().hover(|d| d.text_color(theme.link)).child(div().opacity(0.5).child(id.to_string())).child(e["name"].as_str().unwrap_or("").to_string()).on_click(cx.listener(move |this, _, _, cx| this.open(Node::Char(id), cx))));
                }
                let mut im = v_flex().flex_1().gap_0p5().child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Imports"));
                for i in self.info["imports"].as_array().cloned().unwrap_or_default() {
                    im = im.child(div().font_family("Menlo").child(i["url"].as_str().unwrap_or("").to_string()));
                    for s in i["symbols"].as_array().cloned().unwrap_or_default() {
                        im = im.child(h_flex().pl_3().gap_1().font_family("Menlo").opacity(0.7).child(div().opacity(0.5).child(s["id"].to_string())).child(s["name"].as_str().unwrap_or("").to_string()));
                    }
                }
                out = out.child(h_flex().items_start().gap_2().child(ex).child(im));
            }
            Node::Tags => {
                let q = self.tag_filter.read(cx).value().trim().to_ascii_lowercase();
                out = out.child(div().pb_1().child(Input::new(&self.tag_filter).xsmall()));
                out = out.child(h_flex().gap_2().px_1().font_family("Menlo").text_color(theme.muted_foreground).child(div().w(px(50.0)).child("#")).child(div().w(px(170.0)).child("tag")).child(div().w(px(60.0)).child("id")).child(div().w(px(70.0)).child("bytes")).child("detail"));
                for t in self.info["tags"].as_array().cloned().unwrap_or_default() {
                    let name = t["name"].as_str().unwrap_or("").to_string();
                    let detail = t["detail"].as_str().unwrap_or("").to_string();
                    let id = t["id"].as_u64();
                    if !q.is_empty() && !name.to_ascii_lowercase().contains(&q) && !detail.to_ascii_lowercase().contains(&q) && id.map(|i| i.to_string()) != Some(q.clone()) {
                        continue;
                    }
                    let known = id.is_some_and(|i| self.char(i).is_some());
                    out = out.child(
                        h_flex()
                            .gap_2()
                            .px_1()
                            .font_family("Menlo")
                            .child(div().w(px(50.0)).opacity(0.5).child(t["index"].to_string()))
                            .child(div().w(px(170.0)).child(if name == "?" { format!("? ({})", t["code"]) } else { name }))
                            .child(div().id(("tg-id", t["index"].as_u64().unwrap_or(0) as usize)).w(px(60.0)).opacity(0.6).when(known, |d| d.text_color(theme.link).cursor_pointer()).child(id.map(|i| i.to_string()).unwrap_or_default()).when(known, |d| d.on_click(cx.listener(move |this, _, _, cx| this.open(Node::Char(id.unwrap_or(0)), cx)))))
                            .child(div().w(px(70.0)).opacity(0.6).child(t["len"].to_string()))
                            .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(detail)),
                    );
                }
            }
            _ => {}
        }
        out.into_any_element()
    }

    fn scripts_key(&mut self, e: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        let k = e.keystroke.key.as_str();
        if k != "down" && k != "up" {
            return;
        }
        let flat = self.visible_scripts(cx);
        if flat.is_empty() {
            return;
        }
        let at = flat.iter().position(|s| Some(s.0.clone()) == self.script);
        let i = match (at, k) {
            (None, _) => 0,
            (Some(i), "down") => (i + 1).min(flat.len() - 1),
            (Some(i), _) => i.saturating_sub(1),
        };
        self.select_script(flat[i].0.clone(), cx);
        cx.stop_propagation();
    }

    /// The scripts the filter lets through, grouped in list order: (key, title, script).
    fn visible_scripts(&self, cx: &gpui::App) -> Vec<(String, String, J)> {
        let q = self.script_filter.read(cx).value().trim().to_ascii_lowercase();
        let all = self.scripts();
        let mut out = Vec::new();
        for (kind, _) in GROUPS {
            for sc in all.iter().filter(|s| s["kind"] == *kind) {
                let title = self.script_title(sc);
                let key = sc["key"].as_str().unwrap_or("").to_string();
                if q.is_empty() || title.to_ascii_lowercase().contains(&q) || key == q {
                    out.push((key, title, sc.clone()));
                }
            }
        }
        out
    }

    fn render_scripts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let visible = self.visible_scripts(cx);
        let all = self.scripts();
        let selected = all.iter().find(|s| s["key"].as_str() == self.script.as_deref()).cloned();
        // The view follows the selection.
        match (&selected, &self.script_view) {
            (Some(sc), Some(sv)) if sv.key == sc["key"].as_str().unwrap_or("") && !sv.reload => {}
            (Some(sc), sv) => {
                let pcode = sv.as_ref().is_some_and(|s| s.pcode && s.key == sc["key"].as_str().unwrap_or(""));
                self.script_view = Some(ScriptView { key: sc["key"].as_str().unwrap_or("").to_string(), loc: sc["loc"].clone(), pcode, editor: None, pending: None, base: String::new(), problems: Vec::new(), load_error: None, failure: None, checked: None, busy: false, reload: false });
                self.load_script(cx);
            }
            (None, _) => self.script_view = None,
        }
        let mut list = v_flex().id("mv-scripts").track_focus(&self.tree_focus).flex_1().min_h_0().overflow_y_scroll().pb_1().text_xs().on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| this.scripts_key(e, cx)));
        if all.is_empty() {
            list = list.child(div().p_2().opacity(0.6).child("This movie has no scripts."));
        }
        for (kind, title) in GROUPS {
            let items: Vec<&(String, String, J)> = visible.iter().filter(|(_, _, s)| s["kind"] == *kind).collect();
            if items.is_empty() {
                continue;
            }
            list = list.child(div().px_2().py_0p5().bg(theme.secondary).text_color(theme.muted_foreground).font_weight(gpui::FontWeight::SEMIBOLD).child(format!("{} {}", title.to_uppercase(), items.len())));
            for (k, (key, title, sc)) in items.into_iter().enumerate() {
                let on = self.script.as_deref() == Some(key.as_str());
                let staged = self.script_edit(&sc["loc"]).is_some();
                let key2 = key.clone();
                list = list.child(
                    h_flex()
                        .id(gpui::SharedString::from(format!("sc-{kind}-{k}")))
                        .gap_1()
                        .px_2()
                        .py_0p5()
                        .font_family("Menlo")
                        .cursor_pointer()
                        .when(on, |d| d.bg(theme.primary.opacity(0.2)))
                        .when(!on, |d| d.hover(|d| d.bg(theme.list_hover)))
                        .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(title.clone()))
                        .when(staged, |d| d.child(div().size(px(6.0)).rounded_full().bg(theme.warning)))
                        .tooltip({
                            let tip = format!("{} {}, {} bytes", sc["kind"].as_str().unwrap_or(""), key, sc["bytes"]);
                            move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.tree_focus.focus(window, cx);
                            this.select_script(key2.clone(), cx);
                        })),
                );
            }
        }
        let left = v_flex().w(px(290.0)).flex_none().h_full().border_r_1().border_color(theme.border).child(div().p_1().child(Input::new(&self.script_filter).xsmall())).child(list);
        let right: AnyElement = match (selected, &mut self.script_view) {
            (Some(sc), Some(sv)) => {
                if let Some(text) = sv.pending.take() {
                    let editor = cx.new(|cx| EditorState::new(window, cx).language("javascript").line_number(true));
                    editor.update(cx, |e, cx| e.set_value(text, window, cx));
                    sv.editor = Some(editor);
                }
                let pcode = sv.pcode;
                let modified = sv.editor.as_ref().is_some_and(|e| e.read(cx).value() != sv.base.as_str());
                let busy = sv.busy;
                let problems = sv.problems.clone();
                let load_error = sv.load_error.clone();
                let failure = sv.failure.clone();
                let checked = sv.checked.clone();
                let editor = sv.editor.clone();
                let staged = self.script_edit(&sc["loc"]).is_some();
                let toggle = |b: Button, on: bool| if on { b.primary() } else { b.ghost() };
                let mut v = v_flex().flex_1().min_w_0().h_full().child(
                    h_flex()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .flex_wrap()
                        .border_b_1()
                        .border_color(theme.border)
                        .text_xs()
                        .child(div().font_family("Menlo").font_weight(gpui::FontWeight::SEMIBOLD).overflow_hidden().text_ellipsis().child(self.script_title(&sc)))
                        .child(div().opacity(0.5).child(format!("{} · {} bytes", sc["kind"].as_str().unwrap_or(""), sc["bytes"])))
                        .when(staged, |d| d.child(div().px_1().rounded_sm().bg(theme.warning.opacity(0.3)).child("staged")))
                        .when(modified, |d| d.child(div().px_1().rounded_sm().bg(theme.info.opacity(0.3)).child("modified")))
                        .child(div().flex_1())
                        .child(toggle(Button::new("sc-as").xsmall().label("ActionScript"), !pcode).on_click(cx.listener(|this, _, window, cx| this.switch_mode(false, window, cx))))
                        .child(toggle(Button::new("sc-pcode").xsmall().label("P-code"), pcode).on_click(cx.listener(|this, _, window, cx| this.switch_mode(true, window, cx))))
                        .child(Button::new("sc-revert").xsmall().ghost().icon(Icon::new(IconName::RotateCcw)).label("Revert").disabled(!staged && !modified).tooltip("Back to the script as it is in the movie").on_click(cx.listener(|this, _, _, cx| this.revert_script(cx))))
                        .child(Button::new("sc-check").xsmall().ghost().icon(Icon::new(IconName::Check)).label("Check").disabled(busy).tooltip("Compile only").on_click(cx.listener(|this, _, window, cx| this.check_script(false, window, cx))))
                        .child(Button::new("sc-stage").xsmall().primary().icon(Icon::new(IconName::Upload)).label("Stage").disabled(busy || !modified).tooltip("Keep it as a pending edit of the movie; Apply writes it (⌘Enter)").on_click(cx.listener(|this, _, window, cx| this.check_script(true, window, cx)))),
                );
                if let Some(e) = load_error {
                    v = v.child(div().p_3().text_sm().text_color(theme.danger).child(e));
                } else if !problems.is_empty() && !pcode {
                    let mut d = v_flex().px_2().py_1().bg(theme.warning.opacity(0.1)).border_b_1().border_color(theme.border).text_xs().child(format!("{} part{} kept as P-code (__pcode blocks compile as written)", problems.len(), if problems.len() > 1 { "s" } else { "" }));
                    for p in problems {
                        d = d.child(div().font_family("Menlo").opacity(0.7).child(p));
                    }
                    v = v.child(d);
                }
                if let Some(ed) = editor {
                    v = v.child(div().flex_1().min_h_0().font_family("Menlo").text_sm().on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                        if e.keystroke.key == "enter" && (e.keystroke.modifiers.platform || e.keystroke.modifiers.control) {
                            cx.stop_propagation();
                            this.check_script(true, window, cx);
                        }
                    })).child(Editor::new(&ed).size_full()));
                }
                if let Some((line, msg)) = failure {
                    v = v.child(div().id("sc-failure").px_2().py_1().border_t_1().border_color(theme.border).text_xs().font_family("Menlo").text_color(theme.danger).cursor_pointer().child(msg).on_click(cx.listener(move |this, _, window, cx| {
                        if let (Some(l), Some(ed)) = (line, this.script_view.as_ref().and_then(|s| s.editor.clone())) {
                            ed.update(cx, |s, cx| s.set_cursor_position(Position { line: l.saturating_sub(1), character: 0 }, window, cx));
                        }
                    })));
                } else if let Some(c) = checked {
                    v = v.child(div().px_2().py_1().border_t_1().border_color(theme.border).text_xs().font_family("Menlo").text_color(theme.success).child(format!("Compiles: {c}")));
                }
                v.into_any_element()
            }
            _ => div().p_3().text_xs().opacity(0.6).child("Pick a script. Classes read as ActionScript 2 class definitions; every script can be edited as ActionScript or as P-code (the bytecode, one action per line), checked, and staged as a pending edit that Apply writes.").into_any_element(),
        };
        h_flex().size_full().items_start().child(left).child(right).into_any_element()
    }
}

impl ScriptView {
    pub fn summary(&self) -> J {
        json!({ "key": self.key, "pcode": self.pcode, "editor": self.editor.is_some(), "problems": self.problems.len(), "error": self.load_error, "failure": self.failure.as_ref().map(|f| f.1.clone()), "line": self.failure.as_ref().and_then(|f| f.0), "checked": self.checked })
    }
}

impl MovieEditor {
    pub(super) fn switch_mode_drive(&mut self, pcode: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.switch_mode(pcode, window, cx);
    }

    pub(super) fn check_script_drive(&mut self, stage: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.check_script(stage, window, cx);
    }

    /// Dev: put `text` (with `\n` for new lines) in the script editor.
    pub(super) fn replace_script_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let ed = self.script_view.as_ref().and_then(|s| s.editor.clone()).ok_or("no script editor yet")?;
        let text = text.replace("\\n", "\n");
        ed.update(cx, |s, cx| s.set_value(text, window, cx));
        Ok(())
    }
}
