//! Sounds (SoundNodeWave): the stored Ogg Vorbis decoded, drawn as a
//! waveform, played from the start or the selection; and the editor: trim
//! to the selection, fades, gain, normalise, mono/stereo, quality, heard
//! before it is applied, then re-encoded into the package
//! (`sound.replacePcm`, undoable, saved with the package).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use base64::Engine;
use gpui::{canvas, div, fill, point, prelude::*, px, size, AnyElement, Bounds, Context, MouseButton, Pixels};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use serde_json::json;
use tempest_assets::audio::{apply_edits, decode_ogg, Edits, Pcm};

use super::{Studio, Tone};

pub struct SoundState {
    pub export: usize,
    original: Arc<Pcm>,
    /// The original with the edits applied (`None` while nothing is edited).
    edited: Option<Arc<Pcm>>,
    /// Min/max per column of the shown sound.
    peaks: Vec<(f32, f32)>,
    bytes: u64,
    pub selection: Option<(f64, f64)>,
    drag: Option<f64>,
    editing: bool,
    edits: Edits,
    quality: f32,
    /// The waveform's bounds at the last paint (mouse → time).
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

const COLUMNS: usize = 1200;

fn peaks(pcm: &Pcm) -> Vec<(f32, f32)> {
    let n = pcm.frames();
    if n == 0 {
        return Vec::new();
    }
    let per = n.div_ceil(COLUMNS).max(1);
    (0..n.div_ceil(per))
        .map(|c| {
            let (a, b) = (c * per, ((c + 1) * per).min(n));
            let mut lo = 0f32;
            let mut hi = 0f32;
            for ch in &pcm.channels {
                for s in &ch[a..b] {
                    lo = lo.min(*s);
                    hi = hi.max(*s);
                }
            }
            (lo, hi)
        })
        .collect()
}

impl SoundState {
    fn shown(&self) -> &Arc<Pcm> {
        self.edited.as_ref().unwrap_or(&self.original)
    }

    fn reedit(&mut self) {
        let e = &self.edits;
        let untouched = e.trim.is_none() && e.fade_in == 0.0 && e.fade_out == 0.0 && (e.gain - 1.0).abs() < 1e-6 && e.normalize == 0.0 && e.channels == 0;
        self.edited = (!untouched).then(|| Arc::new(apply_edits(&self.original, &self.edits)));
        self.peaks = peaks(self.shown());
    }
}

impl Studio {
    pub(super) fn load_sound(&mut self, path: String, i: usize, cx: &mut Context<Self>) {
        let call = self.backend.call("media.preview", json!({ "path": path, "index": i }));
        cx.spawn(async move |this, cx| {
            let Ok(r) = call.await else { return };
            let Some(ogg) = r.bin(&r.json["ogg"]).map(|b| b.to_vec()) else { return };
            let bytes = r.json["bytes"].as_u64().unwrap_or(0);
            let busy = crate::backend::Busy::new();
            let decoded = cx.background_executor().spawn(async move { decode_ogg(&ogg).map_err(|e| e.to_string()) }).await;
            let _ = this.update(cx, |this, cx| {
                drop(busy);
                if this.selected_export != Some(i) {
                    return;
                }
                match decoded {
                    Ok(pcm) => {
                        let p = peaks(&pcm);
                        this.sound = Some(SoundState { export: i, original: Arc::new(pcm), edited: None, peaks: p, bytes, selection: None, drag: None, editing: false, edits: Edits { gain: 1.0, ..Default::default() }, quality: 0.7, bounds: Rc::new(Cell::new(Bounds::default())) });
                    }
                    Err(e) => this.set_status(format!("Sound: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn play_sound(&mut self, cx: &mut Context<Self>) {
        let Some(s) = &self.sound else { return };
        if self.player.playing() {
            self.player.stop();
            cx.notify();
            return;
        }
        let pcm = s.shown().clone();
        let (from, to) = match s.selection {
            Some((a, b)) if b - a > 0.01 && s.edited.is_none() => (a, Some(b)),
            _ => (0.0, None),
        };
        if let Err(e) = self.player.play(&pcm, from, to) {
            self.toast(Tone::Error, e, cx);
            return;
        }
        // Move the playhead while it plays.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(crate::audio::TICK).await;
            let Ok(playing) = this.update(cx, |this, cx| {
                cx.notify();
                this.player.position().is_some()
            }) else { break };
            if !playing {
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn edit_sound(&mut self, f: impl FnOnce(&mut SoundState), cx: &mut Context<Self>) {
        self.player.stop();
        if let Some(s) = &mut self.sound {
            f(s);
            s.reedit();
        }
        cx.notify();
    }

    fn apply_sound(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(s)) = (self.open.as_ref().map(|t| t.path.clone()), &self.sound) else { return };
        let Some(pcm) = s.edited.clone() else {
            self.toast(Tone::Info, "Nothing is edited yet", cx);
            return;
        };
        let (i, quality) = (s.export, s.quality);
        let pcm_b64 = base64::engine::general_purpose::STANDARD.encode(pcm.to_interleaved_i16());
        self.player.stop();
        self.set_status("Encoding…", cx);
        let call = self.backend.call("sound.replacePcm", json!({ "path": path, "index": i, "pcm": pcm_b64, "channels": pcm.channels.len(), "sampleRate": pcm.sample_rate, "quality": quality }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.dirty.insert(path.clone());
                    this.toast(Tone::Success, format!("Sound replaced: {} of Ogg Vorbis, {:.2} s (unsaved)", crate::model::human_size(r.json["bytes"].as_u64().unwrap_or(0)), r.json["duration"].as_f64().unwrap_or(0.0)), cx);
                    this.sound = None;
                    this.load_sound(path.clone(), i, cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not replace the sound: {e}"), cx),
            });
        })
        .detach();
    }

    /// Dev: `sound <step>` of the test driver.
    pub(super) fn drive_sound(&mut self, arg: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let (verb, rest) = arg.split_once(' ').unwrap_or((arg, ""));
        let n = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("{verb} <number>"));
        match verb {
            "edit" => self.edit_sound(|s| s.editing = true, cx),
            "select" => {
                let (a, b) = rest.trim().split_once(' ').ok_or("select <from> <to>")?;
                let (a, b) = (n(a)?, n(b)?);
                self.edit_sound(|s| s.selection = Some((a, b)), cx);
            }
            "trim" => self.edit_sound(|s| s.edits.trim = s.selection, cx),
            "fade-in" => {
                let v = n(rest)?;
                self.edit_sound(|s| s.edits.fade_in = v, cx);
            }
            "gain" => {
                let v = n(rest)?;
                self.edit_sound(|s| s.edits.gain = 10f32.powf(v as f32 / 20.0), cx);
            }
            "play" => self.play_sound(cx),
            "apply" => self.apply_sound(cx),
            _ => return Err(format!("unknown sound step {verb}")),
        }
        Ok(())
    }

    pub(super) fn render_sound(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let playhead = self.player.position();
        let playing = self.player.playing();
        let s = self.sound.as_ref()?;
        let shown = s.shown().clone();
        let duration = shown.duration();
        let info = format!(
            "Ogg Vorbis · {} · {} ch · {} Hz · {:.2} s{}",
            crate::model::human_size(s.bytes),
            s.original.channels.len(),
            s.original.sample_rate,
            s.original.duration(),
            if s.edited.is_some() { format!(" · edited: {:.2} s", duration) } else { String::new() }
        );
        let peaks = s.peaks.clone();
        let sel = if s.edited.is_some() { None } else { s.selection };
        let bounds_cell = s.bounds.clone();
        let (wave, accent, selc, muted) = (theme.primary, theme.warning, theme.primary.opacity(0.2), theme.muted);
        let waveform = canvas(
            move |b, _, _| {
                bounds_cell.set(b);
                b
            },
            move |b: Bounds<Pixels>, _, window, _| {
                window.paint_quad(fill(b, muted));
                let w = f32::from(b.size.width);
                let h = f32::from(b.size.height);
                let mid = f32::from(b.origin.y) + h / 2.0;
                if let Some((a, z)) = sel {
                    let x0 = f32::from(b.origin.x) + (a / duration.max(1e-6)) as f32 * w;
                    let x1 = f32::from(b.origin.x) + (z / duration.max(1e-6)) as f32 * w;
                    window.paint_quad(fill(Bounds::new(point(px(x0), b.origin.y), size(px((x1 - x0).max(1.0)), b.size.height)), selc));
                }
                if !peaks.is_empty() {
                    let cols = (w as usize).max(1);
                    for c in 0..cols {
                        let k0 = c * peaks.len() / cols;
                        let k1 = ((c + 1) * peaks.len() / cols).max(k0 + 1).min(peaks.len());
                        let (lo, hi) = peaks[k0..k1].iter().fold((0f32, 0f32), |(l, h), p| (l.min(p.0), h.max(p.1)));
                        let top = mid - hi * h / 2.0;
                        let len = ((hi - lo) * h / 2.0).max(1.0);
                        window.paint_quad(fill(Bounds::new(point(b.origin.x + px(c as f32), px(top)), size(px(1.0), px(len))), wave));
                    }
                }
                if let Some(t) = playhead {
                    let x = f32::from(b.origin.x) + (t / duration.max(1e-6)) as f32 * w;
                    window.paint_quad(fill(Bounds::new(point(px(x), b.origin.y), size(px(2.0), b.size.height)), accent));
                }
            },
        )
        .size_full();
        let (b1, b2) = (s.bounds.clone(), s.bounds.clone());
        let time_at = move |b: &Rc<Cell<Bounds<Pixels>>>, x: Pixels| -> f64 {
            let b = b.get();
            (((f32::from(x) - f32::from(b.origin.x)) / f32::from(b.size.width).max(1.0)).clamp(0.0, 1.0) as f64) * duration
        };
        let editing = s.editing;
        let has_sel = s.selection.is_some_and(|(a, b)| b - a > 0.01);
        let wave_box = div()
            .id("waveform")
            .w_full()
            .h(px(140.0))
            .rounded_md()
            .overflow_hidden()
            .cursor_crosshair()
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                let t = time_at(&b1, e.position.x);
                if let Some(s) = &mut this.sound {
                    s.drag = Some(t);
                    s.selection = None;
                }
                cx.notify();
            }))
            .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
                let t = time_at(&b2, e.position.x);
                let Some(s) = &mut this.sound else { return };
                let Some(a) = s.drag else { return };
                if !e.dragging() {
                    s.drag = None;
                    return;
                }
                s.selection = Some((a.min(t), a.max(t)));
                cx.notify();
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if let Some(s) = &mut this.sound {
                    s.drag = None;
                }
                cx.notify();
            }))
            .child(waveform);
        let sel_text = match s.selection {
            Some((a, b)) if b - a > 0.01 => format!("selection {a:.2}–{b:.2} s"),
            _ => "drag on the waveform to select".into(),
        };
        let mut body = v_flex().gap_2().child(wave_box).child(
            h_flex()
                .gap_2()
                .child(Button::new("play").small().primary().label(if playing { "Stop" } else if has_sel && s.edited.is_none() { "Play the selection" } else if s.edited.is_some() { "Play the edit" } else { "Play" }).on_click(cx.listener(|this, _, _, cx| this.play_sound(cx))))
                .child(div().flex_1().text_xs().text_color(theme.muted_foreground).child(format!("{info} · {sel_text}")))
                .when(!editing, |d| d.child(Button::new("edit-sound").small().label("Edit").tooltip("Trim, fade, gain, normalise, then re-encode").on_click(cx.listener(|this, _, _, cx| this.edit_sound(|s| s.editing = true, cx))))),
        );
        if editing {
            let e = s.edits.clone();
            let q = s.quality;
            let gain_db = 20.0 * e.gain.max(1e-6).log10();
            type Step = dyn Fn(&mut SoundState);
            let stepper = |id: &'static str, label: String, minus: Box<Step>, plus: Box<Step>, cx: &mut Context<Self>| {
                let (m, p): (Rc<Step>, Rc<Step>) = (minus.into(), plus.into());
                h_flex()
                    .gap_1()
                    .child(Button::new((id, 0usize)).xsmall().ghost().label("−").on_click(cx.listener(move |this, _, _, cx| {
                        let m = m.clone();
                        this.edit_sound(move |s| m(s), cx)
                    })))
                    .child(div().min_w(px(110.0)).text_xs().text_center().child(label))
                    .child(Button::new((id, 1usize)).xsmall().ghost().label("+").on_click(cx.listener(move |this, _, _, cx| {
                        let p = p.clone();
                        this.edit_sound(move |s| p(s), cx)
                    })))
            };
            let channels = |label: &'static str, n: usize, cx: &mut Context<Self>| {
                let b = Button::new(label).xsmall().label(label).on_click(cx.listener(move |this, _, _, cx| this.edit_sound(move |s| s.edits.channels = n, cx)));
                if e.channels == n { b.primary() } else { b.ghost() }
            };
            let trimmed = e.trim.map(|(a, b)| format!("kept {a:.2}–{b:.2} s")).unwrap_or_else(|| "whole sound".into());
            body = body.child(
                v_flex()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .child(h_flex().gap_2().child(Button::new("trim").xsmall().label("Trim to the selection").disabled(!has_sel || s.edited.is_some()).on_click(cx.listener(|this, _, _, cx| this.edit_sound(|s| s.edits.trim = s.selection, cx)))).child(div().text_xs().text_color(theme.muted_foreground).child(trimmed)))
                    .child(h_flex().gap_4().flex_wrap().child(stepper("fade-in", format!("fade in {:.2} s", e.fade_in), Box::new(|s| s.edits.fade_in = (s.edits.fade_in - 0.05).max(0.0)), Box::new(|s| s.edits.fade_in += 0.05), cx)).child(stepper("fade-out", format!("fade out {:.2} s", e.fade_out), Box::new(|s| s.edits.fade_out = (s.edits.fade_out - 0.05).max(0.0)), Box::new(|s| s.edits.fade_out += 0.05), cx)).child(stepper("gain", format!("gain {gain_db:+.0} dB"), Box::new(|s| s.edits.gain /= 10f32.powf(0.05)), Box::new(|s| s.edits.gain *= 10f32.powf(0.05)), cx)))
                    .child(
                        h_flex()
                            .gap_2()
                            .child({
                                let b = Button::new("normalize").xsmall().label("Normalise").tooltip("Scale the loudest peak to 98%").on_click(cx.listener(|this, _, _, cx| this.edit_sound(|s| s.edits.normalize = if s.edits.normalize > 0.0 { 0.0 } else { 0.98 }, cx)));
                                if e.normalize > 0.0 { b.primary() } else { b.ghost() }
                            })
                            .child(div().text_xs().text_color(theme.muted_foreground).child("channels"))
                            .child(channels("Keep", 0, cx))
                            .child(channels("Mono", 1, cx))
                            .child(channels("Stereo", 2, cx))
                            .child(stepper("quality", format!("Vorbis quality {q:.1}"), Box::new(|s| s.quality = (s.quality - 0.1).max(0.0)), Box::new(|s| s.quality = (s.quality + 0.1).min(1.0)), cx)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(Button::new("reset-sound").small().ghost().label("Reset").on_click(cx.listener(|this, _, _, cx| this.edit_sound(|s| s.edits = Edits { gain: 1.0, ..Default::default() }, cx))))
                            .child(Button::new("close-edit").small().ghost().label("Close").on_click(cx.listener(|this, _, _, cx| this.edit_sound(|s| s.editing = false, cx))))
                            .child(Button::new("apply-sound").small().primary().label("Apply and replace").disabled(s.edited.is_none()).tooltip("Encode the edit as Ogg Vorbis into the package (undoable)").on_click(cx.listener(|this, _, _, cx| this.apply_sound(cx)))),
                    ),
            );
        }
        Some(body.into_any_element())
    }
}
