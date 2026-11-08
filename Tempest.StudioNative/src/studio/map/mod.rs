//! The Map view (⌘5): a persistent map and the sublevels it streams, drawn
//! in 3D (`viewport.rs` over `crate::map_render`), with the map picker and
//! outliner on the left (`side.rs`), the selected actor's inspector on the
//! right (`panel.rs`), and the level's Kismet graph (`kismet.rs`) or a
//! Matinee timeline (`matinee.rs`) in place of / under the 3D view.
//! Moving an actor writes its Location/Rotation/DrawScale3D (or a
//! collection mesh's component transform) through `map.setTransform`, an
//! unsaved edit of that level's package like any other; actors are added
//! (`map.addActor`, `palette.rs`) and deleted (`map.removeActor`) the same
//! way. Collision (`collision.rs`), light baking (`bake.rs`), the Havok nav
//! mesh (`navmesh.rs`) and the export (`palette.rs`) are the toolbar's.
//! The Electron Studio's map view at parity; the MCP tool `studio_map` and
//! the driver's `map …` steps are in `tool.rs`.

mod bake;
mod collision;
mod curve;
mod kismet;
mod matinee;
mod navmesh;
mod palette;
mod panel;
mod scene;
mod side;
pub(crate) mod tool;
mod viewport;

use std::collections::{HashMap, HashSet, VecDeque};

use glam::Mat4;
use gpui::{div, prelude::*, px, AnyElement, Context, Entity, FocusHandle, Subscription, WeakEntity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Selectable, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::props_view::{PropEditor, PropEvent};
use super::{file_name, Place, Studio, Tone};
use crate::backend::Backend;
use crate::map_render::Shading;

/// Categories of actors the view can hide, with their labels and colours.
pub const CATEGORIES: &[(&str, &str, u32)] = &[
    ("mesh", "Meshes", 0xa6adc8),
    ("playerstart", "Player starts", 0x89b4fa),
    ("spawn", "Spawn points", 0x94e2d5),
    ("botfactory", "Factories", 0xfab387),
    ("gameplay", "Gameplay", 0xf9e2af),
    ("trigger", "Triggers", 0xf5c2e7),
    ("blocking", "Blocking volumes", 0xf38ba8),
    ("volume", "Other volumes", 0xcba6f7),
    ("light", "Lights", 0xf9e2af),
    ("sound", "Sounds", 0xa6e3a1),
    ("emitter", "Emitters", 0xeba0ac),
    ("navigation", "Navigation", 0x74c7ec),
    ("info", "Level info", 0x7f849c),
    ("other", "Other", 0xbac2de),
];

/// Team colours of player starts (TeamIndex).
pub const TEAM_COLORS: [u32; 4] = [0x89b4fa, 0xf38ba8, 0xa6e3a1, 0xf9e2af];

pub fn category_color(id: &str) -> u32 {
    CATEGORIES.iter().find(|c| c.0 == id).map(|c| c.2).unwrap_or(0xbac2de)
}

pub fn hsla(hex: u32) -> gpui::Hsla {
    gpui::rgb(hex).into()
}

/// A map the picker lists (`map.list`).
#[derive(Clone)]
pub struct MapEntry {
    pub path: String,
    pub name: String,
    pub persistent: bool,
    pub kismet: bool,
}

/// An actor of a level (`map.scene`'s `MapActor`).
#[derive(Clone)]
pub struct Actor {
    pub export: usize,
    pub name: String,
    pub class: String,
    pub category: String,
    /// The world matrix, view space (column-major, three.js' `fromArray`).
    pub matrix: Mat4,
    pub location: Option<[f64; 3]>,
    pub rotation: Option<[f64; 3]>,
    pub draw_scale: Option<f64>,
    pub draw_scale3d: Option<[f64; 3]>,
    /// A mesh a StaticMeshCollectionActor gathered: moving it writes its component (`parent` is the collection).
    pub component: bool,
    pub parent: Option<usize>,
    pub parts: Vec<J>,
    pub info: serde_json::Map<String, J>,
    pub can_base_on: bool,
}

impl Actor {
    fn from_json(v: &J) -> Actor {
        let v3 = |k: &str| v[k].as_array().filter(|a| a.len() == 3).map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0)]);
        Actor {
            export: v["export"].as_u64().unwrap_or(0) as usize,
            name: v["name"].as_str().unwrap_or_default().to_string(),
            class: v["class"].as_str().unwrap_or_default().to_string(),
            category: v["category"].as_str().unwrap_or("other").to_string(),
            matrix: mat(&v["matrix"]),
            location: v3("location"),
            rotation: v3("rotation"),
            draw_scale: v["drawScale"].as_f64(),
            draw_scale3d: v3("drawScale3D"),
            component: v["edit"] == "component",
            parent: v["parent"].as_u64().map(|p| p as usize),
            parts: v["parts"].as_array().cloned().unwrap_or_default(),
            info: v["info"].as_object().cloned().unwrap_or_default(),
            can_base_on: v["canBaseOn"].as_bool() != Some(false),
        }
    }

    /// The export moving or flagging it writes: the actor, or a collection mesh's collection.
    pub fn owner(&self) -> usize {
        if self.component {
            self.parent.unwrap_or(self.export)
        } else {
            self.export
        }
    }

    pub fn team(&self) -> Option<usize> {
        self.info.get("TeamIndex").and_then(|t| t.as_f64().or_else(|| t.as_str().and_then(|s| s.parse().ok()))).map(|t| t as usize)
    }

    pub fn color(&self) -> u32 {
        match (self.category.as_str(), self.team()) {
            ("playerstart", Some(t)) if t < TEAM_COLORS.len() => TEAM_COLORS[t],
            _ => category_color(&self.category),
        }
    }

    /// A short JSON summary (MCP).
    pub fn brief(&self, level: &str) -> J {
        json!({ "level": level, "export": self.export, "name": self.name, "class": self.class, "category": self.category, "edit": if self.component { "component" } else { "actor" }, "location": self.location, "rotation": self.rotation, "info": self.info })
    }
}

/// A 16-number view-space matrix.
pub fn mat(v: &J) -> Mat4 {
    let a: Vec<f32> = v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
    if a.len() == 16 {
        Mat4::from_cols_slice(&a)
    } else {
        Mat4::IDENTITY
    }
}

pub fn mat_json(m: &Mat4) -> J {
    json!(m.to_cols_array().iter().map(|x| *x as f64).collect::<Vec<_>>())
}

#[derive(Clone)]
pub struct Level {
    pub path: String,
    pub name: String,
    pub actors: Vec<Actor>,
}

impl Level {
    fn from_json(v: &J) -> Level {
        Level { path: v["path"].as_str().unwrap_or_default().to_string(), name: v["name"].as_str().unwrap_or_default().to_string(), actors: v["actors"].as_array().into_iter().flatten().map(Actor::from_json).collect() }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Scene,
    Kismet,
    Matinee,
}

impl Mode {
    pub fn key(self) -> &'static str {
        match self {
            Mode::Scene => "scene",
            Mode::Kismet => "kismet",
            Mode::Matinee => "matinee",
        }
    }
}

/// The gizmo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Translate,
    Rotate,
    Scale,
}

impl Tool {
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Translate => "translate",
            Tool::Rotate => "rotate",
            Tool::Scale => "scale",
        }
    }
}

/// A question shown over the view (deleting an actor or a node).
pub struct Confirm {
    pub title: String,
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Clone)]
pub enum ConfirmAction {
    RemoveActor,
    RemoveNode(usize),
}

pub type Sel = (String, usize);

/// What to do once a map is read (Back/Forward going on to Kismet or a Matinee).
pub type AfterOpen = Option<Box<dyn FnOnce(&mut MapEditor, &mut Context<MapEditor>)>>;

pub struct MapEditor {
    pub(crate) backend: Backend,
    studio: WeakEntity<Studio>,
    pub(crate) list: Option<Vec<MapEntry>>,
    list_loading: bool,
    persistent_only: bool,
    map_filter: Entity<InputState>,
    pub(crate) path: Option<String>,
    pub(crate) levels: Vec<Level>,
    missing: Vec<String>,
    loading: bool,
    error: Option<String>,
    pub(crate) selected: Option<Sel>,
    props: Entity<PropEditor>,
    pub(crate) mode: Mode,
    pub(crate) shading: Shading,
    pub(crate) hidden_categories: HashSet<String>,
    pub(crate) hidden_levels: HashSet<String>,
    rooftops_only: bool,
    filter: Entity<InputState>,
    pub(crate) heavy: bool,
    pub(crate) xray: bool,
    pub(crate) tool: Tool,
    /// Edited levels in order, for undo.
    edits: Vec<String>,
    /// The map's levels with unsaved edits (the Studio's dirty set, kept here so
    /// the map never reads the Studio while it renders; it tells us of saves).
    pub(crate) dirty: HashSet<String>,
    /// Levels with unsaved edits made here (the Studio's dirty set is the truth).
    pub(crate) view: viewport::View3d,
    /// Scene nodes of layer 0, by (level index, actor index), and back.
    nodes: Vec<(usize, usize)>,
    node_of: HashMap<Sel, usize>,
    /// `mesh.preview` requests waiting, by mesh key.
    mesh_queue: VecDeque<(String, J)>,
    mesh_running: usize,
    pub(crate) confirm: Option<Confirm>,
    /// The value being typed in place (`field`): its key and input.
    field: Option<(String, Entity<InputState>, Subscription)>,
    /// A Kismet node added at a canvas point, placed there once the graph names it.
    pending_node_at: Option<(f32, f32)>,
    palette_sub: Option<Subscription>,
    node_props_sub: Option<Subscription>,
    /// MCP: where an actor being added goes (Unreal units), and what the last add and export said.
    pending_location: Option<[f64; 3]>,
    pending_parent: Option<usize>,
    last_added_warnings: Vec<String>,
    last_export: Option<J>,
    /// The last errors and warnings shown.
    pub(crate) errors: Vec<String>,
    pub(crate) kismet: kismet::Kismet,
    pub(crate) matinee: matinee::Matinee,
    pub(crate) collision: collision::Collision,
    pub(crate) bake: bake::Bake,
    pub(crate) nav: navmesh::Nav,
    pub(crate) palette: palette::ActorPalette,
    pub(crate) export: palette::Export,
    /// Backend progress (`map.progress`, `light.progress`).
    progress: Option<(String, u64, u64)>,
    pub(crate) focus: FocusHandle,
    /// Side bar and inspector widths (kept in the settings).
    pub(crate) panes: [f32; 2],
    /// ⌘B / ⌘J and the compact layout (the window's).
    shared: super::layout::Shared,
    _subs: Vec<Subscription>,
}

impl MapEditor {
    pub fn new(backend: Backend, studio: WeakEntity<Studio>, hidden: Vec<String>, panes: [f32; 2], shared: super::layout::Shared, window: &mut Window, cx: &mut Context<Self>) -> MapEditor {
        let map_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter maps…"));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter actors…"));
        let props = cx.new(|cx| PropEditor::new(backend.clone(), window, cx));
        let subs = vec![
            cx.subscribe(&map_filter, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&filter, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&props, |this, _, e: &PropEvent, cx| match e {
                PropEvent::Changed => this.prop_changed(cx),
                PropEvent::Error(e) => this.toast(Tone::Error, e.clone(), cx),
            }),
        ];
        let mut m = MapEditor {
            kismet: kismet::Kismet::new(window, cx),
            matinee: matinee::Matinee::new(window, cx),
            collision: collision::Collision::new(window, cx),
            bake: bake::Bake::default(),
            nav: navmesh::Nav::new(window, cx),
            palette: palette::ActorPalette::new(window, cx),
            export: palette::Export::default(),
            backend,
            studio,
            list: None,
            list_loading: false,
            persistent_only: true,
            map_filter,
            path: None,
            levels: Vec::new(),
            missing: Vec::new(),
            loading: false,
            error: None,
            selected: None,
            props,
            mode: Mode::Scene,
            shading: Shading::Lit,
            hidden_categories: hidden.into_iter().collect(),
            hidden_levels: HashSet::new(),
            rooftops_only: false,
            filter,
            heavy: false,
            xray: false,
            tool: Tool::Translate,
            edits: Vec::new(),
            dirty: HashSet::new(),
            view: viewport::View3d::new(),
            nodes: Vec::new(),
            node_of: HashMap::new(),
            mesh_queue: VecDeque::new(),
            mesh_running: 0,
            confirm: None,
            field: None,
            pending_node_at: None,
            palette_sub: None,
            node_props_sub: None,
            pending_location: None,
            pending_parent: None,
            last_added_warnings: Vec::new(),
            last_export: None,
            errors: Vec::new(),
            progress: None,
            focus: cx.focus_handle(),
            panes,
            shared,
            _subs: subs,
        };
        m.load_list(cx);
        m
    }

    // ----- the Studio ------------------------------------------------------------

    /// A backend call; arguments left out are not sent as nulls (the backend reads a
    /// present `brush`, `mesh` or `parent` as asked for).
    pub(crate) fn call(&self, method: &str, mut params: J) -> impl std::future::Future<Output = Result<crate::backend::Reply, String>> + use<> {
        if let Some(o) = params.as_object_mut() {
            o.retain(|_, v| !v.is_null());
        }
        self.backend.call(method, params)
    }

    /// Update the Studio after this update (it may be the one updating us).
    fn with_studio(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut Studio, &mut Context<Studio>) + 'static) {
        let studio = self.studio.clone();
        cx.defer(move |cx| {
            let _ = studio.update(cx, f);
        });
    }

    pub(crate) fn toast(&mut self, tone: Tone, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        // Kept for the MCP state: an action's failure only shows as a toast otherwise.
        if matches!(tone, Tone::Error | Tone::Warning) {
            self.errors.push(text.clone());
            let n = self.errors.len();
            self.errors.drain(..n.saturating_sub(5));
        }
        self.with_studio(cx, move |s, cx| s.toast(tone, text, cx));
    }

    fn status(&self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        self.with_studio(cx, move |s, cx| s.set_status(text, cx));
    }

    /// An edit of a level: undo takes it back, the Studio lists the package as unsaved.
    pub(crate) fn note_edit(&mut self, level: &str, cx: &mut Context<Self>) {
        self.edits.push(level.to_string());
        self.dirty.insert(level.to_string());
        let level = level.to_string();
        self.with_studio(cx, move |s, cx| {
            s.dirty.insert(level);
            cx.notify();
        });
    }

    /// The map's level packages with unsaved edits.
    pub(crate) fn dirty_levels(&self, _cx: &gpui::App) -> Vec<String> {
        self.levels.iter().map(|l| l.path.clone()).filter(|p| self.dirty.contains(p)).collect()
    }

    fn is_dirty(&self, path: &str, _cx: &gpui::App) -> bool {
        self.dirty.contains(path)
    }

    pub(crate) fn backend_event(&mut self, name: &str, done: u64, total: u64, cx: &mut Context<Self>) {
        self.progress = (done < total).then(|| (name.to_string(), done, total));
        cx.notify();
    }

    // ----- places ---------------------------------------------------------------------

    pub fn place(&self) -> Option<Place> {
        let path = self.path.clone()?;
        Some(Place::Map {
            path,
            level: self.selected.as_ref().map(|s| s.0.clone()),
            export: self.selected.as_ref().map(|s| s.1),
            view: self.mode.key().to_string(),
            sequence: if self.mode == Mode::Kismet { self.kismet.sequence } else { None },
            matinee: if self.mode == Mode::Matinee { self.matinee.at.clone() } else { None },
        })
    }

    pub(crate) fn record(&self, cx: &mut Context<Self>) {
        if let Some(p) = self.place() {
            self.with_studio(cx, move |s, _| s.record(p));
        }
    }

    /// Back/Forward and tabs: show this place.
    pub fn visit(&mut self, place: Place, cx: &mut Context<Self>) {
        let Place::Map { path, level, export, view, sequence, matinee } = place else { return };
        let sel = level.zip(export);
        let then = match view.as_str() {
            "kismet" => Some(Mode::Kismet),
            "matinee" => Some(Mode::Matinee),
            _ => None,
        };
        let after = move |this: &mut MapEditor, cx: &mut Context<MapEditor>| match then {
            Some(Mode::Kismet) => {
                let level = this.selected.as_ref().map(|s| s.0.clone()).or(this.path.clone()).unwrap_or_default();
                this.open_kismet(this.kismet.level.clone().unwrap_or(level), sequence, false, cx);
            }
            Some(Mode::Matinee) => {
                if let Some((l, i)) = matinee.clone() {
                    this.open_matinee(l, i, false, cx);
                }
            }
            _ => {
                this.mode = Mode::Scene;
                cx.notify();
            }
        };
        if self.path.as_deref() == Some(path.as_str()) && !self.levels.is_empty() {
            if sel != self.selected {
                self.select(sel, false, cx);
            }
            after(self, cx);
        } else {
            self.open(path, sel, false, Some(Box::new(after)), cx);
        }
    }

    // ----- maps -----------------------------------------------------------------------

    pub(crate) fn load_list(&mut self, cx: &mut Context<Self>) {
        if self.list_loading {
            return;
        }
        self.list_loading = true;
        let call = self.call("map.list", json!({}));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.list_loading = false;
                match r {
                    Ok(r) => {
                        let mut maps: Vec<MapEntry> = r.json["maps"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|m| MapEntry { path: m["path"].as_str().unwrap_or_default().to_string(), name: m["name"].as_str().unwrap_or_default().to_string(), persistent: m["persistent"].as_bool() == Some(true), kismet: m["kismet"].as_bool() == Some(true) })
                            .collect();
                        maps.sort_by_key(|m| m.name.to_ascii_lowercase());
                        this.list = Some(maps);
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not list the maps: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Open a map and its sublevels; `then` runs once they are read.
    pub(crate) fn open(&mut self, path: String, select: Option<Sel>, record: bool, then: AfterOpen, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.path = Some(path.clone());
        self.levels.clear();
        self.selected = None;
        self.props.update(cx, |p, cx| p.clear(cx));
        self.mode = Mode::Scene;
        self.kismet.level = None;
        self.kismet.reset();
        self.matinee.close();
        self.collision.data = None;
        self.nav.reset();
        self.palette.reset();
        self.edits.clear();
        let call = self.call("map.scene", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.path.as_deref() != Some(path.as_str()) {
                    return;
                }
                this.loading = false;
                match r {
                    Ok(r) => {
                        this.levels = r.json["levels"].as_array().into_iter().flatten().map(Level::from_json).collect();
                        this.missing = r.json["missing"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();
                        this.hidden_levels.clear();
                        this.rebuild(cx);
                        this.frame_all();
                        if let Some(s) = select {
                            this.select(Some(s), false, cx);
                        }
                        if record {
                            this.record(cx);
                        }
                        if let Some(f) = then {
                            f(this, cx);
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Pick another map.
    pub(crate) fn close_map(&mut self, cx: &mut Context<Self>) {
        self.path = None;
        self.levels.clear();
        self.selected = None;
        self.rebuild(cx);
        cx.notify();
    }

    /// Re-read one level after an edit or undo (the others stay as drawn).
    pub(crate) fn reload_level(&mut self, level: String, cx: &mut Context<Self>) {
        let call = self.call("map.scene", json!({ "path": level, "streamed": false }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        let Some(fresh) = r.json["levels"].get(0).map(Level::from_json) else { return };
                        if let Some(l) = this.levels.iter_mut().find(|l| l.path == level) {
                            let name = std::mem::take(&mut l.name);
                            *l = Level { name, ..fresh };
                        }
                        this.rebuild(cx);
                        if this.selected.as_ref().is_some_and(|s| s.0 == level) {
                            let (l, e) = this.selected.clone().unwrap();
                            if this.actor_at(&l, e).is_some() {
                                this.props.update(cx, |p, cx| p.load(&l, e, cx));
                            } else {
                                this.selected = None;
                                this.props.update(cx, |p, cx| p.clear(cx));
                            }
                        }
                        if this.collision.enabled {
                            this.load_collision(true, cx);
                        }
                        if this.nav.show {
                            this.load_nav(true, cx);
                        }
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not reload the level: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn level(&self, path: &str) -> Option<&Level> {
        self.levels.iter().find(|l| l.path == path)
    }

    /// A level by path or (case-insensitive) name.
    pub(crate) fn level_named(&self, name: &str) -> Option<&Level> {
        self.levels.iter().find(|l| l.path == name || l.name.eq_ignore_ascii_case(name) || file_name(&l.path).eq_ignore_ascii_case(name))
    }

    pub(crate) fn actor_at(&self, level: &str, export: usize) -> Option<&Actor> {
        self.level(level)?.actors.iter().find(|a| a.export == export)
    }

    pub(crate) fn actor(&self) -> Option<&Actor> {
        let (l, e) = self.selected.as_ref()?;
        self.actor_at(l, *e)
    }

    /// The level new actors go into: the selected actor's, else the persistent level.
    pub(crate) fn target_level(&self) -> Option<String> {
        self.selected.as_ref().map(|s| s.0.clone()).or_else(|| self.path.clone())
    }

    // ----- selection ------------------------------------------------------------------

    pub(crate) fn select(&mut self, sel: Option<Sel>, record: bool, cx: &mut Context<Self>) {
        self.selected = sel.clone();
        match &sel {
            Some((l, e)) => {
                let (l, e) = (l.clone(), *e);
                self.props.update(cx, |p, cx| p.load(&l, e, cx));
            }
            None => self.props.update(cx, |p, cx| p.clear(cx)),
        }
        self.apply_visibility();
        self.view.dirty();
        if record {
            self.record(cx);
        }
        cx.notify();
    }

    /// A property changed in the inspector: the actor may have moved.
    fn prop_changed(&mut self, cx: &mut Context<Self>) {
        if let Some((l, _)) = self.selected.clone() {
            self.note_edit(&l, cx);
            self.reload_level(l, cx);
        }
    }

    // ----- edits ----------------------------------------------------------------------

    /// An actor moved in the view (gizmo released): write it, or key its movement in the Matinee view.
    pub(crate) fn move_actor(&mut self, level: String, export: usize, matrix: Mat4, cx: &mut Context<Self>) {
        let Some(actor) = self.actor_at(&level, export).cloned() else { return };
        if self.mode == Mode::Matinee && self.capture_move(&level, &actor, matrix, cx) {
            return;
        }
        let call = self.call("map.setTransform", json!({ "path": level, "index": export, "matrix": mat_json(&matrix), "parent": if actor.component { actor.parent } else { None } }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        let v3 = |v: &J| v.as_array().filter(|a| a.len() == 3).map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0)]);
                        if let Some(a) = this.levels.iter_mut().find(|l| l.path == level).and_then(|l| l.actors.iter_mut().find(|a| a.export == export)) {
                            a.matrix = matrix;
                            if !a.component {
                                a.location = v3(&r.json["translation"]).or(a.location);
                            }
                            a.rotation = v3(&r.json["rotation"]).or(a.rotation);
                        }
                        this.note_edit(&level, cx);
                        if this.selected == Some((level.clone(), export)) {
                            this.props.update(cx, |p, cx| p.load(&level, export, cx));
                        }
                        this.status(format!("Moved {} (unsaved)", actor.name), cx);
                    }
                    Err(e) => {
                        this.toast(Tone::Error, format!("Could not move {}: {e}", actor.name), cx);
                        this.rebuild(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Set Location (Unreal units) and/or Rotation (rotator units) numbers.
    pub(crate) fn set_location(&mut self, level: String, export: usize, location: Option<[f64; 3]>, rotation: Option<[f64; 3]>, cx: &mut Context<Self>) {
        let Some(actor) = self.actor_at(&level, export).cloned() else { return };
        let call = self.call("map.setTransform", json!({ "path": level, "index": export, "location": location, "rotation": rotation, "parent": if actor.component { actor.parent } else { None } }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(_) => {
                    this.note_edit(&level, cx);
                    this.reload_level(level.clone(), cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not move {}: {e}", actor.name), cx),
            });
        })
        .detach();
    }

    /// `m_bCanBaseOn` of the selected actor (or of the collection holding the selected mesh).
    pub(crate) fn set_can_base_on(&mut self, value: bool, cx: &mut Context<Self>) {
        let (Some((level, _)), Some(actor)) = (self.selected.clone(), self.actor().cloned()) else { return };
        let call = self.call("object.set", json!({ "path": level, "index": actor.owner(), "prop": "m_bCanBaseOn", "value": if value { "True" } else { "False" } }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(_) => {
                    this.note_edit(&level, cx);
                    this.reload_level(level.clone(), cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not change m_bCanBaseOn: {e}"), cx),
            });
        })
        .detach();
    }

    /// Copy the selected actor (or collection mesh) next to it, in its level.
    pub(crate) fn duplicate(&mut self, cx: &mut Context<Self>) {
        let (Some((level, _)), Some(actor)) = (self.selected.clone(), self.actor().cloned()) else { return };
        let matrix = Mat4::from_translation(glam::Vec3::X) * actor.matrix;
        self.add_actor_call(level.clone(), json!({ "path": level, "source": { "path": level, "index": actor.export }, "parent": if actor.component { actor.parent } else { None }, "matrix": mat_json(&matrix) }), format!("Could not duplicate {}", actor.name), cx);
    }

    /// `map.addActor` into `level`, then select the new actor.
    pub(crate) fn add_actor_call(&mut self, level: String, params: J, fail: String, cx: &mut Context<Self>) {
        let call = self.call("map.addActor", params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => this.added(level.clone(), &r.json, cx),
                Err(e) => this.toast(Tone::Error, format!("{fail}: {e}"), cx),
            });
        })
        .detach();
    }

    fn added(&mut self, level: String, r: &J, cx: &mut Context<Self>) {
        self.note_edit(&level, cx);
        self.kismet.palette = None;
        let added = r["added"].as_u64().unwrap_or(0) as usize;
        let name = r["name"].as_str().unwrap_or_default().to_string();
        let warnings: Vec<String> = r["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_string)).collect();
        self.last_added_warnings = warnings.clone();
        // Selected once the level is read again.
        self.selected = Some((level.clone(), added));
        match self.pending_location.take() {
            // Not in the list yet: the location goes straight to the backend, then the level is read.
            Some(loc) => {
                let parent = self.pending_parent.take();
                let call = self.call("map.setTransform", json!({ "path": level, "index": added, "location": loc, "parent": parent }));
                let l = level.clone();
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| {
                        match r {
                            Ok(_) => this.note_edit(&l, cx),
                            Err(e) => this.toast(Tone::Error, format!("Could not place the actor: {e}"), cx),
                        }
                        this.reload_level(l, cx);
                    });
                })
                .detach();
            }
            None => self.reload_level(level.clone(), cx),
        }
        self.props.update(cx, |p, cx| p.load(&level, added, cx));
        self.record(cx);
        if warnings.is_empty() {
            self.toast(Tone::Success, format!("Added {name}"), cx);
        } else {
            self.toast(Tone::Warning, format!("Added {name}: {}", warnings.iter().take(5).cloned().collect::<Vec<_>>().join("\n")), cx);
        }
    }

    /// Ask before deleting the selected actor.
    pub(crate) fn ask_remove(&mut self, cx: &mut Context<Self>) {
        let Some(actor) = self.actor() else { return };
        let message = if actor.component {
            "This mesh is taken out of its collection and deleted (⌘Z brings it back).".to_string()
        } else {
            "The actor and its components are deleted from the level, and every reference to it (Kismet included) is cleared (⌘Z brings it back).".to_string()
        };
        self.confirm = Some(Confirm { title: format!("Delete {}?", actor.name), message, action: ConfirmAction::RemoveActor });
        cx.notify();
    }

    pub(crate) fn confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.confirm.take() else { return };
        match c.action {
            ConfirmAction::RemoveActor => self.remove_selected(cx),
            ConfirmAction::RemoveNode(n) => self.remove_node(n, cx),
        }
        cx.notify();
    }

    pub(crate) fn remove_selected(&mut self, cx: &mut Context<Self>) {
        let (Some((level, _)), Some(actor)) = (self.selected.clone(), self.actor().cloned()) else { return };
        self.remove_actor(level, actor, cx);
    }

    pub(crate) fn remove_actor(&mut self, level: String, actor: Actor, cx: &mut Context<Self>) {
        let call = self.call("map.removeActor", json!({ "path": level, "index": actor.export, "parent": if actor.component { actor.parent } else { None } }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(_) => {
                    this.note_edit(&level, cx);
                    if this.selected == Some((level.clone(), actor.export)) {
                        this.select(None, true, cx);
                    }
                    this.reload_level(level.clone(), cx);
                    if this.mode == Mode::Kismet && this.kismet.level.as_deref() == Some(level.as_str()) {
                        this.load_graph(cx);
                    }
                    this.status(format!("Deleted {} (unsaved)", actor.name), cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not delete {}: {e}", actor.name), cx),
            });
        })
        .detach();
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.edits.is_empty()
    }

    /// Take back the last map edit (the level's package undo).
    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(level) = self.edits.pop() else {
            self.status("Nothing to undo in this map", cx);
            return;
        };
        let call = self.call("package.undo", json!({ "path": level }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    let Some(label) = r.json["label"].as_str() else { return };
                    this.status(format!("Undone: {label}"), cx);
                    if r.json["dirty"].as_bool() == Some(false) {
                        this.dirty.remove(&level);
                        let l = level.clone();
                        this.with_studio(cx, move |s, cx| {
                            s.dirty.remove(&l);
                            cx.notify();
                        });
                    }
                    this.reload_level(level.clone(), cx);
                    if this.mode == Mode::Kismet && this.kismet.level.as_deref() == Some(level.as_str()) {
                        this.load_graph(cx);
                    }
                    if this.matinee.at.as_ref().is_some_and(|a| a.0 == level) {
                        this.refresh_matinee(cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("Undo failed: {e}"), cx),
            });
        })
        .detach();
    }

    /// Save the map's unsaved levels into the game folder (the Studio asks first).
    pub(crate) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let levels = self.dirty_levels(cx);
        if levels.is_empty() {
            self.status("Nothing to save in this map", cx);
            return;
        }
        let studio = self.studio.clone();
        window.defer(cx, move |window, cx| {
            let _ = studio.update(cx, |s, cx| s.save_packages(levels, window, cx));
        });
    }

    // ----- values typed in place ------------------------------------------------------

    /// A value shown as text; a click turns it into an input, Enter commits it
    /// (`commit_field` with `key`), leaving it cancels.
    pub(crate) fn field(&self, key: impl Into<String>, value: String, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let key = key.into();
        let theme = cx.theme().clone();
        if let Some((_, input, _)) = self.field.as_ref().filter(|f| f.0 == key) {
            return div().w(px(width)).child(Input::new(input).xsmall()).into_any_element();
        }
        let v = value.clone();
        div()
            .id(gpui::SharedString::from(format!("mf-{key}")))
            .w(px(width))
            .px_1()
            .rounded_sm()
            .bg(theme.secondary)
            .cursor_text()
            .font_family("Menlo")
            .text_xs()
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap()
            .child(crate::driver::probe(format!("mf-{key}")))
            .child(if value.is_empty() { "—".to_string() } else { value })
            .on_click(cx.listener(move |this, _, window, cx| this.start_field(key.clone(), v.clone(), window, cx)))
            .into_any_element()
    }

    pub(crate) fn start_field(&mut self, key: String, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(value, window, cx);
            s
        });
        input.update(cx, |s, cx| s.focus(window, cx));
        let k = key.clone();
        let sub = cx.subscribe_in(&input, window, move |this, input, e: &InputEvent, _, cx| match e {
            InputEvent::PressEnter { .. } => {
                let v = input.read(cx).value().to_string();
                this.field = None;
                this.commit_field(&k, v, cx);
                cx.notify();
            }
            InputEvent::Blur if this.field.as_ref().is_some_and(|f| f.0 == k) => {
                this.field = None;
                cx.notify();
            }
            _ => {}
        });
        self.field = Some((key, input, sub));
        cx.notify();
    }

    /// A typed value: to the part of the view its key names.
    pub(crate) fn commit_field(&mut self, key: &str, value: String, cx: &mut Context<Self>) {
        let (part, rest) = key.split_once(':').unwrap_or((key, ""));
        match part {
            "col" => self.commit_collision_field(rest, value, cx),
            "mat" => self.commit_matinee_field(rest, value, cx),
            "bake" => self.commit_bake_field(rest, value, cx),
            "nav" => self.commit_nav_field(rest, value, cx),
            "exp" => self.commit_export_field(rest, value, cx),
            _ => {}
        }
    }

    // ----- the view's shell -----------------------------------------------------------

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mode = self.mode;
        let mut bar = h_flex().min_h(px(34.0)).flex_wrap().px_2().py_1().gap_1().border_b_1().border_color(theme.border).bg(theme.secondary).text_xs();
        let tab = |id: &'static str, icon: IconName, label: &'static str, on: bool| Button::new(id).xsmall().icon(Icon::new(icon)).label(label).selected(on);
        bar = bar
            .child(tab("mv-scene", IconName::Map, "Scene", mode == Mode::Scene).on_click(cx.listener(|this, _, _, cx| {
                this.mode = Mode::Scene;
                this.record(cx);
                cx.notify();
            })))
            .child(tab("mv-kismet", IconName::Workflow, "Kismet", mode == Mode::Kismet).on_click(cx.listener(|this, _, _, cx| {
                let level = this.kismet.level.clone().or_else(|| this.selected.as_ref().map(|s| s.0.clone())).or(this.path.clone()).unwrap_or_default();
                this.open_kismet(level, None, true, cx);
            })));
        if let Some((l, i)) = self.matinee.at.clone().filter(|_| self.matinee.tl.is_some()) {
            bar = bar.child(tab("mv-matinee", IconName::Clapperboard, "Matinee", mode == Mode::Matinee).tooltip("The Matinee timeline last opened").on_click(cx.listener(move |this, _, _, cx| this.open_matinee(l.clone(), i, true, cx))));
        }
        if mode != Mode::Kismet {
            let tools = [(Tool::Select, IconName::MousePointer2, "Select only (Q)"), (Tool::Translate, IconName::Move, "Move (W)"), (Tool::Rotate, IconName::Rotate3d, "Rotate (E)"), (Tool::Scale, IconName::Scaling, "Scale (R)")];
            let mut row = h_flex().ml_2().gap_0p5();
            for (t, icon, tip) in tools {
                row = row.child(Button::new(t.key()).xsmall().icon(Icon::new(icon)).tooltip(tip).selected(self.tool == t).on_click(cx.listener(move |this, _, _, cx| {
                    this.tool = t;
                    this.view.dirty();
                    cx.notify();
                })));
            }
            let rooftops = self.rooftop_count();
            bar = bar
                .child(row)
                .child(
                    Button::new("mv-heavy")
                        .xsmall()
                        .ml_2()
                        .label(if self.heavy { format!("Heavy walls {rooftops}") } else { "Heavy walls".into() })
                        .selected(self.heavy)
                        .tooltip("Colour what can't be stood on (m_bCanBaseOn False): walls flagged like this are the heavy walls")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.heavy = !this.heavy;
                            this.apply_look();
                            cx.notify();
                        })),
                )
                .when(self.heavy, |b| {
                    b.child(gpui_component::checkbox::Checkbox::new("mv-xray").label("X-ray").checked(self.xray).on_click(cx.listener(|this, v: &bool, _, cx| {
                        this.xray = *v;
                        this.apply_look();
                        cx.notify();
                    })))
                })
                .child(self.render_collision_toolbar(cx))
                .child(Button::new("mv-nav").xsmall().ml_2().label("Nav mesh").selected(self.nav.show).tooltip("Draw the Havok nav mesh the bots walk on: faces by region or cluster, boundaries, jump links; pick and edit faces").on_click(cx.listener(|this, _, _, cx| this.toggle_nav(cx))));
            let me = cx.entity().downgrade();
            let shading = self.shading;
            bar = bar
                .child(Button::new("mv-shading").xsmall().ml_2().label(match shading {
                    Shading::Lit => "Lit",
                    Shading::Unlit => "Unlit",
                    Shading::Untextured => "No textures",
                    Shading::Wireframe => "Wireframe",
                }).dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for (s, label) in [(Shading::Lit, "Lit"), (Shading::Unlit, "Unlit"), (Shading::Untextured, "No textures"), (Shading::Wireframe, "Wireframe")] {
                        let me = me.clone();
                        menu = menu.item(PopupMenuItem::new(label).checked(s == shading).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| {
                                this.shading = s;
                                this.apply_look();
                                cx.notify();
                            });
                        }));
                    }
                    menu
                }))
                .child(div().flex_1())
                .child(Button::new("mv-add-actor").xsmall().ghost().icon(Icon::new(IconName::Plus)).label("Actor").selected(self.palette.open).tooltip("Add an actor: a copy of one any level places, where the view looks (⇧A)").on_click(cx.listener(|this, _, window, cx| this.toggle_palette(window, cx))))
                .child(Button::new("mv-remove-actor").xsmall().ghost().icon(Icon::new(IconName::Trash)).tooltip("Delete the selected actor (Delete)").disabled(self.actor().is_none()).on_click(cx.listener(|this, _, _, cx| this.ask_remove(cx))))
                .child(Button::new("mv-export").xsmall().ghost().icon(Icon::new(IconName::Download)).label("Export").tooltip("Export the map (the levels and kinds shown) as GLB or FBX").on_click(cx.listener(|this, _, _, cx| this.open_export(cx))))
                .child(Button::new("mv-bake").xsmall().ghost().icon(Icon::new(IconName::Sun)).label("Light").tooltip("Bake static light maps for edited, selected or whole levels' meshes").on_click(cx.listener(|this, _, _, cx| this.open_bake(cx))));
        } else {
            bar = bar.child(self.render_kismet_toolbar(cx));
        }
        bar = bar.child(Button::new("mv-reload").xsmall().ghost().icon(Icon::new(IconName::RefreshCw)).tooltip("Read the map again").on_click(cx.listener(|this, _, _, cx| {
            if let Some(p) = this.path.clone() {
                this.open(p, None, false, None, cx);
            }
        })));
        bar.into_any_element()
    }

    pub(crate) fn rooftop_count(&self) -> usize {
        self.levels.iter().map(|l| l.actors.iter().filter(|a| !a.can_base_on).count()).sum()
    }

    fn render_confirm(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let c = self.confirm.as_ref()?;
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.35))
                .child(
                    v_flex()
                        .w(px(420.0))
                        .p_4()
                        .gap_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(c.title.clone()))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(c.message.clone()))
                        .child(
                            h_flex()
                                .justify_end()
                                .gap_2()
                                .child(Button::new("mv-confirm-no").small().label("Cancel").on_click(cx.listener(|this, _, _, cx| {
                                    this.confirm = None;
                                    cx.notify();
                                })))
                                .child(Button::new("mv-confirm-yes").small().danger().label("Delete").on_click(cx.listener(|this, _, _, cx| this.confirmed(cx)))),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl Render for MapEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        self.pump_meshes(cx);
        let side = self.render_side(cx);
        let center: AnyElement = if self.loading {
            div().flex_1().flex().items_center().justify_center().text_sm().text_color(theme.muted_foreground).child("Reading the map and its sublevels…").into_any_element()
        } else if let Some(e) = &self.error {
            div().flex_1().flex().items_center().justify_center().p_6().text_sm().text_color(theme.danger).child(e.clone()).into_any_element()
        } else if self.path.is_none() {
            v_flex().flex_1().items_center().justify_center().gap_2().text_sm().text_color(theme.muted_foreground).child(Icon::new(IconName::Box)).child("Pick a map on the left.").into_any_element()
        } else {
            let toolbar = self.render_toolbar(cx);
            let body: AnyElement = if self.mode == Mode::Kismet {
                self.render_kismet(window, cx)
            } else {
                let viewport = self.render_viewport(window, cx);
                let overlays: Vec<AnyElement> = [self.render_palette(window, cx), self.render_nav_panel(cx)].into_iter().flatten().collect();
                let mut col = v_flex().flex_1().min_h_0().min_w_0().child(div().relative().flex_1().min_h_0().min_w_0().overflow_hidden().child(viewport).children(overlays));
                if self.mode == Mode::Matinee {
                    col = col.child(self.render_matinee(window, cx));
                }
                col.into_any_element()
            };
            v_flex().flex_1().min_w_0().size_full().overflow_hidden().child(toolbar).child(body).into_any_element()
        };
        let inspector = self.path.is_some().then(|| if self.mode == Mode::Kismet { self.render_kismet_inspector(window, cx) } else { self.render_inspector(window, cx) });
        use super::layout::{group, Part, Role};
        let mut parts = vec![Part(Role::Side, Some(self.panes[0]), side), Part(Role::Main, None, center)];
        if let Some(i) = inspector {
            parts.push(Part(Role::Second, Some(self.panes[1]), i));
        }
        // Resized panes are kept for the next run.
        let (me, studio, studio2) = (cx.entity().downgrade(), self.studio.clone(), self.studio.clone());
        let layout = group(
            "map-main",
            self.shared.get(),
            parts,
            move |sizes, cx| {
                let (me, studio) = (me.clone(), studio.clone());
                cx.defer(move |cx| {
                    let Ok(panes) = me.update(cx, |this, _| {
                        this.panes[0] = sizes[0];
                        if let Some(w) = sizes.get(1) {
                            this.panes[1] = *w;
                        }
                        this.panes
                    }) else {
                        return;
                    };
                    let _ = studio.update(cx, |s, _| {
                        if s.prefs.map_panes != panes {
                            s.prefs.map_panes = panes;
                            s.prefs.save();
                        }
                    });
                });
            },
            move |_, cx| {
                let _ = studio2.update(cx, |s, cx| s.close_overlay(cx));
            },
            cx,
        );
        let dialogs: Vec<AnyElement> = [self.render_export(cx), self.render_bake(window, cx), self.render_confirm(cx)].into_iter().flatten().collect();
        div().id("map-editor").track_focus(&self.focus).size_full().relative().child(layout).children(dialogs)
    }
}
