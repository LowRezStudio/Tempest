//! A sprite's (or the root's) timeline under the stage: a frame strip with
//! labels and frame scripts when there is more than one frame, then the
//! display list of the current frame. A row selects the object (the stage
//! highlights it), double-click opens its character. Objects outside the
//! stage, objects drawn over the same spot ("stacked") and objects with
//! pending edits are marked. The eye column hides a layer from the stage
//! render only (Alt+click: show only this layer), with hide off-stage, solo,
//! hide all and show all above the list. Up/Down walk the rows, Left/Right
//! the frames, Enter opens the selected object's character.

use std::collections::HashMap;

use gpui::{div, prelude::*, px, AnyElement, Context, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::Value as J;

use super::geom::{decompose, fmt, Rect, M};
use super::{MovieEditor, Node};

impl MovieEditor {
    fn off_stage(&self, e: &J, outline: Option<Rect>) -> bool {
        match (outline, Rect::from_json(&e["rect"])) {
            (Some(o), Some(r)) => r.outside(&o),
            _ => false,
        }
    }

    /// Show only these layers.
    fn solo(&mut self, depths: &[u64], cx: &mut Context<Self>) {
        let all: Vec<u64> = self.entries().iter().filter_map(|e| e["depth"].as_u64()).filter(|d| !depths.contains(d)).collect();
        self.set_hidden(&all, true, cx);
        self.set_hidden(depths, false, cx);
    }

    fn timeline_key(&mut self, e: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        if e.keystroke.modifiers.alt || e.keystroke.modifiers.platform {
            return;
        }
        let list = self.entries();
        let frames = self.frames();
        match e.keystroke.key.as_str() {
            k @ ("down" | "up") => {
                if list.is_empty() {
                    return;
                }
                let at = self.depth.and_then(|d| list.iter().position(|x| x["depth"].as_u64() == Some(d)));
                let i = match (at, k) {
                    (None, _) => 0,
                    (Some(i), "down") => (i + 1).min(list.len() - 1),
                    (Some(i), _) => i.saturating_sub(1),
                };
                self.depth = list[i]["depth"].as_u64();
            }
            k @ ("left" | "right") if frames > 1 => {
                let f = if k == "right" { (self.frame + 1).min(frames - 1) } else { self.frame.saturating_sub(1) };
                self.set_frame(f, cx);
            }
            "enter" | "space" => {
                if let Some(id) = self.selected_entry().and_then(|s| s["character"].as_u64()) {
                    self.open(Node::Char(id), cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn render_timeline(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let frames = self.frames();
        let t = self.timeline.clone().unwrap_or(J::Null);
        let labels: HashMap<u64, String> = t["labels"].as_array().into_iter().flatten().filter_map(|l| Some((l["frame"].as_u64()?, l["name"].as_str()?.to_string()))).collect();
        let scripts: Vec<u64> = t["actions"].as_array().into_iter().flatten().filter_map(|a| a["frame"].as_u64()).collect();
        let outline = self.outline_of(self.sprite());
        let entries = self.entries();
        let hidden = self.hidden();
        let mut out = v_flex().id("mv-timeline").track_focus(&self.timeline_focus).size_full().text_xs().on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| this.timeline_key(e, cx)));
        if frames > 1 {
            let mut strip = h_flex().flex_1().min_w_0().overflow_hidden();
            for f in 0..frames {
                let on = f == self.frame;
                let label = labels.get(&(f as u64));
                let script = scripts.contains(&(f as u64));
                strip = strip.child(
                    div()
                        .id(("mv-frame", f))
                        .relative()
                        .h(px(18.0))
                        .min_w(px(6.0))
                        .flex_1()
                        .border_r_1()
                        .border_color(theme.background)
                        .bg(if on { theme.primary } else if label.is_some() { theme.secondary_hover } else { theme.secondary })
                        .hover(|d| d.bg(theme.list_hover))
                        .when(script, |d| d.child(div().absolute().top(px(2.0)).left(px(2.0)).size(px(4.0)).rounded_full().bg(theme.warning)))
                        .tooltip({
                            let tip = format!("frame {}{}{}", f + 1, label.map(|l| format!(" · {l}")).unwrap_or_default(), if script { " · script" } else { "" });
                            move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.set_frame(f, cx))),
                );
            }
            out = out.child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().font_family("Menlo").text_color(theme.muted_foreground).child(format!("frame {}/{frames}", self.frame + 1)))
                    .child(strip)
                    .children(labels.get(&(self.frame as u64)).map(|l| div().px_1().rounded_sm().bg(theme.secondary_hover).child(l.clone()))),
            );
        }
        let off: Vec<u64> = entries.iter().filter(|e| self.off_stage(e, outline) && !hidden.contains(&e["depth"].as_u64().unwrap_or(0))).filter_map(|e| e["depth"].as_u64()).collect();
        let hidden_here = entries.iter().filter(|e| hidden.contains(&e["depth"].as_u64().unwrap_or(0))).count();
        let n = entries.len();
        let (off2, all): (Vec<u64>, Vec<u64>) = (off.clone(), entries.iter().filter_map(|e| e["depth"].as_u64()).collect());
        let shown: Vec<u64> = hidden.iter().copied().collect();
        out = out.child(
            h_flex()
                .gap_1()
                .px_2()
                .py_0p5()
                .border_b_1()
                .border_color(theme.border)
                .child(div().text_color(theme.muted_foreground).child(format!("{n} layer{}{}", if n == 1 { "" } else { "s" }, if hidden_here > 0 { format!(" · {hidden_here} hidden in the view") } else { String::new() })))
                .child(div().flex_1())
                .child(Button::new("mv-hide-off").xsmall().ghost().icon(Icon::new(IconName::EyeOff)).label(if off.is_empty() { "Hide off-stage".to_string() } else { format!("Hide off-stage ({})", off.len()) }).tooltip("Hide every layer that lies entirely outside the stage").disabled(off.is_empty()).on_click(cx.listener(move |this, _, _, cx| this.set_hidden(&off2, true, cx))))
                .child(Button::new("mv-solo").xsmall().ghost().icon(Icon::new(IconName::Eye)).label("Solo selected").tooltip("Show only the selected layer (Alt+click an eye does the same for its layer)").disabled(self.depth.is_none()).on_click(cx.listener(|this, _, _, cx| {
                    if let Some(d) = this.depth {
                        this.solo(&[d], cx);
                    }
                })))
                .child(Button::new("mv-hide-all").xsmall().ghost().icon(Icon::new(IconName::EyeOff)).label("Hide all").tooltip("Hide every layer (then tick the ones to look at)").on_click(cx.listener(move |this, _, _, cx| this.set_hidden(&all, true, cx))))
                .child(Button::new("mv-show-all").xsmall().ghost().icon(Icon::new(IconName::Eye)).label("Show all").tooltip("Show every layer again").disabled(shown.is_empty()).on_click(cx.listener(move |this, _, _, cx| this.set_hidden(&shown, false, cx)))),
        );
        // Layers drawn over nearly the same rectangle: stacked states, hiding one changes nothing visible.
        let visible: Vec<(&J, Rect)> = entries.iter().filter(|e| !hidden.contains(&e["depth"].as_u64().unwrap_or(0))).filter_map(|e| Some((e, Rect::from_json(&e["rect"])?))).collect();
        let head = |t: &'static str, w: f32, right: bool| div().w(px(w)).flex_none().when(right, |d| d.text_right()).child(t);
        let mut table = v_flex().id("mv-layers").flex_1().min_h_0().overflow_y_scroll().font_family("Menlo");
        table = table.child(
            h_flex()
                .gap_2()
                .px_2()
                .py_0p5()
                .text_color(theme.muted_foreground)
                .child(head("", 20.0, false))
                .child(head("depth", 46.0, false))
                .child(div().w(px(150.0)).flex_none().child("name"))
                .child(div().flex_1().min_w_0().child("character"))
                .child(head("x", 58.0, true))
                .child(head("y", 58.0, true))
                .child(head("scale", 70.0, true))
                .child(head("rot", 44.0, true))
                .child(head("alpha", 44.0, true))
                .child(head("tag", 70.0, false)),
        );
        if entries.is_empty() {
            table = table.child(div().px_2().py_1().text_color(theme.muted_foreground).child("Nothing on this frame."));
        }
        for (k, e) in entries.iter().enumerate() {
            let depth = e["depth"].as_u64().unwrap_or(0);
            let id = e["character"].as_u64().unwrap_or(0);
            let is_hidden = hidden.contains(&depth);
            let d = decompose(&M::from_json(&e["matrix"]));
            let kind = self.kind_of(id).to_string();
            let class_code = self.char(id).is_some_and(|c| c["attached"].as_array().into_iter().flatten().any(|a| a["name"] == "DoInitAction"));
            let alpha0 = e["cxform"]["mul"][3].as_f64() == Some(0.0);
            let stacked: Vec<String> = Rect::from_json(&e["rect"])
                .map(|r| visible.iter().filter(|(o, orect)| o["depth"] != e["depth"] && r.overlap(orect) >= 0.75).map(|(o, _)| o["name"].as_str().map(str::to_string).unwrap_or_else(|| self.label(o["character"].as_u64().unwrap_or(0)))).collect())
                .unwrap_or_default();
            let scale = if d.sx == 1.0 && d.sy == 1.0 { String::new() } else if (d.sx - d.sy).abs() < 1e-9 { format!("{}%", fmt(d.sx * 100.0, 1)) } else { format!("{}%×{}%", fmt(d.sx * 100.0, 1), fmt(d.sy * 100.0, 1)) };
            let placed = e["placedBy"].as_u64().unwrap_or(0);
            let matrix_by = e["matrixBy"].as_u64().unwrap_or(0);
            let entry = e.clone();
            let mut name = h_flex().w(px(150.0)).flex_none().gap_1().overflow_hidden().child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().child(e["name"].as_str().unwrap_or("").to_string()));
            if self.entry_edited(e) {
                name = name.child(Icon::new(IconName::Pencil).xsmall().text_color(theme.warning));
            }
            if !stacked.is_empty() && !is_hidden {
                let tip = format!("Drawn at the same place as {}: hide those too to see what is under (or Alt+click the eye to show only this one)", stacked.join(", "));
                name = name.child(div().id(("mv-stacked", k)).px_1().rounded_sm().bg(theme.info.opacity(0.3)).child(format!("stacked ×{}", stacked.len() + 1)).tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)));
            }
            if self.off_stage(e, outline) {
                name = name.child(div().px_1().rounded_sm().bg(theme.warning.opacity(0.3)).child("off stage"));
            }
            if alpha0 {
                name = name.child(Icon::new(IconName::EyeOff).xsmall().text_color(theme.muted_foreground));
            }
            let row = h_flex()
                .id(("mv-layer", k))
                .gap_2()
                .px_2()
                .h(px(20.0))
                .cursor_pointer()
                .when(self.depth == Some(depth), |d| d.bg(theme.primary.opacity(0.2)))
                .when(self.depth != Some(depth), |d| d.hover(|d| d.bg(theme.list_hover)))
                .when(is_hidden, |d| d.opacity(0.45))
                .child(
                    div()
                        .id(("mv-eye", k))
                        .w(px(20.0))
                        .flex_none()
                        .child(Icon::new(if is_hidden { IconName::EyeOff } else { IconName::Eye }).xsmall())
                        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(if is_hidden { "Hidden in the stage render: click to show it. Alt+click: show only this layer." } else { "Click to hide it in the stage render (the movie is not changed). Alt+click: show only this layer." }).build(window, cx))
                        .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                            cx.stop_propagation();
                            if e.modifiers().alt {
                                this.solo(&[depth], cx);
                            } else {
                                this.set_hidden(&[depth], !is_hidden, cx);
                            }
                        })),
                )
                .child(div().w(px(46.0)).flex_none().text_color(theme.muted_foreground).child(format!("{depth}{}", e["clipDepth"].as_u64().map(|c| format!(" ⌁{c}")).unwrap_or_default())))
                .child(name)
                .child(h_flex().flex_1().min_w_0().gap_1().overflow_hidden().child(div().text_color(theme.muted_foreground).child(kind)).child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().child(self.label(id))).when(class_code, |d| d.child(Icon::new(IconName::Code).xsmall().text_color(theme.muted_foreground))))
                .child(div().w(px(58.0)).flex_none().text_right().child(fmt(d.x, 2)))
                .child(div().w(px(58.0)).flex_none().text_right().child(fmt(d.y, 2)))
                .child(div().w(px(70.0)).flex_none().text_right().child(scale))
                .child(div().w(px(44.0)).flex_none().text_right().child(if d.rot != 0.0 { format!("{}°", fmt(d.rot, 1)) } else { String::new() }))
                .child(div().w(px(44.0)).flex_none().text_right().child(e["cxform"]["mul"][3].as_f64().map(|a| format!("{}%", fmt(a * 100.0, 1))).unwrap_or_default()))
                .child(div().w(px(70.0)).flex_none().text_color(theme.muted_foreground).child(format!("#{placed}{}", if matrix_by != placed { format!("→{matrix_by}") } else { String::new() })))
                .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
                    this.timeline_focus.focus(window, cx);
                    if ev.click_count() >= 2 {
                        if let Some(id) = entry["character"].as_u64() {
                            this.open(Node::Char(id), cx);
                        }
                        return;
                    }
                    this.depth = Some(depth);
                    this.confirm_remove = false;
                    cx.notify();
                }));
            table = table.child(row);
        }
        out.child(table).into_any_element()
    }
}
