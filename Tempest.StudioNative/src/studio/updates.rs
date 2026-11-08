//! Updates (`crate::update`): checked a few seconds after start (first
//! window, bundled builds, unless turned off in Settings), shown as a banner
//! in every window and in Settings. "Restart to update" downloads, keeps the
//! work of every window (a project saves its file, other unsaved edits go to
//! the recovery workspace file the next run offers), swaps the new build in
//! and quits; the helper starts the new one.

use std::sync::Mutex;

use gpui::{div, prelude::*, App, Context, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{h_flex, ActiveTheme, Sizable};
use serde_json::json;

use super::{Studio, Tone};
use crate::update::{self, Release};

#[derive(Clone, Debug, Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    /// Downloading, the fraction done.
    Downloading(Release, f32),
    /// Keeping the work and swapping the new build in.
    Restarting(Release),
    Failed(String),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);
/// "Later": the banner stays away until the next start.
static DISMISSED: Mutex<bool> = Mutex::new(false);

pub fn state() -> State {
    STATE.lock().map(|s| s.clone()).unwrap_or_default()
}

fn set(s: State, cx: &mut App) {
    if let Ok(mut st) = STATE.lock() {
        *st = s;
    }
    cx.refresh_windows();
}

/// The automatic check at start: once per run, bundled builds only.
pub(super) fn check_at_start(enabled: bool, cx: &mut App) {
    if !enabled || update::install().is_none() || !matches!(state(), State::Idle) {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(std::time::Duration::from_secs(5)).await;
        cx.update(|cx| check(true, cx));
    })
    .detach();
}

/// Ask GitHub; `quiet`: a failure leaves the state idle (no banner for being offline).
pub fn check(quiet: bool, cx: &mut App) {
    if matches!(state(), State::Checking | State::Downloading(..) | State::Restarting(_)) {
        return;
    }
    set(State::Checking, cx);
    cx.spawn(async move |cx| {
        let r = cx.background_executor().spawn(async { update::check() }).await;
        cx.update(|cx| match r {
            Ok(Some(rel)) => set(State::Available(rel), cx),
            Ok(None) => set(State::UpToDate, cx),
            Err(_) if quiet => set(State::Idle, cx),
            Err(e) => set(State::Failed(e), cx),
        });
    })
    .detach();
}

/// Every open Studio.
fn studios(cx: &mut App) -> Vec<gpui::Entity<Studio>> {
    let handles = super::WINDOWS.lock().map(|w| w.clone()).unwrap_or_default();
    handles
        .into_iter()
        .filter_map(|w| w.update(cx, |root, _, cx| root.downcast::<gpui_component::Root>().ok().and_then(|r| r.read(cx).view().clone().downcast::<Studio>().ok())).ok().flatten())
        .collect()
}

impl Studio {
    /// Keep this window's work for a restart: the project file, else the
    /// recovery file when anything is unsaved. `None`: nothing to keep.
    fn keep_for_restart(&mut self, cx: &mut Context<Self>) -> Option<impl std::future::Future<Output = Result<crate::backend::Reply, String>> + use<>> {
        let dir = self.dir.clone()?;
        let out = if self.project.is_some() {
            let mut edited: Vec<String> = self.dirty.iter().cloned().collect();
            edited.extend(self.assembly.as_ref().map(|a| a.read(cx).unsaved_files()).unwrap_or_default());
            let p = self.project.as_mut()?;
            p.note_files(edited);
            p.file.clone()
        } else if self.unsaved_total(cx) > 0 {
            let file = Self::recovery_file(&dir)?;
            let _ = std::fs::create_dir_all(file.parent()?);
            file.to_string_lossy().to_string()
        } else {
            return None;
        };
        Some(self.backend.call("twrk.save", json!({ "out": out, "mode": "linked", "state": self.twrk_state() })))
    }

    /// Download the update, keep every window's work, swap it in and quit.
    pub(super) fn restart_to_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Available(rel) = state() else { return };
        let Some(install) = update::install().filter(|_| update::can_update()) else {
            cx.open_url(&rel.page);
            return;
        };
        let unsaved: usize = studios(cx).iter().map(|s| s.read(cx).unsaved_total(cx)).sum();
        let body = if unsaved > 0 {
            format!("The Studio downloads version {}, closes and opens again. Your {unsaved} unsaved file(s) are kept: a project saves its file, other edits go to a recovery file the Studio offers when it reopens.", rel.version)
        } else {
            format!("The Studio downloads version {}, closes and opens again.", rel.version)
        };
        let notes: String = rel.notes.chars().take(800).collect();
        let body = if notes.is_empty() { body } else { format!("{body}\n\nWhat's new:\n{notes}{}", if rel.notes.chars().count() > 800 { "…" } else { "" }) };
        let me = cx.entity().downgrade();
        super::dialogs::confirm(window, cx, "Update Tempest Studio", body, "Restart to update", false, move |_, cx| {
            let _ = me.update(cx, |this, cx| this.run_update(rel.clone(), install.clone(), cx));
        });
    }

    fn run_update(&mut self, rel: Release, install: update::Install, cx: &mut Context<Self>) {
        set(State::Downloading(rel.clone(), 0.0), cx);
        let progress = std::sync::Arc::new(Mutex::new(0f32));
        let (p2, r2) = (progress.clone(), rel.clone());
        let task = cx.background_executor().spawn(async move {
            update::download(&r2, &install, |f| {
                if let Ok(mut p) = p2.lock() {
                    *p = f;
                }
            })
        });
        cx.spawn(async move |this, cx| {
            let mut task = std::pin::pin!(task);
            // Progress every 150 ms until the download is done.
            let staged = loop {
                let tick = cx.background_executor().timer(std::time::Duration::from_millis(150));
                match futures::future::select(task.as_mut(), tick).await {
                    futures::future::Either::Left((r, _)) => break r,
                    futures::future::Either::Right(_) => {
                        let f = progress.lock().map(|p| *p).unwrap_or(0.0);
                        cx.update(|cx| set(State::Downloading(rel.clone(), f), cx));
                    }
                }
            };
            let staged = match staged {
                Ok(s) => s,
                Err(e) => {
                    cx.update(|cx| set(State::Failed(e), cx));
                    return;
                }
            };
            cx.update(|cx| set(State::Restarting(rel.clone()), cx));
            // Every window's work, then the swap.
            let saves = cx.update(|cx| studios(cx).into_iter().filter_map(|s| s.update(cx, |s, cx| s.keep_for_restart(cx))).collect::<Vec<_>>());
            for r in futures::future::join_all(saves).await {
                if let Err(e) = r {
                    let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("The update was stopped: your work could not be kept ({e})"), cx));
                    cx.update(|cx| set(State::Available(rel.clone()), cx));
                    return;
                }
            }
            match cx.background_executor().spawn(async move { update::apply(staged) }).await {
                Ok(()) => {
                    cx.update(|cx| cx.quit());
                }
                Err(e) => {
                    cx.update(|cx| set(State::Failed(e), cx));
                }
            }
        })
        .detach();
    }

    /// The banner under the header while an update is available, downloading or failed.
    pub(super) fn render_update_banner(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if DISMISSED.lock().map(|d| *d).unwrap_or(false) {
            return None;
        }
        let theme = cx.theme().clone();
        let bar = |tint: gpui::Hsla| h_flex().px_3().py_1().gap_3().bg(tint.opacity(0.15)).border_b_1().border_color(theme.border).text_sm();
        let later = Button::new("upd-later").xsmall().ghost().label("Later").on_click(cx.listener(|_, _, _, cx| {
            if let Ok(mut d) = DISMISSED.lock() {
                *d = true;
            }
            cx.refresh_windows();
        }));
        let el = match state() {
            State::Available(rel) => {
                let installable = update::can_update() && rel.asset.is_some();
                let page = rel.page.clone();
                bar(theme.info)
                    .child(div().flex_1().child(format!("Tempest Studio {} is out (you have {}).", rel.version, update::current())))
                    .child(Button::new("upd-notes").xsmall().ghost().label("What's new").on_click(move |_, _, cx| cx.open_url(&page)))
                    .child(div().relative().child(if installable {
                        Button::new("upd-go").xsmall().primary().label("Restart to update").on_click(cx.listener(|this, _, window, cx| this.restart_to_update(window, cx)))
                    } else {
                        let page = rel.page.clone();
                        Button::new("upd-go").xsmall().primary().label("Download").tooltip("This copy cannot update itself: download the new one").on_click(move |_, _, cx| cx.open_url(&page))
                    }).child(crate::driver::probe("upd-go")))
                    .child(later)
            }
            State::Downloading(rel, f) => bar(theme.info).child(div().flex_1().child(format!("Downloading Tempest Studio {}… {} %", rel.version, (f * 100.0).round()))),
            State::Restarting(rel) => bar(theme.info).child(div().flex_1().child(format!("Keeping your work and installing Tempest Studio {}…", rel.version))),
            State::Failed(e) => bar(theme.danger)
                .child(div().flex_1().child(format!("The update failed: {e}")))
                .child(Button::new("upd-retry").xsmall().label("Try again").on_click(|_, _, cx| check(false, cx)))
                .child(later),
            State::Idle | State::Checking | State::UpToDate => return None,
        };
        Some(el.into_any_element())
    }
}

/// The Settings section's line: the version and what the last check found.
pub(super) fn describe() -> String {
    let v = update::current();
    let place = if update::install().is_some() { "" } else { " (a development build: it does not update itself)" };
    match state() {
        State::Idle => format!("Version {v}{place}."),
        State::Checking => format!("Version {v}. Checking…"),
        State::UpToDate => format!("Version {v}: the newest."),
        State::Available(r) => format!("Version {v}. Version {} is out.", r.version),
        State::Downloading(r, f) => format!("Version {v}. Downloading {}: {} %", r.version, (f * 100.0).round()),
        State::Restarting(r) => format!("Installing {}…", r.version),
        State::Failed(e) => format!("Version {v}. The last check or update failed: {e}"),
    }
}
