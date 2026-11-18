//! The movie editor on SwfMovie exports, laid out like FFDec and like the
//! Electron Studio's: the resource tree (`tree.rs`), the stage (`stage.rs`)
//! over the timeline of the open sprite or the root (`timeline.rs`), and the
//! properties of the selected object or character (`props.rs`,
//! `character.rs`). The "Movie" node plays the whole movie; the tag views
//! (`sections.rs`) list texts, scripts, images, linkage and tags.
//!
//! One editor per movie export, kept while the package is open, so the
//! pending edits and the place survive selecting something else. Edits are
//! the backend's `swf::apply` format, keyed like the Electron Studio's
//! (`place|sprite|tag`, `text|tag|0`, `script|<loc key>`…) and pending until
//! Apply writes them as one undo step; every backend view (tree, info,
//! timeline, preview, character, export) takes them, so what is shown always
//! includes them. ⌘Z takes back the last pending change first (quick changes
//! of one edit are one step).

mod character;
pub mod geom;
mod props;
mod sections;
mod stage;
mod timeline;
mod tool;
mod tree;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use gpui::{div, prelude::*, px, AnyElement, App, Context, Entity, FocusHandle, PathPromptOptions, Subscription, WeakEntity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable, Selectable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{Studio, Tone};
use crate::backend::Backend;
use geom::{Rect, M};

pub fn is_movie(class: &str) -> bool {
    class.eq_ignore_ascii_case("swfmovie")
}

/// What the editor shows: the movie, the root timeline, a tag view or a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Node {
    Movie,
    Frames,
    Texts,
    Scripts,
    Images,
    Linkage,
    Tags,
    Char(u64),
}

impl Node {
    pub fn key(self) -> String {
        match self {
            Node::Char(id) => format!("char:{id}"),
            n => format!("{n:?}").to_ascii_lowercase(),
        }
    }

    pub fn from_key(k: &str) -> Option<Node> {
        Some(match k {
            "movie" => Node::Movie,
            "frames" => Node::Frames,
            "texts" => Node::Texts,
            "scripts" => Node::Scripts,
            "images" => Node::Images,
            "linkage" => Node::Linkage,
            "tags" => Node::Tags,
            k => Node::Char(k.strip_prefix("char:")?.parse().ok()?),
        })
    }

    fn section(self) -> bool {
        matches!(self, Node::Texts | Node::Scripts | Node::Images | Node::Linkage | Node::Tags)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Move,
    Transform,
}

/// The tree's character groups, in FFDec's order: (id, title, kinds).
pub const GROUPS: &[(&str, &str, &[&str])] = &[
    ("sprites", "Sprites", &["sprite"]),
    ("shapes", "Shapes", &["shape", "morph"]),
    ("images", "Images", &["image", "subimage", "bitmap"]),
    ("texts", "Texts", &["edittext", "text"]),
    ("fonts", "Fonts", &["font"]),
    ("buttons", "Buttons", &["button"]),
    ("others", "Sounds, video, data", &["sound", "video", "binary"]),
    ("imports", "Imported", &["import"]),
];

pub fn group_icon(id: &str) -> IconName {
    match id {
        "sprites" => IconName::Film,
        "shapes" => IconName::Palette,
        "images" => IconName::Image,
        "texts" | "fonts" => IconName::FileText,
        "buttons" => IconName::Square,
        "imports" => IconName::Download,
        _ => IconName::FileArchive,
    }
}

/// The backend's key of a script location (`as2::Loc::key`).
pub fn script_key(loc: &J) -> String {
    if let Some(b) = loc["button"].as_u64() {
        return format!("b{}:{b}", loc["tag"]);
    }
    let sprite = loc["sprite"].as_u64().unwrap_or(0);
    if let Some(e) = loc["event"].as_u64() {
        return format!("s{sprite}:{}e{e}", loc["tag"]);
    }
    if sprite == 0 { format!("{}", loc["tag"]) } else { format!("s{sprite}:{}", loc["tag"]) }
}

/// The pending-edit key of an edit (one edit per key; the last one made wins).
pub fn edit_key(e: &J) -> String {
    let tag = &e["tag"];
    match e["kind"].as_str().unwrap_or("") {
        "place" => format!("place|{}|{tag}", e["sprite"]),
        "deletePlace" => format!("delete|{}|{tag}", e["sprite"]),
        "shapeColor" => format!("shapeColor|{tag}|{}", e["key"].as_str().unwrap_or("")),
        "staticText" => format!("staticText|{tag}|{}", e["record"]),
        k @ ("shapeSvg" | "editText" | "fontGlyphs") => format!("{k}|{tag}"),
        "script" | "pcode" => format!("script|{}", script_key(&e["loc"])),
        k => format!("{k}|{tag}|{}", e["index"].as_u64().unwrap_or(0)),
    }
}

/// An earlier state of the pending edits, the key of the change that replaced it, and when.
type Undo = (Vec<(String, J)>, Option<String>, Instant);

/// An inline field being typed in: its key, initial value, and input.
struct Field {
    key: String,
    from: String,
    input: Entity<InputState>,
    _sub: Subscription,
}

/// The Add image dialog's choices.
pub struct AddImage {
    file: Option<String>,
    name: Entity<InputState>,
    instance: Entity<InputState>,
    x: Entity<InputState>,
    y: Entity<InputState>,
    shape: bool,
    place: bool,
    working: bool,
}

pub struct MovieEditor {
    studio: WeakEntity<Studio>,
    this: WeakEntity<MovieEditor>,
    backend: Backend,
    pub path: String,
    pub index: usize,
    /// The movie export's name (file name stems).
    pub name: String,
    pub tree: J,
    pub info: J,
    chars: HashMap<u64, J>,
    /// The root timeline's first frame: where the root places each sprite.
    root_list: Vec<J>,
    pub error: Option<String>,
    pub loading: bool,
    /// Pending edits by key, in the order made.
    edits: Vec<(String, J)>,
    /// Earlier states of the edits (newest last), with the key of the change and when.
    undo: Vec<Undo>,
    /// Pending edits `undo_edit` took back, newest last.
    redo: Vec<Vec<(String, J)>>,
    /// Bumped whenever the edits change.
    pub version: u64,
    pub node: Node,
    pub frame: usize,
    /// Selected depth on the current timeline.
    pub depth: Option<u64>,
    pub timeline: Option<J>,
    timeline_for: String,
    /// A sprite plays (scripts and all) instead of showing its layout.
    pub play: bool,
    pub tool: Tool,
    pub show_bounds: bool,
    /// Layers left out of the stage render, by sprite (0: the root); never written.
    hidden: HashMap<u64, BTreeSet<u64>>,
    /// Character to select once the opened timeline has loaded (going up a level).
    reselect: Option<u64>,
    /// The script the Scripts view shows (`script_key`).
    pub script: Option<String>,
    tree_filter: Entity<InputState>,
    collapsed: BTreeSet<&'static str>,
    tree_scroll: gpui::UniformListScrollHandle,
    /// The node the tree last scrolled to, and its rows as last drawn.
    tree_followed: Option<Node>,
    tree_cache: Vec<tree::Item>,
    /// Bumped when the pointer enters or leaves a tree row (pre-rendering waits for it to rest).
    hover_seq: u64,
    /// Resource tree and properties widths, timeline height (kept in the settings).
    pub(crate) panes: [f32; 3],
    pub hide_tree: bool,
    pub hide_props: bool,
    tree_toggled: bool,
    props_toggled: bool,
    script_filter: Entity<InputState>,
    tag_filter: Entity<InputState>,
    stage: stage::Stage,
    script_view: Option<sections::ScriptView>,
    field: Option<Field>,
    lock_scale: bool,
    confirm_remove: bool,
    /// `swf.character` of the shown character at an edit version.
    detail: Option<(u64, u64, Result<J, String>)>,
    /// Texture thumbnails by (texture export, size).
    thumbs: HashMap<(usize, u32), Option<Arc<gpui::Image>>>,
    pub adding_image: Option<AddImage>,
    applying: bool,
    pub focus: FocusHandle,
    tree_focus: FocusHandle,
    timeline_focus: FocusHandle,
    /// The character editor's choices: SVG fit, PNG scale, the font to take glyphs from.
    fit_svg: bool,
    png_scale: u32,
    glyph_file: Option<String>,
    glyph_chars: Entity<InputState>,
    colors: HashMap<String, (String, Entity<gpui_component::color_picker::ColorPickerState>, Subscription)>,
    texts: HashMap<String, (String, Entity<gpui_component::input::TextareaState>, Subscription)>,
    _subs: Vec<Subscription>,
}

impl MovieEditor {
    pub fn new(studio: WeakEntity<Studio>, backend: Backend, path: String, index: usize, name: String, window: &mut Window, cx: &mut Context<Self>) -> MovieEditor {
        let input = |ph: &str, window: &mut Window, cx: &mut Context<Self>| {
            let ph = ph.to_string();
            cx.new(|cx| InputState::new(window, cx).placeholder(ph))
        };
        let tree_filter = input("Filter by id or name…", window, cx);
        let script_filter = input("Filter scripts…", window, cx);
        let tag_filter = input("Filter tags…", window, cx);
        let glyph_chars = input("characters to add (those already there are skipped)", window, cx);
        let subs = [&tree_filter, &script_filter, &tag_filter].map(|i| cx.subscribe(i, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        }));
        let mut ed = MovieEditor {
            studio,
            backend,
            path,
            index,
            name,
            tree: J::Null,
            info: J::Null,
            chars: HashMap::new(),
            root_list: Vec::new(),
            error: None,
            loading: false,
            edits: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            version: 0,
            node: Node::Frames,
            frame: 0,
            depth: None,
            timeline: None,
            timeline_for: String::new(),
            play: false,
            tool: Tool::Move,
            show_bounds: true,
            hidden: HashMap::new(),
            reselect: None,
            script: None,
            tree_filter,
            collapsed: ["imports", "others"].into_iter().collect(),
            tree_scroll: gpui::UniformListScrollHandle::new(),
            tree_followed: None,
            tree_cache: Vec::new(),
            hover_seq: 0,
            this: cx.entity().downgrade(),
            panes: [230.0, 290.0, 260.0],
            hide_tree: false,
            hide_props: false,
            tree_toggled: false,
            props_toggled: false,
            script_filter,
            tag_filter,
            stage: stage::Stage::new(cx),
            script_view: None,
            field: None,
            lock_scale: true,
            confirm_remove: false,
            detail: None,
            thumbs: HashMap::new(),
            adding_image: None,
            applying: false,
            focus: cx.focus_handle(),
            tree_focus: cx.focus_handle(),
            timeline_focus: cx.focus_handle(),
            fit_svg: true,
            png_scale: 1,
            glyph_file: None,
            glyph_chars,
            colors: HashMap::new(),
            texts: HashMap::new(),
            _subs: subs.into(),
        };
        ed.load(cx);
        ed
    }

    // ----- the studio ---------------------------------------------------------------

    /// Act on the Studio after the current update: the Studio may be the one
    /// updating this editor (an entity cannot be updated twice at once).
    /// Available horizontal width for this movie editor inside the window.
    fn available_width(&self, window: &Window, cx: &App) -> f32 {
        let vw = f32::from(window.viewport_size().width);
        let used = self
            .studio
            .upgrade()
            .map(|s| {
                let s = s.read(cx);
                if s.focus_mode {
                    return 0.0;
                }
                let p = s.panes.get();
                let sizes = s.pane_sizes("packages", &[240.0, 280.0]);
                let side = if p.side() {
                    if p.side_w > 0.0 { p.side_w } else { sizes[0] }
                } else {
                    0.0
                };
                let second = if p.second() { sizes[1] } else { 0.0 };
                side + second
            })
            .unwrap_or(0.0);
        (vw - used).max(320.0)
    }

    /// Which internal side panes are shown and their clamped widths so Stage always has generous room.
    fn shown_side_panes(&self, window: &Window, cx: &App) -> (bool, bool, f32, f32, bool) {
        let vw = f32::from(window.viewport_size().width);
        let avail = self.available_width(window, cx);
        let auto_hide_props = (vw < 1280.0 && avail < 1040.0) || avail < 860.0;
        let auto_hide_tree = (vw < 1280.0 && avail < 720.0) || avail < 560.0;
        let show_tree = if self.tree_toggled { !self.hide_tree } else { !self.hide_tree && !auto_hide_tree };
        let show_props = self.target().is_some() && if self.props_toggled { !self.hide_props } else { !self.hide_props && !auto_hide_props };
        let side_budget = (avail - 320.0).max(150.0);
        let (mut tree_w, mut props_w) = (self.panes[0], self.panes[1]);
        let mut capped = false;
        if show_tree && show_props && tree_w + props_w > side_budget {
            let scale = (side_budget / (tree_w + props_w)).clamp(0.45, 1.0);
            tree_w = (tree_w * scale).max(140.0);
            props_w = (props_w * scale).max(160.0);
            capped = true;
        } else if show_tree && tree_w > side_budget {
            tree_w = side_budget.max(140.0);
            capped = true;
        } else if show_props && props_w > side_budget {
            props_w = side_budget.max(160.0);
            capped = true;
        }
        (show_tree, show_props, tree_w, props_w, capped)
    }

    /// Keep a resized pane's size for every editor and the next run: `0` the main row
    /// (tree, centre, properties), `2` the stage over the timeline.
    fn pane_saver(&self, group: usize, show_tree: bool, show_props: bool, capped: bool) -> impl Fn(&Entity<gpui_component::resizable::ResizableState>, &mut Window, &mut gpui::App) + 'static {
        let (studio, me) = (self.studio.clone(), self.this.clone());
        move |state, _, cx| {
            let sizes: Vec<f32> = state.read(cx).sizes().iter().map(|s| f32::from(*s)).collect();
            let mut panes = match me.upgrade() {
                Some(m) => m.read(cx).panes,
                None => return,
            };
            if group == 2 {
                let Some(t) = sizes.get(1) else { return };
                panes[2] = *t;
            } else {
                if capped {
                    return;
                }
                if show_tree {
                    if let Some(t) = sizes.first() {
                        panes[0] = *t;
                    }
                }
                if show_props && sizes.len() == 3 {
                    panes[1] = sizes[2];
                }
            }
            let studio = studio.clone();
            cx.defer(move |cx| {
                let others = studio
                    .update(cx, |s, _| {
                        s.prefs.movie_panes = panes;
                        s.prefs.save();
                        s.movies.values().cloned().collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                for m in others {
                    m.update(cx, |m, _| m.panes = panes);
                }
            });
        }
    }

    fn with_studio(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut Studio, &mut Context<Studio>) + 'static) {
        let studio = self.studio.clone();
        cx.defer(move |cx| {
            let _ = studio.update(cx, f);
        });
    }

    pub(crate) fn toast(&self, tone: Tone, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        self.with_studio(cx, move |s, cx| s.toast(tone, text, cx));
    }

    fn status(&self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        self.with_studio(cx, move |s, cx| s.set_status(text, cx));
    }

    /// Notify the Studio (and an open Mod view or Changes dialog) that pending stage edits changed.
    fn notify_studio(&self, cx: &mut Context<Self>) {
        self.with_studio(cx, |s, cx| {
            if let Some(c) = s.cooker.clone() {
                c.update(cx, |_, cx| cx.notify());
            }
            cx.notify();
            cx.refresh_windows();
        });
    }

    /// The package now has unsaved changes (Apply, a replaced texture, an added image).
    fn mark_dirty(&self, refresh: bool, cx: &mut Context<Self>) {
        let path = self.path.clone();
        self.with_studio(cx, move |s, cx| {
            s.dirty.insert(path.clone());
            if refresh {
                s.refresh_package(path, None, cx);
            }
            if let Some(c) = s.cooker.clone() {
                c.update(cx, |c, cx| c.refresh(cx));
            }
            cx.notify();
            cx.refresh_windows();
        });
    }

    // ----- the document -------------------------------------------------------------

    /// Number of unapplied staged edits in this movie editor.
    pub fn unapplied_count(&self) -> usize {
        self.edits.len()
    }

    /// Whether this movie editor holds unapplied staged edits.
    /// Whether an undone stage edit can be put back.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn has_unapplied(&self) -> bool {
        !self.edits.is_empty()
    }

    /// Whether `swf.apply` is currently writing staged edits into the package.
    pub fn is_applying(&self) -> bool {
        self.applying
    }

    /// Movie identity: `(package_path, export_index, display_name)`.
    pub fn movie_target(&self) -> (&str, usize, &str) {
        (&self.path, self.index, &self.name)
    }

    pub fn pending(&self) -> J {
        J::Array(self.edits.iter().map(|(_, e)| e.clone()).collect())
    }

    pub fn edit(&self, key: &str) -> Option<&J> {
        self.edits.iter().find(|(k, _)| k == key).map(|(_, e)| e)
    }

    pub fn is_edited(&self, key: &str) -> bool {
        self.edit(key).is_some()
    }

    pub fn char(&self, id: u64) -> Option<&J> {
        self.chars.get(&id)
    }

    pub fn kind_of(&self, id: u64) -> &str {
        self.chars.get(&id).and_then(|c| c["kind"].as_str()).unwrap_or("?")
    }

    /// Label of a character: its linkage name, or kind and id.
    pub fn label(&self, id: u64) -> String {
        match self.chars.get(&id) {
            Some(c) => c["names"][0].as_str().map(|s| s.to_string()).unwrap_or_else(|| format!("{} {id}", c["kind"].as_str().unwrap_or("?"))),
            None => format!("#{id}"),
        }
    }

    /// The sprite whose timeline is shown: 0 for the root, None when none.
    pub fn sprite(&self) -> Option<u64> {
        match self.node {
            Node::Frames => Some(0),
            Node::Char(id) if self.kind_of(id) == "sprite" => Some(id),
            _ => None,
        }
    }

    pub fn character(&self) -> Option<&J> {
        match self.node {
            Node::Char(id) => self.chars.get(&id),
            _ => None,
        }
    }

    /// The display list at the current frame.
    pub fn entries(&self) -> Vec<J> {
        let (Some(t), Some(s)) = (&self.timeline, self.sprite()) else { return Vec::new() };
        if t["sprite"].as_u64() != Some(s) {
            return Vec::new();
        }
        let frames = t["frames"].as_array().map(|a| a.as_slice()).unwrap_or_default();
        frames.get(self.frame.min(frames.len().saturating_sub(1))).and_then(|f| f.as_array()).cloned().unwrap_or_default()
    }

    pub fn selected_entry(&self) -> Option<J> {
        let d = self.depth?;
        self.entries().into_iter().find(|e| e["depth"].as_u64() == Some(d))
    }

    pub fn frames(&self) -> usize {
        self.timeline.as_ref().filter(|t| t["sprite"].as_u64() == self.sprite()).and_then(|t| t["frames"].as_array()).map(|a| a.len()).unwrap_or(1).max(1)
    }

    /// The sprite that places a character: the root when its first frame does, else the first sprite using it.
    fn parent_of(&self, id: u64) -> Option<u64> {
        if id == 0 {
            return None;
        }
        if self.root_list.iter().any(|e| e["character"].as_u64() == Some(id)) {
            return Some(0);
        }
        self.chars.get(&id)?["usedBy"].as_array()?.iter().filter_map(|p| p.as_u64()).find(|&p| p != id && self.kind_of(p) == "sprite")
    }

    /// The movie's stage in a sprite's coordinates (0: the root), when the root places it.
    pub fn outline_of(&self, sprite: Option<u64>) -> Option<Rect> {
        let stage = Rect::from_json(&self.tree["stage"])?;
        let sprite = sprite?;
        if sprite == 0 {
            return Some(stage);
        }
        let p = self.root_list.iter().find(|e| e["character"].as_u64() == Some(sprite))?;
        let inv = M::from_json(&p["matrix"]).invert()?;
        let pts = [inv.apply(stage.x, stage.y), inv.apply(stage.x + stage.w, stage.y), inv.apply(stage.x, stage.y + stage.h), inv.apply(stage.x + stage.w, stage.y + stage.h)];
        let (x0, x1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let (y0, y1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.1), b.max(p.1)));
        Some(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
    }

    /// Root › … › the open sprite, for the breadcrumb (empty outside sprite views).
    pub fn trail(&self) -> Vec<(u64, String)> {
        let Some(sprite) = self.sprite() else { return Vec::new() };
        let mut out = Vec::new();
        let mut cur = Some(sprite);
        let mut seen = BTreeSet::new();
        while let Some(c) = cur.filter(|&c| c != 0 && out.len() < 32 && seen.insert(c)) {
            out.insert(0, (c, self.label(c)));
            cur = self.parent_of(c);
        }
        out.insert(0, (0, "Root".to_string()));
        out
    }

    pub fn hidden(&self) -> BTreeSet<u64> {
        self.sprite().and_then(|s| self.hidden.get(&s).cloned()).unwrap_or_default()
    }

    /// Show or hide layers of the current timeline in the stage render.
    pub fn set_hidden(&mut self, depths: &[u64], hide: bool, cx: &mut Context<Self>) {
        let Some(s) = self.sprite() else { return };
        let set = self.hidden.entry(s).or_default();
        for d in depths {
            if hide {
                set.insert(*d);
            } else {
                set.remove(d);
            }
        }
        if hide && self.depth.is_some_and(|d| depths.contains(&d)) {
            self.depth = None;
        }
        cx.notify();
    }

    /// The external image a character draws: its own, or a sub-image's atlas.
    pub fn image_of(&self, id: u64) -> Option<J> {
        let images = self.info["images"].as_array()?;
        if let Some(own) = images.iter().find(|i| i["id"].as_u64().map(|v| v & 0xffff) == Some(id)) {
            return Some(own.clone());
        }
        let c = self.chars.get(&id)?;
        if c["kind"] != "subimage" {
            return None;
        }
        let deps: Vec<u64> = c["deps"].as_array()?.iter().filter_map(|d| d.as_u64()).collect();
        images.iter().find(|i| i["id"].as_u64().is_some_and(|v| deps.contains(&(v & 0xffff)))).cloned()
    }

    /// The Texture2D of the package an external image names (`Foo.tga` → `Foo`).
    pub fn texture_for(&self, file_name: &str, cx: &App) -> Option<usize> {
        let stem = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
        let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem).to_ascii_lowercase();
        let studio = self.studio.upgrade()?;
        let s = studio.read(cx);
        let t = s.open.as_ref().filter(|t| t.path == self.path)?;
        (0..t.exports.len()).find(|&i| t.exports[i].class.eq_ignore_ascii_case("texture2d") && t.exports[i].name.eq_ignore_ascii_case(&stem))
    }

    /// Pending value of a text/string/image edit, else the original.
    pub fn value(&self, kind: &str, tag: u64, original: &str) -> String {
        self.edit(&format!("{kind}|{tag}|0")).and_then(|e| e["text"].as_str()).unwrap_or(original).to_string()
    }

    // ----- reading ------------------------------------------------------------------

    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let at = json!({ "path": self.path, "index": self.index, "edits": self.pending() });
        let mut root = at.clone();
        root["sprite"] = json!(0);
        root["limit"] = json!(1);
        let (tree, info, root) = (self.backend.call("swf.tree", at.clone()), self.backend.call("swf.info", at), self.backend.call("swf.timeline", root));
        cx.spawn(async move |this, cx| {
            let (tree, info, root) = (tree.await, info.await, root.await);
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match (tree, info) {
                    (Ok(tree), Ok(info)) => {
                        this.set_tree(tree.json);
                        this.info = info.json;
                        this.root_list = root.ok().and_then(|r| r.json["frames"][0].as_array().cloned()).unwrap_or_default();
                        if let Node::Char(id) = this.node {
                            if !this.chars.contains_key(&id) {
                                this.node = Node::Frames;
                            }
                        }
                        this.timeline_for.clear();
                        this.load_timeline(cx);
                    }
                    (Err(e), _) | (_, Err(e)) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_tree(&mut self, tree: J) {
        self.chars = tree["characters"].as_array().into_iter().flatten().filter_map(|c| Some((c["id"].as_u64()?, c.clone()))).collect();
        self.tree = tree;
    }

    /// Read the current sprite's timeline when the sprite or the edits changed.
    pub fn load_timeline(&mut self, cx: &mut Context<Self>) {
        let Some(sprite) = self.sprite() else {
            self.timeline = None;
            return;
        };
        let key = format!("{sprite}|{}", self.version);
        if key == self.timeline_for && self.timeline.is_some() {
            return;
        }
        self.timeline_for = key.clone();
        let call = self.backend.call("swf.timeline", json!({ "path": self.path, "index": self.index, "sprite": sprite, "edits": self.pending() }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.timeline_for != key {
                    return;
                }
                match r {
                    Ok(t) => {
                        let n = t.json["frames"].as_array().map(|a| a.len()).unwrap_or(1);
                        this.timeline = Some(t.json);
                        if this.frame >= n {
                            this.frame = n.saturating_sub(1);
                        }
                        this.select_reselect();
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not read the timeline: {}", crate::studio::ui::explain_error(&e)), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-read what edits change: the tree (bounds), texts and the timeline.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let v = self.version;
        let at = json!({ "path": self.path, "index": self.index, "edits": self.pending() });
        let (tree, info) = (self.backend.call("swf.tree", at.clone()), self.backend.call("swf.info", at));
        cx.spawn(async move |this, cx| {
            let (tree, info) = (tree.await, info.await);
            let _ = this.update(cx, |this, cx| {
                if v != this.version {
                    return;
                }
                match (tree, info) {
                    (Ok(tree), Ok(info)) => {
                        this.set_tree(tree.json);
                        this.info = info.json;
                    }
                    (Err(e), _) | (_, Err(e)) => this.toast(Tone::Error, format!("The edit does not apply: {}", crate::studio::ui::explain_error(&e)), cx),
                }
                this.load_timeline(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn select_reselect(&mut self) {
        let Some(want) = self.reselect else { return };
        if let Some(e) = self.entries().iter().find(|e| e["character"].as_u64() == Some(want)) {
            self.depth = e["depth"].as_u64();
            self.reselect = None;
        }
    }

    // ----- moving around ------------------------------------------------------------

    pub fn open(&mut self, node: Node, cx: &mut Context<Self>) {
        if node == self.node {
            return;
        }
        self.node = node;
        self.frame = 0;
        self.depth = None;
        self.play = false;
        self.reselect = None;
        self.confirm_remove = false;
        self.field = None;
        self.stage.leave(cx);
        self.load_timeline(cx);
        self.record(cx);
        cx.notify();
    }

    /// Open a sprite of the breadcrumb (0: the root), selecting the clip we came from.
    pub fn go_to(&mut self, id: u64, cx: &mut Context<Self>) {
        let from = self.sprite();
        if Some(id) == from {
            return;
        }
        let trail = self.trail();
        let child = from.and_then(|_| trail.iter().position(|t| t.0 == id).and_then(|i| trail.get(i + 1)).map(|t| t.0));
        self.open(if id == 0 { Node::Frames } else { Node::Char(id) }, cx);
        self.reselect = child;
        self.select_reselect();
    }

    /// Go up one level of the breadcrumb.
    pub fn up(&mut self, cx: &mut Context<Self>) {
        let trail = self.trail();
        if trail.len() >= 2 {
            let parent = trail[trail.len() - 2].0;
            self.go_to(parent, cx);
        }
    }

    /// Edit inside a placed clip (sprites only).
    pub fn enter(&mut self, entry: &J, cx: &mut Context<Self>) -> bool {
        let Some(id) = entry["character"].as_u64() else { return false };
        if self.kind_of(id) != "sprite" {
            return false;
        }
        self.open(Node::Char(id), cx);
        true
    }

    pub fn set_frame(&mut self, f: usize, cx: &mut Context<Self>) {
        self.frame = f.min(self.frames() - 1);
        cx.notify();
    }

    /// Record the view as a place of the window's history (Back walks out of clips).
    fn record(&self, cx: &mut Context<Self>) {
        let (path, index, view) = (self.path.clone(), self.index, self.node.key());
        self.with_studio(cx, move |s, _| s.record(super::Place::Package { package: path, export: Some(index), view: Some(view) }));
    }

    // ----- pending edits ------------------------------------------------------------

    /// Replace the pending edits, keeping the previous ones for `undo_edit`;
    /// quick changes of one edit (typing, a held arrow) are one step.
    fn commit(&mut self, next: Vec<(String, J)>, key: Option<String>, cx: &mut Context<Self>) {
        let now = Instant::now();
        match self.undo.last_mut() {
            Some(last) if key.is_some() && last.1 == key && now.duration_since(last.2).as_millis() < 600 => last.2 = now,
            _ => {
                self.redo.clear();
                self.undo.push((self.edits.clone(), key, now));
                if self.undo.len() > 100 {
                    self.undo.remove(0);
                }
            }
        }
        self.edits = next;
        self.changed(cx);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.version += 1;
        self.detail = None;
        self.refresh(cx);
        self.notify_studio(cx);
        cx.notify();
    }

    /// Take back the last change of the pending edits (false: there is none).
    pub fn undo_edit(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((edits, ..)) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.edits, edits));
        self.changed(cx);
        true
    }

    /// Put back the last change `undo_edit` took back (false: there is none).
    pub fn redo_edit(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(edits) = self.redo.pop() else { return false };
        self.undo.push((std::mem::replace(&mut self.edits, edits), None, Instant::now()));
        self.changed(cx);
        true
    }

    pub fn set_edit(&mut self, edit: J, cx: &mut Context<Self>) {
        let key = edit_key(&edit);
        let mut next = self.edits.clone();
        match next.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = edit,
            None => next.push((key.clone(), edit)),
        }
        self.commit(next, Some(key), cx);
    }

    pub fn drop_edit(&mut self, key: &str, cx: &mut Context<Self>) {
        let next = self.edits.iter().filter(|(k, _)| k != key).cloned().collect();
        self.commit(next, None, cx);
    }

    pub fn discard(&mut self, cx: &mut Context<Self>) {
        if self.edits.is_empty() {
            return;
        }
        self.stage.invalidate();
        if let Some(sv) = &mut self.script_view {
            sv.reload = true;
        }
        self.commit(Vec::new(), None, cx);
    }

    /// Drop every unapplied staged edit (and its local undo/redo history), refreshing the stage.
    pub fn discard_unapplied(&mut self, cx: &mut Context<Self>) {
        if self.edits.is_empty() && self.undo.is_empty() && self.redo.is_empty() {
            return;
        }
        self.edits.clear();
        self.undo.clear();
        self.redo.clear();
        self.version += 1;
        self.detail = None;
        self.stage.invalidate();
        if let Some(sv) = &mut self.script_view {
            sv.reload = true;
        }
        self.refresh(cx);
        self.notify_studio(cx);
        cx.notify();
    }

    /// Merge a change into the pending edit of a placement tag.
    pub fn edit_place(&mut self, sprite: u64, tag: u64, patch: J, cx: &mut Context<Self>) {
        let key = format!("place|{sprite}|{tag}");
        let mut next = self.edit(&key).cloned().unwrap_or_else(|| json!({}));
        for (k, v) in patch.as_object().into_iter().flatten() {
            if k == "matrix" && next["matrix"].is_object() {
                for (mk, mv) in v.as_object().into_iter().flatten() {
                    next["matrix"][mk] = mv.clone();
                }
            } else {
                next[k] = v.clone();
            }
        }
        next["kind"] = json!("place");
        next["sprite"] = json!(sprite);
        next["tag"] = json!(tag);
        self.set_edit(next, cx);
    }

    pub fn move_entry(&mut self, entry: &J, dx: f64, dy: f64, cx: &mut Context<Self>) {
        let Some(s) = self.sprite() else { return };
        if let Some(d) = entry["depth"].as_u64() {
            self.float(d);
        }
        let m = M::from_json(&entry["matrix"]);
        let tag = entry["matrixBy"].as_u64().unwrap_or(0);
        self.edit_place(s, tag, json!({ "matrix": { "tx": geom::twips(m.tx + dx), "ty": geom::twips(m.ty + dy) } }), cx);
    }

    pub fn set_matrix(&mut self, entry: &J, m: M, cx: &mut Context<Self>) {
        let Some(s) = self.sprite() else { return };
        if let Some(d) = entry["depth"].as_u64() {
            self.float(d);
        }
        let tag = entry["matrixBy"].as_u64().unwrap_or(0);
        let m = M { tx: geom::twips(m.tx), ty: geom::twips(m.ty), ..m };
        self.edit_place(s, tag, json!({ "matrix": m.json() }), cx);
    }

    /// Every tag of the current stream that placed or changed this object.
    pub fn tags_of(&self, entry: &J) -> Vec<u64> {
        let mut tags = BTreeSet::new();
        for list in self.timeline.as_ref().and_then(|t| t["frames"].as_array()).into_iter().flatten() {
            for e in list.as_array().into_iter().flatten() {
                if e["depth"] != entry["depth"] || e["placedBy"] != entry["placedBy"] {
                    continue;
                }
                for k in ["placedBy", "matrixBy", "changedBy"] {
                    tags.extend(e[k].as_u64());
                }
            }
        }
        tags.into_iter().collect()
    }

    /// Remove the object: its placing tag and every later change of it.
    pub fn remove_entry(&mut self, entry: &J, cx: &mut Context<Self>) {
        let Some(s) = self.sprite() else { return };
        let mut next = self.edits.clone();
        for tag in self.tags_of(entry) {
            next.retain(|(k, _)| *k != format!("place|{s}|{tag}"));
            let d = json!({ "kind": "deletePlace", "sprite": s, "tag": tag });
            let key = edit_key(&d);
            if !next.iter().any(|(k, _)| *k == key) {
                next.push((key, d));
            }
        }
        self.depth = None;
        self.commit(next, None, cx);
    }

    pub fn entry_edited(&self, entry: &J) -> bool {
        let Some(s) = self.sprite() else { return false };
        self.tags_of(entry).iter().any(|t| self.is_edited(&format!("place|{s}|{t}")))
    }

    /// Drop the pending edits of one object.
    pub fn revert_entry(&mut self, entry: &J, cx: &mut Context<Self>) {
        let Some(s) = self.sprite() else { return };
        let keys: Vec<String> = self.tags_of(entry).iter().map(|t| format!("place|{s}|{t}")).collect();
        let next = self.edits.iter().filter(|(k, _)| !keys.contains(k)).cloned().collect();
        self.commit(next, None, cx);
    }

    // ----- shapes, texts, fonts, scripts ----------------------------------------------

    pub fn set_shape_style(&mut self, tag: u64, key: &str, value: String, cx: &mut Context<Self>) {
        self.set_edit(json!({ "kind": "shapeColor", "tag": tag, "key": key, "value": value }), cx);
    }

    /// Redraw a shape from an SVG (its colour edits no longer apply).
    pub fn replace_shape(&mut self, tag: u64, svg: String, fit: bool, cx: &mut Context<Self>) {
        let prefix = format!("shapeColor|{tag}|");
        let mut next: Vec<(String, J)> = self.edits.iter().filter(|(k, _)| !k.starts_with(&prefix)).cloned().collect();
        let key = format!("shapeSvg|{tag}");
        next.retain(|(k, _)| *k != key);
        next.push((key, json!({ "kind": "shapeSvg", "tag": tag, "svg": svg, "fit": fit })));
        self.commit(next, None, cx);
    }

    pub fn set_edit_text(&mut self, tag: u64, props: J, cx: &mut Context<Self>) {
        let mut merged = self.edit(&format!("editText|{tag}")).map(|e| e["props"].clone()).filter(|p| p.is_object()).unwrap_or_else(|| json!({}));
        for (k, v) in props.as_object().into_iter().flatten() {
            merged[k] = v.clone();
        }
        self.set_edit(json!({ "kind": "editText", "tag": tag, "props": merged }), cx);
    }

    pub fn set_static_text(&mut self, tag: u64, record: u64, change: J, cx: &mut Context<Self>) {
        let mut e = self.edit(&format!("staticText|{tag}|{record}")).cloned().unwrap_or_else(|| json!({}));
        for (k, v) in change.as_object().into_iter().flatten() {
            e[k] = v.clone();
        }
        e["kind"] = json!("staticText");
        e["tag"] = json!(tag);
        e["record"] = json!(record);
        self.set_edit(e, cx);
    }

    pub fn script_edit(&self, loc: &J) -> Option<&J> {
        self.edit(&format!("script|{}", script_key(loc)))
    }

    pub fn set_script(&mut self, loc: &J, text: String, pcode: bool, cx: &mut Context<Self>) {
        let e = if pcode { json!({ "kind": "pcode", "loc": loc, "pcode": text }) } else { json!({ "kind": "script", "loc": loc, "source": text }) };
        self.set_edit(e, cx);
    }

    pub fn open_script(&mut self, key: String, cx: &mut Context<Self>) {
        self.open(Node::Scripts, cx);
        self.script = Some(key);
        cx.notify();
    }

    // ----- images -------------------------------------------------------------------

    /// Pick a PNG/DDS and store it in the image's texture (undoable).
    pub fn replace_image(&mut self, texture: usize, image: J, cx: &mut Context<Self>) {
        let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Replace the image (PNG or DDS)".into()) });
        let (backend, path) = (self.backend.clone(), self.path.clone());
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut f))) = files.await else { return };
            let Some(file) = f.pop() else { return };
            let r = backend.call("texture.import", json!({ "path": path, "index": texture, "file": file.to_string_lossy(), "keepAlpha": true, "padBlocks": true })).await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(t) => {
                    this.mark_dirty(false, cx);
                    this.thumbs.clear();
                    this.stage.invalidate();
                    let (sw, sh) = (t.json["source"]["width"].as_f64().unwrap_or(0.0), t.json["source"]["height"].as_f64().unwrap_or(0.0));
                    let (iw, ih) = (image["width"].as_f64().unwrap_or(1.0), image["height"].as_f64().unwrap_or(1.0));
                    let stretched = ((sw / sh.max(1.0)) / (iw / ih.max(1.0)) - 1.0).abs() > 0.01;
                    let tone = if stretched { Tone::Warning } else { Tone::Success };
                    this.toast(tone, format!("Image replaced: {sw}×{sh} {}; the movie draws it at {iw}×{ih}{}. Kept in your mod.", t.json["format"].as_str().unwrap_or(""), if stretched { ", so it is stretched (another aspect ratio)" } else { "" }), cx);
                    cx.notify();
                }
                Err(e) => this.toast(Tone::Error, format!("Could not replace the image: {}", crate::studio::ui::explain_error(&e)), cx),
            });
        })
        .detach();
    }

    /// Write the image's texture as PNG or DDS.
    pub fn export_image(&mut self, texture: usize, image: &J, ext: &'static str, cx: &mut Context<Self>) {
        let file = image["fileName"].as_str().unwrap_or("image");
        let stem = file.rsplit(['/', '\\']).next().unwrap_or(file);
        let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem).to_string();
        let target = cx.prompt_for_new_path(&documents(), Some(&format!("{}.{ext}", safe(&stem))));
        let (backend, path) = (self.backend.clone(), self.path.clone());
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(out))) = target.await else { return };
            let r = backend.call("texture.export", json!({ "path": path, "index": texture, "out": out.to_string_lossy() })).await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => this.toast(Tone::Success, format!("{} bytes written to {}", r.json["bytes"], r.json["out"].as_str().unwrap_or("")), cx),
                Err(e) => this.toast(Tone::Error, format!("Could not export the image: {}", crate::studio::ui::explain_error(&e)), cx),
            });
        })
        .detach();
    }

    /// Show a texture of the package in the object list.
    pub fn go_to_texture(&self, texture: usize, cx: &mut Context<Self>) {
        self.with_studio(cx, move |s, cx| {
            s.reveal(texture, cx);
            s.user_select_export(texture, cx);
        });
    }

    pub fn open_add_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = |ph: &str, v: &str, window: &mut Window, cx: &mut Context<Self>| {
            let (ph, v) = (ph.to_string(), v.to_string());
            cx.new(|cx| {
                let mut s = InputState::new(window, cx).placeholder(ph);
                s.set_value(v, window, cx);
                s
            })
        };
        let name_input = input("texture name (default <movie>_<file name>)", "", window, cx);
        name_input.update(cx, |s, cx| s.focus(window, cx));
        self.adding_image = Some(AddImage {
            file: None,
            name: name_input,
            instance: input("instance name (optional, for scripts)", "", window, cx),
            x: input("x", "0", window, cx),
            y: input("y", "0", window, cx),
            shape: true,
            place: true,
            working: false,
        });
        cx.notify();
    }

    /// Add an external image (and by default a shape drawing it, placed on the
    /// open frame): pending edits are written with it, as one undo step.
    pub fn add_image(&mut self, cx: &mut Context<Self>) {
        let (sprite, frame, path, index, edits) = (self.sprite(), self.frame, self.path.clone(), self.index, self.pending());
        let Some(a) = &mut self.adding_image else { return };
        let Some(file) = a.file.clone() else { return };
        a.working = true;
        let name = a.name.read(cx).value().trim().to_string();
        let num = |i: &Entity<InputState>, cx: &App| i.read(cx).value().trim().parse::<f64>().unwrap_or(0.0);
        let place = (a.shape && a.place && sprite.is_some()).then(|| {
            let inst = a.instance.read(cx).value().trim().to_string();
            json!({ "sprite": sprite, "frame": frame, "x": num(&a.x, cx), "y": num(&a.y, cx), "name": if inst.is_empty() { J::Null } else { J::from(inst) } })
        });
        let params = json!({ "path": path, "index": index, "file": file, "name": if name.is_empty() { J::Null } else { J::from(name) }, "shape": a.shape, "place": place, "edits": edits });
        let call = self.backend.call("swf.addImage", params);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.adding_image = None;
                    this.written(cx);
                    this.mark_dirty(true, cx);
                    let j = &r.json;
                    if let Some(d) = j["placed"]["depth"].as_u64() {
                        this.depth = Some(d);
                    } else if let Some(id) = j["shape"].as_u64().or(j["image"].as_u64()) {
                        this.open(Node::Char(id), cx);
                    }
                    let placed = j["placed"]["depth"].as_u64().map(|d| format!(", placed at depth {d}")).unwrap_or_default();
                    this.toast(Tone::Success, format!("Image added: {} ({}×{}) → {}{placed}. Kept in your mod.", j["fileName"].as_str().unwrap_or(""), j["width"], j["height"], j["texture"]["path"].as_str().unwrap_or("")), cx);
                    this.load(cx);
                }
                Err(e) => {
                    if let Some(a) = &mut this.adding_image {
                        a.working = false;
                    }
                    this.toast(Tone::Error, format!("Could not add the image: {}", crate::studio::ui::explain_error(&e)), cx);
                }
            });
        })
        .detach();
    }

    /// The pending edits were written into the package, whose undo takes over.
    fn written(&mut self, cx: &mut Context<Self>) {
        self.edits.clear();
        self.undo.clear();
        self.redo.clear();
        self.version += 1;
        self.stage.invalidate();
        self.notify_studio(cx);
        cx.notify();
    }

    /// Write the pending edits into the package (one undo step).
    pub fn apply(&mut self, cx: &mut Context<Self>) {
        if self.edits.is_empty() || self.applying {
            return;
        }
        self.applying = true;
        let call = self.backend.call("swf.apply", json!({ "path": self.path, "index": self.index, "edits": self.pending() }));
        self.notify_studio(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.applying = false;
                match r {
                    Ok(r) => {
                        this.mark_dirty(false, cx);
                        this.written(cx);
                        let n = r.json["edits"].as_u64().unwrap_or(0);
                        this.toast(Tone::Success, format!("Movie updated: {n} edit{}, {} bytes in your mod", if n == 1 { "" } else { "s" }, r.json["bytes"]), cx);
                        if let Some(sv) = &mut this.script_view {
                            sv.reload = true;
                        }
                        this.load(cx);
                    }
                    Err(e) => {
                        this.notify_studio(cx);
                        this.toast(Tone::Error, format!("Could not apply the changes: {}", crate::studio::ui::explain_error(&e)), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The package changed under the editor (undo, discard, a saved recompile): read the movie again.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.stage.invalidate();
        self.detail = None;
        self.thumbs.clear();
        if let Some(sv) = &mut self.script_view {
            sv.reload = true;
        }
        self.load(cx);
    }

    // ----- inline fields ------------------------------------------------------------

    /// A value that turns into an input when clicked; Enter commits it (`commit_field`).
    pub fn field(&self, key: String, value: String, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if let Some(f) = self.field.as_ref().filter(|f| f.key == key) {
            return div().w(px(width)).child(Input::new(&f.input).xsmall()).into_any_element();
        }
        let (k_key, v_key) = (key.clone(), value.clone());
        let v = value.clone();
        div()
            .id(gpui::SharedString::from(format!("f-{key}")))
            .focusable()
            .tab_index(0)
            .w(px(width))
            .px_1()
            .rounded_sm()
            .border_1()
            .border_color(gpui::transparent_black())
            .bg(theme.secondary)
            .cursor_text()
            .font_family(crate::studio::ui::mono())
            .text_xs()
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap()
            .focus_visible(|s| s.border_color(theme.ring))
            .child(if value.is_empty() { "—".to_string() } else { value })
            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.start_field(k_key.clone(), v_key.clone(), window, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.start_field(key.clone(), v.clone(), window, cx)))
            .into_any_element()
    }

    pub fn start_field(&mut self, key: String, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let from = value.clone();
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
            }
            InputEvent::Blur if this.field.as_ref().is_some_and(|f| f.key == k) => {
                let v = input.read(cx).value().to_string();
                let changed = this.field.take().is_some_and(|f| v != f.from);
                if changed {
                    this.commit_field(&k, v, cx);
                } else {
                    cx.notify();
                }
            }
            _ => {}
        });
        self.field = Some(Field { key, from, input, _sub: sub });
        cx.notify();
    }

    /// A typed value: `p:<part>` (the selected placement), `text:<tag>`, `image:<tag>`,
    /// `et:<tag>:<prop>`, `st:<tag>:<record>`, `col:<tag>:<key>`, `alpha:<tag>:<key>`, `lw:<tag>:<key>`.
    fn commit_field(&mut self, key: &str, value: String, cx: &mut Context<Self>) {
        let v = value.trim().to_string();
        let num = || v.trim_end_matches(['%', '°']).trim().replace(',', ".").parse::<f64>().ok();
        let parts: Vec<&str> = key.splitn(3, ':').collect();
        match parts.as_slice() {
            ["p", part] => {
                let Some(e) = self.selected_entry() else { return };
                match *part {
                    "name" => {
                        let s = self.sprite().unwrap_or(0);
                        let tag = e["placedBy"].as_u64().unwrap_or(0);
                        self.edit_place(s, tag, json!({ "name": if v.is_empty() { J::Null } else { J::from(v) } }), cx);
                    }
                    "alpha" => {
                        if let Some(a) = num() {
                            self.set_alpha(&e, a, cx);
                        }
                    }
                    p => {
                        if let Some(n) = num() {
                            self.set_part(&e, p, n, cx);
                        }
                    }
                }
            }
            ["text", tag] => self.set_edit(json!({ "kind": "text", "tag": tag.parse::<u64>().unwrap_or(0), "text": value }), cx),
            ["image", tag] => self.set_edit(json!({ "kind": "image", "tag": tag.parse::<u64>().unwrap_or(0), "text": v }), cx),
            ["et", tag, prop] => {
                let tag = tag.parse::<u64>().unwrap_or(0);
                let val = match *prop {
                    "variable" => J::from(v),
                    "maxLength" if v.is_empty() => J::Null,
                    "bx" | "by" | "bw" | "bh" => {
                        let Some(n) = num() else { return };
                        let mut b = self.detail_props(tag).map(|p| p["bounds"].clone()).unwrap_or_else(|| json!({}));
                        b[&prop[1..]] = json!(n);
                        self.set_edit_text(tag, json!({ "bounds": b }), cx);
                        return;
                    }
                    _ => match num() {
                        Some(n) => json!(n),
                        None => return,
                    },
                };
                self.set_edit_text(tag, json!({ *prop: val }), cx);
            }
            ["st", tag, record] => self.set_static_text(tag.parse().unwrap_or(0), record.parse().unwrap_or(0), json!({ "text": value }), cx),
            ["col", tag, k] => {
                let hex = if v.starts_with('#') { v.clone() } else { format!("#{v}") };
                let Some(rgb) = parse_hex(&hex) else {
                    self.toast(Tone::Warning, "A colour is #rrggbb", cx);
                    return;
                };
                self.set_color(tag.parse().unwrap_or(0), k, Some(rgb), None, cx);
            }
            ["alpha", tag, k] => {
                if let Some(a) = num() {
                    self.set_color(tag.parse().unwrap_or(0), k, None, Some(a), cx);
                }
            }
            ["lw", tag, k] => {
                if let Some(n) = num() {
                    self.set_shape_style(tag.parse().unwrap_or(0), k, n.to_string(), cx);
                }
            }
            _ => {}
        }
    }

    /// The edit-text properties `swf.character` reported for `tag`.
    fn detail_props(&self, tag: u64) -> Option<J> {
        let (_, _, Ok(d)) = self.detail.as_ref()? else { return None };
        let c = self.character()?;
        (c["tag"].as_u64() == Some(tag)).then(|| d["props"].clone())
    }

    /// Change one part of a placement (scale keeps the proportions when locked).
    pub fn set_part(&mut self, e: &J, part: &str, v: f64, cx: &mut Context<Self>) {
        let d = geom::decompose(&M::from_json(&e["matrix"]));
        let mut next = d;
        match part {
            "x" => next.x = v,
            "y" => next.y = v,
            "rot" => next.rot = v,
            "skew" => next.skew = v,
            "sx" | "sy" => {
                let s = v / 100.0;
                if self.lock_scale && d.sx != 0.0 && d.sy != 0.0 {
                    let ratio = if part == "sx" { s / d.sx } else { s / d.sy };
                    next.sx = d.sx * ratio;
                    next.sy = d.sy * ratio;
                } else if part == "sx" {
                    next.sx = s;
                } else {
                    next.sy = s;
                }
            }
            _ => return,
        }
        self.set_matrix(e, geom::compose(&next), cx);
    }

    pub fn set_alpha(&mut self, e: &J, percent: f64, cx: &mut Context<Self>) {
        let Some(s) = self.sprite() else { return };
        if let Some(d) = e["depth"].as_u64() {
            self.float(d);
        }
        let base = if e["cxform"].is_object() { e["cxform"].clone() } else { json!({ "mul": [1.0, 1.0, 1.0, 1.0], "add": [0, 0, 0, 0] }) };
        let mut mul: Vec<f64> = (0..4).map(|i| base["mul"][i].as_f64().unwrap_or(1.0)).collect();
        mul[3] = (percent / 100.0).clamp(0.0, 1.0);
        let add: Vec<f64> = (0..4).map(|i| base["add"][i].as_f64().unwrap_or(0.0)).collect();
        let identity = mul.iter().all(|m| *m == 1.0) && add.iter().all(|a| *a == 0.0);
        let tag = e["changedBy"].as_u64().unwrap_or(0);
        self.edit_place(s, tag, json!({ "cxform": if identity { J::Null } else { json!({ "mul": mul, "add": add }) } }), cx);
    }

    /// A shape colour (`#rrggbbaa`): a new RGB and/or alpha (percent) over the current one.
    pub fn set_color(&mut self, tag: u64, key: &str, rgb: Option<[u8; 3]>, alpha: Option<f64>, cx: &mut Context<Self>) {
        let current = self.current_color(tag, key).unwrap_or_else(|| "#000000ff".into());
        let cur = parse_hex(&current).unwrap_or([0, 0, 0]);
        let a = alpha.map(|p| (p.clamp(0.0, 100.0) / 100.0 * 255.0).round() as u8).unwrap_or_else(|| u8::from_str_radix(current.get(7..9).unwrap_or("ff"), 16).unwrap_or(255));
        let [r, g, b] = rgb.unwrap_or(cur);
        let value = format!("#{r:02x}{g:02x}{b:02x}{a:02x}");
        if let Some(rec) = key.strip_prefix("record:") {
            self.set_static_text(tag, rec.parse().unwrap_or(0), json!({ "color": value }), cx);
        } else if key == "editText" {
            self.set_edit_text(tag, json!({ "color": value }), cx);
        } else {
            self.set_shape_style(tag, key, value, cx);
        }
    }

    /// The colour a key has now (pending edits included), from the character's details.
    fn current_color(&self, tag: u64, key: &str) -> Option<String> {
        let (_, _, Ok(d)) = self.detail.as_ref()? else { return None };
        if let Some(rec) = key.strip_prefix("record:") {
            return d["records"][rec.parse::<usize>().ok()?]["color"].as_str().map(str::to_string);
        }
        if key == "editText" {
            return d["props"]["color"].as_str().map(str::to_string);
        }
        let _ = tag;
        d["colors"].as_array().into_iter().flatten().chain(d["lines"].as_array().into_iter().flatten()).find_map(|c| {
            if c["key"].as_str() == Some(key) || c["colorKey"].as_str() == Some(key) {
                c["color"].as_str().map(str::to_string)
            } else {
                None
            }
        })
    }

    // ----- drawing ------------------------------------------------------------------

    fn render_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let sep = || div().w(px(1.0)).h(px(14.0)).mx_0p5().bg(theme.border);
        let trail = self.trail();
        let ring = theme.ring;
        let mut bar = crate::studio::ui::toolbar(&theme).py_1().gap_1p5().flex_wrap().items_center().text_xs();
        if trail.len() > 1 || (trail.len() == 1 && self.node == Node::Frames) {
            let mut crumbs = h_flex().gap_1().min_w_0().items_center();
            let n = trail.len();
            for (k, (id, label)) in trail.into_iter().enumerate() {
                if k > 0 {
                    crumbs = crumbs.child(Icon::new(IconName::ChevronRight).xsmall().text_color(theme.muted_foreground));
                }
                crumbs = crumbs.child(if k + 1 == n {
                    div().font_weight(gpui::FontWeight::SEMIBOLD).child(label).into_any_element()
                } else {
                    div()
                        .id(("crumb", k))
                        .focusable()
                        .tab_index(0)
                        .px_0p5()
                        .rounded_sm()
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .focus_visible(move |s| s.border_color(ring))
                        .cursor_pointer()
                        .text_color(theme.link)
                        .hover(|s| s.underline())
                        .child(label)
                        .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                            if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.go_to(id, cx);
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| this.go_to(id, cx)))
                        .into_any_element()
                });
            }
            bar = bar.child(crumbs);
        } else {
            bar = bar.child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(self.title()));
        }
        let info = &self.info;
        let char_count = self.chars.len();
        bar = bar.child(div().font_family(crate::studio::ui::mono()).text_color(theme.muted_foreground).child(format!(
            "{} v{} · {}×{} · {} fps · {}",
            info["signature"].as_str().unwrap_or(""),
            info["version"],
            info["width"].as_f64().unwrap_or(0.0),
            info["height"].as_f64().unwrap_or(0.0),
            info["frameRate"].as_f64().unwrap_or(0.0),
            if char_count == 1 { "1 character".to_string() } else { format!("{char_count} characters") }
        )));
        bar = bar.child(div().flex_1());
        let (show_tree, show_props, _, _, _) = self.shown_side_panes(window, cx);
        let focused = self.studio.upgrade().is_some_and(|s| s.read(cx).focus_mode);
        let toggle = |b: Button, on: bool| b.ghost().selected(on);
        let target = self.target();
        // Group 1: Panes & Focus (≤ 3 items).
        bar = bar.child(
            h_flex()
                .gap_0p5()
                .items_center()
                .child(toggle(Button::new("mv-toggle-tree").xsmall().icon(Icon::new(IconName::PanelLeft)).tooltip(if show_tree { "Hide the movie resource tree" } else { "Show the movie resource tree" }), show_tree).on_click(cx.listener(move |this, _, _, cx| {
                    this.hide_tree = show_tree;
                    this.tree_toggled = true;
                    cx.notify();
                })))
                .when(target.is_some(), |b| {
                    b.child(toggle(Button::new("mv-toggle-props").xsmall().icon(Icon::new(IconName::SlidersHorizontal)).tooltip(if show_props { "Hide the placement & character inspector" } else { "Show the placement & character inspector" }), show_props).on_click(cx.listener(move |this, _, _, cx| {
                        this.hide_props = show_props;
                        this.props_toggled = true;
                        cx.notify();
                    })))
                })
                .child(toggle(Button::new("mv-focus").xsmall().label("Focus").tooltip(if focused { "Show the package and object lists again" } else { "Give the movie editor the whole window (hides the package and object lists)" }), focused).on_click(cx.listener(|this, _, _, cx| {
                    this.with_studio(cx, |s, cx| {
                        s.focus_mode = !s.focus_mode;
                        cx.notify();
                    });
                }))),
        );
        // Group 2: Stage tools & Zoom (≤ 4 items).
        if target.is_some() {
            let zoom_val = self.stage.zoom;
            let zoom = if zoom_val == 0.0 { "Fit".to_string() } else { format!("{}%", (zoom_val * 100.0).round()) };
            let me_zoom = cx.entity().downgrade();
            let mut tools = h_flex().gap_0p5().items_center();
            if self.layout() {
                tools = tools
                    .child(toggle(Button::new("mv-move").xsmall().icon(Icon::new(IconName::MousePointer2)).tooltip("Move tool (V): drag the selection, arrows nudge"), self.tool == Tool::Move).on_click(cx.listener(|this, _, _, cx| {
                        this.tool = Tool::Move;
                        cx.notify();
                    })))
                    .child(toggle(Button::new("mv-transform").xsmall().icon(Icon::new(IconName::Scan)).tooltip("Free transform (Q): handles scale (Shift keeps proportions, Alt from the centre), the knob rotates (Shift: 15° steps)"), self.tool == Tool::Transform).on_click(cx.listener(|this, _, _, cx| {
                        this.tool = Tool::Transform;
                        cx.notify();
                    })));
            }
            if self.sprite().is_some_and(|s| s != 0) {
                let playing = self.play;
                tools = tools.child(
                    toggle(
                        Button::new("mv-playclip")
                            .xsmall()
                            .icon(Icon::new(if playing { IconName::Pause } else { IconName::Play }))
                            .label(if playing { "Stop" } else { "Play" })
                            .tooltip(if playing { "Return to timeline layout" } else { "Run the sprite with its timeline and scripts" }),
                        playing,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_play(!playing, cx))),
                );
            }
            tools = tools.child(
                Button::new("mv-zoom")
                    .xsmall()
                    .ghost()
                    .label(zoom)
                    .dropdown_caret(true)
                    .tooltip("Stage zoom (⌘+wheel zooms, Space+drag pans)")
                    .dropdown_menu(move |menu, _, _| {
                        let (m_fit, m_in, m_out) = (me_zoom.clone(), me_zoom.clone(), me_zoom.clone());
                        let mut menu = menu
                            .item(PopupMenuItem::new("Fit to view").checked(zoom_val == 0.0).on_click(move |_, _, cx| {
                                let _ = m_fit.update(cx, |this, cx| {
                                    this.stage.fit();
                                    cx.notify();
                                });
                            }))
                            .separator();
                        for (pct, factor) in [(50, 0.5f64), (100, 1.0), (200, 2.0)] {
                            let m = me_zoom.clone();
                            menu = menu.item(PopupMenuItem::new(format!("{pct}%")).checked((zoom_val - factor).abs() < 0.01).on_click(move |_, _, cx| {
                                let _ = m.update(cx, |this, cx| {
                                    this.stage.zoom = factor;
                                    cx.notify();
                                });
                            }));
                        }
                        menu.separator()
                            .item(PopupMenuItem::new("Zoom in (⌘+wheel)").on_click(move |_, _, cx| {
                                let _ = m_in.update(cx, |this, cx| this.stage.zoom_by(1.25, cx));
                            }))
                            .item(PopupMenuItem::new("Zoom out").on_click(move |_, _, cx| {
                                let _ = m_out.update(cx, |this, cx| this.stage.zoom_by(0.8, cx));
                            }))
                    }),
            );
            bar = bar.child(sep()).child(tools);
        }
        // Group 3: Secondary stage, image & export actions ("Stage & export").
        {
            let me = cx.entity().downgrade();
            let is_layout = self.layout();
            let show_bounds = self.show_bounds;
            let is_subsprite = self.sprite().is_some_and(|s| s != 0);
            let is_play = self.play;
            let has_target = target.is_some();
            let export_target = target.clone().filter(|_| !self.play);
            let frames = (self.sprite().is_some() && self.frames() > 1).then(|| (self.sprite().unwrap_or(0), self.frames()));
            bar = bar.child(sep()).child(
                Button::new("mv-stage-menu")
                    .xsmall()
                    .ghost()
                    .label("Stage & export")
                    .dropdown_caret(true)
                    .tooltip("Bounds, layout mode, stage framing, image import and SVG/PNG export")
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        let mut has_view_items = false;
                        if is_layout {
                            let m = me.clone();
                            menu = menu.item(PopupMenuItem::new("Show object bounds").checked(show_bounds).on_click(move |_, _, cx| {
                                let _ = m.update(cx, |this, cx| {
                                    this.show_bounds = !this.show_bounds;
                                    cx.notify();
                                });
                            }));
                            has_view_items = true;
                        }
                        if is_subsprite {
                            let (m1, m2) = (me.clone(), me.clone());
                            menu = menu
                                .item(PopupMenuItem::new("Layout mode (no scripts)").checked(!is_play).on_click(move |_, _, cx| {
                                    let _ = m1.update(cx, |this, cx| this.set_play(false, cx));
                                }))
                                .item(PopupMenuItem::new("Play sprite with scripts").checked(is_play).on_click(move |_, _, cx| {
                                    let _ = m2.update(cx, |this, cx| this.set_play(true, cx));
                                }));
                            has_view_items = true;
                        }
                        if has_target {
                            let m = me.clone();
                            menu = menu.item(PopupMenuItem::new("Fit and centre stage").on_click(move |_, _, cx| {
                                let _ = m.update(cx, |this, cx| {
                                    this.stage.fit();
                                    cx.notify();
                                });
                            }));
                            has_view_items = true;
                        }
                        if has_view_items {
                            menu = menu.separator();
                        }
                        let m_img = me.clone();
                        menu = menu.item(PopupMenuItem::new("Add image…").on_click(move |_, window, cx| {
                            let _ = m_img.update(cx, |this, cx| this.open_add_image(window, cx));
                        }));
                        if let Some(t) = export_target.clone() {
                            let (m_svg, t_svg) = (me.clone(), t.clone());
                            menu = menu.separator().item(PopupMenuItem::new("Export SVG (vectors)…").on_click(move |_, _, cx| {
                                let _ = m_svg.update(cx, |this, cx| this.export_svg(t_svg.clone(), cx));
                            }));
                            for k in [1u32, 2, 4] {
                                let (m_png, t_png) = (me.clone(), t.clone());
                                menu = menu.item(PopupMenuItem::new(format!("Export PNG {k}×…")).on_click(move |_, _, cx| {
                                    let _ = m_png.update(cx, |this, cx| this.export_png(t_png.clone(), k, cx));
                                }));
                            }
                            if let Some((sprite, n)) = frames {
                                let m_fr = me.clone();
                                menu = menu.item(PopupMenuItem::new(if n == 1 { "Export frame as PNG…".to_string() } else { format!("Export all {n} frames as PNGs…") }).on_click(move |_, _, cx| {
                                    let _ = m_fr.update(cx, |this, cx| this.export_frames(sprite, 1, cx));
                                }));
                            }
                        }
                        menu
                    }),
            );
        }
        // Group 4: Staged pending edits (or Mod view jump once applied).
        let pending = self.edits.len();
        let dirty = self.studio.upgrade().is_some_and(|s| s.read(cx).dirty.contains(&self.path));
        if pending > 0 {
            bar = bar.child(sep()).child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(crate::studio::ui::badge(Some(IconName::TriangleAlert), format!("{pending} unapplied"), theme.warning, &theme))
                    .child(Button::new("mv-discard").xsmall().ghost().label("Discard").tooltip("Drop the pending edits").on_click(cx.listener(|this, _, _, cx| this.discard(cx))))
                    .child(
                        Button::new("mv-apply")
                            .xsmall()
                            .label(if self.applying { "Applying…".to_string() } else { format!("Apply {pending} edit{}", if pending > 1 { "s" } else { "" }) })
                            .disabled(self.applying)
                            .tooltip(if self.applying { "Applying pending edits…" } else { "Keep these edits in your mod (one undo step)" })
                            .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
                    ),
            );
        } else if dirty {
            bar = bar.child(sep()).child(Button::new("mv-mod").xsmall().label("Build mod").tooltip("Open Mod: your package changes ship as a delta (only the changed objects)").on_click(cx.listener(|this, _, window, cx| {
                let studio = this.studio.clone();
                window.defer(cx, move |window, cx| {
                    let _ = studio.update(cx, |s, cx| s.set_view(super::View::Mod, window, cx));
                });
            })));
        }
        bar
    }

    fn title(&self) -> String {
        match self.node {
            Node::Movie => "Movie".into(),
            Node::Frames => "Root timeline".into(),
            Node::Texts => "Edit texts".into(),
            Node::Scripts => "Scripts".into(),
            Node::Images => "External images".into(),
            Node::Linkage => "Linkage".into(),
            Node::Tags => "All tags".into(),
            Node::Char(id) => match self.chars.get(&id) {
                Some(c) => format!("{} {id}{}", c["kind"].as_str().unwrap_or("?"), c["names"][0].as_str().map(|n| format!(" · {n}")).unwrap_or_default()),
                None => format!("#{id}"),
            },
        }
    }

    /// What the stage previews: `swf.preview`'s target.
    pub fn target(&self) -> Option<J> {
        match self.node {
            Node::Frames => Some(json!({ "kind": "root", "frame": self.frame })),
            Node::Char(id) => {
                let c = self.chars.get(&id)?;
                if c["kind"] == "sprite" {
                    Some(if self.play { json!({ "kind": "character", "id": id, "play": true }) } else { json!({ "kind": "frame", "id": id, "frame": self.frame }) })
                } else {
                    Some(json!({ "kind": "character", "id": id }))
                }
            }
            _ => None,
        }
    }

    /// The stage shows a timeline layout (objects can be picked and moved).
    pub fn layout(&self) -> bool {
        self.sprite().is_some() && !self.play
    }

    fn set_play(&mut self, play: bool, cx: &mut Context<Self>) {
        if self.play != play {
            self.play = play;
            self.depth = None;
            self.stage.leave(cx);
            cx.notify();
        }
    }
}

impl Render for MovieEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if let (Some(e), true) = (&self.error, self.tree.is_null()) {
            return div().p_3().text_sm().whitespace_normal().text_color(theme.danger).child(crate::studio::ui::explain_error(e)).into_any_element();
        }
        if self.tree.is_null() || self.info.is_null() {
            return div().p_3().text_sm().text_color(theme.muted_foreground).child("Reading the movie…").into_any_element();
        }
        let (show_tree, show_props, tree_w, props_w, capped) = self.shown_side_panes(window, cx);
        let toolbar = self.render_toolbar(window, cx);
        let tree: AnyElement = if show_tree { self.render_tree(window, cx) } else { div().into_any_element() };
        let center: AnyElement = match self.node {
            Node::Movie => v_flex()
                .size_full()
                .p_1()
                .child(div().px_1().pb_1().text_xs().text_color(theme.muted_foreground).child("The whole movie, played by Ruffle with its scripts (pending edits included): the package's textures are embedded and imported shared movies (SharedAssets, fonts, image stacks) come from the game folder. Scaleform-only features can still differ from the game."))
                .child(div().flex_1().min_h_0().child(self.render_stage(window, cx)))
                .into_any_element(),
            n if n.section() => self.render_section(window, cx),
            _ if self.target().is_some() => {
                if self.layout() {
                    v_resizable("mv-center")
                        .child(resizable_panel().child(
                            v_flex()
                                .size_full()
                                .child(div().px_2().py_0p5().border_b_1().border_color(theme.border).text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child("Click or drag clips on stage to move/scale (Q: handles, double-click: edit inside); click Apply edits in the toolbar to keep changes in your mod."))
                                .child(div().flex_1().min_h_0().child(self.render_stage(window, cx))),
                        ))
                        .child(resizable_panel().size(px(self.panes[2])).child(self.render_timeline(window, cx)))
                        .on_resize(self.pane_saver(2, show_tree, show_props, capped))
                        .into_any_element()
                } else {
                    self.render_stage(window, cx)
                }
            }
            _ => div().into_any_element(),
        };
        let group_key = (show_tree as usize) * 1024 + (show_props as usize) * 512 + if capped { (tree_w as usize).min(255) * 2 + usize::from(props_w > 0.0) } else { 0 };
        let mut layout = h_resizable(("mv-main", group_key))
            .child(resizable_panel().visible(show_tree).size(px(tree_w)).flex_none().child(tree))
            .child(resizable_panel().flex_basis(px(0.0)).overflow_hidden().child(div().min_w(px(260.0)).size_full().child(center)))
            .on_resize(self.pane_saver(0, show_tree, show_props, capped));
        if self.target().is_some() {
            let props: AnyElement = if show_props { self.render_props(window, cx) } else { div().into_any_element() };
            layout = layout.child(resizable_panel().visible(show_props).size(px(props_w)).flex_none().child(props));
        }
        let dialog = self.render_add_image(window, cx);
        v_flex()
            .id("movie-editor")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .on_action(cx.listener(|this, _: &super::Cancel, window, cx| {
                if this.field.take().is_some() {
                    cx.notify();
                } else if this.adding_image.is_some() {
                    if this.adding_image.as_ref().is_some_and(|a| !a.working) {
                        this.adding_image = None;
                        if this.target().is_some() {
                            this.stage.focus.focus(window, cx);
                        } else {
                            this.focus.focus(window, cx);
                        }
                        cx.notify();
                    }
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &super::ZoomReset, window, cx| {
                if this.stage.focus.is_focused(window) && this.target().is_some() {
                    this.stage.fit();
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(layout))
            .children(dialog)
            .into_any_element()
    }
}

impl MovieEditor {
    fn render_add_image(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let a = self.adding_image.as_ref()?;
        let where_ = self.sprite().map(|s| format!("{}, frame {}", if s == 0 { "the root timeline".to_string() } else { self.label(s) }, self.frame + 1));
        let file = a.file.clone();
        let check = |id: &'static str, label: String, on: bool| gpui_component::checkbox::Checkbox::new(id).label(label).checked(on);
        let mut body = v_flex()
            .gap_3()
            .text_sm()
            .child(h_flex().gap_2().child(div().flex_1().font_family(crate::studio::ui::mono()).text_xs().child(file.clone().unwrap_or_else(|| "No file yet".into()))).child(Button::new("ai-browse").small().label("Browse…").on_click(cx.listener(|_, _, _, cx| {
                let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Image to add (PNG or DDS)".into()) });
                cx.spawn(async move |this, cx| {
                    let Ok(Ok(Some(mut f))) = files.await else { return };
                    if let Some(f) = f.pop() {
                        let _ = this.update(cx, |this, cx| {
                            if let Some(a) = &mut this.adding_image {
                                a.file = Some(f.to_string_lossy().to_string());
                            }
                            cx.notify();
                        });
                    }
                })
                .detach();
            }))))
            .child(v_flex().gap_1().child(div().text_xs().text_color(theme.muted_foreground).child("Texture name")).child(Input::new(&a.name).small()))
            .child(check("ai-shape", "Make a shape that draws it".into(), a.shape).on_click(cx.listener(|this, v: &bool, _, cx| {
                if let Some(a) = &mut this.adding_image {
                    a.shape = *v;
                }
                cx.notify();
            })));
        if let (true, Some(w)) = (a.shape, where_) {
            body = body.child(check("ai-place", format!("Place it on {w}, above everything"), a.place).on_click(cx.listener(|this, v: &bool, _, cx| {
                if let Some(a) = &mut this.adding_image {
                    a.place = *v;
                }
                cx.notify();
            })));
            if a.place {
                body = body.child(h_flex().pl_6().gap_2().items_center().text_xs().child("x").child(div().w(px(80.0)).child(Input::new(&a.x).xsmall())).child("y").child(div().w(px(80.0)).child(Input::new(&a.y).xsmall())).child(div().flex_1().min_w_0().child(Input::new(&a.instance).xsmall())));
            }
        }
        let pending = self.edits.len();
        body = body.child(div().text_xs().text_color(theme.muted_foreground).child(format!(
            "The picture becomes a Texture2D next to the movie's other images (same settings; PNG is encoded like them, DXT5 when it has transparency) and is listed in the movie's references; the movie gets a DefineExternalImage2 naming it, drawn at the picture's size.{} Undo removes all of it.",
            if pending > 0 { format!(" The {pending} pending edit{} written with it.", if pending > 1 { "s are" } else { " is" }) } else { String::new() }
        )));
        let working = a.working;
        let disabled = file.is_none() || working;
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(Button::new("ai-cancel").ghost().label("Cancel").disabled(working).when(working, |b| b.tooltip("Wait until the image is added")).on_click(cx.listener(|this, _, window, cx| {
                this.adding_image = None;
                if this.target().is_some() {
                    this.stage.focus.focus(window, cx);
                } else {
                    this.focus.focus(window, cx);
                }
                cx.notify();
            })))
            .child(
                Button::new("ai-add")
                    .label(if working { "Adding…" } else { "Add" })
                    .disabled(disabled)
                    .when(disabled, |b| b.tooltip(if working { "Adding the image to the movie…" } else { "Choose a PNG or DDS file above first" }))
                    .on_click(cx.listener(|this, _, _, cx| this.add_image(cx))),
            );
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.overlay)
                .child(v_flex().w(px(520.0)).max_w(gpui::relative(0.94)).p_4().gap_3().rounded_lg().bg(theme.popover).border_1().border_color(theme.border).child(div().text_lg().font_weight(gpui::FontWeight::SEMIBOLD).child("Add an image")).child(body).child(footer))
                .into_any_element(),
        )
    }

    // ----- exports ------------------------------------------------------------------

    /// A file name stem for a target: linkage name or kind and id, plus the frame.
    fn stem_of(&self, target: &J) -> String {
        let movie = &self.name;
        let frame = target["frame"].as_u64().unwrap_or(0) + 1;
        match target["kind"].as_str() {
            Some("root") => safe(&format!("{movie}_frame{frame}")),
            Some("frame") => safe(&format!("{movie}_{}_frame{frame}", self.label(target["id"].as_u64().unwrap_or(0)))),
            _ => safe(&format!("{movie}_{}", self.label(target["id"].as_u64().unwrap_or(0)))),
        }
    }

    pub fn export_svg(&mut self, target: J, cx: &mut Context<Self>) {
        let out = cx.prompt_for_new_path(&documents(), Some(&format!("{}.svg", self.stem_of(&target))));
        let (backend, params) = (self.backend.clone(), json!({ "path": self.path, "index": self.index, "format": "svg", "target": target, "edits": self.pending() }));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(out))) = out.await else { return };
            let mut params = params;
            params["out"] = json!(out.to_string_lossy());
            let r = backend.call("swf.export", params).await;
            let _ = this.update(cx, |this, cx| this.exported(r, cx));
        })
        .detach();
    }

    /// Save a font (TrueType), sound (MP3/WAV) or binary data as a file.
    pub fn export_file(&mut self, id: u64, format: &'static str, ext: &str, cx: &mut Context<Self>) {
        let out = cx.prompt_for_new_path(&documents(), Some(&format!("{}.{ext}", self.stem_of(&json!({ "kind": "character", "id": id })))));
        let (backend, params) = (self.backend.clone(), json!({ "path": self.path, "index": self.index, "format": format, "id": id, "edits": self.pending() }));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(out))) = out.await else { return };
            let mut params = params;
            params["out"] = json!(out.to_string_lossy());
            let r = backend.call("swf.export", params).await;
            let _ = this.update(cx, |this, cx| this.exported(r, cx));
        })
        .detach();
    }

    fn exported(&mut self, r: Result<crate::backend::Reply, String>, cx: &mut Context<Self>) {
        match r {
            Ok(r) => {
                self.toast(Tone::Success, format!("{} bytes written to {}", r.json["bytes"], r.json["out"].as_str().unwrap_or("")), cx);
                let warnings: Vec<&str> = r.json["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()).take(6).collect();
                if !warnings.is_empty() {
                    self.toast(Tone::Warning, warnings.join("\n"), cx);
                }
            }
            Err(e) => self.toast(Tone::Error, format!("Could not export: {}", crate::studio::ui::explain_error(&e)), cx),
        }
    }

    /// A PNG of a target (pending edits included), `scale` × its size, transparent around it; `rect` fixes the area.
    pub(crate) async fn render_png(backend: Backend, params: J, scale: u32, rect: Option<Rect>) -> Result<(Vec<u8>, u32, u32), String> {
        {
            let mut params = params;
            if let Some(r) = rect {
                params["rect"] = r.json();
            }
            let r = backend.call("swf.preview", params).await?;
            let area = rect.or_else(|| Rect::from_json(&r.json["rect"])).ok_or("the preview has no area")?;
            let (w, h) = (((area.w * scale as f64).round() as u32).max(1), ((area.h * scale as f64).round() as u32).max(1));
            if w as u64 * h as u64 > 64_000_000 {
                return Err(format!("{w}×{h} is too large: lower the scale"));
            }
            let libraries = r.json["libraries"].as_array().into_iter().flatten().filter_map(|l| Some((l["url"].as_str()?.to_string(), r.bin(&l["swf"])?.to_vec()))).collect();
            let swf = r.bin(&r.json["swf"]).ok_or("the preview has no movie")?.to_vec();
            let img = crate::swf_player::worker().still(crate::swf_player::Load { swf, libraries, width: w, height: h, transparent: true }, 12).await?;
            let mut png = Vec::new();
            image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
            Ok((png, w, h))
        }
    }

    pub fn export_png(&mut self, target: J, scale: u32, cx: &mut Context<Self>) {
        let out = cx.prompt_for_new_path(&documents(), Some(&format!("{}.png", self.stem_of(&target))));
        let (backend, params) = (self.backend.clone(), json!({ "path": self.path, "index": self.index, "target": target, "edits": self.pending() }));
        self.status("Rendering…", cx);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(out))) = out.await else { return };
            let r = Self::render_png(backend, params, scale, None).await.and_then(|(png, w, h)| std::fs::write(&out, &png).map(|_| (png.len(), w, h)).map_err(|e| e.to_string()));
            let _ = this.update(cx, |this, cx| match r {
                Ok((n, w, h)) => this.toast(Tone::Success, format!("{w}×{h}, {n} bytes written to {}", out.display()), cx),
                Err(e) => this.toast(Tone::Error, format!("Could not render the PNG: {}", crate::studio::ui::explain_error(&e)), cx),
            });
        })
        .detach();
    }

    /// Every frame of a sprite (0: the root) as numbered PNGs, all with the same area.
    pub fn export_frames(&mut self, sprite: u64, scale: u32, cx: &mut Context<Self>) {
        let dir = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Folder for the frames".into()) });
        let stem = if sprite == 0 { safe(&self.name) } else { self.stem_of(&json!({ "kind": "character", "id": sprite })) };
        let (backend, path, index, edits) = (self.backend.clone(), self.path.clone(), self.index, self.pending());
        let stage = Rect::from_json(&self.tree["stage"]);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut d))) = dir.await else { return };
            let Some(dir) = d.pop() else { return };
            let r: Result<usize, String> = async {
                let t = backend.call("swf.timeline", json!({ "path": path, "index": index, "sprite": sprite, "edits": edits })).await?;
                let frames = t.json["frames"].as_array().cloned().unwrap_or_default();
                let mut rect = if sprite == 0 { stage } else { None };
                if sprite != 0 {
                    for e in frames.iter().flat_map(|f| f.as_array().cloned().unwrap_or_default()) {
                        if e["clipDepth"].is_null() {
                            if let Some(r) = Rect::from_json(&e["rect"]) {
                                rect = Some(rect.map(|a| a.union(&r)).unwrap_or(r));
                            }
                        }
                    }
                }
                let n = frames.len();
                let digits = n.to_string().len();
                for f in 0..n {
                    let _ = this.update(cx, |this, cx| this.status(format!("Rendering frame {} of {n}…", f + 1), cx));
                    let target = if sprite == 0 { json!({ "kind": "root", "frame": f }) } else { json!({ "kind": "frame", "id": sprite, "frame": f }) };
                    let (png, ..) = Self::render_png(backend.clone(), json!({ "path": path, "index": index, "target": target, "edits": edits }), scale, rect).await?;
                    std::fs::write(dir.join(format!("{stem}_{:0digits$}.png", f + 1)), png).map_err(|e| e.to_string())?;
                }
                Ok(n)
            }
            .await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(n) => this.toast(Tone::Success, format!("{n} frame{} written to {}", if n == 1 { "" } else { "s" }, dir.display()), cx),
                Err(e) => this.toast(Tone::Error, format!("Could not render the frames: {}", crate::studio::ui::explain_error(&e)), cx),
            });
        })
        .detach();
    }
}

pub fn documents() -> PathBuf {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir)
}

pub fn safe(s: &str) -> String {
    let out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' }).collect();
    let out = out.trim_matches('_').to_string();
    if out.is_empty() { "movie".into() } else { out }
}

/// `#rrggbb` (alpha ignored) → bytes.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() < 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

impl Studio {
    /// The editor of the selected movie export (made on first view: it needs the window).
    pub(super) fn movie_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<MovieEditor>> {
        let t = self.open.as_ref()?;
        let i = self.selected_export?;
        let e = t.exports.get(i)?;
        if !is_movie(&e.class) {
            return None;
        }
        let key = (t.path.clone(), i);
        if let Some(m) = self.movies.get(&key) {
            return Some(m.clone());
        }
        let (me, backend, name) = (cx.entity().downgrade(), self.backend.clone(), e.name.clone());
        let panes = self.prefs.movie_panes;
        let m = cx.new(|cx| {
            let mut m = MovieEditor::new(me, backend, key.0.clone(), i, name, window, cx);
            m.panes = panes;
            m
        });
        // The editor needs the width: the first movie opened hides the lists (Focus turns them back on).
        if self.movies.is_empty() {
            self.focus_mode = true;
        }
        self.movies.insert(key, m.clone());
        self.sync_cooker(cx);
        Some(m)
    }

    /// The editor of the selected movie export, when there is one already.
    pub(super) fn shown_movie(&self) -> Option<Entity<MovieEditor>> {
        let t = self.open.as_ref()?;
        self.movies.get(&(t.path.clone(), self.selected_export?)).cloned()
    }
}

impl MovieEditor {
    /// What the MCP state reports.
    pub fn state(&self) -> J {
        let on_screen = self.selection_on_screen();
        let entry = self.selected_entry();
        json!({
            "node": self.node.key(),
            "trail": self.trail().iter().map(|t| t.1.clone()).collect::<Vec<_>>(),
            "frame": self.frame,
            "frames": self.frames(),
            "play": self.play,
            "tool": format!("{:?}", self.tool),
            "depth": self.depth,
            "selected": entry.as_ref().map(|e| json!({ "name": e["name"], "character": e["character"], "matrix": e["matrix"], "cxform": e["cxform"], "rect": e["rect"] })),
            "placements": self.entries().iter().map(|e| json!([e["depth"], self.kind_of(e["character"].as_u64().unwrap_or(0))])).collect::<Vec<_>>(),
            "hidden": self.hidden().into_iter().collect::<Vec<_>>(),
            "edits": self.pending(),
            "undo": self.undo.len(),
            "characters": self.chars.len(),
            "script": self.script,
            "scriptView": self.script_view.as_ref().map(|s| s.summary()),
            "detail": self.detail.as_ref().map(|(id, _, d)| json!({ "id": id, "kind": d.as_ref().ok().map(|d| d["kind"].clone()) })),
            "addingImage": self.adding_image.is_some(),
            "stage": self.stage.summary(),
            "panes": self.panes,
            "onScreen": on_screen,
            "error": self.error,
        })
    }

    /// Dev: `movie <step>` of the test driver.
    pub fn drive(&mut self, verb: &str, rest: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let words: Vec<&str> = rest.split_whitespace().collect();
        let n = |k: usize| words.get(k).and_then(|w| w.parse::<f64>().ok()).ok_or_else(|| format!("movie {verb}: a number expected"));
        // Steps on a character take its id; edits name its tag.
        let tag_of = |this: &Self, id: &str| -> Result<u64, String> { id.parse::<u64>().ok().and_then(|i| this.char(i)).and_then(|c| c["tag"].as_u64()).ok_or_else(|| format!("no character {id}")) };
        match verb {
            "open" => self.open(Node::from_key(rest.trim()).ok_or("open movie|frames|texts|scripts|images|linkage|tags|char:<id>")?, cx),
            "frame" => self.set_frame(n(0)? as usize, cx),
            "select" => {
                self.depth = Some(n(0)? as u64);
                cx.notify();
            }
            "enter" => {
                let d = n(0)? as u64;
                let e = self.entries().into_iter().find(|e| e["depth"].as_u64() == Some(d)).ok_or("no such depth")?;
                if !self.enter(&e, cx) {
                    return Err("not a clip".into());
                }
            }
            "up" => self.up(cx),
            "goto" => self.go_to(n(0)? as u64, cx),
            "play" => self.set_play(true, cx),
            "layout" => self.set_play(false, cx),
            "tool" => self.tool = if rest.trim() == "transform" { Tool::Transform } else { Tool::Move },
            "move" => {
                let d = n(0)? as u64;
                let e = self.entries().into_iter().find(|e| e["depth"].as_u64() == Some(d)).ok_or("no such depth")?;
                let m = M::from_json(&e["matrix"]);
                self.move_entry(&e, n(1)? - m.tx, n(2)? - m.ty, cx);
            }
            "nudge" => {
                let e = self.selected_entry().ok_or("nothing selected")?;
                self.move_entry(&e, n(0)?, n(1)?, cx);
            }
            "set" => {
                let (f, v) = rest.split_once(' ').ok_or("movie set <field key> <value>")?;
                let key = if f.contains(':') { f.to_string() } else { format!("p:{f}") };
                self.commit_field(&key, v.to_string(), cx);
            }
            "remove" => {
                let e = self.selected_entry().ok_or("nothing selected")?;
                self.remove_entry(&e, cx);
            }
            "revert" => {
                let e = self.selected_entry().ok_or("nothing selected")?;
                self.revert_entry(&e, cx);
            }
            "hide" | "show" => {
                let depths: Vec<u64> = words.iter().filter_map(|w| w.parse().ok()).collect();
                self.set_hidden(&depths, verb == "hide", cx);
            }
            "undo" => {
                self.undo_edit(cx);
            }
            "apply" => self.apply(cx),
            "discard" => self.discard(cx),
            "zoom" => {
                self.stage.zoom = n(0)?;
                cx.notify();
            }
            "fit" => {
                self.stage.fit();
                cx.notify();
            }
            "script" => {
                let k = n(0)? as usize;
                let sc = self.info["scripts"].get(k).cloned().ok_or("no such script")?;
                self.open_script(sc["key"].as_str().unwrap_or("").to_string(), cx);
            }
            "script-mode" => self.switch_mode_drive(rest.trim() == "pcode", window, cx),
            "check" => self.check_script_drive(false, window, cx),
            "stage" => self.check_script_drive(true, window, cx),
            "edit-script" => self.replace_script_text(rest, window, cx)?,
            "color" => {
                let (id, key, hex) = (words.first().ok_or("color <id> <key> <#rrggbb>")?, words.get(1).ok_or("key")?, words.get(2).ok_or("colour")?);
                let tag = tag_of(self, id)?;
                self.commit_field(&format!("col:{tag}:{key}"), hex.to_string(), cx);
            }
            "svg" => {
                let (id, file) = rest.split_once(' ').ok_or("svg <id> <file>")?;
                let svg = std::fs::read_to_string(file.trim()).map_err(|e| e.to_string())?;
                let tag = tag_of(self, id)?;
                self.replace_shape(tag, svg, true, cx);
            }
            "glyphs" => {
                let (id, rest) = rest.split_once(' ').ok_or("glyphs <id> <file> <chars>")?;
                let (file, chars) = rest.split_once(' ').ok_or("glyphs <id> <file> <chars>")?;
                let tag = tag_of(self, id)?;
                self.set_edit(json!({ "kind": "fontGlyphs", "tag": tag, "file": file, "chars": chars }), cx);
            }
            "edittext" => {
                let (id, rest) = rest.split_once(' ').ok_or("edittext <id> <prop> <json value>")?;
                let (prop, v) = rest.split_once(' ').ok_or("edittext <id> <prop> <json value>")?;
                let v: J = serde_json::from_str(v).map_err(|e| e.to_string())?;
                let tag = tag_of(self, id)?;
                self.set_edit_text(tag, json!({ prop: v }), cx);
            }
            "add-image" => {
                self.open_add_image(window, cx);
                if let Some(a) = &mut self.adding_image {
                    a.file = Some(rest.trim().to_string());
                }
                self.add_image(cx);
            }
            "export-png" => {
                let (scale, out) = rest.split_once(' ').ok_or("export-png <scale> <out>")?;
                let target = self.target().ok_or("nothing to export")?;
                let (backend, params) = (self.backend.clone(), json!({ "path": self.path, "index": self.index, "target": target, "edits": self.pending() }));
                let (scale, out) = (scale.parse().unwrap_or(1), out.trim().to_string());
                cx.spawn(async move |this, cx| {
                    let r = Self::render_png(backend, params, scale, None).await.and_then(|(png, w, h)| std::fs::write(&out, &png).map(|_| (w, h)).map_err(|e| e.to_string()));
                    let _ = this.update(cx, |this, cx| match r {
                        Ok((w, h)) => this.status(format!("{w}×{h} written to {out}"), cx),
                        Err(e) => this.toast(Tone::Error, crate::studio::ui::explain_error(&e), cx),
                    });
                })
                .detach();
            }
            "export-svg" => {
                let target = self.target().ok_or("nothing to export")?;
                let params = json!({ "path": self.path, "index": self.index, "format": "svg", "target": target, "out": rest.trim(), "edits": self.pending() });
                let call = self.backend.call("swf.export", params);
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| this.exported(r, cx));
                })
                .detach();
            }
            _ => return Err(format!("unknown movie step {verb}")),
        }
        cx.notify();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(edit_key(&json!({ "kind": "place", "sprite": 3, "tag": 7 })), "place|3|7");
        assert_eq!(edit_key(&json!({ "kind": "text", "tag": 7, "text": "x" })), "text|7|0");
        assert_eq!(edit_key(&json!({ "kind": "script", "loc": { "sprite": 26, "tag": 0 } })), "script|s26:0");
        assert_eq!(edit_key(&json!({ "kind": "pcode", "loc": { "sprite": 0, "tag": 4 } })), "script|4");
        assert_eq!(edit_key(&json!({ "kind": "shapeColor", "tag": 2, "key": "fill:0:1" })), "shapeColor|2|fill:0:1");
        assert_eq!(Node::from_key(&Node::Char(42).key()), Some(Node::Char(42)));
        assert_eq!(parse_hex("#ff8000cc"), Some([255, 128, 0]));
    }
}
