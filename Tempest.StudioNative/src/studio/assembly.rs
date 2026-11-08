//! The Assembly view: the game's content database (`assembly.dat`) and the
//! language files (`Localization/<EXT>/Lang_<EXT>.dat`), MCTS marshals the
//! backend edits (`mcts.*`). Left: the files and their tables. Middle: the
//! table as a grid, paged 500 rows at a time (only the rows in view are
//! laid out), filtered over every cell, sorted by a column; a cell edits on
//! double-click, a row set opens in place (breadcrumbs), `*_MSG_ID` and
//! `*_VALUE_ID` values show their text. Right: the selected row's fields,
//! each message in every language (editable), the `*_ID` links and the
//! rows that refer to it. New row, Duplicate, Delete, Undo, Save, Discard.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use gpui::{div, prelude::*, px, uniform_list, AnyElement, Context, Entity, FocusHandle, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, WeakEntity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::DropdownMenu;
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use serde_json::{json, Map, Value as J};

use super::{dialogs, file_name, Place, Studio, Tone};
use crate::backend::Backend;

const PAGE: usize = 500;
const ROW_H: f32 = 24.0;
const COL_W: f32 = 150.0;

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
    if column.ends_with("_MSG_ID") {
        return None;
    }
    REFS.iter().find(|(suffix, _, _)| column == *suffix || column.ends_with(&format!("_{suffix}"))).map(|(_, t, k)| (*t, *k))
}

fn cell_text(v: &J) -> String {
    match v {
        J::Null => String::new(),
        J::String(s) => s.clone(),
        J::Object(o) if o.contains_key("rows") => format!("{} rows ▸", o["rows"]),
        other => other.to_string(),
    }
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

#[derive(Clone)]
struct Column {
    name: String,
    kind: String,
}

struct Doc {
    id: String,
    path: String,
    dirty: bool,
    undo: usize,
    header: Vec<(String, J)>,
    tables: Vec<(String, usize, Vec<Column>)>,
}

/// The selected row's pane.
struct RowPane {
    index: usize,
    fields: Vec<(String, String, J)>,
    /// Message id → language → text, for the row's `*_MSG_ID` fields.
    messages: HashMap<i64, Vec<(String, String)>>,
    usages: Option<Vec<(J, String, String)>>,
}

pub struct Assembly {
    pub(super) backend: Backend,
    studio: WeakEntity<Studio>,
    dir: String,
    docs: Vec<Doc>,
    loading: bool,
    error: Option<String>,
    doc: Option<String>,
    /// The row set on screen: the rows leading to it, and its table / field name.
    path: Vec<(String, usize)>,
    table: Option<String>,
    columns: Vec<Column>,
    total: usize,
    /// Loaded rows by position: (row index, cells).
    rows: HashMap<usize, (usize, Map<String, J>)>,
    pages: HashSet<usize>,
    /// Bumped whenever the pages are dropped, so late pages are ignored.
    seq: u64,
    filter: Entity<InputState>,
    table_filter: Entity<InputState>,
    sort: Option<(String, bool)>,
    selected: Option<usize>,
    row: Option<RowPane>,
    /// The cell (or field, `row: None` for the header) being edited.
    edit: Option<(Option<usize>, String, Option<String>)>,
    edit_input: Entity<InputState>,
    scroll: UniformListScrollHandle,
    msg_text: HashMap<i64, String>,
    valid: HashMap<i64, String>,
    msg_wanted: HashSet<i64>,
    pending_select: Option<usize>,
    /// Rows marked with ⌘/⇧-click: an edit of one of them sets them all.
    marked: std::collections::BTreeSet<usize>,
    /// Hidden columns per table (kept between runs).
    hidden: HashMap<String, HashSet<String>>,
    /// The grid's sideways scroll (the id and name columns stay at the left).
    hscroll: gpui::ScrollHandle,
    /// ⌘B / ⌘J and the compact layout (the window's).
    panes: super::layout::Shared,
    /// The files and row panes' widths when the view opened.
    sizes: Vec<f32>,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl Assembly {
    pub fn new(backend: Backend, dir: String, studio: WeakEntity<Studio>, panes: super::layout::Shared, sizes: Vec<f32>, window: &mut Window, cx: &mut Context<Self>) -> Assembly {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter rows (any cell)"));
        let table_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter tables"));
        let edit_input = cx.new(|cx| InputState::new(window, cx));
        let subs = vec![
            cx.subscribe(&filter, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.reload_rows(cx);
                }
            }),
            cx.subscribe(&table_filter, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&edit_input, window, |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.commit_edit(window, cx),
                InputEvent::Blur => {
                    this.edit = None;
                    cx.notify();
                }
                _ => {}
            }),
        ];
        let mut a = Assembly {
            backend,
            studio,
            dir,
            docs: Vec::new(),
            loading: true,
            error: None,
            doc: None,
            path: Vec::new(),
            table: None,
            columns: Vec::new(),
            total: 0,
            rows: HashMap::new(),
            pages: HashSet::new(),
            seq: 0,
            filter,
            table_filter,
            sort: None,
            selected: None,
            row: None,
            edit: None,
            edit_input,
            scroll: UniformListScrollHandle::new(),
            msg_text: HashMap::new(),
            valid: HashMap::new(),
            msg_wanted: HashSet::new(),
            pending_select: None,
            marked: Default::default(),
            hidden: load_hidden(),
            hscroll: gpui::ScrollHandle::new(),
            panes,
            sizes,
            focus: cx.focus_handle(),
            _subs: subs,
        };
        a.open(false, cx);
        a
    }

    pub(super) fn toast(&self, tone: Tone, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        let _ = self.studio.update(cx, |s, cx| s.toast(tone, text, cx));
    }

    /// New content's kind and template from the selected row (a device, item or bot row).
    pub(super) fn template_guess(&self) -> (&'static str, Option<i64>) {
        let kind = match self.table.as_deref() {
            Some("DATA_SET_DEVICES") => "device",
            Some("DATA_SET_BOTS") => "champion",
            _ => "item",
        };
        let key = match kind {
            "device" => "DEVICE_ID",
            "champion" => "BOT_ID",
            _ => "ITEM_ID",
        };
        let id = self.row.as_ref().filter(|_| self.path.is_empty()).and_then(|r| r.fields.iter().find(|f| f.0 == key)).and_then(|f| f.2.as_i64());
        (kind, id)
    }

    /// The files changed behind the view's back (a workspace file applied): read them again.
    pub fn reopen(&mut self, cx: &mut Context<Self>) {
        self.open(false, cx);
        self.reload_rows(cx);
    }

    fn open(&mut self, reload: bool, cx: &mut Context<Self>) {
        self.loading = true;
        let call = self.backend.call("mcts.open", json!({ "reload": reload }));
        let valid = self.backend.call("mcts.validValues", json!({}));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let v = valid.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match r {
                    Ok(r) => {
                        this.set_docs(&r.json["docs"]);
                        if this.doc.is_none() {
                            this.doc = this.docs.first().map(|d| d.id.clone());
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                if let Ok(v) = v {
                    if let Some(m) = v.json.as_object() {
                        this.valid = m.iter().filter_map(|(k, x)| Some((k.parse().ok()?, x["text"].as_str()?.to_string()))).collect();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_docs(&mut self, docs: &J) {
        self.docs = docs
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| Doc {
                id: d["id"].as_str().unwrap_or_default().to_string(),
                path: d["path"].as_str().unwrap_or_default().to_string(),
                dirty: d["dirty"].as_bool().unwrap_or(false),
                undo: d["undo"].as_array().map(|u| u.len()).unwrap_or(0),
                header: d["header"].as_array().into_iter().flatten().map(|h| (h["name"].as_str().unwrap_or_default().to_string(), h["value"].clone())).collect(),
                tables: d["tables"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|t| {
                        let cols = t["columns"].as_array().into_iter().flatten().map(|c| Column { name: c["name"].as_str().unwrap_or_default().to_string(), kind: c["kind"].as_str().unwrap_or_default().to_string() }).collect();
                        (t["name"].as_str().unwrap_or_default().to_string(), t["rows"].as_u64().unwrap_or(0) as usize, cols)
                    })
                    .collect(),
            })
            .collect();
    }

    /// A status reply (`McsStatus`): the document's dirty flag and undo depth.
    fn status(&mut self, j: &J, cx: &mut Context<Self>) {
        if let Some(d) = self.docs.iter_mut().find(|d| j["doc"].as_str() == Some(d.id.as_str())) {
            d.dirty = j["dirty"].as_bool().unwrap_or(d.dirty);
            d.undo = j["undo"].as_array().map(|u| u.len()).unwrap_or(d.undo);
        }
        cx.notify();
    }

    pub fn unsaved_count(&self) -> usize {
        self.docs.iter().filter(|d| d.dirty).count()
    }

    pub fn unsaved_files(&self) -> Vec<String> {
        self.docs.iter().filter(|d| d.dirty).map(|d| d.path.clone()).collect()
    }

    pub fn place(&self) -> Place {
        Place::Assembly { file: self.doc.clone().unwrap_or_default(), table: self.table.clone().filter(|_| self.path.is_empty()), row: self.selected.filter(|_| self.path.is_empty()) }
    }

    fn record(&self, cx: &mut Context<Self>) {
        let place = self.place();
        let _ = self.studio.update(cx, |s, _| s.record(place));
    }

    /// Back/Forward and tabs: show this table and row.
    pub fn visit(&mut self, file: String, table: Option<String>, row: Option<usize>, cx: &mut Context<Self>) {
        if !file.is_empty() {
            self.doc = Some(file);
        }
        if table != self.table || !self.path.is_empty() {
            self.path.clear();
            self.table = table;
            self.selected = None;
            self.row = None;
            self.pending_select = row;
            self.load_table(cx);
        } else if let Some(r) = row {
            self.select(r, cx);
            if let Some(pos) = self.rows.iter().find(|(_, v)| v.0 == r).map(|(p, _)| *p) {
                self.scroll.scroll_to_item(pos, ScrollStrategy::Center);
            }
        }
        cx.notify();
    }

    // ----- tables -------------------------------------------------------------------

    fn open_table(&mut self, doc: String, table: String, cx: &mut Context<Self>) {
        super::layout::update(&self.panes, |p| p.overlay = false);
        self.doc = Some(doc);
        self.path.clear();
        self.table = Some(table);
        self.selected = None;
        self.row = None;
        self.sort = None;
        self.load_table(cx);
        self.record(cx);
    }

    fn load_table(&mut self, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        self.columns = if self.path.is_empty() { self.docs.iter().find(|d| d.id == doc).and_then(|d| d.tables.iter().find(|t| t.0 == table)).map(|t| t.2.clone()).unwrap_or_default() } else { Vec::new() };
        // Nested sets: the columns come from `mcts.table`.
        let call = self.backend.call("mcts.table", json!({ "doc": doc, "path": self.path_json(), "table": table }));
        cx.spawn(async move |this, cx| {
            if let Ok(r) = call.await {
                let _ = this.update(cx, |this, cx| {
                    this.columns = r.json["columns"].as_array().into_iter().flatten().map(|c| Column { name: c["name"].as_str().unwrap_or_default().to_string(), kind: c["kind"].as_str().unwrap_or_default().to_string() }).collect();
                    // The key and name columns first.
                    let table = this.table.clone().unwrap_or_default();
                    let key = REFS.iter().find(|r| r.1 == table).map(|r| r.2);
                    this.columns.sort_by_key(|c| {
                        if Some(c.name.as_str()) == key {
                            0
                        } else if c.name.ends_with("_NAME") || c.name.ends_with("NAME_MSG_ID") {
                            1
                        } else {
                            2
                        }
                    });
                    cx.notify();
                });
            }
        })
        .detach();
        self.reload_rows(cx);
    }

    fn path_json(&self) -> J {
        J::from(self.path.iter().map(|(f, i)| json!([f, i])).collect::<Vec<_>>())
    }

    fn row_path(&self, index: usize) -> J {
        let mut p: Vec<J> = self.path.iter().map(|(f, i)| json!([f, i])).collect();
        p.push(json!([self.table.clone().unwrap_or_default(), index]));
        J::from(p)
    }

    fn reload_rows(&mut self, cx: &mut Context<Self>) {
        self.rows.clear();
        self.pages.clear();
        self.seq += 1;
        self.total = 0;
        self.load_page(0, cx);
        cx.notify();
    }

    fn load_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        if !self.pages.insert(page) {
            return;
        }
        let filter = self.filter.read(cx).value().to_string();
        let (sort, desc) = match &self.sort {
            Some((c, d)) => (Some(c.clone()), *d),
            None => (None, false),
        };
        let seq = self.seq;
        let call = self.backend.call("mcts.rows", json!({ "doc": doc, "path": self.path_json(), "table": table, "offset": page * PAGE, "limit": PAGE, "filter": filter, "sort": sort, "desc": desc }));
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
                        }
                        this.want_messages(cx);
                        if let Some(r) = this.pending_select.take() {
                            this.select(r, cx);
                            if let Some(pos) = this.rows.iter().find(|(_, v)| v.0 == r).map(|(p, _)| *p) {
                                this.scroll.scroll_to_item(pos, ScrollStrategy::Center);
                            }
                        }
                    }
                    Err(e) => this.toast(Tone::Error, e, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// INT texts of the `*_MSG_ID` values loaded so far (once each).
    fn want_messages(&mut self, cx: &mut Context<Self>) {
        let mut ids = Vec::new();
        for (_, cells) in self.rows.values() {
            for (k, v) in cells {
                if k.ends_with("_MSG_ID") {
                    if let Some(id) = v.as_i64().filter(|&i| i > 0) {
                        if self.msg_wanted.insert(id) {
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
                            this.msg_text.insert(id, t.to_string());
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    // ----- rows ---------------------------------------------------------------------

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else { return };
        self.selected = Some(index);
        let call = self.backend.call("mcts.row", json!({ "doc": doc, "path": self.row_path(index) }));
        cx.spawn(async move |this, cx| {
            let Ok(r) = call.await else { return };
            let fields: Vec<(String, String, J)> = r.json["fields"].as_array().into_iter().flatten().map(|f| (f["name"].as_str().unwrap_or_default().to_string(), f["kind"].as_str().unwrap_or_default().to_string(), f["value"].clone())).collect();
            let ids: Vec<i64> = fields.iter().filter(|f| f.0.ends_with("_MSG_ID")).filter_map(|f| f.2.as_i64()).filter(|&i| i > 0).collect();
            let msgs = (!ids.is_empty()).then(|| this.update(cx, |this, _| this.backend.call("mcts.messages", json!({ "ids": ids })))).and_then(|r| r.ok());
            let mut messages = HashMap::new();
            if let Some(m) = msgs {
                if let Ok(m) = m.await {
                    for (id, langs) in m.json.as_object().into_iter().flatten() {
                        let mut list: Vec<(String, String)> = langs.as_object().into_iter().flatten().map(|(l, v)| (l.clone(), v["text"].as_str().unwrap_or_default().to_string())).collect();
                        list.sort_by_key(|(l, _)| (l != "INT", l.clone()));
                        if let Ok(id) = id.parse() {
                            messages.insert(id, list);
                        }
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                if this.selected == Some(index) {
                    this.row = Some(RowPane { index, fields, messages, usages: None });
                    this.load_usages(cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn user_select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.select(index, cx);
        if self.path.is_empty() {
            self.record(cx);
        }
    }

    /// The rows of the file naming the selected row's id.
    fn load_usages(&mut self, cx: &mut Context<Self>) {
        let Some(row) = &self.row else { return };
        let Some(table) = &self.table else { return };
        let key = row.fields.iter().find(|f| f.0.ends_with("_ID") && !f.0.ends_with("_MSG_ID") && ref_of(&f.0).is_some_and(|(t, _)| t == table)).or_else(|| row.fields.iter().find(|f| f.0.ends_with("_ID") && !f.0.ends_with("_MSG_ID")));
        let Some((field, _, value)) = key.cloned() else { return };
        let Some(v) = value.as_i64() else { return };
        let index = row.index;
        let call = self.backend.call("mcts.usages", json!({ "doc": self.doc, "field": field, "value": v }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                let Some(row) = this.row.as_mut().filter(|r| r.index == index) else { return };
                row.usages = Some(r.map(|r| r.json["usages"].as_array().into_iter().flatten().map(|u| (u["path"].clone(), u["field"].as_str().unwrap_or_default().to_string(), u["label"].as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default());
                cx.notify();
            });
        })
        .detach();
    }

    /// Open a nested row set (a `{rows}` cell) of row `index`.
    fn open_set(&mut self, index: usize, field: String, cx: &mut Context<Self>) {
        let Some(table) = self.table.clone() else { return };
        self.path.push((table, index));
        self.table = Some(field);
        self.selected = None;
        self.row = None;
        self.sort = None;
        self.load_table(cx);
    }

    /// Up the breadcrumb to depth `depth` (0: the top-level table).
    fn up_to(&mut self, depth: usize, cx: &mut Context<Self>) {
        if depth >= self.path.len() {
            return;
        }
        let (table, row) = self.path[depth].clone();
        self.path.truncate(depth);
        self.table = Some(table);
        self.selected = None;
        self.row = None;
        self.pending_select = Some(row);
        self.load_table(cx);
    }

    /// Follow a `*_ID` value to the row it names.
    fn follow(&mut self, column: &str, value: &J, cx: &mut Context<Self>) {
        let Some((table, key)) = ref_of(column) else { return };
        let Some(v) = value.as_i64() else { return };
        let call = self.backend.call("mcts.find", json!({ "doc": "assembly", "table": table, "key": key, "value": v }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r.ok().and_then(|r| r.json.as_array().and_then(|a| a.first()).and_then(|x| x.as_u64())) {
                Some(row) => {
                    this.doc = Some("assembly".into());
                    this.path.clear();
                    this.table = Some(table.to_string());
                    this.selected = None;
                    this.row = None;
                    this.sort = None;
                    this.pending_select = Some(row as usize);
                    this.load_table(cx);
                    let place = Place::Assembly { file: "assembly".into(), table: Some(table.to_string()), row: Some(row as usize) };
                    let _ = this.studio.update(cx, |s, _| s.record(place));
                }
                None => this.toast(Tone::Warning, format!("{table} has no row with {key} = {v}"), cx),
            });
        })
        .detach();
    }

    // ----- edits ----------------------------------------------------------------------

    fn start_edit(&mut self, row: Option<usize>, field: String, lang: Option<String>, draft: String, window: &mut Window, cx: &mut Context<Self>) {
        self.edit = Some((row, field, lang));
        self.edit_input.update(cx, |s, cx| {
            s.set_value(draft, window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((row, field, lang)) = self.edit.take() else { return };
        let text = self.edit_input.read(cx).value().to_string();
        self.focus.focus(window, cx);
        let Some(doc) = self.doc.clone() else { return };
        if let Some(lang) = lang {
            // A message: `field` is its id.
            let id: i64 = field.parse().unwrap_or(0);
            let call = self.backend.call("mcts.setMessage", json!({ "lang": lang, "id": id, "text": text }));
            cx.spawn(async move |this, cx| {
                let r = call.await;
                let _ = this.update(cx, |this, cx| match r {
                    Ok(r) => {
                        this.status(&r.json, cx);
                        if lang == "INT" {
                            this.msg_text.insert(id, text.clone());
                        }
                        if let Some(row) = &mut this.row {
                            if let Some(list) = row.messages.get_mut(&id) {
                                if let Some(m) = list.iter_mut().find(|m| m.0 == lang) {
                                    m.1 = text.clone();
                                }
                            }
                        }
                        cx.notify();
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not set the message: {e}"), cx),
                });
            })
            .detach();
            return;
        }
        let Some(index) = row else {
            // A header field.
            let value = if field == "GAME_VERSION_RAW" { version_raw(&text).map(J::from) } else { None };
            let call = self.backend.call("mcts.setHeader", json!({ "doc": doc, "field": field, "text": if value.is_some() { None } else { Some(text.clone()) }, "value": value }));
            cx.spawn(async move |this, cx| {
                let r = call.await;
                let _ = this.update(cx, |this, cx| match r {
                    Ok(r) => {
                        this.status(&r.json, cx);
                        this.open(false, cx);
                    }
                    Err(e) => this.toast(Tone::Error, format!("{field}: {e}"), cx),
                });
            })
            .detach();
            return;
        };
        if self.marked.len() > 1 && self.marked.contains(&index) {
            self.set_marked(doc, field, text, cx);
            return;
        }
        let call = self.backend.call("mcts.set", json!({ "doc": doc, "path": self.row_path(index), "field": field, "text": text }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.status(&r.json, cx);
                    if let Some(cells) = r.json["row"].as_object() {
                        for (i, c) in this.rows.values_mut() {
                            if *i == index {
                                *c = cells.clone();
                            }
                        }
                    }
                    let _ = this.studio.update(cx, |s, cx| s.set_status(format!("Set {field} (unsaved)"), cx));
                    if this.selected == Some(index) {
                        this.select(index, cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("{field}: {e}"), cx),
            });
        })
        .detach();
    }

    fn add_row(&mut self, clone: Option<usize>, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        let call = self.backend.call("mcts.addRow", json!({ "doc": doc, "path": self.path_json(), "table": table, "clone": clone, "messages": true }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.status(&r.json, cx);
                    let index = r.json["index"].as_u64().unwrap_or(0) as usize;
                    let what = match (r.json["key"].as_str(), r.json["id"].as_i64()) {
                        (Some(k), Some(id)) => format!("Row {index} added with {k} = {id}"),
                        _ => format!("Row {index} added"),
                    };
                    this.toast(Tone::Success, format!("{what} (unsaved)"), cx);
                    this.pending_select = Some(index);
                    this.reload_rows(cx);
                    // New rows go at the end.
                    this.scroll.scroll_to_item(usize::MAX / 2, ScrollStrategy::Bottom);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not add the row: {e}"), cx),
            });
        })
        .detach();
    }

    fn delete_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.len() > 1 {
            let Some(doc) = self.doc.clone() else { return };
            let paths: Vec<J> = self.marked.iter().map(|&i| self.row_path(i)).collect();
            let n = paths.len();
            let me = cx.entity().downgrade();
            dialogs::confirm(window, cx, format!("Delete {n} rows?"), "Rows after them move up. Rows elsewhere that name their ids keep naming them. Undo takes them back one by one.", "Delete", true, move |_, cx| {
                let (doc, paths) = (doc.clone(), paths.clone());
                let _ = me.update(cx, |this, cx| {
                    let call = this.backend.call("mcts.deleteMany", json!({ "doc": doc, "paths": paths }));
                    cx.spawn(async move |this, cx| {
                        let r = call.await;
                        let _ = this.update(cx, |this, cx| match r {
                            Ok(r) => {
                                this.status(&r.json, cx);
                                this.marked.clear();
                                this.selected = None;
                                this.row = None;
                                this.reload_rows(cx);
                            }
                            Err(e) => this.toast(Tone::Error, format!("Could not delete: {e}"), cx),
                        });
                    })
                    .detach();
                });
            });
            return;
        }
        let (Some(doc), Some(index)) = (self.doc.clone(), self.selected) else { return };
        let path = self.row_path(index);
        let me = cx.entity().downgrade();
        dialogs::confirm(window, cx, format!("Delete row {index}?"), "Rows after it move up. Rows elsewhere that name its id keep naming it. Undo takes it back.", "Delete", true, move |_, cx| {
            let (doc, path) = (doc.clone(), path.clone());
            let _ = me.update(cx, |this, cx| {
                let call = this.backend.call("mcts.delete", json!({ "doc": doc, "path": path }));
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| match r {
                        Ok(r) => {
                            this.status(&r.json, cx);
                            this.selected = None;
                            this.row = None;
                            this.reload_rows(cx);
                        }
                        Err(e) => this.toast(Tone::Error, format!("Could not delete: {e}"), cx),
                    });
                })
                .detach();
            });
        });
    }

    pub fn undo(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else { return };
        let call = self.backend.call("mcts.undo", json!({ "doc": doc }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.status(&r.json, cx);
                    let what = r.json["undone"].as_str().map(|u| format!("Undid {u}")).unwrap_or_else(|| "Nothing to undo in this file".into());
                    let _ = this.studio.update(cx, |s, cx| s.set_status(what, cx));
                    this.reload_rows(cx);
                    if let Some(i) = this.selected {
                        this.select(i, cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, e, cx),
            });
        })
        .detach();
    }

    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dirty: Vec<(String, String)> = self.docs.iter().filter(|d| d.dirty).map(|d| (d.id.clone(), d.path.clone())).collect();
        if dirty.is_empty() {
            self.toast(Tone::Info, "No assembly file has unsaved changes", cx);
            return;
        }
        let names = dirty.iter().map(|d| file_name(&d.1)).collect::<Vec<_>>().join(", ");
        let me = cx.entity().downgrade();
        dialogs::confirm(window, cx, format!("Save {names} into the game folder?"), "The files in the game folder are overwritten. The Studio keeps each original first, so a mod can still ship only the rows you changed.", "Save", false, move |_, cx| {
            let dirty = dirty.clone();
            let _ = me.update(cx, |this, cx| {
                for (doc, path) in dirty {
                    let call = this.backend.call("mcts.save", json!({ "doc": doc }));
                    cx.spawn(async move |this, cx| {
                        let r = call.await;
                        let _ = this.update(cx, |this, cx| match r {
                            Ok(r) => {
                                this.status(&r.json, cx);
                                this.toast(Tone::Success, format!("Saved {}", file_name(&path)), cx);
                            }
                            Err(e) => this.toast(Tone::Error, format!("Could not save {}: {e}", file_name(&path)), cx),
                        });
                    })
                    .detach();
                }
            });
        });
    }

    fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else { return };
        let me = cx.entity().downgrade();
        dialogs::confirm(window, cx, "Discard the changes to this file?", "Every unsaved edit of it is dropped and it is read again from the game folder.", "Discard", true, move |_, cx| {
            let doc = doc.clone();
            let _ = me.update(cx, |this, cx| {
                let call = this.backend.call("mcts.discard", json!({ "doc": doc }));
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| match r {
                        Ok(r) => {
                            this.status(&r.json, cx);
                            this.reload_rows(cx);
                            this.open(false, cx);
                        }
                        Err(e) => this.toast(Tone::Error, e, cx),
                    });
                })
                .detach();
            });
        });
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let pos = self.selected.and_then(|s| self.rows.iter().find(|(_, v)| v.0 == s).map(|(p, _)| *p));
        let next = match pos {
            Some(p) => (p as isize + by).clamp(0, self.total.max(1) as isize - 1) as usize,
            None => 0,
        };
        if let Some((index, _)) = self.rows.get(&next) {
            let index = *index;
            self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
            self.user_select(index, cx);
        }
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
                self.open_table(doc, table, cx);
            }
            "row" => self.user_select(rest.parse().map_err(|_| "row <index>")?, cx),
            "mark" => {
                let i: usize = rest.parse().map_err(|_| "mark <index>")?;
                if let Some(s) = self.selected {
                    self.marked.insert(s);
                }
                self.marked.insert(i);
            }
            "filter" => {
                let q = rest.to_string();
                self.filter.update(cx, |s, cx| s.set_value(q, window, cx));
                self.reload_rows(cx);
            }
            "set" => {
                let (field, text) = rest.split_once('=').ok_or("set <field>=<text>")?;
                let index = self.selected.ok_or("no row selected")?;
                if self.marked.len() > 1 {
                    let doc = self.doc.clone().ok_or("no file")?;
                    self.set_marked(doc, field.to_string(), text.to_string(), cx);
                    return Ok(());
                }
                self.edit = Some((Some(index), field.to_string(), None));
                let t = text.to_string();
                self.edit_input.update(cx, |s, cx| s.set_value(t, window, cx));
                // `set_value` is not read back at once: commit what was set.
                let Some(doc) = self.doc.clone() else { return Err("no file".into()) };
                self.edit = None;
                let call = self.backend.call("mcts.set", json!({ "doc": doc, "path": self.row_path(index), "field": field, "text": text }));
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| {
                        if let Ok(r) = r {
                            this.status(&r.json, cx);
                            this.reload_rows(cx);
                            this.select(index, cx);
                        }
                    });
                })
                .detach();
            }
            "open" => {
                let index = self.selected.ok_or("no row selected")?;
                self.open_set(index, rest.to_string(), cx);
            }
            "up" => self.up_to(0, cx),
            "add" => self.add_row(None, cx),
            "duplicate" => self.add_row(self.selected, cx),
            "new-content" => self.new_content(window, cx),
            "vendor" => self.champion_vendor(window, cx),
            "skins" => self.skins(window, cx),
            "export-mod" => self.export_mod(window, cx),
            "sort" => {
                self.sort = Some((rest.to_string(), false));
                self.reload_rows(cx);
            }
            _ => return Err(format!("unknown assembly step {verb}")),
        }
        Ok(())
    }

    // ----- rendering ------------------------------------------------------------------

    fn render_side(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let f = self.table_filter.read(cx).value().to_ascii_lowercase();
        let mut list = v_flex().gap_0p5();
        for d in &self.docs {
            let name = file_name(&d.path);
            list = list.child(
                h_flex()
                    .pt_2()
                    .gap_1()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1().child(name))
                    .when(d.dirty, |e| e.child(div().size(px(6.0)).rounded_full().bg(theme.warning))),
            );
            for (k, (field, value)) in d.header.iter().enumerate() {
                if field != "GAME_VERSION_RAW" && field != "DB_VERSION" {
                    continue;
                }
                let shown = if field == "GAME_VERSION_RAW" { version_text(value).unwrap_or_else(|| cell_text(value)) } else { cell_text(value) };
                let editing = self.edit.as_ref().is_some_and(|(r, fname, l)| r.is_none() && l.is_none() && fname == field && self.doc.as_deref() == Some(d.id.as_str()));
                let (doc, field2, draft) = (d.id.clone(), field.clone(), shown.clone());
                list = list.child(
                    h_flex()
                        .gap_2()
                        .pl_2()
                        .text_xs()
                        .child(div().w(px(96.0)).text_color(theme.muted_foreground).child(if field == "GAME_VERSION_RAW" { "game version" } else { "db version" }))
                        .child(if editing {
                            div().flex_1().child(Input::new(&self.edit_input).xsmall()).into_any_element()
                        } else {
                            div().id(SharedString::from(format!("hdr-{}-{k}", d.id))).cursor_text().hover(|s| s.bg(theme.list_hover)).child(shown).on_click(cx.listener(move |this, _, window, cx| {
                                this.doc = Some(doc.clone());
                                this.start_edit(None, field2.clone(), None, draft.clone(), window, cx)
                            })).into_any_element()
                        }),
                );
            }
            for (k, (t, rows, _)) in d.tables.iter().enumerate() {
                if !f.is_empty() && !t.to_ascii_lowercase().contains(&f) {
                    continue;
                }
                let active = self.doc.as_deref() == Some(d.id.as_str()) && self.table.as_deref() == Some(t.as_str()) && self.path.is_empty();
                let (doc, table) = (d.id.clone(), t.clone());
                let probe = format!("table-{}", t.to_ascii_lowercase());
                list = list.child(
                    h_flex()
                        .id(SharedString::from(format!("t-{}-{k}", d.id)))
                        .relative()
                        .h(px(22.0))
                        .px_2()
                        .gap_2()
                        .text_sm()
                        .rounded_sm()
                        .cursor_pointer()
                        .when(active, |e| e.bg(theme.list_active))
                        .hover(|s| s.bg(theme.list_hover))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_table(doc.clone(), table.clone(), cx)))
                        .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(t.trim_start_matches("DATA_SET_").to_string()))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(rows.to_string()))
                        .child(crate::driver::probe(probe)),
                );
            }
        }
        v_flex()
            .size_full()
            .child(div().p_1().child(Input::new(&self.table_filter).small()))
            .child(div().id("tables").flex_1().min_h_0().overflow_y_scroll().px_1().child(list))
            .into_any_element()
    }

    fn render_rows(&mut self, range: Range<usize>, _window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        // Pages the rows in view need.
        for pos in [range.start, range.end.saturating_sub(1)] {
            if pos < self.total && !self.rows.contains_key(&pos) {
                self.load_page(pos / PAGE, cx);
            }
        }
        let cols = self.visible_columns();
        let n_pin = self.pinned().len();
        let (pin_w, off) = (56.0 + COL_W * n_pin as f32, self.hoff());
        range
            .map(|pos| {
                let Some((index, cells)) = self.rows.get(&pos).cloned() else {
                    return h_flex().h(px(ROW_H)).px_2().text_xs().text_color(theme.muted_foreground).child("…").into_any_element();
                };
                let selected = self.selected == Some(index);
                let marked = self.marked.contains(&index) && !selected;
                // The pinned cells sit over the others, opaque, with the row's highlight.
                let mut pins = h_flex()
                    .size_full()
                    .when(selected, |d| d.bg(theme.list_active))
                    .when(marked, |d| d.bg(theme.primary.opacity(0.12)))
                    .when(!selected && !marked, |d| d.group_hover("arow", |s| s.bg(theme.list_hover)))
                    .child(div().w(px(56.0)).flex_none().px_2().text_xs().text_color(theme.muted_foreground).child(index.to_string()));
                let mut row = h_flex()
                    .id(("arow", pos))
                    .group("arow")
                    .relative()
                    .h(px(ROW_H))
                    .text_sm()
                    .border_b_1()
                    .border_color(theme.border.opacity(0.4))
                    .when(selected, |d| d.bg(theme.list_active))
                    .hover(|s| s.bg(theme.list_hover))
                    .when(self.marked.contains(&index) && !selected, |d| d.bg(theme.primary.opacity(0.12)))
                    .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                        this.focus.focus(window, cx);
                        let m = e.modifiers();
                        if m.secondary(cx) {
                            // ⌘-click: mark or unmark (the selection counts as marked).
                            if let Some(s) = this.selected {
                                this.marked.insert(s);
                            }
                            if !this.marked.remove(&index) {
                                this.marked.insert(index);
                            }
                        } else if m.shift {
                            // ⇧-click: every row from the selection to this one, by position.
                            let at = |i: usize, this: &Assembly| this.rows.iter().find(|(_, v)| v.0 == i).map(|(p, _)| *p);
                            if let (Some(a), Some(b)) = (this.selected.and_then(|s| at(s, this)), at(index, this)) {
                                for p in a.min(b)..=a.max(b) {
                                    if let Some((i, _)) = this.rows.get(&p) {
                                        this.marked.insert(*i);
                                    }
                                }
                            }
                        } else {
                            this.marked.clear();
                        }
                        this.user_select(index, cx)
                    }))
                    .child(div().w(px(pin_w)).flex_none());
                for (k, c) in cols.iter().enumerate() {
                    let v = cells.get(&c.name).cloned().unwrap_or(J::Null);
                    let editing = self.edit.as_ref().is_some_and(|(r, f, l)| *r == Some(index) && f == &c.name && l.is_none());
                    let content: AnyElement = if editing {
                        Input::new(&self.edit_input).xsmall().into_any_element()
                    } else {
                        let mut text = cell_text(&v);
                        if c.name.ends_with("_MSG_ID") {
                            if let Some(t) = v.as_i64().and_then(|id| self.msg_text.get(&id)) {
                                text = format!("{text} · {t}");
                            }
                        } else if c.name.ends_with("VALUE_ID") {
                            if let Some(t) = v.as_i64().and_then(|id| self.valid.get(&id)) {
                                text = format!("{text} · {t}");
                            }
                        }
                        let link = ref_of(&c.name).is_some() && v.is_number();
                        div().overflow_hidden().text_ellipsis().whitespace_nowrap().when(link, |d| d.text_color(theme.link)).child(text).into_any_element()
                    };
                    let (field, set, draft) = (c.name.clone(), v.get("rows").is_some(), cell_text(&v));
                    let cell = div()
                            .id(SharedString::from(format!("c{pos}-{}", c.name)))
                            .w(px(COL_W))
                            .flex_none()
                            .px_2()
                            .border_r_1()
                            .border_color(theme.border.opacity(0.4))
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                                if e.click_count() == 2 {
                                    cx.stop_propagation();
                                    if set {
                                        this.open_set(index, field.clone(), cx);
                                    } else {
                                        this.start_edit(Some(index), field.clone(), None, draft.clone(), window, cx);
                                    }
                                }
                            }))
                            .child(content);
                    if k < n_pin {
                        pins = pins.child(cell);
                    } else {
                        row = row.child(cell);
                    }
                }
                row.child(div().absolute().top_0().left(px(off)).h_full().bg(theme.background).border_r_1().border_color(theme.border).child(pins)).into_any_element()
            })
            .collect()
    }

    fn render_grid(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(table) = self.table.clone() else {
            let text = if self.loading { "Reading assembly.dat and the language files…".to_string() } else if let Some(e) = &self.error { e.clone() } else { "Pick a table on the left".into() };
            return v_flex().size_full().items_center().justify_center().text_sm().text_color(theme.muted_foreground).child(text).into_any_element();
        };
        let doc_dirty = self.docs.iter().any(|d| Some(&d.id) == self.doc.as_ref() && d.dirty);
        let can_undo = self.docs.iter().any(|d| Some(&d.id) == self.doc.as_ref() && d.undo > 0);
        // Breadcrumbs: the table, then each opened row set.
        let mut crumbs = h_flex().gap_1().text_sm();
        let depth = self.path.len();
        for (k, (t, row)) in self.path.clone().into_iter().enumerate() {
            crumbs = crumbs.child(div().id(("crumb", k)).text_color(theme.link).cursor_pointer().on_click(cx.listener(move |this, _, _, cx| this.up_to(k, cx))).child(format!("{} [{row}]", t.trim_start_matches("DATA_SET_")))).child(div().text_color(theme.muted_foreground).child("›"));
        }
        crumbs = crumbs.child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(table.trim_start_matches("DATA_SET_").to_string()));
        let toolbar = h_flex()
            .px_2()
            .py_1()
            .gap_1()
            .child(crumbs)
            .child(div().flex_1())
            .child(div().w(px(240.0)).child(Input::new(&self.filter).small()))
            .child(Button::new("a-new-content").xsmall().primary().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Plus)).label("New content").tooltip("A new skin, ability, card, item or champion, step by step").on_click(cx.listener(|this, _, window, cx| {
                let _ = this.studio.update(cx, |s, cx| s.open_wizard(None, Some(true), window, cx));
            })))
            .child(Button::new("arow-add").xsmall().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Plus)).tooltip("New row: an empty row shaped like the last one, with a fresh id").on_click(cx.listener(|this, _, _, cx| this.add_row(None, cx))))
            .child(Button::new("arow-dup").xsmall().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Copy)).disabled(self.selected.is_none()).tooltip("Duplicate the row: fresh ids, copied texts (⌘D)").on_click(cx.listener(|this, _, _, cx| this.add_row(this.selected, cx))))
            .child(Button::new("arow-del").xsmall().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Trash)).disabled(self.selected.is_none()).tooltip("Delete the row…").on_click(cx.listener(|this, _, window, cx| this.delete_row(window, cx))))
            .child({
                let me = cx.entity().downgrade();
                let all: Vec<String> = self.columns.iter().map(|c| c.name.clone()).collect();
                let hidden = self.table.as_ref().and_then(|t| self.hidden.get(t)).cloned().unwrap_or_default();
                let pinned = self.pinned();
                Button::new("a-columns").xsmall().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Eye)).tooltip("Columns to show (kept per table)").dropdown_menu(move |menu, _, _| {
                    let mut menu = menu.scrollable(true).max_h(px(420.0));
                    for c in &all {
                        let (me, c2) = (me.clone(), c.clone());
                        menu = menu.item(gpui_component::menu::PopupMenuItem::new(c.clone()).checked(!hidden.contains(c)).disabled(pinned.contains(c)).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.toggle_column(c2.clone(), cx));
                        }));
                    }
                    menu
                })
            })
            .child({
                let me = cx.entity().downgrade();
                let dirty = doc_dirty;
                Button::new("a-more").xsmall().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Ellipsis)).tooltip("Skins, champion vendor, export, undo, discard").dropdown_menu(move |menu, _, _| {
                    let item = |label: &str, f: fn(&mut Assembly, &mut Window, &mut Context<Assembly>)| {
                        let me = me.clone();
                        gpui_component::menu::PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                            let _ = me.update(cx, |this, cx| f(this, window, cx));
                        })
                    };
                    menu.item(item("Copy the rows    ⌘C", |a, _, cx| a.copy_rows(cx)))
                        .item(item("Paste rows    ⌘V", |a, _, cx| a.paste_rows(cx)))
                        .item(item("Copy as tab-separated text", |a, _, cx| a.copy_tsv(cx)))
                        .separator()
                        .item(item("Skins…", |a, w, cx| a.skins(w, cx)))
                        .item(item("Champion vendor…", |a, w, cx| a.champion_vendor(w, cx)))
                        .item(item("Export mod folder…", |a, w, cx| a.export_mod(w, cx)))
                        .separator()
                        .item(item("Undo", |a, w, cx| a.undo(w, cx)).disabled(!can_undo))
                        .item(item("Discard this file's changes…", |a, w, cx| a.discard(w, cx)).disabled(!dirty))
                })
            })
            .child(Button::new("a-save").xsmall().when(doc_dirty, |b| b.primary()).label("Save…").disabled(!doc_dirty).on_click(cx.listener(|this, _, window, cx| this.save(window, cx))));
        let shown_cols = self.visible_columns();
        let width = 56.0 + COL_W * shown_cols.len() as f32;
        let n_pin = self.pinned().len();
        let pin_w = 56.0 + COL_W * n_pin as f32;
        let mut header = h_flex().relative().h(px(26.0)).w(px(width)).text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).border_b_1().border_color(theme.border).child(div().w(px(pin_w)).flex_none());
        let mut pin_head = h_flex().absolute().top_0().left(px(self.hoff())).h_full().bg(theme.background).border_r_1().border_color(theme.border).child(div().w(px(56.0)).flex_none().px_2().child("#"));
        for (k, c) in shown_cols.iter().enumerate() {
            let name = c.name.clone();
            let arrow = match &self.sort {
                Some((s, d)) if s == &c.name => if *d { " ▼" } else { " ▲" },
                _ => "",
            };
            let cell = div()
                    .id(("ahead", k))
                    .w(px(COL_W))
                    .flex_none()
                    .px_2()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .tooltip({
                        let t = format!("{} ({}): click to sort", c.name, c.kind);
                        move |window, cx| gpui_component::tooltip::Tooltip::new(t.clone()).build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.sort = match &this.sort {
                            Some((s, false)) if *s == name => Some((name.clone(), true)),
                            Some((s, true)) if *s == name => None,
                            _ => Some((name.clone(), false)),
                        };
                        this.reload_rows(cx);
                    }))
                    .child(format!("{}{arrow}", c.name));
            if k < n_pin {
                pin_head = pin_head.child(cell);
            } else {
                header = header.child(cell);
            }
        }
        let header = header.child(pin_head);
        let mut list = uniform_list("arows", self.total, cx.processor(Self::render_rows)).track_scroll(&self.scroll).w(px(width)).flex_1();
        // Sideways gestures go to the grid around it, not down the rows.
        list.style().restrict_scroll_to_axis = Some(true);
        let marked = if self.marked.len() > 1 { format!(" · {} marked: edits set them all", self.marked.len()) } else { String::new() };
        let count = format!("{} rows{}{marked}", self.total, if depth > 0 { " in this row set" } else { "" });
        v_flex()
            .size_full()
            .child(toolbar)
            .child(div().id("hgrid").flex_1().min_h_0().overflow_x_scroll().restrict_scroll_to_axis().track_scroll(&self.hscroll).child(v_flex().h_full().w(px(width)).child(header).child(list)))
            .child(h_flex().px_2().gap_3().h(px(22.0)).text_xs().text_color(theme.muted_foreground).child(div().flex_none().child(count)).child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_right().child("double-click: edit · ⌘/⇧-click: mark rows · ⌘C ⌘V: copy, paste rows")))
            .into_any_element()
    }

    fn render_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(row) = &self.row else {
            return v_flex().size_full().items_center().justify_center().text_sm().text_color(theme.muted_foreground).child("Select a row").into_any_element();
        };
        let index = row.index;
        let mut body = v_flex().gap_1().p_2().child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(format!("Row {index}")));
        for (k, (name, kind, value)) in row.fields.clone().into_iter().enumerate() {
            let editing = self.edit.as_ref().is_some_and(|(r, f, l)| *r == Some(index) && f == &name && l.is_none());
            let is_set = value.get("rows").is_some();
            let draft = cell_text(&value);
            let text = if name.ends_with("VALUE_ID") { value.as_i64().and_then(|id| self.valid.get(&id)).map(|t| format!("{draft} · {t}")).unwrap_or(draft.clone()) } else { draft.clone() };
            let (n1, n2, n3, v3) = (name.clone(), name.clone(), name.clone(), value.clone());
            let value_el: AnyElement = if editing {
                div().flex_1().child(Input::new(&self.edit_input).xsmall()).into_any_element()
            } else {
                div()
                    .id(("rf", k))
                    .flex_1()
                    .min_w_0()
                    .px_1()
                    .rounded_sm()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .hover(|s| s.bg(theme.list_hover))
                    .when(is_set, |d| d.text_color(theme.link).cursor_pointer())
                    .when(!is_set, |d| d.cursor_text())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if is_set {
                            this.open_set(index, n1.clone(), cx);
                        } else {
                            this.start_edit(Some(index), n1.clone(), None, draft.clone(), window, cx);
                        }
                    }))
                    .child(text)
                    .into_any_element()
            };
            let link = ref_of(&name).is_some() && value.is_number();
            body = body.child(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .child(div().w(px(180.0)).flex_none().overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().text_color(theme.muted_foreground).child(n2))
                    .child(value_el)
                    .when(link, |d| d.child(Button::new(("go", k)).xsmall().ghost().label("Go").tooltip("Open the row this id names").on_click(cx.listener(move |this, _, _, cx| this.follow(&n3, &v3, cx)))))
                    .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(kind)),
            );
            // The message in every language.
            if let Some(list) = value.as_i64().and_then(|id| row.messages.get(&id)) {
                let id = value.as_i64().unwrap_or(0);
                for (j, (lang, text)) in list.iter().enumerate() {
                    let editing = self.edit.as_ref().is_some_and(|(_, f, l)| f == &id.to_string() && l.as_deref() == Some(lang.as_str()));
                    let (lang2, text2) = (lang.clone(), text.clone());
                    body = body.child(
                        h_flex()
                            .gap_2()
                            .pl(px(24.0))
                            .text_xs()
                            .child(div().w(px(40.0)).flex_none().text_color(theme.muted_foreground).child(lang.clone()))
                            .child(if editing {
                                div().flex_1().child(Input::new(&self.edit_input).xsmall()).into_any_element()
                            } else {
                                div().id(SharedString::from(format!("msg-{k}-{j}"))).flex_1().cursor_text().px_1().rounded_sm().hover(|s| s.bg(theme.list_hover)).child(text.clone()).on_click(cx.listener(move |this, _, window, cx| this.start_edit(Some(index), id.to_string(), Some(lang2.clone()), text2.clone(), window, cx))).into_any_element()
                            }),
                    );
                }
            }
        }
        // Who refers to it.
        if let Some(usages) = &row.usages {
            body = body.child(div().pt_3().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(format!("USED BY ({})", usages.len())));
            for (k, (path, field, label)) in usages.iter().take(200).enumerate() {
                let top = path.as_array().and_then(|p| p.first()).and_then(|t| Some((t[0].as_str()?.to_string(), t[1].as_u64()? as usize)));
                body = body.child(
                    h_flex()
                        .id(("use", k))
                        .gap_2()
                        .text_xs()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.list_hover))
                        .when_some(top, |d, (t, r)| {
                            d.on_click(cx.listener(move |this, _, _, cx| {
                                this.doc = Some("assembly".into());
                                this.visit("assembly".into(), Some(t.clone()), Some(r), cx);
                                this.record(cx);
                            }))
                        })
                        .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(theme.link).child(label.clone()))
                        .child(div().text_color(theme.muted_foreground).child(field.clone())),
                );
            }
        }
        div().id("arow-pane").size_full().overflow_y_scroll().child(body).into_any_element()
    }
}

/// `8.1.5839.0` → the packed `GAME_VERSION_RAW` (as a decimal string).
fn version_raw(text: &str) -> Option<String> {
    let parts: Vec<u64> = text.trim().split('.').map(|p| p.parse().ok().filter(|&n| n <= 0xffff)).collect::<Option<Vec<_>>>()?;
    if parts.len() != 4 {
        return None;
    }
    Some(parts.iter().fold(0u64, |v, p| (v << 16) | p).to_string())
}

impl Render for Assembly {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = &self.dir;
        let side = self.render_side(cx);
        let grid = self.render_grid(cx);
        let row = self.render_row(cx);
        div()
            .key_context("Assembly")
            .track_focus(&self.focus)
            .size_full()
            .on_action(cx.listener(|this, _: &super::Up, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &super::CopyRows, _, cx| this.copy_rows(cx)))
            .on_action(cx.listener(|this, _: &super::PasteRows, _, cx| this.paste_rows(cx)))
            .on_action(cx.listener(|this, _: &super::Down, _, cx| this.step(1, cx)))
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
    }
}

/// `<config>/tempest-studio-native/assembly-columns.json`: hidden columns by table.
fn load_hidden() -> HashMap<String, HashSet<String>> {
    super::state_file("assembly-columns.json").and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_hidden(h: &HashMap<String, HashSet<String>>) {
    let Some(f) = super::state_file("assembly-columns.json") else { return };
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    if let Ok(s) = serde_json::to_string(h) {
        let _ = std::fs::write(f, s);
    }
}

impl Assembly {
    /// Set `field` on every marked row (one undo step per row).
    fn set_marked(&mut self, doc: String, field: String, text: String, cx: &mut Context<Self>) {
            let paths: Vec<J> = self.marked.iter().map(|&i| self.row_path(i)).collect();
            let n = paths.len();
            let call = self.backend.call("mcts.setMany", json!({ "doc": doc, "paths": paths, "field": field, "text": text }));
            cx.spawn(async move |this, cx| {
                let r = call.await;
                let _ = this.update(cx, |this, cx| match r {
                    Ok(r) => {
                        this.status(&r.json, cx);
                        let _ = this.studio.update(cx, |s, cx| s.set_status(format!("Set {field} on {n} rows (unsaved)"), cx));
                        this.reload_rows(cx);
                        if let Some(s) = this.selected {
                            this.select(s, cx);
                        }
                    }
                    Err(e) => this.toast(Tone::Error, format!("{field}: {e}"), cx),
                });
            })
            .detach();
    }

    /// The rows copy and paste act on: the marked ones, else the selected one.
    fn chosen_rows(&self) -> Vec<usize> {
        if self.marked.len() > 1 {
            self.marked.iter().copied().collect()
        } else {
            self.selected.into_iter().collect()
        }
    }

    /// Copy the rows in full as JSON (nested sets included): ⌘V in the same table appends them.
    pub(super) fn copy_rows(&mut self, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        let rows = self.chosen_rows();
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
                let _ = this.studio.update(cx, |s, cx| s.set_status(format!("Copied {n} row(s): ⌘V in this table appends them with fresh ids"), cx));
            });
        })
        .detach();
    }

    /// The visible cells of the rows as tab-separated text (for spreadsheets).
    fn copy_tsv(&mut self, cx: &mut Context<Self>) {
        let rows = self.chosen_rows();
        let cols = self.visible_columns();
        let mut lines = vec![cols.iter().map(|c| c.name.clone()).collect::<Vec<_>>().join("\t")];
        for (_, (i, cells)) in self.rows.iter().filter(|(_, v)| rows.contains(&v.0)).collect::<std::collections::BTreeMap<_, _>>() {
            let _ = i;
            lines.push(cols.iter().map(|c| cell_text(cells.get(&c.name).unwrap_or(&J::Null))).collect::<Vec<_>>().join("\t"));
        }
        let n = lines.len() - 1;
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(lines.join("\n")));
        let _ = self.studio.update(cx, |s, cx| s.set_status(format!("Copied {n} row(s) as tab-separated text"), cx));
    }

    /// Paste rows copied with Copy: each is appended with fresh ids (nested sets renumbered, messages copied).
    pub(super) fn paste_rows(&mut self, cx: &mut Context<Self>) {
        let (Some(doc), Some(table)) = (self.doc.clone(), self.table.clone()) else { return };
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else { return };
        let Ok(data) = serde_json::from_str::<J>(&text) else {
            self.toast(Tone::Info, "The clipboard holds no rows (copy rows with ⌘C first)", cx);
            return;
        };
        let (Some(from), Some(rows)) = (data["tempestRows"].as_str(), data["rows"].as_array()) else {
            self.toast(Tone::Info, "The clipboard holds no rows (copy rows with ⌘C first)", cx);
            return;
        };
        if from != table {
            self.toast(Tone::Warning, format!("The clipboard holds {} rows: open that table to paste them", from.trim_start_matches("DATA_SET_")), cx);
            return;
        }
        let path = self.path_json();
        let calls: Vec<_> = rows.iter().map(|row| self.backend.call("mcts.addRow", json!({ "doc": doc, "path": path, "table": table, "row": row }))).collect();
        cx.spawn(async move |this, cx| {
            let (mut last, mut n) = (None, 0);
            for c in calls {
                match c.await {
                    Ok(r) => {
                        last = r.json["index"].as_u64().map(|i| i as usize);
                        n += 1;
                        let _ = this.update(cx, |this, cx| this.status(&r.json, cx));
                    }
                    Err(e) => {
                        let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not paste a row: {e}"), cx));
                        break;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                if n > 0 {
                    this.toast(Tone::Success, format!("{n} row(s) pasted at the end with fresh ids (unsaved)"), cx);
                    this.pending_select = last;
                    this.reload_rows(cx);
                    this.scroll.scroll_to_item(usize::MAX / 2, ScrollStrategy::Bottom);
                }
            });
        })
        .detach();
    }

    /// The id and name columns, pinned at the left of the grid (`DATA_SET_BOTS` → `BOT_ID`).
    fn pinned(&self) -> Vec<String> {
        let Some(t) = &self.table else { return Vec::new() };
        let has = |n: &str| self.columns.iter().any(|c| c.name == n);
        let stem = t.trim_start_matches("DATA_SET_");
        let key = [Some(format!("{stem}_ID")), stem.strip_suffix("IES").map(|s| format!("{s}Y_ID")), stem.strip_suffix("ES").map(|s| format!("{s}_ID")), stem.strip_suffix('S').map(|s| format!("{s}_ID"))].into_iter().flatten().find(|c| has(c));
        let name = ["REFERENCE_NAME", "NAME", "MESSAGE"].into_iter().find(|n| has(n)).map(str::to_string);
        key.into_iter().chain(name).collect()
    }

    /// The columns shown: the pinned ones first, then the table's less the hidden ones.
    fn visible_columns(&self) -> Vec<Column> {
        let hidden = self.table.as_ref().and_then(|t| self.hidden.get(t));
        let pinned = self.pinned();
        let first = pinned.iter().filter_map(|n| self.columns.iter().find(|c| &c.name == n));
        first.chain(self.columns.iter().filter(|c| !pinned.contains(&c.name) && hidden.is_none_or(|h| !h.contains(&c.name)))).cloned().collect()
    }

    /// How far the grid is scrolled sideways (the pinned columns move back by as much).
    fn hoff(&self) -> f32 {
        (-f32::from(self.hscroll.offset().x)).max(0.0)
    }

    fn toggle_column(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(t) = self.table.clone() else { return };
        let h = self.hidden.entry(t).or_default();
        if !h.remove(&name) {
            h.insert(name);
        }
        save_hidden(&self.hidden);
        cx.notify();
    }
}
