//! The panes the views share. ⌘B folds the side pane away (the package list,
//! the assembly files, the `.pck` list, the map's levels), ⌘J the second one
//! (the object list, the row pane, the events, the map's inspector). In a
//! narrow window the side pane stays folded and ⌘B shows it over the view.
//! Dragged widths are kept in the settings per group (`Prefs::panes`); the
//! side pane has one width in every view (`side` there), so switching views
//! never moves its edge.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{div, prelude::*, px, AnyElement, App, MouseButton, Window};
use gpui_component::resizable::{h_resizable, resizable_panel};
use gpui_component::ActiveTheme;

/// The settings key of the side width shared by every view.
const SIDE_KEY: &str = "side";

/// Narrower than this (logical pixels) the window is compact.
pub const COMPACT_BELOW: f32 = 1120.0;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Panes {
    pub side_hidden: bool,
    pub second_hidden: bool,
    pub compact: bool,
    /// Compact: the side pane is shown over the view.
    pub overlay: bool,
    /// The side pane's width, the same in every view (0: each group's own).
    pub side_w: f32,
}

impl Panes {
    pub fn side(self) -> bool {
        !self.side_hidden && !self.compact
    }

    pub fn second(self) -> bool {
        !self.second_hidden
    }

    /// ⌘B: in a compact window the overlay, else the side pane.
    pub fn toggle_side(&mut self) {
        if self.compact {
            self.overlay = !self.overlay;
        } else {
            self.side_hidden = !self.side_hidden;
        }
    }
}

/// One window's panes, read by every view of it.
pub type Shared = Rc<Cell<Panes>>;

pub fn update(shared: &Shared, f: impl FnOnce(&mut Panes)) {
    let mut p = shared.get();
    f(&mut p);
    shared.set(p);
}

pub enum Role {
    Side,
    Second,
    Main,
}

/// A pane of a group: what it is, its width (None: it takes what is left) and its content.
pub struct Part(pub Role, pub Option<f32>, pub AnyElement);

/// The panes side by side. `remember` gets the widths of the sized parts, in
/// order, after a drag (hidden ones keep the width they had).
pub fn group(id: &'static str, panes: Panes, parts: Vec<Part>, remember: impl Fn(Vec<f32>, &mut App) + 'static, close_overlay: impl Fn(&mut Window, &mut App) + 'static, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let mut overlay = None;
    let mut shown_sized = Vec::new();
    let mut widths = Vec::new();
    // A group's widths are read when it is first drawn: a new side width makes it a new group.
    let mut g = h_resizable((id, panes.side_w as usize));
    for Part(role, size, el) in parts {
        let size = if matches!(role, Role::Side) && panes.side_w > 0.0 { Some(panes.side_w) } else { size };
        let shown = match role {
            Role::Side => panes.side(),
            Role::Second => panes.second(),
            Role::Main => true,
        };
        // Side panes sit a step darker than the work area.
        let el = if matches!(role, Role::Side) { div().size_full().bg(theme.sidebar).child(el).into_any_element() } else { el };
        let el = if matches!(role, Role::Side) && panes.compact {
            if panes.overlay {
                overlay = Some((el, size.unwrap_or(280.0)));
            }
            div().into_any_element()
        } else {
            el
        };
        let panel = resizable_panel().visible(shown);
        let panel = match size {
            // The middle takes what the others leave (with its content's width as basis it squeezed them).
            None => panel.flex_basis(px(0.0)).overflow_hidden(),
            Some(w) => {
                widths.push(w);
                panel.size(px(w)).flex_none()
            }
        };
        shown_sized.push(size.map(|_| shown));
        g = g.child(panel.child(el));
    }
    let g = g.on_resize(move |state, _, cx| {
        let sizes: Vec<f32> = state.read(cx).sizes().iter().map(|s| f32::from(*s)).collect();
        let mut out = widths.clone();
        let mut k = 0;
        for (i, s) in shown_sized.iter().enumerate() {
            let Some(shown) = s else { continue };
            if *shown {
                if let Some(w) = sizes.get(i) {
                    out[k] = *w;
                }
            }
            k += 1;
        }
        remember(out, cx);
    });
    let mut root = div().size_full().relative().child(g);
    if let Some((el, w)) = overlay {
        root = root
            .child(div().id(("pane-backdrop", 0usize)).absolute().top_0().left_0().size_full().bg(theme.overlay).occlude().on_mouse_down(MouseButton::Left, move |_, window, cx| close_overlay(window, cx)))
            .child(div().absolute().top_0().left_0().bottom_0().w(px(w)).bg(theme.background).border_r_1().border_color(theme.border).shadow_lg().occlude().child(el));
    }
    root.into_any_element()
}

impl super::Studio {
    /// The kept widths of a group's sized panes, else `defaults`.
    pub(crate) fn pane_sizes(&self, id: &str, defaults: &[f32]) -> Vec<f32> {
        self.prefs.panes.get(id).filter(|s| s.len() == defaults.len()).cloned().unwrap_or_else(|| defaults.to_vec())
    }

    /// What a group calls after a drag: the widths go into the settings; the
    /// first one, the side pane's, is every view's side width.
    pub(crate) fn remember_panes(studio: gpui::WeakEntity<super::Studio>, id: &'static str) -> impl Fn(Vec<f32>, &mut App) + 'static {
        move |sizes, cx| {
            let studio = studio.clone();
            cx.defer(move |cx| {
                let _ = studio.update(cx, |s, _| {
                    if let Some(w) = sizes.first() {
                        s.remember_side(*w);
                    }
                    if s.prefs.panes.get(id) != Some(&sizes) {
                        s.prefs.panes.insert(id.to_string(), sizes);
                        s.prefs.save();
                    }
                });
            });
        }
    }

    /// The side width every view takes (kept; applied at the next view switch, not mid-drag).
    pub(crate) fn remember_side(&mut self, w: f32) {
        if self.prefs.panes.get(SIDE_KEY).and_then(|v| v.first()) != Some(&w) {
            self.prefs.panes.insert(SIDE_KEY.to_string(), vec![w]);
            self.prefs.save();
        }
    }

    /// Takes up the kept side width (at start and when the view changes).
    pub(crate) fn sync_side(&mut self) {
        let w = self.prefs.panes.get(SIDE_KEY).and_then(|v| v.first().copied()).unwrap_or(super::ui::SIDE_PANE);
        update(&self.panes, |p| p.side_w = w);
    }

    /// ⌘B and ⌘J, in every view.
    pub(crate) fn toggle_pane(&mut self, second: bool, cx: &mut gpui::Context<Self>) {
        update(&self.panes, |p| if second { p.second_hidden = !p.second_hidden } else { p.toggle_side() });
        self.notify_views(cx);
    }

    /// The window's width decides the compact layout (read at every frame).
    pub(crate) fn sync_compact(&mut self, window: &Window) {
        let compact = f32::from(window.viewport_size().width) < COMPACT_BELOW;
        update(&self.panes, |p| {
            if p.compact != compact {
                p.compact = compact;
                p.overlay = false;
            }
        });
    }

    /// Closes the side pane shown over a compact view.
    pub(crate) fn close_overlay(&mut self, cx: &mut gpui::Context<Self>) {
        if self.panes.get().overlay {
            update(&self.panes, |p| p.overlay = false);
            self.notify_views(cx);
        }
    }

    fn notify_views(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(a) = &self.assembly {
            a.update(cx, |_, cx| cx.notify());
        }
        if let Some(m) = &self.map {
            m.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }
}
