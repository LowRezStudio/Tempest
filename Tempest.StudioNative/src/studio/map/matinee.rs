//! The Matinee editor under the 3D view (`matinee.*`): groups and their
//! tracks (add, rename, recolour, remove; drive the actor selected in the
//! view), every key list on a timeline (click a key to select it, drag to
//! retime (Alt: past its neighbours), double-click a row to add a key there,
//! K adds one at the playhead, Delete removes), the selected key or track in
//! the strip below (time, value, interp mode, tangents, event/cut/sound
//! fields; move frame, rotation mode…), a curve editor (`curve.rs`), and
//! playback with loop, section and rate. Playing or scrubbing previews the
//! movements, scales, light colours and toggles in the view exactly like
//! the engine evaluates them (`tempest_assets::matinee_curve`); in this view
//! a gizmo move of a driven actor keys its movement instead of moving it.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use glam::{Mat4, Vec3};
use gpui::{canvas, div, prelude::*, px, AnyElement, Bounds, Context, MouseButton, Pixels, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Selectable, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};
use tempest_assets::map::{decompose, from_view, inverse, mul, to_view, Mat};
use tempest_assets::matinee_curve::{initial_tm, key_transform, rotation_translation, world_transform, Mode as CurveMode, Point};

use super::{Actor, MapEditor, Mode};
use crate::map_render::{rgba, Cv};
use crate::studio::Tone;

/// Interp modes, as the strip lists them.
pub const MODES: &[(&str, &str)] = &[("CIM_CurveAutoClamped", "Auto clamped"), ("CIM_CurveAuto", "Auto"), ("CIM_CurveUser", "User"), ("CIM_CurveBreak", "Break"), ("CIM_Linear", "Linear"), ("CIM_Constant", "Constant")];

/// The track classes the game's maps use: (class, label, properties, director group only).
pub const TRACK_CLASSES: &[(&str, &str, &[&str], bool)] = &[
    ("InterpTrackMove", "Movement", &[], false),
    ("InterpTrackEvent", "Event", &[], false),
    ("InterpTrackAkEvent", "Wwise event", &[], false),
    ("InterpTrackSound", "Sound", &[], false),
    ("InterpTrackAnimControl", "Animation", &[], false),
    ("InterpTrackToggle", "Toggle", &[], false),
    ("InterpTrackVisibility", "Visibility", &[], false),
    ("InterpTrackFade", "Fade", &[], true),
    ("InterpTrackDirector", "Director", &[], true),
    ("InterpTrackFloatProp", "Float property", &["Brightness", "Radius", "FOVAngle", "DrawScale"], false),
    ("InterpTrackVectorProp", "Vector property", &["DrawScale3D"], false),
    ("InterpTrackColorProp", "Colour property", &["LightColor"], false),
    ("InterpTrackFloatMaterialParam", "Material scalar", &[""], false),
    ("InterpTrackVectorMaterialParam", "Material vector", &[""], false),
];

fn arr(v: &J) -> &[J] {
    v.as_array().map(|a| a.as_slice()).unwrap_or(&[])
}

fn nums(v: &J) -> Vec<f64> {
    match v {
        J::Array(a) => a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect(),
        J::Number(n) => vec![n.as_f64().unwrap_or(0.0)],
        _ => Vec::new(),
    }
}

/// The points of a key list (numeric keys only).
pub fn points(keys: &[J]) -> Vec<Point> {
    let width = keys.iter().map(|k| k["value"].as_array().map(|a| a.len()).unwrap_or(1)).max().unwrap_or(1).max(1);
    let fill = |v: &J| {
        let mut n = nums(v);
        n.resize(width, 0.0);
        n
    };
    keys.iter().filter(|k| k["value"].is_number() || k["value"].is_array()).map(|k| Point { time: k["time"].as_f64().unwrap_or(0.0), out: fill(&k["value"]), arrive: fill(&k["arrive"]), leave: fill(&k["leave"]), mode: CurveMode::parse(k["mode"].as_str().unwrap_or("")) }).collect()
}

fn curve_of<'a>(track: &'a J, name: &str) -> Option<&'a J> {
    arr(&track["curves"]).iter().find(|c| c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

pub fn is_move(t: &J) -> bool {
    t["class"].as_str().is_some_and(|c| c.eq_ignore_ascii_case("interptrackmove"))
}

/// What a move track's keys give: position and Euler curves, or its split axes.
pub struct MoveCurves {
    pub pos: Vec<Point>,
    pub euler: Vec<Point>,
    pub axes: Option<[Vec<Point>; 6]>,
}

pub fn move_curves(group: &J, mv: &J) -> MoveCurves {
    let pts = |t: Option<&J>, name: &str| t.and_then(|t| curve_of(t, name)).map(|c| points(arr(&c["keys"]))).unwrap_or_default();
    let subs: Vec<&J> = arr(&group["tracks"]).iter().filter(|x| x["parent"] == mv["export"]).collect();
    let names = ["axis_translationx", "axis_translationy", "axis_translationz", "axis_rotationx", "axis_rotationy", "axis_rotationz"];
    MoveCurves {
        pos: pts(Some(mv), "PosTrack"),
        euler: pts(Some(mv), "EulerTrack"),
        axes: (!subs.is_empty()).then(|| names.map(|a| pts(subs.iter().find(|s| s["moveAxis"].as_str().is_some_and(|m| m.eq_ignore_ascii_case(a))).copied(), "FloatTrack"))),
    }
}

fn view_arr(m: &Mat4) -> Vec<f64> {
    m.to_cols_array().iter().map(|x| *x as f64).collect()
}

fn view_mat(v: &[f64]) -> Mat4 {
    let a: Vec<f32> = v.iter().map(|x| *x as f32).collect();
    Mat4::from_cols_slice(&a)
}

fn remove_scaling(mut m: Mat) -> Mat {
    for row in m.iter_mut().take(3) {
        let len = (row[0] * row[0] + row[1] * row[1] + row[2] * row[2]).sqrt();
        if len > 1e-8 {
            for x in row.iter_mut().take(3) {
                *x /= len;
            }
        }
    }
    m
}

/// Where a driven actor is drawn at `t` (view space): its own scale times the track's world transform.
pub fn move_pose(c: &MoveCurves, t: f64, actor: &Mat4, relative: bool, raw: bool, start: f64) -> Mat4 {
    let Ok(placed) = from_view(&view_arr(actor)) else { return *actor };
    let (_, rot, loc) = decompose(&placed);
    let pre = mul(&placed, &inverse(&rotation_translation(rot, loc)));
    let initial = relative.then(|| initial_tm(rot, loc, key_transform(&c.pos, &c.euler, c.axes.as_ref(), start), raw));
    let (p, r) = key_transform(&c.pos, &c.euler, c.axes.as_ref(), t);
    view_mat(&to_view(&mul(&pre, &world_transform(p, r, initial.as_ref()))))
}

/// The key that puts the actor at view matrix `target` (`UpdateKeyframe`): position and
/// Euler angles in the track's frame, the angles unwound to the nearest of `near`.
pub fn key_from_pose(c: &MoveCurves, target: &Mat4, actor: &Mat4, relative: bool, raw: bool, near: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let (Ok(placed), Ok(goal)) = (from_view(&view_arr(actor)), from_view(&view_arr(target))) else { return ([0.0; 3], near) };
    let (_, rot, loc) = decompose(&placed);
    let pre = mul(&placed, &inverse(&rotation_translation(rot, loc)));
    let world = remove_scaling(mul(&inverse(&pre), &goal));
    let rel = if relative { mul(&world, &inverse(&initial_tm(rot, loc, key_transform(&c.pos, &c.euler, c.axes.as_ref(), 0.0), raw))) } else { world };
    let (_, r, _) = decompose(&rel);
    let k = 180.0 / 32768.0;
    let e = [r[2] * k, r[0] * k, r[1] * k];
    let unwound = [0, 1, 2].map(|i| e[i] + ((near[i] - e[i]) / 360.0).round() * 360.0);
    ([rel[3][0], rel[3][1], rel[3][2]], unwound)
}

/// What to do with an edit's reply (the timeline as it is now).
pub type Then = Option<Box<dyn FnOnce(&mut MapEditor, &J)>>;

#[derive(Default, Clone)]
pub struct Preview {
    /// View matrix per driven actor export.
    pub poses: HashMap<usize, Mat4>,
    pub paths: Vec<(J, Vec<Vec3>)>,
    pub hidden: HashSet<usize>,
    /// Light colour (as stored) and brightness per actor export.
    pub tints: HashMap<usize, (Option<Vec<f64>>, Option<f64>)>,
}

/// The state a timed key list (`ETTA_On`, `EVTA_Hide`…) leaves at `t`.
fn last_key(t: &J, time: f64) -> Option<String> {
    let mut v = None;
    for k in arr(&t["curves"][0]["keys"]) {
        if k["time"].as_f64().unwrap_or(0.0) <= time {
            if let Some(s) = k["value"].as_str() {
                v = Some(s.to_ascii_lowercase());
            }
        }
    }
    v
}

/// What the timeline does to the view at `t`.
pub fn preview(tl: &J, t: f64, actor_of: &dyn Fn(usize) -> Option<Actor>) -> Preview {
    let mut out = Preview::default();
    let start = if tl["settings"]["forceStartPos"] == true { tl["settings"]["forceStartPosition"].as_f64().unwrap_or(0.0) } else { 0.0 };
    let length = tl["length"].as_f64().unwrap_or(1.0);
    for g in arr(&tl["groups"]) {
        let driven: Vec<usize> = arr(&g["actors"]).iter().filter_map(|a| a["export"].as_u64().map(|e| e as usize)).collect();
        let mv = arr(&g["tracks"]).iter().find(|x| is_move(x) && x["disabled"] != true);
        let mut scale: Option<Vec<f64>> = None;
        let (mut color, mut brightness, mut hide) = (None, None, false);
        for tr in arr(&g["tracks"]) {
            if tr["disabled"] == true {
                continue;
            }
            let cls = tr["class"].as_str().unwrap_or("").to_ascii_lowercase();
            let prop = tr["property"].as_str().unwrap_or("").to_ascii_lowercase();
            let value = || arr(&tr["curves"]).iter().find(|c| c["kind"] == "curve").map(|c| points(arr(&c["keys"]))).filter(|p| !p.is_empty()).map(|p| tempest_assets::matinee_curve::eval(&p, t, &[0.0]));
            match (cls.as_str(), prop.as_str()) {
                ("interptrackvectorprop", "drawscale3d") => scale = value(),
                ("interptrackcolorprop", "lightcolor") => color = value().or(color),
                ("interptracklinearcolorprop", "lightcolor") => color = value().map(|v| v.into_iter().take(3).collect()).or(color),
                ("interptrackfloatprop", "brightness") => brightness = value().and_then(|v| v.first().copied()).or(brightness),
                ("interptracktoggle", _) => match last_key(tr, t).as_deref() {
                    Some("etta_off") => hide = true,
                    Some("etta_on") => hide = false,
                    _ => {}
                },
                ("interptrackvisibility", _) => match last_key(tr, t).as_deref() {
                    Some("evta_hide") => hide = true,
                    Some("evta_show") => hide = false,
                    _ => {}
                },
                _ => {}
            }
        }
        for &a in &driven {
            if hide {
                out.hidden.insert(a);
            }
            if color.is_some() || brightness.is_some() {
                out.tints.insert(a, (color.clone(), brightness));
            }
        }
        let scaled = |m: Mat4, placed: &Actor| -> Mat4 {
            let Some(s) = &scale else { return m };
            let base = placed.draw_scale3d.unwrap_or([1.0; 3]);
            let d = |v: f64| if v == 0.0 { 1.0 } else { v };
            // View axes are (x, z, y) of Unreal's.
            let f = [s.first().copied().unwrap_or(1.0) / d(base[0]), s.get(2).copied().unwrap_or(1.0) / d(base[2]), s.get(1).copied().unwrap_or(1.0) / d(base[1])];
            let mut m = m;
            m.x_axis = (m.x_axis.truncate() * f[0] as f32).extend(m.x_axis.w);
            m.y_axis = (m.y_axis.truncate() * f[1] as f32).extend(m.y_axis.w);
            m.z_axis = (m.z_axis.truncate() * f[2] as f32).extend(m.z_axis.w);
            m
        };
        let first = driven.first().and_then(|&e| actor_of(e));
        let Some(mv) = mv else {
            if scale.is_some() {
                if let Some(a) = &first {
                    for &e in &driven {
                        out.poses.insert(e, scaled(a.matrix, a));
                    }
                }
            }
            continue;
        };
        let c = move_curves(g, mv);
        if c.axes.is_none() && c.pos.is_empty() {
            continue;
        }
        let relative = mv["moveFrame"].as_str().is_some_and(|f| f.to_ascii_lowercase().contains("relative"));
        let raw = mv["rawInitial"] == true;
        for &e in &driven {
            let Some(placed) = actor_of(e) else { continue };
            let m = move_pose(&c, t, &placed.matrix, relative, raw, start);
            out.poses.insert(e, scaled(m, &placed));
        }
        // The path: through the first driven actor, or from the world origin.
        let base = first.as_ref().map(|a| a.matrix).unwrap_or(Mat4::IDENTITY);
        let steps = ((length * 30.0).round() as usize).clamp(48, 400);
        let pts = (0..=steps).map(|i| move_pose(&c, length * i as f64 / steps as f64, &base, relative, raw, start).w_axis.truncate()).collect();
        out.paths.push((g["color"].clone(), pts));
    }
    out
}

#[derive(Clone, PartialEq, Debug)]
pub struct KeyRef {
    pub track: usize,
    pub curve: String,
    pub key: usize,
}

#[derive(Clone)]
enum LaneDrag {
    Scrub,
    Key { r: KeyRef, start: f32, lo: f64, hi: f64, time: f64, moved: bool },
}

pub struct Matinee {
    pub at: Option<(String, usize)>,
    pub tl: Option<J>,
    pub time: f64,
    pub playing: bool,
    pub key: Option<KeyRef>,
    pub group: Option<usize>,
    pub track: Option<usize>,
    pub looping: bool,
    pub section_only: bool,
    pub rate: Option<f64>,
    /// The curve editor's track and curve path.
    pub curve: Option<(usize, String)>,
    pub list: Option<Vec<J>>,
    pub list_for: Option<String>,
    pub preview: Preview,
    show_settings: bool,
    add_track_for: Option<usize>,
    track_class: usize,
    track_property: String,
    lane: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    drag: Option<LaneDrag>,
    pub(super) curve_view: super::curve::CurveView,
    play_seq: u64,
}

impl Matinee {
    pub fn new(_window: &mut Window, _cx: &mut Context<MapEditor>) -> Matinee {
        Matinee { at: None, tl: None, time: 0.0, playing: false, key: None, group: None, track: None, looping: true, section_only: false, rate: None, curve: None, list: None, list_for: None, preview: Preview::default(), show_settings: false, add_track_for: None, track_class: 0, track_property: String::new(), lane: Default::default(), drag: None, curve_view: Default::default(), play_seq: 0 }
    }

    pub fn close(&mut self) {
        self.at = None;
        self.tl = None;
        self.playing = false;
        self.key = None;
        self.curve = None;
        self.list = None;
        self.list_for = None;
        self.preview = Preview::default();
    }

    pub fn length(&self) -> f64 {
        self.tl.as_ref().and_then(|t| t["length"].as_f64()).unwrap_or(1.0).max(0.01)
    }

    pub fn track(&self, e: usize) -> Option<&J> {
        arr(&self.tl.as_ref()?["groups"]).iter().flat_map(|g| arr(&g["tracks"])).find(|t| t["export"].as_u64() == Some(e as u64))
    }

    pub fn group_of(&self, track: usize) -> Option<&J> {
        arr(&self.tl.as_ref()?["groups"]).iter().find(|g| arr(&g["tracks"]).iter().any(|t| t["export"].as_u64() == Some(track as u64)))
    }

    pub fn curve(&self, track: usize, path: &str) -> Option<&J> {
        arr(&self.track(track)?["curves"]).iter().find(|c| c["path"] == path)
    }

    pub fn key_json(&self, r: &KeyRef) -> Option<&J> {
        arr(&self.curve(r.track, &r.curve)?["keys"]).get(r.key)
    }

    /// Whether the timeline switches this actor off at the playhead.
    pub fn hides(&self, level: &str, export: usize) -> bool {
        self.at.as_ref().is_some_and(|a| a.0 == level) && self.preview.hidden.contains(&export)
    }

    /// The range playback covers: the whole timeline, or the editor section.
    fn range(&self) -> (f64, f64) {
        let len = self.length();
        let Some(tl) = &self.tl else { return (0.0, len) };
        let s = nums(&tl["section"]);
        if self.section_only && s.len() == 2 && s[1] > s[0] {
            (s[0].max(0.0), s[1].min(len))
        } else {
            (0.0, len)
        }
    }
}

fn fmt(n: f64) -> String {
    format!("{}", (n * 1000.0).round() / 1000.0)
}

impl MapEditor {
    pub(super) fn open_matinee(&mut self, level: String, index: usize, record: bool, cx: &mut Context<Self>) {
        let call = self.call("matinee.get", json!({ "path": level, "index": index }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    let same = this.matinee.at.as_ref() == Some(&(level.clone(), index));
                    this.matinee.tl = Some(r.json);
                    this.matinee.at = Some((level.clone(), index));
                    if !same {
                        this.matinee.time = 0.0;
                        this.matinee.key = None;
                        this.matinee.curve = None;
                    }
                    this.mode = Mode::Matinee;
                    if this.matinee.list_for.as_deref() != Some(level.as_str()) {
                        this.load_matinee_list(level.clone(), false, cx);
                    }
                    this.apply_matinee(cx);
                    if record {
                        this.record(cx);
                    }
                    cx.notify();
                }
                Err(e) => this.toast(Tone::Error, format!("Could not read the Matinee: {e}"), cx),
            });
        })
        .detach();
    }

    /// Re-read the open timeline (after an undo); closed when its action is gone.
    pub(super) fn refresh_matinee(&mut self, cx: &mut Context<Self>) {
        let Some((level, index)) = self.matinee.at.clone() else { return };
        let call = self.call("matinee.get", json!({ "path": level, "index": index }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => this.matinee.tl = Some(r.json),
                    Err(_) => {
                        this.matinee.close();
                        if this.mode == Mode::Matinee {
                            this.mode = Mode::Scene;
                        }
                    }
                }
                this.apply_matinee(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn load_matinee_list(&mut self, level: String, open_first: bool, cx: &mut Context<Self>) {
        let call = self.call("matinee.list", json!({ "path": level }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        this.matinee.list = Some(arr(&r.json["matinees"]).to_vec());
                        this.matinee.list_for = Some(level.clone());
                        if open_first {
                            match this.matinee.list.as_ref().and_then(|l| l.first()).and_then(|m| m["export"].as_u64()) {
                                Some(e) => this.open_matinee(level.clone(), e as usize, true, cx),
                                None => this.toast(Tone::Info, "This level has no Matinee: New Matinee makes one", cx),
                            }
                        }
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not list the Matinees: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A new Matinee in `level`'s top sequence (or `sequence`), driving `actor` when given.
    pub(crate) fn create_matinee(&mut self, level: String, actor: Option<usize>, length: Option<f64>, sequence: Option<usize>, cx: &mut Context<Self>) {
        let call = self.call("matinee.create", json!({ "path": level, "length": length.unwrap_or(5.0), "actor": actor, "sequence": sequence }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.note_edit(&level, cx);
                    this.matinee.list_for = None;
                    if let Some(a) = r.json["action"].as_u64() {
                        this.open_matinee(level.clone(), a as usize, true, cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("Could not create the Matinee: {e}"), cx),
            });
        })
        .detach();
    }

    /// One Matinee edit on the open timeline (one undo step of the level); `then` gets the reply.
    pub(crate) fn matinee_edit(&mut self, method: &'static str, mut params: J, then: Then, cx: &mut Context<Self>) {
        let Some((level, index)) = self.matinee.at.clone() else { return };
        params["path"] = json!(level);
        params["index"] = json!(index);
        let call = self.call(method, params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.matinee.tl = Some(r.json.clone());
                    this.note_edit(&level, cx);
                    if let Some(f) = then {
                        f(this, &r.json);
                    }
                    this.apply_matinee(cx);
                    cx.notify();
                }
                Err(e) => this.toast(Tone::Error, format!("Could not change the Matinee: {e}"), cx),
            });
        })
        .detach();
    }

    pub(crate) fn set_key(&mut self, r: KeyRef, change: J, cx: &mut Context<Self>) {
        let mut p = change;
        p["track"] = json!(r.track);
        p["curve"] = json!(r.curve);
        p["key"] = json!(r.key);
        let r2 = r.clone();
        self.matinee_edit("matinee.setKey", p, Some(Box::new(move |this, j| {
            if let Some(k) = j["key"].as_u64() {
                this.matinee.key = Some(KeyRef { key: k as usize, ..r2 });
            }
        })), cx);
    }

    pub(crate) fn add_key(&mut self, track: usize, curve: String, time: f64, extra: J, cx: &mut Context<Self>) {
        let mut p = if extra.is_object() { extra } else { json!({}) };
        p["track"] = json!(track);
        p["curve"] = json!(curve);
        p["time"] = json!(time);
        self.matinee_edit("matinee.addKey", p, Some(Box::new(move |this, j| {
            if let Some(k) = j["key"].as_u64() {
                this.matinee.key = Some(KeyRef { track, curve: curve.clone(), key: k as usize });
                this.matinee.track = Some(track);
                this.matinee.group = this.matinee.group_of(track).and_then(|g| g["export"].as_u64()).map(|g| g as usize);
            }
        })), cx);
    }

    pub(crate) fn remove_key(&mut self, r: KeyRef, cx: &mut Context<Self>) {
        self.matinee.key = None;
        self.matinee_edit("matinee.removeKey", json!({ "track": r.track, "curve": r.curve, "key": r.key }), None, cx);
    }

    fn select_key(&mut self, r: Option<KeyRef>) {
        if let Some(r) = &r {
            self.matinee.track = Some(r.track);
            self.matinee.group = self.matinee.group_of(r.track).and_then(|g| g["export"].as_u64()).map(|g| g as usize).or(self.matinee.group);
        }
        self.matinee.key = r;
    }

    /// The actor a group drives, as placed in the map.
    fn driven_actor(&self, g: &J) -> Option<Actor> {
        let level = &self.matinee.at.as_ref()?.0;
        let e = arr(&g["actors"]).iter().find_map(|a| a["export"].as_u64())? as usize;
        self.actor_at(level, e).cloned()
    }

    /// A key at the playhead: on a move track from where its actor is drawn, else with the curve's value there.
    pub(crate) fn key_at_playhead(&mut self, track: usize, cx: &mut Context<Self>) {
        let (Some(t), Some(g)) = (self.matinee.track(track).cloned(), self.matinee.group_of(track).cloned()) else { return };
        let time = (self.matinee.time * 1000.0).round() / 1000.0;
        if is_move(&t) {
            let Some(actor) = self.driven_actor(&g) else {
                self.add_key(track, "PosTrack.Points".into(), time, json!({}), cx);
                return;
            };
            let pose = self.matinee.preview.poses.get(&actor.export).copied().unwrap_or(actor.matrix);
            let relative = t["moveFrame"].as_str().is_some_and(|f| f.to_ascii_lowercase().contains("relative"));
            let (pos, euler) = key_from_pose(&move_curves(&g, &t), &pose, &actor.matrix, relative, t["rawInitial"] == true, [0.0; 3]);
            self.add_key(track, "PosTrack.Points".into(), time, json!({ "value": pos, "values": { "EulerTrack.Points": euler } }), cx);
            return;
        }
        if let Some(c) = arr(&t["curves"]).first().and_then(|c| c["path"].as_str()) {
            self.add_key(track, c.to_string(), time, json!({}), cx);
        }
    }

    /// A gizmo move of a driven actor in the Matinee view: the key under the playhead takes the
    /// new placement, or a new key is made there; the actor's own placement is left alone.
    pub(super) fn capture_move(&mut self, level: &str, actor: &Actor, matrix: Mat4, cx: &mut Context<Self>) -> bool {
        let Some(tl) = self.matinee.tl.clone() else { return false };
        if self.matinee.at.as_ref().map(|a| a.0.as_str()) != Some(level) {
            return false;
        }
        for g in arr(&tl["groups"]) {
            if !arr(&g["actors"]).iter().any(|a| a["export"].as_u64() == Some(actor.export as u64)) {
                continue;
            }
            let Some(mv) = arr(&g["tracks"]).iter().find(|t| is_move(t) && t["disabled"] != true && arr(&t["subTracks"]).is_empty()) else { continue };
            let keys = curve_of(mv, "PosTrack").map(|c| arr(&c["keys"]).to_vec()).unwrap_or_default();
            let eulers = curve_of(mv, "EulerTrack").map(|c| arr(&c["keys"]).to_vec()).unwrap_or_default();
            let now = self.matinee.time;
            let k = keys.iter().position(|x| (x["time"].as_f64().unwrap_or(0.0) - now).abs() < 1.0 / 60.0);
            // The angles of the key there, else of the key before the playhead.
            let prev = match k {
                Some(k) => eulers.get(k).cloned(),
                None => eulers.iter().rev().find(|e| e["time"].as_f64().unwrap_or(0.0) <= now).cloned(),
            };
            let near = prev.map(|p| nums(&p["value"])).filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]).unwrap_or([0.0; 3]);
            let relative = mv["moveFrame"].as_str().is_some_and(|f| f.to_ascii_lowercase().contains("relative"));
            let (pos, euler) = key_from_pose(&move_curves(g, mv), &matrix, &actor.matrix, relative, mv["rawInitial"] == true, near);
            let change = json!({ "value": pos, "values": { "EulerTrack.Points": euler } });
            let track = mv["export"].as_u64().unwrap_or(0) as usize;
            match k {
                Some(k) => self.set_key(KeyRef { track, curve: "PosTrack.Points".into(), key: k }, change, cx),
                None => self.add_key(track, "PosTrack.Points".into(), (now * 1000.0).round() / 1000.0, change, cx),
            }
            return true;
        }
        false
    }

    /// Compute the preview at the playhead and put it on the scene: driven actors posed,
    /// switched-off ones hidden, lights tinted.
    pub(super) fn apply_matinee(&mut self, _cx: &mut Context<Self>) {
        let active = self.mode == Mode::Matinee;
        let level = self.matinee.at.as_ref().map(|a| a.0.clone());
        self.matinee.preview = match (&self.matinee.tl, &level, active) {
            (Some(tl), Some(level), true) => {
                let actors: HashMap<usize, Actor> = self.level(level).map(|l| l.actors.iter().map(|a| (a.export, a.clone())).collect()).unwrap_or_default();
                preview(tl, self.matinee.time, &|e| actors.get(&e).cloned())
            }
            _ => Preview::default(),
        };
        let pv = self.matinee.preview.clone();
        let nodes: Vec<(usize, Mat4, Option<[f32; 3]>)> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(k, &(li, ai))| {
                let l = &self.levels[li];
                let a = &l.actors[ai];
                let mine = level.as_deref() == Some(l.path.as_str());
                let m = if mine { pv.poses.get(&a.export).copied().unwrap_or(a.matrix) } else { a.matrix };
                let tint = if mine {
                    pv.tints.get(&a.export).map(|(c, b)| {
                        let c = c.clone().unwrap_or_else(|| vec![1.0; 3]);
                        // Colours are 0..255 (Color) or 0..1 (LinearColor); brightness 1 is as placed.
                        let s = if c.iter().any(|v| *v > 1.0) { 1.0 / 255.0 } else { 1.0 } * b.unwrap_or(1.0).clamp(0.15, 2.0);
                        [0, 1, 2].map(|i| (c.get(i).copied().unwrap_or(1.0) * s).min(1.0) as f32)
                    })
                } else {
                    None
                };
                (k, m, tint)
            })
            .collect();
        self.view.with(|s| {
            for (k, m, tint) in nodes {
                if let Some(n) = s.layers[0].get_mut(k) {
                    n.matrix = m;
                    n.tint = tint;
                }
            }
            s.invalidate();
        });
        self.apply_visibility();
    }

    pub(super) fn matinee_paths(&self, out: &mut Vec<Cv>) {
        for (color, pts) in &self.matinee.preview.paths {
            let c = nums(color);
            let col = if c.len() >= 3 { [(c[0] / 255.0) as f32, (c[1] / 255.0) as f32, (c[2] / 255.0) as f32, 1.0] } else { rgba(0xf9e2af, 1.0) };
            for s in pts.windows(2) {
                out.push(Cv::new(s[0], col));
                out.push(Cv::new(s[1], col));
            }
            if let Some(end) = pts.last() {
                crate::map_render::sphere_lines(*end, 0.15, col, out);
            }
        }
    }

    pub(super) fn set_matinee_time(&mut self, t: f64, cx: &mut Context<Self>) {
        self.matinee.time = t.clamp(0.0, self.matinee.length());
        self.apply_matinee(cx);
        cx.notify();
    }

    pub(super) fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.matinee.playing {
            self.matinee.playing = false;
            cx.notify();
            return;
        }
        let (start, end) = self.matinee.range();
        if self.matinee.time >= end || self.matinee.time < start {
            self.matinee.time = start;
        }
        self.matinee.playing = true;
        self.matinee.play_seq += 1;
        let seq = self.matinee.play_seq;
        cx.spawn(async move |this, cx| {
            let mut last = std::time::Instant::now();
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                let go = this.update(cx, |this, cx| {
                    if !this.matinee.playing || this.matinee.play_seq != seq {
                        return false;
                    }
                    let rate = this.matinee.rate.or_else(|| this.matinee.tl.as_ref().and_then(|t| t["settings"]["playRate"].as_f64())).unwrap_or(1.0);
                    let dt = last.elapsed().as_secs_f64();
                    last = std::time::Instant::now();
                    let (start, end) = this.matinee.range();
                    let t = this.matinee.time + dt * rate;
                    if t >= end {
                        if this.matinee.looping {
                            this.matinee.time = start;
                        } else {
                            this.matinee.time = end;
                            this.matinee.playing = false;
                        }
                    } else {
                        this.matinee.time = t;
                    }
                    this.apply_matinee(cx);
                    cx.notify();
                    this.matinee.playing
                });
                if !matches!(go, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Keys of the Matinee view (in the 3D view): Delete removes the key, Space plays, Home/End, K.
    pub(super) fn matinee_key(&mut self, k: &str, m: &gpui::Modifiers, cx: &mut Context<Self>) -> bool {
        if m.platform || m.control || m.alt {
            return false;
        }
        match k {
            "delete" | "backspace" if self.matinee.key.is_some() => {
                let r = self.matinee.key.clone().unwrap();
                self.remove_key(r, cx);
            }
            "space" => self.toggle_play(cx),
            "home" => self.set_matinee_time(self.matinee.range().0, cx),
            "end" => self.set_matinee_time(self.matinee.range().1, cx),
            "k" if self.matinee.track.is_some() => {
                let t = self.matinee.track.unwrap();
                self.key_at_playhead(t, cx);
            }
            _ => return false,
        }
        true
    }

    // ----- typed values ------------------------------------------------------------------

    pub(super) fn commit_matinee_field(&mut self, key: &str, value: String, cx: &mut Context<Self>) {
        let v = value.trim().to_string();
        let num = v.replace(',', ".").parse::<f64>().ok();
        let tl = self.matinee.tl.clone().unwrap_or_default();
        let (action, data) = (tl["action"].as_u64(), tl["data"].as_u64());
        let set = |this: &mut MapEditor, object: Option<u64>, values: J, cx: &mut Context<MapEditor>| {
            if let Some(o) = object {
                this.matinee_edit("matinee.set", json!({ "object": o, "values": values }), None, cx);
            }
        };
        let (part, rest) = key.split_once(':').unwrap_or((key, ""));
        match part {
            "length" => {
                if let Some(n) = num.filter(|n| *n > 0.0) {
                    set(self, data, json!({ "InterpLength": n }), cx);
                }
            }
            "rate" => self.matinee.rate = num.filter(|n| *n > 0.0),
            "startpos" => set(self, action, json!({ "ForceStartPosition": num.unwrap_or(0.0) }), cx),
            "playrate" => set(self, action, json!({ "PlayRate": num.unwrap_or(1.0) }), cx),
            "secstart" => set(self, data, json!({ "EdSectionStart": num.unwrap_or(0.0) }), cx),
            "secend" => set(self, data, json!({ "EdSectionEnd": num.unwrap_or(0.0) }), cx),
            "newgroup" => {
                let name = if v.is_empty() { format!("Group{}", arr(&tl["groups"]).len() + 1) } else { v };
                self.add_group(name, false, cx);
            }
            "rename" => {
                if let (Ok(g), false) = (rest.parse::<usize>(), v.is_empty()) {
                    self.matinee_edit("matinee.setGroup", json!({ "group": g, "name": v }), None, cx);
                }
            }
            "color" => {
                let h = v.trim_start_matches('#');
                if let (Ok(g), Ok(c)) = (rest.parse::<usize>(), u32::from_str_radix(h, 16)) {
                    self.matinee_edit("matinee.setGroup", json!({ "group": g, "color": [(c >> 16) & 255, (c >> 8) & 255, c & 255] }), None, cx);
                }
            }
            "trackprop" => self.matinee.track_property = v,
            "title" => {
                if let Some(t) = self.matinee.track {
                    set(self, Some(t as u64), json!({ "TrackTitle": format!("\"{v}\"") }), cx);
                }
            }
            "prop" => {
                if let Some(t) = self.matinee.track {
                    let cls = self.matinee.track(t).and_then(|x| x["class"].as_str()).unwrap_or("").to_ascii_lowercase();
                    let name = if cls.contains("materialparam") || cls.contains("particleparam") { "ParamName" } else { "PropertyName" };
                    set(self, Some(t as u64), json!({ name: format!("'{v}'") }), cx);
                }
            }
            "time" | "value" | "arrive" | "leave" | "field" => {
                let Some(r) = self.matinee.key.clone() else { return };
                let Some(k) = self.matinee.key_json(&r).cloned() else { return };
                let idx: usize = rest.parse().unwrap_or(0);
                match part {
                    "time" => {
                        if let Some(t) = num {
                            self.set_key(r, json!({ "time": t }), cx);
                        }
                    }
                    "value" | "arrive" | "leave" => {
                        let Some(n) = num else { return };
                        let src = match part {
                            "value" => &k["value"],
                            "arrive" => &k["arrive"],
                            _ => &k["leave"],
                        };
                        let mut list = nums(src);
                        if list.is_empty() {
                            list = vec![0.0];
                        }
                        if idx < list.len() {
                            list[idx] = n;
                        }
                        let val = if list.len() == 1 { json!(list[0]) } else { json!(list) };
                        let change = match part {
                            "value" => json!({ "value": val }),
                            "arrive" => json!({ "arrive": val, "leave": k["leave"] }),
                            _ => json!({ "arrive": k["arrive"], "leave": val }),
                        };
                        self.set_key(r, change, cx);
                    }
                    _ => {
                        // A timed key's member as `Editor::set` text: names quoted, numbers and booleans as they are.
                        let name = rest.to_string();
                        let old = &k["fields"][&name];
                        let lower = name.to_ascii_lowercase();
                        let text = if old.is_number() || old.is_boolean() || old.is_object() || !(lower.contains("name") || lower.contains("group")) || (v.starts_with('\'') && v.ends_with('\'')) { v } else { format!("'{v}'") };
                        self.set_key(r, json!({ "fields": { name: text } }), cx);
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn add_group(&mut self, name: String, director: bool, cx: &mut Context<Self>) {
        self.matinee_edit("matinee.addGroup", json!({ "name": name, "director": director }), Some(Box::new(|this, j| {
            if let Some(g) = j["group"].as_u64() {
                this.matinee.group = Some(g as usize);
            }
        })), cx);
    }

    pub(crate) fn add_track(&mut self, group: usize, class: String, property: Option<String>, cx: &mut Context<Self>) {
        self.matinee_edit("matinee.addTrack", json!({ "group": group, "class": class, "property": property }), Some(Box::new(|this, j| {
            if let Some(t) = j["track"].as_u64() {
                this.matinee.track = Some(t as usize);
            }
        })), cx);
    }

    pub(crate) fn remove_track(&mut self, track: usize, cx: &mut Context<Self>) {
        if self.matinee.track == Some(track) {
            self.matinee.track = None;
        }
        if self.matinee.curve.as_ref().is_some_and(|c| c.0 == track) {
            self.matinee.curve = None;
        }
        self.matinee.key = None;
        self.matinee_edit("matinee.removeTrack", json!({ "track": track }), None, cx);
    }

    pub(crate) fn remove_group(&mut self, group: usize, cx: &mut Context<Self>) {
        if self.matinee.group == Some(group) {
            self.matinee.group = None;
        }
        self.matinee.key = None;
        self.matinee_edit("matinee.removeGroup", json!({ "group": group }), None, cx);
    }

    /// Make an actor of the timeline's level driven by a group, or not.
    pub(crate) fn connect(&mut self, group: usize, actor: usize, remove: bool, cx: &mut Context<Self>) {
        self.matinee_edit("matinee.connect", json!({ "group": group, "actor": actor, "remove": remove }), None, cx);
    }

    /// The actor selected in the view, when it is in the timeline's level (its owner export).
    fn view_actor(&self) -> Option<(usize, Actor)> {
        let level = &self.matinee.at.as_ref()?.0;
        let (l, _) = self.selected.as_ref()?;
        if l != level {
            return None;
        }
        let a = self.actor()?.clone();
        Some((a.owner(), a))
    }

    // ----- lanes ---------------------------------------------------------------------------

    fn lane_time(&self, x: Pixels) -> f64 {
        let b = self.matinee.lane.get();
        let w = f32::from(b.size.width).max(1.0);
        (((f32::from(x - b.origin.x)) / w) as f64 * self.matinee.length()).clamp(0.0, self.matinee.length())
    }

    /// The rows of the lanes: (track, curve) per row, `None` for group headers and the add-track row.
    fn lane_rows(&self) -> Vec<Option<(usize, String)>> {
        let mut rows = Vec::new();
        for g in self.matinee.tl.as_ref().map(|t| arr(&t["groups"])).unwrap_or(&[]) {
            rows.push(None);
            if self.matinee.add_track_for == g["export"].as_u64().map(|x| x as usize) {
                rows.push(None);
            }
            for t in arr(&g["tracks"]) {
                for c in arr(&t["curves"]) {
                    rows.push(Some((t["export"].as_u64().unwrap_or(0) as usize, c["path"].as_str().unwrap_or("").to_string())));
                }
            }
        }
        rows
    }

    fn lane_down(&mut self, e: &gpui::MouseDownEvent, cx: &mut Context<Self>) {
        let b = self.matinee.lane.get();
        let y = f32::from(e.position.y - b.origin.y);
        if y < 24.0 {
            self.matinee.drag = Some(LaneDrag::Scrub);
            let t = self.lane_time(e.position.x);
            self.set_matinee_time(t, cx);
            return;
        }
        let row = ((y - 24.0) / 24.0).floor() as usize;
        let Some(Some((track, curve))) = self.lane_rows().get(row).cloned() else { return };
        let len = self.matinee.length();
        let w = f32::from(b.size.width).max(1.0);
        let x = f32::from(e.position.x - b.origin.x);
        let keys = self.matinee.curve(track, &curve).map(|c| arr(&c["keys"]).to_vec()).unwrap_or_default();
        let hit = keys.iter().enumerate().map(|(i, k)| (i, ((k["time"].as_f64().unwrap_or(0.0) / len) as f32 * w - x).abs())).filter(|(_, d)| *d < 7.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i);
        if e.click_count == 2 && hit.is_none() {
            let t = (self.lane_time(e.position.x) * 1000.0).round() / 1000.0;
            self.add_key(track, curve, t, json!({}), cx);
            return;
        }
        let Some(k) = hit else {
            self.matinee.track = Some(track);
            self.matinee.key = None;
            cx.notify();
            return;
        };
        let r = KeyRef { track, curve, key: k };
        self.select_key(Some(r.clone()));
        let time = keys[k]["time"].as_f64().unwrap_or(0.0);
        let lo = if k > 0 { keys[k - 1]["time"].as_f64().unwrap_or(0.0) + 0.001 } else { 0.0 };
        let hi = if k + 1 < keys.len() { keys[k + 1]["time"].as_f64().unwrap_or(len) - 0.001 } else { len };
        self.matinee.drag = Some(LaneDrag::Key { r, start: x, lo, hi, time, moved: false });
        cx.notify();
    }

    fn lane_move(&mut self, e: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(d) = self.matinee.drag.clone() else { return };
        if e.pressed_button.is_none() {
            self.matinee.drag = None;
            return;
        }
        match d {
            LaneDrag::Scrub => {
                let t = self.lane_time(e.position.x);
                self.set_matinee_time(t, cx);
            }
            LaneDrag::Key { r, start, lo, hi, moved, .. } => {
                let x = f32::from(e.position.x - self.matinee.lane.get().origin.x);
                if !moved && (x - start).abs() < 3.0 {
                    return;
                }
                let t = self.lane_time(e.position.x);
                // Alt: past the neighbours (the key is re-sorted).
                let time = if e.modifiers.alt { t } else { t.clamp(lo, hi) };
                self.matinee.drag = Some(LaneDrag::Key { r, start, lo, hi, time, moved: true });
                self.set_matinee_time(time, cx);
            }
        }
    }

    fn lane_up(&mut self, cx: &mut Context<Self>) {
        if let Some(LaneDrag::Key { r, time, moved: true, .. }) = self.matinee.drag.take() {
            self.set_key(r, json!({ "time": (time * 1000.0).round() / 1000.0 }), cx);
        }
    }

    // ----- drawing --------------------------------------------------------------------------

    pub(super) fn render_matinee(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(tl) = self.matinee.tl.clone() else { return div().into_any_element() };
        let len = self.matinee.length();
        let level = self.matinee.at.as_ref().map(|a| a.0.clone()).unwrap_or_default();
        let me = cx.entity().downgrade();
        // Toolbar.
        let list: Vec<(usize, String)> = self.matinee.list.iter().flatten().map(|m| {
            let groups = arr(&m["groups"]).iter().filter_map(|g| g.as_str()).collect::<Vec<_>>().join(", ");
            let name = m["comment"].as_str().map(str::to_string).unwrap_or_else(|| m["path"].as_str().unwrap_or("").rsplit('.').next().unwrap_or("").to_string());
            (m["export"].as_u64().unwrap_or(0) as usize, if groups.is_empty() { name } else { format!("{name} · {groups}") })
        }).collect();
        let lvl = level.clone();
        let picker = Button::new("mt-pick").xsmall().icon(Icon::new(IconName::Clapperboard)).label(tl["name"].as_str().unwrap_or("Matinee").to_string()).tooltip("The level's Matinees").dropdown_menu(move |menu, _, _| {
            let mut menu = menu;
            for (e, n) in &list {
                let (me, e, l) = (me.clone(), *e, lvl.clone());
                menu = menu.item(PopupMenuItem::new(n.clone()).on_click(move |_, _, cx| {
                    let _ = me.update(cx, |this, cx| this.open_matinee(l.clone(), e, true, cx));
                }));
            }
            menu
        });
        let view_actor = self.view_actor();
        let new_tip = match &view_actor {
            Some((_, a)) => format!("A new Matinee moving {}", a.name),
            None => "A new, empty Matinee in this level's sequence".into(),
        };
        let va = view_actor.as_ref().map(|v| v.0);
        let l2 = level.clone();
        let st = &tl["settings"];
        let has_director = arr(&tl["groups"]).iter().any(|g| g["director"] == true);
        let toolbar = h_flex()
            .min_h(px(30.0))
            .flex_wrap()
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .child(picker)
            .child(Button::new("mt-new").xsmall().ghost().icon(Icon::new(IconName::Plus)).label("New").tooltip(new_tip).on_click(cx.listener(move |this, _, _, cx| this.create_matinee(l2.clone(), va, None, None, cx))))
            .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border))
            .child(Button::new("mt-start").xsmall().ghost().icon(Icon::new(IconName::SkipBack)).tooltip("Start (Home)").on_click(cx.listener(|this, _, _, cx| {
                let s = this.matinee.range().0;
                this.set_matinee_time(s, cx);
            })))
            .child(Button::new("mt-play").xsmall().ghost().icon(Icon::new(if self.matinee.playing { IconName::Pause } else { IconName::Play })).tooltip("Play / pause (Space)").on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx))))
            .child(Checkbox::new("mt-loop").label("loop").checked(self.matinee.looping).on_click(cx.listener(|this, v: &bool, _, cx| {
                this.matinee.looping = *v;
                cx.notify();
            })))
            .child(Checkbox::new("mt-section").label("section").checked(self.matinee.section_only).tooltip("Play only the section (EdSectionStart..End)").on_click(cx.listener(|this, v: &bool, _, cx| {
                this.matinee.section_only = *v;
                cx.notify();
            })))
            .child(div().text_xs().child("×"))
            .child(self.field("mat:rate", self.matinee.rate.map(fmt).unwrap_or_default(), 40.0, cx))
            .child(div().ml_2().text_xs().font_family("Menlo").child(format!("{:.2} /", self.matinee.time)))
            .child(self.field("mat:length", fmt(len), 52.0, cx))
            .child(div().text_xs().text_color(theme.muted_foreground).child("s"))
            .child(div().flex_1())
            .child(Button::new("mt-settings").xsmall().ghost().icon(Icon::new(IconName::Settings2)).label("Action").selected(self.matinee.show_settings).disabled(tl["action"].is_null()).tooltip("The Matinee action's settings").on_click(cx.listener(|this, _, _, cx| {
                this.matinee.show_settings = !this.matinee.show_settings;
                cx.notify();
            })))
            .child(self.field("mat:newgroup", String::new(), 100.0, cx))
            .child(Button::new("mt-add-group").xsmall().ghost().icon(Icon::new(IconName::Plus)).label("Group").tooltip("Add a group (type its name on the left)").on_click(cx.listener(|this, _, _, cx| this.commit_matinee_field("newgroup", String::new(), cx))))
            .when(!has_director, |d| d.child(Button::new("mt-director").xsmall().ghost().label("Director").tooltip("Add the director group (camera cuts, fades)").on_click(cx.listener(|this, _, _, cx| this.add_group("DirGroup".into(), true, cx)))));
        let mut col = v_flex().h(px(384.0)).flex_none().border_t_1().border_color(theme.border).bg(theme.secondary).text_xs().child(toolbar);
        if self.matinee.show_settings && !tl["action"].is_null() {
            let a = tl["action"].as_u64().unwrap_or(0);
            let mut row = h_flex().flex_wrap().gap_3().px_2().py_1().border_b_1().border_color(theme.border);
            for (k, (prop, key, text)) in [("bLooping", "looping", "Loop"), ("bRewindOnPlay", "rewindOnPlay", "Rewind on play"), ("bRewindIfAlreadyPlaying", "rewindIfAlreadyPlaying", "Rewind if playing"), ("bNoResetOnRewind", "noResetOnRewind", "No reset on rewind"), ("bReversePlayback", "reversePlayback", "Reverse"), ("bClientSideOnly", "clientSideOnly", "Client side only"), ("bForceStartPos", "forceStartPos", "Force start position")].into_iter().enumerate() {
                row = row.child(Checkbox::new(("mt-flag", k)).label(text).checked(st[key] == true).on_click(cx.listener(move |this, v: &bool, _, cx| this.matinee_edit("matinee.set", json!({ "object": a, "values": { prop: *v } }), None, cx))));
            }
            let sec = nums(&tl["section"]);
            row = row
                .child(h_flex().gap_1().child("start at").child(self.field("mat:startpos", fmt(st["forceStartPosition"].as_f64().unwrap_or(0.0)), 50.0, cx)))
                .child(h_flex().gap_1().child("PlayRate").child(self.field("mat:playrate", fmt(st["playRate"].as_f64().unwrap_or(1.0)), 50.0, cx)))
                .child(h_flex().gap_1().child("section").child(self.field("mat:secstart", fmt(sec.first().copied().unwrap_or(0.0)), 50.0, cx)).child("–").child(self.field("mat:secend", fmt(sec.get(1).copied().unwrap_or(0.0)), 50.0, cx)));
            col = col.child(row);
        }
        // Groups, tracks and lanes.
        let mut names = v_flex().w(px(288.0)).flex_none().border_r_1().border_color(theme.border).child(div().h(px(24.0)));
        let width = f32::from(self.matinee.lane.get().size.width).max(1.0);
        let x_of = move |t: f64| (t / len) as f32 * width;
        let mut lanes = v_flex().child(div().h(px(24.0)));
        for (gi, g) in arr(&tl["groups"]).iter().enumerate() {
            let ge = g["export"].as_u64().unwrap_or(0) as usize;
            let director = g["director"] == true;
            let c = nums(&g["color"]);
            let swatch = if c.len() >= 3 { gpui::rgb(((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32).into() } else { super::hsla(0xa6adc8) };
            let hex = if c.len() >= 3 { format!("#{:02x}{:02x}{:02x}", c[0] as u32, c[1] as u32, c[2] as u32) } else { "#a6adc8".into() };
            let actors = arr(&g["actors"]).iter().filter_map(|a| a["name"].as_str()).collect::<Vec<_>>().join(", ");
            let linked = va.is_some_and(|v| arr(&g["actors"]).iter().any(|a| a["export"].as_u64() == Some(v as u64)));
            let group_name = g["name"].as_str().filter(|n| !n.is_empty()).or(g["class"].as_str()).unwrap_or("").to_string();
            let mut head = h_flex()
                .id(("mt-group", gi))
                .group(format!("mtg{gi}"))
                .h(px(24.0))
                .px_2()
                .gap_1p5()
                .bg(theme.background.opacity(0.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .when(self.matinee.group == Some(ge), |d| d.border_1().border_color(theme.primary.opacity(0.5)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.matinee.group = Some(ge);
                    cx.notify();
                }))
                .child(div().size(px(10.0)).rounded_sm().bg(swatch));
            if !director {
                head = head.child(self.field(format!("mat:color:{ge}"), hex, 62.0, cx)).child(self.field(format!("mat:rename:{ge}"), group_name.clone(), 90.0, cx));
            } else {
                head = head.child(div().child(group_name.clone()));
            }
            let mut ops = h_flex().ml_auto().gap_0p5().invisible().group_hover(format!("mtg{gi}"), |s| s.visible());
            if let (false, Some((v, a)), false) = (director, view_actor.clone(), tl["action"].is_null()) {
                let static_mesh = a.class.eq_ignore_ascii_case("staticmeshactor") || a.class.to_ascii_lowercase().contains("collection");
                let tip = if linked { format!("Stop driving {}", a.name) } else { format!("Drive {} (the actor selected in the view){}", a.name, if static_mesh { " — static meshes do not move in game: use an InterpActor" } else { "" }) };
                ops = ops.child(Button::new(("mt-link", gi)).xsmall().ghost().icon(Icon::new(if linked { IconName::Unlink } else { IconName::Link2 })).tooltip(tip).on_click(cx.listener(move |this, _, _, cx| this.connect(ge, v, linked, cx))));
            }
            ops = ops
                .child(Button::new(("mt-add-track", gi)).xsmall().ghost().icon(Icon::new(IconName::Plus)).tooltip("Add a track").on_click(cx.listener(move |this, _, _, cx| {
                    this.matinee.add_track_for = if this.matinee.add_track_for == Some(ge) { None } else { Some(ge) };
                    this.matinee.track_class = if director { TRACK_CLASSES.iter().position(|c| c.0 == "InterpTrackDirector").unwrap_or(0) } else { 0 };
                    this.matinee.track_property.clear();
                    cx.notify();
                })))
                .child(Button::new(("mt-rm-group", gi)).xsmall().ghost().icon(Icon::new(IconName::Trash)).tooltip("Remove the group and its tracks").on_click(cx.listener(move |this, _, _, cx| this.remove_group(ge, cx))));
            head = head.when(!actors.is_empty(), |d| d.child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().font_weight(gpui::FontWeight::NORMAL).text_color(theme.muted_foreground).child(actors))).child(ops);
            names = names.child(head);
            lanes = lanes.child(div().h(px(24.0)).bg(theme.background.opacity(0.5)));
            if self.matinee.add_track_for == Some(ge) {
                let (ci, me) = (self.matinee.track_class, cx.entity().downgrade());
                let class_menu = Button::new(("mt-track-class", gi)).xsmall().label(TRACK_CLASSES[ci].1).dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for (k, c) in TRACK_CLASSES.iter().enumerate().filter(|(_, c)| !c.3 || director) {
                        let me = me.clone();
                        menu = menu.item(PopupMenuItem::new(c.1).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| {
                                this.matinee.track_class = k;
                                cx.notify();
                            });
                        }));
                    }
                    menu
                });
                let spec = TRACK_CLASSES[ci];
                let prop = self.matinee.track_property.clone();
                names = names.child(
                    h_flex()
                        .h(px(24.0))
                        .px_2()
                        .gap_1()
                        .bg(theme.background)
                        .child(class_menu)
                        .when(!spec.2.is_empty(), |d| d.child(self.field("mat:trackprop", if prop.is_empty() { spec.2[0].to_string() } else { prop.clone() }, 90.0, cx)))
                        .child(Button::new(("mt-track-add", gi)).xsmall().primary().label("Add").on_click(cx.listener(move |this, _, _, cx| {
                            this.matinee.add_track_for = None;
                            let spec = TRACK_CLASSES[this.matinee.track_class];
                            let p = (!spec.2.is_empty()).then(|| if this.matinee.track_property.trim().is_empty() { spec.2[0].to_string() } else { this.matinee.track_property.trim().to_string() });
                            this.add_track(ge, spec.0.to_string(), p, cx);
                        }))),
                );
                lanes = lanes.child(div().h(px(24.0)));
            }
            for (ti, tr) in arr(&g["tracks"]).iter().enumerate() {
                let te = tr["export"].as_u64().unwrap_or(0) as usize;
                let label = tr["moveAxis"].as_str().map(|a| a.trim_start_matches("axis_").to_string()).or_else(|| tr["property"].as_str().map(str::to_string)).unwrap_or_else(|| tr["title"].as_str().unwrap_or("").to_string());
                for (ci, c) in arr(&tr["curves"]).iter().enumerate() {
                    let path = c["path"].as_str().unwrap_or("").to_string();
                    let gid = format!("mtt{gi}-{ti}-{ci}");
                    let mut row = h_flex()
                        .id(gpui::SharedString::from(gid.clone()))
                        .group(gid.clone())
                        .h(px(24.0))
                        .px_2()
                        .when(!tr["parent"].is_null(), |d| d.pl_5())
                        .gap_1()
                        .when(tr["disabled"] == true, |d| d.opacity(0.4))
                        .when(self.matinee.track == Some(te), |d| d.bg(theme.primary.opacity(0.1)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.matinee.track = Some(te);
                            this.matinee.group = Some(ge);
                            this.matinee.key = None;
                            cx.notify();
                        }));
                    row = if ci == 0 {
                        row.child(Checkbox::new(("mt-on", te)).checked(tr["disabled"] != true).tooltip("Enabled (bDisableTrack)").on_click(cx.listener(move |this, v: &bool, _, cx| this.matinee_edit("matinee.set", json!({ "object": te, "values": { "bDisableTrack": !*v } }), None, cx))))
                    } else {
                        row.child(div().w(px(14.0)))
                    };
                    let p1 = path.clone();
                    let p2 = path.clone();
                    let mut ops = h_flex().ml_auto().gap_0p5().invisible().group_hover(gid.clone(), |s| s.visible());
                    if c["kind"] == "curve" {
                        let on = self.matinee.curve.as_ref() == Some(&(te, path.clone()));
                        ops = ops.child(Button::new(("mt-curve", te * 16 + ci)).xsmall().ghost().icon(Icon::new(IconName::Activity)).selected(on).tooltip("Curve editor").on_click(cx.listener(move |this, _, _, cx| {
                            this.matinee.curve = Some((te, p1.clone()));
                            this.matinee.curve_view.fit_for = None;
                            cx.notify();
                        })));
                    }
                    let mv = is_move(tr);
                    ops = ops.child(Button::new(("mt-k", te * 16 + ci)).xsmall().ghost().label("K").tooltip(if mv { "Add a key at the playhead (K) from where the actor is" } else { "Add a key at the playhead (K)" }).on_click(cx.listener(move |this, _, _, cx| {
                        this.matinee.track = Some(te);
                        if ci == 0 {
                            this.key_at_playhead(te, cx);
                        } else {
                            let t = (this.matinee.time * 1000.0).round() / 1000.0;
                            this.add_key(te, p2.clone(), t, json!({}), cx);
                        }
                    })));
                    if ci == 0 {
                        ops = ops.child(Button::new(("mt-rm-track", te)).xsmall().ghost().icon(Icon::new(IconName::Trash)).tooltip("Remove the track").on_click(cx.listener(move |this, _, _, cx| this.remove_track(te, cx))));
                    }
                    names = names.child(row.child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().child(label.clone())).child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(theme.muted_foreground).child(c["name"].as_str().unwrap_or("").to_string())).child(ops));
                    // The lane: one diamond per key.
                    let mut lane = div().relative().h(px(24.0)).border_b_1().border_color(theme.border.opacity(0.5));
                    for (ki, k) in arr(&c["keys"]).iter().enumerate() {
                        let active = self.matinee.key.as_ref() == Some(&KeyRef { track: te, curve: path.clone(), key: ki });
                        let mut t = k["time"].as_f64().unwrap_or(0.0);
                        if let Some(LaneDrag::Key { r, time, moved: true, .. }) = &self.matinee.drag {
                            if r.track == te && r.curve == path && r.key == ki {
                                t = *time;
                            }
                        }
                        let x = x_of(t);
                        let color = if active { theme.warning } else if c["kind"] == "curve" { theme.primary } else { theme.info };
                        lane = lane.child(div().absolute().top(px(6.0)).left(px(x - 6.0)).size(px(12.0)).rounded_sm().bg(color.opacity(0.8)).border_1().border_color(color));
                        if let Some(s) = k["value"].as_str().filter(|_| c["kind"] == "keys") {
                            let l = s.to_ascii_lowercase();
                            let short = if l.starts_with("etta_") || l.starts_with("evta_") { s[5..].to_string() } else { s.to_string() };
                            lane = lane.child(div().absolute().top(px(4.0)).left(px(x + 8.0)).max_w(px(120.0)).overflow_hidden().text_ellipsis().whitespace_nowrap().font_family("Menlo").text_size(px(9.0)).opacity(0.7).child(short));
                        }
                    }
                    lanes = lanes.child(lane);
                }
            }
        }
        // Ruler, section and playhead over the lanes.
        let ruler_ticks = {
            let step = if len > 20.0 { 5.0 } else if len > 5.0 { 1.0 } else if len > 1.0 { 0.25 } else { 0.05 };
            (0..=((len / step).floor() as usize)).map(|i| i as f64 * step).collect::<Vec<_>>()
        };
        let mut ruler = div().absolute().top_0().left_0().right_0().h(px(24.0)).bg(theme.background.opacity(0.3)).cursor(gpui::CursorStyle::ResizeLeftRight);
        let sec = nums(&tl["section"]);
        if sec.len() == 2 && sec[1] > sec[0] {
            ruler = ruler.child(div().absolute().top_0().h_full().left(px(x_of(sec[0]))).w(px(x_of(sec[1]) - x_of(sec[0]))).bg(theme.info.opacity(0.15)));
        }
        for t in ruler_ticks {
            ruler = ruler.child(div().absolute().top_0().h_full().left(px(x_of(t))).border_l_1().border_color(theme.foreground.opacity(0.1)).pl_0p5().font_family("Menlo").text_size(px(9.0)).opacity(0.6).child(format!("{}", (t * 100.0).round() / 100.0)));
        }
        let head = div().absolute().top_0().bottom_0().left(px(x_of(self.matinee.time))).w(px(1.0)).bg(theme.warning);
        let lane_bounds = self.matinee.lane.clone();
        let measure = canvas(move |b, _, _| lane_bounds.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full();
        let lane_area = div()
            .id("mt-lanes")
            .relative()
            .flex_1()
            .min_w_0()
            .child(measure)
            .child(lanes)
            .child(ruler)
            .child(head)
            .child(crate::driver::probe("mt-lanes"))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e, _, cx| this.lane_down(e, cx)))
            .on_mouse_move(cx.listener(|this, e, _, cx| this.lane_move(e, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.lane_up(cx)));
        col = col.child(div().id("mt-body").flex_1().min_h_0().overflow_y_scroll().child(h_flex().items_start().child(names).child(lane_area)));
        if self.matinee.curve.is_some() {
            col = col.child(self.render_curve(window, cx));
        }
        col = col.child(self.render_matinee_strip(&tl, cx));
        col.into_any_element()
    }

    /// The selected key, or track, under the timeline.
    fn render_matinee_strip(&mut self, tl: &J, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut strip = h_flex().min_h(px(30.0)).flex_wrap().px_2().py_1().gap_1().border_t_1().border_color(theme.border);
        let sel = self.matinee.key.clone().and_then(|r| {
            let t = self.matinee.track(r.track)?.clone();
            let c = self.matinee.curve(r.track, &r.curve)?.clone();
            let k = arr(&c["keys"]).get(r.key)?.clone();
            Some((r, t, c, k))
        });
        let label = |t: &J| t["moveAxis"].as_str().map(|a| a.trim_start_matches("axis_").to_string()).or_else(|| t["property"].as_str().map(str::to_string)).unwrap_or_else(|| t["title"].as_str().unwrap_or("").to_string());
        if let Some((r, t, c, k)) = sel {
            strip = strip.child(div().text_color(theme.muted_foreground).child(format!("{} · {} key {}", label(&t), c["name"].as_str().unwrap_or(""), r.key))).child(h_flex().gap_1().child("t").child(self.field("mat:time", fmt(k["time"].as_f64().unwrap_or(0.0)), 60.0, cx)));
            if c["kind"] == "curve" {
                for (i, v) in nums(&k["value"]).iter().enumerate() {
                    strip = strip.child(self.field(format!("mat:value:{i}"), fmt(*v), 74.0, cx));
                }
                let mode = k["mode"].as_str().unwrap_or("CIM_Linear").to_string();
                let me = cx.entity().downgrade();
                let r2 = r.clone();
                strip = strip.child(Button::new("mt-mode").xsmall().label(MODES.iter().find(|m| m.0.eq_ignore_ascii_case(&mode)).map(|m| m.1).unwrap_or("Linear")).tooltip("Interp mode (a move key's mode applies to its position and rotation)").dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for (id, l) in MODES {
                        let (me, r) = (me.clone(), r2.clone());
                        menu = menu.item(PopupMenuItem::new(*l).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.set_key(r.clone(), json!({ "mode": id }), cx));
                        }));
                    }
                    menu
                }));
                if !mode.eq_ignore_ascii_case("CIM_Linear") && !mode.eq_ignore_ascii_case("CIM_Constant") {
                    strip = strip.child(div().text_color(theme.muted_foreground).child("in"));
                    for (i, v) in nums(&k["arrive"]).iter().enumerate() {
                        strip = strip.child(self.field(format!("mat:arrive:{i}"), fmt(*v), 60.0, cx));
                    }
                    strip = strip.child(div().text_color(theme.muted_foreground).child("out"));
                    for (i, v) in nums(&k["leave"]).iter().enumerate() {
                        strip = strip.child(self.field(format!("mat:leave:{i}"), fmt(*v), 60.0, cx));
                    }
                    let r3 = r.clone();
                    strip = strip.child(Button::new("mt-auto").xsmall().ghost().label("Auto").tooltip("Auto clamped tangents").on_click(cx.listener(move |this, _, _, cx| this.set_key(r3.clone(), json!({ "mode": "CIM_CurveAutoClamped" }), cx))));
                }
            } else {
                for (name, v) in k["fields"].as_object().into_iter().flatten() {
                    let text = if let Some(p) = v.get("path").and_then(|p| p.as_str()) { p.to_string() } else if v.is_null() { "None".into() } else { v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()) };
                    strip = strip.child(h_flex().gap_1().child(div().text_color(theme.muted_foreground).child(name.clone())).child(self.field(format!("mat:field:{name}"), text, 130.0, cx)));
                }
            }
            let r4 = r.clone();
            strip = strip.child(div().flex_1()).child(Button::new("mt-rm-key").xsmall().ghost().icon(Icon::new(IconName::Trash)).tooltip("Remove the key (Delete)").on_click(cx.listener(move |this, _, _, cx| this.remove_key(r4.clone(), cx))));
        } else if let Some(t) = self.matinee.track.and_then(|t| self.matinee.track(t)).cloned() {
            let te = t["export"].as_u64().unwrap_or(0);
            strip = strip.child(div().text_color(theme.muted_foreground).child(t["class"].as_str().unwrap_or("").to_string())).child(h_flex().gap_1().child("title").child(self.field("mat:title", t["title"].as_str().unwrap_or("").to_string(), 110.0, cx)));
            let menu = |id: &'static str, label: String, tip: &'static str, items: Vec<(String, J)>, prop: &'static str, me: gpui::WeakEntity<MapEditor>| {
                Button::new(id).xsmall().label(label).tooltip(tip).dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for (l, v) in &items {
                        let (me, v) = (me.clone(), v.clone());
                        menu = menu.item(PopupMenuItem::new(l.clone()).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.matinee_edit("matinee.set", json!({ "object": te, "values": { prop: v.clone() } }), None, cx));
                        }));
                    }
                    menu
                })
            };
            if let Some(frame) = t["moveFrame"].as_str() {
                let me = cx.entity().downgrade();
                strip = strip.child(menu("mt-frame", if frame.eq_ignore_ascii_case("imf_world") { "World".into() } else { "Relative to initial".into() }, "MoveFrame: keys in world space, or relative to where the actor starts", vec![("World".into(), json!("imf_world")), ("Relative to initial".into(), json!("imf_relativetoinitial"))], "MoveFrame", me.clone()));
                let rot = t["rotMode"].as_str().unwrap_or("imr_keyframed").to_ascii_lowercase();
                strip = strip.child(menu("mt-rot", match rot.as_str() { "imr_lookatgroup" => "Look at group", "imr_ignore" => "Ignore", _ => "Keyframed" }.into(), "RotMode", vec![("Keyframed".into(), json!("imr_keyframed")), ("Look at group".into(), json!("imr_lookatgroup")), ("Ignore".into(), json!("imr_ignore"))], "RotMode", me.clone()));
                if rot == "imr_lookatgroup" {
                    let mut items = vec![("(none)".to_string(), json!("''"))];
                    items.extend(arr(&tl["groups"]).iter().filter(|g| g["director"] != true).filter_map(|g| g["name"].as_str()).map(|n| (n.to_string(), json!(format!("'{n}'")))));
                    strip = strip.child(menu("mt-lookat", t["lookAtGroup"].as_str().filter(|g| !g.is_empty()).unwrap_or("(none)").to_string(), "LookAtGroupName", items, "LookAtGroupName", me.clone()));
                }
                strip = strip.child(Checkbox::new("mt-raw").label("raw initial").checked(t["rawInitial"] == true).tooltip("bUseRawActorTMforRelativeToInitial").on_click(cx.listener(move |this, v: &bool, _, cx| this.matinee_edit("matinee.set", json!({ "object": te, "values": { "bUseRawActorTMforRelativeToInitial": *v } }), None, cx))));
            }
            if let Some(p) = t["property"].as_str() {
                strip = strip.child(h_flex().gap_1().child("property").child(self.field("mat:prop", p.to_string(), 110.0, cx)));
            }
            strip = strip.child(div().flex_1()).child(div().text_color(theme.muted_foreground).child(if t["moveFrame"].is_string() { "K: key at the playhead · moving the actor with the gizmo keys it" } else { "K: key at the playhead" }));
        } else {
            strip = strip.child(div().text_color(theme.muted_foreground).child("click a key to edit it · double-click a row to add one · K adds one at the playhead on the selected track · in this view, moving a driven actor keys its movement"));
        }
        strip.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_move_track_puts_the_actor_on_its_keys() {
        let group = json!({ "tracks": [] });
        let mv = json!({ "export": 5, "curves": [
            { "name": "PosTrack", "kind": "curve", "keys": [ { "time": 0.0, "value": [0.0, 0.0, 0.0], "mode": "CIM_Linear" }, { "time": 2.0, "value": [200.0, 0.0, 100.0], "mode": "CIM_Linear" } ] },
            { "name": "EulerTrack", "kind": "curve", "keys": [ { "time": 0.0, "value": [0.0, 0.0, 0.0], "mode": "CIM_Linear" }, { "time": 2.0, "value": [0.0, 0.0, 0.0], "mode": "CIM_Linear" } ] }
        ] });
        let c = move_curves(&group, &mv);
        let m = move_pose(&c, 1.0, &Mat4::IDENTITY, false, false, 0.0);
        // Unreal (100, 0, 50) is view (1, 0.5, 0).
        let p = m.w_axis.truncate();
        assert!((p - Vec3::new(1.0, 0.5, 0.0)).length() < 1e-4, "{p}");
        // The key that puts it back where it is drawn.
        let (pos, _) = key_from_pose(&c, &m, &Mat4::IDENTITY, false, false, [0.0; 3]);
        assert!((pos[0] - 100.0).abs() < 1e-3 && (pos[2] - 50.0).abs() < 1e-3, "{pos:?}");
    }
}
