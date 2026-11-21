//! Window tabs, as in the Electron Studio: each tab keeps a place (package,
//! export); moving around changes the active tab's place, Cmd/Ctrl-click or
//! middle-click on a package or export opens it in a new tab, Cmd/Ctrl+T
//! duplicates the tab, Cmd/Ctrl+W closes it, Ctrl+Tab cycles. One Back /
//! Forward history for the window.

use gpui::{div, prelude::*, px, Context, KeyDownEvent, MouseButton};
use gpui_component::menu::ContextMenuExt;
use gpui_component::{h_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;

use super::{file_name, Place, Studio};

#[derive(Clone)]
pub struct Tab {
    pub place: Option<Place>,
    pub label: String,
}

impl Studio {
    fn label_of(&self, place: &Place) -> String {
        match place {
            Place::Home => "Start".into(),
            Place::Champions(id) => id
                .and_then(|id| u64::try_from(id).ok())
                .and_then(|id| self.champions.as_ref()?.all().iter().find(|c| c.id == id).map(|c| c.name.clone()))
                .unwrap_or_else(|| "Champions".into()),
            Place::Package { package, export, .. } => match (export, &self.open) {
                (Some(i), Some(t)) if &t.path == package => t.exports.get(*i).map(|e| e.name.clone()).unwrap_or_default(),
                _ => file_name(package).trim_end_matches(".upk").to_string(),
            },
            Place::Assembly { file, table, .. } => table
                .as_deref()
                .map(|t| super::assembly::friendly_table(t).map(str::to_string).unwrap_or_else(|| t.to_string()))
                .unwrap_or_else(|| file_name(file)),
            Place::Audio(pck) => match pck.as_deref() {
                // A champion's or item's set is named as the list names it.
                Some(p) if self.open_audio_path() == Some(p) => self.audio.as_ref().and_then(|a| a.title()).unwrap_or_else(|| "Sounds".into()),
                Some(p) if !p.to_ascii_lowercase().ends_with(".pck") => "Sounds".into(),
                Some(p) => file_name(p).trim_end_matches(".pck").to_string(),
                None => "Sounds".into(),
            },
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

    /// Close every tab but `k`.
    fn close_other_tabs(&mut self, k: usize, cx: &mut Context<Self>) {
        if k >= self.tabs.len() {
            return;
        }
        let keep = self.tabs.remove(k);
        self.tabs = vec![keep];
        self.active_tab = usize::MAX;
        self.switch_tab(0, cx);
    }

    /// Close the tabs after `k`.
    fn close_tabs_right(&mut self, k: usize, cx: &mut Context<Self>) {
        if k + 1 >= self.tabs.len() {
            return;
        }
        self.tabs.truncate(k + 1);
        if self.active_tab > k {
            self.active_tab = usize::MAX;
            self.switch_tab(k, cx);
        }
        cx.notify();
    }

    /// The tab's menu (right-click).
    fn tab_menu(&self, k: usize, menu: gpui_component::menu::PopupMenu, cx: &mut Context<Self>) -> gpui_component::menu::PopupMenu {
        use gpui_component::menu::PopupMenuItem;
        let me = cx.entity().downgrade();
        let n = self.tabs.len();
        let item = |label: &str, f: fn(&mut Studio, usize, &mut Context<Studio>)| {
            let me = me.clone();
            PopupMenuItem::new(label.to_string()).on_click(move |_, _, cx| {
                let _ = me.update(cx, |this, cx| f(this, k, cx));
            })
        };
        menu.item(item("Close", |s, k, cx| s.close_tab(k, cx)))
            .item(item("Close the other tabs", |s, k, cx| s.close_other_tabs(k, cx)).disabled(n < 2))
            .item(item("Close the tabs to the right", |s, k, cx| s.close_tabs_right(k, cx)).disabled(k + 1 >= n))
            .separator()
            .item(item("Duplicate", |s, k, cx| {
                let place = s.tabs.get(k).and_then(|t| t.place.clone());
                s.new_tab(place, cx);
            }))
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
        let me = cx.entity();
        let tabs = self.tabs.iter().enumerate().map(|(k, t)| {
            let active = k == self.active_tab;
            let me = me.clone();
            h_flex()
                .id(("tab", k))
                .focusable()
                .tab_index(0)
                .cursor_pointer()
                .min_h(px(28.0))
                .py_0p5()
                .px_2()
                .gap_1()
                .max_w(px(220.0))
                .text_sm()
                .border_r_1()
                .border_color(theme.border)
                .focus_visible(|s| s.border_1().border_color(theme.ring))
                .when(active, |d| d.bg(theme.tab_active).text_color(theme.tab_active_foreground))
                .when(!active, |d| d.text_color(theme.tab_foreground).hover(|s| s.bg(theme.list_hover)))
                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                    if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.switch_tab(k, cx);
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.switch_tab(k, cx)))
                .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _, _, cx| this.close_tab(k, cx)))
                .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(t.label.clone()))
                .child(
                    div()
                        .id(("close", k))
                        .flex_none()
                        .size(px(18.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .hover(|s| s.bg(theme.muted))
                        .child(Icon::new(IconName::X).xsmall())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_tab(k, cx);
                        })),
                )
                .context_menu(move |menu, _, cx| me.update(cx, |this, cx| this.tab_menu(k, menu, cx)))
        });
        Some(h_flex().id("tabs").w_full().overflow_x_scroll().bg(theme.tab_bar).border_b_1().border_color(theme.border).children(tabs).into_any_element())
    }
}
