//! The Assembly view: the game's content database (`assembly.dat`) and the
//! language files (`Localization/<EXT>/Lang_<EXT>.dat`), MCTS marshals the
//! backend edits (`mcts.*`). Left (`side.rs`): the files, their header fields
//! and tables. Middle (`grid.rs`): the table as a grid, read 500 rows at a
//! time (only the rows and columns in view are laid out), filtered over
//! every cell, sorted by a column; a cell cursor moved with the keyboard,
//! cells edited in place, `*_FLAG` cells as switches, row sets opened in
//! place (breadcrumbs), `*_MSG_ID` and `*_VALUE_ID` values with their text,
//! `*_ID` values with a link to the row they name. Right (`row.rs`): the
//! selected row's fields, each message in every language, and the rows that
//! refer to it. The Electron Studio's assembly editor at parity.

mod content;
mod grid;
mod row;
mod side;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use gpui::{div, prelude::*, Context, Entity, FocusHandle, Focusable, KeyDownEvent, ScrollStrategy, Subscription, Task, UniformListScrollHandle, WeakEntity, Window};
use gpui_component::input::{InputEvent, InputState, TextareaState};
use gpui_component::ActiveTheme;
use serde_json::{json, Map, Value as J};

use super::{dialogs, file_name, Place, Studio, Tone};
use crate::backend::Backend;

/// Rows read per request: the grid lays out only the rows in view, so a page can be long.
const PAGE: usize = 500;

/// Where a `*_ID` column points: the table and its key column (most specific first).
const REFS: &[(&str, &str, &str)] = &[
    ("DEVICE_PROJECTILE_ID", "DATA_SET_PROJECTILES", "DEVICE_PROJECTILE_ID"),
    ("BOT_SPAWN_TABLE_ID", "DATA_SET_BOT_SPAWN_TABLES", "BOT_SPAWN_TABLE_ID"),
    ("MATERIAL_RES_GROUP_ID", "DATA_SET_MAT_RES_GROUPS", "MATERIAL_RES_GROUP_ID"),
    ("LOOT_TABLE_GROUP_ID", "DATA_SET_LOOT_TABLE_GROUPS", "LOOT_TABLE_GROUP_ID"),
    ("LOOT_TABLE_ID", "DATA_SET_LOOT_TABLES", "LOOT_TABLE_ID"),
    ("EFFECT_GROUP_ID", "DATA_SET_EFFECT_GROUPS", "EFFECT_GROUP_ID"),
    ("GAME_TIP_ID", "DATA_SET_TIPS", "GAME_TIP_ID"),
    ("DEPLOYABLE_ID", "DATA_SET_DEPLOYABLES", "DEPLOYABLE_ID"),
    ("ITEM_ID", "DATA_SET_ITEMS", "ITEM_ID"),
    ("DEVICE_ID", "DATA_SET_DEVICES", "DEVICE_ID"),
    ("SKIN_ID", "DATA_SET_SKINS", "SKIN_ID"),
    ("BOT_ID", "DATA_SET_BOTS", "BOT_ID"),
    ("FX_ID", "DATA_SET_SPECIAL_FX", "FX_ID"),
    ("RES_ID", "DATA_SET_RESOURCES", "RES_ID"),
    ("ICON_ID", "DATA_SET_ICONS", "ICON_ID"),
    ("ASM_ID", "DATA_SET_ASSEMBLY_MESHES", "ASM_ID"),
    ("VALUE_ID", "DATA_SET_VALID_VALUES", "VALUE_ID"),
    ("PROP_ID", "DATA_SET_PROPERTIES", "PROP_ID"),
    ("XP_ID", "DATA_SET_XP_TABLES", "XP_ID"),
    ("VGS_ID", "DATA_SET_VGS", "VGS_ID"),
    ("LANG_ID", "DATA_SET_LANGUAGES", "LANG_ID"),
    ("BEHAVIOR_ID", "DATA_SET_BOT_BEHAVIORS", "BEHAVIOR_ID"),
];

/// The table a column refers to, by naming convention (`CARD_RES_ID` → resources).
fn ref_of(column: &str) -> Option<(&'static str, &'static str)> {
    if is_message(column) {
        return None;
    }
    REFS.iter().find(|(suffix, _, _)| column == *suffix || column.strip_suffix(suffix).is_some_and(|rest| rest.ends_with('_'))).map(|(_, t, k)| (*t, *k))
}

fn is_message(column: &str) -> bool {
    column.ends_with("_MSG_ID")
}

fn is_flag(column: &str) -> bool {
    column.ends_with("_FLAG")
}

fn is_value_id(column: &str) -> bool {
    column.ends_with("_VALUE_ID")
}

/// A cell as the grid and the editors write it (64-bit numbers travel as strings, row sets as `{rows}`).
fn cell_text(v: &J) -> String {
    match v {
        J::Null => String::new(),
        J::String(s) => s.clone(),
        J::Object(o) if o.contains_key("rows") => format!("{} rows", o["rows"]),
        other => other.to_string(),
    }
}

/// The id a cell holds, when it is a number.
fn cell_id(v: &J) -> Option<i64> {
    v.as_i64()
}

fn short(table: &str) -> &str {
    table.strip_prefix("DATA_SET_").unwrap_or(table)
}

/// `GAME_VERSION_RAW` packs four 16-bit numbers: 8.1.5839.0.
fn version_text(v: &J) -> Option<String> {
    let n: u64 = match v {
        J::String(s) => s.parse().ok()?,
        J::Number(n) => n.as_u64()?,
        _ => return None,
    };
    Some([48, 32, 16, 0].iter().map(|s| ((n >> s) & 0xffff).to_string()).collect::<Vec<_>>().join("."))
}

/// `8.1.5839.0` → the packed `GAME_VERSION_RAW` (as a decimal string).
fn version_raw(text: &str) -> Option<String> {
    let parts: Vec<u64> = text.trim().split('.').map(|p| p.parse().ok().filter(|&n| n <= 0xffff)).collect::<Option<Vec<_>>>()?;
    if parts.len() != 4 {
        return None;
    }
    Some(parts.iter().fold(0u64, |v, p| (v << 16) | p).to_string())
}

/// A header field as its editor shows it.
fn header_text(field: &str, value: &J) -> String {
    if field == "GAME_VERSION_RAW" {
        if let Some(t) = version_text(value) {
            return t;
        }
    }
    match value {
        J::Object(_) => String::new(),
        other => cell_text(other),
    }
}

/// A row set address as the backend takes it: `[[field, row], …]`.
type Path = Vec<(String, usize)>;

fn path_json(path: &[(String, usize)]) -> J {
    J::from(path.iter().map(|(f, i)| json!([f, i])).collect::<Vec<_>>())
}

fn path_of(j: &J) -> Path {
    j.as_array().into_iter().flatten().filter_map(|s| Some((s[0].as_str()?.to_string(), s[1].as_u64()? as usize))).collect()
}

#[derive(Clone)]
struct Column {
    name: String,
    kind: String,
    /// Rows of the set that hold the column.
    count: usize,
}

impl Column {
    fn of(c: &J) -> Column {
        Column { name: c["name"].as_str().unwrap_or_default().to_string(), kind: c["kind"].as_str().unwrap_or_default().to_string(), count: c["count"].as_u64().unwrap_or(0) as usize }
    }

    fn editable(&self) -> bool {
        self.kind != "rowset" && self.kind != "blob"
    }
}

struct Table {
    name: String,
    rows: usize,
    columns: usize,
}

struct Doc {
    id: String,
    path: String,
    /// The marshal's function name (shown beside the file).
    function: String,
    dirty: bool,
    /// What Undo would take back, last first.
    undo: Vec<String>,
    header: Vec<(String, J)>,
    tables: Vec<Table>,
}

impl Doc {
    fn of(d: &J) -> Doc {
        Doc {
            id: d["id"].as_str().unwrap_or_default().to_string(),
            path: d["path"].as_str().unwrap_or_default().to_string(),
            function: d["function"].as_str().unwrap_or_default().to_string(),
            dirty: d["dirty"].as_bool().unwrap_or(false),
            undo: d["undo"].as_array().into_iter().flatten().filter_map(|u| u.as_str().map(str::to_string)).collect(),
            header: d["header"].as_array().into_iter().flatten().map(|h| (h["name"].as_str().unwrap_or_default().to_string(), h["value"].clone())).collect(),
            tables: d["tables"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|t| Table { name: t["name"].as_str().unwrap_or_default().to_string(), rows: t["rows"].as_u64().unwrap_or(0) as usize, columns: t["columns"].as_array().map(|c| c.len()).unwrap_or(0) })
                .collect(),
        }
    }

    /// `assembly.dat`, `Lang_INT.dat`.
    fn label(&self) -> String {
        if self.id == "assembly" {
            "assembly.dat".into()
        } else {
            format!("Lang_{}.dat", self.id)
        }
    }
}

struct Field {
    name: String,
    kind: String,
    value: J,
}

/// A row that names the selected row's id.
struct Usage {
    path: Path,
    field: String,
    label: String,
}

/// The selected row's pane.
struct RowPane {
    index: usize,
    fields: Vec<Field>,
    /// The rows that refer to it and whether the list was cut, once asked for.
    usages: Option<(Vec<Usage>, bool)>,
    usages_loading: bool,
}

/// A message in one language: its text, and its row in that language's file.
struct Message {
    text: String,
    name: String,
    path: Path,
}

/// What is being edited.
#[derive(Clone, PartialEq, Debug)]
enum Edit {
    /// A cell of the grid (Enter goes down, Tab sideways).
    Cell { row: usize, field: String },
    /// A field of the row pane (`long`: in the multi-line editor).
    Field { row: usize, field: String, long: bool },
    Header { doc: String, field: String },
    Message { id: i64, lang: String },
}

impl Edit {
    /// Edited in the multi-line editor.
    fn in_area(&self) -> bool {
        matches!(self, Edit::Message { .. } | Edit::Field { long: true, .. })
    }
}

/// Where the cursor goes once a cell edit is applied.
#[derive(Clone, Copy, PartialEq)]
enum Move {
    Stay,
    Down,
    Right,
    Left,
}

pub struct Assembly {
    pub(super) backend: Backend,
    studio: WeakEntity<Studio>,
    docs: Vec<Doc>,
    loading: bool,
    error: Option<String>,
    doc: Option<String>,
    /// The row set on screen: the rows leading to it, and its table / field name.
    path: Path,
    table: Option<String>,
    columns: Vec<Column>,
    total: usize,
    /// Loaded rows by position: (row index, cells).
    rows: HashMap<usize, (usize, Map<String, J>)>,
    /// Position of a loaded row, by its index.
    positions: HashMap<usize, usize>,
    pages: HashSet<usize>,
    /// Bumped whenever the pages are dropped, so late pages are ignored.
    seq: u64,
    filter: Entity<InputState>,
    /// The filter the rows were asked with (the field's text, applied after a pause).
    filter_text: String,
    filter_task: Option<Task<()>>,
    table_filter: Entity<InputState>,
    /// Files whose header fields are shown in the side list (folded by default).
    headers_open: HashSet<String>,
    sort: Option<(String, bool)>,
    selected: Option<usize>,
    /// The cell cursor's column (its row is the selected one).
    col: Option<String>,
    /// Where ⇧-click marks from.
    anchor: Option<usize>,
    row: Option<RowPane>,
    edit: Option<Edit>,
    /// The text the edit started with (an unchanged value is not written).
    edit_from: String,
    edit_input: Entity<InputState>,
    edit_area: Entity<TextareaState>,
    scroll: UniformListScrollHandle,
    /// INT texts of the `*_MSG_ID` cells seen so far (grid hints).
    labels: HashMap<i64, String>,
    label_wanted: HashSet<i64>,
    /// Message id → language → message, read for the selected row.
    messages: HashMap<i64, HashMap<String, Message>>,
    valid: HashMap<i64, String>,
    /// The row at this position is selected once its page is read.
    want_pos: Option<usize>,
    /// The row with this index is selected once the rows are read.
    pending_select: Option<usize>,
    /// Rows marked with ⌘/⇧-click: an edit of one of them sets them all.
    marked: BTreeSet<usize>,
    /// Hidden columns per table (kept between runs).
    hidden: HashMap<String, HashSet<String>>,
    /// The widest cell of each column, in characters, over the rows read so far.
    chars: HashMap<String, usize>,
    chars_stale: bool,
    /// The grid's sideways scroll (the id and name columns stay at the left).
    hscroll: gpui::ScrollHandle,
    /// The cursor's column is scrolled into view at the next frame.
    reveal_col: bool,
    row_scroll: gpui::ScrollHandle,
    /// ⌘B / ⌘J and the compact layout (the window's).
    panes: super::layout::Shared,
    /// The files and row panes' widths when the view opened.
    sizes: Vec<f32>,
    focus: FocusHandle,
    /// The table list's focus: ↑ ↓ open the next table.
    tables_focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl Assembly {
    pub fn new(backend: Backend, studio: WeakEntity<Studio>, panes: super::layout::Shared, sizes: Vec<f32>, id_range: Option<(i64, i64)>, window: &mut Window, cx: &mut Context<Self>) -> Assembly {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter rows (any cell contains…)"));
        let table_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter tables"));
        let edit_input = cx.new(|cx| InputState::new(window, cx));
        let edit_area = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 10));
        let subs = vec![
            cx.subscribe(&filter, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.filter_later(cx);
                }
            }),
            cx.subscribe(&table_filter, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&edit_input, window, |this, _, e: &InputEvent, window, cx| {
                if this.edit.as_ref().is_none_or(|e| e.in_area()) {
                    return;
                }
                match e {
                    InputEvent::PressEnter { .. } => this.commit_edit(Move::Down, window, cx),
                    // Leaving the field applies it (Escape is the way out).
                    InputEvent::Blur => this.commit_edit(Move::Stay, window, cx),
                    _ => {}
                }
            }),
            cx.subscribe_in(&edit_area, window, |this, _, e: &InputEvent, window, cx| {
                if this.edit.as_ref().is_none_or(|e| !e.in_area()) {
                    return;
                }
                // Enter is a line break here: ⌘Enter (bound in `AsmEdit`) or leaving the field applies.
                if matches!(e, InputEvent::Blur) {
                    this.commit_edit(Move::Stay, window, cx);
                }
            }),
        ];
        let mut a = Assembly {
            backend,
            studio,
            docs: Vec::new(),
            loading: true,
            error: None,
            doc: None,
            path: Vec::new(),
            table: None,
            columns: Vec::new(),
            total: 0,
            rows: HashMap::new(),
            positions: HashMap::new(),
            pages: HashSet::new(),
            seq: 0,
            filter,
            filter_text: String::new(),
            filter_task: None,
            table_filter,
            headers_open: HashSet::new(),
            sort: None,
            selected: None,
            col: None,
            anchor: None,
            row: None,
            edit: None,
            edit_from: String::new(),
            edit_input,
            edit_area,
            scroll: UniformListScrollHandle::new(),
            labels: HashMap::new(),
            label_wanted: HashSet::new(),
            messages: HashMap::new(),
            valid: HashMap::new(),
            want_pos: None,
            pending_select: None,
            marked: BTreeSet::new(),
            hidden: load_hidden(),
            chars: HashMap::new(),
            chars_stale: true,
            hscroll: gpui::ScrollHandle::new(),
            reveal_col: false,
            row_scroll: gpui::ScrollHandle::new(),
            panes,
            sizes,
            focus: cx.focus_handle(),
            tables_focus: cx.focus_handle(),
            _subs: subs,
        };
        // New content takes its ids from the user's range (the settings').
        if let Some((start, end)) = id_range {
            let (open, range) = (a.backend.call("mcts.open", json!({})), a.backend.call("mcts.idRange", json!({ "start": start, "end": end })));
            cx.spawn(async move |this, cx| {
                let _ = open.await;
                if let Err(e) = range.await {
                    let _ = this.update(cx, |this, cx| this.toast(Tone::Warning, format!("Could not apply the id range {start}…{end}: {e}"), cx));
                }
            })
            .detach();
        }
        a.open(false, cx);
        a
    }

    /// Run `f` on the Studio once this update is over: the Studio may be the
    /// one updating this view (a command, Back/Forward), and an entity cannot
    /// be updated while it is.
    fn with_studio(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut Studio, &mut Context<Studio>) + 'static) {
        let studio = self.studio.clone();
        cx.defer(move |cx| {
            let _ = studio.update(cx, f);
        });
    }

    pub(super) fn toast(&self, tone: Tone, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        self.with_studio(cx, move |s, cx| s.toast(tone, text, cx));
    }

    fn set_status(&self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        self.with_studio(cx, move |s, cx| s.set_status(text, cx));
    }


    /// What the selected row is, for the mod export's fields: a device or a champion (bot) row with its id.
    pub(super) fn template_guess(&self) -> (&'static str, Option<i64>) {
        let (kind, key) = match self.table.as_deref() {
            Some("DATA_SET_DEVICES") => ("device", "DEVICE_ID"),
            Some("DATA_SET_BOTS") => ("champion", "BOT_ID"),
            _ => ("item", "ITEM_ID"),
        };
        let id = self.row.as_ref().filter(|_| self.path.is_empty()).and_then(|r| r.fields.iter().find(|f| f.name == key)).and_then(|f| f.value.as_i64());
        (kind, id)
    }

    /// The files changed behind the view's back (a workspace file applied, new content made): read them again.
    pub fn reopen(&mut self, cx: &mut Context<Self>) {
        self.open(false, cx);
        self.messages.clear();
        self.labels.clear();
        self.label_wanted.clear();
        self.reload_rows(cx);
        if let Some(i) = self.selected {
            self.select(i, cx);
        }
    }

    /// Read the files (`reload`: again from the game folder, dropping the backend's copies).
    pub(super) fn open(&mut self, reload: bool, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let call = self.backend.call("mcts.open", json!({ "reload": reload }));
        let valid = self.backend.call("mcts.validValues", json!({}));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let v = valid.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match r {
                    Ok(r) => {
                        this.docs = r.json["docs"].as_array().into_iter().flatten().map(Doc::of).collect();
                        if this.doc.as_ref().is_none_or(|d| !this.docs.iter().any(|x| &x.id == d)) {
                            this.doc = this.docs.first().map(|d| d.id.clone());
                        }
                        // The first table shows at once, like a package's first object.
                        if this.table.is_none() {
                            if let Some((doc, table)) = this.docs.first().and_then(|d| Some((d.id.clone(), d.tables.first()?.name.clone()))) {
                                this.show(doc, Vec::new(), table, None, false, cx);
                            }
                        } else if reload {
                            this.messages.clear();
                            this.labels.clear();
                            this.label_wanted.clear();
                            this.load_table(cx);
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                if let Ok(v) = v {
                    if let Some(m) = v.json.as_object() {
                        this.valid = m.iter().filter_map(|(k, x)| Some((k.parse().ok()?, x["text"].as_str()?.to_string()))).collect();
                        this.chars_stale = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Reload assembly.dat and the language files from the game folder (asks when edits would be lost).
    pub(super) fn reload_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unsaved_count() == 0 {
            self.open(true, cx);
            return;
        }
        let me = cx.entity().downgrade();
        dialogs::confirm(window, cx, "Reload the assembly files?", "Their edits are dropped and the files are read again from the game folder.", "Reload", true, move |_, cx| {
            let _ = me.update(cx, |this, cx| this.open(true, cx));
        });
    }

    /// A status reply (`McsStatus`): the document's dirty flag and undo list.
    fn status(&mut self, j: &J, cx: &mut Context<Self>) {
        if let Some(d) = self.docs.iter_mut().find(|d| j["doc"].as_str() == Some(d.id.as_str())) {
            d.dirty = j["dirty"].as_bool().unwrap_or(d.dirty);
            if let Some(u) = j["undo"].as_array() {
                d.undo = u.iter().filter_map(|u| u.as_str().map(str::to_string)).collect();
            }
        }
        cx.notify();
    }

    /// Read one document's summary again (row counts, header, dirty flag) after an edit that changes them.
    fn refresh_doc(&mut self, id: String, cx: &mut Context<Self>) {
        let call = self.backend.call("mcts.doc", json!({ "doc": id }));
        cx.spawn(async move |this, cx| {
            if let Ok(r) = call.await {
                let _ = this.update(cx, |this, cx| {
                    let doc = Doc::of(&r.json);
                    if let Some(d) = this.docs.iter_mut().find(|d| d.id == doc.id) {
                        *d = doc;
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// The language files, in the order the game folder lists them.
    fn languages(&self) -> Vec<String> {
        self.docs.iter().filter(|d| d.id != "assembly").map(|d| d.id.clone()).collect()
    }

    fn current_doc(&self) -> Option<&Doc> {
        self.docs.iter().find(|d| Some(&d.id) == self.doc.as_ref())
    }

    pub fn unsaved_count(&self) -> usize {
        self.docs.iter().filter(|d| d.dirty).count()
    }

    pub fn unsaved_files(&self) -> Vec<String> {
        self.docs.iter().filter(|d| d.dirty).map(|d| d.path.clone()).collect()
    }


    /// Whether the file on screen has unsaved edits.
    pub fn doc_dirty(&self) -> bool {
        self.current_doc().is_some_and(|d| d.dirty)
    }


    /// The table (or row set) on screen.
    pub fn table_name(&self) -> Option<String> {
        self.table.clone()
    }


    /// The tables of every file, for the palette: (file id, file label, table, rows).
    pub fn tables(&self) -> Vec<(String, String, String, usize)> {
        self.docs.iter().flat_map(|d| d.tables.iter().map(move |t| (d.id.clone(), d.label(), t.name.clone(), t.rows))).collect()
    }

    pub fn place(&self) -> Place {
        Place::Assembly { file: self.doc.clone().unwrap_or_default(), path: self.path.clone(), table: self.table.clone(), row: self.selected }
    }

    /// The place of row `index` of the set on screen (a new tab opens there).
    fn place_of(&self, index: usize) -> Place {
        Place::Assembly { file: self.doc.clone().unwrap_or_default(), path: self.path.clone(), table: self.table.clone(), row: Some(index) }
    }

    fn record(&self, cx: &mut Context<Self>) {
        let place = self.place();
        self.with_studio(cx, move |s, _| s.record(place));
    }

    /// Show a top-level table and row (new content, the vendor and skins dialogs).
    pub fn visit(&mut self, file: String, table: Option<String>, row: Option<usize>, cx: &mut Context<Self>) {
        self.visit_path(file, Vec::new(), table, row, cx);
    }

    /// Back/Forward and tabs: show this row set and row.
    pub fn visit_path(&mut self, file: String, path: Path, table: Option<String>, row: Option<usize>, cx: &mut Context<Self>) {
        let file = if file.is_empty() { self.doc.clone().unwrap_or_else(|| "assembly".into()) } else { file };
        let Some(table) = table else {
            self.doc = Some(file);
            cx.notify();
            return;
        };
        if self.doc.as_deref() != Some(file.as_str()) || self.table.as_deref() != Some(table.as_str()) || self.path != path {
            self.show(file, path, table, row, false, cx);
        } else if let Some(r) = row {
            self.select(r, cx);
            self.reveal_index(r);
        }
        cx.notify();
    }

    // ----- tables -------------------------------------------------------------------

    /// Show a row set (a top-level table, or a nested one under `path`), with row `select` selected.
    fn show(&mut self, doc: String, path: Path, table: String, select: Option<usize>, keep_filter: bool, cx: &mut Context<Self>) {
        self.cancel_edit(cx);
        self.doc = Some(doc);
        self.path = path;
        self.table = Some(table);
        self.selected = None;
        self.row = None;
        self.marked.clear();
        self.anchor = None;
        if !keep_filter {
            self.sort = None;
            self.filter_text.clear();
            self.filter_task = None;
        }
        self.pending_select = select;
        self.hscroll.set_offset(gpui::point(gpui::px(0.0), gpui::px(0.0)));
        self.load_table(cx);
    }

    /// The user opens a row set: shown, and a place of the history.
    fn open_at(&mut self, doc: String, path: Path, table: String, select: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        super::layout::update(&self.panes, |p| p.overlay = false);
        self.show(doc, path, table, select, false, cx);
        // The filter box follows (cleared when another table opens).
        self.filter.update(cx, |s, cx| s.set_value("", window, cx));
        self.record(cx);
    }

    fn open_table(&mut self, doc: String, table: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_at(doc, Vec::new(), table, None, window, cx);
    }

    fn load_table(&mut self, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        self.columns.clear();
        self.chars.clear();
        self.chars_stale = true;
        let call = self.backend.call("mcts.table", json!({ "doc": doc, "path": path_json(&self.path), "table": table }));
        let at = (doc, self.path.clone(), table);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if (this.doc.as_ref(), &this.path, this.table.as_ref()) != (Some(&at.0), &at.1, Some(&at.2)) {
                    return;
                }
                match r {
                    Ok(r) => {
                        this.columns = r.json["columns"].as_array().into_iter().flatten().map(Column::of).collect();
                        this.chars_stale = true;
                        // Keep the cursor on a column of this table.
                        if this.col.as_ref().is_none_or(|c| !this.columns.iter().any(|x| &x.name == c)) {
                            this.col = this.visible_columns().first().map(|c| c.name.clone());
                        }
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not load the table: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
        self.reload_rows(cx);
    }

    fn row_path(&self, index: usize) -> J {
        let mut p = self.path.clone();
        p.push((self.table.clone().unwrap_or_default(), index));
        path_json(&p)
    }

    /// Drop the rows read and read the first page again (a filter, a sort, an edit that moves rows).
    fn reload_rows(&mut self, cx: &mut Context<Self>) {
        self.rows.clear();
        self.positions.clear();
        self.pages.clear();
        self.seq += 1;
        self.want_pos = None;
        // A row to select further down: its page too (unfiltered and unsorted, a row sits at its index).
        let first = self.pending_select.filter(|_| self.plain_order()).map(|r| r / PAGE).unwrap_or(0);
        self.load_page(first, cx);
        cx.notify();
    }

    /// The rows are in file order: a row's position is its index.
    fn plain_order(&self) -> bool {
        self.filter_text.is_empty() && self.sort.is_none()
    }

    /// Apply the filter field once typing pauses.
    fn filter_later(&mut self, cx: &mut Context<Self>) {
        self.filter_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let _ = this.update(cx, |this, cx| this.apply_filter(cx));
        }));
    }

    fn apply_filter(&mut self, cx: &mut Context<Self>) {
        let text = self.filter.read(cx).value().to_string();
        if text == self.filter_text {
            return;
        }
        self.filter_text = text;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.reload_rows(cx);
    }

    /// Filter the rows of the table on screen (the palette).
    pub fn set_filter(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.update(cx, |s, cx| s.set_value(text.clone(), window, cx));
        self.filter_task = None;
        if text != self.filter_text {
            self.filter_text = text;
            self.scroll.scroll_to_item(0, ScrollStrategy::Top);
            self.reload_rows(cx);
        }
    }

    fn set_sort(&mut self, column: String, cx: &mut Context<Self>) {
        self.sort = match &self.sort {
            Some((s, false)) if *s == column => Some((column, true)),
            Some((s, true)) if *s == column => None,
            _ => Some((column, false)),
        };
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.reload_rows(cx);
    }

    fn load_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        if !self.pages.insert(page) {
            return;
        }
        let mut params = json!({ "doc": doc, "path": path_json(&self.path), "table": table, "offset": page * PAGE, "limit": PAGE, "filter": self.filter_text });
        if let Some((c, desc)) = &self.sort {
            params["sort"] = json!(c);
            params["desc"] = json!(desc);
        }
        let seq = self.seq;
        let call = self.backend.call("mcts.rows", params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.seq != seq {
                    return;
                }
                match r {
                    Ok(r) => {
                        this.total = r.json["total"].as_u64().unwrap_or(0) as usize;
                        let offset = r.json["offset"].as_u64().unwrap_or(0) as usize;
                        for (k, row) in r.json["rows"].as_array().into_iter().flatten().enumerate() {
                            let index = row[0].as_u64().unwrap_or(0) as usize;
                            let cells = row[1].as_object().cloned().unwrap_or_default();
                            this.rows.insert(offset + k, (index, cells));
                            this.positions.insert(index, offset + k);
                        }
                        this.chars_stale = true;
                        this.want_labels(cx);
                        if let Some(r) = this.pending_select.take() {
                            this.select(r, cx);
                            this.reveal_index(r);
                        }
                        if let Some(pos) = this.want_pos {
                            if let Some((index, _)) = this.rows.get(&pos) {
                                let index = *index;
                                this.want_pos = None;
                                this.user_select(index, cx);
                            }
                        }
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not load the rows: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// INT texts of the `*_MSG_ID` values read so far (once each): the grid's hints.
    fn want_labels(&mut self, cx: &mut Context<Self>) {
        if !self.docs.iter().any(|d| d.id == "INT") {
            return;
        }
        let mut ids = Vec::new();
        for (_, cells) in self.rows.values() {
            for (k, v) in cells {
                if is_message(k) {
                    if let Some(id) = v.as_i64().filter(|&i| i != 0) {
                        if self.label_wanted.insert(id) {
                            ids.push(id);
                        }
                    }
                }
            }
        }
        if ids.is_empty() {
            return;
        }
        let call = self.backend.call("mcts.messages", json!({ "ids": ids, "langs": ["INT"] }));
        cx.spawn(async move |this, cx| {
            if let Ok(r) = call.await {
                let _ = this.update(cx, |this, cx| {
                    for (id, langs) in r.json.as_object().into_iter().flatten() {
                        if let (Ok(id), Some(t)) = (id.parse::<i64>(), langs["INT"]["text"].as_str()) {
                            this.labels.insert(id, t.to_string());
                        }
                    }
                    this.chars_stale = true;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// The INT text of a message id, when known.
    fn label(&self, id: i64) -> Option<&str> {
        self.messages.get(&id).and_then(|m| m.get("INT")).map(|m| m.text.as_str()).or_else(|| self.labels.get(&id).map(String::as_str)).filter(|t| !t.is_empty())
    }

    /// What a cell's value means: a message's text, a valid value's name.
    fn hint(&self, column: &str, v: &J) -> Option<&str> {
        let id = cell_id(v)?;
        if is_message(column) {
            self.label(id)
        } else if is_value_id(column) {
            self.valid.get(&id).map(String::as_str)
        } else {
            None
        }
    }

    // ----- rows ---------------------------------------------------------------------

    fn cells(&self, index: usize) -> Option<&Map<String, J>> {
        self.positions.get(&index).and_then(|p| self.rows.get(p)).map(|r| &r.1)
    }

    fn cell(&self, index: usize, column: &str) -> Option<&J> {
        self.cells(index).and_then(|c| c.get(column))
    }

    /// Scroll the row with this index into view (its page is read when it is not yet).
    fn reveal_index(&mut self, index: usize) {
        let pos = self.positions.get(&index).copied().or_else(|| self.plain_order().then_some(index));
        if let Some(pos) = pos {
            self.scroll.scroll_to_item(pos, ScrollStrategy::Center);
        }
    }

    /// Select a row and read its fields and messages.
    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else { return };
        if self.selected != Some(index) {
            self.row = None;
        }
        self.selected = Some(index);
        let call = self.backend.call("mcts.row", json!({ "doc": doc, "path": self.row_path(index) }));
        let seq = self.seq;
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.selected != Some(index) {
                    return;
                }
                match r {
                    Ok(r) => {
                        let fields: Vec<Field> = r.json["fields"].as_array().into_iter().flatten().map(|f| Field { name: f["name"].as_str().unwrap_or_default().to_string(), kind: f["kind"].as_str().unwrap_or_default().to_string(), value: f["value"].clone() }).collect();
                        let ids: Vec<i64> = fields.iter().filter(|f| is_message(&f.name)).filter_map(|f| f.value.as_i64()).collect();
                        // The usages found stay while the same row is read again (after an edit of it).
                        let kept = this.row.take().filter(|r| r.index == index && this.seq == seq).and_then(|r| r.usages);
                        this.row = Some(RowPane { index, fields, usages: kept, usages_loading: false });
                        this.load_messages(ids, cx);
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not load the row: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The texts of message ids not read yet, in every language.
    fn load_messages(&mut self, ids: Vec<i64>, cx: &mut Context<Self>) {
        let mut missing: Vec<i64> = ids.into_iter().filter(|id| *id != 0 && !self.messages.contains_key(id)).collect();
        missing.sort_unstable();
        missing.dedup();
        if missing.is_empty() {
            return;
        }
        let call = self.backend.call("mcts.messages", json!({ "ids": missing }));
        cx.spawn(async move |this, cx| {
            let Ok(r) = call.await else { return };
            let _ = this.update(cx, |this, cx| {
                for id in missing {
                    let langs = r.json[id.to_string()].as_object().into_iter().flatten().map(|(l, m)| (l.clone(), Message { text: m["text"].as_str().unwrap_or_default().to_string(), name: m["name"].as_str().unwrap_or_default().to_string(), path: path_of(&m["path"]) })).collect();
                    this.messages.insert(id, langs);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A selection the user made: a place of the history.
    fn user_select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.select(index, cx);
        self.reveal_row(index);
        self.record(cx);
    }

    /// Keep the selected row in view without moving it when it already is.
    fn reveal_row(&mut self, index: usize) {
        if let Some(pos) = self.positions.get(&index) {
            self.scroll.scroll_to_item(*pos, ScrollStrategy::Nearest);
        }
    }

    /// Select the row at a position of the set (its page is read first when needed).
    fn select_pos(&mut self, pos: usize, cx: &mut Context<Self>) {
        if self.total == 0 {
            return;
        }
        let pos = pos.min(self.total - 1);
        match self.rows.get(&pos) {
            Some((index, _)) => {
                let index = *index;
                self.user_select(index, cx);
            }
            None => {
                self.want_pos = Some(pos);
                self.scroll.scroll_to_item(pos, ScrollStrategy::Nearest);
                self.load_page(pos / PAGE, cx);
            }
        }
    }

    /// The selected row's own id field (`DATA_SET_BOTS` → `BOT_ID`), for "Used by".
    fn id_field(&self) -> Option<(String, i64)> {
        let table = self.table.as_ref()?;
        let row = self.row.as_ref()?;
        let names = key_names(table);
        row.fields.iter().find(|f| names.contains(&f.name) && f.value.is_number()).and_then(|f| Some((f.name.clone(), f.value.as_i64()?)))
    }

    /// The rows of the file that name the selected row's id.
    fn find_usages(&mut self, cx: &mut Context<Self>) {
        let Some((field, value)) = self.id_field() else { return };
        let Some(row) = &mut self.row else { return };
        let index = row.index;
        row.usages_loading = true;
        let own = self.row_path(index);
        let call = self.backend.call("mcts.usages", json!({ "doc": self.doc.clone().unwrap_or_else(|| "assembly".into()), "field": field, "value": value }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                let Some(row) = this.row.as_mut().filter(|r| r.index == index) else { return };
                row.usages_loading = false;
                match r {
                    Ok(r) => {
                        let list = r.json["usages"].as_array().into_iter().flatten().filter(|u| u["path"] != own).map(|u| Usage { path: path_of(&u["path"]), field: u["field"].as_str().unwrap_or_default().to_string(), label: u["label"].as_str().unwrap_or_default().to_string() }).collect();
                        row.usages = Some((list, r.json["truncated"].as_bool().unwrap_or(false)));
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not look for usages: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Open a place in a new tab (⌘-click, middle click).
    fn open_in_tab(&self, place: Place, cx: &mut Context<Self>) {
        self.with_studio(cx, move |s, cx| s.new_tab(Some(place), cx));
    }

    /// Go to the row at `path` (a usage): its set opens with it selected.
    fn go_to_path(&mut self, path: Path, new_tab: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else { return };
        let Some(((table, index), parents)) = path.split_last().map(|(l, p)| (l.clone(), p.to_vec())) else { return };
        if new_tab {
            self.open_in_tab(Place::Assembly { file: doc, path: parents, table: Some(table), row: Some(index) }, cx);
        } else {
            self.open_at(doc, parents, table, Some(index), window, cx);
        }
    }

    /// Open a nested row set (a `{rows}` cell) of row `index`.
    fn open_set(&mut self, index: usize, field: String, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        let mut path = self.path.clone();
        path.push((table, index));
        self.open_at(doc, path, field, None, window, cx);
    }

    /// Up the breadcrumb to depth `depth` (0: the top-level table), on the row that was opened.
    fn up_to(&mut self, depth: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else { return };
        if depth >= self.path.len() {
            return;
        }
        let (table, row) = self.path[depth].clone();
        let path = self.path[..depth].to_vec();
        self.open_at(doc, path, table, Some(row), window, cx);
    }

    /// Follow a `*_ID` value to the row it names.
    fn follow(&mut self, column: &str, value: &J, new_tab: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((table, key)) = ref_of(column) else { return };
        let Some(v) = value.as_i64() else { return };
        let call = self.backend.call("mcts.find", json!({ "doc": "assembly", "table": table, "key": key, "value": v }));
        cx.spawn_in(window, async move |this, cx| {
            let r = call.await;
            let _ = this.update_in(cx, |this, window, cx| match r {
                Ok(r) => match r.json.as_array().and_then(|a| a.first()).and_then(|x| x.as_u64()) {
                    Some(row) if new_tab => this.open_in_tab(Place::Assembly { file: "assembly".into(), path: Vec::new(), table: Some(table.to_string()), row: Some(row as usize) }, cx),
                    Some(row) => this.open_at("assembly".into(), Vec::new(), table.to_string(), Some(row as usize), window, cx),
                    None => this.toast(Tone::Warning, format!("{table} has no row with {key} = {v}"), cx),
                },
                Err(e) => this.toast(Tone::Error, format!("Could not follow the reference: {e}"), cx),
            });
        })
        .detach();
    }

    /// Jump to a message's row in a language file.
    fn open_message(&mut self, id: i64, lang: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.messages.get(&id).and_then(|m| m.get(&lang)).map(|m| m.path.clone()) else { return };
        let Some((table, index)) = path.first().cloned() else { return };
        self.open_at(lang, Vec::new(), table, Some(index), window, cx);
    }

    // ----- edits ----------------------------------------------------------------------

    /// Start editing: `draft` is the text shown; `select_all` replaces it on typing.
    fn start_edit(&mut self, edit: Edit, draft: String, original: String, select_all: bool, window: &mut Window, cx: &mut Context<Self>) {
        // An edit left open is applied first, like leaving its field.
        if self.edit.is_some() {
            self.commit_edit(Move::Stay, window, cx);
        }
        self.edit_from = original;
        if edit.in_area() {
            self.edit_area.update(cx, |s, cx| {
                let end = draft.len();
                s.set_value(draft, window, cx);
                // The caret after the text, as a line's editor has it.
                s.set_selected_range(end..end, cx);
                s.focus(window, cx);
            });
        } else {
            self.edit_input.update(cx, |s, cx| {
                s.set_value(draft, window, cx);
                if select_all {
                    s.select_all(window, cx);
                }
                s.focus(window, cx);
            });
        }
        self.edit = Some(edit);
        self.chars_stale = true;
        cx.notify();
    }

    /// Edit a cell of the grid; `typed` starts it with that text instead of its value.
    fn edit_cell(&mut self, index: usize, field: String, typed: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = self.columns.iter().find(|c| c.name == field) else { return };
        if !c.editable() {
            return;
        }
        let value = self.cell(index, &field).cloned().unwrap_or(J::Null);
        if value.get("rows").is_some() {
            return;
        }
        if is_flag(&field) && typed.is_none() && matches!(value.as_str(), Some("y" | "n")) {
            self.toggle_flag(index, field, cx);
            return;
        }
        let text = cell_text(&value);
        self.col = Some(field.clone());
        self.reveal_col = true;
        let all = typed.is_none();
        self.start_edit(Edit::Cell { row: index, field }, typed.unwrap_or_else(|| text.clone()), text, all, window, cx);
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if self.edit.take().is_some() {
            self.chars_stale = true;
            cx.notify();
        }
    }

    /// Apply the open edit (an unchanged text writes nothing).
    fn commit_edit(&mut self, mv: Move, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.edit.take() else { return };
        self.chars_stale = true;
        let text = if edit.in_area() { self.edit_area.read(cx).value().to_string() } else { self.edit_input.read(cx).value().to_string() };
        let changed = text != self.edit_from;
        // The keyboard goes back to the grid (not when the user left for another field).
        let here = if edit.in_area() { self.edit_area.read(cx).focus_handle(cx).is_focused(window) } else { self.edit_input.read(cx).focus_handle(cx).is_focused(window) };
        if here {
            self.focus.focus(window, cx);
        }
        match edit {
            Edit::Message { id, lang } => {
                if changed {
                    self.set_message(id, lang, text, cx);
                }
            }
            Edit::Header { doc, field } => {
                if changed {
                    self.set_header(doc, field, text, cx);
                }
            }
            Edit::Field { row, field, .. } => {
                if changed {
                    self.set_cell(row, field, text, cx);
                }
            }
            Edit::Cell { row, field } => {
                if changed {
                    self.set_cell(row, field, text, cx);
                }
                match mv {
                    Move::Down => {
                        if let Some(pos) = self.positions.get(&row).copied() {
                            if pos + 1 < self.total {
                                self.marked.clear();
                                self.select_pos(pos + 1, cx);
                            }
                        }
                    }
                    Move::Right => self.move_col(1, cx),
                    Move::Left => self.move_col(-1, cx),
                    Move::Stay => {}
                }
            }
        }
        cx.notify();
    }

    /// Set a cell: on every marked row when `index` is one of several marked rows.
    fn set_cell(&mut self, index: usize, field: String, text: String, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else { return };
        if self.marked.len() > 1 && self.marked.contains(&index) {
            self.set_many(doc, self.marked.iter().copied().collect(), field, text, cx);
            return;
        }
        let call = self.backend.call("mcts.set", json!({ "doc": doc, "path": self.row_path(index), "field": field, "text": text }));
        let seq = self.seq;
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.status(&r.json, cx);
                    if this.seq != seq {
                        return;
                    }
                    if let (Some(cells), Some(pos)) = (r.json["row"].as_object(), this.positions.get(&index)) {
                        if let Some(row) = this.rows.get_mut(pos) {
                            row.1 = cells.clone();
                        }
                    }
                    this.chars_stale = true;
                    if is_message(&field) {
                        this.want_labels(cx);
                    }
                    this.set_status(format!("Set {field}"), cx);
                    if this.selected == Some(index) {
                        this.select(index, cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("Could not set {field}: {e}"), cx),
            });
        })
        .detach();
    }

    /// Set one field on several rows (one request).
    fn set_many(&mut self, doc: String, rows: Vec<usize>, field: String, text: String, cx: &mut Context<Self>) {
        let paths: Vec<J> = rows.iter().map(|&i| self.row_path(i)).collect();
        let call = self.backend.call("mcts.setMany", json!({ "doc": doc.clone(), "paths": paths, "field": field, "text": text }));
        let seq = self.seq;
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.status(&r.json, cx);
                    if this.seq != seq {
                        return;
                    }
                    for (index, cells) in rows.iter().zip(r.json["rows"].as_array().into_iter().flatten()) {
                        if let (Some(cells), Some(pos)) = (cells.as_object(), this.positions.get(index)) {
                            if let Some(row) = this.rows.get_mut(pos) {
                                row.1 = cells.clone();
                            }
                        }
                    }
                    this.chars_stale = true;
                    if is_message(&field) {
                        this.want_labels(cx);
                    }
                    if let Some(s) = this.selected.filter(|s| rows.contains(s)) {
                        this.select(s, cx);
                    }
                    if rows.len() > 1 {
                        this.toast(Tone::Success, format!("{field} set on {} rows", rows.len()), cx);
                    }
                }
                Err(e) => {
                    this.toast(Tone::Error, format!("Could not set {field}: {e}"), cx);
                    // Some rows may have changed before the error.
                    this.refresh_doc(doc, cx);
                    this.reload_rows(cx);
                }
            });
        })
        .detach();
    }

    fn toggle_flag(&mut self, index: usize, field: String, cx: &mut Context<Self>) {
        let on = self.cell(index, &field).and_then(|v| v.as_str()) == Some("y");
        self.set_cell(index, field, if on { "n" } else { "y" }.into(), cx);
    }

    fn set_message(&mut self, id: i64, lang: String, text: String, cx: &mut Context<Self>) {
        let call = self.backend.call("mcts.setMessage", json!({ "lang": lang, "id": id, "text": text }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(_) => {
                    let entry = this.messages.entry(id).or_default();
                    match entry.get_mut(&lang) {
                        Some(m) => m.text = text.clone(),
                        None => {
                            entry.insert(lang.clone(), Message { text: text.clone(), name: String::new(), path: Vec::new() });
                        }
                    }
                    if lang == "INT" {
                        this.labels.insert(id, text);
                        this.chars_stale = true;
                    }
                    this.set_status(format!("Set message {id} in {lang}"), cx);
                    // A message added to a language is a new row of its file.
                    this.refresh_doc(lang, cx);
                    cx.notify();
                }
                Err(e) => this.toast(Tone::Error, format!("Could not set the message: {e}"), cx),
            });
        })
        .detach();
    }

    fn set_header(&mut self, doc: String, field: String, text: String, cx: &mut Context<Self>) {
        let text = if field == "GAME_VERSION_RAW" {
            match version_raw(&text) {
                Some(raw) => raw,
                None => {
                    self.toast(Tone::Warning, "A game version is four numbers: major.minor.build.revision", cx);
                    return;
                }
            }
        } else {
            text
        };
        let call = self.backend.call("mcts.setHeader", json!({ "doc": doc.clone(), "field": field, "text": text }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.status(&r.json, cx);
                    this.refresh_doc(doc, cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not set {field}: {e}"), cx),
            });
        })
        .detach();
    }

    /// Append a row at the end of the set: empty, or a copy of row `clone` with a fresh id and its own messages.
    pub(super) fn add_row(&mut self, clone: Option<usize>, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        let mut params = json!({ "doc": doc.clone(), "path": path_json(&self.path), "table": table, "messages": true });
        if let Some(c) = clone {
            params["clone"] = json!(c);
        }
        let call = self.backend.call("mcts.addRow", params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    let index = r.json["index"].as_u64().unwrap_or(0) as usize;
                    let what = match clone {
                        Some(c) => format!("Copy of row {c}"),
                        None => "New row".into(),
                    };
                    let id = match (r.json["key"].as_str(), r.json["id"].as_i64()) {
                        (Some(k), Some(id)) => format!(" with {k} {id}"),
                        _ => String::new(),
                    };
                    let msgs = match r.json["messages"].as_array().map(|m| m.len()).unwrap_or(0) {
                        0 => String::new(),
                        n => format!(", {n} messages copied"),
                    };
                    this.toast(Tone::Success, format!("{what} added at the end{id}{msgs}"), cx);
                    this.refresh_doc(doc, cx);
                    // New rows go at the end: shown unfiltered, in file order.
                    this.marked.clear();
                    this.sort = None;
                    this.filter_text.clear();
                    this.pending_select = Some(index);
                    this.reload_rows(cx);
                    this.record(cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not add the row: {e}"), cx),
            });
        })
        .detach();
    }

    /// A new empty row (the command).
    pub(super) fn new_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.update(cx, |s, cx| s.set_value("", window, cx));
        self.add_row(None, cx);
    }

    /// A copy of the selected row (the command).
    pub(super) fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.selected else {
            self.toast(Tone::Info, "Select a row first", cx);
            return;
        };
        self.filter.update(cx, |s, cx| s.set_value("", window, cx));
        self.add_row(Some(i), cx);
    }

    /// The rows an action applies to: the marked ones, else the selected one.
    fn targets(&self) -> Vec<usize> {
        if !self.marked.is_empty() {
            self.marked.iter().copied().collect()
        } else {
            self.selected.into_iter().collect()
        }
    }

    /// Delete the selected or marked rows, after asking.
    pub(super) fn delete_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        let rows = self.targets();
        if rows.is_empty() {
            return;
        }
        let (key, name) = (self.key_column(), self.name_column());
        let mut list: Vec<String> = rows
            .iter()
            .take(12)
            .map(|&i| {
                let cell = |c: &Option<String>| c.as_ref().and_then(|c| self.cell(i, c)).map(cell_text).unwrap_or_default();
                format!("#{i} {} {}", cell(&key), cell(&name)).trim().to_string()
            })
            .collect();
        if rows.len() > list.len() {
            list.push(format!("… and {} more", rows.len() - list.len()));
        }
        let many = rows.len() > 1;
        let title = if many { format!("Delete {} rows?", rows.len()) } else { "Delete this row?".to_string() };
        let body = format!("From {}: {}. Undo brings {} back until you discard.", short(&table), list.join(", "), if many { "them" } else { "it" });
        let paths: Vec<J> = rows.iter().map(|&i| self.row_path(i)).collect();
        let me = cx.entity().downgrade();
        dialogs::confirm(window, cx, title, body, "Delete", true, move |_, cx| {
            let (doc, paths) = (doc.clone(), paths.clone());
            let _ = me.update(cx, |this, cx| {
                let call = this.backend.call("mcts.deleteMany", json!({ "doc": doc.clone(), "paths": paths }));
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| {
                        match r {
                            Ok(r) => this.status(&r.json, cx),
                            Err(e) => this.toast(Tone::Error, format!("Could not delete the rows: {e}"), cx),
                        }
                        this.marked.clear();
                        this.anchor = None;
                        this.selected = None;
                        this.row = None;
                        this.refresh_doc(doc, cx);
                        this.reload_rows(cx);
                    });
                })
                .detach();
            });
        });
    }

    pub fn undo(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.step(false, cx);
    }

    pub fn redo(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.step(true, cx);
    }

    /// Undo the last edit of the file on screen, or redo the last one undone.
    fn step(&mut self, redo: bool, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else { return };
        let call = self.backend.call(if redo { "mcts.redo" } else { "mcts.undo" }, json!({ "doc": doc.clone() }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.status(&r.json, cx);
                    let what = match (redo, r.json[if redo { "redone" } else { "undone" }].as_str()) {
                        (false, Some(u)) => format!("Undone: {u}"),
                        (true, Some(u)) => format!("Redone: {u}"),
                        (false, None) => "Nothing to undo in this file".into(),
                        (true, None) => "Nothing to redo in this file".into(),
                    };
                    this.set_status(what, cx);
                    this.after_rewrite(doc, cx);
                }
                Err(e) => this.toast(Tone::Error, format!("{} failed: {e}", if redo { "Redo" } else { "Undo" }), cx),
            });
        })
        .detach();
    }

    /// A file's rows changed under the grid (undo, discard): read what is on screen again.
    fn after_rewrite(&mut self, doc: String, cx: &mut Context<Self>) {
        self.messages.clear();
        self.labels.clear();
        self.label_wanted.clear();
        self.refresh_doc(doc, cx);
        // The view stays where it is: the rows are swapped when they arrive.
        self.pending_select = None;
        self.reload_rows(cx);
        if let Some(i) = self.selected {
            self.select(i, cx);
        }
    }

    /// The rows filter, for ⌘F.
    pub(crate) fn filter_input(&self) -> Entity<InputState> {
        self.filter.clone()
    }

    /// Write the files with changes into the game folder, after asking (for quick tests; a mod does not need it).
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dirty: Vec<(String, String)> = self.docs.iter().filter(|d| d.dirty).map(|d| (d.id.clone(), d.path.clone())).collect();
        if dirty.is_empty() {
            self.toast(Tone::Info, "No game data file has changes", cx);
            return;
        }
        let names = dirty.iter().map(|d| file_name(&d.1)).collect::<Vec<_>>().join(", ");
        let me = cx.entity().downgrade();
        dialogs::confirm(window, cx, format!("Overwrite {names} in the game folder?"), "This writes your changes straight into the game's own files, to try them without building a mod. The Studio cannot put the original files back afterwards: your launcher's \"Verify game files\" does. A mod never needs this.", "Overwrite", true, move |_, cx| {
            let dirty = dirty.clone();
            let _ = me.update(cx, |this, cx| this.save_docs(dirty.clone(), None, cx));
        });
    }

    /// Write copies of the files with edits into a folder the user picks (the edits stay unsaved here).
    pub(super) fn export_copies(&mut self, cx: &mut Context<Self>) {
        let docs: Vec<(String, String)> = self.docs.iter().filter(|d| d.dirty).map(|d| (d.id.clone(), d.path.clone())).collect();
        if docs.is_empty() {
            self.toast(Tone::Info, "No assembly file has edits to export", cx);
            return;
        }
        let pick = cx.prompt_for_paths(gpui::PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Export here".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = pick.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            let _ = this.update(cx, |this, cx| this.save_docs(docs, Some(dir.to_string_lossy().to_string()), cx));
        })
        .detach();
    }

    fn save_docs(&mut self, docs: Vec<(String, String)>, out_dir: Option<String>, cx: &mut Context<Self>) {
        for (doc, path) in docs {
            let mut params = json!({ "doc": doc });
            if let Some(dir) = &out_dir {
                params["out"] = json!(std::path::Path::new(dir).join(file_name(&path)).to_string_lossy());
            }
            let copy = out_dir.is_some();
            let call = self.backend.call("mcts.save", params);
            cx.spawn(async move |this, cx| {
                let r = call.await;
                let _ = this.update(cx, |this, cx| match r {
                    Ok(r) => {
                        this.status(&r.json, cx);
                        let out = r.json["out"].as_str().unwrap_or(&path).to_string();
                        if copy {
                            this.toast(Tone::Success, format!("Copy exported: {out}. The edits stay here; the Mod view lists the copy."), cx);
                        } else {
                            this.toast(Tone::Success, format!("Saved {}", file_name(&path)), cx);
                        }
                        this.with_studio(cx, |s, cx| s.assembly_saved(cx));
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not save {}: {e}", file_name(&path)), cx),
                });
            })
            .detach();
        }
    }

    /// Drop the unsaved edits of the file on screen, after asking.
    pub(super) fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.current_doc().filter(|d| d.dirty) else {
            self.toast(Tone::Info, "This file has no changes", cx);
            return;
        };
        let (id, name) = (doc.id.clone(), file_name(&doc.path));
        let me = cx.entity().downgrade();
        dialogs::confirm(window, cx, format!("Discard the changes to {name}?"), "Every edit of this file is lost and it is read again from the game folder. Copies you exported and files saved in place are not touched.", "Discard changes", true, move |_, cx| {
            let id = id.clone();
            let _ = me.update(cx, |this, cx| this.discard_doc(id, cx));
        });
    }

    /// Drop the unsaved edits of every file (Discard all, already confirmed).
    pub(super) fn discard_all(&mut self, cx: &mut Context<Self>) {
        for id in self.docs.iter().filter(|d| d.dirty).map(|d| d.id.clone()).collect::<Vec<_>>() {
            self.discard_doc(id, cx);
        }
    }

    fn discard_doc(&mut self, doc: String, cx: &mut Context<Self>) {
        let call = self.backend.call("mcts.discard", json!({ "doc": doc.clone() }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.status(&r.json, cx);
                    this.after_rewrite(doc, cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not discard: {e}"), cx),
            });
        })
        .detach();
    }

    // ----- clipboard --------------------------------------------------------------------

    /// Copy the rows in full as JSON (nested sets included): ⌘V in the same table appends them.
    pub(super) fn copy_rows(&mut self, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        let rows = self.targets();
        if rows.is_empty() {
            return;
        }
        let calls: Vec<_> = rows.iter().map(|&i| self.backend.call("mcts.rowFull", json!({ "doc": doc, "path": self.row_path(i) }))).collect();
        cx.spawn(async move |this, cx| {
            let mut out = Vec::new();
            for c in calls {
                match c.await {
                    Ok(r) => out.push(r.json),
                    Err(e) => {
                        let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not copy: {e}"), cx));
                        return;
                    }
                }
            }
            let n = out.len();
            let text = serde_json::to_string_pretty(&json!({ "tempestRows": table, "rows": out })).unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                this.toast(Tone::Success, format!("{n} row(s) copied; paste them into this table with {}", super::commands::key_hint("⌘V")), cx);
            });
        })
        .detach();
    }

    /// The visible cells of the rows as tab-separated text (for spreadsheets).
    fn copy_tsv(&mut self, cx: &mut Context<Self>) {
        let rows = self.targets();
        let cols = self.visible_columns();
        let mut lines = vec![cols.iter().map(|c| c.name.clone()).collect::<Vec<_>>().join("\t")];
        let mut at: Vec<usize> = rows.iter().filter_map(|i| self.positions.get(i).copied()).collect();
        at.sort_unstable();
        for pos in at {
            if let Some((_, cells)) = self.rows.get(&pos) {
                lines.push(cols.iter().map(|c| cell_text(cells.get(&c.name).unwrap_or(&J::Null))).collect::<Vec<_>>().join("\t"));
            }
        }
        let n = lines.len() - 1;
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(lines.join("\n")));
        self.set_status(format!("Copied {n} row(s) as tab-separated text"), cx);
    }

    /// Paste: rows copied with Copy are appended with fresh ids; plain text sets the active cell (of every marked row).
    pub(super) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else { return };
        if self.paste_rows(&text, window, cx) {
            return;
        }
        let (Some(index), Some(col)) = (self.selected, self.col.clone()) else { return };
        let value = text.strip_suffix('\n').map(|t| t.strip_suffix('\r').unwrap_or(t)).unwrap_or(&text);
        if value.contains('\n') {
            self.toast(Tone::Info, "The clipboard holds several lines; paste one value at a time.", cx);
            return;
        }
        if !self.columns.iter().any(|c| c.name == col && c.editable()) || self.cell(index, &col).is_some_and(|v| v.get("rows").is_some()) {
            return;
        }
        self.set_cell(index, col, value.to_string(), cx);
    }

    /// Paste rows copied with `copy_rows`; false when the clipboard holds none.
    fn paste_rows(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return false };
        let Ok(data) = serde_json::from_str::<J>(text) else { return false };
        let (Some(from), Some(rows)) = (data["tempestRows"].as_str(), data["rows"].as_array()) else { return false };
        if from != table {
            self.toast(Tone::Warning, format!("The clipboard holds {from} rows; open that table to paste them."), cx);
            return true;
        }
        let path = path_json(&self.path);
        let calls: Vec<_> = rows.iter().map(|row| self.backend.call("mcts.addRow", json!({ "doc": doc, "path": path, "table": table, "row": row }))).collect();
        self.filter.update(cx, |s, cx| s.set_value("", window, cx));
        cx.spawn(async move |this, cx| {
            let (mut last, mut n) = (None, 0);
            for c in calls {
                match c.await {
                    Ok(r) => {
                        last = r.json["index"].as_u64().map(|i| i as usize);
                        n += 1;
                    }
                    Err(e) => {
                        let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not paste a row: {e}"), cx));
                        break;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.refresh_doc(doc, cx);
                if n > 0 {
                    this.toast(Tone::Success, format!("{n} row(s) pasted at the end with fresh ids"), cx);
                    this.marked.clear();
                    this.sort = None;
                    this.filter_text.clear();
                    this.pending_select = last;
                    this.reload_rows(cx);
                }
            });
        })
        .detach();
        true
    }

    // ----- columns ------------------------------------------------------------------------

    /// The table's id column (`DATA_SET_BOTS` → `BOT_ID`), when it has one.
    fn key_column(&self) -> Option<String> {
        let names = key_names(self.table.as_ref()?);
        names.into_iter().find(|n| self.columns.iter().any(|c| &c.name == n))
    }

    fn name_column(&self) -> Option<String> {
        ["REFERENCE_NAME", "NAME", "MESSAGE", "NAME_MSG_ID"].into_iter().find(|n| self.columns.iter().any(|c| c.name == *n)).map(str::to_string)
    }

    /// The id and name columns, pinned at the left of the grid.
    fn pinned(&self) -> Vec<String> {
        self.key_column().into_iter().chain(self.name_column()).collect()
    }

    /// The columns shown: the pinned ones first, then the table's less the hidden ones.
    fn visible_columns(&self) -> Vec<Column> {
        let hidden = self.table.as_ref().and_then(|t| self.hidden.get(t));
        let pinned = self.pinned();
        let first = pinned.iter().filter_map(|n| self.columns.iter().find(|c| &c.name == n));
        first.chain(self.columns.iter().filter(|c| !pinned.contains(&c.name) && hidden.is_none_or(|h| !h.contains(&c.name)))).cloned().collect()
    }

    fn toggle_column(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(t) = self.table.clone() else { return };
        let h = self.hidden.entry(t).or_default();
        if !h.remove(&name) {
            h.insert(name);
        }
        save_hidden(&self.hidden);
        self.chars_stale = true;
        cx.notify();
    }

    fn show_all_columns(&mut self, cx: &mut Context<Self>) {
        let Some(t) = &self.table else { return };
        if self.hidden.remove(t).is_some() {
            save_hidden(&self.hidden);
            self.chars_stale = true;
            cx.notify();
        }
    }

    // ----- state for agents and tests -------------------------------------------------------

    /// What the view shows (`studio_state.assembly`, the driver's `assembly dump`).
    pub fn state_json(&self) -> J {
        json!({
            "file": self.doc,
            "path": path_json(&self.path),
            "table": self.table,
            "row": self.selected,
            "column": self.col,
            "total": self.total,
            "loaded": self.rows.len(),
            "filter": self.filter_text,
            "sort": self.sort.as_ref().map(|(c, d)| json!({ "column": c, "desc": d })),
            "marked": self.marked.iter().collect::<Vec<_>>(),
            "columns": self.visible_columns().iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
            "editing": self.edit.as_ref().map(|e| format!("{e:?}")),
            "fields": self.row.as_ref().map(|r| r.fields.len()),
            "usages": self.row.as_ref().and_then(|r| r.usages.as_ref()).map(|u| u.0.len()),
            "unsaved": self.docs.iter().filter(|d| d.dirty).map(|d| d.id.clone()).collect::<Vec<_>>(),
            "undo": self.current_doc().map(|d| d.undo.clone()),
            "error": self.error,
        })
    }

    /// Dev: `assembly <step>` of the test driver.
    pub fn drive(&mut self, arg: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let (verb, rest) = arg.split_once(' ').unwrap_or((arg, ""));
        let rest = rest.trim();
        match verb {
            "table" => {
                let (doc, table) = match rest.split_once(' ') {
                    Some((d, t)) => (d.to_string(), t.to_string()),
                    None => ("assembly".to_string(), rest.to_string()),
                };
                self.open_table(doc, table, window, cx);
            }
            "row" => {
                let i: usize = rest.parse().map_err(|_| "row <index>")?;
                self.marked.clear();
                self.anchor = Some(i);
                self.focus.focus(window, cx);
                self.select(i, cx);
                self.reveal_index(i);
                self.record(cx);
            }
            "col" => self.col = Some(rest.to_string()),
            "mark" => {
                let i: usize = rest.parse().map_err(|_| "mark <index>")?;
                if let Some(s) = self.selected {
                    self.marked.insert(s);
                }
                self.marked.insert(i);
            }
            "filter" => self.set_filter(rest.to_string(), window, cx),
            "set" => {
                let (field, text) = rest.split_once('=').ok_or("set <field>=<text>")?;
                let index = self.selected.ok_or("no row selected")?;
                self.set_cell(index, field.to_string(), text.to_string(), cx);
            }
            "edit" => {
                let index = self.selected.ok_or("no row selected")?;
                let field = if rest.is_empty() { self.col.clone().ok_or("no column")? } else { rest.to_string() };
                self.edit_cell(index, field, None, window, cx);
            }
            "message" => {
                // message <id> <lang> <text>
                let mut w = rest.splitn(3, ' ');
                let (id, lang, text) = (w.next().and_then(|i| i.parse().ok()).ok_or("message <id> <lang> <text>")?, w.next().ok_or("message <id> <lang> <text>")?, w.next().unwrap_or(""));
                self.set_message(id, lang.to_string(), text.replace("\\n", "\n"), cx);
            }
            "header" => {
                // header <doc> <field>=<text>
                let (doc, rest) = rest.split_once(' ').ok_or("header <doc> <field>=<text>")?;
                let (field, text) = rest.split_once('=').ok_or("header <doc> <field>=<text>")?;
                self.set_header(doc.to_string(), field.to_string(), text.to_string(), cx);
            }
            "open" => {
                let index = self.selected.ok_or("no row selected")?;
                self.open_set(index, rest.to_string(), window, cx);
            }
            "up" => self.up_to(0, window, cx),
            "follow" => {
                let index = self.selected.ok_or("no row selected")?;
                let v = self.cell(index, rest).cloned().ok_or("no such cell")?;
                self.follow(rest, &v, false, window, cx);
            }
            "usages" => self.find_usages(cx),
            "usage" => {
                let k: usize = rest.parse().map_err(|_| "usage <n>")?;
                let path = self.row.as_ref().and_then(|r| r.usages.as_ref()).and_then(|u| u.0.get(k)).map(|u| u.path.clone()).ok_or("no such usage")?;
                self.go_to_path(path, false, window, cx);
            }
            "add" => self.add_row(None, cx),
            "duplicate" => self.add_row(self.selected, cx),
            "delete" => self.delete_rows(window, cx),
            "undo" => self.undo(window, cx),
            "redo" => self.redo(window, cx),
            "copy" => self.copy_rows(cx),
            "tsv" => self.copy_tsv(cx),
            "paste" => self.paste(window, cx),
            "hide" => self.toggle_column(rest.to_string(), cx),
            "show-all" => self.show_all_columns(cx),
            "reload" => self.open(true, cx),
            "new-content" => self.new_content(window, cx),
            "vendor" => self.champion_vendor(window, cx),
            "skins" => self.skins(window, cx),
            "export-mod" => self.export_mod(window, cx),
            "sort" => self.set_sort(rest.to_string(), cx),
            "dump" => {
                let mut j = self.state_json();
                // The selected row as the grid holds it, and the row pane's fields.
                j["cells"] = self.selected.and_then(|i| self.cells(i)).map(|c| J::Object(c.clone())).unwrap_or(J::Null);
                j["messages"] = json!(self.messages.len());
                j["widths"] = J::from(self.last_widths());
                println!("{j}");
            }
            _ => return Err(format!("unknown assembly step {verb}")),
        }
        cx.notify();
        Ok(())
    }
}

/// The names a table's id column can have (`DATA_SET_ABILITIES` → `ABILITIES_ID`, `ABILITY_ID`, `ABILITI_ID`…).
fn key_names(table: &str) -> Vec<String> {
    let stem = short(table);
    [Some(format!("{stem}_ID")), stem.strip_suffix("IES").map(|s| format!("{s}Y_ID")), stem.strip_suffix("ES").map(|s| format!("{s}_ID")), stem.strip_suffix('S').map(|s| format!("{s}_ID"))].into_iter().flatten().collect()
}

impl Render for Assembly {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.docs.is_empty() {
            return self.render_empty(cx).into_any_element();
        }
        let side = self.render_side(cx);
        let grid = self.render_grid(window, cx);
        let row = self.render_row(cx);
        div()
            .key_context("Assembly")
            .track_focus(&self.focus)
            .size_full()
            .on_action(cx.listener(|this, _: &super::Up, window, cx| this.key_step(-1, false, window, cx)))
            .on_action(cx.listener(|this, _: &super::Down, window, cx| this.key_step(1, false, window, cx)))
            .on_action(cx.listener(|this, _: &super::ExtendUp, window, cx| this.key_step(-1, true, window, cx)))
            .on_action(cx.listener(|this, _: &super::ExtendDown, window, cx| this.key_step(1, true, window, cx)))
            .on_action(cx.listener(|this, _: &super::Left, window, cx| this.key_col(-1, window, cx)))
            .on_action(cx.listener(|this, _: &super::Right, window, cx| this.key_col(1, window, cx)))
            .on_action(cx.listener(|this, _: &super::NextCell, window, cx| this.key_tab(1, window, cx)))
            .on_action(cx.listener(|this, _: &super::PrevCell, window, cx| this.key_tab(-1, window, cx)))
            .on_action(cx.listener(|this, _: &super::First, window, cx| this.key_end(false, window, cx)))
            .on_action(cx.listener(|this, _: &super::Last, window, cx| this.key_end(true, window, cx)))
            .on_action(cx.listener(|this, _: &super::PageUp, window, cx| this.key_page(-1, window, cx)))
            .on_action(cx.listener(|this, _: &super::PageDown, window, cx| this.key_page(1, window, cx)))
            .on_action(cx.listener(|this, _: &super::Activate, window, cx| this.key_activate(window, cx)))
            .on_action(cx.listener(|this, _: &super::Cancel, window, cx| this.key_cancel(window, cx)))
            .on_action(cx.listener(|this, _: &super::SelectAll, window, cx| this.key_select_all(window, cx)))
            .on_action(cx.listener(|this, _: &super::Delete, window, cx| this.key_delete(window, cx)))
            .on_action(cx.listener(|this, _: &super::CopyRows, window, cx| this.key_copy(window, cx)))
            .on_action(cx.listener(|this, _: &super::PasteRows, window, cx| this.key_paste(window, cx)))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| this.key_typed(e, window, cx)))
            .child({
                use super::layout::{group, Part, Role};
                let studio = self.studio.clone();
                group(
                    "assembly",
                    self.panes.get(),
                    vec![Part(Role::Side, Some(self.sizes[0]), side), Part(Role::Main, None, grid), Part(Role::Second, Some(self.sizes[1]), row)],
                    Studio::remember_panes(self.studio.clone(), "assembly"),
                    move |_, cx| {
                        let _ = studio.update(cx, |s, cx| s.close_overlay(cx));
                    },
                    cx,
                )
            })
            .into_any_element()
    }
}

impl Assembly {
    /// Nothing read yet: reading, or no assembly next to the packages.
    fn render_empty(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::button::Button;
        use gpui_component::Sizable;
        let theme = cx.theme().clone();
        if self.loading {
            return super::ui::loading("Reading the game data (assembly.dat and the language files)…", &theme);
        }
        let detail = self.error.clone().unwrap_or_else(|| "There is no assembly.dat next to this folder's packages. Open the game's ChaosGame/CookedPCConsole folder.".into());
        super::ui::error("The game data could not be read", detail, &theme).child(Button::new("asm-retry").small().label("Try again").on_click(cx.listener(|this, _, _, cx| this.open(false, cx))))
    }
}

/// `<config>/tempest-studio-native/assembly-columns.json`: hidden columns by table.
fn load_hidden() -> HashMap<String, HashSet<String>> {
    super::state_file("assembly-columns.json").and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_hidden(h: &HashMap<String, HashSet<String>>) {
    let Some(f) = super::state_file("assembly-columns.json") else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string(h) {
        let _ = std::fs::write(f, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_follow_the_column_names() {
        assert_eq!(ref_of("CARD_RES_ID"), Some(("DATA_SET_RESOURCES", "RES_ID")));
        assert_eq!(ref_of("ITEM_ID"), Some(("DATA_SET_ITEMS", "ITEM_ID")));
        // The most specific suffix wins, and a message id is not a reference.
        assert_eq!(ref_of("DEVICE_PROJECTILE_ID"), Some(("DATA_SET_PROJECTILES", "DEVICE_PROJECTILE_ID")));
        assert_eq!(ref_of("NAME_MSG_ID"), None);
        // A suffix must be a whole word of the name.
        assert_eq!(ref_of("XITEM_ID"), None);
    }

    #[test]
    fn versions_round_trip() {
        let raw = version_raw("8.1.5839.0").unwrap();
        assert_eq!(version_text(&J::from(raw.clone())).as_deref(), Some("8.1.5839.0"));
        assert_eq!(header_text("GAME_VERSION_RAW", &J::from(raw)), "8.1.5839.0");
        assert_eq!(version_raw("8.1.5839"), None);
        assert_eq!(version_raw("8.1.70000.0"), None);
    }

    #[test]
    fn key_columns_by_table_name() {
        assert!(key_names("DATA_SET_BOTS").contains(&"BOT_ID".to_string()));
        assert!(key_names("DATA_SET_PROPERTIES").contains(&"PROPERTY_ID".to_string()));
        assert!(key_names("DATA_SET_RESOURCES").contains(&"RESOURCE_ID".to_string()));
    }
}
