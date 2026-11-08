//! The Studio window: a header (Back/Forward, the game folder, the search
//! field, Undo, unsaved files, Save), an activity bar choosing the view
//! (Packages, Assembly, Mod), window tabs, the view, a status line and
//! notifications. Every action is a command (`commands.rs`): the keys, the
//! palette, the menus and the shortcut list read the same table.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{actions, div, prelude::*, px, App, Context, Entity, FocusHandle, Image, PathPromptOptions, SharedString, Subscription, UniformListScrollHandle, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::notification::Notification;
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Selectable, Sizable, WindowExt};
use gpui_kit_assets::IconName;
use serde_json::json;

mod account;
mod actions;
mod anim;
pub mod assembly;
mod audio_view;
mod commands;
mod content;
pub mod cooker;
mod dialogs;
mod inspector;
pub(crate) mod layout;
mod material;
mod mesh;
pub(crate) mod swf;
pub(crate) mod map;
mod props_view;
mod packages;
mod modmeta;
mod palette;
mod particle;
mod prefs;
mod project;
mod settings;
mod sound;
mod tabs;
mod texture;
mod updates;
mod welcome;
mod wizard;
mod mcp_tools;
mod workspace;
mod wwise;

use crate::backend::Backend;
use crate::model::{self, Inspected, PackageEntry, Tables};
pub use commands::Cmd;

const ROW: f32 = 22.0;

actions!(studio, [NewTab, CloseTab, NextTab, PrevTab, Undo, Save, Back, Forward, Up, Down, Left, Right, First, Last, Commands, GoToPackage, GoToObject, SearchAll, Shortcuts, ShowPackages, ShowAssembly, ShowMod, ShowAudio, ShowMap, Rename, Duplicate, Delete, SaveWorkspace, OpenWorkspaceFile, Settings, CopyPath, Create, ZoomIn, ZoomOut, ZoomReset, CopyRows, PasteRows, NewWindow, ToggleSide, ToggleSecond]);

/// The views of the activity bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Packages,
    Assembly,
    Audio,
    Mod,
    Map,
}

/// Where the user was: what Back/Forward and the tabs return to.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// `view`: the movie editor's view (`swf::Node::key`) when the export is a movie.
    Package { package: String, export: Option<usize>, view: Option<String> },
    Assembly { file: String, table: Option<String>, row: Option<usize> },
    Mod,
    /// The map view: a map, the selected actor, the view (scene, kismet, matinee) and what it shows.
    Map { path: String, level: Option<String>, export: Option<usize>, view: String, sequence: Option<usize>, matinee: Option<(String, usize)> },
}

impl Place {
    pub fn package(package: impl Into<String>, export: Option<usize>) -> Place {
        Place::Package { package: package.into(), export, view: None }
    }
}

#[derive(Clone, Copy)]
pub enum Tone {
    Info,
    Success,
    Warning,
    Error,
}

pub struct Studio {
    backend: Backend,
    dir: Option<String>,
    view: View,
    packages: Vec<PackageEntry>,
    /// Indices into `packages` the filter lets through.
    shown_packages: Vec<usize>,
    package_filter: Entity<InputState>,
    export_filter: Entity<InputState>,
    package_scroll: UniformListScrollHandle,
    export_scroll: UniformListScrollHandle,
    open: Option<Tables>,
    expanded: HashSet<usize>,
    /// The export tree's visible rows: (export, depth).
    rows: Vec<(usize, u16)>,
    selected_package: Option<String>,
    selected_export: Option<usize>,
    inspected: Option<Inspected>,
    preview: Option<Arc<Image>>,
    status: SharedString,
    focus: FocusHandle,
    /// The inspector's in-place editor and the property path it edits.
    value_input: Entity<InputState>,
    editing: Option<String>,
    /// Packages with unsaved edits (paths).
    dirty: HashSet<String>,
    /// The two lists take the arrow keys when focused.
    packages_focus: FocusHandle,
    exports_focus: FocusHandle,
    /// Back/Forward: places visited, and where in them the window is.
    history: Vec<Place>,
    history_at: usize,
    /// The export to select once the package being opened is loaded.
    pending_export: Option<usize>,
    palette: Option<palette::Palette>,
    tabs: Vec<tabs::Tab>,
    active_tab: usize,
    tab: inspector::Tab,
    deps: Option<(usize, model::Deps)>,
    source: Option<inspector::Source>,
    mesh: Option<std::rc::Rc<std::cell::RefCell<mesh::MeshState>>>,
    material: Option<material::MaterialState>,
    slots: Option<material::Slots>,
    sound: Option<sound::SoundState>,
    wwise: Option<wwise::WwiseState>,
    anim: Option<anim::AnimState>,
    player: crate::audio::Player,
    /// Notifications waiting for a window to be shown in.
    toasts: Vec<(Tone, String)>,
    /// Game folders opened before, newest first.
    recent: Vec<String>,
    assembly: Option<Entity<assembly::Assembly>>,
    map: Option<Entity<map::MapEditor>>,
    cooker: Option<Entity<cooker::Cooker>>,
    /// A picker or plan to show once there is a window (asynchronous answers).
    pending_pick: Option<palette::Pick>,
    pending_batch: Option<(String, serde_json::Value)>,
    pending_go: Option<Place>,
    prefs: prefs::Prefs,
    /// The view to show once there is a window (the restored session).
    pending_view: Option<View>,
    /// The open mod project, the state of its file and the edit count it last saw.
    project: Option<project::Project>,
    project_saving: project::Saving,
    project_seen: usize,
    project_changed: Option<std::time::Instant>,
    /// A workspace file put assembly edits back (closing a project discards them).
    applied_docs: bool,
    /// A package to open once the folder is (a `.upk` given at start).
    open_after: Option<String>,
    /// A file's animations to choose from for Replace: (names, the import call, the sequence, the file).
    pending_anim_pick: Option<(Vec<String>, serde_json::Value, String, String)>,
    /// A recovery file to offer: (path, how long ago).
    recovery_offer: Option<(String, String)>,
    /// The Wwise medium playing.
    ww_playing: Option<u64>,
    /// The Audio view: every .pck of the folder.
    audio: Option<audio_view::AudioBrowser>,
    /// The texture preview's mips, zoom and background.
    tex: Option<texture::TexView>,
    /// The inspector shows the engine's raw data under the properties.
    show_native: bool,
    /// The MCP server's URL and token (None: it could not start).
    pub(crate) mcp: Option<(String, String)>,
    /// ⌘B / ⌘J and the compact layout, shared with the Assembly and Map views.
    pub(crate) panes: layout::Shared,
    /// Create: the open wizard.
    pub(crate) wizard: Option<Entity<wizard::Wizard>>,
    /// The package list shows only packages with unsaved edits.
    unsaved_only: bool,
    /// The particle system preview.
    particles: Option<particle::ParticleState>,
    /// The movie editors, one per SwfMovie export (package path, export), kept while the package is open.
    movies: std::collections::HashMap<(String, usize), Entity<swf::MovieEditor>>,
    /// The package and object lists are hidden (a movie's stage takes the width).
    pub(crate) focus_mode: bool,
    _subscriptions: Vec<Subscription>,
}

impl Studio {
    /// `start`: what to open (a folder, a `.upk`, a `.twrk`); `None` for a new
    /// window (the welcome screen); `Some("")` at launch: the argument, else the last folder.
    pub fn new(backend: Backend, events: futures::channel::mpsc::UnboundedReceiver<serde_json::Value>, start: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Studio {
        let package_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter packages"));
        let export_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter objects by name or class"));
        let value_input = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&value_input, window, |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.commit_edit(window, cx),
                InputEvent::Blur => this.cancel_edit(cx),
                _ => {}
            }),
            cx.subscribe(&package_filter, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.refilter_packages(cx);
                }
            }),
            cx.subscribe(&export_filter, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.refresh_rows(cx);
                }
            }),
        ];
        // Backend events (search, bake, map progress) become the status line.
        cx.spawn(async move |this, cx| {
            use futures::StreamExt;
            let mut events = events;
            while let Some(e) = events.next().await {
                let text = match (e["event"].as_str(), e["done"].as_u64(), e["total"].as_u64()) {
                    (Some("search.progress"), Some(d), Some(t)) => format!("Indexing packages for search: {d} / {t}"),
                    (Some(name), Some(d), Some(t)) => format!("{name}: {d} / {t}"),
                    (Some(name), _, _) => name.to_string(),
                    _ => continue,
                };
                // The map view shows its exports' and bakes' progress itself.
                let name = e["event"].as_str().unwrap_or("").to_string();
                if name == "map.progress" || name == "light.progress" {
                    let (d, t) = (e["done"].as_u64().unwrap_or(0), e["total"].as_u64().unwrap_or(0));
                    let _ = this.update(cx, |this, cx| {
                        if let Some(m) = this.map.clone() {
                            m.update(cx, |m, cx| m.backend_event(&name, d, t, cx));
                        }
                    });
                }
                if this.update(cx, |this, cx| this.set_status(text, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut studio = Studio {
            backend,
            dir: None,
            view: View::Packages,
            packages: Vec::new(),
            shown_packages: Vec::new(),
            package_filter,
            export_filter,
            package_scroll: UniformListScrollHandle::new(),
            export_scroll: UniformListScrollHandle::new(),
            open: None,
            expanded: HashSet::new(),
            rows: Vec::new(),
            selected_package: None,
            selected_export: None,
            inspected: None,
            preview: None,
            status: "".into(),
            focus: cx.focus_handle(),
            value_input,
            editing: None,
            dirty: HashSet::new(),
            packages_focus: cx.focus_handle(),
            exports_focus: cx.focus_handle(),
            history: Vec::new(),
            history_at: 0,
            pending_export: None,
            palette: None,
            tabs: Vec::new(),
            active_tab: 0,
            tab: inspector::Tab::Properties,
            deps: None,
            source: None,
            mesh: None,
            material: None,
            slots: None,
            sound: None,
            wwise: None,
            anim: None,
            player: crate::audio::Player::default(),
            toasts: Vec::new(),
            recent: recent_workspaces(),
            assembly: None,
            map: None,
            cooker: None,
            pending_pick: None,
            pending_batch: None,
            pending_go: None,
            prefs: prefs::Prefs::load(),
            pending_view: None,
            unsaved_only: false,
            particles: None,
            movies: Default::default(),
            focus_mode: false,
            wizard: None,
            panes: Default::default(),
            mcp: None,
            show_native: false,
            tex: None,
            ww_playing: None,
            recovery_offer: None,
            pending_anim_pick: None,
            open_after: None,
            audio: None,
            project: None,
            project_saving: project::Saving::Saved,
            project_seen: 0,
            project_changed: None,
            applied_docs: false,
            _subscriptions: subscriptions,
        };
        Self::watch_project(cx);
        // One server, for the first window (the port is the agents' address).
        let first = WINDOWS.lock().map(|w| w.is_empty()).unwrap_or(true);
        match if first { crate::mcp::start(studio.backend.clone()) } else { Err(String::new()) } {
            Ok((m, calls)) => {
                studio.mcp = Some((m.url, m.token));
                cx.spawn_in(window, async move |this, cx| {
                    use futures::StreamExt;
                    let mut calls = calls;
                    while let Some(c) = calls.next().await {
                        let r = this.update_in(cx, |this, window, cx| this.mcp_tool(&c.tool, &c.args, window, cx));
                        let _ = c.reply.send(r.unwrap_or_else(|_| Err("the Studio has closed".into())));
                    }
                })
                .detach();
            }
            Err(e) if !e.is_empty() => eprintln!("MCP server not started: {e}"),
            Err(_) => {}
        }
        Self::watch_recovery(cx);
        if first {
            updates::check_at_start(studio.prefs.check_updates, cx);
        }
        let launch = start.as_deref() == Some("");
        let arg = if launch { std::env::args().nth(1) } else { start };
        let arg = if launch { arg.or_else(|| std::env::var("TEMPEST_STUDIO_DIR").ok()).or_else(|| studio.recent.first().cloned().filter(|d| std::path::Path::new(d).is_dir())) } else { arg };
        if let Some(a) = arg {
            studio.open_path(a, cx);
        }
        studio
    }

    /// Open what a path names: a workspace file, a package (its folder, then it) or a folder.
    pub(crate) fn open_path(&mut self, path: String, cx: &mut Context<Self>) {
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".twrk") {
            self.open_workspace_file(path, cx);
        } else if lower.ends_with(".upk") {
            let Some(dir) = std::path::Path::new(&path).parent().map(|d| d.to_string_lossy().to_string()) else { return };
            if self.dir.as_deref().is_some_and(|d| std::path::Path::new(d) == std::path::Path::new(&dir)) {
                if let Some(p) = self.packages.iter().find(|p| std::path::Path::new(&p.path) == std::path::Path::new(&path)).map(|p| p.path.clone()) {
                    self.user_open_package(p, cx);
                }
            } else {
                self.open_after = Some(path);
                self.open_workspace(dir, cx);
            }
        } else {
            self.open_workspace(path, cx);
        }
    }

    pub(crate) fn set_status(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = text.into();
        cx.notify();
    }

    /// A notification (shown at the next frame) and the status line.
    pub(crate) fn toast(&mut self, tone: Tone, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        self.status = text.clone().into();
        self.toasts.push((tone, text));
        cx.notify();
    }

    // ----- workspace -------------------------------------------------------

    fn pick_workspace(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Open the game's CookedPCConsole folder".into()) });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut p))) = paths.await {
                if let Some(dir) = p.pop() {
                    let _ = this.update(cx, |this, cx| this.open_workspace(dir.to_string_lossy().to_string(), cx));
                }
            }
        })
        .detach();
    }

    fn open_workspace(&mut self, dir: String, cx: &mut Context<Self>) {
        // The game folder itself is fine: its CookedPCConsole is meant.
        let dir = {
            let cooked = std::path::Path::new(&dir).join("ChaosGame").join("CookedPCConsole");
            if cooked.is_dir() { cooked.to_string_lossy().to_string() } else { dir }
        };
        self.set_status(format!("Opening {dir}…"), cx);
        let call = self.backend.call("workspace.open", json!({ "dir": dir }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.packages = model::packages(&r.json["packages"]);
                    this.dir = Some(dir.clone());
                    this.open = None;
                    this.rows.clear();
                    this.inspected = None;
                    this.preview = None;
                    this.selected_package = None;
                    this.selected_export = None;
                    this.dirty.clear();
                    this.history.clear();
                    this.history_at = 0;
                    this.tabs.clear();
                    this.assembly = None;
                    this.map = None;
                    this.cooker = None;
                    remember_workspace(&dir);
                    this.recent = recent_workspaces();
                    this.refilter_packages(cx);
                    match this.open_after.take().and_then(|f| this.packages.iter().find(|p| std::path::Path::new(&p.path) == std::path::Path::new(&f)).map(|p| p.path.clone())) {
                        Some(p) => {
                            let place = Place::package(p, None);
                            this.record(place.clone());
                            this.go_later(place, cx);
                        }
                        None => this.restore_session(cx),
                    }
                    this.check_recovery(cx);
                    let ms = r.json["loadMs"].as_u64().unwrap_or(0);
                    if this.packages.is_empty() {
                        this.toast(Tone::Warning, format!("{dir} holds no packages: pick the game's ChaosGame/CookedPCConsole folder"), cx);
                    } else {
                        this.set_status(format!("{} packages, opened in {ms} ms", this.packages.len()), cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("Could not open {dir}: {e}"), cx),
            });
        })
        .detach();
    }

    /// The package list: what the filter and the Unsaved switch let through, pinned packages first.
    pub(crate) fn refilter_packages(&mut self, cx: &mut Context<Self>) {
        let f = self.package_filter.read(cx).value().to_ascii_lowercase();
        let pinned = self.pinned();
        let mut shown: Vec<usize> = (0..self.packages.len())
            .filter(|&i| f.is_empty() || self.packages[i].name.to_ascii_lowercase().contains(&f))
            .filter(|&i| !self.unsaved_only || self.dirty.contains(&self.packages[i].path))
            .collect();
        // Stable: the backend's order within each part.
        shown.sort_by_key(|&i| !pinned.contains(&self.packages[i].path));
        self.shown_packages = shown;
        cx.notify();
    }

    pub(crate) fn set_view(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        if self.view == view {
            return;
        }
        self.view = view;
        match view {
            View::Packages => {
                if let Some(p) = self.selected_package.clone() {
                    self.record(Place::package(p, self.selected_export));
                }
            }
            View::Assembly => {
                let a = self.assembly_view(window, cx);
                let place = a.read(cx).place();
                self.record(place);
            }
            View::Mod => {
                self.cooker_view(window, cx);
                self.record(Place::Mod);
            }
            View::Audio => {}
            View::Map => {
                let m = self.map_view(window, cx);
                if let Some(place) = m.read(cx).place() {
                    self.record(place);
                }
            }
        }
        cx.notify();
    }

    /// A level saved or discarded: the map view no longer lists it as unsaved.
    pub(crate) fn forget_map_edit(&self, path: &str, cx: &mut Context<Self>) {
        if let Some(m) = &self.map {
            let p = path.to_string();
            m.update(cx, |m, cx| {
                m.dirty.remove(&p);
                cx.notify();
            });
        }
    }

    pub(crate) fn map_entity(&self) -> Option<Entity<map::MapEditor>> {
        self.map.clone()
    }

    fn map_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<map::MapEditor> {
        if let Some(m) = &self.map {
            return m.clone();
        }
        let me = cx.entity().downgrade();
        let (hidden, panes) = (self.prefs.map_hidden.clone(), self.prefs.map_panes);
        let shared = self.panes.clone();
        let m = cx.new(|cx| map::MapEditor::new(self.backend.clone(), me, hidden, panes, shared, window, cx));
        self.map = Some(m.clone());
        m
    }

    pub(crate) fn assembly_entity(&self) -> Option<Entity<assembly::Assembly>> {
        self.assembly.clone()
    }

    pub(crate) fn cooker_entity(&self) -> Option<Entity<cooker::Cooker>> {
        self.cooker.clone()
    }

    fn assembly_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<assembly::Assembly> {
        if let Some(a) = &self.assembly {
            return a.clone();
        }
        let me = cx.entity().downgrade();
        let (shared, sizes) = (self.panes.clone(), self.pane_sizes("assembly", &[260.0, 420.0]));
        let a = cx.new(|cx| assembly::Assembly::new(self.backend.clone(), self.dir.clone().unwrap_or_default(), me, shared, sizes, window, cx));
        self.assembly = Some(a.clone());
        a
    }

    fn cooker_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<cooker::Cooker> {
        if let Some(c) = &self.cooker {
            return c.clone();
        }
        let me = cx.entity().downgrade();
        let project = self.project.clone();
        let c = cx.new(|cx| cooker::Cooker::new(self.backend.clone(), self.dir.clone().unwrap_or_default(), project, me, window, cx));
        self.cooker = Some(c.clone());
        c
    }

    // ----- history ---------------------------------------------------------------

    /// Record where the user went (not when walking the history).
    pub(crate) fn record(&mut self, place: Place) {
        if self.history.get(self.history_at) == Some(&place) {
            return;
        }
        // An export picked right after its package replaces the package-only place;
        // a row of the same assembly table replaces the table-only place.
        let refines = match (self.history.get(self.history_at), &place) {
            (Some(Place::Package { package: a, export: None, .. }), Place::Package { package: b, .. }) => a == b,
            (Some(Place::Package { package: a, export: Some(i), view: None }), Place::Package { package: b, export: Some(j), view: Some(_) }) => a == b && i == j,
            (Some(Place::Assembly { file: a, table: ta, row: None }), Place::Assembly { file: b, table: tb, .. }) => a == b && ta == tb,
            (Some(Place::Map { path: a, export: None, view: va, .. }), Place::Map { path: b, view: vb, .. }) => a == b && va == vb,
            _ => false,
        };
        if refines {
            self.history[self.history_at] = place;
            self.history.truncate(self.history_at + 1);
        } else {
            if !self.history.is_empty() {
                self.history.truncate(self.history_at + 1);
            }
            self.history.push(place);
            self.history_at = self.history.len() - 1;
        }
        self.sync_tab();
        self.remember_place();
    }

    pub(crate) fn go(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        match place {
            Place::Package { package, export, view } => {
                self.view = View::Packages;
                if self.selected_package.as_deref() == Some(package.as_str()) && self.open.is_some() {
                    if let Some(i) = export {
                        self.reveal(i, cx);
                        self.select_export(i, cx);
                        // A movie editor's view (Back walks out of clips the way it came).
                        if let (Some(v), Some(m)) = (view.as_deref().and_then(swf::Node::from_key), self.movies.get(&(package.clone(), i)).cloned()) {
                            m.update(cx, |m, cx| m.open(v, cx));
                        }
                    }
                } else {
                    self.pending_export = export;
                    self.open_package(package, cx);
                }
            }
            Place::Assembly { file, table, row } => {
                self.view = View::Assembly;
                let a = self.assembly_view(window, cx);
                a.update(cx, |a, cx| a.visit(file, table, row, cx));
            }
            Place::Mod => {
                self.view = View::Mod;
                self.cooker_view(window, cx);
            }
            place @ Place::Map { .. } => {
                self.view = View::Map;
                let m = self.map_view(window, cx);
                m.update(cx, |m, cx| m.visit(place, cx));
            }
        }
        cx.notify();
    }

    /// `go` at the next frame (from where there is no window).
    pub(crate) fn go_later(&mut self, place: Place, cx: &mut Context<Self>) {
        self.pending_go = Some(place);
        cx.notify();
    }

    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_at > 0 {
            self.history_at -= 1;
            self.go(self.history[self.history_at].clone(), window, cx);
            self.sync_tab();
        }
    }

    fn forward(&mut self, _: &Forward, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_at + 1 < self.history.len() {
            self.history_at += 1;
            self.go(self.history[self.history_at].clone(), window, cx);
            self.sync_tab();
        }
    }

    // ----- editing -------------------------------------------------------------

    fn start_edit(&mut self, path: String, draft: String, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(path);
        self.value_input.update(cx, |input, cx| {
            input.set_value(draft, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            cx.notify();
        }
    }

    fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prop) = self.editing.take() else { return };
        let value = self.value_input.read(cx).value().to_string();
        self.focus.focus(window, cx);
        if prop == actions::RENAME {
            self.rename(value, cx);
            return;
        }
        if let Some(field) = prop.strip_prefix(particle::FIELD) {
            self.commit_particle_field(field, value, cx);
            return;
        }
        if let Some(name) = prop.strip_prefix(material::PARAM) {
            self.commit_parameter(name.to_string(), value, cx);
            return;
        }
        self.set_property(prop, value, cx);
    }

    /// `object.set` on the inspected export; the reply is the object as it is now.
    fn set_property(&mut self, prop: String, value: String, cx: &mut Context<Self>) {
        let (Some(t), Some(i)) = (&self.open, self.selected_export) else { return };
        let path = t.path.clone();
        let call = self.backend.call("object.set", json!({ "path": path, "index": i, "prop": prop, "value": value }));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.apply_object(&path, i, &r.json, cx);
                    this.set_status(format!("Set {prop} (unsaved)"), cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not set {prop}: {e}"), cx),
            });
        })
        .detach();
    }

    /// Take back the last edit: of the assembly files in the Assembly view,
    /// else of the open package, then read its tables and the selected
    /// object again (undo may have renamed or deleted exports).
    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if self.view == View::Assembly {
            if let Some(a) = self.assembly.clone() {
                a.update(cx, |a, cx| a.undo(window, cx));
                return;
            }
        }
        // The map's edits, last first (each is an undo step of its level).
        if self.view == View::Map {
            if let Some(m) = self.map.clone().filter(|m| m.read(cx).can_undo()) {
                m.update(cx, |m, cx| m.undo(cx));
                return;
            }
        }
        // A movie's pending edits go first, then the package's undo.
        if self.view == View::Packages {
            if let Some(m) = self.shown_movie() {
                if m.update(cx, |m, cx| m.undo_edit(cx)) {
                    self.set_status("Took back the last movie edit", cx);
                    return;
                }
            }
        }
        self.undo_package(cx);
    }

    fn undo_package(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.open.as_ref().map(|t| t.path.clone()) else { return };
        let call = self.backend.call("package.undo", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let Ok(r) = r else { return };
            let label = r.json["label"].as_str().map(|s| s.to_string());
            let dirty = r.json["dirty"].as_bool().unwrap_or(false);
            let Ok(tables) = this.update(cx, |this, cx| {
                if dirty {
                    this.dirty.insert(path.clone());
                } else {
                    this.dirty.remove(&path);
                }
                this.set_status(
                    match &label {
                        Some(l) => format!("Undid {l}"),
                        None => "Nothing to undo".into(),
                    },
                    cx,
                );
                this.backend.call("package.tables", json!({ "path": path }))
            }) else { return };
            if label.is_none() {
                return;
            }
            let Ok(t) = tables.await else { return };
            let _ = this.update(cx, |this, cx| this.reload_tables(&path, &t.json, None, cx));
        })
        .detach();
    }

    /// Save the open package into the game folder, after asking (the Studio
    /// records the original first, the base of the mod's delta).
    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.is_some() {
            self.save_project(cx);
            self.set_status("Project saved (the game folder is untouched)", cx);
            return;
        }
        self.write_to_game(window, cx);
    }

    /// Save into the game folder: the open package, or the assembly files in the Assembly view.
    pub(crate) fn write_to_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view == View::Assembly {
            if let Some(a) = self.assembly.clone() {
                a.update(cx, |a, cx| a.save(window, cx));
                return;
            }
        }
        if self.view == View::Map {
            if let Some(m) = self.map.clone() {
                m.update(cx, |m, cx| m.save(window, cx));
                return;
            }
        }
        let Some(path) = self.open.as_ref().map(|t| t.path.clone()).filter(|p| self.dirty.contains(p)) else {
            self.set_status("Nothing to save in this package", cx);
            return;
        };
        self.save_packages(vec![path], window, cx);
    }

    /// Save packages in place, after one question.
    pub(crate) fn save_packages(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            self.set_status("Nothing to save", cx);
            return;
        }
        if !self.prefs.ask_in_place {
            self.save_now(paths, window, cx);
            return;
        }
        let what = if paths.len() == 1 { file_name(&paths[0]) } else { format!("{} packages", paths.len()) };
        let me = cx.entity().downgrade();
        let dont_ask = std::rc::Rc::new(std::cell::Cell::new(false));
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let (me, paths, flag, set) = (me.clone(), paths.clone(), dont_ask.clone(), dont_ask.clone());
            dialog
                .title(format!("Save {what} into the game folder?"))
                .w(px(520.0))
                .child(
                    v_flex()
                        .gap_3()
                        .child(div().text_sm().text_color(theme.muted_foreground).child("The files in the game folder are overwritten. The Studio keeps each original first, so a mod can still be made as a delta against it. To keep the game folder untouched, use Export a copy or a workspace file instead."))
                        .child(gpui_component::checkbox::Checkbox::new("dont-ask").label("Don't ask me again (Settings can turn it back on)").checked(set.get()).on_click(move |v, window, _| {
                            set.set(*v);
                            window.refresh();
                        })),
                )
                .footer(dialogs::footer("Save", false, true, move |window, cx| {
                    window.close_dialog(cx);
                    let dont = flag.get();
                    let _ = me.update(cx, |this, cx| {
                        if dont {
                            this.prefs.ask_in_place = false;
                            this.prefs.save();
                        }
                        this.save_now(paths.clone(), window, cx)
                    });
                }))
        });
    }

    fn save_now(&mut self, paths: Vec<String>, _window: &mut Window, cx: &mut Context<Self>) {
        for path in paths {
            self.set_status(format!("Saving {}…", file_name(&path)), cx);
            let call = self.backend.call("package.save", json!({ "path": path }));
            cx.spawn(async move |this, cx| {
                let r = call.await;
                let _ = this.update(cx, |this, cx| match r {
                    Ok(_) => {
                        this.dirty.remove(&path);
                        this.forget_map_edit(&path, cx);
                        this.toast(Tone::Success, format!("Saved {}", file_name(&path)), cx);
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not save {}: {e}", file_name(&path)), cx),
                });
            })
            .detach();
        }
    }

    // ----- shell -----------------------------------------------------------------

    fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let folder = self.dir.as_deref().map(folder_label).unwrap_or_else(|| "No game folder".into());
        let me = cx.entity().downgrade();
        let recent = self.recent.clone();
        let workspace = Button::new("workspace")
            .small()
            .ghost()
            .icon(Icon::new(IconName::FolderOpen))
            .label(folder)
            .tooltip("The game folder: open another, or a recent one")
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.item(PopupMenuItem::new("Open game folder…").on_click({
                    let me = me.clone();
                    move |_, _, cx| {
                        let _ = me.update(cx, |this, cx| this.pick_workspace(cx));
                    }
                }));
                if !recent.is_empty() {
                    menu = menu.separator().label("Recent");
                    for d in recent.iter().take(8) {
                        let (me, d) = (me.clone(), d.clone());
                        menu = menu.item(PopupMenuItem::new(folder_label(&d)).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.open_workspace(d.clone(), cx));
                        }));
                    }
                }
                menu
            });
        let search = h_flex()
            .id("search-field")
            .w(px(360.0))
            .h(px(26.0))
            .px_2()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .text_sm()
            .text_color(theme.muted_foreground)
            .cursor_pointer()
            .hover(|s| s.border_color(theme.ring))
            .on_click(cx.listener(|this, _, window, cx| this.open_palette("", window, cx)))
            .child(Icon::new(IconName::Search).small())
            .child(div().flex_1().child("Go to a package, object or command"))
            .child(div().text_xs().child(commands::key_hint("⌘K")))
            .child(crate::driver::probe("search-field"));
        let project_chip = self.render_project_chip(cx);
        let in_project = self.project.is_some();
        let unsaved = self.dirty.len() + self.assembly.as_ref().map(|a| a.read(cx).unsaved_count()).unwrap_or(0);
        let can_save = match self.view {
            View::Assembly => self.assembly.as_ref().is_some_and(|a| a.read(cx).unsaved_count() > 0),
            View::Map => self.map.as_ref().is_some_and(|m| !m.read(cx).dirty_levels(cx).is_empty()),
            _ => self.open.as_ref().is_some_and(|t| self.dirty.contains(&t.path)),
        };
        let me = cx.entity().downgrade();
        let save = Button::new("save")
            .small()
            .when(can_save && !in_project, |b| b.primary())
            .icon(Icon::new(IconName::Save))
            .label(if in_project { "Project" } else { "Save" })
            .disabled(!in_project && !can_save && unsaved == 0)
            .tooltip(if in_project { "The project saves itself; this menu writes into the game folder or exports" } else { "Save into the game folder (asks first)" })
            .dropdown_menu(move |menu, _, _| {
                let item = |label: &str, cmd: Cmd| {
                    let me = me.clone();
                    PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                        let _ = me.update(cx, |this, cx| this.run(cmd, window, cx));
                    })
                };
                let menu = if in_project { menu.item(item("Save the project now", Cmd::Save)).separator() } else { menu };
                menu.item(item(if in_project { "Write this file into the game folder…" } else { "Save this file" }, Cmd::WriteToGame))
                    .item(item("Save every unsaved file into the game folder", Cmd::SaveAll))
                    .separator()
                    .item(item("Export a copy…  (keeps the game folder untouched)", Cmd::SaveAs))
                    .item(item("Discard this file's changes…", Cmd::Discard))
            });
        h_flex()
            .h(px(40.0))
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .child(Button::new("back").small().ghost().icon(Icon::new(IconName::ArrowLeft)).tooltip("Back (⌥←)").disabled(self.history_at == 0).on_click(cx.listener(|this, _, window, cx| this.back(&Back, window, cx))))
            .child(Button::new("forward").small().ghost().icon(Icon::new(IconName::ArrowRight)).tooltip("Forward (⌥→)").disabled(self.history_at + 1 >= self.history.len()).on_click(cx.listener(|this, _, window, cx| this.forward(&Forward, window, cx))))
            .child(workspace)
            .children(project_chip)
            .child(div().flex_1())
            .child(div().relative().child(Button::new("create").small().primary().icon(Icon::new(IconName::Plus)).label("Create").tooltip("Make something new: a texture, mesh, material, sound, skin, ability… (⌘N)").disabled(self.dir.is_none()).on_click(cx.listener(|this, _, window, cx| this.open_wizard(None, None, window, cx)))).child(crate::driver::probe("create")))
            .child(search)
            .child(div().flex_1())
            .when(in_project, |d| d.child(Button::new("build").small().primary().icon(Icon::new(IconName::Hammer)).label("Build mod").tooltip("The Mod view: what your project's edits become").on_click(cx.listener(|this, _, window, cx| this.set_view(View::Mod, window, cx)))))
            .child(Button::new("undo").small().ghost().icon(Icon::new(IconName::Undo2)).tooltip("Undo the last edit (⌘Z)").on_click(cx.listener(|this, _, window, cx| this.undo(&Undo, window, cx))))
            .when(unsaved > 0, |d| {
                d.child(
                    Button::new("unsaved")
                        .small()
                        .ghost()
                        .label(format!("{unsaved} unsaved"))
                        .tooltip("The files with edits not saved yet")
                        .on_click(cx.listener(|this, _, window, cx| this.show_unsaved(window, cx))),
                )
            })
            .child(save)
    }

    fn render_activity(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let item = |id: &'static str, icon: IconName, tip: &'static str, active: bool, cx: &mut Context<Self>, f: fn(&mut Studio, &mut Window, &mut Context<Studio>)| {
            div()
                .relative()
                .child(Button::new(id).ghost().icon(Icon::new(icon)).tooltip(tip).selected(active).on_click(cx.listener(move |this, _, window, cx| f(this, window, cx))))
                .child(crate::driver::probe(id))
        };
        let v = self.view;
        v_flex()
            .w(px(44.0))
            .h_full()
            .py_1()
            .gap_1()
            .items_center()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(item("view-packages", IconName::Package, "Packages: objects, textures, meshes, sounds, scripts (⌘1)", v == View::Packages, cx, |s, w, cx| s.set_view(View::Packages, w, cx)))
            .child(item("view-assembly", IconName::Database, "Assembly: the game's content tables and texts (⌘2)", v == View::Assembly, cx, |s, w, cx| s.set_view(View::Assembly, w, cx)))
            .child(item("view-audio", IconName::Volume2, "Audio: the game's Wwise sounds, by event (⌘4)", v == View::Audio, cx, |s, w, cx| s.set_view(View::Audio, w, cx)))
            .child(item("view-map", IconName::Map, "Maps: a map and its sublevels in 3D, Kismet, Matinee (⌘5)", v == View::Map, cx, |s, w, cx| s.set_view(View::Map, w, cx)))
            .child(item("view-mod", IconName::Hammer, "Make a mod from your changes (⌘3)", v == View::Mod, cx, |s, w, cx| s.set_view(View::Mod, w, cx)))
            .child(div().flex_1())
            .child(item("view-search", IconName::Search, "Search every package (⌘⇧F)", false, cx, |s, w, cx| s.open_palette("#", w, cx)))
            .child(item("view-shortcuts", IconName::Keyboard, "Keyboard shortcuts (F1)", false, cx, |s, w, cx| s.show_shortcuts(w, cx)))
            .child(item("view-settings", IconName::Settings, "Settings (⌘,)", false, cx, |s, w, cx| s.show_settings(w, cx)))
    }

    fn render_status(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let right = match (&self.dir, &self.open) {
            (Some(_), Some(t)) if self.view == View::Packages => format!("{} · {} objects", t.name, t.exports.len()),
            (Some(_), _) => format!("{} packages", self.packages.len()),
            _ => String::new(),
        };
        h_flex().h(px(24.0)).px_2().gap_2().border_t_1().border_color(theme.border).text_xs().text_color(theme.muted_foreground).child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(self.status.clone())).child(right)
    }
}

impl Render for Studio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        self.sync_compact(window);
        for (tone, text) in std::mem::take(&mut self.toasts) {
            let n = match tone {
                Tone::Info => Notification::info(text),
                Tone::Success => Notification::success(text),
                Tone::Warning => Notification::warning(text),
                Tone::Error => Notification::error(text),
            };
            window.push_notification(n, cx);
        }
        if let Some(pick) = self.pending_pick.take() {
            self.open_picker(pick, window, cx);
        }
        if self.pending_batch.is_some() {
            self.show_batch(window, cx);
        }
        if let Some(place) = self.pending_go.take() {
            self.go(place, window, cx);
        }
        if self.pending_anim_pick.is_some() {
            self.ask_anim_pick(window, cx);
        }
        if let Some(view) = self.pending_view.take() {
            self.set_view(view, window, cx);
        }
        // Views dropped behind the window's back (a project opened or closed) come back fresh.
        if self.dir.is_some() && self.view == View::Mod && self.cooker.is_none() {
            self.cooker_view(window, cx);
        }
        if self.dir.is_some() && self.view == View::Map && self.map.is_none() {
            self.map_view(window, cx);
        }
        // The Unsaved switch follows edits made anywhere.
        if self.unsaved_only && self.shown_packages.len() != self.packages.iter().filter(|p| self.dirty.contains(&p.path)).count() {
            self.refilter_packages(cx);
        }
        let body: gpui::AnyElement = if self.dir.is_none() {
            self.render_welcome(cx).into_any_element()
        } else {
            match self.view {
                View::Packages => self.render_packages_view(window, cx).into_any_element(),
                View::Assembly => match self.assembly.clone() {
                    Some(a) => a.into_any_element(),
                    None => div().into_any_element(),
                },
                View::Audio => self.render_audio_view(window, cx),
                View::Map => match self.map.clone() {
                    Some(m) => m.into_any_element(),
                    None => div().into_any_element(),
                },
                View::Mod => match self.cooker.clone() {
                    Some(c) => c.into_any_element(),
                    None => div().into_any_element(),
                },
            }
        };
        let palette = self.render_palette(cx);
        let tab_bar = self.render_tabs(cx);
        let header = self.render_header(cx);
        let activity = self.render_activity(cx);
        let status = self.render_status(cx);
        let recovery = self.render_recovery(cx);
        let update = self.render_update_banner(cx);
        let root = v_flex()
            .id("studio")
            .key_context("Studio")
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .relative()
            .child(header)
            .children(update)
            .children(recovery)
            .child(h_flex().items_start().flex_1().min_h_0().child(activity).child(v_flex().flex_1().min_w_0().h_full().children(tab_bar).child(div().flex_1().min_h_0().child(body))))
            .child(status)
            .children(palette)
            .children(self.wizard.clone());
        self.bind_actions(root, cx)
    }
}

/// `texture2d` → `Texture 2D`: cooked packages store class names lower-case.
pub(crate) fn pretty_class(class: &str) -> String {
    let known = [
        ("texture2d", "Texture 2D"),
        ("staticmesh", "Static mesh"),
        ("skeletalmesh", "Skeletal mesh"),
        ("materialinstanceconstant", "Material instance"),
        ("materialinstancetimevarying", "Material instance (animated)"),
        ("material", "Material"),
        ("soundnodewave", "Sound"),
        ("soundcue", "Sound cue"),
        ("akevent", "Wwise event"),
        ("akbank", "Wwise bank"),
        ("animsequence", "Animation"),
        ("animset", "Animation set"),
        ("swfmovie", "UI movie"),
        ("particlesystem", "Particle system"),
        ("physicsasset", "Physics asset"),
        ("package", "Group"),
        ("lightmaptexture2d", "Light map"),
        ("shadowmaptexture2d", "Shadow map"),
        ("texturecube", "Cube map"),
        ("class", "Class"),
        ("function", "Function"),
    ];
    if let Some((_, p)) = known.iter().find(|(k, _)| class.eq_ignore_ascii_case(k)) {
        return p.to_string();
    }
    let mut c = class.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The icon of an object of `class` in the lists and the inspector.
pub(crate) fn class_icon(class: &str) -> IconName {
    let c = class.to_ascii_lowercase();
    if c == "package" {
        IconName::Folder
    } else if c.contains("texture") || c.contains("lightmap") || c.contains("shadowmap") {
        IconName::Image
    } else if c.contains("mesh") {
        IconName::Box
    } else if c.starts_with("material") {
        IconName::Palette
    } else if c.starts_with("sound") || c.starts_with("ak") {
        IconName::Music
    } else if c.starts_with("anim") {
        IconName::Film
    } else if c == "swfmovie" {
        IconName::Layers
    } else if c.starts_with("particle") {
        IconName::Sparkles
    } else if matches!(c.as_str(), "class" | "function" | "state" | "scriptstruct" | "enum" | "const") || c.ends_with("property") {
        IconName::Code
    } else {
        IconName::FileText
    }
}

pub(crate) fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// `…/8.1/ChaosGame/CookedPCConsole` → `8.1`: the game copy's folder name.
fn folder_label(dir: &str) -> String {
    let p = std::path::Path::new(dir);
    let name = |p: &std::path::Path| p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    if name(p).eq_ignore_ascii_case("cookedpcconsole") {
        if let Some(game) = p.parent().and_then(|c| c.parent()) {
            return name(game);
        }
    }
    name(p)
}

/// `<config>/tempest-studio-native/<name>`.
pub(crate) fn state_file(name: &str) -> Option<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("tempest-studio-native").join(name))
}

fn recent_workspaces() -> Vec<String> {
    let Some(f) = state_file("recent-workspaces") else { return Vec::new() };
    let mut list: Vec<String> = std::fs::read_to_string(f).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    // The single-folder file of earlier builds.
    if let Some(old) = state_file("last-workspace").and_then(|f| std::fs::read_to_string(f).ok()) {
        let old = old.trim().to_string();
        if !old.is_empty() && !list.contains(&old) {
            list.push(old);
        }
    }
    list
}

fn remember_workspace(dir: &str) {
    let mut list = recent_workspaces();
    list.retain(|d| d != dir);
    list.insert(0, dir.to_string());
    list.truncate(12);
    if let Some(f) = state_file("recent-workspaces") {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, list.join("\n"));
    }
}

/// The Studio windows, newest last: files opened from the OS go to the last one.
static WINDOWS: std::sync::Mutex<Vec<gpui::AnyWindowHandle>> = std::sync::Mutex::new(Vec::new());

/// A Studio window with its own backend (a backend holds one game folder).
pub fn open_window(start: Option<String>, cx: &mut App) -> Option<Entity<Studio>> {
    use gpui::{size, Bounds, WindowBounds, WindowOptions};
    let (backend, events) = crate::backend::Backend::start();
    let n = WINDOWS.lock().map(|w| w.len()).unwrap_or(0) as f32;
    let mut bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
    bounds.origin.x += px(28.0 * n);
    bounds.origin.y += px(28.0 * n);
    let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
    let mut studio = None;
    let window = cx
        .open_window(options, |window, cx| {
            let view = cx.new(|cx| Studio::new(backend, events, start, window, cx));
            studio = Some(view.clone());
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        })
        .ok()?;
    if let Ok(mut w) = WINDOWS.lock() {
        w.push(window.into());
    }
    studio
}

/// Files opened from the OS (Finder, file associations): to the window shown last.
pub fn open_urls(urls: Vec<String>, cx: &mut App) {
    let paths: Vec<String> = urls.into_iter().map(|u| u.strip_prefix("file://").map(|p| p.replace("%20", " ")).unwrap_or(u)).collect();
    let target = cx.active_window().or_else(|| WINDOWS.lock().ok().and_then(|w| w.last().copied()));
    for p in paths {
        let done = target.and_then(|w| w.update(cx, |root, _, cx| {
            let Ok(root) = root.downcast::<gpui_component::Root>() else { return false };
            let Ok(view) = root.read(cx).view().clone().downcast::<Studio>() else { return false };
            view.update(cx, |s, cx| s.open_path(p.clone(), cx));
            true
        }).ok()).unwrap_or(false);
        if !done {
            open_window(Some(p), cx);
        }
    }
}

pub fn init(cx: &mut App) {
    commands::bind_keys(cx);
    prefs::theme_mode(&prefs::Prefs::load().theme, None, cx);
}
