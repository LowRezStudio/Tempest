//! The map view's side bar: which map, its levels (show/hide, Kismet), what
//! kinds of actors are drawn, and the outliner (a virtual list: maps have
//! thousands of actors).

use gpui::{div, prelude::*, px, uniform_list, AnyElement, Context};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;

use super::{hsla, MapEditor, CATEGORIES};

impl MapEditor {
    /// The outliner's rows: (level, export, name, class, colour).
    pub(super) fn outliner_rows(&self, cx: &gpui::App) -> Vec<(String, usize, String, String, u32)> {
        let q = self.filter.read(cx).value().trim().to_ascii_lowercase();
        let mut out = Vec::new();
        for l in &self.levels {
            if self.hidden_levels.contains(&l.path) {
                continue;
            }
            for a in &l.actors {
                if self.rooftops_only {
                    if a.can_base_on {
                        continue;
                    }
                } else if self.hidden_categories.contains(&a.category) {
                    continue;
                }
                if !q.is_empty() && !a.name.to_ascii_lowercase().contains(&q) && !a.class.to_ascii_lowercase().contains(&q) {
                    continue;
                }
                out.push((l.path.clone(), a.export, a.name.clone(), a.class.clone(), a.color()));
            }
        }
        out
    }

    pub(super) fn set_category(&mut self, id: &str, shown: bool, cx: &mut Context<Self>) {
        if shown {
            self.hidden_categories.remove(id);
        } else {
            self.hidden_categories.insert(id.to_string());
        }
        let hidden: Vec<String> = self.hidden_categories.iter().cloned().collect();
        self.with_studio(cx, move |s, _| {
            s.prefs.map_hidden = hidden;
            s.prefs.save();
        });
        self.apply_visibility();
        if self.collision.enabled {
            self.rebuild_collision();
        }
        cx.notify();
    }

    pub(super) fn set_level_shown(&mut self, path: &str, shown: bool, cx: &mut Context<Self>) {
        if shown {
            self.hidden_levels.remove(path);
        } else {
            self.hidden_levels.insert(path.to_string());
        }
        self.apply_visibility();
        if self.collision.enabled {
            self.rebuild_collision();
        }
        cx.notify();
    }

    pub(super) fn render_side(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(path) = self.path.clone() else {
            let q = self.map_filter.read(cx).value().trim().to_ascii_lowercase();
            let maps: Vec<_> = self.list.iter().flatten().filter(|m| (!self.persistent_only || m.persistent) && m.name.to_ascii_lowercase().contains(&q)).cloned().collect();
            let rows: Vec<AnyElement> = maps
                .iter()
                .enumerate()
                .map(|(k, m)| {
                    let p = m.path.clone();
                    h_flex()
                        .id(("map-pick", k))
                        .px_2()
                        .py_0p5()
                        .gap_2()
                        .text_xs()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.list_hover))
                        .child(crate::driver::probe(format!("map-{}", m.name.trim_end_matches(".upk").to_ascii_lowercase())))
                        .child(Icon::new(IconName::Map).xsmall())
                        .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family("Menlo").child(m.name.trim_end_matches(".upk").to_string()))
                        .when(m.kismet, |d| d.child(Icon::new(IconName::Workflow).xsmall().text_color(theme.muted_foreground)))
                        .on_click(cx.listener(move |this, _, _, cx| this.open(p.clone(), None, true, None, cx)))
                        .into_any_element()
                })
                .collect();
            return v_flex()
                .size_full()
                .text_xs()
                .child(div().p_2().border_b_1().border_color(theme.border).child(Input::new(&self.map_filter).xsmall()))
                .child(div().px_2().py_1().child(Checkbox::new("map-persistent").label("Playable maps only (those that stream sublevels)").checked(self.persistent_only).on_click(cx.listener(|this, v: &bool, _, cx| {
                    this.persistent_only = *v;
                    cx.notify();
                }))))
                .when(self.list_loading, |d| d.child(div().p_2().text_color(theme.muted_foreground).child("Looking for levels…")))
                .child(div().id("map-list").flex_1().min_h_0().overflow_y_scroll().child(v_flex().children(rows)))
                .into_any_element();
        };
        let mut levels = v_flex().pb_1();
        for (k, l) in self.levels.iter().enumerate() {
            let hidden = self.hidden_levels.contains(&l.path);
            let dirty = self.is_dirty(&l.path, cx);
            let (p1, p2) = (l.path.clone(), l.path.clone());
            levels = levels.child(
                h_flex()
                    .px_2()
                    .gap_1()
                    .when(hidden, |d| d.opacity(0.5))
                    .child(Button::new(("lvl-eye", k)).xsmall().ghost().icon(Icon::new(if hidden { IconName::EyeOff } else { IconName::Eye })).tooltip(if hidden { "Show this level" } else { "Hide this level" }).on_click(cx.listener(move |this, _, _, cx| {
                        let shown = this.hidden_levels.contains(&p1);
                        this.set_level_shown(&p1, shown, cx);
                    })))
                    .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family("Menlo").child(l.name.clone()))
                    .when(dirty, |d| d.child(div().text_color(theme.warning).child("●")))
                    .child(div().text_color(theme.muted_foreground).child(l.actors.len().to_string()))
                    .child(Button::new(("lvl-kismet", k)).xsmall().ghost().icon(Icon::new(IconName::Workflow)).tooltip("Kismet of this level").on_click(cx.listener(move |this, _, _, cx| this.open_kismet(p2.clone(), None, true, cx)))),
            );
        }
        if !self.missing.is_empty() {
            levels = levels.child(div().px_2().py_1().text_color(theme.warning).child(format!("Not in this folder: {}", self.missing.join(", "))));
        }
        let mut totals = std::collections::HashMap::new();
        for l in &self.levels {
            for a in &l.actors {
                *totals.entry(a.category.as_str()).or_insert(0usize) += 1;
            }
        }
        let mut cats = div().grid().grid_cols(2).gap_x_2().px_2().pb_1();
        for (ci, (id, label, color)) in CATEGORIES.iter().enumerate() {
            let Some(n) = totals.get(id) else { continue };
            let shown = !self.hidden_categories.contains(*id);
            cats = cats.child(
                h_flex()
                    .gap_1p5()
                    .py_0p5()
                    .child(Checkbox::new(("cat", ci)).checked(shown).on_click(cx.listener(move |this, v: &bool, _, cx| this.set_category(id, *v, cx))))
                    .child(div().size(px(8.0)).rounded_full().bg(hsla(*color)))
                    .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(label.to_string()))
                    .child(div().text_color(theme.muted_foreground).child(n.to_string())),
            );
        }
        let rows = self.outliner_rows(cx);
        let count = rows.len();
        let sel = self.selected.clone();
        let rows = std::rc::Rc::new(rows);
        let list = uniform_list(
            "map-outliner",
            count,
            cx.processor({
                let rows = rows.clone();
                move |_this, range: std::ops::Range<usize>, _, cx| {
                    let theme = cx.theme().clone();
                    range
                        .map(|k| {
                            let (level, export, name, class, color) = rows[k].clone();
                            let on = sel.as_ref() == Some(&(level.clone(), export));
                            let (l2, l3) = (level.clone(), level.clone());
                            h_flex()
                                .id(("map-row", k))
                                .h(px(20.0))
                                .px_2()
                                .gap_1p5()
                                .text_xs()
                                .cursor_pointer()
                                .when(on, |d| d.bg(theme.primary.opacity(0.2)))
                                .when(!on, |d| d.hover(|d| d.bg(theme.list_hover)))
                                .child(div().size(px(8.0)).flex_none().rounded_full().bg(hsla(color)))
                                .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family("Menlo").child(name))
                                .child(div().max_w(px(120.0)).overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(theme.muted_foreground).child(class))
                                .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                    this.select(Some((l2.clone(), export)), true, cx);
                                    if e.click_count() == 2 {
                                        this.frame_selection();
                                    }
                                    let _ = &l3;
                                }))
                                .into_any_element()
                        })
                        .collect()
                }
            }),
        )
        .size_full();
        let name = super::file_name(&path);
        v_flex()
            .size_full()
            .text_xs()
            .child(
                h_flex()
                    .px_2()
                    .py_1p5()
                    .gap_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(Icon::new(IconName::Map).xsmall())
                    .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family("Menlo").font_weight(gpui::FontWeight::SEMIBOLD).child(name))
                    .child(Button::new("map-change").xsmall().ghost().label("Change").tooltip("Pick another map").on_click(cx.listener(|this, _, _, cx| this.close_map(cx)))),
            )
            .child(div().px_2().pt_1().text_color(theme.muted_foreground).font_weight(gpui::FontWeight::SEMIBOLD).child("LEVELS"))
            .child(levels)
            .child(div().px_2().pt_1().border_t_1().border_color(theme.border).text_color(theme.muted_foreground).font_weight(gpui::FontWeight::SEMIBOLD).child("SHOW"))
            .child(cats)
            .child(
                h_flex()
                    .p_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(div().flex_1().child(Input::new(&self.filter).xsmall()))
                    .child(div().text_color(theme.muted_foreground).child(count.to_string())),
            )
            .child(div().px_2().pb_1().child(Checkbox::new("map-rooftops").label("Only what can't be stood on (rooftops)").checked(self.rooftops_only).on_click(cx.listener(|this, v: &bool, _, cx| {
                this.rooftops_only = *v;
                cx.notify();
            }))))
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }
}
