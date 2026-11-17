//! The first screen, before a game folder is open: what the Studio is for,
//! the games found on this computer (one click opens one), the mods and the
//! folders opened before, and why the last folder did not open.

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
                            .child(Button::new(("recent-forget", k)).xsmall().ghost().icon(Icon::new(IconName::X)).tooltip("Remove from the list (the folder stays where it is)").on_click(cx.listener(move |this, _, _, cx| {
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
                        div().invisible().group_hover("project-row", |s| s.visible()).child(Button::new(("project-forget", k)).xsmall().ghost().icon(Icon::new(IconName::X)).tooltip("Remove from the list (the mod stays where it is)").on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.prefs.projects.retain(|p| p != &f2);
                            this.prefs.save();
                            cx.notify();
                        })))
                    }),
            );
        }
        let has_projects = !self.prefs.projects.is_empty();
        // The games found here: the first one is the obvious choice.
        let mut games = v_flex().gap_1();
        for (k, g) in self.games.iter().enumerate() {
            let path = g.path.clone();
            let open = Button::new(("game-open", k)).small().label("Open").on_click(cx.listener(move |this, _, _, cx| this.open_workspace(path.clone(), cx)));
            games = games.child(
                h_flex()
                    .relative()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .child(Icon::new(IconName::Gamepad2).text_color(theme.muted_foreground))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(match &g.version {
                                Some(v) => format!("{} · Paladins {v}", g.label),
                                None => g.label.clone(),
                            }))
                            .child(div().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(g.path.clone())),
                    )
                    .child(if k == 0 { open.primary() } else { open.ghost() })
                    .child(crate::driver::probe(format!("game-{k}"))),
            );
        }
        let found = !self.games.is_empty();
        let error = self.open_error.clone().map(|e| {
            h_flex()
                .items_start()
                .gap_2()
                .p_3()
                .rounded_lg()
                .bg(theme.danger.opacity(0.12))
                .child(Icon::new(IconName::CircleAlert).small().text_color(theme.danger))
                .child(
                    v_flex()
                        .flex_1()
                        .gap_0p5()
                        .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child("That folder could not be opened"))
                        .child(div().text_xs().text_color(theme.muted_foreground).whitespace_normal().child(e))
                        .child(super::ui::hint("Pick the Paladins folder itself (the one with a ChaosGame folder inside).", &theme)),
                )
        });
        div().id("welcome").size_full().overflow_y_scroll().child(
            v_flex().w_full().items_center().py_12().child(
                v_flex()
                    .w(px(600.0))
                    .gap_4()
                    .child(div().text_2xl().font_weight(gpui::FontWeight::BOLD).child("Tempest Studio"))
                    .child(div().text_sm().text_color(theme.muted_foreground).child("Make Paladins mods: change skins, numbers, sounds, maps and more, then build a mod you can install and share. The game's own files are never touched."))
                    .children(error)
                    .when(found, |d| d.child(super::ui::section("Your games", &theme).pb_0()).child(games))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().relative().child({
                                let b = Button::new("welcome-open").icon(Icon::new(IconName::FolderOpen)).label(if found { "Open another folder…" } else { "Open the Paladins folder…" }).on_click(cx.listener(|this, _, _, cx| this.pick_workspace(cx)));
                                if found { b.ghost() } else { b.primary() }
                            }).child(crate::driver::probe("welcome-open")))
                            .child(Button::new("welcome-project").ghost().icon(Icon::new(IconName::Briefcase)).label("Open a mod…").on_click(cx.listener(|this, _, _, cx| this.pick_project(cx))))
                            .child(Button::new("welcome-file").ghost().icon(Icon::new(IconName::Package)).label("Open a package file…").tooltip("A .upk from anywhere: its folder opens with it").on_click(cx.listener(|this, _, window, cx| this.run(Cmd::OpenFile, window, cx)))),
                    )
                    .when(!found, |d| d.child(super::ui::hint("The Paladins folder is the one with a ChaosGame folder inside. Tempest Launcher's games show up here by themselves.", &theme)))
                    .when(has_projects, |d| {
                        d.child(h_flex().pt_2().child(div().flex_1().min_w_0().child(super::ui::section("Your mods", &theme))).child(Button::new("projects-clear").xsmall().ghost().label("Clear").tooltip("Clear the list (the mods stay where they are)").on_click(cx.listener(|this, _, _, cx| {
                            this.prefs.projects.clear();
                            this.prefs.save();
                            cx.notify();
                        }))))
                        .child(projects)
                    })
                    .when(has_recent, |d| {
                        d.child(h_flex().pt_2().child(div().flex_1().min_w_0().child(super::ui::section("Recent folders", &theme))).child(Button::new("recent-clear").xsmall().ghost().label("Clear").tooltip("Clear the list (the folders stay where they are)").on_click(cx.listener(|this, _, _, cx| {
                            this.recent = super::forget_workspace(None);
                            cx.notify();
                        }))))
                        .child(recent)
                    }),
            ),
        )
    }
}
