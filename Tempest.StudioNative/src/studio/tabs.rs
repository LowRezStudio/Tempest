//! Window tabs, as in the Electron Studio: each tab keeps a place (package,
//! export); moving around changes the active tab's place, Cmd/Ctrl-click or
//! middle-click on a package or export opens it in a new tab, Cmd/Ctrl+T
//! duplicates the tab, Cmd/Ctrl+W closes it, Ctrl+Tab cycles. One Back /
//! Forward history for the window.

use gpui::{div, prelude::*, px, Context, MouseButton};
use gpui_component::{h_flex, ActiveTheme};

use super::{file_name, Place, Studio};

#[derive(Clone)]
pub struct Tab {
    pub place: Option<Place>,
    pub label: String,
}

impl Studio {
    fn label_of(&self, place: &Place) -> String {
        match place {
            Place::Package { package, export, .. } => match (export, &self.open) {
                (Some(i), Some(t)) if &t.path == package => t.exports.get(*i).map(|e| e.name.clone()).unwrap_or_default(),
                _ => file_name(package).trim_end_matches(".upk").to_string(),
            },
            Place::Assembly { file, table, .. } => table.clone().unwrap_or_else(|| file_name(file)),
            Place::Mod => "Mod".into(),
            Place::Map { path, view, .. } => format!("{}{}", file_name(path).trim_end_matches(".upk"), if view == "scene" { String::new() } else { format!(" · {view}") }),
        }
    }

    /// The active tab shows the history's current place.
    pub(super) fn sync_tab(&mut self) {
        let Some(place) = self.history.get(self.history_at).cloned() else { return };
        let label = self.label_of(&place);
        if self.tabs.is_empty() {
            self.tabs.push(Tab { place: None, label: String::new() });
            self.active_tab = 0;
        }
        self.tabs[self.active_tab] = Tab { place: Some(place), label };
    }

    pub(super) fn new_tab(&mut self, place: Option<Place>, cx: &mut Context<Self>) {
        let place = place.or_else(|| self.tabs.get(self.active_tab).and_then(|t| t.place.clone()));
        let label = place.as_ref().map(|p| self.label_of(p)).unwrap_or_else(|| "New tab".into());
        self.tabs.push(Tab { place: place.clone(), label });
        self.active_tab = self.tabs.len() - 1;
        if let Some(p) = place {
            self.record(p.clone());
            self.go_later(p, cx);
        }
        cx.notify();
    }

    pub(super) fn switch_tab(&mut self, k: usize, cx: &mut Context<Self>) {
        if k >= self.tabs.len() || k == self.active_tab {
            return;
        }
        self.active_tab = k;
        if let Some(p) = self.tabs[k].place.clone() {
            self.record(p.clone());
            self.go_later(p, cx);
        }
        cx.notify();
    }

    pub(super) fn close_tab(&mut self, k: usize, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 || k >= self.tabs.len() {
            return;
        }
        self.tabs.remove(k);
        let next = if k < self.active_tab || self.active_tab >= self.tabs.len() { self.active_tab.saturating_sub(1) } else { self.active_tab };
        self.active_tab = usize::MAX;
        self.switch_tab(next.min(self.tabs.len() - 1), cx);
    }

    pub(super) fn cycle_tab(&mut self, step: isize, cx: &mut Context<Self>) {
        let n = self.tabs.len() as isize;
        if n > 1 {
            self.switch_tab(((self.active_tab as isize + step).rem_euclid(n)) as usize, cx);
        }
    }

    pub(super) fn render_tabs(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.tabs.len() < 2 {
            return None;
        }
        let theme = cx.theme().clone();
        let tabs = self.tabs.iter().enumerate().map(|(k, t)| {
            let active = k == self.active_tab;
            h_flex()
                .id(("tab", k))
                .h(px(28.0))
                .px_2()
                .gap_1()
                .max_w(px(220.0))
                .text_sm()
                .border_r_1()
                .border_color(theme.border)
                .when(active, |d| d.bg(theme.tab_active).text_color(theme.tab_active_foreground))
                .when(!active, |d| d.text_color(theme.muted_foreground).hover(|s| s.bg(theme.list_hover)))
                .on_click(cx.listener(move |this, _, _, cx| this.switch_tab(k, cx)))
                .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _, _, cx| this.close_tab(k, cx)))
                .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().child(t.label.clone()))
                .child(div().id(("close", k)).px_1().rounded_sm().text_xs().hover(|s| s.bg(theme.muted)).child("×").on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_tab(k, cx);
                })))
        });
        Some(h_flex().id("tabs").w_full().overflow_x_scroll().bg(theme.tab_bar).border_b_1().border_color(theme.border).children(tabs).into_any_element())
    }
}
