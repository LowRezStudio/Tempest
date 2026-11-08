//! Workspace files (`.twrk`): every unsaved package and assembly file of the
//! session plus where the user was, kept for later without touching the
//! game folder. Linked files hold deltas against the game folder (small);
//! portable files hold the changed files whole (open on another machine).
//! Opening one opens its game folder, puts the changes back as unsaved
//! edits and returns to the place it was saved at.

use gpui::{Context, PathPromptOptions};
use serde_json::{json, Value as J};

use super::{file_name, Place, Studio, Tone, View};

impl Studio {
    pub(super) fn twrk_state(&self) -> J {
        json!({
            "project": self.project,
            "native": true,
            "view": match self.view { View::Packages => "packages", View::Assembly => "assembly", View::Mod => "mod", View::Audio => "audio", View::Map => "map" },
            "package": self.selected_package,
            "export": self.selected_export,
        })
    }

    /// Ask where, then write the workspace file (`portable`: whole files).
    pub(super) fn save_workspace(&mut self, portable: bool, cx: &mut Context<Self>) {
        self.save_workspace_to(portable, None, cx);
    }

    /// `out` given (the driver), else asked for.
    pub(super) fn save_workspace_to(&mut self, portable: bool, out: Option<String>, cx: &mut Context<Self>) {
        let unsaved = self.dirty.len() + self.assembly.as_ref().map(|a| a.read(cx).unsaved_count()).unwrap_or(0);
        if unsaved == 0 {
            self.toast(Tone::Info, "Nothing is unsaved: a workspace file keeps unsaved edits", cx);
            return;
        }
        let docs = std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        let name = format!("{}.twrk", super::folder_label(self.dir.as_deref().unwrap_or("workspace")));
        let target = out.is_none().then(|| cx.prompt_for_new_path(&docs, Some(&name)));
        let state = self.twrk_state();
        cx.spawn(async move |this, cx| {
            let mut out = match (out, target) {
                (Some(o), _) => o,
                (None, Some(t)) => {
                    let Ok(Ok(Some(o))) = t.await else { return };
                    o.to_string_lossy().to_string()
                }
                _ => return,
            };
            if !out.to_ascii_lowercase().ends_with(".twrk") {
                out.push_str(".twrk");
            }
            let Ok(call) = this.update(cx, |this, cx| {
                this.set_status("Writing the workspace file…", cx);
                this.backend.call("twrk.save", json!({ "out": out, "mode": if portable { "portable" } else { "linked" }, "state": state }))
            }) else { return };
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    let n = r.json["files"].as_array().map(|f| f.len()).unwrap_or(0);
                    this.toast(Tone::Success, format!("Wrote {} ({n} files, {}); the edits stay unsaved here", file_name(&out), crate::model::human_size(r.json["size"].as_u64().unwrap_or(0))), cx);
                    for w in r.json["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()).take(3) {
                        this.toast(Tone::Warning, w.to_string(), cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("Could not write the workspace file: {e}"), cx),
            });
        })
        .detach();
    }

    pub(super) fn pick_workspace_file(&mut self, cx: &mut Context<Self>) {
        let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Open a workspace file (.twrk)".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut p))) = files.await else { return };
            if let Some(f) = p.pop() {
                let _ = this.update(cx, |this, cx| this.open_workspace_file(f.to_string_lossy().to_string(), cx));
            }
        })
        .detach();
    }

    /// Read the file, open its game folder when it is another, apply the changes, restore the place.
    pub(crate) fn open_workspace_file(&mut self, path: String, cx: &mut Context<Self>) {
        let read = self.backend.call("twrk.read", json!({ "path": path }));
        let current = self.dir.clone();
        cx.spawn(async move |this, cx| {
            let r = match read.await {
                Ok(r) => r,
                Err(e) => {
                    let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not read {}: {e}", file_name(&path)), cx));
                    return;
                }
            };
            let game = r.json["gameDir"].as_str().unwrap_or_default().to_string();
            let state = r.json["state"].clone();
            if !game.is_empty() && current.as_deref() != Some(game.as_str()) {
                if !std::path::Path::new(&game).is_dir() {
                    let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Its game folder {game} is not here: open that game folder first, or use a portable workspace file"), cx));
                    return;
                }
                let Ok(open) = this.update(cx, |this, _| this.backend.call("workspace.open", json!({ "dir": game }))) else { return };
                let opened = open.await;
                let ok = this.update(cx, |this, cx| match opened {
                    Ok(o) => {
                        this.packages = crate::model::packages(&o.json["packages"]);
                        this.dir = Some(game.clone());
                        this.open = None;
                        this.clear_selection();
                        this.selected_package = None;
                        this.dirty.clear();
                        this.assembly = None;
                        this.cooker = None;
                        this.refilter_packages(cx);
                        true
                    }
                    Err(e) => {
                        this.toast(Tone::Error, format!("Could not open {game}: {e}"), cx);
                        false
                    }
                });
                if !matches!(ok, Ok(true)) {
                    return;
                }
            }
            let Ok(apply) = this.update(cx, |this, cx| {
                this.set_status(format!("Applying {}…", file_name(&path)), cx);
                this.backend.call("twrk.apply", json!({ "path": path }))
            }) else { return };
            let applied = apply.await;
            let _ = this.update(cx, |this, cx| match applied {
                Ok(a) => {
                    let mut n = 0;
                    for p in a.json["packages"].as_array().into_iter().flatten() {
                        if let Some(pp) = p["path"].as_str() {
                            this.dirty.insert(pp.to_string());
                            n += 1;
                        }
                    }
                    for p in a.json["pcks"].as_array().into_iter().flatten().filter_map(|p| p.as_str()) {
                        this.dirty.insert(p.to_string());
                    }
                    let docs = a.json["docs"].as_array().map(|d| d.len()).unwrap_or(0);
                    this.applied_docs |= docs > 0;
                    this.adopt_project(&path, &state);
                    match &this.project {
                        Some(p) if p.file == path => {
                            let name = p.name.clone();
                            this.toast(Tone::Success, format!("Opened the project {name}: {n} packages and {docs} assembly files with edits"), cx)
                        }
                        _ => this.toast(Tone::Success, format!("Opened {}: {n} packages and {docs} assembly files with unsaved edits", file_name(&path)), cx),
                    }
                    // Back where it was saved.
                    if let Some(pkg) = state["package"].as_str() {
                        let place = Place::package(pkg, state["export"].as_u64().map(|i| i as usize));
                        this.record(place.clone());
                        this.selected_package = None;
                        this.go_later(place, cx);
                    }
                    if let Some(a) = &this.assembly {
                        a.update(cx, |a, cx| a.reopen(cx));
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("Could not apply {}: {e}", file_name(&path)), cx),
            });
        })
        .detach();
    }

    /// `<config>/tempest-studio-native/recovery/<hash of the folder>.twrk`.
    pub(super) fn recovery_file(dir: &str) -> Option<std::path::PathBuf> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        dir.hash(&mut h);
        super::state_file(&format!("recovery/{:016x}.twrk", h.finish()))
    }

    /// Every two minutes with unsaved edits and no project, a linked
    /// recovery copy; deleted once nothing is unsaved.
    pub(super) fn watch_recovery(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_secs(120)).await;
            let alive = this.update(cx, |this, cx| {
                let Some(dir) = this.dir.clone() else { return };
                let Some(file) = Self::recovery_file(&dir) else { return };
                if this.unsaved_total(cx) == 0 || this.project.is_some() {
                    if file.exists() && this.recovery_offer.is_none() {
                        let _ = std::fs::remove_file(&file);
                    }
                    return;
                }
                let _ = std::fs::create_dir_all(file.parent().unwrap());
                let call = this.backend.call("twrk.save", json!({ "out": file.to_string_lossy(), "mode": "linked", "state": this.twrk_state() }));
                cx.background_executor().spawn(async move { let _ = call.await; }).detach();
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    /// When a folder opens: a recovery copy left by a run that did not save is offered.
    pub(super) fn check_recovery(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.dir.as_deref().and_then(Self::recovery_file).filter(|f| f.exists()) else { return };
        let when = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
        let ago = when.and_then(|w| w.elapsed().ok()).map(|d| {
            let m = d.as_secs() / 60;
            if m < 60 { format!("{m} min ago") } else if m < 48 * 60 { format!("{} h ago", m / 60) } else { format!("{} days ago", m / 1440) }
        });
        self.recovery_offer = Some((file.to_string_lossy().to_string(), ago.unwrap_or_default()));
        cx.notify();
    }

    pub(super) fn render_recovery(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        use gpui::{div, prelude::*};
        use gpui_component::button::{Button, ButtonVariants};
        use gpui_component::{h_flex, ActiveTheme, Sizable};
        let (file, ago) = self.recovery_offer.clone()?;
        let theme = cx.theme().clone();
        let f2 = file.clone();
        Some(
            h_flex()
                .px_3()
                .py_1()
                .gap_3()
                .bg(theme.warning.opacity(0.15))
                .border_b_1()
                .border_color(theme.border)
                .text_sm()
                .child(div().flex_1().child(format!("Unsaved work from an earlier session was kept ({ago}). Put it back?")))
                .child(Button::new("rec-open").xsmall().primary().label("Restore it").on_click(cx.listener(move |this, _, _, cx| {
                    this.recovery_offer = None;
                    this.open_workspace_file(file.clone(), cx);
                })))
                .child(Button::new("rec-drop").xsmall().ghost().label("Throw it away").on_click(cx.listener(move |this, _, _, cx| {
                    this.recovery_offer = None;
                    let _ = std::fs::remove_file(&f2);
                    cx.notify();
                })))
                .into_any_element(),
        )
    }
}
