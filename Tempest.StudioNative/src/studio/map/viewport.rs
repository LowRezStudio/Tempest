//! The 3D view: the scene drawn into a canvas (a frame only when something
//! changed, at half resolution while the camera moves), the camera (left
//! drag orbits, right drag looks around and WASD/QE fly while it is held,
//! the arrows and Page Up/Down always fly, Shift faster, middle drag pans,
//! the wheel zooms), click picking, and the gizmo (W move, E rotate, R
//! scale, Q select only), which writes the actor on release.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use glam::{Mat4, Quat, Vec2, Vec3};
use gpui::{canvas, div, prelude::*, px, AnyElement, Bounds, Context, Corners, FocusHandle, MouseButton, Pixels, Point, RenderImage, Window};
use gpui_component::{h_flex, v_flex, ActiveTheme};

use super::{MapEditor, Mode, Sel, Tool};
use crate::map_render::{box_lines, circle, rgba, Cam, Cv, MapScene};

struct Inner {
    scene: Result<MapScene, String>,
    image: Option<(Arc<RenderImage>, (u32, u32))>,
    stale: Vec<Arc<RenderImage>>,
    dirty: bool,
    /// Half resolution while the camera moves.
    coarse: bool,
    /// The canvas, in window coordinates.
    bounds: Bounds<Pixels>,
    /// How long the last frame took to draw and read back, and its size.
    last_frame: (f32, (u32, u32)),
}

/// A gizmo handle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle {
    Axis(usize),
    /// The plane across the other two axes.
    Plane(usize),
    Ring(usize),
    Uniform,
}

#[derive(Clone, Copy)]
pub struct Gizmo {
    pub center: Vec3,
    pub axes: [Vec3; 3],
    pub size: f32,
}

#[derive(Clone)]
struct GizmoDrag {
    handle: Handle,
    start: Vec2,
    m0: Mat4,
    g: Gizmo,
    node: usize,
    sel: Sel,
    plane0: Option<Vec3>,
}

#[derive(Clone)]
enum Drag {
    /// Left button down: a click unless it moves, then an orbit.
    Press { start: Vec2, last: Vec2, orbit: bool },
    Look { last: Vec2 },
    Pan { last: Vec2 },
    Gizmo(GizmoDrag),
}

pub struct View3d {
    inner: Rc<RefCell<Inner>>,
    drag: Option<Drag>,
    /// Flying keys held (fwd, back, left, right, up, down) and Shift.
    keys: HashSet<&'static str>,
    fast: bool,
    flying: bool,
    settle: u64,
    pub hot: Option<Handle>,
    pub focus: Option<FocusHandle>,
}

const AXIS_COLORS: [u32; 3] = [0xf38ba8, 0xa6e3a1, 0x89b4fa];
const HOT: u32 = 0xf9e2af;

impl View3d {
    pub fn new() -> View3d {
        let scene = MapScene::new();
        View3d { inner: Rc::new(RefCell::new(Inner { scene, image: None, stale: Vec::new(), dirty: true, coarse: false, bounds: Bounds::default(), last_frame: (0.0, (0, 0)) })), drag: None, keys: HashSet::new(), fast: false, flying: false, settle: 0, hot: None, focus: None }
    }

    /// Change the scene (`None` without a GPU).
    pub fn with<R>(&self, f: impl FnOnce(&mut MapScene) -> R) -> Option<R> {
        self.inner.borrow_mut().scene.as_mut().ok().map(f)
    }

    pub fn dirty(&self) {
        self.inner.borrow_mut().dirty = true;
    }

    pub fn error(&self) -> Option<String> {
        self.inner.borrow().scene.as_ref().err().cloned()
    }

    /// The canvas size in points.
    pub fn size(&self) -> (f32, f32) {
        let b = self.inner.borrow().bounds;
        (f32::from(b.size.width).max(1.0), f32::from(b.size.height).max(1.0))
    }

    fn local(&self, p: Point<Pixels>) -> Vec2 {
        let b = self.inner.borrow().bounds;
        Vec2::new(f32::from(p.x - b.origin.x), f32::from(p.y - b.origin.y))
    }

    /// The last frame's time in milliseconds and its size in pixels.
    pub fn last_frame(&self) -> (f32, (u32, u32)) {
        self.inner.borrow().last_frame
    }

    pub fn cam(&self) -> Option<Cam> {
        self.with(|s| s.cam)
    }
}

/// Spherical angles of an offset (three's `Spherical`: theta around Y from +Z, phi from +Y).
fn spherical(v: Vec3) -> (f32, f32, f32) {
    let r = v.length().max(1e-6);
    (v.x.atan2(v.z), (v.y / r).clamp(-1.0, 1.0).acos(), r)
}

fn from_spherical(theta: f32, phi: f32, r: f32) -> Vec3 {
    Vec3::new(r * phi.sin() * theta.sin(), r * phi.cos(), r * phi.sin() * theta.cos())
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

impl MapEditor {
    // ----- the gizmo ----------------------------------------------------------------

    /// The gizmo on the selected actor: world axes to move and rotate, its own to scale.
    pub(super) fn gizmo(&self) -> Option<Gizmo> {
        if self.tool == Tool::Select {
            return None;
        }
        let k = self.selected_node()?;
        let cam = self.view.cam()?;
        let m = self.view.with(|s| s.layers[0].get(k).map(|n| n.matrix))??;
        let center = m.w_axis.truncate();
        let axes = if self.tool == Tool::Scale { [m.x_axis.truncate().normalize_or(Vec3::X), m.y_axis.truncate().normalize_or(Vec3::Y), m.z_axis.truncate().normalize_or(Vec3::Z)] } else { [Vec3::X, Vec3::Y, Vec3::Z] };
        // A constant size on screen.
        let size = (cam.eye - center).length() * 0.13;
        Some(Gizmo { center, axes, size })
    }

    fn gizmo_lines(&self, g: &Gizmo, out: &mut Vec<Cv>) {
        let col = |h: Handle, k: usize| rgba(if self.view.hot == Some(h) || matches!(&self.view.drag, Some(Drag::Gizmo(d)) if d.handle == h) { HOT } else { AXIS_COLORS[k] }, 1.0);
        let mut seg = |a: Vec3, b: Vec3, c: [f32; 4]| {
            out.push(Cv::new(a, c));
            out.push(Cv::new(b, c));
        };
        match self.tool {
            Tool::Translate => {
                for k in 0..3 {
                    let c = col(Handle::Axis(k), k);
                    let tip = g.center + g.axes[k] * g.size;
                    seg(g.center, tip, c);
                    let side = g.axes[(k + 1) % 3] * g.size * 0.06;
                    let up = g.axes[(k + 2) % 3] * g.size * 0.06;
                    let back = tip - g.axes[k] * g.size * 0.15;
                    for s in [side, -side, up, -up] {
                        seg(tip, back + s, c);
                    }
                    // The plane handle across the other two axes.
                    let (u, v) = (g.axes[(k + 1) % 3] * g.size * 0.3, g.axes[(k + 2) % 3] * g.size * 0.3);
                    let pc = col(Handle::Plane(k), k);
                    seg(g.center + u, g.center + u + v, pc);
                    seg(g.center + v, g.center + u + v, pc);
                }
            }
            Tool::Rotate => {
                for k in 0..3 {
                    let mut v = Vec::new();
                    circle(g.center, g.axes[k], g.size, 64, col(Handle::Ring(k), k), &mut v);
                    out.extend(v);
                }
            }
            Tool::Scale => {
                let mut boxes = Vec::new();
                for k in 0..3 {
                    let c = col(Handle::Axis(k), k);
                    let tip = g.center + g.axes[k] * g.size;
                    seg(g.center, tip, c);
                    let h = g.size * 0.05;
                    box_lines(tip - Vec3::splat(h), tip + Vec3::splat(h), c, &mut boxes);
                }
                out.extend(boxes);
                let h = g.size * 0.07;
                box_lines(g.center - Vec3::splat(h), g.center + Vec3::splat(h), rgba(if self.view.hot == Some(Handle::Uniform) { HOT } else { 0xcdd6f4 }, 1.0), out);
            }
            Tool::Select => {}
        }
    }

    /// The handle under a point of the view (within 8 points).
    fn gizmo_hit(&self, g: &Gizmo, p: Vec2) -> Option<Handle> {
        let cam = self.view.cam()?;
        let (w, h) = self.view.size();
        let pr = |v: Vec3| cam.project(v, w, h).map(|(x, y)| Vec2::new(x, y));
        let c = pr(g.center)?;
        let mut best: Option<(Handle, f32)> = None;
        let mut take = |h: Handle, d: f32| {
            if d < 8.0 && best.is_none_or(|(_, b)| d < b) {
                best = Some((h, d));
            }
        };
        match self.tool {
            Tool::Translate | Tool::Scale => {
                if self.tool == Tool::Scale && (p - c).length() < 9.0 {
                    take(Handle::Uniform, 0.0);
                }
                for k in 0..3 {
                    if let Some(t) = pr(g.center + g.axes[k] * g.size) {
                        take(Handle::Axis(k), seg_dist(p, c, t));
                    }
                    if self.tool == Tool::Translate {
                        let (u, v) = (g.axes[(k + 1) % 3] * g.size * 0.3, g.axes[(k + 2) % 3] * g.size * 0.3);
                        if let (Some(a), Some(b), Some(d)) = (pr(g.center + u), pr(g.center + u + v), pr(g.center + v)) {
                            // Inside the small square (as a quad on screen).
                            let inside = {
                                let q = [c, a, b, d];
                                let s = (0..4).map(|i| (q[(i + 1) % 4] - q[i]).perp_dot(p - q[i])).collect::<Vec<_>>();
                                s.iter().all(|x| *x >= 0.0) || s.iter().all(|x| *x <= 0.0)
                            };
                            if inside {
                                take(Handle::Plane(k), 4.0);
                            }
                        }
                    }
                }
            }
            Tool::Rotate => {
                for k in 0..3 {
                    let n = 64;
                    let u = g.axes[k].any_orthonormal_vector();
                    let v = g.axes[k].cross(u);
                    let pts: Vec<Option<Vec2>> = (0..=n).map(|i| {
                        let a = i as f32 / n as f32 * std::f32::consts::TAU;
                        pr(g.center + (u * a.cos() + v * a.sin()) * g.size)
                    }).collect();
                    for s in pts.windows(2) {
                        if let (Some(a), Some(b)) = (s[0], s[1]) {
                            take(Handle::Ring(k), seg_dist(p, a, b));
                        }
                    }
                }
            }
            Tool::Select => {}
        }
        best.map(|b| b.0)
    }

    /// Where a ray meets the plane through `o` with normal `n`.
    fn plane_point(&self, p: Vec2, o: Vec3, n: Vec3) -> Option<Vec3> {
        let cam = self.view.cam()?;
        let (w, h) = self.view.size();
        let (ro, rd) = cam.ray(p.x, p.y, w, h);
        let den = rd.dot(n);
        if den.abs() < 1e-6 {
            return None;
        }
        let t = (o - ro).dot(n) / den;
        (t > 0.0).then(|| ro + rd * t)
    }

    /// The node's matrix for the pointer at `p` during a gizmo drag.
    fn gizmo_matrix(&self, d: &GizmoDrag, p: Vec2) -> Option<Mat4> {
        let cam = self.view.cam()?;
        let (w, h) = self.view.size();
        let pr = |v: Vec3| cam.project(v, w, h).map(|(x, y)| Vec2::new(x, y));
        let g = d.g;
        let c = pr(g.center)?;
        let along = |k: usize| -> Option<f32> {
            // How far along axis k the pointer moved, in world units.
            let t = pr(g.center + g.axes[k] * g.size)?;
            let s = t - c;
            Some((p - d.start).dot(s) / s.length_squared().max(1e-6) * g.size)
        };
        Some(match (self.tool, d.handle) {
            (Tool::Translate, Handle::Axis(k)) => Mat4::from_translation(g.axes[k] * along(k)?) * d.m0,
            (Tool::Translate, Handle::Plane(k)) => {
                let now = self.plane_point(p, g.center, g.axes[k])?;
                Mat4::from_translation(now - d.plane0?) * d.m0
            }
            (Tool::Rotate, Handle::Ring(k)) => {
                let (a0, a1) = (d.start - c, p - c);
                let delta = a1.y.atan2(a1.x) - a0.y.atan2(a0.x);
                // The screen's y points down: facing the axis, a turn on screen is the other way round.
                let facing = g.axes[k].dot(cam.eye - g.center) > 0.0;
                let angle = if facing { -delta } else { delta };
                Mat4::from_translation(g.center) * Mat4::from_quat(Quat::from_axis_angle(g.axes[k], angle)) * Mat4::from_translation(-g.center) * d.m0
            }
            (Tool::Scale, Handle::Axis(k)) => {
                let f = (1.0 + along(k)? / g.size).max(0.01);
                let mut s = Vec3::ONE;
                s[k] = f;
                d.m0 * Mat4::from_scale(s)
            }
            (Tool::Scale, Handle::Uniform) => {
                let f = (1.0 - (p.y - d.start.y) / 100.0).max(0.01);
                d.m0 * Mat4::from_scale(Vec3::splat(f))
            }
            _ => return None,
        })
    }

    // ----- input ------------------------------------------------------------------------

    fn settle_later(&mut self, cx: &mut Context<Self>) {
        self.view.inner.borrow_mut().coarse = true;
        self.view.settle += 1;
        let seq = self.view.settle;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(220)).await;
            let _ = this.update(cx, |this, cx| {
                if this.view.settle == seq && this.view.drag.is_none() && this.view.keys.is_empty() {
                    this.view.inner.borrow_mut().coarse = false;
                    this.view.dirty();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn camera_moved(&mut self, cx: &mut Context<Self>) {
        self.view.dirty();
        self.settle_later(cx);
        cx.notify();
    }

    fn mouse_down(&mut self, e: &gpui::MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = &self.view.focus {
            f.focus(window, cx);
        }
        let p = self.view.local(e.position);

        match e.button {
            MouseButton::Left => {
                if let (Some(g), Some(k), Some(sel)) = (self.gizmo(), self.selected_node(), self.selected.clone()) {
                    if let Some(handle) = self.gizmo_hit(&g, p) {
                        let m0 = self.view.with(|s| s.layers[0][k].matrix).unwrap_or(Mat4::IDENTITY);
                        let plane0 = match handle {
                            Handle::Plane(a) => self.plane_point(p, g.center, g.axes[a]),
                            _ => None,
                        };
                        self.view.drag = Some(Drag::Gizmo(GizmoDrag { handle, start: p, m0, g, node: k, sel, plane0 }));
                        return;
                    }
                }
                self.view.drag = Some(Drag::Press { start: p, last: p, orbit: false });
            }
            MouseButton::Right => self.view.drag = Some(Drag::Look { last: p }),
            MouseButton::Middle => self.view.drag = Some(Drag::Pan { last: p }),
            _ => {}
        }
    }

    fn mouse_move(&mut self, e: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        let p = self.view.local(e.position);
        let Some(drag) = self.view.drag.clone() else {
            // Highlight the handle under the pointer.
            let hot = self.gizmo().and_then(|g| self.gizmo_hit(&g, p));
            if hot != self.view.hot {
                self.view.hot = hot;
                self.view.dirty();
                cx.notify();
            }
            return;
        };
        if e.pressed_button.is_none() {
            self.view.drag = None;
            return;
        }
        let (_, h) = self.view.size();
        match drag {
            Drag::Press { start, last, orbit } => {
                let orbit = orbit || (p - start).length() > 4.0;
                if orbit {
                    let d = p - last;
                    self.view.with(|s| {
                        let (theta, phi, r) = spherical(s.cam.eye - s.cam.target);
                        let theta = theta - d.x * std::f32::consts::TAU / h;
                        let phi = (phi - d.y * std::f32::consts::TAU / h).clamp(0.01, std::f32::consts::PI - 0.01);
                        s.cam.eye = s.cam.target + from_spherical(theta, phi, r);
                    });
                    self.camera_moved(cx);
                }
                self.view.drag = Some(Drag::Press { start, last: p, orbit });
            }
            Drag::Look { last } => {
                let d = p - last;
                // Turn the view around the camera: the orbit target moves, the camera stays.
                self.view.with(|s| {
                    let (theta, phi, r) = spherical(s.cam.target - s.cam.eye);
                    let theta = theta - d.x * 0.004;
                    let phi = (phi + d.y * 0.004).clamp(0.05, std::f32::consts::PI - 0.05);
                    s.cam.target = s.cam.eye + from_spherical(theta, phi, r.max(1.0));
                });
                self.view.drag = Some(Drag::Look { last: p });
                self.camera_moved(cx);
            }
            Drag::Pan { last } => {
                let d = p - last;
                self.view.with(|s| {
                    let dist = (s.cam.eye - s.cam.target).length();
                    let scale = 2.0 * dist * (Cam::FOV.to_radians() / 2.0).tan() / h;
                    let (f, r) = s.cam.axes();
                    let up = r.cross(f);
                    let mv = -r * d.x * scale + up * d.y * scale;
                    s.cam.eye += mv;
                    s.cam.target += mv;
                });
                self.view.drag = Some(Drag::Pan { last: p });
                self.camera_moved(cx);
            }
            Drag::Gizmo(d) => {
                if let Some(m) = self.gizmo_matrix(&d, p) {
                    self.view.with(|s| {
                        s.layers[0][d.node].matrix = m;
                        s.invalidate();
                    });
                    self.view.dirty();
                    cx.notify();
                }
            }
        }
    }

    fn mouse_up(&mut self, e: &gpui::MouseUpEvent, cx: &mut Context<Self>) {
        let p = self.view.local(e.position);
        let Some(drag) = self.view.drag.take() else { return };
        match drag {
            Drag::Press { orbit: false, .. } if e.button == MouseButton::Left => self.click(p, e.modifiers.shift, cx),
            Drag::Gizmo(d) => {
                let m = self.view.with(|s| s.layers[0][d.node].matrix).unwrap_or(d.m0);
                if (m - d.m0).abs_diff_eq(Mat4::ZERO, 1e-6) {
                    return;
                }
                self.move_actor(d.sel.0, d.sel.1, m, cx);
            }
            _ => {
                self.settle_later(cx);
            }
        }
    }

    /// A click: pick an actor (or nav mesh faces when picking them).
    fn click(&mut self, p: Vec2, shift: bool, cx: &mut Context<Self>) {
        let Some(cam) = self.view.cam() else { return };
        let (w, h) = self.view.size();
        let (o, d) = cam.ray(p.x, p.y, w, h);
        if self.nav_click(o, d, shift, cx) {
            return;
        }
        let hit = self.view.with(|s| s.pick(0, o, d)).flatten();
        let sel = hit.and_then(|(k, _)| self.node_actor(k).map(|(l, a)| (l.to_string(), a.export)));
        self.select(sel, true, cx);
    }

    fn scroll(&mut self, e: &gpui::ScrollEvent, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let dy = f32::from(e.delta.pixel_delta(px(16.0)).y);
        self.view.with(|s| {
            let off = s.cam.eye - s.cam.target;
            let f = (-dy * 0.002).exp();
            let len = (off.length() / f).clamp(0.2, 40000.0);
            s.cam.eye = s.cam.target + off.normalize_or(Vec3::Z) * len;
        });
        self.camera_moved(cx);
    }

    fn key_down(&mut self, e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = e.keystroke.key.as_str();
        let m = &e.keystroke.modifiers;
        self.view.fast = m.shift;
        if self.mode == Mode::Matinee && self.matinee_key(k, m, cx) {
            cx.stop_propagation();
            return;
        }
        if m.platform || m.control {
            if k == "d" {
                self.duplicate(cx);
                cx.stop_propagation();
            }
            return;
        }
        if m.alt {
            return;
        }
        let looking = matches!(self.view.drag, Some(Drag::Look { .. }));
        let fly = match k {
            "up" => Some("fwd"),
            "down" => Some("back"),
            "left" => Some("left"),
            "right" => Some("right"),
            "pageup" => Some("up"),
            "pagedown" => Some("down"),
            "w" if looking => Some("fwd"),
            "s" if looking => Some("back"),
            "a" if looking => Some("left"),
            "d" if looking => Some("right"),
            "e" if looking => Some("up"),
            "q" if looking => Some("down"),
            _ => None,
        };
        if let Some(f) = fly {
            self.view.keys.insert(f);
            self.start_flying(cx);
            cx.stop_propagation();
            return;
        }
        match k {
            "w" => self.tool = Tool::Translate,
            "e" => self.tool = Tool::Rotate,
            "r" => self.tool = Tool::Scale,
            "q" => self.tool = Tool::Select,
            "f" => self.frame_selection(),
            "escape" => self.select(None, true, cx),
            "delete" | "backspace" => self.ask_remove(cx),
            "a" if m.shift => self.toggle_palette(window, cx),
            _ => return,
        }
        self.view.dirty();
        cx.stop_propagation();
        cx.notify();
    }

    fn key_up(&mut self, e: &gpui::KeyUpEvent, cx: &mut Context<Self>) {
        let k = e.keystroke.key.as_str();
        self.view.fast = e.keystroke.modifiers.shift;
        let f = match k {
            "up" | "w" => "fwd",
            "down" | "s" => "back",
            "left" | "a" => "left",
            "right" | "d" => "right",
            "pageup" | "e" => "up",
            "pagedown" | "q" => "down",
            _ => return,
        };
        self.view.keys.remove(f);
        cx.notify();
    }

    /// Fly while movement keys are held: camera and orbit target move together.
    fn start_flying(&mut self, cx: &mut Context<Self>) {
        if self.view.flying {
            return;
        }
        self.view.flying = true;
        cx.spawn(async move |this, cx| {
            let mut last = std::time::Instant::now();
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                let go = this.update(cx, |this, cx| {
                    if this.view.keys.is_empty() || matches!(this.view.drag, Some(Drag::Gizmo(_))) {
                        this.view.flying = false;
                        this.settle_later(cx);
                        return false;
                    }
                    let dt = last.elapsed().as_secs_f32().min(0.1);
                    last = std::time::Instant::now();
                    let speed = if this.view.fast { 60.0 } else { 15.0 } * dt;
                    let keys = this.view.keys.clone();
                    this.view.with(|s| {
                        let (f, r) = s.cam.axes();
                        let mut mv = Vec3::ZERO;
                        for k in &keys {
                            mv += match *k {
                                "fwd" => f,
                                "back" => -f,
                                "right" => r,
                                "left" => -r,
                                "up" => Vec3::Y,
                                "down" => -Vec3::Y,
                                _ => Vec3::ZERO,
                            };
                        }
                        let mv = mv.normalize_or_zero() * speed;
                        s.cam.eye += mv;
                        s.cam.target += mv;
                    });
                    this.camera_moved(cx);
                    true
                });
                if !matches!(go, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    // ----- drawing ----------------------------------------------------------------------

    /// What is drawn over the scene this frame: the selection's box and light reach, the gizmo, Matinee paths.
    fn overlays(&mut self) {
        let mut over = Vec::new();
        let mut top = Vec::new();
        if let Some(k) = self.selected_node() {
            if let Some((lo, hi)) = self.view.with(|s| s.bounds(0, k)).flatten() {
                box_lines(lo, hi, rgba(0xf9e2af, 1.0), &mut over);
            }
            self.light_reach(&mut over);
        }
        self.matinee_paths(&mut over);
        self.nav_links(&mut top);
        if let Some(g) = self.gizmo() {
            self.gizmo_lines(&g, &mut top);
        }
        self.view.with(|s| {
            s.overlay = over;
            s.overlay_top = top;
        });
    }

    pub(super) fn render_viewport(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if self.view.focus.is_none() {
            self.view.focus = Some(cx.focus_handle());
        }
        if let Some(e) = self.view.error() {
            return div().size_full().flex().items_center().justify_center().text_sm().text_color(theme.danger).child(format!("The 3D view needs a GPU: {e}")).into_any_element();
        }
        // The background follows the theme (sRGB → linear for the clear colour).
        let bg = theme.background.to_rgb();
        let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } as f64;
        let clear = [lin(bg.r), lin(bg.g), lin(bg.b)];
        if self.view.with(|s| std::mem::replace(&mut s.clear, clear) != clear) == Some(true) {
            self.view.dirty();
        }
        self.overlays();
        let inner = self.view.inner.clone();
        let _ = window;
        let view = canvas(
            move |bounds: Bounds<Pixels>, window: &mut Window, _| {
                let mut st = inner.borrow_mut();
                st.bounds = bounds;
                let scale = if st.coarse { 1.0 } else { window.scale_factor() };
                let size = ((f32::from(bounds.size.width) * scale) as u32, (f32::from(bounds.size.height) * scale) as u32);
                if st.dirty || st.image.as_ref().map(|(_, s)| *s) != Some(size) {
                    let st = &mut *st;
                    if let Ok(scene) = &mut st.scene {
                        let t = std::time::Instant::now();
                        let image = scene.render(size.0, size.1);
                        st.last_frame = (t.elapsed().as_secs_f32() * 1000.0, size);
                        if let Some((old, _)) = st.image.replace((image, size)) {
                            st.stale.push(old);
                        }
                    }
                    st.dirty = false;
                }
                for old in std::mem::take(&mut st.stale) {
                    let _ = window.drop_image(old);
                }
                st.image.as_ref().map(|(i, _)| i.clone())
            },
            |bounds, image, window, _| {
                if let Some(image) = image {
                    let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
                }
            },
        )
        .size_full();
        let (loading, loaded, failed) = self.view.with(|s| s.mesh_counts()).unwrap_or_default();
        let progress = if loading > 0 || !self.mesh_queue.is_empty() {
            Some(format!("meshes {loaded}/{}{}", loaded + loading + failed, if failed > 0 { format!(", {failed} failed") } else { String::new() }))
        } else if failed > 0 {
            Some(format!("{failed} meshes could not be loaded (red boxes)"))
        } else {
            None
        };
        let focus = self.view.focus.clone().expect("made above");
        let legend = self.heavy.then(|| {
            let sw = |c: u32, t: &str| h_flex().gap_1p5().child(div().size(px(10.0)).rounded_sm().bg(super::hsla(c))).child(t.to_string());
            let mut l = v_flex().absolute().top(px(8.0)).left(px(8.0)).p_2().gap_0p5().rounded_md().bg(theme.background.opacity(0.85)).text_xs().child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Can't be stood on (m_bCanBaseOn False)"));
            l = if self.xray { l.child(sw(0xff2d55, "flagged meshes (seen through walls)")).child(sw(0xffb020, "flagged volumes")) } else { l.child(sw(0xf24d66, "walls: heavy walls")).child(sw(0xfa994d, "slopes")).child(sw(0x9499b3, "roofs and floors (as intended)")) };
            l.child(div().text_color(theme.muted_foreground).child(format!("{} actors flagged", self.rooftop_count())))
        });
        div()
            .id("map-viewport")
            .track_focus(&focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .child(crate::driver::probe("map-viewport"))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e, window, cx| this.mouse_down(e, window, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e, window, cx| this.mouse_down(e, window, cx)))
            .on_mouse_down(MouseButton::Middle, cx.listener(|this, e, window, cx| this.mouse_down(e, window, cx)))
            .on_mouse_move(cx.listener(|this, e, _, cx| this.mouse_move(e, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, e, _, cx| this.mouse_up(e, cx)))
            .on_mouse_up(MouseButton::Right, cx.listener(|this, e, _, cx| {
                for k in ["fwd", "back", "left", "right", "up", "down"] {
                    this.view.keys.remove(k);
                }
                this.mouse_up(e, cx)
            }))
            .on_mouse_up(MouseButton::Middle, cx.listener(|this, e, _, cx| this.mouse_up(e, cx)))
            .on_scroll(cx.listener(|this, e, _, cx| this.scroll(e, cx)))
            .on_key_down(cx.listener(|this, e, window, cx| this.key_down(e, window, cx)))
            .on_key_up(cx.listener(|this, e, _, cx| this.key_up(e, cx)))
            .child(view)
            .children(progress.map(|p| div().absolute().bottom(px(8.0)).left(px(8.0)).px_2().py_1().rounded_md().bg(theme.background.opacity(0.8)).text_xs().font_family("Menlo").child(p)))
            .children(legend)
            .child(
                div()
                    .absolute()
                    .bottom(px(8.0))
                    .right(px(8.0))
                    .text_size(px(10.0))
                    .font_family("Menlo")
                    .text_color(theme.muted_foreground)
                    .text_right()
                    .child("left drag orbit · right drag look, + WASD fly (Q E down/up) · middle drag pan · arrows fly · wheel zoom")
                    .child("click select · W move · E rotate · R scale · Q select only · F frame")
                    .child("⇧A add an actor · ⌘D duplicate · Delete delete"),
            )
            .into_any_element()
    }

    /// Driver and MCP: click the view at a point (window coordinates relative to the canvas).
    pub(crate) fn click_at(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        self.click(Vec2::new(x, y), false, cx);
    }

    /// Where a selected actor's gizmo handles are on screen (canvas points), for tests.
    pub(crate) fn gizmo_on_screen(&self) -> Option<serde_json::Value> {
        let g = self.gizmo()?;
        let cam = self.view.cam()?;
        let (w, h) = self.view.size();
        let pr = |v: Vec3| cam.project(v, w, h).map(|(x, y)| [x, y]);
        let b = self.view.inner.borrow().bounds;
        Some(serde_json::json!({
            "origin": [f32::from(b.origin.x), f32::from(b.origin.y)],
            "center": pr(g.center),
            "axes": (0..3).map(|k| pr(g.center + g.axes[k] * g.size)).collect::<Vec<_>>(),
        }))
    }
}
