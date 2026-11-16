//! The Assembly view's side pane: the files (`assembly.dat` and each
//! `Lang_<EXT>.dat`), their header fields (edited in place; the game version
//! as major.minor.build.revision) and their tables, filtered by name.

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, MouseButton, MouseDownEvent, SharedString, Window};
use gpui_component::input::Input;
use gpui_component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;

use super::{header_text, short, Assembly, Edit};
use crate::studio::Place;

impl Assembly {
    /// The tables the list shows, in order: (file, table).
    fn listed_tables(&self, cx: &Context<Self>) -> Vec<(String, String)> {
        let f = self.table_filter.read(cx).value().trim().to_ascii_lowercase();
        self.docs.iter().flat_map(|d| d.tables.iter().filter(|t| f.is_empty() || t.name.to_ascii_lowercase().contains(&f)).map(|t| (d.id.clone(), t.name.clone()))).collect::<Vec<_>>()
    }

    /// ↑ ↓ in the table list: open the next table.
    pub(super) fn step_table(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let all = self.listed_tables(cx);
        if all.is_empty() {
            return;
        }
        let at = if self.path.is_empty() { all.iter().position(|(d, t)| Some(d) == self.doc.as_ref() && Some(t) == self.table.as_ref()) } else { None };
        let next = match at {
            Some(i) => (i as isize + by).clamp(0, all.len() as isize - 1) as usize,
            None => 0,
        };
        if at == Some(next) {
            return;
        }
        let (doc, table) = all[next].clone();
        self.open_table(doc, table, window, cx);
        self.tables_focus.focus(window, cx);
    }

    fn table_menu(&mut self, doc: String, table: String, path: String, menu: PopupMenu, cx: &mut Context<Self>) -> PopupMenu {
        let me = cx.entity().downgrade();
        type Act = dyn Fn(&mut Assembly, &mut Window, &mut Context<Assembly>);
        let item = |label: &str, f: Box<Act>| {
            let me = me.clone();
            let f: std::rc::Rc<Act> = f.into();
            PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                let f = f.clone();
                let _ = me.update(cx, |this, cx| f(this, window, cx));
            })
        };
        let (d1, t1, d2, t2, t3) = (doc.clone(), table.clone(), doc, table.clone(), table.clone());
        menu.label(table)
            .item(item("Open", Box::new(move |a, w, cx| a.open_table(d1.clone(), t1.clone(), w, cx))))
            .item(item("Open in a new tab", Box::new(move |a, _, cx| a.open_in_tab(Place::Assembly { file: d2.clone(), path: Vec::new(), table: Some(t2.clone()), row: None }, cx))))
            .separator()
            .item(item("Copy the table name", Box::new(move |a, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(t3.clone()));
                a.set_status(format!("Copied {t3}"), cx);
            })))
            .item(item(if cfg!(target_os = "macos") { "Show the file in Finder" } else { "Show the file in the file manager" }, Box::new(move |_, _, cx| cx.reveal_path(std::path::Path::new(&path)))))
    }

    pub(super) fn render_side(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let f = self.table_filter.read(cx).value().trim().to_ascii_lowercase();
        let me = cx.entity();
        let mut list = v_flex();
        for d in &self.docs {
            let tip = d.path.clone();
            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("doc-{}", d.id)))
                    .px_2()
                    .py_1()
                    .gap_1p5()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .bg(theme.secondary.opacity(0.6))
                    .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                    .child(Icon::new(if d.id == "assembly" { IconName::Database } else { IconName::FileText }).size_3().text_color(muted))
                    .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(d.label()))
                    .when(d.dirty, |e| e.child(div().size(px(6.0)).flex_none().rounded_full().bg(theme.warning)))
                    .child(div().flex_1())
                    .child(div().flex_none().font_family(mono.clone()).font_weight(gpui::FontWeight::NORMAL).text_color(muted.opacity(0.7)).child(d.function.clone())),
            );
            // The header fields (hidden while the tables are filtered).
            if f.is_empty() {
                for (k, (field, value)) in d.header.iter().enumerate() {
                    let shown = header_text(field, value);
                    let editing = matches!(&self.edit, Some(Edit::Header { doc, field: fl }) if doc == &d.id && fl == field);
                    let (doc, field2, draft) = (d.id.clone(), field.clone(), shown.clone());
                    let name_tip = field.clone();
                    let tip = if field == "GAME_VERSION_RAW" { format!("{} (click to edit as major.minor.build.rev)", super::cell_text(value)) } else { format!("{shown} (click to edit)") };
                    list = list.child(
                        h_flex()
                            .gap_2()
                            .px_2()
                            .py_0p5()
                            .text_xs()
                            .font_family(mono.clone())
                            .child(
                                div()
                                    .id(SharedString::from(format!("hdrn-{}-{k}", d.id)))
                                    .w(px(124.0))
                                    .flex_none()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_color(muted)
                                    .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(name_tip.clone()).build(window, cx))
                                    .child(field.clone()),
                            )
                            .child(if editing {
                                div().key_context("AsmEdit").flex_1().min_w_0().child(Input::new(&self.edit_input).xsmall()).into_any_element()
                            } else {
                                div()
                                    .id(SharedString::from(format!("hdr-{}-{k}", d.id)))
                                    .flex_1()
                                    .min_w_0()
                                    .min_h(px(16.0))
                                    .px_1()
                                    .rounded_sm()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .cursor_text()
                                    .hover(|s| s.bg(theme.list_hover))
                                    .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                                    .on_click(cx.listener(move |this, _, window, cx| this.start_edit(Edit::Header { doc: doc.clone(), field: field2.clone() }, draft.clone(), draft.clone(), true, window, cx)))
                                    .child(shown)
                                    .into_any_element()
                            }),
                    );
                }
            }
            for (k, t) in d.tables.iter().enumerate() {
                if !f.is_empty() && !t.name.to_ascii_lowercase().contains(&f) {
                    continue;
                }
                let active = self.doc.as_deref() == Some(d.id.as_str()) && self.table.as_deref() == Some(t.name.as_str()) && self.path.is_empty();
                let (doc, table) = (d.id.clone(), t.name.clone());
                let (doc2, table2) = (d.id.clone(), t.name.clone());
                let (doc3, table3, path3) = (d.id.clone(), t.name.clone(), d.path.clone());
                let probe = format!("table-{}", t.name.to_ascii_lowercase());
                let tip = format!("{} columns", t.columns);
                let me = me.clone();
                list = list.child(
                    h_flex()
                        .id(SharedString::from(format!("t-{}-{k}", d.id)))
                        .relative()
                        .px_3()
                        .py_0p5()
                        .gap_2()
                        .text_xs()
                        .cursor_pointer()
                        .when(active, |e| e.bg(theme.list_active))
                        .when(!active, |e| e.hover(|s| s.bg(theme.list_hover)))
                        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                        .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                            if e.modifiers().secondary(cx) {
                                this.open_in_tab(Place::Assembly { file: doc.clone(), path: Vec::new(), table: Some(table.clone()), row: None }, cx);
                            } else {
                                this.open_table(doc.clone(), table.clone(), window, cx);
                                this.tables_focus.focus(window, cx);
                            }
                        }))
                        .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _: &MouseDownEvent, _, cx| this.open_in_tab(Place::Assembly { file: doc2.clone(), path: Vec::new(), table: Some(table2.clone()), row: None }, cx)))
                        .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family(mono.clone()).child(short(&t.name).to_string()))
                        .child(div().flex_none().font_family(mono.clone()).text_color(muted.opacity(0.7)).child(t.rows.to_string()))
                        .child(crate::driver::probe(probe))
                        .context_menu(move |menu, _, cx| me.update(cx, |this, cx| this.table_menu(doc3.clone(), table3.clone(), path3.clone(), menu, cx))),
                );
            }
        }
        v_flex()
            .size_full()
            .child(
                crate::studio::ui::pane_header("Tables", div().text_xs().font_weight(gpui::FontWeight::NORMAL).child(format!("{} files", self.docs.len())), &theme),
            )
            .child(div().p_1().border_b_1().border_color(theme.border).child(Input::new(&self.table_filter).small().cleanable(true)))
            .child(div().id("tables").track_focus(&self.tables_focus).flex_1().min_h_0().overflow_y_scroll().child(list))
            .into_any_element()
    }
}
