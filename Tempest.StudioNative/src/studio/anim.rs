//! Animations: an AnimSequence (or an AnimSet, by its first sequence) plays
//! on a skeletal mesh (the set's preview mesh, else the best match of the
//! workspace; a picker switches) in the 3D view, skinned on the CPU; play,
//! pause, scrub on the timeline (notifies marked), speed and loop. Export
//! writes the mesh, skeleton and animation as glTF.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use gpui::{canvas, div, fill, point, prelude::*, px, size, AnyElement, Bounds, Context, MouseButton, Pixels, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{h_flex, v_flex, ActiveTheme, Sizable};
use serde_json::{json, Value as J};

use super::mesh::{viewport, MeshState};
use super::{Studio, Tone};
use crate::mesh_view::MeshView;

pub fn is_anim(class: &str) -> bool {
    matches!(class.to_ascii_lowercase().as_str(), "animsequence" | "animset")
}

pub struct AnimState {
    pub export: usize,
    info: J,
    mesh: Option<(String, usize)>,
    view: Option<Rc<RefCell<MeshState>>>,
    time: f32,
    /// Playing since (wall clock, the time it started from).
    playing: Option<(Instant, f32)>,
    speed: f32,
    looping: bool,
    note: String,
    bar: Rc<std::cell::Cell<Bounds<Pixels>>>,
    /// A notify being dragged on the timeline: (index, its time now).
    drag: Option<(usize, f32)>,
}

impl AnimState {
    fn length(&self) -> f32 {
        self.view.as_ref().map(|v| v.borrow().view.length()).unwrap_or(0.0)
    }

    fn seek(&mut self, t: f32) {
        self.time = t.clamp(0.0, self.length());
        if let Some(v) = &self.view {
            let mut v = v.borrow_mut();
            v.view.set_time(self.time);
            v.dirty = true;
        }
    }
}

impl Studio {
    pub(super) fn load_anim(&mut self, path: String, i: usize, mesh: Option<(String, usize)>, cx: &mut Context<Self>) {
        let keep = self.anim.as_ref().filter(|a| a.export == i).map(|a| a.time);
        let info = self.backend.call("anim.info", json!({ "path": path, "index": i }));
        let preview = self.backend.call("anim.preview", json!({ "path": path, "index": i, "mesh": mesh.as_ref().map(|(p, k)| json!({ "path": p, "index": k })), "textureSize": 512 }));
        cx.spawn(async move |this, cx| {
            let info = info.await;
            let _ = this.update(cx, |this, cx| {
                if this.selected_export != Some(i) {
                    return;
                }
                match info {
                    Ok(r) => this.anim = Some(AnimState { export: i, info: r.json, mesh: mesh.clone(), view: None, time: 0.0, playing: None, speed: 1.0, looping: true, note: "Loading the mesh…".into(), bar: Default::default(), drag: None }),
                    Err(e) => this.set_status(format!("Animation: {e}"), cx),
                }
                cx.notify();
            });
            let r = preview.await;
            let (glb, note) = match &r {
                Ok(r) => {
                    let missing = r.json["tracksWithoutBone"].as_array().map(|a| a.len()).unwrap_or(0);
                    let note = format!("on {}{}", r.json["mesh"]["name"].as_str().unwrap_or_default(), if missing > 0 { format!(" · {missing} tracks have no bone in this mesh") } else { String::new() });
                    (r.bin(&r.json["glb"]).map(|b| b.to_vec()), note)
                }
                Err(e) => (None, e.clone()),
            };
            let busy = crate::backend::Busy::new();
            let built = match glb {
                Some(glb) => Some(cx.background_executor().spawn(async move { MeshView::new(&glb) }).await),
                None => None,
            };
            let _ = this.update(cx, |this, cx| {
                drop(busy);
                let Some(a) = this.anim.as_mut().filter(|a| a.export == i) else { return };
                a.note = note;
                match built {
                    Some(Ok(mut view)) => {
                        let t = keep.unwrap_or(0.0).min(view.length());
                        view.set_time(t);
                        a.time = t;
                        let summary = format!("{} triangles", view.triangles);
                        a.view = Some(Rc::new(RefCell::new(MeshState::new(i, 0, view, summary))));
                    }
                    Some(Err(e)) => a.note = e,
                    None => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn anim_play(&mut self, cx: &mut Context<Self>) {
        let Some(a) = &mut self.anim else { return };
        if a.playing.take().is_some() {
            cx.notify();
            return;
        }
        if a.time >= a.length() - 1e-3 {
            a.seek(0.0);
        }
        a.playing = Some((Instant::now(), a.time));
        let export = a.export;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
            let Ok(go) = this.update(cx, |this, cx| {
                let Some(a) = this.anim.as_mut().filter(|a| a.export == export) else { return false };
                let Some((at, from)) = a.playing else { return false };
                let len = a.length().max(1e-3);
                let mut t = from + at.elapsed().as_secs_f32() * a.speed;
                if t > len {
                    if a.looping {
                        t %= len;
                    } else {
                        t = len;
                        a.playing = None;
                    }
                }
                a.seek(t);
                cx.notify();
                a.playing.is_some()
            }) else { break };
            if !go {
                break;
            }
        })
        .detach();
        cx.notify();
    }

    /// `anim.edit {op…}` on the shown sequence, then read it again (the time is kept).
    fn anim_edit(&mut self, params: J, done: String, cx: &mut Context<Self>) {
        let (Some(path), Some(a)) = (self.open.as_ref().map(|t| t.path.clone()), self.anim.as_ref()) else { return };
        // The sequence itself (an AnimSet shows its first one).
        let (i, mesh) = (a.info["index"].as_u64().map(|i| i as usize).unwrap_or(a.export), a.mesh.clone());
        let shown = a.export;
        let mut params = params;
        params["path"] = json!(path);
        params["index"] = json!(i);
        let call = self.backend.call("anim.edit", params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(_) => {
                    this.dirty.insert(path.clone());
                    this.set_status(format!("{done} (unsaved)"), cx);
                    this.load_anim(path.clone(), shown, mesh.clone(), cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not edit the animation: {e}"), cx),
            });
        })
        .detach();
    }

    /// Trim to frames, a new length, and the rate scale, in one dialog.
    fn anim_edit_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::input::{Input, InputState};
        use gpui_component::WindowExt;
        let Some(a) = &self.anim else { return };
        let frames = a.info["numFrames"].as_u64().unwrap_or(1);
        let length = a.info["length"].as_f64().unwrap_or(0.0);
        let rate = a.info["rateScale"].as_f64().unwrap_or(1.0);
        let field = |v: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut s = InputState::new(window, cx);
                s.set_value(v, window, cx);
                s
            })
        };
        let from = field("0".into(), window, cx);
        let to = field((frames.saturating_sub(1)).to_string(), window, cx);
        let len = field(format!("{length:.3}"), window, cx);
        let rate_in = field(format!("{rate}"), window, cx);
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let row = |label: &'static str, hint: String, i: &gpui::Entity<InputState>| h_flex().gap_3().child(div().w(px(150.0)).text_sm().child(label)).child(div().w(px(120.0)).child(Input::new(i).small())).child(div().flex_1().text_xs().text_color(theme.muted_foreground).child(hint));
            let (me, f2, t2, l2, r2) = (me.clone(), from.clone(), to.clone(), len.clone(), rate_in.clone());
            dialog
                .title("Edit the sequence")
                .w(px(600.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(row("Keep frames from", format!("0 … {}", frames.saturating_sub(1)), &from))
                        .child(row("to", "the frames outside are cut; notifies follow".into(), &to))
                        .child(row("Length (s)", format!("now {length:.3} s: the keys are spread over it"), &len))
                        .child(row("Rate scale", "how fast the game plays it (1 = as stored)".into(), &rate_in)),
                )
                .footer(super::dialogs::footer("Apply", false, true, move |window, cx| {
                    window.close_dialog(cx);
                    let num = |e: &gpui::Entity<InputState>, cx: &gpui::App| e.read(cx).value().trim().parse::<f64>().ok();
                    let (fa, fb, l, r) = (num(&f2, cx), num(&t2, cx), num(&l2, cx), num(&r2, cx));
                    let _ = me.update(cx, |this, cx| {
                        if let (Some(fa), Some(fb)) = (fa, fb) {
                            if fa > 0.0 || (fb as u64) < frames.saturating_sub(1) {
                                this.anim_edit(json!({ "op": "trim", "start": fa as u64, "end": fb as u64 }), format!("Trimmed to frames {fa}–{fb}"), cx);
                            }
                        }
                        if let Some(l) = l.filter(|l| (l - length).abs() > 1e-4 && *l > 0.0) {
                            this.anim_edit(json!({ "op": "retime", "length": l }), format!("New length {l:.3} s"), cx);
                        }
                        if let Some(r) = r.filter(|r| (r - rate).abs() > 1e-6) {
                            this.set_property("RateScale".into(), format!("{r}"), cx);
                        }
                    });
                }))
        });
    }

    fn render_notifies(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(a) = &self.anim else { return div().into_any_element() };
        let now = a.time;
        let list = a.info["notifies"].as_array().cloned().unwrap_or_default();
        let mut body = v_flex().gap_0p5();
        for n in list {
            let k = n["index"].as_u64().unwrap_or(0) as usize;
            let t = n["time"].as_f64().unwrap_or(0.0) as f32;
            let name = match (n["notify"]["class"].as_str().filter(|c| !c.is_empty()), n["notify"]["path"].as_str()) {
                (Some(c), Some(p)) => format!("{} · {}", c.trim_start_matches("animnotify_"), p.rsplit('.').next().unwrap_or(p)),
                _ => "(empty notify)".into(),
            };
            let comment = n["comment"].as_str().unwrap_or("").to_string();
            body = body.child(
                h_flex()
                    .gap_2()
                    .px_1()
                    .rounded_sm()
                    .hover(|s| s.bg(theme.list_hover))
                    .child(div().w(px(56.0)).font_family("Menlo").text_xs().child(format!("{t:.2} s")))
                    .child(div().flex_1().min_w_0().text_sm().overflow_hidden().text_ellipsis().whitespace_nowrap().child(if comment.is_empty() { name } else { format!("{name} · {comment}") }))
                    .child(Button::new(("nt-go", k)).xsmall().ghost().label("Go").tooltip("Move the playhead here").on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(a) = &mut this.anim {
                            a.playing = None;
                            a.seek(t);
                        }
                        cx.notify();
                    })))
                    .child(Button::new(("nt-here", k)).xsmall().ghost().label("To the playhead").on_click(cx.listener(move |this, _, _, cx| this.anim_edit(json!({ "op": "notifyMove", "notify": k, "time": now }), format!("Moved notify {k} to {now:.2} s"), cx))))
                    .child(Button::new(("nt-copy", k)).xsmall().ghost().label("Copy at the playhead").on_click(cx.listener(move |this, _, _, cx| this.anim_edit(json!({ "op": "notifyAdd", "notify": k, "time": now }), format!("Copied notify {k} to {now:.2} s"), cx))))
                    .child(Button::new(("nt-rm", k)).xsmall().ghost().label("Remove").on_click(cx.listener(move |this, _, _, cx| this.anim_edit(json!({ "op": "notifyRemove", "notify": k }), format!("Removed notify {k}"), cx)))),
            );
        }
        v_flex()
            .gap_1()
            .child(h_flex().child(div().flex_1().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("NOTIFIES (drag the orange marks on the timeline)")).child(Button::new("anim-edit").xsmall().label("Edit the sequence…").tooltip("Trim, change the length or the rate").on_click(cx.listener(|this, _, window, cx| this.anim_edit_dialog(window, cx)))))
            .child(body)
            .into_any_element()
    }

    /// The animation's viewport (dev: the driver's view switches).
    pub(super) fn anim_view(&self) -> Option<Rc<RefCell<MeshState>>> {
        self.anim.as_ref().and_then(|a| a.view.clone())
    }

    /// Dev: `anim <step>` of the test driver.
    pub(super) fn drive_anim(&mut self, arg: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let (verb, rest) = arg.split_once(' ').unwrap_or((arg, ""));
        match verb {
            "play" => self.anim_play(cx),
            "seek" => {
                let t = rest.trim().parse::<f32>().map_err(|_| "seek <seconds>")?;
                let a = self.anim.as_mut().ok_or("no animation")?;
                a.playing = None;
                a.seek(t);
                cx.notify();
            }
            // `notify-move <k> <seconds>`, `notify-copy <k>` (at the playhead), `notify-remove <k>`, `retime <seconds>`.
            "notify-move" => {
                let (k, t) = rest.trim().split_once(' ').ok_or("notify-move <k> <seconds>")?;
                let (k, t): (usize, f32) = (k.parse().map_err(|_| "k")?, t.parse().map_err(|_| "seconds")?);
                self.anim_edit(json!({ "op": "notifyMove", "notify": k, "time": t }), format!("Moved notify {k}"), cx);
            }
            "notify-copy" => {
                let k: usize = rest.trim().parse().map_err(|_| "notify-copy <k>")?;
                let t = self.anim.as_ref().map(|a| a.time).unwrap_or(0.0);
                self.anim_edit(json!({ "op": "notifyAdd", "notify": k, "time": t }), format!("Copied notify {k}"), cx);
            }
            "notify-remove" => {
                let k: usize = rest.trim().parse().map_err(|_| "notify-remove <k>")?;
                self.anim_edit(json!({ "op": "notifyRemove", "notify": k }), format!("Removed notify {k}"), cx);
            }
            "retime" => {
                let l: f64 = rest.trim().parse().map_err(|_| "retime <seconds>")?;
                self.anim_edit(json!({ "op": "retime", "length": l }), format!("New length {l}"), cx);
            }
            _ => return Err(format!("unknown anim step {verb}")),
        }
        Ok(())
    }

    pub(super) fn render_anim(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let a = self.anim.as_ref().filter(|a| Some(a.export) == self.selected_export)?;
        let info = &a.info;
        let len = a.length();
        let view = match &a.view {
            Some(v) => viewport("anim-view", v.clone(), 420.0, Some(gpui_component::h_flex().gap_1().children(super::mesh::view_toggles(v)).into_any_element()), theme.muted, theme.muted_foreground),
            None => div().h(px(420.0)).w_full().rounded_md().bg(theme.muted).flex().items_center().justify_center().text_sm().text_color(theme.muted_foreground).child(a.note.clone()).into_any_element(),
        };
        // The timeline: progress, notifies; click or drag to scrub.
        let mut notifies: Vec<f32> = info["notifies"].as_array().into_iter().flatten().filter_map(|n| n["time"].as_f64()).map(|t| t as f32).collect();
        if let Some((k, t)) = a.drag {
            if let Some(n) = notifies.get_mut(k) {
                *n = t;
            }
        }
        let (t, bar_cell) = (a.time, a.bar.clone());
        let (accent, muted, mark) = (theme.primary, theme.secondary, theme.warning);
        let notifies_draw = notifies.clone();
        let bar = canvas(
            move |b, _, _| {
                bar_cell.set(b);
                b
            },
            move |b: Bounds<Pixels>, _, window, _| {
                window.paint_quad(fill(b, muted));
                let w = f32::from(b.size.width);
                let frac = if len > 0.0 { t / len } else { 0.0 };
                window.paint_quad(fill(Bounds::new(b.origin, size(px(w * frac), b.size.height)), accent.opacity(0.5)));
                for n in &notifies_draw {
                    let x = f32::from(b.origin.x) + w * (n / len.max(1e-3));
                    window.paint_quad(fill(Bounds::new(point(px(x - 1.0), b.origin.y), size(px(2.0), b.size.height)), mark));
                }
            },
        )
        .size_full();
        let cell = a.bar.clone();
        let seek_at = move |x: Pixels| -> f32 {
            let b = cell.get();
            ((f32::from(x) - f32::from(b.origin.x)) / f32::from(b.size.width).max(1.0)).clamp(0.0, 1.0) * len
        };
        let (seek2, seek3) = (seek_at.clone(), seek_at.clone());
        let marks: Vec<f32> = notifies.clone();
        let cell2 = a.bar.clone();
        let timeline = div()
            .id("anim-timeline")
            .w_full()
            .h(px(18.0))
            .rounded_sm()
            .overflow_hidden()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                // On a notify's mark: drag it; elsewhere: scrub.
                let b = cell2.get();
                let w = f32::from(b.size.width).max(1.0);
                let x = f32::from(e.position.x) - f32::from(b.origin.x);
                let hit = marks.iter().position(|n| (n / len.max(1e-3) * w - x).abs() <= 5.0);
                if let Some(a) = &mut this.anim {
                    a.playing = None;
                    match hit {
                        Some(k) => a.drag = Some((k, marks[k])),
                        None => a.seek(seek_at(e.position.x)),
                    }
                }
                cx.notify();
            }))
            .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
                if e.dragging() {
                    if let Some(a) = &mut this.anim {
                        a.playing = None;
                        let t = seek2(e.position.x);
                        match &mut a.drag {
                            Some((_, nt)) => *nt = t,
                            None => a.seek(t),
                        }
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(move |this, e: &gpui::MouseUpEvent, _, cx| {
                let Some((k, _)) = this.anim.as_mut().and_then(|a| a.drag.take()) else { return };
                let t = seek3(e.position.x);
                this.anim_edit(json!({ "op": "notifyMove", "notify": k, "time": t }), format!("Moved notify {k} to {t:.2} s"), cx);
            }))
            .child(bar);
        let fps = info["fps"].as_f64().unwrap_or(30.0) as f32;
        let speed = a.speed;
        let speed_btn = |label: &'static str, v: f32, cx: &mut Context<Self>| {
            let b = Button::new(label).xsmall().label(label).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(a) = &mut this.anim {
                    let t = a.time;
                    a.speed = v;
                    if a.playing.is_some() {
                        a.playing = Some((Instant::now(), t));
                    }
                }
                cx.notify();
            }));
            if (speed - v).abs() < 1e-3 { b.primary() } else { b.ghost() }
        };
        let controls = h_flex()
            .gap_2()
            .child(Button::new("anim-play").small().primary().label(if a.playing.is_some() { "Pause" } else { "Play" }).on_click(cx.listener(|this, _, _, cx| this.anim_play(cx))))
            .child(div().font_family("Menlo").text_xs().child(format!("{:.2} / {:.2} s · frame {} / {}", a.time, len, (a.time * fps).round() as i64, info["numFrames"])))
            .child(div().flex_1())
            .child(speed_btn("0.25×", 0.25, cx))
            .child(speed_btn("0.5×", 0.5, cx))
            .child(speed_btn("1×", 1.0, cx))
            .child(speed_btn("2×", 2.0, cx))
            .child({
                let b = Button::new("anim-loop").xsmall().label("Loop").on_click(cx.listener(|this, _, _, cx| {
                    if let Some(a) = &mut this.anim {
                        a.looping = !a.looping;
                    }
                    cx.notify();
                }));
                if a.looping { b.primary() } else { b.ghost() }
            });
        let mut body = v_flex().gap_2().child(view).child(timeline).child(controls).child(div().text_xs().text_color(theme.muted_foreground).child(format!(
            "{} · {} tracks, {} keys, {} · rate {} · {}",
            a.note,
            info["tracks"],
            info["keys"],
            info["keyFormat"].as_str().unwrap_or_default(),
            info["rateScale"],
            if notifies_count(info) > 0 { format!("{} notifies (orange)", notifies_count(info)) } else { "no notifies".into() }
        )));
        // Another mesh to play it on.
        let meshes = info["meshes"].as_array().cloned().unwrap_or_default();
        if meshes.len() > 1 {
            let mut row = h_flex().gap_1().flex_wrap().child(div().text_xs().text_color(theme.muted_foreground).child("play on"));
            let current = a.mesh.clone();
            for (k, m) in meshes.into_iter().take(8).enumerate() {
                let (p, i) = (m["path"].as_str().unwrap_or_default().to_string(), m["index"].as_u64().unwrap_or(0) as usize);
                let on = current.as_ref().map(|c| c.0 == p && c.1 == i).unwrap_or(k == 0);
                let label = m["object"].as_str().unwrap_or_default().rsplit('.').next().unwrap_or_default().to_string();
                let b = Button::new(("anim-mesh", k)).xsmall().label(label).tooltip(format!("{} bones in common", m["matched"])).on_click(cx.listener(move |this, _, _, cx| {
                    let (Some(path), Some(e)) = (this.open.as_ref().map(|t| t.path.clone()), this.selected_export) else { return };
                    this.load_anim(path, e, Some((p.clone(), i)), cx);
                }));
                row = row.child(if on { b.primary() } else { b.ghost() });
            }
            body = body.child(row);
        }
        // The set's other sequences.
        let seqs = info["sequences"].as_array().cloned().unwrap_or_default();
        if seqs.len() > 1 {
            let mut row = h_flex().gap_1().flex_wrap().child(div().text_xs().text_color(theme.muted_foreground).child(format!("{} sequences in the set", seqs.len())));
            let me = info["index"].as_u64();
            for (k, sq) in seqs.into_iter().take(40).enumerate() {
                let i = sq["index"].as_u64().unwrap_or(0) as usize;
                let b = Button::new(("anim-seq", k)).xsmall().label(sq["name"].as_str().unwrap_or_default().to_string()).on_click(cx.listener(move |this, _, _, cx| {
                    this.reveal(i, cx);
                    this.user_select_export(i, cx);
                }));
                row = row.child(if me == Some(i as u64) { b.primary() } else { b.ghost() });
            }
            body = body.child(row);
        }
        if info["animSet"].is_object() && info["numFrames"].as_u64().is_some() {
            body = body.child(self.render_notifies(cx));
        }
        Some(body.into_any_element())
    }
}

fn notifies_count(info: &J) -> usize {
    info["notifies"].as_array().map(|a| a.len()).unwrap_or(0)
}
