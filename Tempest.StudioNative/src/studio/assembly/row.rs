//! The Assembly view's row pane: every field of the selected row (edited in
//! place, flags as switches, row sets and `*_ID` links opened), each message
//! in every language (edited, added, opened in its language file), and the
//! rows of the file that refer to the row's id, looked for on demand.

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, KeyDownEvent, MouseButton, MouseDownEvent, SharedString};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, Textarea};
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit_assets::IconName;

use super::{cell_text, is_flag, is_message, is_value_id, ref_of, short, Assembly, Edit};

/// A string field longer than this is edited in the multi-line editor.
const LONG_TEXT: usize = 60;

/// Humanize screaming-snake MCTS column names (`HIBERNATE_INVULNERABILITY_FLAG` → `Hibernate invulnerability`).
pub(super) fn pretty_column(name: &str) -> String {
    let stem = name.strip_prefix("DATA_SET_").filter(|s| !s.is_empty()).unwrap_or(name);
    let stem = stem.strip_suffix("_FLAG").filter(|s| !s.is_empty()).unwrap_or(stem);
    let words: Vec<&str> = stem.split('_').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return name.to_string();
    }
    let is_acronym = |w: &str| {
        matches!(
            w,
            "ID" | "IDS" | "UI" | "FX" | "SFX" | "VFX" | "XP" | "VGS" | "VOX" | "NPC" | "AI" | "HUD" | "FOV" | "DPS" | "HP" | "URL" | "GUID" | "CRC" | "ASM" | "PVP" | "PVE" | "DLC"
        )
    };
    let mut out = String::new();
    for (i, &w) in words.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let upper = w.to_ascii_uppercase();
        if is_acronym(&upper) {
            out.push_str(&upper);
        } else if upper == "MSG" && words.get(i + 1).is_some_and(|n| n.eq_ignore_ascii_case("ID")) {
            out.push_str(if i == 0 { "Message" } else { "message" });
        } else if i == 0 {
            let mut chars = w.chars();
            if let Some(first) = chars.next() {
                for c in first.to_uppercase() {
                    out.push(c);
                }
                out.push_str(&chars.as_str().to_ascii_lowercase());
            }
        } else {
            out.push_str(&w.to_ascii_lowercase());
        }
    }
    out
}

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
                        Some((list, cut)) => {
                            let noun = if list.len() == 1 && !*cut { "row" } else { "rows" };
                            div().text_color(muted).child(format!("{}{} {noun}", list.len(), if *cut { "+" } else { "" })).into_any_element()
                        }
                        None => Button::new("usages-find").xsmall().ghost().label(if row.usages_loading { "Looking…" } else { "Find" }).disabled(row.usages_loading).tooltip(if row.usages_loading { "Searching all tables for references…" } else { "Find rows in this file that reference this ID" }).on_click(cx.listener(|this, _, _, cx| this.find_usages(cx))).into_any_element(),
                    }),
            );
            if let Some((list, _)) = &row.usages {
                if list.is_empty() {
                    used = used.child(div().mt_1().text_color(muted).child("Nothing in this file refers to it."));
                } else {
                    let mut rows = v_flex().id("usages").mt_1().max_h(px(192.0)).overflow_y_scroll().font_family(mono.clone());
                    for (k, u) in list.iter().enumerate() {
                        let at = u.path.iter().map(|(t, i)| format!("{}[{i}]", short(t))).collect::<Vec<_>>().join(" › ");
                        let (p1, p2, p_key) = (u.path.clone(), u.path.clone(), u.path.clone());
                        rows = rows.child(
                            h_flex()
                                .id(("use", k))
                                .focusable()
                                .tab_index(0)
                                .gap_2()
                                .px_1()
                                .py_0p5()
                                .rounded_sm()
                                .border_1()
                                .border_color(gpui::transparent_black())
                                .focus_visible(|s| s.border_color(theme.ring))
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.list_hover))
                                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                    if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        this.go_to_path(p_key.clone(), false, window, cx);
                                    }
                                }))
                                .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| this.go_to_path(p1.clone(), e.modifiers().secondary(cx), window, cx)))
                                .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _: &MouseDownEvent, window, cx| this.go_to_path(p2.clone(), true, window, cx)))
                                .child(div().flex_none().max_w(gpui::relative(0.6)).overflow_hidden().text_ellipsis().whitespace_nowrap().child(at))
                                .child(div().flex_none().text_color(muted).child(u.field.clone()))
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
                .w(gpui::rems(11.0))
                .min_w(px(160.0))
                .max_w(gpui::relative(0.45))
                .flex_none()
                .whitespace_normal()
                .text_color(muted)
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                .child(pretty_column(&name));
            let value_el: AnyElement = if is_set {
                let field = name.clone();
                Button::new(("rset", k)).xsmall().ghost().icon(Icon::new(IconName::Table)).label(text.clone()).tooltip("Open the row set").on_click(cx.listener(move |this, _, window, cx| this.open_set(index, field.clone(), window, cx))).into_any_element()
            } else if flag {
                let field = name.clone();
                let yes = f.value.as_str() == Some("y");
                h_flex()
                    .gap_2()
                    .child(Switch::new(("rflag", k)).xsmall().checked(yes).on_click(cx.listener(move |this, _: &bool, _, cx| this.toggle_flag(index, field.clone(), cx))))
                    .child(div().text_color(muted).child(if yes { "Yes" } else { "No" }))
                    .into_any_element()
            } else if editing {
                div().key_context("AsmEdit").flex_1().min_w_0().child(if long { Textarea::new(&self.edit_area).into_any_element() } else { Input::new(&self.edit_input).xsmall().into_any_element() }).into_any_element()
            } else {
                let (field, draft) = (name.clone(), text.clone());
                let (key_field, key_draft) = (name.clone(), text.clone());
                let tip = if blob { "binary" } else { "click to edit" };
                div()
                    .id(("rf", k))
                    .flex_1()
                    .min_w_0()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(|s| s.border_color(theme.ring))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .font_family(mono.clone())
                    .hover(|s| s.bg(theme.list_hover))
                    .when(blob, |d| d.text_color(muted))
                    .when(!blob, |d| d.focusable().tab_index(0).cursor_text())
                    // An empty value keeps a line to click on.
                    .min_h(px(16.0))
                    .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip).build(window, cx))
                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                        if !blob && (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.start_edit(Edit::Field { row: index, field: key_field.clone(), long }, key_draft.clone(), key_draft.clone(), !long, window, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if !blob {
                            this.start_edit(Edit::Field { row: index, field: field.clone(), long }, draft.clone(), draft.clone(), !long, window, cx);
                        }
                    }))
                    .child(text.clone())
                    .into_any_element()
            };
            let mut value_row = h_flex().gap_1().items_start().min_w_0().child(value_el);
            if let (Some((table, _)), false) = (link, editing) {
                let (field, value) = (name.clone(), f.value.clone());
                value_row = value_row.child(Button::new(("rgo", k)).xsmall().ghost().icon(Icon::new(IconName::ExternalLink)).tooltip(format!("Go to {}", short(table))).on_click(cx.listener(move |this, e: &ClickEvent, window, cx| this.follow(&field, &value, e.modifiers().secondary(cx), window, cx))));
            }
            if is_value_id(&name) && !editing && !is_set {
                let field = name.clone();
                value_row = value_row.child(Button::new(("rpick", k)).xsmall().ghost().icon(Icon::new(IconName::ListTree)).tooltip("Pick one of the valid values").on_click(cx.listener(move |this, _, _, cx| this.pick_value(index, field.clone(), cx))));
            }
            let mut value_col = v_flex().flex_1().min_w_0().gap_0p5().child(value_row);
            // What a valid value id means.
            if let Some(t) = f.value.as_i64().filter(|_| is_value_id(&name)).and_then(|id| self.valid.get(&id)) {
                value_col = value_col.child(div().px_1().text_color(muted).child(t.clone()));
            }
            let line = h_flex().gap_2().items_start().child(label).child(value_col);
            let mut block = v_flex().py_1().border_b_1().border_color(theme.border.opacity(0.6)).child(line);
            // The message in every language.
            if let Some(id) = f.value.as_i64().filter(|id| is_message(&name) && *id != 0) {
                let mut langs = v_flex().mt_1().ml_2().pl_2().gap_0p5().border_l_1().border_color(theme.border);
                let known = self.messages.get(&id);
                for (j, lang) in languages.iter().enumerate() {
                    let m = known.and_then(|m| m.get(lang));
                    let editing = matches!(&self.edit, Some(Edit::Message { id: i, lang: l }) if *i == id && l == lang);
                    let lang_tip = match super::side::friendly_lang(lang) {
                        Some(friendly) => format!("{friendly} (Lang_{lang}.dat)"),
                        None => format!("Lang_{lang}.dat"),
                    };
                    let mut line = h_flex().gap_2().items_start().child(
                        div()
                            .id(SharedString::from(format!("msgl-{k}-{j}")))
                            .w(px(32.0))
                            .flex_none()
                            .font_family(mono.clone())
                            .text_color(muted)
                            .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(lang_tip.clone()).build(window, cx))
                            .child(lang.clone()),
                    );
                    if editing {
                        line = line.child(div().key_context("AsmEdit").flex_1().min_w_0().child(Textarea::new(&self.edit_area)));
                    } else {
                        let (lang2, draft) = (lang.clone(), m.map(|m| m.text.clone()).unwrap_or_default());
                        let (key_lang, key_draft) = (lang2.clone(), draft.clone());
                        let tip = match m {
                            Some(m) if !m.name.is_empty() => format!("{} (click to edit)", m.name),
                            Some(_) => "click to edit".to_string(),
                            None => "no message in this language (click to add)".to_string(),
                        };
                        line = line.child(
                            div()
                                .id(SharedString::from(format!("msg-{k}-{j}")))
                                .focusable()
                                .tab_index(0)
                                .flex_1()
                                .min_w_0()
                                .px_1()
                                .rounded_sm()
                                .border_1()
                                .border_color(gpui::transparent_black())
                                .focus_visible(|s| s.border_color(theme.ring))
                                .cursor_text()
                                .whitespace_normal()
                                .hover(|s| s.bg(theme.list_hover))
                                .when(m.is_none(), |d| d.italic().text_color(muted))
                                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                    if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        this.start_edit(Edit::Message { id, lang: key_lang.clone() }, key_draft.clone(), key_draft.clone(), false, window, cx);
                                    }
                                }))
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
            body = body.child(div().mt_2().text_color(muted).child("Messages are looked up by MSG_ID from the game data's *_MSG_ID columns; keep ids unique."));
        }
        v_flex().size_full().child(title).child(div().id("arow-pane").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.row_scroll).child(body)).child(crate::driver::probe("asm-row")).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::pretty_column;

    #[test]
    fn humanizes_screaming_snake_columns() {
        assert_eq!(pretty_column("HIBERNATE_INVULNERABILITY_FLAG"), "Hibernate invulnerability");
        assert_eq!(pretty_column("BOT_ID"), "Bot ID");
        assert_eq!(pretty_column("REFERENCE_NAME"), "Reference name");
        assert_eq!(pretty_column("NAME_MSG_ID"), "Name message ID");
        assert_eq!(pretty_column("UI_HUD_FX_ID"), "UI HUD FX ID");
        assert_eq!(pretty_column("DATA_SET_BOT_DEVICES"), "Bot devices");
        assert_eq!(pretty_column("FLAG"), "Flag");
    }
}
