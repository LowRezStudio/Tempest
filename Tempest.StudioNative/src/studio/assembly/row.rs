//! The Assembly view's row pane: every field of the selected row (edited in
//! place, flags as switches, row sets and `*_ID` links opened), each message
//! in every language (edited, added, opened in its language file), and the
//! rows of the file that refer to the row's id, looked for on demand.

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, MouseButton, MouseDownEvent, SharedString};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, Textarea};
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit_assets::IconName;

use super::{cell_text, is_flag, is_message, is_value_id, ref_of, short, Assembly, Edit};

/// A string field longer than this is edited in the multi-line editor.
const LONG_TEXT: usize = 60;

impl Assembly {
    pub(super) fn render_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let title = crate::studio::ui::pane_header(
            "Row",
            div().when_some(self.selected, |d, i| d.text_xs().font_family(mono.clone()).font_weight(gpui::FontWeight::NORMAL).child(format!("#{i}"))),
            &theme,
        );
        let Some(row) = &self.row else {
            let text = if self.selected.is_some() { "Reading the row…" } else { "Select a row to see every field, follow its references and edit its messages." };
            let body = if self.selected.is_some() { crate::studio::ui::loading(text, &theme) } else { crate::studio::ui::empty(IconName::Table, "No row selected", text, &theme) };
            return v_flex().size_full().child(title).child(div().flex_1().min_h_0().child(body)).into_any_element();
        };
        let index = row.index;
        let is_lang = self.doc.as_deref() != Some("assembly");
        let languages = self.languages();
        let mut body = v_flex().p_2().text_xs();

        // Who refers to it (asked for: it reads the whole file).
        if let Some((field, value)) = self.id_field().filter(|_| !is_lang) {
            let mut used = v_flex().mb_2().p_2().rounded_md().border_1().border_color(theme.border).child(
                h_flex()
                    .gap_2()
                    .child(Icon::new(IconName::Link2).size_3().text_color(muted))
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Used by"))
                    .child(div().font_family(mono.clone()).text_color(muted).child(format!("{field} {value}")))
                    .child(div().flex_1())
                    .child(match &row.usages {
                        Some((list, cut)) => div().text_color(muted).child(format!("{}{} rows", list.len(), if *cut { "+" } else { "" })).into_any_element(),
                        None => Button::new("usages-find").xsmall().ghost().label(if row.usages_loading { "Looking…" } else { "Find" }).disabled(row.usages_loading).tooltip("The rows of this file that name this id").on_click(cx.listener(|this, _, _, cx| this.find_usages(cx))).into_any_element(),
                    }),
            );
            if let Some((list, _)) = &row.usages {
                if list.is_empty() {
                    used = used.child(div().mt_1().text_color(muted).child("Nothing in this file refers to it."));
                } else {
                    let mut rows = v_flex().id("usages").mt_1().max_h(px(192.0)).overflow_y_scroll().font_family(mono.clone());
                    for (k, u) in list.iter().enumerate() {
                        let at = u.path.iter().map(|(t, i)| format!("{}[{i}]", short(t))).collect::<Vec<_>>().join(" › ");
                        let (p1, p2) = (u.path.clone(), u.path.clone());
                        rows = rows.child(
                            h_flex()
                                .id(("use", k))
                                .gap_2()
                                .px_1()
                                .py_0p5()
                                .rounded_sm()
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.list_hover))
                                .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| this.go_to_path(p1.clone(), e.modifiers().secondary(cx), window, cx)))
                                .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _: &MouseDownEvent, window, cx| this.go_to_path(p2.clone(), true, window, cx)))
                                .child(div().flex_none().max_w(gpui::relative(0.6)).overflow_hidden().text_ellipsis().whitespace_nowrap().child(at))
                                .child(div().flex_none().text_color(muted.opacity(0.8)).child(u.field.clone()))
                                .when(!u.label.is_empty(), |d| d.child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(muted).child(u.label.clone()))),
                        );
                    }
                    used = used.child(rows);
                }
            }
            body = body.child(used);
        }

        for (k, f) in row.fields.iter().enumerate() {
            let name = f.name.clone();
            let text = cell_text(&f.value);
            let is_set = f.kind == "rowset" || f.value.get("rows").is_some();
            let flag = f.kind == "string" && is_flag(&name) && matches!(f.value.as_str(), Some("y" | "n"));
            let blob = f.kind == "blob";
            let editing = matches!(&self.edit, Some(Edit::Field { row, field, .. }) if *row == index && field == &name);
            let long = f.kind == "string" && (text.chars().count() > LONG_TEXT || text.contains('\n'));
            let link = ref_of(&name).filter(|_| f.value.is_number());
            let tip = format!("{name} ({})", f.kind);
            let label = div()
                .id(("rfn", k))
                .w(px(176.0))
                .flex_none()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .font_family(mono.clone())
                .text_color(muted)
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                .child(name.clone());
            let value_el: AnyElement = if is_set {
                let field = name.clone();
                Button::new(("rset", k)).xsmall().ghost().icon(Icon::new(IconName::Table)).label(text.clone()).tooltip("Open the row set").on_click(cx.listener(move |this, _, window, cx| this.open_set(index, field.clone(), window, cx))).into_any_element()
            } else if flag {
                let field = name.clone();
                h_flex()
                    .gap_2()
                    .child(Switch::new(("rflag", k)).xsmall().checked(f.value.as_str() == Some("y")).on_click(cx.listener(move |this, _: &bool, _, cx| this.toggle_flag(index, field.clone(), cx))))
                    .child(div().text_color(muted).child(text.clone()))
                    .into_any_element()
            } else if editing {
                div().key_context("AsmEdit").flex_1().min_w_0().child(if long { Textarea::new(&self.edit_area).into_any_element() } else { Input::new(&self.edit_input).xsmall().into_any_element() }).into_any_element()
            } else {
                let (field, draft) = (name.clone(), text.clone());
                let tip = if blob { "binary" } else { "click to edit" };
                div()
                    .id(("rf", k))
                    .flex_1()
                    .min_w_0()
                    .px_1()
                    .rounded_sm()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .font_family(mono.clone())
                    .hover(|s| s.bg(theme.list_hover))
                    .when(blob, |d| d.text_color(muted))
                    .when(!blob, |d| d.cursor_text())
                    // An empty value keeps a line to click on.
                    .min_h(px(16.0))
                    .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip).build(window, cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if !blob {
                            this.start_edit(Edit::Field { row: index, field: field.clone(), long }, draft.clone(), draft.clone(), !long, window, cx);
                        }
                    }))
                    .child(text.clone())
                    .into_any_element()
            };
            let mut line = h_flex().gap_2().items_start().child(label).child(value_el);
            if let (Some((table, _)), false) = (link, editing) {
                let (field, value) = (name.clone(), f.value.clone());
                line = line.child(Button::new(("rgo", k)).xsmall().ghost().icon(Icon::new(IconName::ExternalLink)).tooltip(format!("Go to {}", short(table))).on_click(cx.listener(move |this, e: &ClickEvent, window, cx| this.follow(&field, &value, e.modifiers().secondary(cx), window, cx))));
            }
            if is_value_id(&name) && !editing && !is_set {
                let field = name.clone();
                line = line.child(Button::new(("rpick", k)).xsmall().ghost().icon(Icon::new(IconName::ListTree)).tooltip("Pick one of the valid values").on_click(cx.listener(move |this, _, _, cx| this.pick_value(index, field.clone(), cx))));
            }
            let mut block = v_flex().py_1().border_b_1().border_color(theme.border.opacity(0.6)).child(line);
            // What a valid value id means.
            if let Some(t) = f.value.as_i64().filter(|_| is_value_id(&name)).and_then(|id| self.valid.get(&id)) {
                block = block.child(div().pl(px(184.0)).text_color(muted).child(t.clone()));
            }
            // The message in every language.
            if let Some(id) = f.value.as_i64().filter(|id| is_message(&name) && *id != 0) {
                let mut langs = v_flex().mt_1().ml_2().pl_2().gap_0p5().border_l_1().border_color(theme.border);
                let known = self.messages.get(&id);
                for (j, lang) in languages.iter().enumerate() {
                    let m = known.and_then(|m| m.get(lang));
                    let editing = matches!(&self.edit, Some(Edit::Message { id: i, lang: l }) if *i == id && l == lang);
                    let mut line = h_flex().gap_2().items_start().child(div().w(px(32.0)).flex_none().font_family(mono.clone()).text_color(muted.opacity(0.8)).child(lang.clone()));
                    if editing {
                        line = line.child(div().key_context("AsmEdit").flex_1().min_w_0().child(Textarea::new(&self.edit_area)));
                    } else {
                        let (lang2, draft) = (lang.clone(), m.map(|m| m.text.clone()).unwrap_or_default());
                        let tip = match m {
                            Some(m) if !m.name.is_empty() => format!("{} (click to edit)", m.name),
                            Some(_) => "click to edit".to_string(),
                            None => "no message in this language (click to add)".to_string(),
                        };
                        line = line.child(
                            div()
                                .id(SharedString::from(format!("msg-{k}-{j}")))
                                .flex_1()
                                .min_w_0()
                                .px_1()
                                .rounded_sm()
                                .cursor_text()
                                .whitespace_normal()
                                .hover(|s| s.bg(theme.list_hover))
                                .when(m.is_none(), |d| d.italic().text_color(muted.opacity(0.6)))
                                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                                .on_click(cx.listener(move |this, _, window, cx| this.start_edit(Edit::Message { id, lang: lang2.clone() }, draft.clone(), draft.clone(), false, window, cx)))
                                .child(m.map(|m| m.text.clone()).unwrap_or_else(|| "—".into())),
                        );
                        if m.is_some_and(|m| !m.path.is_empty()) {
                            let lang3 = lang.clone();
                            line = line.child(Button::new(SharedString::from(format!("msg-open-{k}-{j}"))).xsmall().ghost().icon(Icon::new(IconName::ExternalLink)).tooltip(format!("Open in Lang_{lang}.dat")).on_click(cx.listener(move |this, _, window, cx| this.open_message(id, lang3.clone(), window, cx))));
                        }
                    }
                    langs = langs.child(line);
                }
                block = block.child(langs);
            }
            body = body.child(block);
        }
        if is_lang {
            body = body.child(div().mt_2().text_color(muted.opacity(0.8)).child("Messages are looked up by MSG_ID from the assembly's *_MSG_ID columns; keep ids unique."));
        }
        v_flex().size_full().child(title).child(div().id("arow-pane").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.row_scroll).child(body)).child(crate::driver::probe("asm-row")).into_any_element()
    }
}
