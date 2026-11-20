//! Wwise sounds: an AkEvent or AkBank export previews what it plays (the
//! bank's `.pck` in the game folder, the event's play actions followed to
//! their media). Every media file plays, exports (.ogg/.wav/.wem) and is
//! replaced from a file (re-encoded to Wwise Vorbis); a replaced medium is
//! an undoable unsaved edit of the `.pck`, saved like a package.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyElement, Context, PathPromptOptions};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Selectable, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{file_name, Studio, Tone};

static WWISE_UNDO_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub(super) struct WwiseStep {
    pub pck: String,
    pub id: u64,
    pub before_wem: PathBuf,
    pub after_wem: PathBuf,
    pub was_clean: bool,
}

fn wwise_undo_paths(id: u64) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join("tempest-studio-wwise-undo");
    let _ = std::fs::create_dir_all(&dir);
    let seq = WWISE_UNDO_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    (dir.join(format!("{pid}-{seq}-{id}-before.wem")), dir.join(format!("{pid}-{seq}-{id}-after.wem")))
}

pub struct WwiseState {
    pub export: usize,
    info: J,
}

pub fn is_wwise(class: &str) -> bool {
    matches!(class.to_ascii_lowercase().as_str(), "akevent" | "akbank")
}

impl Studio {
    pub(super) fn load_wwise(&mut self, path: String, i: usize, cx: &mut Context<Self>) {
        let call = self.backend.call("wwise.forObject", json!({ "path": path, "index": i }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.selected_export != Some(i) {
                    return;
                }
                match r {
                    Ok(r) => this.wwise = Some(WwiseState { export: i, info: r.json }),
                    Err(e) => this.toast(Tone::Error, super::ui::explain_error(&format!("Could not read sound bank info: {e}")), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn wwise_play(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(pck) = self.wwise.as_ref().and_then(|w| w.info["pck"].as_str().map(|s| s.to_string())) else { return };
        self.wwise_play_in(pck, id, cx);
    }

    /// Play medium `id` of `.pck` `pck` (again: stop).
    pub(super) fn wwise_play_in(&mut self, pck: String, id: u64, cx: &mut Context<Self>) {
        if self.player.playing() && self.ww_playing == Some(id) {
            self.player.stop();
            cx.notify();
            return;
        }
        let call = self.backend.call("wwise.preview", json!({ "path": pck, "id": id }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let ogg = match &r {
                Ok(r) => r.bin(&r.json["ogg"]).map(|b| b.to_vec()),
                Err(_) => None,
            };
            let busy = crate::backend::Busy::new();
            let pcm = match ogg {
                Some(ogg) => cx.background_executor().spawn(async move { tempest_assets::audio::decode_ogg(&ogg).map_err(|e| e.to_string()) }).await,
                None => Err(r.err().unwrap_or_else(|| "no audio".into())),
            };
            let _ = this.update(cx, |this, cx| {
                drop(busy);
                match pcm {
                    Ok(pcm) => {
                        let pcm = Arc::new(pcm);
                        if let Err(e) = this.player.play(&pcm, 0.0, None) {
                            this.toast(Tone::Error, super::ui::explain_error(&e), cx);
                        }
                        this.ww_playing = Some(id);
                        cx.spawn(async move |this, cx| loop {
                            cx.background_executor().timer(crate::audio::TICK * 4).await;
                            let Ok(p) = this.update(cx, |this, cx| {
                                cx.notify();
                                this.player.position().is_some()
                            }) else { break };
                            if !p {
                                break;
                            }
                        })
                        .detach();
                    }
                    Err(e) => this.toast(Tone::Error, super::ui::explain_error(&format!("Could not play it: {e}")), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn wwise_export(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(pck) = self.wwise.as_ref().and_then(|w| w.info["pck"].as_str().map(|s| s.to_string())) else { return };
        self.wwise_export_in(pck, id, cx);
    }

    pub(super) fn wwise_export_in(&mut self, pck: String, id: u64, cx: &mut Context<Self>) {
        let docs = std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        let target = cx.prompt_for_new_path(&docs, Some(&format!("{id}.ogg")));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(out))) = target.await else { return };
            let Ok(call) = this.update(cx, |this, _| this.backend.call("wwise.export", json!({ "path": pck, "id": id, "out": out.to_string_lossy() }))) else { return };
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => this.toast(Tone::Success, format!("Exported {} ({})", out.display(), crate::model::human_size(r.json["bytes"].as_u64().unwrap_or(0))), cx),
                Err(e) => this.toast(Tone::Error, super::ui::explain_error(&format!("Export failed: {e}")), cx),
            });
        })
        .detach();
    }

    /// Batch-export every medium in `ids` from `pck` to a chosen folder as `.wav`, `.ogg`, or `.wem`.
    pub(super) fn wwise_export_all(&mut self, pck: String, mut ids: Vec<u64>, ext: &'static str, cx: &mut Context<Self>) {
        let mut seen = HashSet::new();
        ids.retain(|id| seen.insert(*id));
        if pck.is_empty() || ids.is_empty() {
            return;
        }
        let total = ids.len();
        let sound_noun = if total == 1 { "sound" } else { "sounds" };
        let pick = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(format!("Export {total} {sound_noun} (.{ext}) here").into()),
        });
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut dirs))) = pick.await else { return };
            let Some(dir) = dirs.pop() else { return };
            let busy = crate::backend::Busy::new();
            let bank_name = file_name(&pck);
            let _ = this.update(cx, |this, cx| this.set_status(format!("Exporting {total} {sound_noun} from {bank_name}…"), cx));
            let mut ok = 0usize;
            let mut failed = 0usize;
            let mut first_err: Option<String> = None;
            for (idx, id) in ids.iter().copied().enumerate() {
                let out = dir.join(format!("{id}.{ext}"));
                match backend.call("wwise.export", json!({ "path": pck, "id": id, "out": out.to_string_lossy() })).await {
                    Ok(_) => ok += 1,
                    Err(e) => {
                        failed += 1;
                        if first_err.is_none() {
                            first_err = Some(e);
                        }
                    }
                }
                if (idx + 1) % 20 == 0 && idx + 1 < total {
                    let done = idx + 1;
                    let name = bank_name.clone();
                    if this.update(cx, |this, cx| this.set_status(format!("Exporting {name}: {done} / {total}…"), cx)).is_err() {
                        return;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                drop(busy);
                match (ok, failed, first_err) {
                    (n, 0, _) => this.toast(
                        Tone::Success,
                        format!("Exported {n} .{ext} {} from {bank_name} to {}", if n == 1 { "file" } else { "files" }, dir.display()),
                        cx,
                    ),
                    (n, f, Some(e)) if n > 0 => this.toast(
                        Tone::Warning,
                        format!("Exported {n} of {total} {} ({f} failed: {})", if total == 1 { "file" } else { "files" }, super::ui::explain_error(&e)),
                        cx,
                    ),
                    (_, _, Some(e)) => this.toast(Tone::Error, super::ui::explain_error(&format!("Could not export {bank_name}: {e}")), cx),
                    _ => {}
                }
            });
        })
        .detach();
    }

    fn wwise_replace(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(pck) = self.wwise.as_ref().and_then(|w| w.info["pck"].as_str().map(|s| s.to_string())) else { return };
        self.wwise_replace_in(pck, id, cx);
    }

    pub(super) fn wwise_replace_in(&mut self, pck: String, id: u64, cx: &mut Context<Self>) {
        let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Replace with an .ogg, .wav or .wem file".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut picked))) = files.await else { return };
            let Some(file) = picked.pop().map(|f| f.to_string_lossy().to_string()) else { return };
            let _ = this.update(cx, |this, cx| this.wwise_replace_with_file(pck, id, file, cx));
        })
        .detach();
    }

    pub(super) fn wwise_replace_with_file(&mut self, pck: String, id: u64, file: String, cx: &mut Context<Self>) {
        if self.player.playing() && self.ww_playing == Some(id) {
            self.player.stop();
        }
        let was_clean = !self.dirty.iter().any(|p| super::project::same_path(p, &pck));
        let (before_wem, after_wem) = wwise_undo_paths(id);
        let backend = self.backend.clone();
        self.set_status("Encoding…", cx);
        cx.spawn(async move |this, cx| {
            let before_ok = backend
                .call("wwise.export", json!({ "path": pck, "id": id, "format": "wem", "out": before_wem.to_string_lossy() }))
                .await
                .is_ok();
            let r = backend.call("wwise.import", json!({ "path": pck, "id": id, "file": file })).await;
            let after_ok = if r.is_ok() && before_ok {
                backend
                    .call("wwise.export", json!({ "path": pck, "id": id, "format": "wem", "out": after_wem.to_string_lossy() }))
                    .await
                    .is_ok()
            } else {
                false
            };
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.dirty.insert(pck.clone());
                    this.seen_dirty.insert(pck.clone());
                    if before_ok && after_ok {
                        this.wwise_undo.push(WwiseStep { pck: pck.clone(), id, before_wem, after_wem, was_clean });
                        this.wwise_redo.clear();
                        this.last_undo_was_wwise = true;
                        this.last_undo_was_assembly = false;
                    }
                    this.refresh_wwise_views(&pck, cx);
                    this.toast(
                        Tone::Success,
                        format!("Replaced medium {id} with {} ({}); {} is edited", file_name(&file), crate::model::human_size(r.json["bytes"].as_u64().unwrap_or(0)), file_name(&pck)),
                        cx,
                    );
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&before_wem);
                    this.toast(Tone::Error, super::ui::explain_error(&format!("Could not replace it: {e}")), cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn can_step_wwise(&self, redo: bool) -> bool {
        if redo { !self.wwise_redo.is_empty() } else { !self.wwise_undo.is_empty() }
    }

    pub(super) fn step_wwise(&mut self, redo: bool, cx: &mut Context<Self>) -> bool {
        let prefer = if self.view == super::View::Audio { self.open_audio_path().map(str::to_string) } else { None };
        let pop_from = |stack: &mut Vec<WwiseStep>| -> Option<WwiseStep> {
            if let Some(cur) = prefer.as_deref() {
                if let Some(pos) = stack.iter().rposition(|s| super::project::same_path(&s.pck, cur)) {
                    return Some(stack.remove(pos));
                }
            }
            stack.pop()
        };
        let Some(step) = (if redo { pop_from(&mut self.wwise_redo) } else { pop_from(&mut self.wwise_undo) }) else {
            return false;
        };
        if self.player.playing() && self.ww_playing == Some(step.id) {
            self.player.stop();
        }
        let should_discard = !redo && step.was_clean && !self.wwise_undo.iter().any(|s| super::project::same_path(&s.pck, &step.pck));
        if should_discard {
            self.dirty.retain(|p| !super::project::same_path(p, &step.pck));
            self.seen_dirty.retain(|p| !super::project::same_path(p, &step.pck));
        } else {
            self.dirty.insert(step.pck.clone());
            self.seen_dirty.insert(step.pck.clone());
        }
        if redo {
            self.wwise_undo.push(step.clone());
        } else {
            self.wwise_redo.push(step.clone());
        }
        self.last_undo_was_wwise = true;
        self.last_undo_was_assembly = false;
        let call = if should_discard {
            self.backend.call("package.discard", json!({ "path": step.pck }))
        } else {
            let wem = if redo { &step.after_wem } else { &step.before_wem };
            self.backend.call("wwise.import", json!({ "path": step.pck, "id": step.id, "file": wem.to_string_lossy() }))
        };
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(_) => {
                    this.refresh_wwise_views(&step.pck, cx);
                    this.set_status(format!("{} sound replacement in {}", if redo { "Redone" } else { "Undone" }, file_name(&step.pck)), cx);
                }
                Err(e) => {
                    this.toast(Tone::Error, super::ui::explain_error(&format!("Could not {} sound replacement: {e}", if redo { "redo" } else { "undo" })), cx);
                }
            });
        })
        .detach();
        true
    }

    pub(super) fn refresh_wwise_views(&mut self, pck: &str, cx: &mut Context<Self>) {
        self.refresh_audio_open(pck, cx);
        if self.wwise.as_ref().and_then(|w| w.info["pck"].as_str()).is_some_and(|wp| super::project::same_path(wp, pck)) {
            if let (Some(pkg_path), Some(exp)) = (self.open.as_ref().map(|t| t.path.clone()), self.selected_export) {
                self.load_wwise(pkg_path, exp, cx);
            }
        }
        if let Some(c) = self.cooker.clone() {
            c.update(cx, |c, cx| c.refresh(cx));
        }
    }

    pub(super) fn render_wwise(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let playing = self.player.playing();
        let w = self.wwise.as_ref().filter(|w| Some(w.export) == self.selected_export)?;
        let info = &w.info;
        // A language folder's package (voice lines) is named with its folder.
        let pck = info["pck"].as_str().map(|p| {
            let path = std::path::Path::new(p);
            let in_folder = self.open.as_ref().and_then(|t| std::path::Path::new(&t.path).parent().map(|d| path.parent() == Some(d))).unwrap_or(true);
            match path.parent().and_then(|d| d.file_name()).filter(|_| !in_folder) {
                Some(folder) => format!("{}/{}", folder.to_string_lossy(), file_name(p)),
                None => file_name(p),
            }
        });
        let bank = info["bank"].as_str().unwrap_or_default();
        let mut body = v_flex().gap_1().child(super::ui::section("Sound bank", &theme));
        body = body.child(div().text_sm().child(match &pck {
            Some(p) => format!("Sound bank {bank} in {p}"),
            None => format!("Sound bank {bank}: {bank}.pck was not found in this game folder or its language folders"),
        }));
        if info["found"] == false {
            body = body.child(div().text_xs().text_color(theme.warning).child("The event was not found in its bank"));
        }
        let media = info["media"].as_array().cloned().unwrap_or_default();
        if media.is_empty() && pck.is_some() {
            body = body.child(div().text_xs().text_color(theme.muted_foreground).child("It plays no sound file (a stop, a state change, or media of another bank)"));
        }
        for (k, m) in media.into_iter().enumerate() {
            let id = m["id"].as_u64().unwrap_or(0);
            let now = playing && self.ww_playing == Some(id);
            body = body.child(
                h_flex()
                    .min_h(px(super::ui::ROW))
                    .py_0p5()
                    .gap_2()
                    .flex_wrap()
                    .text_sm()
                    .child(Button::new(("ww-play", k)).xsmall().selected(now).icon(Icon::new(if now { IconName::Square } else { IconName::Play })).label(if now { "Stop" } else { "Play" }).on_click(cx.listener(move |this, _, _, cx| this.wwise_play(id, cx))))
                    .child(div().font_family(crate::studio::ui::mono()).text_xs().child(id.to_string()))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().text_color(theme.muted_foreground).child(format!(
                        "{} · {} ch · {} Hz · {:.2} s · {}{}",
                        m["codec"].as_str().unwrap_or_default(),
                        m["channels"],
                        m["sampleRate"],
                        m["duration"].as_f64().unwrap_or(0.0),
                        crate::model::human_size(m["bytes"].as_u64().unwrap_or(0)),
                        if m["streamed"].as_bool().unwrap_or(false) { " · streamed" } else { "" }
                    )))
                    .child(Button::new(("ww-exp", k)).xsmall().ghost().label("Export…").on_click(cx.listener(move |this, _, _, cx| this.wwise_export(id, cx))))
                    .child(Button::new(("ww-rep", k)).xsmall().ghost().label("Replace…").tooltip("An .ogg, .wav or .wem file, re-encoded to Wwise Vorbis").on_click(cx.listener(move |this, _, _, cx| this.wwise_replace(id, cx)))),
            );
        }
        Some(body.into_any_element())
    }
}
