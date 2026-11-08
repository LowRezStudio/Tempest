//! A level's Kismet as a node graph (`kismet.sequences`, `kismet.graph`).
//! The cooker drops the node positions but keeps every connector's on the
//! editor canvas (`DrawY` of inputs and outputs, `DrawX` of variable and
//! event connectors), so `layout` rebuilds them from those, then places the
//! rest next to what they link to; positions the user dragged win (kept in
//! the Studio, not the package). Drag the background to pan, the wheel to
//! zoom, a node to move it; drag from an output to an input, or from a
//! variable connector to a variable, to link them (`kismet.link`); Alt+click
//! a link (or select it and press Delete) to break it. Double-click a
//! sub-sequence to go into it, a Matinee to open its timeline. Right-click
//! or A adds a node (`kismet.palette` / `kismet.add`), Delete removes one.

use std::collections::HashMap;

use gpui::{canvas, div, point, prelude::*, px, AnyElement, Bounds, Context, Entity, MouseButton, PathBuilder, Pixels, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{ConfirmAction, Confirm, MapEditor, Mode};
use crate::studio::props_view::{PropEditor, PropEvent};
use crate::studio::Tone;

/// Title bar and connector spacing of a drawn node (points at zoom 1).
pub const TITLE: f32 = 22.0;
pub const ROW: f32 = 20.0;
const GAP: f32 = 80.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box2 {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

fn arr(v: &J) -> &[J] {
    v.as_array().map(|a| a.as_slice()).unwrap_or(&[])
}

fn f(v: &J) -> Option<f32> {
    v.as_f64().map(|x| x as f32)
}

pub fn node_size(n: &J) -> (f32, f32) {
    if n["kind"] == "variable" {
        return (64.0, 64.0);
    }
    // Hidden connectors are drawn too (dimmed): links may still use them.
    let rows = arr(&n["inputs"]).len().max(arr(&n["outputs"]).len()).max(1) as f32;
    let vars = (arr(&n["variables"]).len() + arr(&n["events"]).len()) as f32;
    let w = f(&n["width"]).unwrap_or(0.0).max(150.0).max(vars * 64.0);
    let h = TITLE + rows * ROW + if vars > 0.0 { 34.0 } else { 8.0 };
    (w, h)
}

fn min_of(xs: impl Iterator<Item = Option<f32>>) -> Option<f32> {
    xs.flatten().fold(None, |m: Option<f32>, x| Some(m.map_or(x, |m| m.min(x))))
}

/// Where to draw every node (the Electron `kismet-layout.ts`).
pub fn layout(nodes: &[J], moved: &HashMap<usize, (f32, f32)>) -> HashMap<usize, Box2> {
    struct P {
        x: Option<f32>,
        y: Option<f32>,
        w: f32,
        h: f32,
    }
    let id = |n: &J| n["export"].as_u64().unwrap_or(0) as usize;
    let mut partial: Vec<(usize, P)> = Vec::new();
    let mut at: HashMap<usize, usize> = HashMap::new();
    for n in nodes {
        if n["kind"] == "frame" {
            continue;
        }
        let (w, h) = node_size(n);
        let e = id(n);
        at.insert(e, partial.len());
        if let Some(&(x, y)) = moved.get(&e) {
            partial.push((e, P { x: Some(x), y: Some(y), w, h }));
            continue;
        }
        if let (Some(x), Some(y)) = (f(&n["posX"]), f(&n["posY"])) {
            if x != 0.0 || y != 0.0 {
                partial.push((e, P { x: Some(x), y: Some(y), w, h }));
                continue;
            }
        }
        let top = min_of(arr(&n["inputs"]).iter().chain(arr(&n["outputs"])).map(|c| f(&c["drawY"])));
        let vars_x: Vec<Option<f32>> = arr(&n["variables"]).iter().chain(arr(&n["events"])).map(|v| f(&v["drawX"])).collect();
        let left = min_of(vars_x.iter().copied());
        let count = vars_x.iter().filter(|x| x.is_some()).count().max(1) as f32;
        let width = f(&n["width"]).unwrap_or(w);
        partial.push((e, P { x: left.map(|l| l - width / (count + 1.0)), y: top.map(|t| t - TITLE - ROW / 2.0), w, h }));
    }
    // Links between ops: place unknown x (and y) next to a placed neighbour.
    for _ in 0..6 {
        let mut changed = false;
        for n in nodes {
            let Some(&mi) = at.get(&id(n)) else { continue };
            for o in arr(&n["outputs"]) {
                for l in arr(&o["links"]) {
                    let Some(&ti) = l["op"].as_u64().and_then(|op| at.get(&(op as usize))) else { continue };
                    let (me_x, me_y, me_w) = (partial[mi].1.x, partial[mi].1.y, partial[mi].1.w);
                    let (t_x, t_y) = (partial[ti].1.x, partial[ti].1.y);
                    match (me_x, t_x) {
                        (None, Some(t)) => {
                            partial[mi].1.x = Some(t - me_w - GAP);
                            changed = true;
                        }
                        (Some(m), None) => {
                            partial[ti].1.x = Some(m + me_w + GAP);
                            changed = true;
                        }
                        _ => {}
                    }
                    match (me_y, t_y) {
                        (None, Some(_)) => {
                            partial[mi].1.y = t_y;
                            changed = true;
                        }
                        (Some(_), None) => {
                            partial[ti].1.y = me_y;
                            changed = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    // Variables: under the first connector that names them.
    let kinds: HashMap<usize, bool> = nodes.iter().map(|n| (id(n), n["kind"] == "variable")).collect();
    for n in nodes {
        let Some(&mi) = at.get(&id(n)) else { continue };
        if n["kind"] == "variable" {
            continue;
        }
        let vars = arr(&n["variables"]);
        for (k, v) in vars.iter().enumerate() {
            let linked: Vec<Option<usize>> = arr(&v["linked"]).iter().map(|x| x.as_u64().map(|x| x as usize)).collect();
            for (j, lid) in linked.iter().enumerate() {
                let Some(lid) = lid else { continue };
                let Some(&ti) = at.get(lid) else { continue };
                if (partial[ti].1.x.is_some() && partial[ti].1.y.is_some()) || kinds.get(lid) != Some(&true) {
                    continue;
                }
                let (mx, my, mw, mh) = (partial[mi].1.x.unwrap_or(0.0), partial[mi].1.y.unwrap_or(0.0), partial[mi].1.w, partial[mi].1.h);
                let vx = f(&v["drawX"]).unwrap_or(mx + (k as f32 + 1.0) * mw / (vars.len() as f32 + 1.0));
                let tw = partial[ti].1.w;
                partial[ti].1.x = Some(vx - tw / 2.0 + j as f32 * (tw + 12.0));
                partial[ti].1.y = Some(my + mh + 40.0);
            }
        }
    }
    // Whatever is left: a row under everything placed.
    let mut bottom = 0f32;
    let mut right = 0f32;
    for (_, b) in &partial {
        if let Some(y) = b.y {
            bottom = bottom.max(y + b.h);
        }
        if let Some(x) = b.x {
            right = right.min(x);
        }
    }
    let mut cursor = right;
    let mut out = HashMap::new();
    for (e, b) in partial {
        let x = b.x.unwrap_or(cursor);
        let y = b.y.unwrap_or(bottom + 80.0);
        if b.x.is_none() || b.y.is_none() {
            cursor += b.w + 40.0;
        }
        out.insert(e, Box2 { x, y, w: b.w, h: b.h });
    }
    out
}

pub fn input_point(b: &Box2, k: usize) -> (f32, f32) {
    (b.x, b.y + TITLE + k as f32 * ROW + ROW / 2.0)
}
pub fn output_point(b: &Box2, k: usize) -> (f32, f32) {
    (b.x + b.w, b.y + TITLE + k as f32 * ROW + ROW / 2.0)
}
pub fn variable_point(b: &Box2, k: usize, count: usize) -> (f32, f32) {
    (b.x + (k as f32 + 1.0) * b.w / (count as f32 + 1.0), b.y + b.h)
}

/// A drawn link: what removing it sends, and its curve (canvas points).
#[derive(Clone)]
struct Link {
    id: String,
    exec: bool,
    remove: J,
    pts: Vec<(f32, f32)>,
}

fn bezier(a: (f32, f32), c1: (f32, f32), c2: (f32, f32), b: (f32, f32)) -> Vec<(f32, f32)> {
    (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let u = 1.0 - t;
            (u * u * u * a.0 + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * b.0, u * u * u * a.1 + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * b.1)
        })
        .collect()
}

#[derive(Clone)]
enum KDrag {
    Pan { last: (f32, f32) },
    Node { node: usize, start: (f32, f32), from: (f32, f32), moved: bool },
    Link { from: usize, variable: bool, index: usize, at: (f32, f32), to: (f32, f32) },
}

pub struct Kismet {
    pub level: Option<String>,
    pub sequences: Vec<J>,
    pub sequence: Option<usize>,
    pub graph: Option<J>,
    /// Node positions the user moved, by `level#sequence`.
    pub layout: HashMap<String, HashMap<usize, (f32, f32)>>,
    pub selected: Option<usize>,
    selected_link: Option<String>,
    pan: (f32, f32),
    zoom: f32,
    fitted_for: Option<String>,
    drag: Option<KDrag>,
    pub palette: Option<Vec<J>>,
    /// The palette: where it shows (view points) and where the node goes (canvas).
    adding: Option<(f32, f32, f32, f32)>,
    palette_filter: Option<Entity<InputState>>,
    pub node_props: Option<Entity<PropEditor>>,
    bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    focus: Option<gpui::FocusHandle>,
}

impl Default for Kismet {
    fn default() -> Kismet {
        Kismet { level: None, sequences: Vec::new(), sequence: None, graph: None, layout: HashMap::new(), selected: None, selected_link: None, pan: (40.0, 40.0), zoom: 1.0, fitted_for: None, drag: None, palette: None, adding: None, palette_filter: None, node_props: None, bounds: Default::default(), focus: None }
    }
}

impl Kismet {
    pub fn new(_window: &mut Window, _cx: &mut Context<MapEditor>) -> Kismet {
        Kismet::default()
    }

    pub fn reset(&mut self) {
        self.sequences.clear();
        self.sequence = None;
        self.graph = None;
        self.selected = None;
        self.selected_link = None;
        self.palette = None;
        self.adding = None;
        self.fitted_for = None;
    }

    fn key(&self) -> String {
        format!("{}#{}", self.level.as_deref().unwrap_or(""), self.sequence.map(|s| s.to_string()).unwrap_or_default())
    }

    pub fn nodes(&self) -> &[J] {
        self.graph.as_ref().map(|g| arr(&g["nodes"])).unwrap_or(&[])
    }

    pub fn node(&self, e: usize) -> Option<&J> {
        self.nodes().iter().find(|n| n["export"].as_u64() == Some(e as u64))
    }

    fn boxes(&self) -> HashMap<usize, Box2> {
        layout(self.nodes(), self.layout.get(&self.key()).unwrap_or(&HashMap::new()))
    }

    fn links(&self, boxes: &HashMap<usize, Box2>) -> Vec<Link> {
        let mut out = Vec::new();
        for n in self.nodes() {
            let e = n["export"].as_u64().unwrap_or(0) as usize;
            let Some(b) = boxes.get(&e) else { continue };
            for (k, o) in arr(&n["outputs"]).iter().enumerate() {
                let a = output_point(b, k);
                for l in arr(&o["links"]) {
                    let Some(op) = l["op"].as_u64().map(|x| x as usize) else { continue };
                    let Some(tb) = boxes.get(&op) else { continue };
                    let input = l["input"].as_u64().unwrap_or(0) as usize;
                    let t = input_point(tb, input);
                    let dx = ((t.0 - a.0).abs() / 2.0).max(40.0);
                    out.push(Link { id: format!("o{e}:{k}>{op}:{input}"), exec: true, remove: json!({ "from": e, "output": k, "to": op, "input": input, "remove": true }), pts: bezier(a, (a.0 + dx, a.1), (t.0 - dx, t.1), t) });
                }
            }
            let count = arr(&n["variables"]).len() + arr(&n["events"]).len();
            for (k, v) in arr(&n["variables"]).iter().enumerate() {
                let a = variable_point(b, k, count);
                for id in arr(&v["linked"]) {
                    let Some(id) = id.as_u64().map(|x| x as usize) else { continue };
                    let Some(tb) = boxes.get(&id) else { continue };
                    let t = (tb.x + tb.w / 2.0, tb.y);
                    out.push(Link { id: format!("v{e}:{k}>{id}"), exec: false, remove: json!({ "from": e, "variable": k, "to": id, "remove": true }), pts: bezier(a, (a.0, a.1 + 40.0), (t.0, t.1 - 40.0), t) });
                }
            }
        }
        out
    }
}

fn title_of(n: &J) -> String {
    if let Some(t) = n["title"].as_str().filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    let c = n["class"].as_str().unwrap_or("");
    let lower = c.to_ascii_lowercase();
    let mut s = c;
    for p in ["tgseqact_", "tgseqevent_", "tgseqcond_", "tgseqvar_", "seqact_", "seqevent_", "seqcond_", "seqvar_"] {
        if lower.starts_with(p) {
            s = &c[p.len()..];
            break;
        }
    }
    s.replace('_', " ")
}

impl MapEditor {
    pub(super) fn open_kismet(&mut self, level: String, sequence: Option<usize>, record: bool, cx: &mut Context<Self>) {
        self.mode = Mode::Kismet;
        let same = self.kismet.level.as_deref() == Some(level.as_str()) && !self.kismet.sequences.is_empty();
        let go = move |this: &mut MapEditor, cx: &mut Context<MapEditor>| {
            let seq = sequence.or_else(|| this.kismet.sequences.first().and_then(|s| s["export"].as_u64()).map(|s| s as usize));
            this.kismet.sequence = seq;
            this.kismet.selected = None;
            this.kismet.selected_link = None;
            this.load_graph(cx);
            if record {
                this.record(cx);
            }
            cx.notify();
        };
        if same {
            go(self, cx);
            return;
        }
        let call = self.call("kismet.sequences", json!({ "path": level }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.kismet.sequences = arr(&r.json["sequences"]).to_vec();
                    this.kismet.level = Some(level.clone());
                    this.kismet.palette = None;
                    go(this, cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not read the Kismet sequences: {e}"), cx),
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn load_graph(&mut self, cx: &mut Context<Self>) {
        let (Some(level), Some(seq)) = (self.kismet.level.clone(), self.kismet.sequence) else {
            self.kismet.graph = None;
            return;
        };
        let call = self.call("kismet.graph", json!({ "path": level, "index": seq }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => this.kismet.graph = Some(r.json),
                    Err(e) => this.toast(Tone::Error, format!("Could not read the sequence: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn kismet_call(&mut self, method: &'static str, mut params: J, fail: &'static str, cx: &mut Context<Self>) {
        let (Some(level), Some(seq)) = (self.kismet.level.clone(), self.kismet.sequence) else { return };
        params["path"] = json!(level);
        params["sequence"] = json!(seq);
        let call = self.call(method, params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        let changed = method != "kismet.link" || r.json["changed"].as_bool() == Some(true);
                        if let Some(added) = r.json["added"].as_u64() {
                            this.kismet.selected = Some(added as usize);
                        }
                        if method == "kismet.remove" {
                            this.kismet.selected = None;
                        }
                        this.kismet.graph = Some(r.json);
                        if changed {
                            this.note_edit(&level, cx);
                        }
                    }
                    Err(e) => this.toast(Tone::Error, format!("{fail}: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn link(&mut self, p: J, cx: &mut Context<Self>) {
        let fail = if p["remove"] == true { "Could not unlink" } else { "Could not link" };
        self.kismet_call("kismet.link", p, fail, cx);
    }

    pub(crate) fn load_kismet_palette(&mut self, cx: &mut Context<Self>) {
        if self.kismet.palette.is_some() {
            return;
        }
        let call = self.call("kismet.palette", json!({ "path": self.kismet.level }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => this.kismet.palette = Some(arr(&r.json["classes"]).to_vec()),
                    Err(e) => this.toast(Tone::Error, format!("Could not list the Kismet nodes: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Add a node (a copy of `example`) at canvas point (x, y). Events fire from, and object
    /// variables hold, the actor selected in the 3D view when it is in this level.
    /// `holders`: the event's originator and the object variable's value, when given.
    pub(crate) fn add_node(&mut self, example: J, kind: &str, (x, y): (f32, f32), (originator, object): (Option<usize>, Option<usize>), cx: &mut Context<Self>) {
        let target = self.selected.as_ref().filter(|s| Some(&s.0) == self.kismet.level.as_ref()).and_then(|_| self.actor()).map(|a| a.owner());
        let (originator, object) = (originator.or(if kind == "event" { target } else { None }), object.or(if kind == "variable" { target } else { None }));
        let (xr, yr) = (x.round(), y.round());
        self.kismet_call("kismet.add", json!({ "example": { "path": example["path"], "index": example["index"] }, "x": xr, "y": yr, "originator": originator, "object": object }), "Could not add the node", cx);
        // Where it was put: kept once the reply names it (`selected`).
        self.pending_node_at = Some((xr, yr));
    }

    pub(crate) fn remove_node(&mut self, node: usize, cx: &mut Context<Self>) {
        self.kismet_call("kismet.remove", json!({ "node": node }), "Could not delete the node", cx);
    }

    fn ask_remove_node(&mut self, cx: &mut Context<Self>) {
        let Some(n) = self.kismet.selected.and_then(|e| self.kismet.node(e)) else { return };
        let e = n["export"].as_u64().unwrap_or(0) as usize;
        self.confirm = Some(Confirm { title: format!("Delete {}?", title_of(n)), message: "The node and every link to it are removed from the sequence (⌘Z brings it back).".into(), action: ConfirmAction::RemoveNode(e) });
        cx.notify();
    }

    // ----- input ------------------------------------------------------------------------

    fn kismet_local(&self, p: gpui::Point<Pixels>) -> (f32, f32) {
        let b = self.kismet.bounds.get();
        (f32::from(p.x - b.origin.x), f32::from(p.y - b.origin.y))
    }

    fn to_canvas(&self, l: (f32, f32)) -> (f32, f32) {
        ((l.0 - self.kismet.pan.0) / self.kismet.zoom, (l.1 - self.kismet.pan.1) / self.kismet.zoom)
    }

    fn fit_graph(&mut self) {
        let boxes = self.kismet.boxes();
        if boxes.is_empty() {
            return;
        }
        let b = self.kismet.bounds.get();
        let (w, h) = (f32::from(b.size.width).max(100.0), f32::from(b.size.height).max(100.0));
        let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for b in boxes.values() {
            x0 = x0.min(b.x);
            y0 = y0.min(b.y);
            x1 = x1.max(b.x + b.w);
            y1 = y1.max(b.y + b.h);
        }
        let z = (w / (x1 - x0 + 120.0)).min(h / (y1 - y0 + 120.0)).clamp(0.15, 1.2);
        self.kismet.zoom = z;
        self.kismet.pan = ((w - (x1 - x0) * z) / 2.0 - x0 * z, (h - (y1 - y0) * z) / 2.0 - y0 * z);
    }

    /// What is under a canvas point: an output or variable connector, a node, or a link.
    fn kismet_hit(&self, c: (f32, f32), boxes: &HashMap<usize, Box2>) -> Option<(&'static str, usize, usize)> {
        let near = |a: (f32, f32)| ((a.0 - c.0).powi(2) + (a.1 - c.1).powi(2)).sqrt() < 8.0 / self.kismet.zoom.min(1.0);
        for n in self.kismet.nodes() {
            let e = n["export"].as_u64().unwrap_or(0) as usize;
            let Some(b) = boxes.get(&e) else { continue };
            for k in 0..arr(&n["outputs"]).len() {
                if near(output_point(b, k)) {
                    return Some(("output", e, k));
                }
            }
            let count = arr(&n["variables"]).len() + arr(&n["events"]).len();
            for k in 0..arr(&n["variables"]).len() {
                if near(variable_point(b, k, count)) {
                    return Some(("variable", e, k));
                }
            }
        }
        // The topmost node (the last drawn) under the point.
        for n in self.kismet.nodes().iter().rev() {
            let e = n["export"].as_u64().unwrap_or(0) as usize;
            let Some(b) = boxes.get(&e) else { continue };
            if c.0 >= b.x && c.0 <= b.x + b.w && c.1 >= b.y && c.1 <= b.y + b.h {
                return Some(("node", e, 0));
            }
        }
        None
    }

    fn kismet_down(&mut self, e: &gpui::MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = &self.kismet.focus {
            f.focus(window, cx);
        }
        let l = self.kismet_local(e.position);
        let c = self.to_canvas(l);
        if e.button == MouseButton::Right {
            if self.kismet_hit(c, &self.kismet.boxes()).is_none_or(|h| h.0 != "node") {
                self.open_node_palette(Some(l), window, cx);
            }
            return;
        }
        self.kismet.adding = None;
        let boxes = self.kismet.boxes();
        match self.kismet_hit(c, &boxes) {
            Some(("output", n, k)) => {
                let a = output_point(&boxes[&n], k);
                self.kismet.drag = Some(KDrag::Link { from: n, variable: false, index: k, at: a, to: c });
            }
            Some(("variable", n, k)) => {
                let count = self.kismet.node(n).map(|x| arr(&x["variables"]).len() + arr(&x["events"]).len()).unwrap_or(1);
                let a = variable_point(&boxes[&n], k, count);
                self.kismet.drag = Some(KDrag::Link { from: n, variable: true, index: k, at: a, to: c });
            }
            Some(("node", n, _)) => {
                if e.click_count == 2 {
                    self.kismet.drag = None;
                    let node = self.kismet.node(n).cloned().unwrap_or_default();
                    let level = self.kismet.level.clone().unwrap_or_default();
                    if node["kind"] == "sequence" {
                        self.open_kismet(level, Some(n), true, cx);
                    } else if node["class"].as_str().is_some_and(|c| c.eq_ignore_ascii_case("seqact_interp")) {
                        self.open_matinee(level, n, true, cx);
                    }
                    return;
                }
                self.select_node(Some(n), cx);
                let b = boxes[&n];
                self.kismet.drag = Some(KDrag::Node { node: n, start: c, from: (b.x, b.y), moved: false });
            }
            _ => {
                // A link under the pointer (within a few points)?
                let hit = self.kismet.links(&boxes).into_iter().find(|lk| lk.pts.windows(2).any(|s| {
                    let (a, b) = (glam::Vec2::new(s[0].0, s[0].1), glam::Vec2::new(s[1].0, s[1].1));
                    let p = glam::Vec2::new(c.0, c.1);
                    let ab = b - a;
                    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                    (a + ab * t - p).length() < 5.0 / self.kismet.zoom
                }));
                if let Some(lk) = hit {
                    if e.modifiers.alt {
                        self.link(lk.remove, cx);
                    } else {
                        self.kismet.selected_link = Some(lk.id);
                    }
                    cx.notify();
                    return;
                }
                self.select_node(None, cx);
                self.kismet.selected_link = None;
                self.kismet.drag = Some(KDrag::Pan { last: l });
            }
        }
        cx.notify();
    }

    fn kismet_move(&mut self, e: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(d) = self.kismet.drag.clone() else { return };
        if e.pressed_button.is_none() {
            self.kismet.drag = None;
            return;
        }
        let l = self.kismet_local(e.position);
        let c = self.to_canvas(l);
        match d {
            KDrag::Pan { last } => {
                self.kismet.pan.0 += l.0 - last.0;
                self.kismet.pan.1 += l.1 - last.1;
                self.kismet.drag = Some(KDrag::Pan { last: l });
            }
            KDrag::Node { node, start, from, moved } => {
                let moved = moved || ((c.0 - start.0).powi(2) + (c.1 - start.1).powi(2)).sqrt() >= 3.0;
                if moved {
                    let key = self.kismet.key();
                    self.kismet.layout.entry(key).or_default().insert(node, ((from.0 + c.0 - start.0).round(), (from.1 + c.1 - start.1).round()));
                }
                self.kismet.drag = Some(KDrag::Node { node, start, from, moved });
            }
            KDrag::Link { from, variable, index, at, .. } => self.kismet.drag = Some(KDrag::Link { from, variable, index, at, to: c }),
        }
        cx.notify();
    }

    fn kismet_up(&mut self, _e: &gpui::MouseUpEvent, cx: &mut Context<Self>) {
        let Some(KDrag::Link { from, variable, index, to, .. }) = self.kismet.drag.take() else { return };
        let boxes = self.kismet.boxes();
        // The node under the pointer, and for an output the input row it is on.
        let Some(("node", n, _)) = self.kismet_hit(to, &boxes) else { return };
        let b = boxes[&n];
        let target = self.kismet.node(n).cloned().unwrap_or_default();
        if variable {
            if target["kind"] == "variable" {
                self.link(json!({ "from": from, "variable": index, "to": n }), cx);
            }
        } else {
            let inputs = arr(&target["inputs"]).len();
            let row = ((to.1 - b.y - TITLE) / ROW).floor();
            if inputs > 0 && row >= 0.0 && (row as usize) < inputs && to.0 < b.x + b.w * 0.6 {
                self.link(json!({ "from": from, "output": index, "to": n, "input": row as usize }), cx);
            }
        }
        cx.notify();
    }

    fn kismet_scroll(&mut self, e: &gpui::ScrollEvent, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let l = self.kismet_local(e.position);
        let dy = f32::from(e.delta.pixel_delta(px(16.0)).y);
        let z = self.kismet.zoom;
        let next = (z * (dy * 0.0015).exp()).clamp(0.1, 3.0);
        self.kismet.pan = (l.0 - (l.0 - self.kismet.pan.0) * next / z, l.1 - (l.1 - self.kismet.pan.1) * next / z);
        self.kismet.zoom = next;
        cx.notify();
    }

    fn kismet_key(&mut self, e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = e.keystroke.key.as_str();
        if e.keystroke.modifiers.platform || e.keystroke.modifiers.control {
            return;
        }
        match k {
            "escape" if self.kismet.adding.is_some() => self.kismet.adding = None,
            "delete" | "backspace" if self.kismet.selected_link.is_some() => {
                let boxes = self.kismet.boxes();
                if let Some(lk) = self.kismet.links(&boxes).into_iter().find(|l| Some(&l.id) == self.kismet.selected_link.as_ref()) {
                    self.link(lk.remove, cx);
                }
                self.kismet.selected_link = None;
            }
            "delete" | "backspace" if self.kismet.selected.is_some() => self.ask_remove_node(cx),
            "a" => self.open_node_palette(None, window, cx),
            "f" => self.fit_graph(),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn select_node(&mut self, n: Option<usize>, cx: &mut Context<Self>) {
        self.kismet.selected = n;
        if let (Some(n), Some(level), Some(p)) = (n, self.kismet.level.clone(), self.kismet.node_props.clone()) {
            p.update(cx, |p, cx| p.load(&level, n, cx));
        }
        cx.notify();
    }

    /// Open the palette at a view point (or the middle of the view).
    pub(crate) fn open_node_palette(&mut self, at: Option<(f32, f32)>, window: &mut Window, cx: &mut Context<Self>) {
        let b = self.kismet.bounds.get();
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        let l = at.unwrap_or((w / 2.0, h / 3.0));
        let c = self.to_canvas(l);
        self.kismet.adding = Some((l.0.min(w - 330.0).max(0.0), l.1.min(h - 340.0).max(0.0), c.0, c.1));
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Add a node: type to filter (delay, toggle, touch…)"));
        input.update(cx, |s, cx| s.focus(window, cx));
        let sub = cx.subscribe(&input, |this, input, e: &InputEvent, cx| match e {
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter { .. } => {
                let q = input.read(cx).value().to_string();
                if let Some(c) = this.shown_palette(&q).first().cloned() {
                    this.pick_node(c, cx);
                }
            }
            _ => {}
        });
        self.palette_sub = Some(sub);
        self.kismet.palette_filter = Some(input);
        self.load_kismet_palette(cx);
        cx.notify();
    }

    fn shown_palette(&self, q: &str) -> Vec<J> {
        const KINDS: [&str; 6] = ["event", "action", "condition", "variable", "frame", "other"];
        let q = q.trim().to_ascii_lowercase();
        let mut v: Vec<J> = self.kismet.palette.iter().flatten().filter(|c| c["kind"] != "sequence" && (q.is_empty() || c["class"].as_str().is_some_and(|s| s.contains(&q)))).cloned().collect();
        let rank = |c: &J| KINDS.iter().position(|k| c["kind"] == *k).unwrap_or(9);
        v.sort_by(|a, b| rank(a).cmp(&rank(b)).then(b["count"].as_u64().cmp(&a["count"].as_u64())));
        v
    }

    fn pick_node(&mut self, c: J, cx: &mut Context<Self>) {
        let Some((_, _, x, y)) = self.kismet.adding.take() else { return };
        let kind = c["kind"].as_str().unwrap_or("").to_string();
        self.add_node(c["example"].clone(), &kind, (x, y), (None, None), cx);
    }

    // ----- drawing ----------------------------------------------------------------------

    pub(super) fn render_kismet_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let me = cx.entity().downgrade();
        let levels: Vec<(String, String)> = self.levels.iter().map(|l| (l.path.clone(), l.name.clone())).collect();
        let current = self.kismet.level.as_ref().and_then(|p| self.level(p)).map(|l| l.name.clone()).unwrap_or_default();
        let level_menu = Button::new("kis-level").xsmall().ml_2().label(current).dropdown_menu(move |menu, _, _| {
            let mut menu = menu;
            for (p, n) in &levels {
                let (me, p) = (me.clone(), p.clone());
                menu = menu.item(PopupMenuItem::new(n.clone()).on_click(move |_, _, cx| {
                    let _ = me.update(cx, |this, cx| this.open_kismet(p.clone(), None, true, cx));
                }));
            }
            menu
        });
        // The breadcrumb of nested sequences.
        let mut trail = Vec::new();
        let mut cur = self.kismet.sequence.and_then(|s| self.kismet.sequences.iter().find(|x| x["export"].as_u64() == Some(s as u64)));
        while let Some(s) = cur {
            trail.insert(0, (s["export"].as_u64().unwrap_or(0) as usize, s["title"].as_str().filter(|t| !t.is_empty()).or(s["name"].as_str()).unwrap_or("").to_string()));
            cur = s["parent"].as_u64().and_then(|p| self.kismet.sequences.iter().find(|x| x["export"].as_u64() == Some(p)));
        }
        let mut crumbs = h_flex().ml_2().gap_1().min_w_0();
        for (k, (e, name)) in trail.into_iter().enumerate() {
            if k > 0 {
                crumbs = crumbs.child(div().text_color(theme.muted_foreground).child("›"));
            }
            let level = self.kismet.level.clone().unwrap_or_default();
            crumbs = crumbs.child(div().id(("kis-crumb", k)).font_family("Menlo").cursor_pointer().hover(|s| s.underline()).child(name).on_click(cx.listener(move |this, _, _, cx| this.open_kismet(level.clone(), Some(e), true, cx))));
        }
        h_flex()
            .gap_1()
            .flex_1()
            .child(level_menu)
            .child(crumbs)
            .child(div().flex_1())
            .child(Button::new("kis-add").xsmall().ghost().icon(Icon::new(IconName::Plus)).label("Node").tooltip("Add a node (A, or right-click the graph)").on_click(cx.listener(|this, _, window, cx| this.open_node_palette(None, window, cx))))
            .child(div().text_color(theme.muted_foreground).child(format!("{} nodes", self.kismet.nodes().len())))
            .into_any_element()
    }

    pub(super) fn render_kismet(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if self.kismet.focus.is_none() {
            self.kismet.focus = Some(cx.focus_handle());
        }
        // A node just added goes where it was asked for (the reply names it).
        if let (Some((x, y)), Some(n)) = (self.pending_node_at, self.kismet.selected) {
            if self.kismet.node(n).is_some() {
                let key = self.kismet.key();
                self.kismet.layout.entry(key).or_default().insert(n, (x, y));
                self.pending_node_at = None;
            }
        }
        let key = self.kismet.key();
        if !self.kismet.nodes().is_empty() && self.kismet.fitted_for.as_ref() != Some(&key) && f32::from(self.kismet.bounds.get().size.width) > 10.0 {
            self.kismet.fitted_for = Some(key);
            self.fit_graph();
        }
        let boxes = self.kismet.boxes();
        let links = self.kismet.links(&boxes);
        let (pan, z) = (self.kismet.pan, self.kismet.zoom);
        let sc = move |p: (f32, f32)| (pan.0 + p.0 * z, pan.1 + p.1 * z);
        let pending = match &self.kismet.drag {
            Some(KDrag::Link { at, to, .. }) => Some((*at, *to)),
            _ => None,
        };
        let selected_link = self.kismet.selected_link.clone();
        let (ln_exec, ln_var, ln_sel, ln_new, dot) = (theme.foreground.opacity(0.7), theme.danger.opacity(0.6), theme.warning, theme.primary, theme.muted_foreground.opacity(0.25));
        let bounds = self.kismet.bounds.clone();
        let painted = canvas(
            move |b, _, _| {
                bounds.set(b);
                b
            },
            move |b, _, window, _| {
                // The dot grid, following the pan.
                let step = 24.0 * z;
                if step > 6.0 {
                    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
                    let mut x = pan.0.rem_euclid(step);
                    while x < w {
                        let mut y = pan.1.rem_euclid(step);
                        while y < h {
                            window.paint_quad(gpui::fill(Bounds::new(point(b.origin.x + px(x), b.origin.y + px(y)), gpui::size(px(1.5), px(1.5))), dot));
                            y += step;
                        }
                        x += step;
                    }
                }
                let o = b.origin;
                let stroke = |pts: &[(f32, f32)], width: f32, color: gpui::Hsla, window: &mut Window| {
                    let mut p = PathBuilder::stroke(px(width));
                    for (k, q) in pts.iter().enumerate() {
                        let s = sc(*q);
                        let at = point(o.x + px(s.0), o.y + px(s.1));
                        if k == 0 { p.move_to(at) } else { p.line_to(at) }
                    }
                    if let Ok(p) = p.build() {
                        window.paint_path(p, color);
                    }
                };
                for l in &links {
                    let sel = selected_link.as_ref() == Some(&l.id);
                    stroke(&l.pts, if sel { 3.0 } else { 1.6 }, if sel { ln_sel } else if l.exec { ln_exec } else { ln_var }, window);
                }
                if let Some((a, t)) = pending {
                    stroke(&[a, t], 2.0, ln_new, window);
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let fs = 11.0 * z;
        let mut nodes: Vec<AnyElement> = Vec::new();
        for n in self.kismet.nodes() {
            let e = n["export"].as_u64().unwrap_or(0) as usize;
            let Some(b) = boxes.get(&e) else { continue };
            let (x, y) = sc((b.x, b.y));
            let sel = self.kismet.selected == Some(e);
            if n["kind"] == "variable" {
                nodes.push(
                    v_flex()
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .w(px(b.w * z))
                        .h(px(b.h * z))
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .border_2()
                        .border_color(if sel { theme.warning } else { theme.danger.opacity(0.6) })
                        .bg(theme.background)
                        .text_size(px(fs * 0.9))
                        .child(crate::driver::probe(format!("kis-node-{e}")))
                        .child(div().px_1().w_full().text_center().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family("Menlo").child(n["value"].as_str().unwrap_or("?").to_string()))
                        .child(div().opacity(0.5).child(n["class"].as_str().unwrap_or("").trim_start_matches("SeqVar_").trim_start_matches("seqvar_").to_string()))
                        .into_any_element(),
                );
                continue;
            }
            let (border, bg) = match n["kind"].as_str().unwrap_or("") {
                "event" => (theme.danger.opacity(0.7), theme.danger.opacity(0.1)),
                "action" => (theme.info.opacity(0.6), theme.secondary),
                "condition" => (theme.warning.opacity(0.7), theme.warning.opacity(0.1)),
                "sequence" => (theme.primary.opacity(0.7), theme.primary.opacity(0.1)),
                _ => (theme.foreground.opacity(0.3), theme.secondary),
            };
            let mut node = div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(b.w * z))
                .h(px(b.h * z))
                .rounded_md()
                .border_2()
                .border_color(if sel { theme.warning } else { border })
                .bg(bg)
                .text_size(px(fs))
                .child(crate::driver::probe(format!("kis-node-{e}")))
                .child(
                    h_flex()
                        .h(px(TITLE * z))
                        .px(px(6.0 * z))
                        .gap_1()
                        .bg(theme.foreground.opacity(0.08))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .overflow_hidden()
                        .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(title_of(n)))
                        .when(n["kind"] == "sequence", |d| d.child(div().opacity(0.5).child(format!("{} ›", n["subObjects"])))),
                );
            for (k, c) in arr(&n["inputs"]).iter().enumerate() {
                let cy = (TITLE + k as f32 * ROW + ROW / 2.0) * z;
                node = node.child(h_flex().absolute().left(px(-5.0 * z)).top(px(cy - 7.0 * z)).gap_1().when(c["hidden"] == true, |d| d.opacity(0.4)).child(div().size(px(10.0 * z)).rounded_sm().bg(theme.foreground.opacity(0.7))).child(div().opacity(0.8).child(c["desc"].as_str().unwrap_or("").to_string())));
            }
            for (k, c) in arr(&n["outputs"]).iter().enumerate() {
                let cy = (TITLE + k as f32 * ROW + ROW / 2.0) * z;
                node = node.child(h_flex().absolute().right(px(-5.0 * z)).top(px(cy - 7.0 * z)).gap_1().when(c["hidden"] == true, |d| d.opacity(0.4)).child(div().opacity(0.8).child(c["desc"].as_str().unwrap_or("").to_string())).child(div().size(px(10.0 * z)).rounded_sm().bg(theme.foreground.opacity(0.7))));
            }
            let count = arr(&n["variables"]).len() + arr(&n["events"]).len();
            for (k, v) in arr(&n["variables"]).iter().enumerate() {
                let (vx, _) = variable_point(b, k, count);
                node = node.child(
                    v_flex()
                        .absolute()
                        .left(px((vx - b.x) * z - 32.0 * z))
                        .w(px(64.0 * z))
                        .bottom(px(-5.0 * z))
                        .items_center()
                        .when(v["hidden"] == true, |d| d.opacity(0.4))
                        .child(div().max_w(px(64.0 * z)).overflow_hidden().text_ellipsis().whitespace_nowrap().text_size(px(fs * 0.9)).opacity(0.8).child(v["desc"].as_str().unwrap_or("").to_string()))
                        .child(div().size(px(10.0 * z)).rounded_full().bg(if v["writeable"] == true { theme.warning } else { theme.danger.opacity(0.8) })),
                );
            }
            nodes.push(node.into_any_element());
        }
        let frames: Vec<String> = self.kismet.nodes().iter().filter(|n| n["kind"] == "frame").map(|n| n["comment"].as_str().filter(|c| !c.is_empty()).or(n["name"].as_str()).unwrap_or("").to_string()).collect();
        let palette = self.kismet.adding.map(|(left, top, _, _)| {
            let q = self.kismet.palette_filter.as_ref().map(|f| f.read(cx).value().to_string()).unwrap_or_default();
            let shown = self.shown_palette(&q);
            let who = self.selected.as_ref().filter(|s| Some(&s.0) == self.kismet.level.as_ref()).and_then(|_| self.actor()).map(|a| a.name.clone());
            let rows: Vec<AnyElement> = shown
                .iter()
                .take(200)
                .enumerate()
                .map(|(k, c)| {
                    let color = match c["kind"].as_str().unwrap_or("") {
                        "event" => theme.danger,
                        "condition" => theme.warning,
                        "variable" => theme.danger.opacity(0.5),
                        _ => theme.info,
                    };
                    let item = c.clone();
                    h_flex()
                        .id(("kis-pal", k))
                        .px_2()
                        .gap_2()
                        .text_xs()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.list_hover))
                        .child(div().size(px(8.0)).rounded_full().bg(color))
                        .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family("Menlo").child(c["class"].as_str().unwrap_or("").to_string()))
                        .child(div().text_color(theme.muted_foreground).child(c["count"].to_string()))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, _, cx| this.pick_node(item.clone(), cx)))
                        .into_any_element()
                })
                .collect();
            v_flex()
                .id("kis-palette")
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(320.0))
                .max_h(px(330.0))
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .shadow_lg()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_scroll(|_, _, cx| cx.stop_propagation())
                .children(self.kismet.palette_filter.as_ref().map(|f| div().p_1p5().child(Input::new(f).xsmall())))
                .children(who.map(|w| div().px_2().pb_1().text_size(px(10.0)).text_color(theme.muted_foreground).child(format!("Events fire from, and object variables hold, {w} (selected in the scene)."))))
                .child(div().id("kis-pal-list").flex_1().min_h_0().overflow_y_scroll().pb_1().child(if self.kismet.palette.is_none() { div().p_2().text_xs().text_color(theme.muted_foreground).child("Listing the nodes the game uses…").into_any_element() } else { v_flex().children(rows).into_any_element() }))
                .into_any_element()
        });
        let focus = self.kismet.focus.clone().expect("made above");
        let empty = self.kismet.nodes().is_empty().then(|| div().absolute().inset_0().flex().items_center().justify_center().text_sm().text_color(theme.muted_foreground).child(if self.kismet.graph.is_some() { "This sequence is empty." } else { "No sequence." }));
        let _ = window;
        v_flex()
            .flex_1()
            .min_h_0()
            .when(!frames.is_empty(), |d| d.child(h_flex().flex_wrap().gap_1().px_2().py_1().border_b_1().border_color(theme.border).bg(theme.secondary).text_xs().child(div().text_color(theme.muted_foreground).child("Comments:")).children(frames.into_iter().map(|f| div().px_1().rounded_sm().bg(theme.background).child(f)))))
            .child(
                div()
                    .id("kismet-graph")
                    .track_focus(&focus)
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .bg(theme.secondary.opacity(0.4))
                    .child(crate::driver::probe("kismet-graph"))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, e, window, cx| this.kismet_down(e, window, cx)))
                    .on_mouse_down(MouseButton::Right, cx.listener(|this, e, window, cx| this.kismet_down(e, window, cx)))
                    .on_mouse_down(MouseButton::Middle, cx.listener(|this, e, window, cx| this.kismet_down(e, window, cx)))
                    .on_mouse_move(cx.listener(|this, e, _, cx| this.kismet_move(e, cx)))
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, e, _, cx| this.kismet_up(e, cx)))
                    .on_mouse_up(MouseButton::Middle, cx.listener(|this, e, _, cx| this.kismet_up(e, cx)))
                    .on_scroll(cx.listener(|this, e, _, cx| this.kismet_scroll(e, cx)))
                    .on_key_down(cx.listener(|this, e, window, cx| this.kismet_key(e, window, cx)))
                    .child(painted)
                    .children(nodes)
                    .child(div().absolute().right(px(8.0)).bottom(px(8.0)).text_size(px(10.0)).font_family("Menlo").text_right().text_color(theme.muted_foreground).child("drag background pan · wheel zoom · drag a node to move it · drag a connector to link").child("Alt+click or Delete a link to break it · double-click a sequence to enter · F fit").child("right-click or A to add a node · Delete removes the selected node"))
                    .children(palette)
                    .children(empty),
            )
            .into_any_element()
    }

    pub(super) fn render_kismet_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if self.kismet.node_props.is_none() {
            let p = cx.new(|cx| PropEditor::new(self.backend.clone(), window, cx));
            let sub = cx.subscribe(&p, |this, _, e: &PropEvent, cx| match e {
                PropEvent::Changed => {
                    if let Some(l) = this.kismet.level.clone() {
                        this.note_edit(&l, cx);
                    }
                    this.load_graph(cx);
                }
                PropEvent::Error(e) => this.toast(Tone::Error, e.clone(), cx),
            });
            self.node_props_sub = Some(sub);
            if let (Some(n), Some(level)) = (self.kismet.selected, self.kismet.level.clone()) {
                p.update(cx, |p, cx| p.load(&level, n, cx));
            }
            self.kismet.node_props = Some(p);
        }
        let Some(n) = self.kismet.selected.and_then(|e| self.kismet.node(e)).cloned() else {
            return div().w_full().p_4().text_xs().text_color(theme.muted_foreground).child("Select a node to see and edit its properties. The node positions are rebuilt from the connectors the cooker kept; moving a node only changes this view.").into_any_element();
        };
        let origin = n["originator"]["export"].as_u64().map(|e| (e as usize, n["originator"]["path"].as_str().unwrap_or("").rsplit('.').next().unwrap_or("").to_string()));
        let props = self.kismet.node_props.clone().expect("made above");
        v_flex()
            .size_full()
            .text_xs()
            .child(
                v_flex()
                    .p_2()
                    .gap_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().font_family("Menlo").text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(title_of(&n)))
                    .child(h_flex().flex_wrap().gap_x_3().text_color(theme.muted_foreground).child(div().font_family("Menlo").child(n["class"].as_str().unwrap_or("").to_string())).child(n["kind"].as_str().unwrap_or("").to_string()).child(format!("export {}", n["export"])))
                    .children(n["comment"].as_str().filter(|c| !c.is_empty()).map(|c| div().text_color(theme.muted_foreground).child(c.to_string())))
                    .children(origin.map(|(e, name)| {
                        div().id("kis-origin").cursor_pointer().text_color(theme.link).hover(|s| s.underline()).child(format!("Fired by {name}: show it")).on_click(cx.listener(move |this, _, _, cx| {
                            this.mode = Mode::Scene;
                            let level = this.kismet.level.clone().unwrap_or_default();
                            this.select(Some((level, e)), true, cx);
                            this.frame_selection();
                        }))
                    })),
            )
            .child(div().id("kis-props").flex_1().min_h_0().overflow_y_scroll().p_2().child(props))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_without_positions_follow_their_links() {
        let nodes = vec![
            json!({ "export": 1, "kind": "event", "inputs": [], "outputs": [{ "drawY": 100, "links": [{ "op": 2, "input": 0 }] }], "variables": [{ "drawX": 50, "linked": [] }], "events": [], "width": 120 }),
            json!({ "export": 2, "kind": "action", "inputs": [{ "drawY": null }], "outputs": [], "variables": [{ "drawX": null, "linked": [3] }], "events": [] }),
            json!({ "export": 3, "kind": "variable", "inputs": [], "outputs": [], "variables": [], "events": [] }),
        ];
        let b = layout(&nodes, &HashMap::new());
        let (a, act, var) = (b[&1], b[&2], b[&3]);
        assert_eq!(a.y, 100.0 - TITLE - ROW / 2.0);
        assert_eq!(act.x, a.x + a.w + GAP);
        assert_eq!(act.y, a.y);
        assert_eq!(var.y, act.y + act.h + 40.0);
        let moved = layout(&nodes, &HashMap::from([(2, (7.0, 9.0))]));
        assert_eq!((moved[&2].x, moved[&2].y), (7.0, 9.0));
    }
}
