//! The selected actor (the map view's inspector): where it is, what it
//! draws, whether it can be stood on, its collision (`collision.rs`), and
//! its properties, edited in its own level package (a change that moves it
//! redraws it).

use gpui::{div, prelude::*, px, AnyElement, Context, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;

use super::{hsla, MapEditor};
use crate::studio::Place;

fn fmt(v: f64) -> String {
    format!("{}", (v * 100.0).round() / 100.0)
}

impl MapEditor {
    pub(super) fn render_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (Some((level, _)), Some(actor)) = (self.selected.clone(), self.actor().cloned()) else {
            return div()
                .w_full()
                .p_4()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("Click an actor in the view or the list. Drag the gizmo to move it (W), rotate it (E) or scale it (R); the change is written to its level package like any other edit. ⇧A adds an actor, ⌘D duplicates the selected one, Delete deletes it.")
                .into_any_element();
        };
        let level_name = super::file_name(&level);
        let (l2, e2) = (level.clone(), actor.export);
        let mut head = v_flex()
            .p_2()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .text_xs()
            .child(
                h_flex()
                    .gap_1()
                    .child(div().size(px(10.0)).flex_none().rounded_full().bg(hsla(actor.color())))
                    .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family("Menlo").text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(actor.name.clone()))
                    .child(Button::new("mi-frame").xsmall().ghost().icon(Icon::new(IconName::Focus)).tooltip("Frame it (F)").on_click(cx.listener(|this, _, _, cx| {
                        this.frame_selection();
                        cx.notify();
                    })))
                    .child(Button::new("mi-dup").xsmall().ghost().icon(Icon::new(IconName::Copy)).tooltip("Duplicate it next to itself (⌘D)").on_click(cx.listener(|this, _, _, cx| this.duplicate(cx))))
                    .child(Button::new("mi-del").xsmall().ghost().icon(Icon::new(IconName::Trash)).tooltip("Delete it (Delete)").on_click(cx.listener(|this, _, _, cx| this.ask_remove(cx))))
                    .child(Button::new("mi-open").xsmall().ghost().icon(Icon::new(IconName::ExternalLink)).tooltip("Open it in the package view").on_click(cx.listener(move |this, _, _, cx| {
                        let place = Place::package(l2.clone(), Some(e2));
                        this.with_studio(cx, move |s, cx| {
                            s.record(place.clone());
                            s.go_later(place, cx);
                        });
                    }))),
            )
            .child(h_flex().flex_wrap().gap_x_3().text_color(theme.muted_foreground).child(div().font_family("Menlo").child(actor.class.clone())).child(level_name).child(format!("export {}", actor.export)));
        if actor.component {
            head = head.child(div().text_color(theme.muted_foreground).child("A mesh the cooker gathered into a collection: moving it writes this component's Translation, Rotation and Scale3D."));
        }
        let tip = if actor.component { "m_bCanBaseOn: off makes its surfaces rooftops pawns slide off; on a wall that is a heavy wall. For this mesh it is set on its collection actor, so every mesh of the collection changes." } else { "m_bCanBaseOn: off makes its surfaces rooftops pawns slide off; on a wall that is a heavy wall" };
        head = head.child(
            h_flex()
                .gap_2()
                .child(Switch::new("mi-canbase").small().checked(actor.can_base_on).tooltip(tip).on_click(cx.listener(|this, v: &bool, _, cx| this.set_can_base_on(*v, cx))))
                .child("Can be stood on")
                .child(div().font_family("Menlo").text_color(theme.muted_foreground).child(format!("m_bCanBaseOn {}", if actor.can_base_on { "True" } else { "False" })))
                .when(!actor.can_base_on, |d| d.child(div().px_1().rounded_sm().bg(theme.danger).text_color(theme.danger_foreground).child("rooftop"))),
        );
        if let Some(loc) = actor.location {
            let mut grid = div().grid().grid_cols(4).gap_x_2().font_family("Menlo").child(div().text_color(theme.muted_foreground).child("Location")).child(fmt(loc[0])).child(fmt(loc[1])).child(fmt(loc[2]));
            if let Some(r) = actor.rotation {
                grid = grid.child(div().text_color(theme.muted_foreground).child("Rotation")).child(fmt(r[0])).child(fmt(r[1])).child(fmt(r[2]));
            }
            head = head.child(grid);
        }
        if !actor.info.is_empty() {
            head = head.child(h_flex().flex_wrap().gap_1().children(actor.info.iter().map(|(k, v)| div().px_1().rounded_sm().bg(theme.secondary).font_family("Menlo").child(format!("{k} {}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))))));
        }
        if !actor.parts.is_empty() {
            head = head.child(h_flex().flex_wrap().gap_1().children(actor.parts.iter().map(|p| {
                let kind = p["kind"].as_str().unwrap_or("?");
                let label = if kind == "mesh" { p["meshName"].as_str().and_then(|n| n.rsplit('.').next()).unwrap_or("mesh").to_string() } else { kind.to_string() };
                div().px_1().rounded_sm().border_1().border_color(theme.border).font_family("Menlo").child(label)
            })));
        }
        let collision = self.render_collision_panel(&level, &actor, window, cx);
        v_flex()
            .size_full()
            .min_w_0()
            .child(head)
            .child(collision)
            .child(div().id("mi-props").flex_1().min_h_0().overflow_y_scroll().p_2().child(self.props.clone()))
            .into_any_element()
    }
}
