//! The unified omnibox and object picker (`⌘K`, `⌘⇧P`, `⌘P`, `⌘⇧O`, `⌘⇧F`):
//! one search bar across champions, game data tables, UI movies, game files,
//! commands, and objects inside every package. Power-user prefixes (`>`
//! commands, `@` objects of this package or rows of this table, `#` objects
//! across every package, `+` properties to add) and clickable scope pills
//! share the same state. Also serves every "choose an object" picker (`Pick`).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::{div, prelude::*, px, Context, Entity, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::command::{Command, CommandItem, CommandState};
use gpui_component::{h_flex, ActiveTheme, Icon, Selectable, Sizable};
use gpui_kit_assets::IconName;
use serde_json::json;

use super::commands::key_hint;
use super::{Cmd, Place, Studio, View};

/// Rows shown at most (the widget measures the supplied items).
const SHOWN: usize = 200;

/// Clickable scope tabs in the omnibox header (`Tab` / `Shift+Tab` cycles).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    All,
    Files,
    Ui,
    Objects,
    Commands,
}

impl Scope {
    pub fn next(self) -> Scope {
        match self {
            Scope::All => Scope::Files,
            Scope::Files => Scope::Ui,
            Scope::Ui => Scope::Objects,
            Scope::Objects => Scope::Commands,
            Scope::Commands => Scope::All,
        }
    }

    pub fn prev(self) -> Scope {
        match self {
            Scope::All => Scope::Commands,
            Scope::Files => Scope::All,
            Scope::Ui => Scope::Files,
            Scope::Objects => Scope::Ui,
            Scope::Commands => Scope::Objects,
        }
    }
}

fn strip_tag_prefix<'a>(q: &'a str, tag: &str) -> Option<&'a str> {
    let trimmed = q.trim_start();
    let head = trimmed.get(..tag.len())?;
    if !head.eq_ignore_ascii_case(tag) {
        return None;
    }
    let rest = &trimmed[tag.len()..];
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then_some(rest)
}

fn scope_for_query(q: &str, fallback: Scope) -> Scope {
    if q.starts_with('>') {
        Scope::Commands
    } else if q.starts_with('#') {
        Scope::Objects
    } else if strip_tag_prefix(q, "@ui").is_some() {
        Scope::Ui
    } else if strip_tag_prefix(q, "@files").is_some() {
        Scope::Files
    } else {
        fallback
    }
}

fn scope_query(cur: &str, scope: Scope) -> String {
    let trimmed = cur.trim_start();
    let words = if let Some(rest) = strip_tag_prefix(trimmed, "@ui").or_else(|| strip_tag_prefix(trimmed, "@files")) {
        rest.trim_start()
    } else {
        trimmed.trim_start_matches(['>', '#', '@', '+']).trim_start()
    };
    match scope {
        Scope::All | Scope::Files | Scope::Ui => words.to_string(),
        Scope::Objects => format!("#{words}"),
        Scope::Commands => format!(">{words}"),
    }
}

/// What the palette is picking for, when it is a picker.
#[derive(Clone, Debug)]
pub enum Pick {
    /// The package to copy the selected export into.
    CopyTo,
    /// An object of the selected export's class, in another package.
    ReplaceWith,
    /// The material of a mesh slot.
    SlotMaterial(usize),
    /// The texture of a material's parameter.
    Texture(String),
    /// The value of a `*_VALUE_ID` cell of the assembly: a row index, the column, the valid values.
    AssemblyValue { row: usize, field: String, values: std::rc::Rc<Vec<(i64, String)>> },
}

impl Pick {
    fn placeholder(&self) -> &'static str {
        match self {
            Pick::CopyTo => "The package to copy it into",
            Pick::ReplaceWith => "An object of the same class in another package",
            Pick::SlotMaterial(_) => "A material or material instance, e.g. mic_ or a champion's name",
            Pick::Texture(_) => "A texture of any package",
            Pick::AssemblyValue { .. } => "A valid value, by name or id",
        }
    }

    /// The class the search keeps (`isA`), for object picks.
    fn is_a(&self, studio: &Studio) -> Option<String> {
        match self {
            Pick::CopyTo | Pick::AssemblyValue { .. } => None,
            Pick::ReplaceWith => studio.selection_class(),
            Pick::SlotMaterial(_) => Some("MaterialInterface".into()),
            Pick::Texture(_) => Some("Texture".into()),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Item {
    Command(Cmd),
    /// A champion from the champion vendor.
    Champion { id: u64, name: String, reference: String },
    /// Index into `Studio::packages`, with an optional plain-English description.
    Package { index: usize, title: Option<String>, ui_movie: bool },
    /// Export of the open package.
    Export(usize),
    /// A search result in any package.
    Hit { package: String, path: String, index: usize, object: String, class: String },
    /// A property the selected object's class declares and the object does not store.
    AddProperty { name: String, kind: String, ty: String },
    /// A table of an assembly file.
    Table { doc: String, file: String, table: String, rows: usize },
    /// Filter the rows of the assembly table on screen (empty: clear the filter).
    FilterRows { table: String, query: String },
    /// A valid value of the assembly (`DATA_SET_VALID_VALUES`).
    Value { id: i64, text: String },
}

pub struct Palette {
    pub state: Entity<CommandState>,
    /// Latest query text (kept synchronously so `Tab` cycling never reads stale pre-render `InputState`).
    query: String,
    pub items: Vec<Item>,
    /// Instant local matches retained while an async `workspace.search` runs in `Scope::All`.
    local_items: Vec<Item>,
    /// Counts queries so a late answer to an older query is dropped.
    seq: u64,
    pub note: Option<SharedString>,
    /// `+`: what can be added to the selected object (`object.missing`, read once).
    missing: Option<Vec<(String, String, String)>>,
    /// `+` lists the fields of the tagged struct at this property path instead of the object's properties.
    at: Option<String>,
    pub pick: Option<Pick>,
    pub scope: Scope,
    /// Loaded champion count when `palette_query` last ran, so async warmup refreshes the open omnibox.
    champ_count: usize,
}

/// Curated plain-English labels for interface & HUD movie packages (`(title, is_core_ui)`).
fn friendly_ui_package(name: &str) -> Option<(&'static str, bool)> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "lobbyui_sf" | "lobbyui" => Some(("Main menu, play screen & lobby", true)),
        "gameui_sf" | "gameui" | "tggameui" | "tggameui_sf" => Some(("In-match HUD, health bars & scoreboard", true)),
        "globalui_sf" | "globalui" => Some(("Shared menus, popups & buttons", true)),
        "spectatorui_sf" | "spectatorui" => Some(("Spectator & replay HUD", true)),
        "loadingscreen_sf" | "loading_sf" | "loadingui_sf" => Some(("Match loading screen UI", true)),
        "customloadingscreenassets_sf" => Some(("Loading screen backgrounds & frames", true)),
        "hud_assets_sf" | "hud_sf" | "hud" => Some(("HUD icons & interface art", true)),
        "generalui_assets_sf" | "generalui_sf" => Some(("Shared interface buttons, frames & icons", true)),
        "gfxfonts" | "ui_fonts_sf" | "fonts_sf" => Some(("Interface fonts", true)),
        _ if lower.ends_with("_ui_assets_sf") => Some(("Champion portraits & ability icons", false)),
        _ if lower.ends_with("_type_ui_sf") => Some(("Cosmetic & store category icons", false)),
        _ if lower.starts_with("ui_") => Some(("Interface movie & HUD element", true)),
        _ if lower.contains("hud") && lower.ends_with("_sf") => Some(("HUD & match interface", true)),
        _ => None,
    }
}

fn ui_priority(name: &str) -> u8 {
    match name.to_ascii_lowercase().as_str() {
        "lobbyui_sf" | "lobbyui" => 0,
        "gameui_sf" | "gameui" | "tggameui" | "tggameui_sf" => 1,
        "globalui_sf" | "globalui" => 2,
        "spectatorui_sf" | "spectatorui" => 3,
        "loadingscreen_sf" | "loading_sf" | "loadingui_sf" => 4,
        "customloadingscreenassets_sf" => 5,
        "hud_assets_sf" | "hud_sf" | "hud" => 6,
        "generalui_assets_sf" | "generalui_sf" => 7,
        "gfxfonts" | "ui_fonts_sf" | "fonts_sf" => 8,
        _ => 9,
    }
}

/// Relevance key for `workspace.search` object hits:
/// 0 = exact object name match,
/// 1 = package prefix matches query (`CASSIE_*` when searching `"Cassie"`) or object name starts with query,
/// 2 = everything else.
fn hit_rank(package: &str, object: &str, query_low: &str, query_pre: &str) -> (u8, bool, bool) {
    let leaf = object.rsplit('.').next().unwrap_or(object);
    let leaf_low = leaf.to_ascii_lowercase();
    if leaf_low == query_low || object.eq_ignore_ascii_case(query_low) {
        return (0, false, false);
    }
    let pkg_head = package.split('_').next().unwrap_or(package);
    let pkg_matches = package.to_ascii_lowercase().starts_with(query_low)
        || (!query_pre.is_empty() && (pkg_head.eq_ignore_ascii_case(query_pre) || super::champions::prefix(pkg_head).starts_with(query_pre)));
    let leaf_starts = leaf_low.starts_with(query_low)
        || (!query_pre.is_empty() && super::champions::prefix(leaf).starts_with(query_pre));
    if pkg_matches || leaf_starts {
        (1, !pkg_matches, !leaf_starts)
    } else {
        (2, true, true)
    }
}

fn class_priority(class: &str) -> u8 {
    let c = class.to_ascii_lowercase();
    match c.as_str() {
        "skeletalmesh" | "staticmesh" | "texture2d" | "swfmovie" | "gfxmovieinfo" => 0,
        "materialinstanceconstant" | "materialinstancetimevarying" | "material" | "particlesystem" => 1,
        "soundnodewave" | "soundcue" | "animset" | "physicsasset" => 2,
        "akevent" | "akbank" | "animsequence" => 3,
        _ if c.starts_with("animnotify") || c.starts_with("animnode") || c.starts_with("distribution") => 5,
        _ => 4,
    }
}

fn parse_sorted_hits(results: &serde_json::Value, query: &str) -> Vec<Item> {
    let q_low = query.trim().to_ascii_lowercase();
    let q_pre = super::champions::prefix(&q_low);
    let mut hits: Vec<Item> = results
        .as_array()
        .into_iter()
        .flatten()
        .map(|h| Item::Hit {
            package: h["package"].as_str().unwrap_or_default().into(),
            path: h["path"].as_str().unwrap_or_default().into(),
            index: h["index"].as_u64().unwrap_or(0) as usize,
            object: h["object"].as_str().unwrap_or_default().into(),
            class: h["class"].as_str().unwrap_or_default().into(),
        })
        .collect();
    hits.sort_by_key(|it| match it {
        Item::Hit { package, object, class, .. } => {
            let (tier, p_miss, l_miss) = hit_rank(package, object, &q_low, &q_pre);
            (tier, p_miss, class_priority(class), l_miss)
        }
        _ => (2, true, 3, true),
    });
    hits
}

/// Known `assembly.dat` tables so the omnibox finds Game data tables immediately before `View::Assembly` is opened.
fn fallback_tables() -> Vec<(String, String, String, usize)> {
    const NAMES: &[&str] = &[
        "DATA_SET_ITEMS",
        "DATA_SET_DEVICES",
        "DATA_SET_SKINS",
        "DATA_SET_BOTS",
        "DATA_SET_LOOT_TABLES",
        "DATA_SET_RESOURCES",
        "DATA_SET_ICONS",
        "DATA_SET_PROJECTILES",
        "DATA_SET_DEPLOYABLES",
        "DATA_SET_SPECIAL_FX",
        "DATA_SET_EFFECT_GROUPS",
        "DATA_SET_VALID_VALUES",
        "DATA_SET_TIPS",
        "DATA_SET_PROPERTIES",
        "DATA_SET_MAT_RES_GROUPS",
        "DATA_SET_LOOT_TABLE_GROUPS",
        "DATA_SET_XP_TABLES",
        "DATA_SET_LANGUAGES",
        "DATA_SET_VGS",
        "DATA_SET_VOX_CHATTER",
        "DATA_SET_ASSEMBLY_MESHES",
        "DATA_SET_ITEM_CHILDREN",
        "DATA_SET_BOT_SPAWN_TABLES",
        "DATA_SET_BOT_BEHAVIORS",
    ];
    NAMES.iter().map(|&t| ("assembly".into(), "assembly.dat".into(), t.into(), 0)).collect()
}

impl Studio {
    pub(super) fn open_palette(&mut self, prefix: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette_with(prefix, None, None, window, cx);
    }

    pub(super) fn open_palette_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette_with("", None, Some(Scope::Files), window, cx);
    }

    pub(super) fn open_palette_ui(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette_with("", None, Some(Scope::Ui), window, cx);
    }

    /// The palette's `+` list for the tagged struct at property path `at`.
    pub(super) fn open_palette_at(&mut self, prefix: &str, at: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette_with(prefix, None, None, window, cx);
        if let Some(p) = &mut self.palette {
            p.at = Some(at);
            p.missing = None;
        }
        self.palette_query(prefix.to_string(), cx);
    }

    pub(super) fn open_picker(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette_with("", Some(pick), None, window, cx);
    }

    fn open_palette_with(&mut self, prefix: &str, pick: Option<Pick>, scope: Option<Scope>, window: &mut Window, cx: &mut Context<Self>) {
        if pick.is_none() && self.dir.is_some() && self.champions.is_none() {
            self.champions_view(window, cx);
        }
        let scope = scope.unwrap_or_else(|| scope_for_query(prefix, Scope::All));
        let champ_count = self.champions.as_ref().map(|c| c.all().len()).unwrap_or(0);
        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |s, cx| {
            s.set_query(prefix.to_string(), window, cx);
            s.focus(window, cx);
        });
        self.palette = Some(Palette {
            state,
            query: prefix.to_string(),
            items: Vec::new(),
            local_items: Vec::new(),
            seq: 0,
            note: None,
            missing: None,
            at: None,
            pick,
            scope,
            champ_count,
        });
        self.palette_query(prefix.to_string(), cx);
    }

    /// Refresh the open omnibox once warmed-up champions arrive.
    pub(super) fn sync_palette(&mut self, cx: &mut Context<Self>) {
        let cur = self.champions.as_ref().map(|c| c.all().len()).unwrap_or(0);
        let refresh = self.palette.as_ref().filter(|p| p.pick.is_none() && p.champ_count != cur).map(|p| p.query.clone());
        if let Some(q) = refresh {
            self.palette_query(q, cx);
        }
    }

    pub(super) fn set_palette_scope(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.palette else { return };
        let new_q = scope_query(&p.query, scope);
        p.scope = scope;
        p.query = new_q.clone();
        let state = p.state.clone();
        state.update(cx, |s, cx| {
            s.set_query(new_q.clone(), window, cx);
            s.focus(window, cx);
        });
        self.palette_query(new_q, cx);
    }

    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    /// Exclusive `workspace.search` (`#` mode and object `Pick`s).
    fn search(&mut self, query: String, is_a: Option<String>, seq: u64, cx: &mut Context<Self>) {
        let Some(p) = &mut self.palette else { return };
        p.items.clear();
        p.local_items.clear();
        if query.len() < 2 {
            p.note = Some("Type two characters or more to search every package".into());
            return;
        }
        let q_for_sort = query.clone();
        let call = self.backend.call("workspace.search", json!({ "query": query, "limit": 1500, "isA": is_a }));
        p.note = Some("Searching…".into());
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                let Some(p) = &mut this.palette else { return };
                if p.seq != seq {
                    return;
                }
                p.note = None;
                match r {
                    Ok(r) => {
                        let mut hits = parse_sorted_hits(&r.json["results"], &q_for_sort);
                        let truncated = r.json["truncated"].as_bool().unwrap_or(false) || hits.len() > SHOWN;
                        hits.truncate(SHOWN);
                        p.items = hits;
                        if truncated && p.items.len() >= SHOWN {
                            p.note = Some(format!("First {SHOWN} matches: type more to narrow it down").into());
                        } else if p.items.is_empty() {
                            p.note = Some("Nothing matches in any package: try a shorter part of the name".into());
                        }
                    }
                    Err(e) => p.note = Some(super::ui::explain_error(&e).into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Debounced `workspace.search` for unified `Scope::All`: appends relevance-sorted `Item::Hit` without wiping instant local matches.
    fn search_append(&mut self, query: String, seq: u64, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        if let Some(p) = self.palette.as_mut().filter(|p| p.items.is_empty()) {
            p.note = Some("Searching inside every package…".into());
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(120)).await;
            let still_current = this.update(cx, |this, _| this.palette.as_ref().is_some_and(|p| p.seq == seq)).unwrap_or(false);
            if !still_current {
                return;
            }
            let q_for_sort = query.clone();
            let r = backend.call("workspace.search", json!({ "query": query, "limit": 1500 })).await;
            let _ = this.update(cx, |this, cx| {
                let Some(p) = &mut this.palette else { return };
                if p.seq != seq {
                    return;
                }
                p.note = None;
                if let Ok(r) = r {
                    let mut merged = p.local_items.clone();
                    let room = SHOWN.saturating_sub(merged.len());
                    let sorted_hits = parse_sorted_hits(&r.json["results"], &q_for_sort);
                    let more_hits = sorted_hits.len() > room;
                    merged.extend(sorted_hits.into_iter().take(room));
                    p.items = merged;
                    if p.items.is_empty() {
                        p.note = Some("Nothing matched in champions, game data, files, objects or commands".into());
                    } else if (r.json["truncated"].as_bool().unwrap_or(false) || more_hits) && p.items.len() >= SHOWN {
                        p.note = Some(format!("First {SHOWN} matches: type more to narrow it down").into());
                    }
                } else if p.items.is_empty() {
                    p.note = Some("Nothing matched".into());
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn palette_query(&mut self, q: String, cx: &mut Context<Self>) {
        let has_dir = self.dir.is_some();
        let enabled: Vec<Cmd> = Cmd::ALL.iter().copied().filter(|c| c.enabled(self)).collect();
        let (recent, pinned) = (self.recent_packages(), self.pinned());
        let champs: Vec<super::champions::Champion> = self.champions.as_ref().map(|c| c.all().to_vec()).unwrap_or_default();
        let champ_by_prefix: HashMap<String, String> = champs.iter().map(|c| (super::champions::prefix(&c.reference), c.name.clone())).collect();
        let live_tables = self.assembly.as_ref().map(|a| a.read(cx).tables()).unwrap_or_default();
        let tables = if !live_tables.is_empty() { live_tables } else if has_dir { fallback_tables() } else { Vec::new() };
        let asm_table = if self.view == View::Assembly { self.assembly.as_ref().and_then(|a| a.read(cx).table_name()) } else { None };
        let in_assembly = self.view == View::Assembly;
        let packages = &self.packages;

        let pkg_meta = |idx: usize| -> (Option<String>, bool) {
            let name = &packages[idx].name;
            if let Some((ui_label, core)) = friendly_ui_package(name) {
                let champ_ui = (!core).then(|| name.split_once('_')).flatten().and_then(|(pre, _)| {
                    champ_by_prefix
                        .get(&pre.to_ascii_uppercase())
                        .cloned()
                        .or_else(|| super::champions::fallback_package_label(name).and_then(|l| l.split_once(" · ").map(|(c, _)| c.to_string())))
                });
                let label = match champ_ui {
                    Some(ch) => format!("{ch} · {ui_label}"),
                    None if name.to_ascii_lowercase().starts_with("device") => "Item & ability icons".to_string(),
                    None => ui_label.to_string(),
                };
                return (Some(label), true);
            }
            if let Some((pre, _)) = name.split_once('_') {
                let up = pre.to_ascii_uppercase();
                if let (Some(ch), Some(role)) = (champ_by_prefix.get(&up), super::champions::package_role(name, &up)) {
                    return (Some(format!("{ch} · {role}")), false);
                }
            }
            (super::champions::fallback_package_label(name), false)
        };

        let Some(p) = &mut self.palette else { return };
        p.query = q.clone();
        p.seq += 1;
        let seq = p.seq;
        p.champ_count = champs.len();
        p.note = None;
        let has = |text: &str, f: &str| f.is_empty() || text.to_ascii_lowercase().contains(f);

        if let Some(pick) = p.pick.clone() {
            if let Pick::AssemblyValue { values, .. } = &pick {
                let f = q.trim().to_ascii_lowercase();
                p.items = values.iter().filter(|(id, text)| has(text, &f) || id.to_string().starts_with(&f)).take(SHOWN).map(|(id, text)| Item::Value { id: *id, text: text.clone() }).collect();
                if p.items.is_empty() {
                    p.note = Some("No valid value by that name or id".into());
                }
            } else if matches!(pick, Pick::CopyTo) {
                let f = q.trim().to_ascii_lowercase();
                let here = self.open.as_ref().map(|t| t.path.clone());
                p.items = (0..packages.len())
                    .filter(|&i| Some(&packages[i].path) != here.as_ref() && !packages[i].script && has(&packages[i].name, &f))
                    .take(SHOWN)
                    .map(|i| Item::Package { index: i, title: None, ui_movie: false })
                    .collect();
            } else {
                let is_a = pick.is_a(self);
                self.search(q.trim().to_string(), is_a, seq, cx);
            }
            cx.notify();
            return;
        }

        if let Some(rest) = q.strip_prefix('+') {
            let f = rest.trim().to_ascii_lowercase();
            match &p.missing {
                Some(list) => {
                    p.items = list.iter().filter(|(n, _, _)| has(n, &f)).take(SHOWN).map(|(name, kind, ty)| Item::AddProperty { name: name.clone(), kind: kind.clone(), ty: ty.clone() }).collect();
                    if list.is_empty() {
                        p.note = Some("Every property of the class is already stored".into());
                    }
                }
                None => {
                    let (Some(t), Some(i)) = (&self.open, self.selected_export) else { return };
                    let mut params = json!({ "path": t.path, "index": i });
                    if let Some(at) = &p.at {
                        params["at"] = json!(at);
                    }
                    let call = self.backend.call("object.missing", params);
                    cx.spawn(async move |this, cx| {
                        let r = call.await;
                        let _ = this.update(cx, |this, cx| {
                            let Some(p) = &mut this.palette else { return };
                            p.missing = Some(r.map(|r| r.json.as_array().into_iter().flatten().map(|m| (m["name"].as_str().unwrap_or_default().to_string(), m["kind"].as_str().unwrap_or_default().to_string(), m["type"].as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default());
                            let q = p.query.clone();
                            this.palette_query(if q.starts_with('+') { q } else { "+".into() }, cx);
                        });
                    })
                    .detach();
                }
            }
            cx.notify();
            return;
        }

        if let Some(rest) = q.strip_prefix('>') {
            p.scope = Scope::Commands;
            let f = rest.trim().to_ascii_lowercase();
            p.items = enabled.into_iter().filter(|c| has(c.info().title, &f) || has(c.info().group, &f) || has(c.info().detail, &f)).map(Item::Command).collect();
            cx.notify();
            return;
        }

        let rank = |path: &str| recent.iter().position(|r| r == path).or_else(|| pinned.iter().position(|r| r == path).map(|k| 100 + k)).unwrap_or(usize::MAX);

        let files_rest = strip_tag_prefix(&q, "@files");
        let ui_filter_opt = if let Some(rest) = strip_tag_prefix(&q, "@ui") {
            p.scope = Scope::Ui;
            Some(rest.trim().to_ascii_lowercase())
        } else if p.scope == Scope::Ui && files_rest.is_none() && !q.starts_with(['+', '>', '@', '#']) {
            Some(q.trim().to_ascii_lowercase())
        } else {
            None
        };
        if let Some(uf) = ui_filter_opt {
            let u_squeezed = super::champions::prefix(&uf);
            let mut ui_hits: Vec<(usize, Option<String>, bool, u8)> = (0..packages.len())
                .filter_map(|i| {
                    let name = &packages[i].name;
                    let (ui_label, core) = friendly_ui_package(name)?;
                    let (title, _) = pkg_meta(i);
                    if uf.is_empty() {
                        return core.then_some((i, title, false, ui_priority(name)));
                    }
                    let matches = has(name, &uf)
                        || has(ui_label, &uf)
                        || title.as_deref().is_some_and(|t| has(t, &uf))
                        || (!u_squeezed.is_empty() && super::champions::prefix(name).contains(&u_squeezed));
                    matches.then_some((i, title, !core, ui_priority(name)))
                })
                .collect();
            ui_hits.sort_by_key(|(i, _, non_core, prio)| (*non_core, *prio, rank(&packages[*i].path), packages[*i].name.to_ascii_lowercase()));
            p.items = ui_hits
                .into_iter()
                .take(SHOWN)
                .map(|(index, title, _, _)| Item::Package { index, title, ui_movie: true })
                .collect();
            p.local_items = p.items.clone();
            if p.items.is_empty() {
                p.note = Some(if has_dir {
                    "No matching menu or HUD package".into()
                } else {
                    "Open a game folder first to browse menus and HUD movies".into()
                });
            }
            cx.notify();
            return;
        }

        if files_rest.is_none() {
            if let (Some(rest), Some(table)) = (q.strip_prefix('@'), asm_table) {
                p.items = vec![Item::FilterRows { table, query: rest.trim().to_string() }];
                cx.notify();
                return;
            }

            if let Some(rest) = q.strip_prefix('@') {
                let f = rest.trim().to_ascii_lowercase();
                p.items = match &self.open {
                    Some(t) => (0..t.exports.len()).filter(|&i| has(&t.exports[i].name, &f) || has(&t.exports[i].class, &f)).take(SHOWN).map(Item::Export).collect(),
                    None => {
                        p.note = Some("No package is open".into());
                        Vec::new()
                    }
                };
                cx.notify();
                return;
            }

            if let Some(rest) = q.strip_prefix('#') {
                p.scope = Scope::Objects;
                self.search(rest.trim().to_string(), None, seq, cx);
                cx.notify();
                return;
            }
        }

        let f = if let Some(rest) = files_rest {
            p.scope = Scope::Files;
            rest.trim().to_ascii_lowercase()
        } else {
            if matches!(p.scope, Scope::Commands | Scope::Objects) {
                p.scope = Scope::All;
            }
            q.trim().to_ascii_lowercase()
        };
        let scope = p.scope;
        let squeezed = super::champions::prefix(&f);
        let ui_intent = !f.is_empty() && ["ui", "hud", "menu", "menus", "lobby", "interface", "scoreboard"].iter().any(|w| f.contains(w));
        fn short(t: &str) -> &str {
            t.strip_prefix("DATA_SET_").unwrap_or(t)
        }

        let mut items: Vec<Item> = Vec::new();
        let mut added_pkgs: HashSet<usize> = HashSet::new();

        if !has_dir {
            items.extend(enabled.into_iter().filter(|c| has(c.info().title, &f) || has(c.info().group, &f) || has(c.info().detail, &f)).map(Item::Command));
            p.local_items = items.clone();
            p.items = items;
            cx.notify();
            return;
        }

        if f.is_empty() {
            // Empty query: recent & pinned packages, curated UI & core tables, then commands / packages.
            let mut top_pkgs: Vec<usize> = (0..packages.len()).filter(|&i| rank(&packages[i].path) < usize::MAX).collect();
            top_pkgs.sort_by_key(|&i| rank(&packages[i].path));

            let matched_tables: Vec<Item> = tables.into_iter().map(|(doc, file, table, rows)| Item::Table { doc, file, table, rows }).collect();
            if in_assembly {
                items.extend(matched_tables.clone());
            }
            for i in top_pkgs {
                added_pkgs.insert(i);
                let (title, ui_movie) = pkg_meta(i);
                items.push(Item::Package { index: i, title, ui_movie });
            }
            if scope == Scope::All && !in_assembly {
                // Surface the main UI movies and most-used game data tables right away for newcomers.
                for (i, pkg) in packages.iter().enumerate() {
                    if friendly_ui_package(&pkg.name).is_some_and(|(_, core)| core) && added_pkgs.insert(i) {
                        let (title, ui_movie) = pkg_meta(i);
                        items.push(Item::Package { index: i, title, ui_movie });
                    }
                }
                items.extend(matched_tables.into_iter().take(6));
            } else if !in_assembly && scope == Scope::Files {
                items.extend(matched_tables.into_iter().take(8));
            }
            for i in 0..packages.len() {
                if items.len() >= SHOWN {
                    break;
                }
                if added_pkgs.insert(i) {
                    let (title, ui_movie) = pkg_meta(i);
                    items.push(Item::Package { index: i, title, ui_movie });
                }
            }
            items.truncate(SHOWN);
            p.local_items = items.clone();
            p.items = items;
            cx.notify();
            return;
        }

        // 1. Exact package name match always wins slot 0 (keeps `query <pkg>` + `pick 0` deterministic).
        for (i, pkg) in packages.iter().enumerate() {
            if pkg.name.eq_ignore_ascii_case(&f) && added_pkgs.insert(i) {
                let (title, ui_movie) = pkg_meta(i);
                items.push(Item::Package { index: i, title, ui_movie });
            }
        }

        // 2. Champions (in unified All mode).
        if scope == Scope::All {
            let mut champ_hits: Vec<&super::champions::Champion> = champs
                .iter()
                .filter(|c| has(&c.name, &f) || has(&c.reference, &f) || (!squeezed.is_empty() && (super::champions::prefix(&c.name).contains(&squeezed) || super::champions::prefix(&c.reference).contains(&squeezed))))
                .collect();
            champ_hits.sort_by_key(|c| (!c.name.eq_ignore_ascii_case(&f), !c.name.to_ascii_lowercase().starts_with(&f)));
            for c in champ_hits.into_iter().take(12) {
                items.push(Item::Champion { id: c.id, name: c.name.clone(), reference: c.reference.clone() });
            }
        }

        // 3. Curated UI & HUD movie packages when query asks for UI / HUD / menus / lobby.
        let mut core_ui: Vec<usize> = (0..packages.len())
            .filter(|&i| {
                friendly_ui_package(&packages[i].name).is_some_and(|(label, core)| {
                    core && (ui_intent || has(label, &f) || has(&packages[i].name, &f))
                })
            })
            .collect();
        core_ui.sort_by_key(|&i| rank(&packages[i].path));
        for i in core_ui {
            if added_pkgs.insert(i) {
                let (title, ui_movie) = pkg_meta(i);
                items.push(Item::Package { index: i, title, ui_movie });
            }
        }

        // 4. Game data tables (friendly name AND raw `DATA_SET_*` name).
        let table_hits: Vec<Item> = tables
            .into_iter()
            .filter(|(_, _, t, _)| has(short(t), &f) || has(t, &f) || super::assembly::friendly_table(t).is_some_and(|ft| has(ft, &f)))
            .map(|(doc, file, table, rows)| Item::Table { doc, file, table, rows })
            .collect();
        if in_assembly {
            items.splice(0..0, table_hits);
        } else {
            items.extend(table_hits);
        }

        // 5. Game files / Packages (matching raw name, space-stripped champion prefix, or friendly role).
        let mut pkg_hits: Vec<(usize, Option<String>, bool)> = (0..packages.len())
            .filter(|i| !added_pkgs.contains(i))
            .filter_map(|i| {
                let (title, ui_movie) = pkg_meta(i);
                let name = &packages[i].name;
                let matches = has(name, &f)
                    || (!squeezed.is_empty() && super::champions::prefix(name).contains(&squeezed))
                    || title.as_deref().is_some_and(|t| has(t, &f));
                matches.then_some((i, title, ui_movie))
            })
            .collect();
        pkg_hits.sort_by_key(|(i, _, _)| {
            let name_low = packages[*i].name.to_ascii_lowercase();
            let starts = name_low.starts_with(&f) || (!squeezed.is_empty() && super::champions::prefix(&packages[*i].name).starts_with(&squeezed));
            (!starts, rank(&packages[*i].path))
        });
        for (i, title, ui_movie) in pkg_hits.into_iter().take(80) {
            if added_pkgs.insert(i) {
                items.push(Item::Package { index: i, title, ui_movie });
            }
        }

        // 6. Studio commands (in unified All mode).
        if scope == Scope::All {
            for c in enabled {
                let info = c.info();
                if has(info.title, &f) || has(info.group, &f) || has(info.detail, &f) {
                    items.push(Item::Command(c));
                }
            }
        }

        items.truncate(SHOWN);
        p.local_items = items.clone();
        p.items = items;

        // 7. Objects across every package (debounced async append in unified All mode).
        if scope == Scope::All && f.len() >= 2 {
            self.search_append(f, seq, cx);
        } else if p.items.is_empty() {
            p.note = Some("Nothing matched: try # to search inside packages, or > for commands".into());
        }
        cx.notify();
    }

    pub(super) fn palette_confirm(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((item, pick)) = self.palette.as_ref().and_then(|p| p.items.get(row).cloned().map(|i| (i, p.pick.clone()))) else { return };
        let at = self.palette.as_ref().and_then(|p| p.at.clone());
        self.close_palette(window, cx);
        if let Some(pick) = pick {
            self.picked(pick, item, window, cx);
            return;
        }
        match item {
            Item::Command(c) => self.run(c, window, cx),
            Item::Champion { id, .. } => {
                let place = Place::Champions(Some(id as i64));
                self.record(place.clone());
                self.go(place, window, cx);
            }
            Item::Package { index, .. } => {
                let path = self.packages[index].path.clone();
                self.user_open_package(path, cx);
            }
            Item::Export(i) => {
                self.reveal(i, cx);
                self.user_select_export(i, cx);
            }
            Item::Hit { path, index, .. } => {
                let place = Place::package(path, Some(index));
                self.record(place.clone());
                self.go(place, window, cx);
            }
            Item::AddProperty { name, .. } => self.add_property(name, at, cx),
            Item::Table { doc, table, .. } => {
                let place = Place::Assembly { file: doc, path: Vec::new(), table: Some(table), row: None };
                self.record(place.clone());
                self.go(place, window, cx);
            }
            Item::FilterRows { query, .. } => {
                if let Some(a) = self.assembly.clone() {
                    a.update(cx, |a, cx| a.set_filter(query, window, cx));
                }
            }
            Item::Value { .. } => {}
        }
    }

    /// The picker's answer.
    fn picked(&mut self, pick: Pick, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        match (pick, item) {
            (Pick::CopyTo, Item::Package { index, .. }) => {
                let dst = self.packages[index].path.clone();
                self.copy_to(dst, cx);
            }
            (Pick::ReplaceWith, Item::Hit { path, index, .. }) => self.replace_with(path, index, window, cx),
            (Pick::SlotMaterial(slot), Item::Hit { path, index, .. }) => self.set_slot_material(slot, path, index, cx),
            (Pick::Texture(name), Item::Hit { path, index, .. }) => self.set_texture(name, path, index, cx),
            (Pick::AssemblyValue { row, field, .. }, Item::Value { id, .. }) => {
                if let Some(a) = self.assembly.clone() {
                    a.update(cx, |a, cx| a.pick_value_done(row, field, id, cx));
                }
            }
            _ => {}
        }
    }

    pub(super) fn render_palette(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let theme = cx.theme().clone();
        let p = self.palette.as_ref()?;
        let open = self.open.as_ref();
        let q_now = p.query.clone();
        let is_at_ui = strip_tag_prefix(&q_now, "@ui").is_some();
        let is_at_files = strip_tag_prefix(&q_now, "@files").is_some();
        let show_scopes = p.pick.is_none() && !q_now.starts_with('+') && (!q_now.starts_with('@') || is_at_ui || is_at_files);
        let active_scope = scope_for_query(&q_now, p.scope);
        let mono = super::ui::mono();
        let items: Vec<CommandItem> = p
            .items
            .iter()
            .map(|it| {
                let (icon, label, code, detail): (IconName, String, Option<String>, String) = match it {
                    Item::Command(c) => {
                        let info = c.info();
                        let keys = if info.keys.is_empty() { String::new() } else { format!("  {}", key_hint(info.keys)) };
                        (IconName::SquareTerminal, format!("{}: {}", info.group, info.title), None, format!("{}{keys}", info.detail))
                    }
                    Item::Champion { name, reference, .. } => {
                        let code = (reference != name).then(|| reference.clone());
                        (IconName::Users, name.clone(), code, "Champion · skins, abilities & files".into())
                    }
                    Item::Package { index, title, ui_movie } => {
                        let pkg = &self.packages[*index];
                        let icon = if *ui_movie { IconName::Layers } else { IconName::Package };
                        let size = crate::model::human_size(pkg.size);
                        match title {
                            Some(t) => (icon, t.clone(), Some(pkg.name.clone()), size),
                            None => (icon, pkg.name.clone(), None, size),
                        }
                    }
                    Item::Export(i) => open
                        .map(|t| {
                            let e = &t.exports[*i];
                            (super::class_icon(&e.class), e.name.clone(), None, super::pretty_class(&e.class))
                        })
                        .unwrap_or((IconName::FileText, String::new(), None, String::new())),
                    Item::Hit { package, object, class, .. } => (super::class_icon(class), object.clone(), None, format!("{} · {package}", super::pretty_class(class))),
                    Item::AddProperty { name, kind, ty } => (IconName::Plus, name.clone(), None, if ty.is_empty() { kind.clone() } else { format!("{kind} {ty}") }),
                    Item::Table { file, table, rows, .. } => {
                        let raw = table.strip_prefix("DATA_SET_").unwrap_or(table).to_string();
                        let meta = if *rows == 1 {
                            format!("{file} · 1 row")
                        } else if *rows > 1 {
                            format!("{file} · {rows} rows")
                        } else {
                            format!("{file} · game data")
                        };
                        match super::assembly::friendly_table(table) {
                            Some(friendly) => (IconName::Database, friendly.to_string(), Some(raw), meta),
                            None => (IconName::Database, raw, None, meta),
                        }
                    }
                    Item::FilterRows { table, query } => {
                        let t = table.strip_prefix("DATA_SET_").unwrap_or(table);
                        (IconName::Search, if query.is_empty() { format!("Clear the row filter of {t}") } else { format!("Filter {t} rows containing \"{query}\"") }, None, "Game data".to_string())
                    }
                    Item::Value { id, text } => (IconName::Check, text.clone(), None, id.to_string()),
                };
                let muted = theme.muted_foreground;
                CommandItem::new().label(label.clone()).child(move |_, _| {
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(Icon::new(icon).small().text_color(muted))
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1p5()
                                .overflow_hidden()
                                .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().child(label.clone()))
                                .when_some(code.clone(), |d, c| {
                                    d.child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family(mono).text_xs().text_color(muted).child(c))
                                }),
                        )
                        .child(div().flex_none().text_xs().text_color(muted).whitespace_nowrap().overflow_hidden().text_ellipsis().max_w(px(280.0)).child(detail.clone()))
                })
            })
            .collect();
        let me = cx.entity();
        let (on_query, on_confirm, on_cancel, for_scopes) = (me.clone(), me.clone(), me.clone(), me);
        let note = p.note.clone();
        let placeholder = p.pick.as_ref().map(|k| k.placeholder()).unwrap_or(match active_scope {
            Scope::Files => "Filter game files (.upk) & game data tables…",
            Scope::Ui => "Filter menus & HUD movies (lobby, HUD, scoreboard, loading screen, or a champion)…",
            _ => "Search champions, game data, files, UI movies, objects or commands…",
        });
        let border = theme.border;
        let theme_hdr = theme.clone();
        let palette = Command::new(&p.state)
            .bordered(false)
            .filterable(false)
            .items(items)
            .placeholder(placeholder)
            .max_h(px(420.0))
            .when(show_scopes, |c| {
                c.header(move |_, _, _| {
                    let pill = |id: &'static str, label: &'static str, sc: Scope, ent: gpui::Entity<Studio>| {
                        div()
                            .relative()
                            .child(
                                Button::new(id)
                                    .xsmall()
                                    .ghost()
                                    .tab_stop(false)
                                    .selected(active_scope == sc)
                                    .label(label)
                                    .on_click(move |_, window, cx| {
                                        ent.update(cx, |s, cx| s.set_palette_scope(sc, window, cx));
                                    }),
                            )
                            .child(crate::driver::probe(id))
                    };
                    h_flex()
                        .px_3()
                        .py_1p5()
                        .gap_1()
                        .flex_wrap()
                        .border_b_1()
                        .border_color(border)
                        .child(pill("palette-all", "All", Scope::All, for_scopes.clone()))
                        .child(pill("palette-files", "Files (@files)", Scope::Files, for_scopes.clone()))
                        .child(pill("palette-ui", "Menus & HUD (@ui)", Scope::Ui, for_scopes.clone()))
                        .child(pill("palette-objects", "Objects (#)", Scope::Objects, for_scopes.clone()))
                        .child(div().w(px(1.0)).h(px(14.0)).mx_1().bg(border))
                        .child(pill("palette-commands", "Commands (>)", Scope::Commands, for_scopes.clone()))
                        .child(div().flex_1())
                        .child(
                            h_flex()
                                .flex_none()
                                .gap_1()
                                .text_xs()
                                .text_color(theme_hdr.muted_foreground)
                                .child(super::ui::kbd("Tab", &theme_hdr))
                                .child("scope"),
                        )
                })
            })
            .on_query(move |q, _, cx| on_query.update(cx, |s, cx| s.palette_query(q.to_string(), cx)))
            .on_confirm(move |ix, window, cx| on_confirm.update(cx, |s, cx| s.palette_confirm(ix.row, window, cx)))
            .on_cancel(move |window, cx| on_cancel.update(cx, |s, cx| s.close_palette(window, cx)))
            .when_some(note, |c, n| c.footer(move |_, _, _| div().px_3().py_1p5().text_xs().child(n.clone()).into_any_element()));
        Some(
            div()
                .id("palette-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_start()
                .justify_center()
                .bg(theme.overlay)
                .pt(px(72.0))
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, window, cx| this.close_palette(window, cx)))
                .child(
                    div()
                        .id("palette")
                        .w(px(680.0))
                        .max_w(gpui::relative(0.92))
                        .h_auto()
                        .overflow_hidden()
                        .bg(theme.popover)
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .when(show_scopes, |d| {
                            d.capture_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                                let m = &e.keystroke.modifiers;
                                if e.keystroke.key == "tab" && !m.platform && !m.control && !m.alt && !m.function {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    let cur = this.palette.as_ref().map(|p| scope_for_query(&p.query, p.scope)).unwrap_or(active_scope);
                                    let next_scope = if m.shift { cur.prev() } else { cur.next() };
                                    this.set_palette_scope(next_scope, window, cx);
                                }
                            }))
                        })
                        .child(palette),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{class_priority, friendly_ui_package, hit_rank, parse_sorted_hits, scope_for_query, scope_query, strip_tag_prefix, Item, Scope};

    #[test]
    fn cycles_scopes_and_handles_at_files_and_at_ui_prefixes() {
        assert_eq!(Scope::All.next(), Scope::Files);
        assert_eq!(Scope::Files.next(), Scope::Ui);
        assert_eq!(Scope::Ui.next(), Scope::Objects);
        assert_eq!(Scope::Objects.next(), Scope::Commands);
        assert_eq!(Scope::Commands.next(), Scope::All);

        assert_eq!(Scope::All.prev(), Scope::Commands);
        assert_eq!(Scope::Commands.prev(), Scope::Objects);
        assert_eq!(Scope::Objects.prev(), Scope::Ui);
        assert_eq!(Scope::Ui.prev(), Scope::Files);
        assert_eq!(Scope::Files.prev(), Scope::All);

        assert_eq!(strip_tag_prefix("@files", "@files"), Some(""));
        assert_eq!(strip_tag_prefix("  @files cassie", "@files"), Some(" cassie"));
        assert_eq!(strip_tag_prefix("@FILES\tcassie", "@files"), Some("\tcassie"));
        assert_eq!(strip_tag_prefix("@filesystem", "@files"), None);

        assert_eq!(scope_for_query("@files", Scope::All), Scope::Files);
        assert_eq!(scope_for_query("@files cassie", Scope::Ui), Scope::Files);
        assert_eq!(scope_for_query("@ui lobby", Scope::All), Scope::Ui);
        assert_eq!(scope_for_query("#cassie", Scope::All), Scope::Objects);
        assert_eq!(scope_for_query(">save", Scope::All), Scope::Commands);

        assert_eq!(scope_query("@files cassie", Scope::Ui), "cassie");
        assert_eq!(scope_query("@files cassie", Scope::Objects), "#cassie");
        assert_eq!(scope_query("@files cassie", Scope::Commands), ">cassie");
        assert_eq!(scope_query("@files", Scope::All), "");
        assert_eq!(scope_query("@ui lobby", Scope::Files), "lobby");
        assert_eq!(scope_query("#cassie", Scope::Files), "cassie");
        assert_eq!(scope_query(">save", Scope::All), "save");
    }

    #[test]
    fn lobby_assets_are_not_misclassified_as_ui() {
        assert_eq!(friendly_ui_package("CASSIE_LOBBY_ASSETS_SF"), None);
        assert_eq!(friendly_ui_package("LobbyUI_SF"), Some(("Main menu, play screen & lobby", true)));
        assert_eq!(friendly_ui_package("GameUI_SF"), Some(("In-match HUD, health bars & scoreboard", true)));
        assert_eq!(friendly_ui_package("TgGameUI"), Some(("In-match HUD, health bars & scoreboard", true)));
        assert_eq!(friendly_ui_package("CASSIE_UI_ASSETS_SF"), Some(("Champion portraits & ability icons", false)));
    }

    #[test]
    fn hit_rank_prefers_matching_champion_package_over_alphabetical() {
        let exact = hit_rank("CASSIE_ASSETS_SF", "CASSIE_ASSETS_SF.Group.Cassie", "cassie", "CASSIE");
        let own_pkg = hit_rank("CASSIE_ASSETS_SF", "CASSIE_ASSETS_SF.Meshes.SK_Cassie_Body", "cassie", "CASSIE");
        let other_pkg = hit_rank("ANDROXUS_ASSETS_SF", "ANDROXUS_ASSETS_SF.VOX.Kill_Cassie", "cassie", "CASSIE");
        assert!(exact < own_pkg);
        assert!(own_pkg < other_pkg);
    }

    #[test]
    fn creative_visual_assets_rank_ahead_of_voice_events_and_anim_sequences() {
        assert_eq!(class_priority("SkeletalMesh"), 0);
        assert_eq!(class_priority("Texture2D"), 0);
        assert_eq!(class_priority("GFxMovieInfo"), 0);
        assert_eq!(class_priority("MaterialInstanceConstant"), 1);
        assert_eq!(class_priority("AnimSet"), 2);
        assert_eq!(class_priority("AkEvent"), 3);
        assert_eq!(class_priority("AnimSequence"), 3);
        assert_eq!(class_priority("Package"), 4);
        assert_eq!(class_priority("AnimNotify_AkEvent"), 5);

        let raw = json!([
            { "package": "CASSIE_ASSETS_SF", "path": "/g/CASSIE_ASSETS_SF.upk", "index": 1, "object": "CASSIE_ASSETS_SF.Anims.AnimNotify_AkEvent_0", "class": "AnimNotify_AkEvent" },
            { "package": "CASSIE_ASSETS_SF", "path": "/g/CASSIE_ASSETS_SF.upk", "index": 2, "object": "CASSIE_ASSETS_SF.VOX.Play_VOX_Cassie_Taunt", "class": "AkEvent" },
            { "package": "CASSIE_ASSETS_SF", "path": "/g/CASSIE_ASSETS_SF.upk", "index": 3, "object": "CASSIE_ASSETS_SF.Anims.AS_Cassie_Idle", "class": "AnimSequence" },
            { "package": "CASSIE_ASSETS_SF", "path": "/g/CASSIE_ASSETS_SF.upk", "index": 4, "object": "CASSIE_ASSETS_SF.Anims.K_Cassie_AnimSet", "class": "AnimSet" },
            { "package": "CASSIE_ASSETS_SF", "path": "/g/CASSIE_ASSETS_SF.upk", "index": 5, "object": "CASSIE_ASSETS_SF.Textures.T_Cassie_Body_Diff", "class": "Texture2D" },
            { "package": "CASSIE_ASSETS_SF", "path": "/g/CASSIE_ASSETS_SF.upk", "index": 6, "object": "CASSIE_ASSETS_SF.Meshes.SK_Cassie_Body", "class": "SkeletalMesh" }
        ]);
        let sorted = parse_sorted_hits(&raw, "Cassie");
        let classes: Vec<&str> = sorted
            .iter()
            .filter_map(|it| match it {
                Item::Hit { class, .. } => Some(class.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(classes[0..2], ["Texture2D", "SkeletalMesh"]);
        assert_eq!(classes[2], "AnimSet");
        assert_eq!(classes[3..5], ["AkEvent", "AnimSequence"]);
        assert_eq!(classes[5], "AnimNotify_AkEvent");
    }
}
