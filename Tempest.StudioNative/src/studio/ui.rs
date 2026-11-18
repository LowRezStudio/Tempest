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
/// The side pane's width (every view's, until the user drags it).
pub const SIDE_PANE: f32 = 260.0;
/// A list row in a pane.
pub const ROW: f32 = 24.0;

/// A list row's height, scaling cleanly with text zoom.
pub fn row_h(window: &gpui::Window) -> gpui::Pixels {
    gpui::px((1.5 * f32::from(window.rem_size())).max(ROW).round())
}

/// A pane's filter field, under its header: the same inset in every pane.
pub fn pane_filter(input: impl IntoElement) -> Div {
    div().flex_none().px_2().pb_2().child(input)
}

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
        .min_h(px(PANE_HEADER))
        .py_0p5()
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

/// One titled part of a page (the Mod view's, like the website's panels): a
/// small round icon, the title, room on the right; the caller adds the body.
pub fn panel(icon: IconName, title: impl Into<SharedString>, aside: impl IntoElement, theme: &Theme) -> Div {
    v_flex().min_w_0().p_4().gap_3().rounded_lg().border_1().border_color(theme.border).bg(theme.sidebar).child(
        h_flex()
            .min_h(px(24.0))
            .gap_2()
            .child(div().flex_none().size(px(20.0)).rounded_full().bg(theme.secondary).flex().items_center().justify_center().child(Icon::new(icon).xsmall().text_color(theme.muted_foreground)))
            .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.foreground).child(title.into()))
            .child(h_flex().flex_none().gap_1().text_xs().text_color(theme.muted_foreground).child(aside)),
    )
}

/// A form field: its label (and on the right a counter or "optional") over the control.
pub fn field(label: impl Into<SharedString>, aside: impl IntoElement, control: impl IntoElement, theme: &Theme) -> Div {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(h_flex().gap_2().text_xs().child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(theme.muted_foreground).child(label.into())).child(div().flex_none().text_color(theme.muted_foreground).child(aside)))
        .child(control)
}

/// A small fact drawn as a badge (the mod's kind, signed or not, a tag).
pub fn badge(icon: Option<IconName>, text: impl Into<SharedString>, color: gpui::Hsla, theme: &Theme) -> Div {
    h_flex()
        .flex_none()
        .min_h(px(20.0))
        .py_0p5()
        .px_1p5()
        .gap_1()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .text_xs()
        .text_color(color)
        .when_some(icon, |d, i| d.child(Icon::new(i).xsmall().text_color(color)))
        .child(text.into())
}

/// A mod's icon: its image, else its initials on a tone picked from its id
/// (as the website draws a mod without one).
pub fn mod_icon(image: Option<&str>, id: &str, name: &str, size: f32, theme: &Theme) -> AnyElement {
    if let Some(path) = image {
        return gpui::img(std::path::PathBuf::from(path)).flex_none().size(px(size)).rounded_lg().into_any_element();
    }
    let plate = div().flex_none().size(px(size)).rounded_lg().flex().items_center().justify_center();
    let initials: String = name.split_whitespace().take(2).filter_map(|w| w.chars().next()).flat_map(char::to_uppercase).collect();
    if id.is_empty() || initials.is_empty() {
        return plate.bg(theme.secondary).child(Icon::new(IconName::Package).large().text_color(theme.muted_foreground)).into_any_element();
    }
    let tones = [theme.magenta, theme.blue, theme.green, theme.yellow, theme.cyan, theme.red];
    let hash = id.chars().fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    plate.bg(tones[hash as usize % tones.len()]).text_size(px(size * 0.36)).font_weight(gpui::FontWeight::BOLD).text_color(theme.primary_foreground).child(initials).into_any_element()
}

/// A keyboard shortcut drawn as a key cap.
pub fn kbd(keys: impl Into<SharedString>, theme: &Theme) -> AnyElement {
    div().px_1().rounded_sm().border_1().border_color(theme.border).bg(theme.secondary).text_xs().text_color(theme.muted_foreground).child(keys.into()).into_any_element()
}

/// An error as the backend or the OS words it, followed by what to do about
/// it when the cause is a common one (a file the running game holds, a
/// missing file, a folder that is not the game's).
pub fn explain_error(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let hint = if ["permission denied", "access is denied", "being used by another process", "os error 32)", "os error 13)", "read-only file system"].iter().any(|p| lower.contains(p)) {
        "The file is locked or read-only: close the game (and anything else using it), then try again."
    } else if lower.contains("no space left") || lower.contains("os error 28)") {
        "The disk is full: free some space, then try again."
    } else if ["no such file", "cannot find the file", "os error 2)", "not found"].iter().any(|p| lower.contains(p)) {
        "A file it needs is missing: it may have been moved or deleted."
    } else if lower.contains("holds no script packages") || lower.contains("no packages") {
        "Open the Paladins folder itself (the one with a ChaosGame folder inside)."
    } else if lower.contains("backend has stopped") {
        "The Studio's engine stopped. Your mod is kept: restart the Studio to go on."
    } else {
        return text.to_string();
    };
    format!("{text}. {hint}")
}

#[cfg(test)]
mod tests {
    use super::explain_error;

    #[test]
    fn explains_common_errors() {
        assert!(explain_error("Could not write A.upk: Permission denied (os error 13)").ends_with("then try again."));
        assert!(explain_error("Could not open x: No such file or directory (os error 2)").contains("missing"));
        assert!(explain_error("Folder holds no script packages").contains("ChaosGame"));
        assert!(explain_error("The backend has stopped").contains("restart the Studio"));
        assert!(explain_error("Write failed: No space left on device (os error 28)").contains("disk is full"));
        assert_eq!(explain_error("Undo failed: no export 3"), "Undo failed: no export 3");
    }
}
