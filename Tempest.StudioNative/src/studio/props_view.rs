//! A property editor for any export of any package (the map's actors and
//! Kismet nodes live in level packages that are not the open one): the
//! inspector's rows (edited in place, array elements added, duplicated,
//! removed, properties reset or added), each change an `object.*` call
//! whose reply is the object as it is now, announced as `PropEvent::Changed`.

use gpui::{div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, Subscription, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use crate::backend::Backend;
use crate::model::{self, Edit, Inspected, ListRole};

pub enum PropEvent {
    /// The object changed (an edit went through).
    Changed,
    /// An edit failed.
    Error(String),
}

pub struct PropEditor {
    backend: Backend,
    pub path: String,
    pub index: usize,
    ins: Option<Inspected>,
    error: Option<String>,
    editing: Option<String>,
    /// The text the edit started with (an unchanged value writes nothing).
    edit_from: String,
    /// Rows folded or unfolded by the user, by property path.
    open: std::collections::HashMap<String, bool>,
    input: Entity<InputState>,
    /// "Add a property": what the class declares and the object does not store.
    adding: Option<Vec<(String, String)>>,
    add_filter: Entity<InputState>,
    /// Bumped on every load, so a late reply for another object is dropped.
    seq: u64,
    _subs: Vec<Subscription>,
}

impl EventEmitter<PropEvent> for PropEditor {}

impl PropEditor {
    pub fn new(backend: Backend, window: &mut Window, cx: &mut Context<Self>) -> PropEditor {
        let input = cx.new(|cx| InputState::new(window, cx));
        let add_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter properties"));
        let subs = vec![
            cx.subscribe_in(&input, window, |this, _, e: &InputEvent, _, cx| match e {
                // Enter and leaving the field apply; Escape leaves the value as it was.
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit(cx),
                _ => {}
            }),
            cx.subscribe(&add_filter, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        PropEditor { backend, path: String::new(), index: 0, ins: None, error: None, editing: None, edit_from: String::new(), open: std::collections::HashMap::new(), input, adding: None, add_filter, seq: 0, _subs: subs }
    }

    /// Show an export (read again even when it is the one shown).
    pub fn load(&mut self, path: &str, index: usize, cx: &mut Context<Self>) {
        if self.path != path || self.index != index {
            self.ins = None;
            self.adding = None;
        }
        self.path = path.to_string();
        self.index = index;
        self.error = None;
        self.seq += 1;
        let seq = self.seq;
        let call = self.backend.call("object.get", json!({ "path": path, "index": index }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.seq != seq {
                    return;
                }
                match r {
                    Ok(r) => this.ins = Some(model::inspected(&r.json)),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.ins = None;
        self.path.clear();
        self.seq += 1;
        cx.notify();
    }

    /// An `object.*` call whose reply is the object.
    fn edit(&mut self, method: &'static str, mut params: J, what: String, cx: &mut Context<Self>) {
        params["path"] = json!(self.path);
        params["index"] = json!(self.index);
        let call = self.backend.call(method, params);
        let seq = self.seq;
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        if this.seq == seq {
                            this.ins = Some(model::inspected(&r.json));
                        }
                        cx.emit(PropEvent::Changed);
                    }
                    Err(e) => cx.emit(PropEvent::Error(format!("{what}: {e}"))),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn set(&mut self, prop: String, value: String, cx: &mut Context<Self>) {
        self.edit("object.set", json!({ "prop": prop.clone(), "value": value }), format!("Could not set {prop}"), cx);
    }

    fn list_op(&mut self, prop: String, op: &'static str, at: Option<usize>, cx: &mut Context<Self>) {
        self.edit("object.list", json!({ "prop": prop.clone(), "op": op, "at": at }), prop, cx);
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        let Some(prop) = self.editing.take() else { return };
        let value = self.input.read(cx).value().to_string();
        cx.notify();
        if value != self.edit_from {
            self.set(prop, value, cx);
        }
    }

    fn start(&mut self, prop: String, draft: String, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(prop);
        self.edit_from = draft.clone();
        self.input.update(cx, |s, cx| {
            s.set_value(draft, window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    fn toggle_adding(&mut self, cx: &mut Context<Self>) {
        if self.adding.take().is_some() {
            cx.notify();
            return;
        }
        self.adding = Some(Vec::new());
        let call = self.backend.call("object.missing", json!({ "path": self.path, "index": self.index }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.adding.is_some() {
                    this.adding = Some(r.map(|r| r.json.as_array().into_iter().flatten().map(|m| (m["name"].as_str().unwrap_or_default().to_string(), m["type"].as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default());
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The text of a property as shown (for state reports).
    pub fn rows(&self) -> Vec<(String, String)> {
        self.ins.as_ref().map(|i| i.rows.iter().map(|r| (r.path.clone(), r.value.clone())).collect()).unwrap_or_default()
    }
}

impl Render for PropEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let Some(ins) = &self.ins else {
            let has_err = self.error.is_some();
            let text = self.error.as_deref().map(super::ui::explain_error).unwrap_or_else(|| if self.path.is_empty() { String::new() } else { "Reading…".into() });
            return div().p_2().text_xs().whitespace_normal().text_color(if has_err { theme.danger } else { theme.muted_foreground }).child(text).into_any_element();
        };
        // Structs and arrays fold: nested ones and long arrays start folded.
        let ring = theme.ring;
        let hover_bg = theme.list_hover;
        let shown = crate::model::unfolded(&ins.rows, |r| self.open.get(&r.path).copied().unwrap_or_else(|| r.starts_open()));
        let rows: Vec<AnyElement> = shown
            .into_iter()
            .map(|k| (k, &ins.rows[k]))
            .map(|(k, r)| {
                let value: AnyElement = if self.editing.as_deref() == Some(r.path.as_str()) {
                    div().flex_1().min_w_0().child(Input::new(&self.input).xsmall()).into_any_element()
                } else {
                    let cell = div().id(("pv", k)).flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().px_1().rounded_sm().child(r.value.clone());
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
                                        this.set(path_k.clone(), if b { "False" } else { "True" }.into(), cx);
                                    }
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| this.set(path.clone(), if b { "False" } else { "True" }.into(), cx)))
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
                                        this.start(path_k.clone(), draft_k.clone(), window, cx);
                                    }
                                }))
                                .on_click(cx.listener(move |this, _, window, cx| this.start(path.clone(), draft.clone(), window, cx)))
                                .into_any_element()
                        }
                    }
                };
                let mut ops = h_flex().gap_0p5();
                match &r.list {
                    ListRole::Array => {
                        let p = r.path.clone();
                        ops = ops.child(Button::new(("pv-add", k)).xsmall().ghost().text_color(theme.muted_foreground).icon(Icon::new(IconName::Plus)).tooltip("Add an element").on_click(cx.listener(move |this, _, _, cx| this.list_op(p.clone(), "add", None, cx))));
                    }
                    ListRole::Element { array, at } => {
                        let (a1, a2, at) = (array.clone(), array.clone(), *at);
                        ops = ops
                            .child(Button::new(("pv-dup", k)).xsmall().ghost().text_color(theme.muted_foreground).icon(Icon::new(IconName::Copy)).tooltip("Duplicate the element").on_click(cx.listener(move |this, _, _, cx| this.list_op(a1.clone(), "duplicate", Some(at), cx))))
                            .child(Button::new(("pv-rm", k)).xsmall().ghost().text_color(theme.muted_foreground).icon(Icon::new(IconName::Trash)).tooltip("Remove the element").on_click(cx.listener(move |this, _, _, cx| this.list_op(a2.clone(), "remove", Some(at), cx))));
                    }
                    ListRole::None => {}
                }
                if r.resettable {
                    let p = r.path.clone();
                    ops = ops.child(Button::new(("pv-reset", k)).xsmall().ghost().text_color(theme.muted_foreground).icon(Icon::new(IconName::RotateCcw)).tooltip("Back to the default (stop storing it)").on_click(cx.listener(move |this, _, _, cx| this.list_op(p.clone(), "removeProperty", None, cx))));
                }
                h_flex()
                    .min_h(px(20.0))
                    .py_0p5()
                    .pl(px(r.depth as f32 * 12.0))
                    .gap_1()
                    .text_xs()
                    .child({
                        let foldable = r.children > 0;
                        let open = self.open.get(&r.path).copied().unwrap_or_else(|| r.starts_open());
                        let (path, path_k) = (r.path.clone(), r.path.clone());
                        let tip = if r.kind.is_empty() { r.path.clone() } else { format!("{} ({})", r.path, r.kind) };
                        h_flex()
                            .id(("pv-name", k))
                            .w(gpui::rems(9.5))
                            .min_w(px(140.0))
                            .max_w(px(220.0))
                            .flex_none()
                            .gap_0p5()
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
                                            this.open.insert(path_k.clone(), !open);
                                            cx.notify();
                                        }
                                    }))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open.insert(path.clone(), !open);
                                        cx.notify();
                                    }))
                            })
                            .child(div().w(px(12.0)).flex_none().when(foldable, |d| d.child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall().text_color(theme.muted_foreground))))
                            .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(r.name.clone()))
                    })
                    .child(value)
                    .child(ops)
                    .child(div().flex_none().pr_1().text_color(theme.muted_foreground).child(r.kind.clone()))
                    .into_any_element()
            })
            .collect();
        let mut body = v_flex().gap_0p5().children(rows);
        if ins.kind == "object" {
            body = body.child(h_flex().pt_1().child(Button::new("pv-add-prop").xsmall().ghost().label(if self.adding.is_some() { "Close the list" } else { "Add a property…" }).on_click(cx.listener(|this, _, _, cx| this.toggle_adding(cx)))));
            if let Some(list) = &self.adding {
                let q = self.add_filter.read(cx).value().to_ascii_lowercase();
                let items: Vec<AnyElement> = list
                    .iter()
                    .filter(|(n, _)| q.is_empty() || n.to_ascii_lowercase().contains(&q))
                    .take(200)
                    .enumerate()
                    .map(|(k, (name, ty))| {
                        let (n, n_k) = (name.clone(), name.clone());
                        h_flex()
                            .id(("pv-missing", k))
                            .focusable()
                            .tab_index(0)
                            .min_h(px(20.0))
                            .py_0p5()
                            .px_1()
                            .gap_2()
                            .text_xs()
                            .rounded_sm()
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .focus_visible(move |s| s.border_color(ring))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .child(div().flex_1().min_w_0().child(name.clone()))
                            .child(div().text_color(theme.muted_foreground).child(ty.clone()))
                            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                                if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.adding = None;
                                    this.edit("object.addProperty", json!({ "name": n_k.clone() }), format!("Could not add {n_k}"), cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.adding = None;
                                this.edit("object.addProperty", json!({ "name": n.clone() }), format!("Could not add {n}"), cx);
                            }))
                            .into_any_element()
                    })
                    .collect();
                body = body.child(v_flex().gap_0p5().child(Input::new(&self.add_filter).xsmall().cleanable(true)).child(if list.is_empty() { div().text_xs().text_color(theme.muted_foreground).child("Reading…").into_any_element() } else { v_flex().children(items).into_any_element() }));
            }
        }
        // Escape leaves a value being edited as it was.
        body.on_action(cx.listener(|this, _: &super::Cancel, _, cx| {
            if this.editing.take().is_some() {
                cx.notify();
            } else {
                cx.propagate();
            }
        }))
        .into_any_element()
    }
}
