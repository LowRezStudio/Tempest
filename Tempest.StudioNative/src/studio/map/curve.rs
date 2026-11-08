//! The curve editor of a Matinee curve: one line per component (x/y/z or
//! r/g/b/a) drawn with the engine's evaluation, keys as dots (drag to move
//! them in time and value: Shift value only, Ctrl time only), and the
//! selected key's tangent handles (drag to set the slope: an auto key
//! becomes CIM_CurveUser, a CIM_CurveBreak key moves one side only). The
//! wheel zooms the values, F (or Fit) fits.

use std::collections::HashSet;

use gpui::{canvas, div, fill, point, prelude::*, px, size, AnyElement, Bounds, Context, MouseButton, PathBuilder, Pixels, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::{h_flex, v_flex, ActiveTheme, Sizable};
use serde_json::json;
use tempest_assets::matinee_curve::{eval, Mode, Point};

use super::matinee::{points, KeyRef};
use super::MapEditor;

const COLORS: [u32; 4] = [0xf38ba8, 0xa6e3a1, 0x89b4fa, 0xf9e2af];
const PAD: f32 = 8.0;
/// Handle length on screen, in points.
const HANDLE: f32 = 40.0;

#[derive(Clone, Copy)]
enum Grab {
    Key { key: usize, comp: usize },
    Handle { key: usize, comp: usize, arrive: bool },
}

#[derive(Default)]
pub struct CurveView {
    lo: f64,
    hi: f64,
    hidden: HashSet<usize>,
    /// The curve the range was fitted to (refitted when another opens).
    pub fit_for: Option<(usize, String, usize)>,
    bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    grab: Option<(Grab, (f32, f32), bool)>,
    /// The live drag: key time and value, or a handle's slope.
    live: Option<(f64, f64, Option<f64>)>,
}

/// Screen mapping of the curve view.
#[derive(Clone, Copy)]
struct Map {
    w: f32,
    h: f32,
    len: f64,
    lo: f64,
    hi: f64,
}

impl Map {
    fn sx(&self, t: f64) -> f32 {
        PAD + (t / self.len) as f32 * (self.w - 2.0 * PAD)
    }
    fn sy(&self, v: f64) -> f32 {
        let span = if self.hi - self.lo == 0.0 { 1.0 } else { self.hi - self.lo };
        PAD + (1.0 - ((v - self.lo) / span) as f32) * (self.h - 2.0 * PAD)
    }
    fn tx(&self, x: f32) -> f64 {
        ((x - PAD) / (self.w - 2.0 * PAD)) as f64 * self.len
    }
    fn vy(&self, y: f32) -> f64 {
        self.lo + (1.0 - ((y - PAD) / (self.h - 2.0 * PAD)) as f64) * (self.hi - self.lo)
    }
    /// Where a tangent handle is drawn: a fixed length along (dt, slope·dt) on screen.
    fn handle(&self, p: &Point, comp: usize, arrive: bool, slope: Option<f64>) -> (f32, f32) {
        let s = slope.unwrap_or_else(|| if arrive { p.arrive.get(comp).copied().unwrap_or(0.0) } else { p.leave.get(comp).copied().unwrap_or(0.0) });
        let dx = self.sx(1.0) - self.sx(0.0);
        let dy = self.sy(s) - self.sy(0.0);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let dir = if arrive { -1.0 } else { 1.0 };
        let v = p.out.get(comp).copied().unwrap_or(0.0);
        (self.sx(p.time) + dir * HANDLE * dx / len, self.sy(v) + dir * HANDLE * dy / len)
    }
}

impl MapEditor {
    fn curve_points(&self) -> (Vec<Point>, usize, Vec<String>, String) {
        let Some((track, path)) = &self.matinee.curve else { return (Vec::new(), 1, Vec::new(), String::new()) };
        let Some(c) = self.matinee.curve(*track, path) else { return (Vec::new(), 1, Vec::new(), String::new()) };
        let pts = points(c["keys"].as_array().map(|a| a.as_slice()).unwrap_or(&[]));
        let width = c["width"].as_u64().map(|w| w as usize).unwrap_or_else(|| pts.first().map(|p| p.out.len()).unwrap_or(1)).max(1);
        let names: Vec<String> = match c["components"].as_array() {
            Some(a) if !a.is_empty() => a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect(),
            _ if width == 1 => vec!["value".into()],
            _ => ["x", "y", "z", "w"].iter().take(width).map(|s| s.to_string()).collect(),
        };
        (pts, width, names, c["name"].as_str().unwrap_or("Curve").to_string())
    }

    fn fit_curve(&mut self) {
        let (pts, ..) = self.curve_points();
        let len = self.matinee.length();
        let (mut a, mut b) = (f64::INFINITY, f64::NEG_INFINITY);
        for i in 0..=100 {
            let v = if pts.is_empty() { vec![0.0] } else { eval(&pts, len * i as f64 / 100.0, &[0.0]) };
            for (k, x) in v.iter().enumerate() {
                if self.matinee.curve_view.hidden.contains(&k) {
                    continue;
                }
                a = a.min(*x);
                b = b.max(*x);
            }
        }
        if !a.is_finite() {
            (a, b) = (0.0, 1.0);
        }
        let pad = ((b - a) * 0.1).max(0.5);
        self.matinee.curve_view.lo = a - pad;
        self.matinee.curve_view.hi = b + pad;
    }

    fn curve_map(&self) -> Map {
        let b = self.matinee.curve_view.bounds.get();
        Map { w: f32::from(b.size.width).max(40.0), h: f32::from(b.size.height).max(40.0), len: self.matinee.length(), lo: self.matinee.curve_view.lo, hi: self.matinee.curve_view.hi }
    }

    /// Where the open curve's keys are drawn (key, component, x, y in the editor's points), for tests.
    pub(crate) fn curve_on_screen(&self) -> Vec<[f32; 4]> {
        let (pts, ..) = self.curve_points();
        let m = self.curve_map();
        pts.iter().enumerate().flat_map(|(i, p)| p.out.iter().enumerate().map(move |(k, v)| [i as f32, k as f32, m.sx(p.time), m.sy(*v)])).collect()
    }

    fn sel_key(&self) -> Option<usize> {
        let (t, p) = self.matinee.curve.as_ref()?;
        self.matinee.key.as_ref().filter(|k| k.track == *t && &k.curve == p).map(|k| k.key)
    }

    fn curve_local(&self, p: gpui::Point<Pixels>) -> (f32, f32) {
        let b = self.matinee.curve_view.bounds.get();
        (f32::from(p.x - b.origin.x), f32::from(p.y - b.origin.y))
    }

    fn curve_down(&mut self, e: &gpui::MouseDownEvent, cx: &mut Context<Self>) {
        let (pts, ..) = self.curve_points();
        let m = self.curve_map();
        let q = self.curve_local(e.position);
        let near = |x: f32, y: f32| ((x - q.0).powi(2) + (y - q.1).powi(2)).sqrt() < 6.0;
        let sel = self.sel_key();
        let hidden = self.matinee.curve_view.hidden.clone();
        let mut grab = None;
        if let Some(i) = sel {
            if let Some(p) = pts.get(i).filter(|p| p.mode != Mode::Linear && p.mode != Mode::Constant) {
                for comp in (0..p.out.len()).filter(|c| !hidden.contains(c)) {
                    for arrive in [true, false] {
                        let (x, y) = m.handle(p, comp, arrive, None);
                        if near(x, y) {
                            grab = Some(Grab::Handle { key: i, comp, arrive });
                        }
                    }
                }
            }
        }
        if grab.is_none() {
            'outer: for (i, p) in pts.iter().enumerate() {
                for (comp, v) in p.out.iter().enumerate() {
                    if !hidden.contains(&comp) && near(m.sx(p.time), m.sy(*v)) {
                        grab = Some(Grab::Key { key: i, comp });
                        break 'outer;
                    }
                }
            }
        }
        let Some(g) = grab else { return };
        if let (Grab::Key { key, .. }, Some((track, path))) = (g, self.matinee.curve.clone()) {
            self.matinee.key = Some(KeyRef { track, curve: path, key });
        }
        self.matinee.curve_view.grab = Some((g, q, false));
        self.matinee.curve_view.live = None;
        cx.notify();
    }

    fn curve_move(&mut self, e: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((g, start, moved)) = self.matinee.curve_view.grab else { return };
        if e.pressed_button.is_none() {
            self.matinee.curve_view.grab = None;
            return;
        }
        let (pts, ..) = self.curve_points();
        let m = self.curve_map();
        let q = self.curve_local(e.position);
        if !moved && ((q.0 - start.0).powi(2) + (q.1 - start.1).powi(2)).sqrt() < 3.0 {
            return;
        }
        self.matinee.curve_view.grab = Some((g, start, true));
        match g {
            Grab::Key { key, comp } => {
                let p = &pts[key];
                let low = if key > 0 { pts[key - 1].time + 0.001 } else { 0.0 };
                let high = if key + 1 < pts.len() { pts[key + 1].time - 0.001 } else { m.len };
                let t = if e.modifiers.shift { p.time } else { m.tx(q.0).clamp(low, high) };
                let v = if e.modifiers.control { p.out.get(comp).copied().unwrap_or(0.0) } else { m.vy(q.1) };
                self.matinee.curve_view.live = Some((t, v, None));
                self.set_matinee_time(t, cx);
            }
            Grab::Handle { key, comp, arrive } => {
                let p = &pts[key];
                let dt = m.tx(q.0) - p.time;
                if dt.abs() < 1e-6 || (if arrive { dt > 0.0 } else { dt < 0.0 }) {
                    return;
                }
                let slope = (m.vy(q.1) - p.out.get(comp).copied().unwrap_or(0.0)) / dt;
                self.matinee.curve_view.live = Some((p.time, p.out.get(comp).copied().unwrap_or(0.0), Some(slope)));
            }
        }
        cx.notify();
    }

    fn curve_up(&mut self, cx: &mut Context<Self>) {
        let Some((g, _, moved)) = self.matinee.curve_view.grab.take() else { return };
        let live = self.matinee.curve_view.live.take();
        let (Some((t, v, slope)), true, Some((track, path))) = (live, moved, self.matinee.curve.clone()) else { return };
        let (pts, ..) = self.curve_points();
        let round = |x: f64| (x * 1000.0).round() / 1000.0;
        let one = |v: Vec<f64>| if v.len() == 1 { json!(v[0]) } else { json!(v) };
        match g {
            Grab::Key { key, comp } => {
                let p = &pts[key];
                let mut value = p.out.clone();
                value[comp] = round(v);
                let mut change = json!({ "value": one(value) });
                if (t - p.time).abs() > 1e-6 {
                    change["time"] = json!(round(t));
                }
                self.set_key(KeyRef { track, curve: path, key }, change, cx);
            }
            Grab::Handle { key, comp, arrive } => {
                let p = &pts[key];
                let s = round(slope.unwrap_or(0.0));
                let (mut a, mut l) = (p.arrive.clone(), p.leave.clone());
                // CurveBreak: each side on its own; otherwise both sides follow.
                if p.mode == Mode::CurveBreak {
                    if arrive { a[comp] = s } else { l[comp] = s }
                } else {
                    a[comp] = s;
                    l[comp] = s;
                }
                self.set_key(KeyRef { track, curve: path, key }, json!({ "arrive": one(a), "leave": one(l) }), cx);
            }
        }
    }

    fn curve_scroll(&mut self, e: &gpui::ScrollEvent, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let dy = f32::from(e.delta.pixel_delta(px(16.0)).y);
        let f = if dy < 0.0 { 1.15 } else { 1.0 / 1.15 };
        let m = self.curve_map();
        let c = m.vy(self.curve_local(e.position).1);
        let v = &mut self.matinee.curve_view;
        v.lo = c + (v.lo - c) * f;
        v.hi = c + (v.hi - c) * f;
        cx.notify();
    }

    pub(super) fn render_curve(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (pts, width, names, name) = self.curve_points();
        let fit_key = self.matinee.curve.clone().map(|(t, p)| (t, p, pts.len()));
        if self.matinee.curve_view.fit_for != fit_key {
            self.matinee.curve_view.fit_for = fit_key;
            self.fit_curve();
        }
        let mut head = h_flex().h(px(24.0)).px_2().gap_2().child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(name));
        for (k, n) in names.iter().enumerate() {
            let shown = !self.matinee.curve_view.hidden.contains(&k);
            head = head.child(h_flex().gap_1().text_color(super::hsla(COLORS[k % 4])).child(Checkbox::new(("cv-comp", k)).checked(shown).on_click(cx.listener(move |this, v: &bool, _, cx| {
                if *v {
                    this.matinee.curve_view.hidden.remove(&k);
                } else {
                    this.matinee.curve_view.hidden.insert(k);
                }
                cx.notify();
            }))).child(n.clone()));
        }
        head = head
            .child(div().flex_1())
            .child(div().text_color(theme.muted_foreground).child("drag keys (Shift: value only, Ctrl: time only) and handles · wheel zooms · F fits"))
            .child(Button::new("cv-fit").xsmall().ghost().label("Fit").on_click(cx.listener(|this, _, _, cx| {
                this.fit_curve();
                cx.notify();
            })))
            .child(Button::new("cv-close").xsmall().ghost().label("Close").on_click(cx.listener(|this, _, _, cx| {
                this.matinee.curve = None;
                cx.notify();
            })));
        let bounds = self.matinee.curve_view.bounds.clone();
        let (lo, hi, len, time) = (self.matinee.curve_view.lo, self.matinee.curve_view.hi, self.matinee.length(), self.matinee.time);
        let hidden = self.matinee.curve_view.hidden.clone();
        let sel = self.sel_key();
        let live = self.matinee.curve_view.live;
        let grab = self.matinee.curve_view.grab.map(|g| g.0);
        let (zero_c, head_c, sel_c, dot_border) = (theme.foreground.opacity(0.2), theme.warning, super::hsla(0xf9e2af), gpui::black().opacity(0.5));
        let painted = canvas(
            move |b, _, _| {
                bounds.set(b);
                b
            },
            move |b, _, window, _| {
                let m = Map { w: f32::from(b.size.width).max(40.0), h: f32::from(b.size.height).max(40.0), len, lo, hi };
                let o = b.origin;
                let at = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
                window.paint_quad(fill(Bounds::new(at(0.0, m.sy(0.0)), size(b.size.width, px(1.0))), zero_c));
                window.paint_quad(fill(Bounds::new(at(m.sx(time), 0.0), size(px(1.0), b.size.height)), head_c));
                let line = |a: (f32, f32), c: (f32, f32), color: gpui::Hsla, width: f32, window: &mut Window| {
                    let mut p = PathBuilder::stroke(px(width));
                    p.move_to(at(a.0, a.1));
                    p.line_to(at(c.0, c.1));
                    if let Ok(p) = p.build() {
                        window.paint_path(p, color);
                    }
                };
                if !pts.is_empty() {
                    let n = ((m.w / 3.0).round() as usize).clamp(80, 400);
                    let samples: Vec<Vec<f64>> = (0..=n).map(|i| eval(&pts, len * i as f64 / n as f64, &[0.0])).collect();
                    for k in 0..width {
                        if hidden.contains(&k) {
                            continue;
                        }
                        let mut p = PathBuilder::stroke(px(1.5));
                        for (i, v) in samples.iter().enumerate() {
                            let q = at(m.sx(len * i as f64 / n as f64), m.sy(v.get(k).copied().unwrap_or(0.0)));
                            if i == 0 { p.move_to(q) } else { p.line_to(q) }
                        }
                        if let Ok(p) = p.build() {
                            window.paint_path(p, super::hsla(COLORS[k % 4]));
                        }
                    }
                }
                for (i, p) in pts.iter().enumerate() {
                    for (k, v) in p.out.iter().enumerate() {
                        if hidden.contains(&k) {
                            continue;
                        }
                        let color = super::hsla(COLORS[k % 4]);
                        let dragging_key = matches!(grab, Some(Grab::Key { key, comp }) if key == i && comp == k);
                        let (t, v) = match (dragging_key, live) {
                            (true, Some((t, v, None))) => (t, v),
                            _ => (p.time, *v),
                        };
                        if sel == Some(i) && p.mode != Mode::Linear && p.mode != Mode::Constant {
                            for arrive in [true, false] {
                                let slope = match (grab, live) {
                                    (Some(Grab::Handle { key, comp, arrive: a }), Some((_, _, Some(s)))) if key == i && comp == k && a == arrive => Some(s),
                                    _ => None,
                                };
                                let hp = m.handle(p, k, arrive, slope);
                                line((m.sx(p.time), m.sy(p.out[k])), hp, color.opacity(0.6), 1.0, window);
                                window.paint_quad(gpui::quad(Bounds::new(at(hp.0 - 4.0, hp.1 - 4.0), size(px(8.0), px(8.0))), px(4.0), gpui::white(), px(1.0), color, Default::default()));
                            }
                        }
                        let r = if sel == Some(i) { 5.0 } else { 4.0 };
                        let (x, y) = (m.sx(t), m.sy(v));
                        window.paint_quad(gpui::quad(Bounds::new(at(x - r, y - r), size(px(2.0 * r), px(2.0 * r))), px(r), if sel == Some(i) { sel_c } else { color }, px(1.0), dot_border, Default::default()));
                    }
                }
            },
        )
        .size_full();
        v_flex()
            .h(px(176.0))
            .flex_none()
            .border_t_1()
            .border_color(theme.border)
            .child(head)
            .child(
                div()
                    .id("curve-editor")
                    .flex_1()
                    .min_h_0()
                    .bg(theme.background.opacity(0.4))
                    .child(crate::driver::probe("curve-editor"))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, e, _, cx| this.curve_down(e, cx)))
                    .on_mouse_move(cx.listener(|this, e, _, cx| this.curve_move(e, cx)))
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.curve_up(cx)))
                    .on_scroll(cx.listener(|this, e, _, cx| this.curve_scroll(e, cx)))
                    .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                        if e.keystroke.key == "f" {
                            this.fit_curve();
                            cx.notify();
                        }
                    }))
                    .child(painted),
            )
            .into_any_element()
    }
}
