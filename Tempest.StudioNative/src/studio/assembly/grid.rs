//! The Assembly view's grid: the toolbar, the header and the rows of the set
//! on screen. Columns are as wide as their widest cell read so far (the
//! pinned id and name columns have fixed widths and stay at the left); only
//! the rows and the columns in view are laid out. The cell cursor is the
//! selected row and `Assembly::col`; the keyboard moves it, edits its cell
//! and marks rows.

use std::ops::Range;

use gpui::{div, prelude::*, px, uniform_list, AnyElement, ClickEvent, Context, KeyDownEvent, MouseButton, MouseDownEvent, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::Value as J;

use super::{cell_text, is_flag, is_message, is_value_id, ref_of, short, Assembly, Column, Edit, Move, PAGE};
use crate::studio::commands::key_hint;

/// A row's height, in rems (it follows the text size).
const ROW_REMS: f32 = 1.5;
/// The row number column, the pinned id and name columns, and the widest a column grows to, in rems.
const INDEX_REMS: f32 = 3.5;
const PIN_REMS: [f32; 2] = [7.0, 13.0];
const MAX_REMS: f32 = 28.0;
/// A cell being edited is at least this wide.
const EDIT_REMS: f32 = 14.0;
/// Columns laid out beyond each side of the view.
const COL_OVERSCAN: usize = 2;

/// A column of the grid with where it sits (unpinned columns from the pinned ones' right edge).
#[derive(Clone)]
pub(super) struct Laid {
    pub col: Column,
    pub x: f32,
    pub w: f32,
    pub pinned: bool,
}

/// The grid's columns for one frame.
pub(super) struct Layout {
    pub cols: Vec<Laid>,
    pub index_w: f32,
    /// The row number and pinned columns together.
    pub pin_w: f32,
    pub width: f32,
    pub row_h: f32,
}

impl Layout {
    /// The unpinned columns to lay out for a view scrolled `off` sideways and `view` wide, and the space before and after them.
    fn window(&self, off: f32, view: f32) -> (Range<usize>, f32, f32) {
        let first_flow = self.cols.iter().position(|c| !c.pinned).unwrap_or(self.cols.len());
        let flow = &self.cols[first_flow..];
        if flow.is_empty() {
            return (first_flow..first_flow, 0.0, 0.0);
        }
        let (lo, hi) = (off + self.pin_w, off + view);
        let mut first = flow.iter().position(|c| c.x + c.w >= lo).unwrap_or(flow.len());
        let mut last = first;
        while last < flow.len() && flow[last].x <= hi {
            last += 1;
        }
        first = first.saturating_sub(COL_OVERSCAN);
        last = (last + COL_OVERSCAN).min(flow.len());
        let start = flow.get(first).map(|c| c.x).unwrap_or(self.width);
        let end = if last > first { flow[last - 1].x + flow[last - 1].w } else { start };
        (first_flow + first..first_flow + last, start - self.pin_w, (self.width - end).max(0.0))
    }
}

impl Assembly {
    /// The widest cell of each column in characters, over the rows read so far (the hints and link icons included).
    fn measure(&mut self) {
        if !self.chars_stale {
            return;
        }
        self.chars_stale = false;
        let cols = self.visible_columns();
        let mut out = std::collections::HashMap::new();
        for c in &cols {
            let (msg, value_id, flag, link) = (is_message(&c.name), is_value_id(&c.name), is_flag(&c.name), ref_of(&c.name).is_some());
            let mut w = 0usize;
            for (_, cells) in self.rows.values() {
                let Some(v) = cells.get(&c.name) else { continue };
                let n = match v {
                    J::Null => 0,
                    J::Object(o) if o.contains_key("rows") => o["rows"].to_string().len() + 5,
                    J::String(s) if flag && (s == "y" || s == "n") => 4,
                    v => {
                        let mut n = match v {
                            J::String(s) => s.chars().count(),
                            other => other.to_string().len(),
                        };
                        if msg || value_id {
                            if let Some(h) = self.hint(&c.name, v) {
                                n += h.chars().count() + 1;
                            }
                        }
                        if link && v.is_number() {
                            n += 3;
                        }
                        n
                    }
                };
                w = w.max(n);
            }
            out.insert(c.name.clone(), w);
        }
        self.chars = out;
    }

    /// Lay the columns out for this frame's text size.
    pub(super) fn layout(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Layout {
        self.measure();
        let rem = f32::from(window.rem_size());
        let font_size = px(rem * 0.75);
        let font = gpui::font(cx.theme().mono_font_family.clone());
        let ch = {
            let ts = window.text_system();
            let id = ts.resolve_font(&font);
            ts.ch_advance(id, font_size).map(f32::from).unwrap_or(rem * 0.45)
        };
        let pinned = self.pinned();
        let editing = match &self.edit {
            Some(Edit::Cell { field, .. }) => Some(field.clone()),
            _ => None,
        };
        let index_w = INDEX_REMS * rem;
        let mut x = index_w;
        let mut cols = Vec::new();
        for (k, c) in self.visible_columns().into_iter().enumerate() {
            let is_pinned = k < pinned.len();
            let mut w = if is_pinned {
                PIN_REMS.get(k).copied().unwrap_or(8.0) * rem
            } else {
                let sorted = self.sort.as_ref().is_some_and(|(s, _)| s == &c.name);
                let chars = self.chars.get(&c.name).copied().unwrap_or(0).max(c.name.chars().count() + if sorted { 2 } else { 0 });
                (chars as f32 * ch + rem).min(MAX_REMS * rem)
            };
            if editing.as_deref() == Some(c.name.as_str()) {
                w = w.max(EDIT_REMS * rem);
            }
            cols.push(Laid { col: c, x, w: w.ceil(), pinned: is_pinned });
            x += w.ceil();
        }
        let pin_w = cols.iter().filter(|c| c.pinned).map(|c| c.w).sum::<f32>() + index_w;
        Layout { cols, index_w, pin_w, width: x, row_h: (ROW_REMS * rem).round() }
    }

    /// Column widths as last measured (the driver's `assembly dump`).
    pub(super) fn last_widths(&self) -> Vec<J> {
        self.visible_columns().iter().map(|c| serde_json::json!([c.name, self.chars.get(&c.name)])).collect()
    }

    /// How far the grid is scrolled sideways (the pinned columns move back by as much).
    fn hoff(&self) -> f32 {
        (-f32::from(self.hscroll.offset().x)).max(0.0)
    }

    fn view_width(&self) -> f32 {
        let w = f32::from(self.hscroll.bounds().size.width);
        if w > 0.0 {
            w
        } else {
            1600.0
        }
    }

    /// Rows that fit in the grid (a screenful, for Page Up / Page Down).
    fn rows_in_view(&self, window: &Window) -> usize {
        let h = f32::from(self.scroll.0.borrow().base_handle.bounds().size.height);
        let row = (ROW_REMS * f32::from(window.rem_size())).round().max(1.0);
        ((h / row).floor() as usize).max(2)
    }

    // ----- the cell cursor --------------------------------------------------------------

    /// Move the cursor `step` columns, scrolling it into view.
    pub(super) fn move_col(&mut self, step: isize, cx: &mut Context<Self>) {
        let cols = self.visible_columns();
        if cols.is_empty() {
            return;
        }
        let at = self.col.as_ref().and_then(|c| cols.iter().position(|x| &x.name == c)).unwrap_or(0) as isize;
        let next = (at + step).clamp(0, cols.len() as isize - 1) as usize;
        self.col = Some(cols[next].name.clone());
        self.reveal_col = true;
        cx.notify();
    }

    /// Scroll sideways so the cursor's column is in view (the pinned columns cover the left edge).
    fn reveal_column(&mut self, layout: &Layout) {
        let Some(c) = self.col.as_ref().and_then(|c| layout.cols.iter().find(|l| &l.col.name == c)) else { return };
        if c.pinned {
            return;
        }
        let (off, view) = (self.hoff(), self.view_width());
        let target = if c.x < off + layout.pin_w {
            c.x - layout.pin_w
        } else if c.x + c.w > off + view {
            c.x + c.w - view
        } else {
            return;
        };
        let y = self.hscroll.offset().y;
        self.hscroll.set_offset(gpui::point(px(-target.max(0.0)), y));
    }

    fn grid_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    /// ↑ ↓ (with ⇧: marking the rows passed). In the table list: the next table.
    pub(super) fn key_step(&mut self, by: isize, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.tables_focus.is_focused(window) {
            self.step_table(by, window, cx);
            return;
        }
        if !self.grid_focused(window) {
            cx.propagate();
            return;
        }
        let at = self.selected.and_then(|s| self.positions.get(&s).copied());
        let next = match at {
            Some(p) => (p as isize + by).clamp(0, self.total.max(1) as isize - 1) as usize,
            None => 0,
        };
        if extend {
            if let (Some(s), Some((index, _))) = (self.selected, self.rows.get(&next)) {
                if self.marked.is_empty() {
                    self.marked.insert(s);
                }
                self.marked.insert(*index);
            }
        } else {
            self.marked.clear();
        }
        if at != Some(next) {
            self.select_pos(next, cx);
        }
        cx.notify();
    }

    pub(super) fn key_col(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_focused(window) {
            cx.propagate();
            return;
        }
        self.move_col(by, cx);
    }

    /// Tab: the next cell; in a cell's editor: applied first.
    pub(super) fn key_tab(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.edit, Some(Edit::Cell { .. })) {
            self.commit_edit(if by > 0 { Move::Right } else { Move::Left }, window, cx);
        } else if self.grid_focused(window) {
            self.move_col(by, cx);
        } else {
            cx.propagate();
        }
    }

    pub(super) fn key_end(&mut self, last: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_focused(window) {
            cx.propagate();
            return;
        }
        self.marked.clear();
        self.select_pos(if last { self.total.saturating_sub(1) } else { 0 }, cx);
    }

    pub(super) fn key_page(&mut self, dir: isize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_focused(window) {
            cx.propagate();
            return;
        }
        let n = self.rows_in_view(window).saturating_sub(1).max(1) as isize;
        let at = self.selected.and_then(|s| self.positions.get(&s).copied()).unwrap_or(0) as isize;
        self.marked.clear();
        self.select_pos((at + dir * n).clamp(0, self.total.max(1) as isize - 1) as usize, cx);
    }

    /// Enter / F2: open the cell's row set, else edit it.
    pub(super) fn key_activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Enter in a cell's editor, ⌘Enter in any editor: applied (a cell's cursor goes down).
        if self.edit.is_some() {
            self.commit_edit(Move::Down, window, cx);
            return;
        }
        if !self.grid_focused(window) {
            cx.propagate();
            return;
        }
        let (Some(index), Some(col)) = (self.selected, self.col.clone()) else { return };
        if self.cell(index, &col).is_some_and(|v| v.get("rows").is_some()) {
            self.open_set(index, col, window, cx);
        } else {
            self.edit_cell(index, col, None, window, cx);
        }
    }

    /// Escape: leave the editor without applying; in the grid: drop the marks.
    pub(super) fn key_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit.is_some() {
            self.cancel_edit(cx);
            self.focus.focus(window, cx);
        } else if self.grid_focused(window) && !self.marked.is_empty() {
            self.marked.clear();
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    pub(super) fn key_select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_focused(window) {
            cx.propagate();
            return;
        }
        // Every row read so far (a table is read as it is scrolled).
        self.marked = self.rows.values().map(|r| r.0).collect();
        if self.rows.len() < self.total {
            self.set_status(format!("{} of {} rows marked: the rest are not read yet (scroll to read them)", self.rows.len(), self.total), cx);
        }
        cx.notify();
    }

    pub(super) fn key_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Only from the grid: the key must not delete rows from the table list.
        if self.grid_focused(window) {
            self.delete_rows(window, cx);
        }
    }

    pub(super) fn key_copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_focused(window) {
            cx.propagate();
            return;
        }
        self.copy_rows(cx);
    }

    pub(super) fn key_paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_focused(window) {
            cx.propagate();
            return;
        }
        self.paste(window, cx);
    }

    /// Typing starts editing the active cell with that character; Space flips a flag.
    pub(super) fn key_typed(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_focused(window) || self.edit.is_some() {
            return;
        }
        let m = e.keystroke.modifiers;
        if m.platform || m.control || m.alt || m.function {
            return;
        }
        let space = (e.keystroke.key == "space").then(|| " ".to_string());
        let Some(typed) = e.keystroke.key_char.clone().or(space).filter(|c| c.chars().count() == 1 && !c.chars().any(char::is_control)) else { return };
        let (Some(index), Some(col)) = (self.selected, self.col.clone()) else { return };
        if is_flag(&col) && matches!(self.cell(index, &col).and_then(|v| v.as_str()), Some("y" | "n")) {
            if typed == " " {
                self.toggle_flag(index, col, cx);
                cx.stop_propagation();
            }
            return;
        }
        cx.stop_propagation();
        self.edit_cell(index, col, Some(typed), window, cx);
    }

    // ----- clicks ---------------------------------------------------------------------------

    /// A click on a row: select it; ⌘ marks or unmarks, ⇧ marks from the last plain click.
    fn row_click(&mut self, index: usize, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        let m = e.modifiers();
        if m.shift && self.anchor.is_some() {
            let at = |i: usize| self.positions.get(&i).copied();
            if let (Some(a), Some(b)) = (self.anchor.and_then(at), at(index)) {
                self.marked = (a.min(b)..=a.max(b)).filter_map(|p| self.rows.get(&p).map(|r| r.0)).collect();
            }
        } else if m.secondary(cx) {
            // The selection counts as marked once a second row is.
            if self.marked.is_empty() {
                if let Some(s) = self.selected {
                    self.marked.insert(s);
                }
            }
            if !self.marked.remove(&index) {
                self.marked.insert(index);
            }
            self.anchor = Some(index);
        } else {
            self.marked.clear();
            self.anchor = Some(index);
        }
        self.select(index, cx);
        self.record(cx);
    }

    /// The row menu (right-click): what can be done with the row or the marked rows.
    fn row_menu(&mut self, index: usize, menu: PopupMenu, cx: &mut Context<Self>) -> PopupMenu {
        // A right-click outside the marked rows selects its row.
        if !self.marked.contains(&index) && self.selected != Some(index) {
            self.marked.clear();
            self.anchor = Some(index);
            self.select(index, cx);
            self.record(cx);
        }
        let many = self.marked.contains(&index) && self.marked.len() > 1;
        let n = self.marked.len();
        let me = cx.entity().downgrade();
        type Act = dyn Fn(&mut Assembly, &mut Window, &mut Context<Assembly>);
        let item = |label: String, f: Box<Act>| {
            let me = me.clone();
            let f: std::rc::Rc<Act> = f.into();
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                let f = f.clone();
                let _ = me.update(cx, |this, cx| f(this, window, cx));
            })
        };
        let column = self.col.as_ref().and_then(|c| self.columns.iter().find(|x| &x.name == c)).cloned();
        let value = column.as_ref().and_then(|c| self.cell(index, &c.name)).cloned().unwrap_or(J::Null);
        let mut menu = menu.label(if many { format!("{n} marked rows") } else { format!("Row #{index}") });
        if let Some(c) = &column {
            let name = c.name.clone();
            if c.editable() && value.get("rows").is_none() {
                let field = name.clone();
                menu = menu.item(item(format!("Edit {name}    F2"), Box::new(move |a, w, cx| a.edit_cell(index, field.clone(), None, w, cx))));
            }
            if is_value_id(&name) && c.editable() {
                let field = name.clone();
                menu = menu.item(item("Pick a value…".into(), Box::new(move |a, _, cx| a.pick_value(index, field.clone(), cx))));
            }
            let text = cell_text(&value);
            let shown = if text.is_empty() { "(empty)".to_string() } else { text.chars().take(40).collect() };
            menu = menu.item(item(format!("Copy the value: {shown}"), Box::new(move |a, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                a.set_status("Copied the value", cx);
            })));
            if ref_of(&name).is_some() && value.is_number() {
                let (field, v) = (name.clone(), value.clone());
                menu = menu.item(item(format!("Go to the {} row", short(ref_of(&name).map(|r| r.0).unwrap_or_default())), Box::new(move |a, w, cx| a.follow(&field, &v, false, w, cx))));
            }
        }
        menu.separator()
            .item(item(format!("{}    {}", if many { "Copy the rows" } else { "Copy the row" }, key_hint("⌘C")), Box::new(|a, _, cx| a.copy_rows(cx))))
            .item(item("Copy as tab-separated text".into(), Box::new(|a, _, cx| a.copy_tsv(cx))))
            .item(item(format!("Paste    {}", key_hint("⌘V")), Box::new(|a, w, cx| a.paste(w, cx))))
            .separator()
            .item(item(format!("Duplicate    {}", key_hint("⌘D")), Box::new(move |a, _, cx| a.add_row(Some(index), cx))).disabled(many))
            .item(item(format!("New row    {}", key_hint("⌘⇧↩")), Box::new(|a, w, cx| a.new_row(w, cx))))
            .item(item("Open in a new tab".into(), Box::new(move |a, _, cx| a.open_in_tab(a.place_of(index), cx))))
            .separator()
            .item(item(if many { format!("Delete {n} rows…") } else { "Delete the row…".into() }, Box::new(|a, w, cx| a.delete_rows(w, cx))))
    }

    /// Choose a `*_VALUE_ID` cell's value from the valid values (the palette).
    pub(super) fn pick_value(&mut self, index: usize, field: String, cx: &mut Context<Self>) {
        let mut values: Vec<(i64, String)> = self.valid.iter().map(|(id, t)| (*id, t.clone())).collect();
        values.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.cmp(&b.0)));
        let pick = crate::studio::palette::Pick::AssemblyValue { row: index, field, values: std::rc::Rc::new(values) };
        let studio = self.studio.clone();
        cx.defer(move |cx| {
            let Some(window) = cx.active_window() else { return };
            let _ = window.update(cx, |_, window, cx| {
                let _ = studio.update(cx, |s, cx| s.open_picker(pick, window, cx));
            });
        });
    }

    /// The palette's answer: the value goes into the cell (of every marked row).
    pub(in crate::studio) fn pick_value_done(&mut self, index: usize, field: String, id: i64, cx: &mut Context<Self>) {
        self.set_cell(index, field, id.to_string(), cx);
    }

    // ----- rendering -------------------------------------------------------------------------

    fn render_rows(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        // Pages the rows in view need.
        for pos in [range.start, range.end.saturating_sub(1)] {
            if pos < self.total && !self.rows.contains_key(&pos) {
                self.load_page(pos / PAGE, cx);
            }
        }
        let layout = self.layout(window, cx);
        let off = self.hoff();
        let (flow, before, after) = layout.window(off, self.view_width());
        let n_pin = layout.cols.iter().filter(|c| c.pinned).count();
        let mono = theme.mono_font_family.clone();
        let me = cx.entity();
        range
            .map(|pos| {
                let Some((index, cells)) = self.rows.get(&pos).cloned() else {
                    return h_flex().h(px(layout.row_h)).w(px(layout.width)).px_2().text_xs().text_color(theme.muted_foreground).child("…").into_any_element();
                };
                let selected = self.selected == Some(index);
                let marked = self.marked.contains(&index);
                let tint = |d: gpui::Div| if selected { d.bg(theme.list_active) } else if marked { d.bg(theme.primary.opacity(0.12)) } else { d };
                // The pinned cells sit over the others, opaque, with the row's highlight.
                let mut pins = tint(h_flex().h_full().when(!selected && !marked, |d| d.group_hover("arow", |s| s.bg(theme.list_hover))))
                    .child(div().w(px(layout.index_w)).flex_none().px_2().text_right().font_family(mono.clone()).text_xs().text_color(if marked { theme.primary } else { theme.muted_foreground }).child(index.to_string()));
                for l in &layout.cols[..n_pin] {
                    pins = pins.child(self.render_cell(pos, index, &cells, l, selected, cx));
                }
                let mut row = tint(h_flex().h(px(layout.row_h)).w(px(layout.width)))
                    .id(("arow", pos))
                    .group("arow")
                    .relative()
                    .border_b_1()
                    .border_color(theme.border.opacity(0.4))
                    .when(!selected && !marked, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| this.row_click(index, e, window, cx)))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| this.open_in_tab(this.place_of(index), cx)),
                    )
                    .child(div().w(px(layout.pin_w + before)).flex_none());
                for l in &layout.cols[flow.clone()] {
                    row = row.child(self.render_cell(pos, index, &cells, l, selected, cx));
                }
                let row = row.child(div().w(px(after)).flex_none()).child(div().absolute().top_0().left(px(off)).h_full().bg(theme.background).border_r_1().border_color(theme.border).child(pins));
                let me = me.clone();
                row.context_menu(move |menu, _, cx| me.update(cx, |this, cx| this.row_menu(index, menu, cx))).into_any_element()
            })
            .collect()
    }

    /// One cell: its editor, a switch for a flag, a link for a row set or an id, else its text and what it means.
    fn render_cell(&self, pos: usize, index: usize, cells: &serde_json::Map<String, J>, l: &Laid, selected: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let name = l.col.name.clone();
        let present = cells.get(&name);
        let v = present.cloned().unwrap_or(J::Null);
        let editing = matches!(&self.edit, Some(Edit::Cell { row, field }) if *row == index && field == &name);
        let cursor = selected && self.col.as_deref() == Some(name.as_str());
        let is_set = v.get("rows").is_some();
        let flag = is_flag(&name) && matches!(v.as_str(), Some("y" | "n"));
        let link = ref_of(&name).filter(|_| v.is_number());
        let hint = self.hint(&name, &v).map(str::to_string);
        let muted = theme.muted_foreground;
        let content: AnyElement = if editing {
            div().key_context("AsmEdit").w_full().child(Input::new(&self.edit_input).xsmall()).into_any_element()
        } else if is_set {
            let field = name.clone();
            div()
                .id(SharedString::from(format!("set{pos}-{name}")))
                .text_color(theme.link)
                .cursor_pointer()
                .hover(|s| s.underline())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_set(index, field.clone(), window, cx)
                }))
                .child(cell_text(&v))
                .into_any_element()
        } else if flag {
            let field = name.clone();
            Switch::new(SharedString::from(format!("flag{pos}-{name}")))
                .xsmall()
                .checked(v.as_str() == Some("y"))
                .on_click(cx.listener(move |this, _: &bool, window, cx| {
                    this.focus.focus(window, cx);
                    this.col = Some(field.clone());
                    // A flag of a marked row is set on all of them, like any cell.
                    this.toggle_flag(index, field.clone(), cx)
                }))
                .into_any_element()
        } else {
            let mut line = h_flex().gap_1().min_w_0().child(div().flex_none().child(cell_text(&v)));
            if let Some((table, _)) = link {
                let (field, value) = (name.clone(), v.clone());
                let tip = format!("Go to {} ({}-click: new tab)", short(table), if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" });
                line = line.child(
                    div()
                        .id(SharedString::from(format!("go{pos}-{name}")))
                        .flex_none()
                        .cursor_pointer()
                        .text_color(muted.opacity(0.6))
                        .hover(|s| s.text_color(theme.link))
                        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                        .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.follow(&field, &value, e.modifiers().secondary(cx), window, cx)
                        }))
                        .child(Icon::new(IconName::ExternalLink).size_3()),
                );
            }
            if let Some(h) = &hint {
                line = line.child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(muted).child(h.clone()));
            }
            line.into_any_element()
        };
        let field = name.clone();
        let tip = hint.or_else(|| present.is_none().then(|| format!("no {name} in this row")));
        div()
            .id(SharedString::from(format!("c{pos}-{name}")))
            .relative()
            .w(px(l.w))
            .h_full()
            .flex()
            .items_center()
            .flex_none()
            .px_2()
            .overflow_hidden()
            .whitespace_nowrap()
            // The font is the cells' own: the row's menu keeps the window's.
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .border_r_1()
            .border_color(theme.border.opacity(0.4))
            .when(present.is_none(), |d| d.opacity(0.3))
            .when_some(tip.filter(|_| !editing), |d, tip| d.tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)))
            // The cursor follows the cell clicked (either button), before the row's own handlers.
            .on_mouse_down(MouseButton::Left, cx.listener({
                let field = field.clone();
                move |this, _: &MouseDownEvent, _, cx| {
                    this.col = Some(field.clone());
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Right, cx.listener({
                let field = field.clone();
                move |this, _: &MouseDownEvent, _, _| this.col = Some(field.clone())
            }))
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                if e.click_count() == 2 && !editing {
                    cx.stop_propagation();
                    if is_set {
                        this.open_set(index, field.clone(), window, cx);
                    } else {
                        this.edit_cell(index, field.clone(), None, window, cx);
                    }
                }
            }))
            .child(content)
            .when(cursor && !editing, |d| d.child(div().absolute().inset_0().border_2().border_color(theme.primary)))
            .into_any_element()
    }

    pub(super) fn render_grid(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(table) = self.table.clone() else {
            return v_flex().size_full().items_center().justify_center().text_sm().text_color(theme.muted_foreground).child("Pick a table.").into_any_element();
        };
        let layout = self.layout(window, cx);
        if std::mem::take(&mut self.reveal_col) {
            self.reveal_column(&layout);
        }
        let doc_dirty = self.doc_dirty();
        let undo = self.current_doc().and_then(|d| d.undo.first().cloned());
        let in_assembly = self.doc.as_deref() == Some("assembly");
        let depth = self.path.len();
        let mono = theme.mono_font_family.clone();

        // Breadcrumbs: each opened row, then the set on screen.
        let mut crumbs = h_flex().gap_1().min_w_0().text_sm().font_family(mono.clone());
        for (k, (t, row)) in self.path.clone().into_iter().enumerate() {
            crumbs = crumbs
                .child(div().id(("crumb", k)).flex_none().text_color(theme.link).cursor_pointer().hover(|s| s.underline()).on_click(cx.listener(move |this, _, window, cx| this.up_to(k, window, cx))).child(format!("{}[{row}]", short(&t))))
                .child(div().flex_none().text_color(theme.muted_foreground).child("/"));
        }
        crumbs = crumbs
            .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().font_weight(gpui::FontWeight::SEMIBOLD).child(short(&table).to_string()))
            .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(format!("{} rows", self.total)));

        let targets = self.targets().len();
        let mut toolbar = h_flex().px_2().py_1().gap_1().child(crumbs).child(div().flex_1().min_w_2()).child(div().w(px(240.0)).flex_shrink(1.0).min_w(px(120.0)).child(Input::new(&self.filter).small().cleanable(true)));
        if in_assembly && depth == 0 {
            toolbar = toolbar.child(Button::new("a-new-content").xsmall().primary().icon(Icon::new(IconName::Sparkles)).label("New content").tooltip("A new skin, ability, card, item or champion, step by step").on_click(cx.listener(|this, _, _, cx| this.open_wizard(None, cx))));
        }
        toolbar = toolbar
            .child(Button::new("arow-add").xsmall().ghost().icon(Icon::new(IconName::Plus)).tooltip(format!("New row: an empty row at the end, with a fresh id ({})", key_hint("⌘⇧↩"))).on_click(cx.listener(|this, _, window, cx| this.new_row(window, cx))))
            .child(Button::new("arow-dup").xsmall().ghost().icon(Icon::new(IconName::Copy)).disabled(self.selected.is_none()).tooltip(format!("Duplicate: the selected row copied to the end with a fresh id and its own copies of its messages ({})", key_hint("⌘D"))).on_click(cx.listener(|this, _, window, cx| this.duplicate(window, cx))))
            .child(Button::new("arow-del").xsmall().ghost().icon(Icon::new(IconName::Trash)).disabled(targets == 0).tooltip(if targets > 1 { format!("Delete the {targets} marked rows…") } else { "Delete the selected row… (Delete)".to_string() }).on_click(cx.listener(|this, _, window, cx| this.delete_rows(window, cx))))
            .child({
                let me = cx.entity().downgrade();
                let all: Vec<String> = self.columns.iter().map(|c| c.name.clone()).collect();
                let hidden = self.hidden.get(&table).cloned().unwrap_or_default();
                let pinned = self.pinned();
                Button::new("a-columns").xsmall().ghost().icon(Icon::new(IconName::Eye)).tooltip("Columns to show (kept per table)").dropdown_menu(move |menu, _, _| {
                    let mut menu = menu.scrollable(true).max_h(px(420.0)).item({
                        let me = me.clone();
                        PopupMenuItem::new("Show all").disabled(hidden.is_empty()).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.show_all_columns(cx));
                        })
                    });
                    menu = menu.separator();
                    for c in &all {
                        let (me, c2) = (me.clone(), c.clone());
                        let label = if pinned.contains(c) { format!("{c}  (pinned)") } else { c.clone() };
                        menu = menu.item(PopupMenuItem::new(label).checked(!hidden.contains(c)).disabled(pinned.contains(c)).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.toggle_column(c2.clone(), cx));
                        }));
                    }
                    menu
                })
            })
            .child({
                let me = cx.entity().downgrade();
                let undo = undo.clone();
                Button::new("a-more").xsmall().ghost().icon(Icon::new(IconName::Ellipsis)).tooltip("Copy and paste rows, skins, champion vendor, export, reload, undo, discard").dropdown_menu(move |menu, _, _| {
                    let item = |label: String, f: fn(&mut Assembly, &mut Window, &mut Context<Assembly>)| {
                        let me = me.clone();
                        PopupMenuItem::new(label).on_click(move |_, window, cx| {
                            let _ = me.update(cx, |this, cx| f(this, window, cx));
                        })
                    };
                    let mut menu = menu
                        .item(item(format!("Copy the rows    {}", key_hint("⌘C")), |a, _, cx| a.copy_rows(cx)))
                        .item(item(format!("Paste    {}", key_hint("⌘V")), |a, w, cx| a.paste(w, cx)))
                        .item(item("Copy as tab-separated text".into(), |a, _, cx| a.copy_tsv(cx)))
                        .separator();
                    if in_assembly {
                        menu = menu
                            .item(item("Skins…".into(), |a, w, cx| a.skins(w, cx)))
                            .item(item("Champion vendor…".into(), |a, w, cx| a.champion_vendor(w, cx)))
                            .item(item("Export a mod from the changes…".into(), |a, w, cx| a.export_mod(w, cx)))
                            .separator();
                    }
                    menu.item(item("Export copies of the changed files…".into(), |a, _, cx| a.export_copies(cx)).disabled(!doc_dirty))
                        .item(item("Reload the files from the game folder".into(), |a, w, cx| a.reload_files(w, cx)))
                        .separator()
                        .item(item(match &undo { Some(u) => format!("Undo: {u}"), None => "Undo".into() }, |a, w, cx| a.undo(w, cx)).disabled(undo.is_none()))
                        .item(item("Discard this file's changes…".into(), |a, w, cx| a.discard(w, cx)).disabled(!doc_dirty))
                })
            })
            .child(Button::new("a-save").xsmall().when(doc_dirty, |b| b.primary()).label("Save…").disabled(!doc_dirty).on_click(cx.listener(|this, _, window, cx| this.save(window, cx))));

        // Several rows marked: what an edit does, and their own actions.
        let marked_bar = (self.marked.len() > 1).then(|| {
            h_flex()
                .px_2()
                .py_0p5()
                .gap_2()
                .text_xs()
                .bg(theme.primary.opacity(0.1))
                .border_b_1()
                .border_color(theme.border)
                .child(format!("{} rows marked: editing a cell of one of them sets it on all.", self.marked.len()))
                .child(div().flex_1())
                .child(Button::new("am-copy").xsmall().ghost().icon(Icon::new(IconName::Copy)).label("Copy").on_click(cx.listener(|this, _, _, cx| this.copy_rows(cx))))
                .child(Button::new("am-tsv").xsmall().ghost().label("Copy as TSV").on_click(cx.listener(|this, _, _, cx| this.copy_tsv(cx))))
                .child(Button::new("am-clear").xsmall().ghost().label("Clear").tooltip("Clear the marks (Escape)").on_click(cx.listener(|this, _, _, cx| {
                    this.marked.clear();
                    cx.notify();
                })))
        });

        // The header: the pinned columns over the scrolled ones, like the rows.
        let off = self.hoff();
        let (flow, before, after) = layout.window(off, self.view_width());
        let n_pin = layout.cols.iter().filter(|c| c.pinned).count();
        let head_h = (1.75 * f32::from(window.rem_size())).round();
        let mut header = h_flex().relative().h(px(head_h)).w(px(layout.width)).flex_none().font_family(mono).text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).border_b_1().border_color(theme.border).child(div().w(px(layout.pin_w + before)).flex_none());
        let mut pin_head = h_flex().absolute().top_0().left(px(off)).h_full().bg(theme.background).border_r_1().border_color(theme.border).child(div().w(px(layout.index_w)).flex_none().px_2().text_right().child("#"));
        let head = |l: &Laid, this: &Assembly, cx: &mut Context<Assembly>| {
            let name = l.col.name.clone();
            let arrow = match &this.sort {
                Some((s, d)) if s == &name => if *d { " ▼" } else { " ▲" },
                _ => "",
            };
            let tip = format!("{}{} (click to sort)", l.col.kind, if l.col.count < this.total { format!(" · in {} rows", l.col.count) } else { String::new() });
            div()
                .id(SharedString::from(format!("ahead-{name}")))
                .w(px(l.w))
                .h_full()
                .flex()
                .items_center()
                .flex_none()
                .px_2()
                .overflow_hidden()
                .whitespace_nowrap()
                .cursor_pointer()
                .hover(|s| s.bg(theme.list_hover))
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                .on_click(cx.listener({
                    let name = name.clone();
                    move |this, _, _, cx| this.set_sort(name.clone(), cx)
                }))
                .child(format!("{name}{arrow}"))
        };
        for l in &layout.cols[..n_pin] {
            pin_head = pin_head.child(head(l, self, cx));
        }
        for l in &layout.cols[flow] {
            header = header.child(head(l, self, cx));
        }
        let header = header.child(div().w(px(after)).flex_none()).child(pin_head);

        let mut list = uniform_list("arows", self.total, cx.processor(Self::render_rows)).track_scroll(&self.scroll).w(px(layout.width)).flex_1();
        // Sideways gestures go to the grid around it, not down the rows.
        list.style().restrict_scroll_to_axis = Some(true);
        let empty = self.total == 0 && self.rows.is_empty() && !self.pages.is_empty();
        let count = if !self.filter_text.is_empty() { format!("{} rows match", self.total) } else { format!("{} rows{}", self.total, if depth > 0 { " in this row set" } else { "" }) };
        v_flex()
            .size_full()
            .child(toolbar)
            .children(marked_bar)
            .child(
                div().id("hgrid").relative().flex_1().min_h_0().overflow_x_scroll().restrict_scroll_to_axis().track_scroll(&self.hscroll).child(v_flex().h_full().w(px(layout.width)).child(header).child(list)).when(empty && !self.filter_text.is_empty(), |d| {
                    d.child(div().absolute().top(px(head_h + 24.0)).left_0().right_0().flex().justify_center().text_sm().text_color(theme.muted_foreground).child("No row matches the filter."))
                }),
            )
            .child(
                h_flex()
                    .px_2()
                    .gap_3()
                    .h(px(22.0))
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().flex_none().child(count))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_right().child(format!("Enter or double-click: edit · {0}/⇧-click: mark rows · {0}C {0}V: copy, paste · right-click: more", if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" }))),
            )
            .child(crate::driver::probe("asm-grid"))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(widths: &[f32], pinned: usize) -> Layout {
        let mut x = 50.0;
        let cols: Vec<Laid> = widths
            .iter()
            .enumerate()
            .map(|(k, w)| {
                let l = Laid { col: Column { name: format!("C{k}"), kind: "dword".into(), count: 0 }, x, w: *w, pinned: k < pinned };
                x += w;
                l
            })
            .collect();
        let pin_w = 50.0 + widths[..pinned].iter().sum::<f32>();
        Layout { cols, index_w: 50.0, pin_w, width: x, row_h: 24.0 }
    }

    #[test]
    fn only_the_columns_in_view_are_laid_out() {
        // 2 pinned columns of 100, then 40 of 100: 4250 wide.
        let l = layout(&[100.0; 42], 2);
        let (cols, before, after) = l.window(0.0, 800.0);
        // In view from 250 to 800: columns 2..=7, plus the overscan to the right.
        assert_eq!(cols, 2..10);
        assert_eq!(before, 0.0);
        assert_eq!(after, l.width - 1050.0);
        // Scrolled: the spacers stand for what is not laid out.
        let (cols, before, after) = l.window(1000.0, 800.0);
        let shown: f32 = l.cols[cols.clone()].iter().map(|c| c.w).sum();
        assert_eq!(l.pin_w + before + shown + after, l.width);
        assert!(l.cols[cols.start].x <= 1000.0 + l.pin_w && l.cols[cols.end - 1].x + 100.0 >= 1800.0);
        // Scrolled to the end, and a table of pinned columns only.
        let (cols, _, after) = l.window(l.width - 800.0, 800.0);
        assert_eq!((cols.end, after), (42, 0.0));
        let l = layout(&[100.0, 100.0], 2);
        assert_eq!(l.window(0.0, 800.0), (2..2, 0.0, 0.0));
    }
}
