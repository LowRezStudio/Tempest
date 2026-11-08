//! Settings (⌘,) and what the Studio remembers per game folder: pinned
//! packages, packages opened lately (the palette lists them first), and the
//! place the user left (reopened with the folder). Also the small
//! clipboard / file manager actions of the context menus.

use gpui::{div, prelude::*, px, ClipboardItem, Context, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable, WindowExt};
use gpui_kit_assets::IconName;

use super::{prefs, Place, Studio, Tone, View};

impl Studio {
    pub(crate) fn pinned(&self) -> Vec<String> {
        self.dir.as_deref().map(|d| self.prefs.folder(d).pinned).unwrap_or_default()
    }

    pub(crate) fn is_pinned(&self, path: &str) -> bool {
        self.pinned().iter().any(|p| p == path)
    }

    pub(crate) fn toggle_pin(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(dir) = self.dir.clone() else { return };
        let f = self.prefs.folder_mut(&dir);
        if let Some(k) = f.pinned.iter().position(|p| *p == path) {
            f.pinned.remove(k);
            self.set_status(format!("Unpinned {}", super::file_name(&path)), cx);
        } else {
            f.pinned.push(path.clone());
            self.set_status(format!("Pinned {}: it stays at the top of the list", super::file_name(&path)), cx);
        }
        self.prefs.save();
        self.refilter_packages(cx);
    }

    /// Packages opened lately in this folder, newest first.
    pub(crate) fn recent_packages(&self) -> Vec<String> {
        self.dir.as_deref().map(|d| self.prefs.folder(d).recent).unwrap_or_default()
    }

    /// Where the user is, for the next run (written on every move: a small file).
    pub(super) fn remember_place(&mut self) {
        let Some(dir) = self.dir.clone() else { return };
        let Some(place) = self.history.get(self.history_at).cloned() else { return };
        let f = self.prefs.folder_mut(&dir);
        match place {
            Place::Package { package, export, .. } => {
                f.view = Some("packages".into());
                if f.recent.first() != Some(&package) {
                    f.recent.retain(|p| *p != package);
                    f.recent.insert(0, package.clone());
                    f.recent.truncate(20);
                }
                f.package = Some(package);
                f.export = export;
            }
            Place::Assembly { .. } => f.view = Some("assembly".into()),
            Place::Mod => f.view = Some("mod".into()),
            Place::Map { .. } => f.view = Some("map".into()),
        }
        self.prefs.save();
    }

    /// Back where the user left this folder (unless turned off).
    pub(super) fn restore_session(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.dir.clone() else { return };
        if !self.prefs.restore {
            return;
        }
        let f = self.prefs.folder(&dir);
        if let Some(pkg) = f.package.filter(|p| self.packages.iter().any(|e| e.path == *p)) {
            let place = Place::package(pkg, f.export);
            self.record(place.clone());
            self.go_later(place, cx);
        }
        match f.view.as_deref() {
            Some("assembly") => self.pending_view = Some(View::Assembly),
            Some("mod") => self.pending_view = Some(View::Mod),
            Some("map") => self.pending_view = Some(View::Map),
            _ => {}
        }
    }

    pub(crate) fn copy_text(&mut self, text: String, what: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        self.set_status(format!("Copied the {what}: {text}"), cx);
    }

    pub(crate) fn reveal_file(&mut self, path: &str, cx: &mut Context<Self>) {
        cx.reveal_path(std::path::Path::new(path));
    }

    /// Bigger or smaller text (0: back to the default size).
    pub(crate) fn zoom_by(&mut self, step: f32, cx: &mut Context<Self>) {
        let z = if step == 0.0 { 1.0 } else { ((self.prefs.zoom.max(0.7) + step) * 10.0).round() / 10.0 };
        self.prefs.zoom = z.clamp(0.7, 1.6);
        self.prefs.save();
        prefs::apply_zoom(self.prefs.zoom, cx);
        self.set_status(format!("Text size {} %", (self.prefs.zoom * 100.0).round()), cx);
    }

    pub(super) fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let Some(studio) = me.upgrade() else { return dialog };
            let s = studio.read(cx);
            let p = s.prefs.clone();
            let dir = s.dir.clone();
            let pinned = s.pinned().len();
            let recent = s.recent.len();
            let mcp = s.mcp.clone();
            let section = |title: &'static str| div().pt_2().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(title.to_uppercase());
            let hint = |text: &'static str| div().text_xs().text_color(theme.muted_foreground).child(text);
            let theme_button = |id: &'static str, label: &'static str, icon: IconName, value: &'static str| {
                let me = me.clone();
                Button::new(id).small().icon(Icon::new(icon)).label(label).when(p.theme == value, |b| b.primary()).on_click(move |_, window, cx| {
                    prefs::theme_mode(value, Some(window), cx);
                    let _ = me.update(cx, |this, _| {
                        this.prefs.theme = value.into();
                        this.prefs.save();
                    });
                    window.refresh();
                })
            };
            let switch = |id: &'static str, label: &'static str, on: bool, set: fn(&mut prefs::Prefs, bool)| {
                let me = me.clone();
                Switch::new(id).checked(on).label(label).on_click(move |v, window, cx| {
                    let v = *v;
                    let _ = me.update(cx, |this, _| {
                        set(&mut this.prefs, v);
                        this.prefs.save();
                    });
                    window.refresh();
                })
            };
            let data = super::state_file("").map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            let (me1, me2, me3) = (me.clone(), me.clone(), me.clone());
            let body = v_flex()
                .gap_2()
                .child(section("Appearance"))
                .child(h_flex().gap_2().child(theme_button("theme-dark", "Dark", IconName::Moon, "dark")).child(theme_button("theme-light", "Light", IconName::Sun, "light")).child(theme_button("theme-system", "Like the system", IconName::Settings, "system")))
                .child({
                    let (z1, z2, z3) = (me.clone(), me.clone(), me.clone());
                    h_flex()
                        .gap_2()
                        .child(div().text_sm().child(format!("Text size {} %", (p.zoom.max(0.7) * 100.0).round())))
                        .child(Button::new("zoom-out").xsmall().label("A−").tooltip("⌘−").on_click(move |_, window, cx| {
                            let _ = z1.update(cx, |s, cx| s.zoom_by(-0.1, cx));
                            window.refresh();
                        }))
                        .child(Button::new("zoom-in").xsmall().label("A+").tooltip("⌘=").on_click(move |_, window, cx| {
                            let _ = z2.update(cx, |s, cx| s.zoom_by(0.1, cx));
                            window.refresh();
                        }))
                        .child(Button::new("zoom-reset").xsmall().ghost().label("Reset").tooltip("⌘0").on_click(move |_, window, cx| {
                            let _ = z3.update(cx, |s, cx| s.zoom_by(0.0, cx));
                            window.refresh();
                        }))
                })
                .child(section("Saving"))
                .child(switch("ask-in-place", "Ask before saving into the game folder", p.ask_in_place, |p, v| p.ask_in_place = v))
                .child(hint("Saving overwrites the game's files (the Studio keeps the originals for the mod cooker). Workspace files and Export a copy never touch the game folder."))
                .child(section("Starting"))
                .child(switch("restore", "Reopen the package and object I was on", p.restore, |p, v| p.restore = v))
                .child(hint("When a game folder opens, the Studio goes back to where you left it."))
                .child(section("Lists"))
                .child(
                    h_flex()
                        .gap_2()
                        .child(Button::new("forget-recent").small().label(format!("Forget the recent game folders ({recent})")).on_click(move |_, window, cx| {
                            let _ = me1.update(cx, |this, cx| {
                                this.recent.clear();
                                if let Some(f) = super::state_file("recent-workspaces") {
                                    let _ = std::fs::write(f, "");
                                }
                                if let Some(f) = super::state_file("last-workspace") {
                                    let _ = std::fs::remove_file(f);
                                }
                                this.toast(Tone::Info, "The recent game folders are forgotten", cx);
                            });
                            window.refresh();
                        }))
                        .when(dir.is_some(), |d| {
                            let dir = dir.clone().unwrap_or_default();
                            d.child(Button::new("forget-pins").small().label(format!("Unpin every package ({pinned})")).on_click(move |_, window, cx| {
                                let _ = me2.update(cx, |this, cx| {
                                    let f = this.prefs.folder_mut(&dir);
                                    f.pinned.clear();
                                    f.recent.clear();
                                    this.prefs.save();
                                    this.refilter_packages(cx);
                                });
                                window.refresh();
                            }))
                        }),
                )
                .child(section("MCP server (Claude and other agents)"))
                .child(match mcp.clone() {
                    Some((url, token)) => {
                        let cmd = format!("claude mcp add --transport http tempest-studio {url} --header \"Authorization: Bearer {token}\"");
                        let c2 = cmd.clone();
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().child(format!("Listening on {url}")))
                            .child(hint("Agents can read what the Studio shows, open packages and tables, run commands and call the backend. The token changes every run (also in mcp.json next to the settings)."))
                            .child(div().p_2().rounded_md().bg(theme.secondary).text_xs().font_family("Menlo").child(cmd))
                            .child(h_flex().child(Button::new("mcp-copy").xsmall().label("Copy the claude command").on_click(move |_, _, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(c2.clone())))))
                            .into_any_element()
                    }
                    None => hint("Not running: the port was taken (another Studio is open?). Set TEMPEST_STUDIO_MCP_PORT to use another.").into_any_element(),
                })
                .child(section("Updates"))
                .child(h_flex().gap_2().child(div().flex_1().text_sm().child(super::updates::describe())).child(Button::new("upd-check").small().label("Check now").on_click(|_, window, cx| {
                    super::updates::check(false, cx);
                    window.refresh();
                })))
                .child(switch("check-updates", "Look for a new version when the Studio starts", p.check_updates, |p, v| p.check_updates = v))
                .child(section("About"))
                .child(div().text_sm().child(format!("Tempest Studio (native) {}", env!("CARGO_PKG_VERSION"))))
                .child(h_flex().gap_2().child(div().flex_1().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(format!("Settings: {data}"))).child(Button::new("reveal-data").xsmall().ghost().icon(Icon::new(IconName::FolderOpen)).label("Show").on_click(move |_, _, cx| {
                    let data = data.clone();
                    let _ = me3.update(cx, |this, cx| this.reveal_file(&data, cx));
                })));
            dialog.title("Settings").w(px(560.0)).child(body)
        });
    }
}
