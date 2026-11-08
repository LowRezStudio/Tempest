//! Wwise sounds: an AkEvent or AkBank export previews what it plays (the
//! bank's `.pck` in the game folder, the event's play actions followed to
//! their media). Every media file plays, exports (.ogg/.wav/.wem) and is
//! replaced from a file (re-encoded to Wwise Vorbis); a replaced medium is
//! an unsaved edit of the `.pck`, saved like a package.

use std::sync::Arc;

use gpui::{div, prelude::*, AnyElement, Context, PathPromptOptions};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{h_flex, v_flex, ActiveTheme, Sizable};
use serde_json::{json, Value as J};

use super::{file_name, Studio, Tone};

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
                    Err(e) => this.set_status(format!("Wwise: {e}"), cx),
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
                            this.toast(Tone::Error, e, cx);
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
                    Err(e) => this.toast(Tone::Error, format!("Could not play it: {e}"), cx),
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
                Err(e) => this.toast(Tone::Error, format!("Export failed: {e}"), cx),
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
            let Ok(call) = this.update(cx, |this, cx| {
                this.set_status("Encoding…", cx);
                this.backend.call("wwise.import", json!({ "path": pck, "id": id, "file": file }))
            }) else { return };
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.dirty.insert(pck.clone());
                    this.toast(Tone::Success, format!("Replaced medium {id} with {} ({}); {} is unsaved", file_name(&file), crate::model::human_size(r.json["bytes"].as_u64().unwrap_or(0)), file_name(&pck)), cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not replace it: {e}"), cx),
            });
        })
        .detach();
    }

    pub(super) fn render_wwise(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let playing = self.player.playing();
        let w = self.wwise.as_ref().filter(|w| Some(w.export) == self.selected_export)?;
        let info = &w.info;
        let pck = info["pck"].as_str().map(file_name);
        let mut body = v_flex().gap_1().child(div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("WWISE"));
        body = body.child(div().text_sm().child(match &pck {
            Some(p) => format!("Bank {} in {p}", info["bank"].as_str().unwrap_or_default()),
            None => format!("Bank {}: no {}.pck in the game folder", info["bank"].as_str().unwrap_or_default(), info["bank"].as_str().unwrap_or_default()),
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
                    .gap_2()
                    .text_sm()
                    .child(Button::new(("ww-play", k)).xsmall().when(now, |b| b.primary()).label(if now { "Stop" } else { "Play" }).on_click(cx.listener(move |this, _, _, cx| this.wwise_play(id, cx))))
                    .child(div().font_family("Menlo").text_xs().child(id.to_string()))
                    .child(div().flex_1().text_xs().text_color(theme.muted_foreground).child(format!(
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
