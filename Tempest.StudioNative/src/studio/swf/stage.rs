//! The stage: the target (a timeline frame or one character) as Ruffle
//! draws it (`swf.preview` → the player thread's stills; the Movie node and
//! Play run a live player), and over it the stage outline, every object's
//! bounds and the selection. The visible area is fixed per target, so an
//! edit never re-fits the view; a new picture replaces the old one only
//! when it is drawn ("updating view…" meanwhile).
//!
//! Click selects the smallest object under the pointer (Alt+click cycles),
//! press and drag moves the selection (Shift: along one axis), double-click,
//! Enter or a Space tap edits inside a clip, Backspace goes up, arrows nudge
//! (Shift: 10 px). The transform tool (Q; V goes back) adds handles: corners
//! and edges scale (Shift keeps proportions, Alt from the centre), the knob
//! rotates (Shift: 15° steps). Middle-drag, Ctrl+drag or Space+drag pans, the
//! wheel scrolls (Shift: sideways), ⌘/Ctrl+wheel zooms at the pointer.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::{canvas, div, fill, outline, point, prelude::*, px, size, AnyElement, BorderStyle, Bounds, Context, Corners, FocusHandle, MouseButton, PathBuilder, Pixels, Point, RenderImage, Window};
use gpui_component::ActiveTheme;
use serde_json::{json, Value as J};

use super::geom::{Rect, M};
use super::{MovieEditor, Node, Tool};
use crate::swf_player::{self, Load, Pointer};

/// Space around the picture when it is fitted (pixels).
const PAD: f64 = 24.0;

struct Pic {
    image: Arc<RenderImage>,
    rect: Rect,
    target: String,
    /// The picture's pixels.
    px: (u32, u32),
}

/// An object on a layer of its own, as in the Electron Studio: once it is
/// edited, the frame is drawn without it and the object alone on a
/// transparent layer, so moving or scaling it only moves or stretches its
/// layer (gpui draws images without rotation: a rotated or skewed change
/// falls back to drawing the frame, and the layer is drawn again).
struct Float {
    depth: u64,
    target: String,
    /// The frame without the object: (request key, picture).
    base: Option<(String, Pic)>,
    /// The object alone: (request key, picture, the matrix it was drawn with).
    layer: Option<(String, Pic, M)>,
    /// Request keys in flight.
    asked: Vec<String>,
    /// Bumped to draw the layer again (its matrix was rotated).
    redraw: u64,
}

enum Drag {
    Move { entry: J, m0: M, p0: (f64, f64), start: Point<Pixels>, moved: bool, click: Option<u64> },
    Scale { entry: J, m0: M, hx: f64, hy: f64 },
    Rotate { entry: J, m0: M, p0: (f64, f64) },
}

pub struct Stage {
    /// 0 fits the view.
    pub zoom: f64,
    /// The point shown at the middle of the stage, in movie pixels from the view's centre.
    pub pan: (f64, f64),
    pic: Option<Pic>,
    stale: Vec<Arc<RenderImage>>,
    /// The request drawn last and the one in flight.
    shown: String,
    requested: String,
    generation: u64,
    busy: bool,
    error: Option<String>,
    missing: Vec<String>,
    /// The area chosen for each timeline target.
    views: HashMap<String, Rect>,
    /// A character target's bounds, from its preview.
    char_rect: HashMap<String, Rect>,
    viewport: Rc<Cell<Bounds<Pixels>>>,
    drag: Option<Drag>,
    live: Option<(u64, M)>,
    /// Set on release until the timeline reports the new matrix: (matrix it replaced, edit key, edit, matrix).
    committed: HashMap<u64, (J, String, J, M)>,
    panning: Option<(Point<Pixels>, (f64, f64))>,
    /// Space is held (since when, and whether it panned): a quick tap opens the selection.
    space: Option<(Instant, bool)>,
    pub focus: FocusHandle,
    /// Bumped when the package changes under the movie (a replaced texture, an undo).
    stamp: u64,
    /// The place the pan belongs to: another clip or character opens centred.
    pan_for: String,
    /// The live player: its request and generation, frames run, rate.
    live_key: String,
    playing: Option<(u64, u64, f64)>,
    scale: f64,
    float: Option<Float>,
    /// Pictures drawn, by request key (with their missing images), newest last; and the views rendered ahead.
    cache: Vec<(String, Pic, Vec<String>)>,
    prerendered: Vec<String>,
}

impl Stage {
    pub fn new(cx: &mut Context<MovieEditor>) -> Stage {
        Stage {
            zoom: 0.0,
            pan: (0.0, 0.0),
            pic: None,
            stale: Vec::new(),
            shown: String::new(),
            requested: String::new(),
            generation: 0,
            busy: false,
            error: None,
            missing: Vec::new(),
            views: HashMap::new(),
            char_rect: HashMap::new(),
            viewport: Default::default(),
            drag: None,
            live: None,
            committed: HashMap::new(),
            panning: None,
            space: None,
            focus: cx.focus_handle(),
            stamp: 0,
            pan_for: String::new(),
            live_key: String::new(),
            playing: None,
            scale: 1.0,
            float: None,
            cache: Vec::new(),
            prerendered: Vec::new(),
        }
    }

    pub fn fit(&mut self) {
        self.zoom = 0.0;
        self.pan = (0.0, 0.0);
    }

    pub fn zoom_by(&mut self, f: f64, cx: &mut Context<MovieEditor>) {
        let z = if self.zoom > 0.0 { self.zoom } else { self.scale };
        self.zoom = (z * f).clamp(0.05, 16.0);
        cx.notify();
    }

    /// Draw everything again (the package changed under the movie).
    pub fn invalidate(&mut self) {
        self.stamp += 1;
        self.committed.clear();
        self.float = None;
        self.prerendered.clear();
    }

    /// Another view: the live player stops, a drag in progress is dropped.
    pub fn leave(&mut self, _cx: &mut Context<MovieEditor>) {
        if self.playing.take().is_some() || !self.live_key.is_empty() {
            swf_player::worker().stop();
        }
        self.live_key.clear();
        self.drag = None;
        self.live = None;
        self.error = None;
    }

    pub fn summary(&self) -> J {
        json!({
            "zoom": self.zoom,
            "pan": [self.pan.0, self.pan.1],
            "picture": self.pic.as_ref().map(|p| p.rect.json()),
            "shownTarget": self.pic.as_ref().map(|p| p.target.clone()),
            "busy": self.busy,
            "error": self.error,
            "missing": self.missing,
            "playing": self.playing.map(|(_, n, fps)| json!({ "frames": n, "frameRate": fps })),
            "float": self.float.as_ref().map(|f| json!({ "depth": f.depth, "base": f.base.is_some(), "layer": f.layer.is_some() })),
            "cached": self.cache.len(),
            "prerendered": self.prerendered,
        })
    }
}

/// The area a timeline target shows when first opened: the root's stage, else
/// every object of the frame and the stage outline, with a 3 % margin.
fn default_view(root: bool, entries: &[J], outline: Option<Rect>) -> Option<Rect> {
    if root {
        if let Some(o) = outline {
            return Some(o);
        }
    }
    let mut r = outline;
    for e in entries {
        if let Some(er) = Rect::from_json(&e["rect"]) {
            r = Some(r.map(|a| a.union(&er)).unwrap_or(er));
        }
    }
    let r = r?;
    let pad = r.w.max(r.h) * 0.03;
    Some(Rect { x: r.x - pad, y: r.y - pad, w: r.w + pad * 2.0, h: r.h + pad * 2.0 })
}

fn fit_scale(rect: &Rect, vw: f64, vh: f64) -> f64 {
    ((vw - PAD * 2.0) / rect.w).min((vh - PAD * 2.0) / rect.h).clamp(0.02, 4.0)
}

impl MovieEditor {
    fn target_key(&self) -> String {
        self.target().map(|t| t.to_string()).unwrap_or_default()
    }

    /// The area the stage shows and the scale it draws at.
    fn stage_geometry(&mut self) -> (Option<Rect>, f64) {
        let b = self.stage.viewport.get();
        let (vw, vh) = (f64::from(f32::from(b.size.width)).max(64.0), f64::from(f32::from(b.size.height)).max(64.0));
        let key = self.target_key();
        let rect = if self.node == Node::Movie {
            let (w, h) = (self.info["width"].as_f64().unwrap_or(1.0), self.info["height"].as_f64().unwrap_or(1.0));
            Some(Rect { x: 0.0, y: 0.0, w, h })
        } else if self.target().is_some_and(|t| t["kind"] != "character") {
            match self.stage.views.get(&key) {
                Some(r) => Some(*r),
                None => {
                    let entries = self.entries();
                    let ready = self.timeline.as_ref().is_some_and(|t| t["sprite"].as_u64() == self.sprite());
                    let v = default_view(self.node == Node::Frames, &entries, self.outline_of(self.sprite())).filter(|_| ready);
                    if let Some(v) = v {
                        self.stage.views.insert(key.clone(), v);
                    }
                    v
                }
            }
        } else {
            self.stage.char_rect.get(&key).copied()
        };
        let scale = match (&rect, self.stage.zoom) {
            (_, z) if z > 0.0 => z,
            (Some(r), _) => fit_scale(r, vw, vh),
            _ => 1.0,
        };
        self.stage.scale = scale;
        (rect, scale)
    }

    /// The `swf.preview` request of a view and its key (what a picture is cached under).
    fn request_for(&self, target: &J, view: Option<Rect>, scale: f64, dpr: f64, hidden: Vec<u64>) -> (J, String) {
        let mut params = json!({ "path": self.path, "index": self.index, "target": target, "edits": self.pending(), "exclude": hidden });
        if let Some(v) = view {
            params["rect"] = v.json();
        }
        let b = self.stage.viewport.get();
        // Timeline views: the picture's pixels, rounded so small changes reuse it; a character's size
        // comes with its preview, so its key is the stage's size.
        let size = match view {
            Some(r) => format!("{}", (((r.w * scale * dpr) / 64.0).ceil() * 64.0).clamp(16.0, 4096.0) as u32),
            None => format!("{}x{}", (f32::from(b.size.width) / 64.0).round(), (f32::from(b.size.height) / 64.0).round()),
        };
        let key = format!("{params}|{size}|{}|{}", self.stage.zoom, self.stage.stamp);
        (params, key)
    }

    /// Render a request on the player thread; `on_stage`: it is what the stage asked for.
    fn render_request(&mut self, params: J, key: String, target: &J, dpr: f64, on_stage: bool, cx: &mut Context<Self>) {
        let b = self.stage.viewport.get();
        let (vw, vh) = (f64::from(f32::from(b.size.width)), f64::from(f32::from(b.size.height)));
        let generation = if on_stage {
            self.stage.requested = key.clone();
            self.stage.generation += 1;
            self.stage.busy = true;
            self.stage.generation
        } else {
            0
        };
        let (backend, zoom, transparent) = (self.backend.clone(), self.stage.zoom, target["kind"] != "root");
        let timeline = target["kind"] != "character";
        let tkey = target.to_string();
        cx.spawn(async move |this, cx| {
            let r: Result<(image::RgbaImage, Rect, Vec<String>), String> = async {
                let r = backend.call("swf.preview", params).await?;
                let area = Rect::from_json(&r.json["rect"]).ok_or("the preview has no area")?;
                let s = if zoom > 0.0 { zoom } else { fit_scale(&area, vw.max(64.0), vh.max(64.0)) };
                let w = (((area.w * s * dpr) / 64.0).ceil() * 64.0).clamp(16.0, 4096.0);
                let h = (w * area.h / area.w.max(0.01)).clamp(16.0, 4096.0);
                let libraries = r.json["libraries"].as_array().into_iter().flatten().filter_map(|l| Some((l["url"].as_str()?.to_string(), r.bin(&l["swf"])?.to_vec()))).collect();
                let swf = r.bin(&r.json["swf"]).ok_or("the preview has no movie")?.to_vec();
                let missing = r.json["missing"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();
                let img = swf_player::worker().still(Load { swf, libraries, width: w as u32, height: h as u32, transparent }, 12).await?;
                Ok((img, area, missing))
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                let current = on_stage && this.stage.generation == generation;
                if current {
                    this.stage.busy = false;
                    this.stage.requested.clear();
                }
                match r {
                    Ok((img, area, missing)) => {
                        let px = (img.width(), img.height());
                        let pic = Pic { image: swf_player::to_render_image(img), rect: area, target: tkey.clone(), px };
                        if !timeline {
                            this.stage.char_rect.entry(tkey.clone()).or_insert(area);
                        }
                        this.stage.cache.retain(|(k, ..)| *k != key);
                        this.stage.cache.push((key.clone(), Pic { image: pic.image.clone(), rect: area, target: tkey, px }, missing.clone()));
                        if this.stage.cache.len() > 24 {
                            let (_, old, _) = this.stage.cache.remove(0);
                            if this.stage.pic.as_ref().is_none_or(|p| !Arc::ptr_eq(&p.image, &old.image)) {
                                this.stage.stale.push(old.image);
                            }
                        }
                        if current {
                            this.stage.shown = key;
                            this.stage.error = None;
                            this.stage.missing = missing;
                            this.stage.pic = Some(pic);
                        }
                    }
                    Err(e) if current => {
                        this.stage.shown = key;
                        this.stage.error = Some(e);
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask for the picture the stage should show, when it is not the one shown or asked for.
    fn stage_request(&mut self, rect: Option<Rect>, scale: f64, dpr: f64, cx: &mut Context<Self>) {
        let Some(target) = self.target() else { return };
        let timeline = target["kind"] != "character";
        let view = if timeline { rect } else { None };
        if timeline && view.is_none() {
            return; // the timeline is not read yet
        }
        let hidden: Vec<u64> = if timeline { self.hidden().into_iter().collect() } else { Vec::new() };
        let (params, key) = self.request_for(&target, view, scale, dpr, hidden);
        if key == self.stage.shown || key == self.stage.requested {
            return;
        }
        // Drawn before (or ahead of time): shown at once.
        if let Some((_, pic, missing)) = self.stage.cache.iter().find(|(k, ..)| *k == key) {
            let pic = Pic { image: pic.image.clone(), rect: pic.rect, target: pic.target.clone(), px: pic.px };
            self.stage.missing = missing.clone();
            self.stage.error = None;
            self.stage.shown = key;
            self.stage.pic = Some(pic);
            self.stage.generation += 1;
            self.stage.busy = false;
            self.stage.requested.clear();
            return;
        }
        self.render_request(params, key, &target, dpr, true, cx);
    }

    /// Render a node's first view ahead of time, as the stage would ask for it
    /// (resting on a tree row, a clip selected on the stage).
    pub(super) fn prerender(&mut self, node: Node, cx: &mut Context<Self>) {
        if node == self.node || self.stage.viewport.get().size.width <= gpui::px(32.0) {
            return;
        }
        let pk = node.key();
        if self.stage.prerendered.contains(&pk) {
            return;
        }
        let (target, sprite) = match node {
            Node::Frames => (json!({ "kind": "root", "frame": 0 }), Some(0)),
            Node::Char(id) if self.kind_of(id) == "sprite" => (json!({ "kind": "frame", "id": id, "frame": 0 }), Some(id)),
            Node::Char(id) if self.kind_of(id) != "import" && self.char(id).is_some() => (json!({ "kind": "character", "id": id }), None),
            _ => return,
        };
        self.stage.prerendered.push(pk);
        let dpr = 2.0;
        let Some(sprite) = sprite else {
            let (params, key) = self.request_for(&target, None, 1.0, dpr, Vec::new());
            if !self.stage.cache.iter().any(|(k, ..)| *k == key) {
                self.render_request(params, key, &target, dpr, false, cx);
            }
            return;
        };
        // A timeline view needs its first frame's objects for its area.
        let call = self.backend.call("swf.timeline", json!({ "path": self.path, "index": self.index, "sprite": sprite, "edits": self.pending(), "limit": 1 }));
        cx.spawn(async move |this, cx| {
            let Ok(t) = call.await else { return };
            let _ = this.update(cx, |this, cx| {
                let entries = t.json["frames"][0].as_array().cloned().unwrap_or_default();
                let Some(view) = default_view(sprite == 0, &entries, this.outline_of(Some(sprite))) else { return };
                let b = this.stage.viewport.get();
                let scale = if this.stage.zoom > 0.0 { this.stage.zoom } else { fit_scale(&view, f64::from(f32::from(b.size.width)).max(64.0), f64::from(f32::from(b.size.height)).max(64.0)) };
                let hidden: Vec<u64> = this.hidden.get(&sprite).map(|h| h.iter().copied().collect()).unwrap_or_default();
                let (params, key) = this.request_for(&target, Some(view), scale, dpr, hidden);
                if !this.stage.cache.iter().any(|(k, ..)| *k == key) {
                    this.render_request(params, key, &target, dpr, false, cx);
                }
            });
        })
        .detach();
    }

    /// The Movie node and Play: a live player at the movie's rate.
    fn stage_live(&mut self, rect: Option<Rect>, scale: f64, dpr: f64, cx: &mut Context<Self>) {
        let target = self.target();
        let key = format!("{}|{}|{}|{}", self.node.key(), target.as_ref().map(|t| t.to_string()).unwrap_or_default(), self.version, self.stage.stamp);
        if key == self.stage.live_key {
            return;
        }
        self.stage.live_key = key.clone();
        self.stage.generation += 1;
        self.stage.busy = true;
        self.stage.error = None;
        let generation = self.stage.generation;
        let at = json!({ "path": self.path, "index": self.index });
        let movie = if self.node == Node::Movie {
            let mut p = at.clone();
            p["edits"] = self.pending();
            self.backend.call("swf.player", p)
        } else {
            self.backend.call("swf.preview", json!({ "path": self.path, "index": self.index, "target": target, "edits": self.pending() }))
        };
        let libs = self.backend.call("swf.libraries", at);
        let width = rect.map(|r| (r.w * scale * dpr).clamp(16.0, 4096.0) as u32).unwrap_or(1280);
        let tkey = target.map(|t| t.to_string()).unwrap_or_else(|| "movie".into());
        let movie_node = self.node == Node::Movie;
        cx.spawn(async move |this, cx| {
            let (movie, libs) = (movie.await, libs.await);
            let started: Result<(swf_player::Info, Rect), String> = async {
                let movie = movie?;
                let libs = libs?;
                let libraries = libs.json["libraries"].as_array().into_iter().flatten().filter_map(|l| Some((l["url"].as_str()?.to_string(), libs.bin(&l["swf"])?.to_vec()))).collect();
                let swf = movie.bin(&movie.json["swf"]).ok_or("no movie")?.to_vec();
                let area = if movie_node { None } else { Rect::from_json(&movie.json["rect"]) };
                let h = area.map(|a| (width as f64 * a.h / a.w.max(0.01)) as u32).unwrap_or(width * 9 / 16);
                let info = swf_player::worker().start(Load { swf, libraries, width, height: h.max(16), transparent: !movie_node }).await?;
                Ok((info, area.unwrap_or(Rect { x: 0.0, y: 0.0, w: info.width, h: info.height })))
            }
            .await;
            let Ok((info, area)) = started.map_err(|e| {
                let _ = this.update(cx, |this, cx| {
                    if this.stage.generation == generation {
                        this.stage.busy = false;
                        this.stage.error = Some(e);
                        cx.notify();
                    }
                });
            }) else {
                return;
            };
            let _ = this.update(cx, |this, _| {
                this.stage.busy = false;
                this.stage.char_rect.insert(tkey.clone(), area);
                this.stage.playing = Some((generation, 0, info.frame_rate));
            });
            let interval = std::time::Duration::from_secs_f64(1.0 / info.frame_rate.clamp(1.0, 120.0));
            let mut drawn = 0u32;
            loop {
                let started = Instant::now();
                let want = this.update(cx, |this, _| {
                    let b = this.stage.viewport.get();
                    let (vw, vh) = (f64::from(f32::from(b.size.width)).max(64.0), f64::from(f32::from(b.size.height)).max(64.0));
                    let s = if this.stage.zoom > 0.0 { this.stage.zoom } else { fit_scale(&area, vw, vh) };
                    (this.stage.generation == generation).then_some(((area.w * s * dpr).clamp(16.0, 4096.0)) as u32)
                });
                let Ok(Some(w)) = want else { break };
                let size = (w.abs_diff(drawn) > 32).then(|| {
                    drawn = w;
                    (w, ((w as f64 * area.h / area.w.max(0.01)) as u32).max(16))
                });
                let pic = swf_player::worker().advance(1, size).await;
                let go = this.update(cx, |this, cx| {
                    if this.stage.generation != generation {
                        return false;
                    }
                    match pic {
                        Ok(img) => {
                            if let Some(p) = &mut this.stage.playing {
                                p.1 += 1;
                            }
                            let px = (img.width(), img.height());
                            if let Some(old) = this.stage.pic.replace(Pic { image: swf_player::to_render_image(img), rect: area, target: tkey.clone(), px }) {
                                this.stage.stale.push(old.image);
                            }
                            cx.notify();
                            true
                        }
                        Err(e) => {
                            this.stage.error = Some(e);
                            this.stage.playing = None;
                            cx.notify();
                            false
                        }
                    }
                });
                if !matches!(go, Ok(true)) {
                    break;
                }
                cx.background_executor().timer(interval.saturating_sub(started.elapsed()).max(std::time::Duration::from_millis(1))).await;
            }
        })
        .detach();
    }

    // ----- floating layers ----------------------------------------------------------

    /// Give the object at `depth` a layer of its own (before editing its placement).
    pub(super) fn float(&mut self, depth: u64) {
        let target = self.target_key();
        if self.stage.float.as_ref().is_some_and(|f| f.depth == depth && f.target == target) {
            return;
        }
        self.stage.float = Some(Float { depth, target, base: None, layer: None, asked: Vec::new(), redraw: 0 });
    }

    /// An object's matrix with its pending edit (the timeline may not have caught up).
    fn pending_matrix(&self, e: &J) -> M {
        let mut m = e["matrix"].clone();
        let key = format!("place|{}|{}", self.sprite().unwrap_or(0), e["matrixBy"]);
        if let Some(edit) = self.edit(&key) {
            for (k, v) in edit["matrix"].as_object().into_iter().flatten() {
                m[k] = v.clone();
            }
        }
        M::from_json(&m)
    }

    /// The floating object's change since its layer was drawn, when it is a
    /// move and/or a positive axis-aligned scale (what a picture can show).
    fn float_delta(&self) -> Option<M> {
        let f = self.stage.float.as_ref()?;
        let (_, _, m0) = f.layer.as_ref()?;
        let e = self.entries().into_iter().find(|e| e["depth"].as_u64() == Some(f.depth))?;
        let cur = match self.stage.live {
            Some((d, m)) if d == f.depth => m,
            _ => self.pending_matrix(&e),
        };
        let d = cur.mul(&m0.invert()?);
        (d.b.abs() < 1e-6 && d.c.abs() < 1e-6 && d.a > 0.0 && d.d > 0.0).then_some(d)
    }

    /// Ask for the floating object's two pictures when they are missing or out of date.
    fn float_requests(&mut self, rect: Option<Rect>, scale: f64, dpr: f64, cx: &mut Context<Self>) {
        let Some(view) = rect else { return };
        let target = self.target_key();
        let Some(f) = self.stage.float.as_ref() else { return };
        if f.target != target || !self.layout() {
            self.stage.float = None;
            return;
        }
        let depth = f.depth;
        let Some(e) = self.entries().into_iter().find(|e| e["depth"].as_u64() == Some(depth)) else {
            self.stage.float = None;
            return;
        };
        // A rotated or skewed change shows by drawing the frame; once let go, the layer is drawn again.
        if self.stage.drag.is_none() && f.layer.is_some() && self.float_delta().is_none() {
            if let Some(f) = &mut self.stage.float {
                f.redraw += 1;
                f.layer = None;
            }
        }
        let s = self.sprite().unwrap_or(0);
        let tags = self.tags_of(&e);
        let own = |k: &str| tags.iter().any(|t| k == format!("place|{s}|{t}"));
        let others: Vec<J> = self.edits.iter().filter(|(k, _)| !own(k)).map(|(_, e)| e.clone()).collect();
        // The object's own edits other than where it is (alpha, name) change its layer.
        let looks: Vec<J> = self.edits.iter().filter(|(k, _)| own(k)).map(|(_, e)| {
            let mut e = e.clone();
            if let Some(o) = e.as_object_mut() {
                o.remove("matrix");
            }
            e
        }).collect();
        let mut hidden: Vec<u64> = self.hidden().into_iter().collect();
        hidden.push(depth);
        let all: Vec<u64> = self.entries().iter().filter_map(|x| x["depth"].as_u64()).filter(|d| *d != depth).collect();
        let Some(t) = self.target() else { return };
        let px_w = (((view.w * scale * dpr) / 64.0).ceil() * 64.0).clamp(16.0, 4096.0) as u32;
        let common = format!("{}|{px_w}|{}|{}", self.stage.zoom, self.stage.stamp, view.json());
        let base = json!({ "path": self.path, "index": self.index, "target": t, "edits": others, "exclude": hidden, "rect": view.json() });
        let base_key = format!("base|{base}|{common}");
        let layer = json!({ "path": self.path, "index": self.index, "target": t, "edits": self.pending(), "exclude": all, "rect": view.json() });
        let f = self.stage.float.as_ref().unwrap();
        let layer_key = format!("layer|{}|{}|{}|{}|{common}", J::Array(others.clone()), J::Array(looks), f.redraw, t);
        let want_base = f.base.as_ref().is_none_or(|b| b.0 != base_key) && !f.asked.contains(&base_key);
        let want_layer = f.layer.as_ref().is_none_or(|l| l.0 != layer_key) && !f.asked.contains(&layer_key);
        let drawn_with = self.pending_matrix(&e);
        for (want, key, params, is_layer) in [(want_base, base_key, base, false), (want_layer, layer_key, layer, true)] {
            if !want {
                continue;
            }
            if let Some(f) = &mut self.stage.float {
                f.asked.push(key.clone());
            }
            let (backend, w) = (self.backend.clone(), px_w);
            let tkey = target.clone();
            cx.spawn(async move |this, cx| {
                let r: Result<image::RgbaImage, String> = async {
                    let r = backend.call("swf.preview", params).await?;
                    let area = Rect::from_json(&r.json["rect"]).ok_or("no area")?;
                    let h = (w as f64 * area.h / area.w.max(0.01)).clamp(16.0, 4096.0) as u32;
                    let libraries = r.json["libraries"].as_array().into_iter().flatten().filter_map(|l| Some((l["url"].as_str()?.to_string(), r.bin(&l["swf"])?.to_vec()))).collect();
                    let swf = r.bin(&r.json["swf"]).ok_or("no movie")?.to_vec();
                    swf_player::worker().still(Load { swf, libraries, width: w, height: h, transparent: is_layer || tkey.contains("\"frame\"") }, 12).await
                }
                .await;
                let _ = this.update(cx, |this, cx| {
                    let Some(f) = this.stage.float.as_mut().filter(|f| f.depth == depth) else { return };
                    f.asked.retain(|k| *k != key);
                    if let Ok(img) = r {
                        let px = (img.width(), img.height());
                        let pic = Pic { image: swf_player::to_render_image(img), rect: view, target: tkey, px };
                        let old = if is_layer { f.layer.replace((key, pic, drawn_with)).map(|l| l.1.image) } else { f.base.replace((key, pic)).map(|b| b.1.image) };
                        this.stage.stale.extend(old);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    // ----- picking and dragging -----------------------------------------------------

    fn local_of(&self, e: &J) -> Option<Rect> {
        Rect::from_json(&self.char(e["character"].as_u64()?)?["rect"])
    }

    /// The matrix an object is drawn with: the drag's, a just committed one, or the timeline's.
    fn matrix_of(&self, e: &J) -> M {
        let depth = e["depth"].as_u64().unwrap_or(0);
        if let Some((d, m)) = self.stage.live {
            if d == depth {
                return m;
            }
        }
        if let Some((from, key, edit, m)) = self.stage.committed.get(&depth) {
            if *from == e["matrix"] && self.edit(key) == Some(edit) {
                return *m;
            }
        }
        M::from_json(&e["matrix"])
    }

    fn pickable(&self) -> Vec<J> {
        let hidden = self.hidden();
        self.entries().into_iter().filter(|e| !e["rect"].is_null() && e["clipDepth"].is_null() && !hidden.contains(&e["depth"].as_u64().unwrap_or(0))).collect()
    }

    fn contains(&self, e: &J, x: f64, y: f64) -> bool {
        if self.depth == e["depth"].as_u64() {
            if let (Some(l), Some(inv)) = (self.local_of(e), self.matrix_of(e).invert()) {
                let (lx, ly) = inv.apply(x, y);
                return l.contains(lx, ly);
            }
        }
        Rect::from_json(&e["rect"]).is_some_and(|r| r.contains(x, y))
    }

    /// Objects under a movie point, smallest first.
    fn under(&self, x: f64, y: f64) -> Vec<J> {
        let mut hits: Vec<J> = self.pickable().into_iter().filter(|e| self.contains(e, x, y)).collect();
        let area = |e: &J| Rect::from_json(&e["rect"]).map(|r| r.w * r.h).unwrap_or(0.0);
        hits.sort_by(|a, b| area(a).total_cmp(&area(b)));
        hits
    }

    /// Movie pixels ↔ stage pixels (relative to the stage's top-left).
    fn to_stage(&self, rect: &Rect, scale: f64, x: f64, y: f64) -> (f64, f64) {
        let b = self.stage.viewport.get();
        let (vw, vh) = (f64::from(f32::from(b.size.width)), f64::from(f32::from(b.size.height)));
        let left = vw / 2.0 - rect.w * scale / 2.0 - self.stage.pan.0 * scale;
        let top = vh / 2.0 - rect.h * scale / 2.0 - self.stage.pan.1 * scale;
        (left + (x - rect.x) * scale, top + (y - rect.y) * scale)
    }

    fn to_movie(&self, rect: &Rect, scale: f64, p: Point<Pixels>) -> (f64, f64) {
        let b = self.stage.viewport.get();
        let (sx, sy) = (f64::from(f32::from(p.x - b.origin.x)), f64::from(f32::from(p.y - b.origin.y)));
        let (ox, oy) = self.to_stage(rect, scale, rect.x, rect.y);
        ((sx - ox) / scale + rect.x, (sy - oy) / scale + rect.y)
    }

    /// The handles of the selection in stage pixels: (hx, hy, point).
    fn handles(&self, rect: &Rect, scale: f64) -> Vec<(f64, f64, (f64, f64))> {
        let Some(e) = self.selected_entry() else { return Vec::new() };
        let (Some(l), m) = (self.local_of(&e), self.matrix_of(&e)) else { return Vec::new() };
        [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.5, 1.0), (0.0, 1.0), (0.0, 0.5)]
            .into_iter()
            .map(|(hx, hy)| {
                let (x, y) = m.apply(l.x + hx * l.w, l.y + hy * l.h);
                (hx, hy, self.to_stage(rect, scale, x, y))
            })
            .collect()
    }

    /// The rotation knob: above the top edge's middle, away from the centre.
    fn knob(&self, rect: &Rect, scale: f64) -> Option<((f64, f64), (f64, f64))> {
        let e = self.selected_entry()?;
        let (l, m) = (self.local_of(&e)?, self.matrix_of(&e));
        let top = { let (x, y) = m.apply(l.x + l.w / 2.0, l.y); self.to_stage(rect, scale, x, y) };
        let mid = { let (x, y) = m.apply(l.x + l.w / 2.0, l.y + l.h / 2.0); self.to_stage(rect, scale, x, y) };
        let (mut dx, mut dy) = (top.0 - mid.0, top.1 - mid.1);
        let n = dx.hypot(dy);
        if n < 1.0 {
            (dx, dy) = (0.0, -1.0);
        } else {
            dx /= n;
            dy /= n;
        }
        Some((top, (top.0 + dx * 22.0, top.1 + dy * 22.0)))
    }

    fn stage_down(&mut self, e: &gpui::MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.stage.focus.focus(window, cx);
        let (rect, scale) = self.stage_geometry();
        if self.node == Node::Movie || self.play {
            if let Some(p) = self.live_pointer(e.position) {
                swf_player::worker().pointer(Pointer::Down(p.0, p.1));
            }
            return;
        }
        // Space or Ctrl held: the left button pans.
        if self.stage.space.is_some() || e.modifiers.control {
            if let Some(s) = &mut self.stage.space {
                s.1 = true;
            }
            self.stage.panning = Some((e.position, self.stage.pan));
            return;
        }
        let Some(rect) = rect else { return };
        if !self.layout() {
            return;
        }
        let b = self.stage.viewport.get();
        let at = (f64::from(f32::from(e.position.x - b.origin.x)), f64::from(f32::from(e.position.y - b.origin.y)));
        if let (Tool::Transform, Some(entry)) = (self.tool, self.selected_entry()) {
            let m0 = self.matrix_of(&entry);
            if let Some((_, k)) = self.knob(&rect, scale) {
                if (k.0 - at.0).hypot(k.1 - at.1) <= 7.0 {
                    if let Some(d) = entry["depth"].as_u64() {
                        self.float(d);
                    }
                    let p0 = self.to_movie(&rect, scale, e.position);
                    self.stage.drag = Some(Drag::Rotate { entry, m0, p0 });
                    return;
                }
            }
            if let Some((hx, hy, _)) = self.handles(&rect, scale).into_iter().find(|(_, _, p)| (p.0 - at.0).abs() <= 6.0 && (p.1 - at.1).abs() <= 6.0) {
                if let Some(d) = entry["depth"].as_u64() {
                    self.float(d);
                }
                self.stage.drag = Some(Drag::Scale { entry, m0, hx, hy });
                return;
            }
        }
        let (x, y) = self.to_movie(&rect, scale, e.position);
        let hits = self.under(x, y);
        if e.click_count >= 2 {
            // Alt+clicks cycle; two quick ones must not enter one.
            if !e.modifiers.alt {
                if let Some(sel) = self.selected_entry() {
                    self.stage.drag = None;
                    self.enter(&sel, cx);
                }
            }
            return;
        }
        if hits.is_empty() {
            self.depth = None;
            cx.notify();
            return;
        }
        let sel = self.depth.and_then(|d| hits.iter().position(|h| h["depth"].as_u64() == Some(d)));
        let (entry, click) = match sel {
            Some(i) if e.modifiers.alt => (hits[(i + 1) % hits.len()].clone(), None),
            Some(i) => {
                // Pressing on the selection drags it; a plain click still picks the smallest object under the pointer.
                let click = (i != 0).then(|| hits[0]["depth"].as_u64()).flatten();
                (hits[i].clone(), click)
            }
            None => (hits[0].clone(), None),
        };
        self.depth = entry["depth"].as_u64();
        self.confirm_remove = false;
        if let Some(d) = self.depth {
            self.float(d);
        }
        let m0 = self.matrix_of(&entry);
        self.stage.drag = Some(Drag::Move { entry, m0, p0: (x, y), start: e.position, moved: false, click });
        cx.notify();
    }

    fn stage_move(&mut self, e: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        if self.node == Node::Movie || self.play {
            if let Some(p) = self.live_pointer(e.position) {
                swf_player::worker().pointer(Pointer::Move(p.0, p.1));
            }
            return;
        }
        let (rect, scale) = self.stage_geometry();
        if let Some((start, from)) = self.stage.panning {
            if !e.dragging() && e.pressed_button.is_none() {
                self.stage.panning = None;
                return;
            }
            self.stage.pan = (from.0 - f64::from(f32::from(e.position.x - start.x)) / scale, from.1 - f64::from(f32::from(e.position.y - start.y)) / scale);
            cx.notify();
            return;
        }
        let Some(rect) = rect else { return };
        let Some(drag) = &mut self.stage.drag else { return };
        if !e.dragging() {
            self.stage.drag = None;
            self.stage.live = None;
            return;
        }
        let local = |entry: &J, this: &MovieEditor| this.local_of(entry);
        let (x, y) = {
            let b = self.stage.viewport.get();
            let (sx, sy) = (f64::from(f32::from(e.position.x - b.origin.x)), f64::from(f32::from(e.position.y - b.origin.y)));
            let bw = f64::from(f32::from(b.size.width));
            let bh = f64::from(f32::from(b.size.height));
            let left = bw / 2.0 - rect.w * scale / 2.0 - self.stage.pan.0 * scale;
            let top = bh / 2.0 - rect.h * scale / 2.0 - self.stage.pan.1 * scale;
            ((sx - left) / scale + rect.x, (sy - top) / scale + rect.y)
        };
        let live = match drag {
            Drag::Move { entry, m0, p0, start, moved, .. } => {
                let far = f32::from((e.position.x - start.x).abs() + (e.position.y - start.y).abs()) > 3.0;
                if !*moved && !far {
                    return;
                }
                *moved = true;
                let (mut dx, mut dy) = (x - p0.0, y - p0.1);
                if e.modifiers.shift {
                    if dx.abs() > dy.abs() {
                        dy = 0.0;
                    } else {
                        dx = 0.0;
                    }
                }
                Some((entry["depth"].as_u64().unwrap_or(0), M::translate(dx, dy).mul(m0)))
            }
            Drag::Scale { entry, m0, hx, hy } => {
                let (entry, m0, hx, hy) = (entry.clone(), *m0, *hx, *hy);
                let (Some(inv), Some(l)) = (m0.invert(), local(&entry, self)) else { return };
                let (px, py) = inv.apply(x, y);
                let (hpx, hpy) = (l.x + hx * l.w, l.y + hy * l.h);
                let cx0 = if e.modifiers.alt { l.x + l.w / 2.0 } else { l.x + (1.0 - hx) * l.w };
                let cy0 = if e.modifiers.alt { l.y + l.h / 2.0 } else { l.y + (1.0 - hy) * l.h };
                let mut sx = if hx == 0.5 || hpx == cx0 { 1.0 } else { (px - cx0) / (hpx - cx0) };
                let mut sy = if hy == 0.5 || hpy == cy0 { 1.0 } else { (py - cy0) / (hpy - cy0) };
                if e.modifiers.shift {
                    let s = if hx == 0.5 { sy } else if hy == 0.5 || sx.abs() > sy.abs() { sx } else { sy };
                    sx = if sx < 0.0 { -s.abs() } else { s.abs() };
                    sy = if sy < 0.0 { -s.abs() } else { s.abs() };
                }
                let clamp = |v: f64| if v.abs() < 0.01 { if v < 0.0 { -0.01 } else { 0.01 } } else { v };
                Some((entry["depth"].as_u64().unwrap_or(0), m0.mul(&M::translate(cx0, cy0).mul(&M::scaling(clamp(sx), clamp(sy))).mul(&M::translate(-cx0, -cy0)))))
            }
            Drag::Rotate { entry, m0, p0 } => {
                let (entry, m0, p0) = (entry.clone(), *m0, *p0);
                let Some(l) = local(&entry, self) else { return };
                let (cx0, cy0) = m0.apply(l.x + l.w / 2.0, l.y + l.h / 2.0);
                let mut r = (y - cy0).atan2(x - cx0) - (p0.1 - cy0).atan2(p0.0 - cx0);
                if e.modifiers.shift {
                    let step = std::f64::consts::PI / 12.0;
                    r = (r / step).round() * step;
                }
                Some((entry["depth"].as_u64().unwrap_or(0), M::translate(cx0, cy0).mul(&M::rotation(r)).mul(&M::translate(-cx0, -cy0)).mul(&m0)))
            }
        };
        self.stage.live = live;
        cx.notify();
    }

    fn stage_up(&mut self, e: &gpui::MouseUpEvent, cx: &mut Context<Self>) {
        if self.node == Node::Movie || self.play {
            if let Some(p) = self.live_pointer(e.position) {
                swf_player::worker().pointer(Pointer::Up(p.0, p.1));
            }
            return;
        }
        self.stage.panning = None;
        let Some(drag) = self.stage.drag.take() else { return };
        let live = self.stage.live.take();
        let (entry, moved, click) = match &drag {
            Drag::Move { entry, moved, click, .. } => (entry.clone(), *moved, *click),
            Drag::Scale { entry, .. } | Drag::Rotate { entry, .. } => (entry.clone(), true, None),
        };
        match live {
            Some((depth, m)) if moved => {
                self.set_matrix(&entry, m, cx);
                let key = format!("place|{}|{}", self.sprite().unwrap_or(0), entry["matrixBy"]);
                let edit = self.edit(&key).cloned().unwrap_or_default();
                self.stage.committed.insert(depth, (entry["matrix"].clone(), key, edit, M { tx: super::geom::twips(m.tx), ty: super::geom::twips(m.ty), ..m }));
            }
            _ => {
                if let Some(d) = click {
                    self.depth = Some(d);
                }
            }
        }
        cx.notify();
    }

    fn stage_scroll(&mut self, e: &gpui::ScrollEvent, cx: &mut Context<Self>) {
        let (rect, scale) = self.stage_geometry();
        let d = e.delta.pixel_delta(px(16.0));
        let (mut dx, mut dy) = (f64::from(f32::from(d.x)), f64::from(f32::from(d.y)));
        if e.modifiers.platform || e.modifiers.control {
            // Zoom at the pointer: the movie point under it stays put.
            let Some(_) = rect else { return };
            let b = self.stage.viewport.get();
            let px_ = f64::from(f32::from(e.position.x - b.origin.x)) - f64::from(f32::from(b.size.width)) / 2.0;
            let py_ = f64::from(f32::from(e.position.y - b.origin.y)) - f64::from(f32::from(b.size.height)) / 2.0;
            let f = 1.25f64.powf(dy / 100.0).clamp(0.5, 2.0);
            let z = ((if self.stage.zoom > 0.0 { self.stage.zoom } else { scale }) * f).clamp(0.05, 16.0);
            self.stage.pan = (self.stage.pan.0 + px_ / scale - px_ / z, self.stage.pan.1 + py_ / scale - py_ / z);
            self.stage.zoom = z;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if e.modifiers.shift && dx == 0.0 {
            (dx, dy) = (dy, 0.0);
        }
        self.stage.pan = (self.stage.pan.0 - dx / scale, self.stage.pan.1 - dy / scale);
        cx.stop_propagation();
        cx.notify();
    }

    fn stage_key(&mut self, e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = e.keystroke.key.as_str();
        let m = e.keystroke.modifiers;
        if key == "space" {
            if self.stage.space.is_none() {
                self.stage.space = Some((Instant::now(), false));
            }
            cx.stop_propagation();
            return;
        }
        if key == "escape" {
            self.depth = None;
            cx.notify();
            return;
        }
        // The window's shortcuts (⌘Z, Back…) are not the stage's.
        if m.control || m.alt || m.platform {
            return;
        }
        let sel = self.selected_entry();
        match key {
            "v" => self.tool = Tool::Move,
            "q" => self.tool = Tool::Transform,
            "enter" => {
                if let Some(s) = &sel {
                    self.enter(s, cx);
                }
            }
            "backspace" => self.up(cx),
            "delete" => {
                if let Some(s) = &sel {
                    self.remove_entry(s, cx);
                }
            }
            "left" | "right" | "up" | "down" => {
                let Some(s) = &sel else { return };
                let step = if m.shift { 10.0 } else { 1.0 };
                let (dx, dy) = match key {
                    "left" => (-step, 0.0),
                    "right" => (step, 0.0),
                    "up" => (0.0, -step),
                    _ => (0.0, step),
                };
                self.move_entry(s, dx, dy, cx);
            }
            _ => return,
        }
        let _ = window;
        cx.stop_propagation();
        cx.notify();
    }

    fn stage_key_up(&mut self, e: &gpui::KeyUpEvent, cx: &mut Context<Self>) {
        if e.keystroke.key == "space" {
            let tap = self.stage.space.take().is_some_and(|(at, panned)| !panned && at.elapsed().as_millis() < 400);
            if tap {
                if let Some(s) = self.selected_entry() {
                    self.enter(&s, cx);
                }
            }
        }
    }

    /// A window point as pixels of the live player's picture.
    fn live_pointer(&self, p: Point<Pixels>) -> Option<(f64, f64)> {
        let pic = self.stage.pic.as_ref()?;
        let b = self.stage.viewport.get();
        let (vw, vh) = (f64::from(f32::from(b.size.width)), f64::from(f32::from(b.size.height)));
        let s = self.stage.scale;
        let left = vw / 2.0 - pic.rect.w * s / 2.0 - self.stage.pan.0 * s;
        let top = vh / 2.0 - pic.rect.h * s / 2.0 - self.stage.pan.1 * s;
        let (x, y) = (f64::from(f32::from(p.x - b.origin.x)) - left, f64::from(f32::from(p.y - b.origin.y)) - top);
        let (w, h) = (pic.rect.w * s, pic.rect.h * s);
        (x >= 0.0 && y >= 0.0 && x <= w && y <= h).then(|| (x / w * pic.px.0 as f64, y / h * pic.px.1 as f64))
    }

    pub(super) fn render_stage(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let dpr = f64::from(window.scale_factor());
        // Another clip or character opens centred.
        let place = format!("{}|{}", self.node.key(), self.play);
        if self.stage.pan_for != place {
            self.stage.pan_for = place;
            self.stage.pan = (0.0, 0.0);
        }
        let (rect, scale) = self.stage_geometry();
        if self.node == Node::Movie || self.play {
            self.stage_live(rect, scale, dpr, cx);
        } else {
            self.float_requests(rect, scale, dpr, cx);
        }
        // A floating object with both pictures drawn and a change they can show: no frame render needed.
        let floated = self.stage.float.as_ref().and_then(|f| {
            let (_, base) = f.base.as_ref()?;
            let (_, layer, _) = f.layer.as_ref()?;
            let d = self.float_delta()?;
            let r = layer.rect;
            let moved = Rect { x: d.a * r.x + d.tx, y: d.d * r.y + d.ty, w: d.a * r.w, h: d.d * r.h };
            Some((base.image.clone(), base.rect, layer.image.clone(), moved))
        });
        if floated.is_none() && self.node != Node::Movie && !self.play {
            self.stage_request(rect, scale, dpr, cx);
        }
        // A selected clip is often entered next: render it ahead.
        if let Some(id) = self.selected_entry().and_then(|e| e["character"].as_u64()).filter(|id| self.kind_of(*id) == "sprite") {
            self.prerender(Node::Char(id), cx);
        }
        let tkey = if self.node == Node::Movie { "movie".to_string() } else { self.target_key() };
        let ready = floated.is_some() || self.stage.pic.as_ref().is_some_and(|p| p.target == tkey);
        let pic = if floated.is_some() { None } else { self.stage.pic.as_ref().filter(|_| ready).map(|p| (p.image.clone(), p.rect)) };
        let stale = std::mem::take(&mut self.stage.stale);
        let viewport = self.stage.viewport.clone();
        let layout = self.layout();
        // What the overlay draws, in stage pixels.
        let pan = self.stage.pan;
        let to = move |r: &Rect, b: &Bounds<Pixels>, x: f64, y: f64| -> Point<Pixels> {
            let (vw, vh) = (f64::from(f32::from(b.size.width)), f64::from(f32::from(b.size.height)));
            let left = vw / 2.0 - r.w * scale / 2.0 - pan.0 * scale;
            let top = vh / 2.0 - r.h * scale / 2.0 - pan.1 * scale;
            point(b.origin.x + px((left + (x - r.x) * scale) as f32), b.origin.y + px((top + (y - r.y) * scale) as f32))
        };
        let outline_rect = if layout { self.outline_of(self.sprite()) } else { None };
        let selected = self.selected_entry();
        let bounds: Vec<(Rect, bool)> = if layout && self.show_bounds {
            self.pickable().iter().filter(|e| e["depth"].as_u64() != self.depth).filter_map(|e| Some((Rect::from_json(&e["rect"])?, self.entry_edited(e)))).collect()
        } else {
            Vec::new()
        };
        let poly: Option<Vec<(f64, f64)>> = selected.as_ref().filter(|_| layout).and_then(|e| {
            let l = self.local_of(e)?;
            let m = self.matrix_of(e);
            Some(vec![m.apply(l.x, l.y), m.apply(l.x + l.w, l.y), m.apply(l.x + l.w, l.y + l.h), m.apply(l.x, l.y + l.h)])
        });
        let sel_rect = selected.as_ref().filter(|_| layout && poly.is_none()).and_then(|e| Rect::from_json(&e["rect"]));
        let transform = layout && self.tool == Tool::Transform && poly.is_some();
        let handle_pts: Vec<(f64, f64)> = if transform {
            selected.as_ref().and_then(|e| {
                let l = self.local_of(e)?;
                let m = self.matrix_of(e);
                Some([(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.5, 1.0), (0.0, 1.0), (0.0, 0.5)].iter().map(|(hx, hy)| m.apply(l.x + hx * l.w, l.y + hy * l.h)).collect())
            }).unwrap_or_default()
        } else {
            Vec::new()
        };
        let knob_line = if transform { rect.and_then(|r| self.knob(&r, scale)) } else { None };
        let (line, edited_c, accent, warn, bg) = (gpui::white().opacity(0.25), theme.warning.opacity(0.8), theme.primary, theme.warning.opacity(0.7), theme.background);
        let r0 = rect;
        let poly_paint = poly.clone();
        let stage = canvas(
            move |b, window, _| {
                viewport.set(b);
                for img in &stale {
                    let _ = window.drop_image(img.clone());
                }
                b
            },
            move |_, b, window, _| {
                // A checkerboard that follows the pan.
                let tile = 16.0f32;
                let (ox, oy) = (f32::from(b.origin.x), f32::from(b.origin.y));
                let shift = (((-pan.0 * scale) as f32).rem_euclid(tile * 2.0), ((-pan.1 * scale) as f32).rem_euclid(tile * 2.0));
                let cols = (f32::from(b.size.width) / tile) as i32 + 3;
                let rows = (f32::from(b.size.height) / tile) as i32 + 3;
                for j in -2..rows {
                    for i in -2..cols {
                        if (i + j).rem_euclid(2) == 0 {
                            continue;
                        }
                        let x = ox + i as f32 * tile + shift.0 - tile * 2.0;
                        let y = oy + j as f32 * tile + shift.1 - tile * 2.0;
                        let q = Bounds::new(point(px(x.max(ox)), px(y.max(oy))), size(px((x + tile).min(ox + f32::from(b.size.width)) - x.max(ox)), px((y + tile).min(oy + f32::from(b.size.height)) - y.max(oy))));
                        if f32::from(q.size.width) > 0.0 && f32::from(q.size.height) > 0.0 {
                            window.paint_quad(fill(q, gpui::white().opacity(0.04)));
                        }
                    }
                }
                let Some(r) = r0.or(pic.as_ref().map(|p| p.1)) else { return };
                let draw = |window: &mut Window, img: &Arc<RenderImage>, pr: &Rect| {
                    let a = to(&r, &b, pr.x, pr.y);
                    let ib = Bounds::new(a, size(px((pr.w * scale) as f32), px((pr.h * scale) as f32)));
                    window.with_content_mask(Some(gpui::ContentMask { bounds: b }), |window| {
                        let _ = window.paint_image(ib, ib, Corners::default(), img.clone(), 0, false);
                    });
                };
                if let Some((img, pr)) = &pic {
                    draw(window, img, pr);
                }
                if let Some((base, br, layer, lr)) = &floated {
                    draw(window, base, br);
                    draw(window, layer, lr);
                }
                window.with_content_mask(Some(gpui::ContentMask { bounds: b }), |window| {
                    if let Some(o) = outline_rect {
                        let a = to(&r, &b, o.x, o.y);
                        window.paint_quad(outline(Bounds::new(a, size(px((o.w * scale) as f32), px((o.h * scale) as f32))), warn, BorderStyle::Dashed));
                    }
                    for (er, edited) in &bounds {
                        let a = to(&r, &b, er.x, er.y);
                        window.paint_quad(outline(Bounds::new(a, size(px(((er.w * scale) as f32).max(1.0)), px(((er.h * scale) as f32).max(1.0)))), if *edited { edited_c } else { line }, BorderStyle::Solid));
                    }
                    if let Some(sr) = sel_rect {
                        let a = to(&r, &b, sr.x, sr.y);
                        let q = Bounds::new(a, size(px((sr.w * scale) as f32), px((sr.h * scale) as f32)));
                        window.paint_quad(fill(q, accent.opacity(0.1)));
                        window.paint_quad(outline(q, accent, BorderStyle::Solid));
                    }
                    if let Some(p) = &poly_paint {
                        let pts: Vec<Point<Pixels>> = p.iter().map(|(x, y)| to(&r, &b, *x, *y)).collect();
                        let mut fillp = PathBuilder::fill();
                        let mut stroke = PathBuilder::stroke(px(1.5));
                        for (k, q) in pts.iter().chain(pts.first()).enumerate() {
                            if k == 0 {
                                fillp.move_to(*q);
                                stroke.move_to(*q);
                            } else {
                                fillp.line_to(*q);
                                stroke.line_to(*q);
                            }
                        }
                        if let Ok(f) = fillp.build() {
                            window.paint_path(f, accent.opacity(0.1));
                        }
                        if let Ok(s) = stroke.build() {
                            window.paint_path(s, accent);
                        }
                    }
                    if let Some((top, at)) = knob_line {
                        let p = |(x, y): (f64, f64)| point(b.origin.x + px(x as f32), b.origin.y + px(y as f32));
                        let mut l = PathBuilder::stroke(px(1.0));
                        l.move_to(p(top));
                        l.line_to(p(at));
                        if let Ok(l) = l.build() {
                            window.paint_path(l, accent);
                        }
                        let c = p(at);
                        window.paint_quad(fill(Bounds::new(point(c.x - px(5.0), c.y - px(5.0)), size(px(10.0), px(10.0))), accent).corner_radii(px(5.0)));
                    }
                    for (x, y) in &handle_pts {
                        let c = to(&r, &b, *x, *y);
                        let q = Bounds::new(point(c.x - px(4.0), c.y - px(4.0)), size(px(8.0), px(8.0)));
                        window.paint_quad(fill(q, bg));
                        window.paint_quad(outline(q, accent, BorderStyle::Solid));
                    }
                });
            },
        )
        .size_full();
        let mut out = div()
            .id("movie-stage")
            .track_focus(&self.stage.focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme.muted)
            .when(self.stage.panning.is_some(), |d| d.cursor_grabbing())
            .when(self.stage.space.is_some() && self.stage.panning.is_none(), |d| d.cursor_grab())
            .child(stage)
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| this.stage_key(e, window, cx)))
            .on_key_up(cx.listener(|this, e: &gpui::KeyUpEvent, _, cx| this.stage_key_up(e, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &gpui::MouseDownEvent, window, cx| this.stage_down(e, window, cx)))
            .on_mouse_down(MouseButton::Middle, cx.listener(|this, e: &gpui::MouseDownEvent, window, cx| {
                this.stage.focus.focus(window, cx);
                this.stage.panning = Some((e.position, this.stage.pan));
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, e: &gpui::MouseUpEvent, _, cx| this.stage_up(e, cx)))
            .on_mouse_up(MouseButton::Middle, cx.listener(|this, _, _, _| this.stage.panning = None))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, e: &gpui::MouseUpEvent, _, cx| this.stage_up(e, cx)))
            .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| this.stage_move(e, cx)))
            .on_scroll(cx.listener(|this, e: &gpui::ScrollEvent, _, cx| this.stage_scroll(e, cx)))
            .on_hover(cx.listener(|this, hovered: &bool, _, _| {
                if !*hovered && (this.node == Node::Movie || this.play) {
                    swf_player::worker().pointer(Pointer::Leave);
                }
            }));
        // The selection's label: its name and depth (and the position while dragging).
        if let (Some(e), Some(p), Some(r)) = (&selected, &poly, rect) {
            let pts: Vec<(f64, f64)> = p.iter().map(|(x, y)| self.to_stage(&r, scale, *x, *y)).collect();
            let (lx, ly) = (pts.iter().map(|q| q.0).fold(f64::INFINITY, f64::min), pts.iter().map(|q| q.1).fold(f64::INFINITY, f64::min));
            let live = self.stage.live.map(|(_, m)| format!(" · {:.1}, {:.1}", m.tx, m.ty)).unwrap_or_default();
            let name = e["name"].as_str().map(str::to_string).unwrap_or_else(|| self.label(e["character"].as_u64().unwrap_or(0)));
            out = out.child(div().absolute().left(px(lx as f32)).top(px((ly - 15.0) as f32)).px_1().bg(theme.primary).text_color(theme.primary_foreground).text_xs().font_family("Menlo").whitespace_nowrap().child(format!("{name} · {}{live}", e["depth"])));
        }
        if let Some(o) = outline_rect.zip(rect) {
            let (x, y) = self.to_stage(&o.1, scale, o.0.x, o.0.y);
            out = out.child(div().absolute().left(px(x as f32)).top(px((y - 14.0) as f32)).px_1().bg(theme.warning.opacity(0.8)).text_color(theme.background).text_xs().child("stage"));
        }
        if !ready && self.stage.error.is_none() {
            out = out.child(div().absolute().inset_0().flex().items_center().justify_center().text_xs().text_color(theme.muted_foreground).child("Rendering…"));
        }
        if let Some(e) = &self.stage.error {
            out = out.child(div().absolute().top(px(12.0)).left(px(12.0)).right(px(12.0)).p_2().rounded_md().bg(theme.danger.opacity(0.9)).text_color(theme.background).text_xs().child(v_text("The preview could not be rendered", e)));
        }
        if self.stage.busy && ready {
            out = out.child(div().absolute().top(px(8.0)).right(px(8.0)).px_2().rounded_sm().bg(theme.background.opacity(0.9)).text_xs().child("updating view…"));
        }
        if !self.stage.missing.is_empty() {
            let n = self.stage.missing.len();
            out = out.child(div().absolute().bottom_0().left_0().right_0().px_2().bg(theme.warning.opacity(0.9)).text_color(theme.background).text_xs().font_family("Menlo").overflow_hidden().text_ellipsis().whitespace_nowrap().child(format!("{n} image{} not found: {}", if n == 1 { "" } else { "s" }, self.stage.missing[0])));
        }
        out.into_any_element()
    }
}

fn v_text(title: &str, detail: &str) -> impl IntoElement {
    gpui_component::v_flex().gap_1().child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title.to_string())).child(div().font_family("Menlo").child(detail.to_string()))
}

impl MovieEditor {
    /// Where the selection is on screen (window points, title bar excluded): its
    /// centre, the transform handles and the knob, for the test driver.
    pub(super) fn selection_on_screen(&self) -> J {
        // The view and scale the stage drew last.
        let key = self.target_key();
        let rect = self.stage.views.get(&key).or_else(|| self.stage.char_rect.get(&key)).copied();
        let scale = self.stage.scale;
        let (Some(r), Some(e)) = (rect, self.selected_entry()) else { return J::Null };
        let b = self.stage.viewport.get();
        let (ox, oy) = (f64::from(f32::from(b.origin.x)), f64::from(f32::from(b.origin.y)));
        let at = |p: (f64, f64)| json!([p.0 + ox, p.1 + oy]);
        let center = self.local_of(&e).map(|l| {
            let (x, y) = self.matrix_of(&e).apply(l.x + l.w / 2.0, l.y + l.h / 2.0);
            self.to_stage(&r, scale, x, y)
        });
        json!({
            "center": center.map(at),
            "handles": self.handles(&r, scale).into_iter().map(|(hx, hy, p)| json!({ "hx": hx, "hy": hy, "at": at(p) })).collect::<Vec<_>>(),
            "knob": self.knob(&r, scale).map(|(_, k)| at(k)),
            "scale": scale,
        })
    }
}
