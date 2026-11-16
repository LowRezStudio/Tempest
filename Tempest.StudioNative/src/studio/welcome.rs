//! The first screen, before a game folder is open: what the Studio is for,
//! Open game folder, and the folders opened before.

use gpui::{div, prelude::*, px, Context};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;

use super::{folder_label, Cmd, Studio};

impl Studio {
    pub(super) fn render_welcome(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mut recent = v_flex().gap_1();
        for (k, d) in self.recent.iter().take(8).enumerate() {
            let dir = d.clone();
            let exists = std::path::Path::new(d).is_dir();
            recent = recent.child(
                h_flex()
                    .id(("recent", k))
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .group("recent-row")
                    .when(exists, |d| d.on_click(cx.listener(move |this, _, _, cx| this.open_workspace(dir.clone(), cx))))
                    .child(Icon::new(IconName::Folder).small())
                    .child(div().text_sm().child(folder_label(d)))
                    .child(div().flex_1().min_w_0().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(if exists { d.clone() } else { format!("{d} (not found)") }))
                    .child({
                        let (d1, d2) = (d.clone(), d.clone());
                        h_flex()
                            .gap_0p5()
                            .invisible()
                            .group_hover("recent-row", |s| s.visible())
                            .when(exists, |row| {
                                row.child(Button::new(("recent-window", k)).xsmall().ghost().icon(Icon::new(IconName::ExternalLink)).tooltip("Open in a new window").on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    let dir = d1.clone();
                                    cx.defer(move |cx| {
                                        super::open_window(Some(dir), cx);
                                    });
                                }))
                            })
                            .child(Button::new(("recent-forget", k)).xsmall().ghost().label("×").tooltip("Remove from the list (the folder stays where it is)").on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.recent = super::forget_workspace(Some(&d2));
                                cx.notify();
                            })))
                    }),
            );
        }
        let has_recent = !self.recent.is_empty();
        let mut projects = v_flex().gap_1();
        for (k, f) in self.prefs.projects.iter().take(6).enumerate() {
            let file = f.clone();
            let exists = std::path::Path::new(f).is_file();
            projects = projects.child(
                h_flex()
                    .id(("recent-project", k))
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .group("project-row")
                    .on_click(cx.listener(move |this, _, _, cx| this.open_project(file.clone(), cx)))
                    .child(Icon::new(IconName::Briefcase).small())
                    .child(div().text_sm().child(std::path::Path::new(f).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()))
                    .child(div().flex_1().min_w_0().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(if exists { f.clone() } else { format!("{f} (not found)") }))
                    .child({
                        let f2 = f.clone();
                        div().invisible().group_hover("project-row", |s| s.visible()).child(Button::new(("project-forget", k)).xsmall().ghost().label("×").tooltip("Remove from the list (the project stays where it is)").on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.prefs.projects.retain(|p| p != &f2);
                            this.prefs.save();
                            cx.notify();
                        })))
                    }),
            );
        }
        let has_projects = !self.prefs.projects.is_empty();
        v_flex().size_full().items_center().justify_center().child(
            v_flex()
                .w(px(560.0))
                .gap_4()
                .child(div().text_xl().font_weight(gpui::FontWeight::BOLD).child("Tempest Studio"))
                .child(div().text_sm().text_color(theme.muted_foreground).child("Edit the game's packages (textures, meshes, materials, sounds, scripts) and its content tables, then turn your changes into a mod. Your edits stay in the Studio (and go into your mod); the game folder keeps its originals for the mod cooker."))
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().relative().child(Button::new("welcome-open").primary().icon(Icon::new(IconName::FolderOpen)).label("Open game folder…").on_click(cx.listener(|this, _, _, cx| this.pick_workspace(cx)))).child(crate::driver::probe("welcome-open")))
                        .child(Button::new("welcome-project").icon(Icon::new(IconName::Briefcase)).label("Open a mod project…").on_click(cx.listener(|this, _, _, cx| this.pick_project(cx))))
                        .child(Button::new("welcome-file").ghost().icon(Icon::new(IconName::Package)).label("Open a package file…").tooltip("A .upk from anywhere: its folder opens with it").on_click(cx.listener(|this, _, window, cx| this.run(Cmd::OpenFile, window, cx)))),
                )
                .child(super::ui::hint("Pick the game's folder or its ChaosGame/CookedPCConsole folder.", &theme))
                .when(has_projects, |d| {
                    d.child(h_flex().child(div().flex_1().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("Mod projects")).child(Button::new("projects-clear").xsmall().ghost().label("Clear").tooltip("Clear the list (the project files stay where they are)").on_click(cx.listener(|this, _, _, cx| {
                        this.prefs.projects.clear();
                        this.prefs.save();
                        cx.notify();
                    }))))
                    .child(projects)
                })
                .when(has_recent, |d| {
                    d.child(h_flex().child(div().flex_1().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("Recent folders")).child(Button::new("recent-clear").xsmall().ghost().label("Clear").tooltip("Clear the list (the folders stay where they are)").on_click(cx.listener(|this, _, _, cx| {
                        this.recent = super::forget_workspace(None);
                        cx.notify();
                    }))))
                    .child(recent)
                }),
        )
    }
}
