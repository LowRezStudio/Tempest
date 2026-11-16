//! The Studio's shared look, so every view reads as one editor: the frame and
//! pane surfaces, pane headers, toolbars, section labels, hints, and the
//! empty, loading and error states.
//!
//! Surfaces, darkest to lightest in the dark theme: the frame (header,
//! activity bar, status line: `title_bar`), side panes (`sidebar`), the work
//! area (`background`), raised things (`secondary`: inputs, cards). The accent
//! (`primary`) is for one main action per surface and the selection only.

use gpui::{div, prelude::*, px, AnyElement, Div, SharedString};
use gpui_component::spinner::Spinner;
use gpui_component::{h_flex, v_flex, Icon, Sizable, Theme};
use gpui_kit_assets::IconName;

/// Every pane's title row.
pub const PANE_HEADER: f32 = 32.0;
/// A view's own toolbar, under the window's header.
pub const TOOLBAR: f32 = 36.0;

/// The monospace face for ids, paths, code and numbers.
pub fn mono() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}

/// A pane's title row: its name, then its tools on the right.
pub fn pane_header(title: impl Into<SharedString>, tools: impl IntoElement, theme: &Theme) -> Div {
    h_flex()
        .flex_none()
        .h(px(PANE_HEADER))
        .px_2()
        .gap_1()
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(theme.foreground)
        .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(title.into()))
        .child(h_flex().flex_none().gap_0p5().text_color(theme.muted_foreground).child(tools))
}

/// A view's toolbar: one height everywhere, a rule under it.
pub fn toolbar(theme: &Theme) -> Div {
    h_flex().flex_none().min_h(px(TOOLBAR)).px_2().gap_1().border_b_1().border_color(theme.border).bg(theme.background)
}

/// A label over a group of fields or rows inside a pane.
pub fn section(title: impl Into<SharedString>, theme: &Theme) -> Div {
    div().pt_3().pb_1().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(title.into())
}

/// A line of help under a control or a section.
pub fn hint(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div().text_xs().text_color(theme.muted_foreground).whitespace_normal().child(text.into())
}

/// A pane with nothing to show yet: what it is for and what to do next; the
/// caller adds the action buttons as children.
pub fn empty(icon: IconName, title: impl Into<SharedString>, body: impl Into<SharedString>, theme: &Theme) -> Div {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .p_6()
        .child(Icon::new(icon).large().text_color(theme.muted_foreground.opacity(0.7)))
        .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.foreground).child(title.into()))
        .child(div().max_w(px(340.0)).text_xs().text_center().text_color(theme.muted_foreground).whitespace_normal().child(body.into()))
}

/// Work in progress in a pane.
pub fn loading(text: impl Into<SharedString>, theme: &Theme) -> Div {
    h_flex().size_full().items_center().justify_center().gap_2().text_sm().text_color(theme.muted_foreground).child(Spinner::new().small().color(theme.muted_foreground)).child(text.into())
}

/// Something failed in a pane: what happened; the caller adds the way out
/// (Retry, Choose another…) as children.
pub fn error(title: impl Into<SharedString>, detail: impl Into<SharedString>, theme: &Theme) -> Div {
    let detail: SharedString = detail.into();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .p_6()
        .child(Icon::new(IconName::CircleAlert).large().text_color(theme.danger))
        .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.foreground).child(title.into()))
        .when(!detail.is_empty(), |d| d.child(div().max_w(px(420.0)).text_xs().text_center().text_color(theme.muted_foreground).whitespace_normal().child(detail)))
}

/// A keyboard shortcut drawn as a key cap.
pub fn kbd(keys: impl Into<SharedString>, theme: &Theme) -> AnyElement {
    div().px_1().rounded_sm().border_1().border_color(theme.border).bg(theme.secondary).text_xs().text_color(theme.muted_foreground).child(keys.into()).into_any_element()
}
