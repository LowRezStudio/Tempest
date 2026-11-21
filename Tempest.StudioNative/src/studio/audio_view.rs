//! The Audio view (⌘4): every Wwise `.pck` of the game folder, or a
//! champion's or item's sounds (`wwise.owners`: the files the assembly links
//! to it, opened together with a section per file). Pick a file, then an
//! event (named from the packages' AkEvent exports) to see the media it
//! plays, or browse every medium; each one plays, is edited (the sound
//! editor), exports (singly or all at once) and is replaced from a file (an
//! unsaved edit of the `.pck`, saved like a package). With Settings ▸ Name
//! sounds by what plays them, events read in words with their ability, and
//! sounds by their role (heard by enemies, variant 2 of 5).

use std::collections::HashMap;
use std::ops::Range;

use gpui::{div, prelude::*, px, uniform_list, AnyElement, Context, Entity, FocusHandle, KeyDownEvent, ScrollStrategy, UniformListScrollHandle, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Selectable, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::sound_names;
use super::{file_name, layout, Studio, Tone};

/// What the left list shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListMode {
    Files,
    Champions,
    Items,
}

pub struct AudioBrowser {
    /// Name, path, size and subfolder (`English(US)` for voice lines) of each `.pck`.
    packages: Vec<(String, String, u64, Option<String>)>,
    mode: ListMode,
    /// `wwise.owners`: champions and items with their sound files, read when first listed.
    owners: Option<J>,
    owners_loading: bool,
    filter: Entity<InputState>,
    events_filter: Entity<InputState>,
    banks_focus: FocusHandle,
    events_focus: FocusHandle,
    media_focus: FocusHandle,
    /// What is open: a `.pck` path, or `champion:<id>` / `item:<id>`.
    open: Option<String>,
    /// The `wwise.open` call for `open` (its files, labels, owner), to read it again.
    open_params: Option<J>,
    open_error: Option<String>,
    info: Option<J>,
    /// The event whose media are shown (None: every medium).
    event: Option<u64>,
    selected_media: Option<u64>,
    loading: bool,
    banks_scroll: UniformListScrollHandle,
    media_scroll: UniformListScrollHandle,
}

/// A sound of the list: the medium, why it plays and the event playing it.
struct MediaRow<'a> {
    m: &'a J,
    pck: String,
    when: Option<&'a J>,
    event: Option<String>,
}

impl MediaRow<'_> {
    fn id(&self) -> u64 {
        self.m["id"].as_u64().unwrap_or(0)
    }

    /// `Ultimate · heard by enemies`, or `Sound 3` without names.
    fn title(&self, k: usize, names: bool) -> String {
        let role = self.when.map(sound_names::when_text).unwrap_or_default();
        match (&self.event, names) {
            (Some(e), true) if role.is_empty() => e.clone(),
            (Some(e), true) => format!("{e} · {role}"),
            _ => format!("Sound {}", k + 1),
        }
    }

    /// The export file name: the title and id, or the id.
    fn stem(&self, k: usize, names: bool) -> String {
        if names && self.event.is_some() {
            format!("{} ({})", sound_names::file_stem(&self.title(k, true)), self.id())
        } else {
            self.id().to_string()
        }
    }
}

fn owner_key(kind: &str, id: u64) -> String {
    format!("{kind}:{id}")
}

/// `("champion", 2560)` of `champion:2560`.
fn parse_owner_key(key: &str) -> Option<(&str, u64)> {
    let (kind, id) = key.split_once(':')?;
    matches!(kind, "champion" | "item").then_some(())?;
    Some((kind, id.parse().ok()?))
}

/// An event's name as shown: in words with names on, else the game's.
fn event_name(e: &J, names: bool) -> String {
    let raw = e["name"].as_str().map(str::to_string).unwrap_or_else(|| format!("event {}", e["id"]));
    if names { e["title"].as_str().filter(|t| !t.is_empty()).map(str::to_string).unwrap_or(raw) } else { raw }
}

/// The ability, card or skin an event belongs to (not the champion itself).
fn event_part(e: &J) -> Option<String> {
    let part = e["part"].as_str().filter(|p| !p.is_empty() && *p != "Champion")?;
    Some(match e["skin"].as_str() {
        Some(s) if s != part => format!("{part} · {s}"),
        _ => part.to_string(),
    })
}

impl AudioBrowser {
    fn owner_list(&self, mode: ListMode) -> &[J] {
        let Some(o) = &self.owners else { return &[] };
        let list = match mode {
            ListMode::Items => &o["items"],
            _ => &o["champions"],
        };
        list.as_array().map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn owner(&self, key: &str) -> Option<&J> {
        let (kind, id) = parse_owner_key(key)?;
        let mode = if kind == "item" { ListMode::Items } else { ListMode::Champions };
        self.owner_list(mode).iter().find(|o| o["id"].as_u64() == Some(id))
    }

    /// The `.pck` files open.
    fn open_paths(&self) -> Vec<String> {
        let Some(p) = &self.open_params else { return Vec::new() };
        match p["path"].as_str() {
            Some(path) => vec![path.to_string()],
            None => p["banks"].as_array().into_iter().flatten().filter_map(|b| b["path"].as_str().map(str::to_string)).collect(),
        }
    }

    /// What the media list shows: the picked event's sounds, or every one of the files.
    fn media(&self, names: bool) -> Vec<MediaRow<'_>> {
        let Some(info) = &self.info else { return Vec::new() };
        let pool = bank_media(info);
        let events = || info["banks"].as_array().into_iter().flatten().flat_map(|b| b["events"].as_array().into_iter().flatten().map(move |e| (b, e)));
        match self.event {
            Some(ev) => {
                let Some((bank, e)) = events().find(|(_, e)| e["id"].as_u64() == Some(ev)) else { return Vec::new() };
                let pck = bank["pck"].as_str().unwrap_or("").to_string();
                // The event's own file first: an id may be in two files of a set.
                let by_id: HashMap<u64, &J> = pool.iter().rev().filter(|m| m["pck"] != pck.as_str()).chain(pool.iter().rev().filter(|m| m["pck"] == pck.as_str())).filter_map(|m| Some((m["id"].as_u64()?, *m))).collect();
                let title = event_name(e, names);
                e["sounds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| {
                        let m = *by_id.get(&s["id"].as_u64()?)?;
                        Some(MediaRow { m, pck: m["pck"].as_str().unwrap_or("").to_string(), when: Some(&s["when"]), event: Some(title.clone()) })
                    })
                    .collect()
            }
            None => {
                let titles: HashMap<&str, String> = events().filter_map(|(_, e)| Some((e["name"].as_str()?, event_name(e, names)))).collect();
                pool.into_iter()
                    .map(|m| MediaRow {
                        m,
                        pck: m["pck"].as_str().unwrap_or("").to_string(),
                        when: m.get("when"),
                        event: m["event"].as_str().map(|e| titles.get(e).cloned().unwrap_or_else(|| e.to_string())),
                    })
                    .collect()
            }
        }
    }

    /// Indices into the current list (files, champions or items) of the rows the filter lets through.
    fn shown_rows(&self, cx: &gpui::App) -> Vec<usize> {
        let f = self.filter.read(cx).value().to_ascii_lowercase();
        match self.mode {
            ListMode::Files => (0..self.packages.len()).filter(|&i| f.is_empty() || self.packages[i].0.to_ascii_lowercase().contains(&f)).collect(),
            mode => {
                let list = self.owner_list(mode);
                (0..list.len())
                    .filter(|&i| f.is_empty() || [&list[i]["name"], &list[i]["kind"], &list[i]["reference"]].iter().any(|v| v.as_str().is_some_and(|s| s.to_ascii_lowercase().contains(&f))))
                    .collect()
            }
        }
    }

    /// The key a row of the current list opens.
    fn row_key(&self, i: usize) -> Option<String> {
        match self.mode {
            ListMode::Files => self.packages.get(i).map(|p| p.1.clone()),
            ListMode::Champions => self.owner_list(ListMode::Champions).get(i).and_then(|o| o["id"].as_u64()).map(|id| owner_key("champion", id)),
            ListMode::Items => self.owner_list(ListMode::Items).get(i).and_then(|o| o["id"].as_u64()).map(|id| owner_key("item", id)),
        }
    }

    /// Dev: the path of the sound file named `name`.
    pub fn package_path(&self, name: &str) -> Option<String> {
        self.packages
            .iter()
            .find(|p| p.1 == name || p.0.eq_ignore_ascii_case(name) || file_name(&p.1).eq_ignore_ascii_case(name))
            .map(|p| p.1.clone())
    }

    /// Dev: the key of the champion or item named `name`.
    pub fn owner_by_name(&self, name: &str) -> Option<String> {
        for (mode, kind) in [(ListMode::Champions, "champion"), (ListMode::Items, "item")] {
            if let Some(id) = self.owner_list(mode).iter().find(|o| o["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))).and_then(|o| o["id"].as_u64()) {
                return Some(owner_key(kind, id));
            }
        }
        None
    }

    pub(super) fn clear_open(&mut self) {
        self.open = None;
        self.open_params = None;
        self.open_error = None;
        self.info = None;
        self.event = None;
        self.selected_media = None;
    }

    /// The events filter once a sound file is open, else the files filter (⌘F).
    pub(super) fn filter_input(&self) -> Entity<InputState> {
        if self.open.is_some() { self.events_filter.clone() } else { self.filter.clone() }
    }

    /// What is open, as a tab or status shows it: the file, champion or item name.
    pub(super) fn title(&self) -> Option<String> {
        let open = self.open.as_deref()?;
        Some(match parse_owner_key(open) {
            Some(_) => self.owner(open).and_then(|o| o["name"].as_str().map(str::to_string)).or_else(|| self.info.as_ref().and_then(|i| i["name"].as_str().map(str::to_string))).unwrap_or_else(|| "Sounds".into()),
            None => file_name(open).trim_end_matches(".pck").to_string(),
        })
    }

    /// The events of the open files the filter lets through, as `(bank, event)`.
    fn shown_events<'a>(&'a self, filter: &str, names: bool) -> Vec<(&'a J, &'a J)> {
        let Some(info) = &self.info else { return Vec::new() };
        info["banks"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|b| b["events"].as_array().into_iter().flatten().map(move |e| (b, e)))
            .filter(|(_, e)| {
                filter.is_empty()
                    || [Some(event_name(e, names)), e["name"].as_str().map(str::to_string), event_part(e)].into_iter().flatten().any(|s| s.to_ascii_lowercase().contains(filter))
            })
            .collect()
    }
}

fn bank_media(info: &J) -> Vec<&J> {
    info["banks"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|b| b["media"].as_array().into_iter().flatten())
        .chain(info["streams"].as_array().into_iter().flatten())
        .collect()
}

impl Studio {
    pub(super) fn audio_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.audio.is_some() {
            return;
        }
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter sound files"));
        let events_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter events by name"));
        for f in [&filter, &events_filter] {
            cx.subscribe(f, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
        }
        self.audio = Some(AudioBrowser {
            packages: Vec::new(),
            mode: ListMode::Files,
            owners: None,
            owners_loading: false,
            filter,
            events_filter,
            banks_focus: cx.focus_handle(),
            events_focus: cx.focus_handle(),
            media_focus: cx.focus_handle(),
            open: None,
            open_params: None,
            open_error: None,
            info: None,
            event: None,
            selected_media: None,
            loading: true,
            banks_scroll: UniformListScrollHandle::new(),
            media_scroll: UniformListScrollHandle::new(),
        });
        let call = self.backend.call("wwise.packages", json!({}));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        if let Some(a) = &mut this.audio {
                            a.loading = false;
                            a.packages = r.json["packages"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(|p| (p["name"].as_str().unwrap_or("").to_string(), p["path"].as_str().unwrap_or("").to_string(), p["size"].as_u64().unwrap_or(0), p["folder"].as_str().map(str::to_string)))
                                .collect();
                        }
                    }
                    Err(e) => {
                        if let Some(a) = &mut this.audio {
                            a.loading = false;
                        }
                        this.toast(Tone::Error, super::ui::explain_error(&format!("Could not list sound banks: {e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Read the champions and items with sounds (once); a champion or item
    /// waiting to open opens then.
    fn load_audio_owners(&mut self, cx: &mut Context<Self>) {
        let Some(a) = &mut self.audio else { return };
        if a.owners.is_some() || a.owners_loading {
            return;
        }
        a.owners_loading = true;
        self.set_status("Reading which sounds each champion and item plays (once per game folder)…", cx);
        let call = self.backend.call("wwise.owners", json!({}));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                let Some(a) = &mut this.audio else { return };
                a.owners_loading = false;
                match r {
                    Ok(r) => {
                        a.owners = Some(r.json);
                        let waiting = a.open.clone().filter(|k| parse_owner_key(k).is_some() && a.open_params.is_none());
                        this.set_status("Sounds by champion and item", cx);
                        if let Some(k) = waiting {
                            this.audio_open(k, cx);
                        }
                    }
                    Err(e) => {
                        a.owners = Some(json!({ "champions": [], "items": [] }));
                        this.toast(Tone::Error, super::ui::explain_error(&format!("Could not read which sounds the champions play: {e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_audio_mode(&mut self, mode: ListMode, cx: &mut Context<Self>) {
        let Some(a) = &mut self.audio else { return };
        a.mode = mode;
        a.banks_scroll.scroll_to_item(0, ScrollStrategy::Top);
        if mode != ListMode::Files {
            self.load_audio_owners(cx);
        }
        cx.notify();
    }

    pub(super) fn open_audio_path(&self) -> Option<&str> {
        self.audio.as_ref()?.open.as_deref()
    }

    /// The `.pck` files the Audio view shows.
    pub(super) fn open_audio_paths(&self) -> Vec<String> {
        self.audio.as_ref().map(|a| a.open_paths()).unwrap_or_default()
    }

    /// Re-read the open files after a replacement, undo, or redo without resetting event/media selection.
    pub(super) fn refresh_audio_open(&mut self, pck: &str, cx: &mut Context<Self>) {
        let Some(a) = self.audio.as_ref() else { return };
        if !a.open_paths().iter().any(|p| super::project::same_path(p, pck)) {
            return;
        }
        let (Some(open), Some(params)) = (a.open.clone(), a.open_params.clone()) else { return };
        let call = self.backend.call("wwise.open", params);
        cx.spawn(async move |this, cx| {
            if let Ok(r) = call.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(a) = this.audio.as_mut().filter(|a| a.open.as_deref() == Some(open.as_str())) {
                        a.info = Some(r.json);
                        a.open_error = None;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// Settings ▸ Name sounds by what plays them changed: read the open files again (labels).
    pub(super) fn sound_names_changed(&mut self, cx: &mut Context<Self>) {
        let names = self.prefs.sound_names;
        if let Some(p) = self.audio.as_mut().and_then(|a| a.open_params.as_mut()) {
            p["labels"] = J::from(names);
        }
        if let Some(first) = self.open_audio_paths().first().cloned() {
            self.refresh_audio_open(&first, cx);
        }
        cx.notify();
    }

    /// Open a `.pck` path, or a champion's or item's files (`champion:<id>`, `item:<id>`).
    pub(super) fn audio_open(&mut self, key: String, cx: &mut Context<Self>) {
        let names = self.prefs.sound_names;
        let Some(a) = &mut self.audio else { return };
        a.open = Some(key.clone());
        a.open_params = None;
        a.open_error = None;
        a.info = None;
        a.event = None;
        a.selected_media = None;
        let params = match parse_owner_key(&key) {
            Some((kind, id)) => {
                a.mode = if kind == "item" { ListMode::Items } else { ListMode::Champions };
                if a.owners.is_none() {
                    // Opened once they are read.
                    self.load_audio_owners(cx);
                    return;
                }
                let Some(o) = a.owner(&key) else {
                    a.open_error = Some("This champion or item has no sounds in this game folder".into());
                    cx.notify();
                    return;
                };
                let banks: Vec<J> = o["banks"].as_array().into_iter().flatten().map(|b| json!({ "path": b["path"], "only": b["only"] })).collect();
                json!({ "banks": banks, "labels": names, "owner": { "kind": kind, "id": id }, "title": o["name"] })
            }
            None => json!({ "path": key, "labels": names }),
        };
        a.open_params = Some(params.clone());
        let title = a.title().unwrap_or_default();
        let call = self.backend.call("wwise.open", params);
        self.set_status(format!("Reading {title}… (the first time, every package is indexed for event names)"), cx);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        if let Some(a) = this.audio.as_mut().filter(|a| a.open.as_deref() == Some(key.as_str())) {
                            a.info = Some(r.json);
                            a.open_error = None;
                        }
                        this.set_status(title.clone(), cx);
                    }
                    Err(e) => {
                        if let Some(a) = this.audio.as_mut().filter(|a| a.open.as_deref() == Some(key.as_str())) {
                            a.open_error = Some(e.clone());
                        }
                        this.toast(Tone::Error, super::ui::explain_error(&format!("Could not open {title}: {e}")), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn user_audio_open(&mut self, key: String, close_drawer: bool, cx: &mut Context<Self>) {
        if close_drawer {
            layout::update(&self.panes, |p| p.overlay = false);
        }
        self.record(super::Place::Audio(Some(key.clone())));
        self.audio_open(key, cx);
    }

    /// Dev: `audio <pck name>`, `audio champion|item <name>` (waits for the
    /// list), `audio mode files|champions|items`, `audio event <name>` (the
    /// first event whose shown or game name holds it), `audio edit <k>` (the
    /// editor on the k-th sound shown, from 0).
    pub(super) fn drive_audio(&mut self, arg: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let names = self.prefs.sound_names;
        let (verb, rest) = arg.split_once(' ').unwrap_or((arg, ""));
        let rest = rest.trim().to_string();
        match verb {
            "mode" => {
                let m = match rest.as_str() {
                    "files" => ListMode::Files,
                    "champions" => ListMode::Champions,
                    "items" => ListMode::Items,
                    _ => return Err("mode files|champions|items".into()),
                };
                self.set_audio_mode(m, cx);
            }
            "champion" | "item" => {
                self.set_audio_mode(if verb == "item" { ListMode::Items } else { ListMode::Champions }, cx);
                cx.spawn(async move |this, cx| {
                    for _ in 0..600 {
                        let Ok(found) = this.update(cx, |this, cx| {
                            let a = this.audio.as_ref()?;
                            a.owners.as_ref()?;
                            let key = a.owner_by_name(&rest);
                            if let Some(k) = key.clone() {
                                this.user_audio_open(k, true, cx);
                            }
                            Some(key.is_some())
                        }) else { return };
                        if found.is_some() {
                            return;
                        }
                        cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
                    }
                })
                .detach();
            }
            "event" => {
                let a = self.audio.as_mut().ok_or("no Audio view")?;
                let want = rest.to_ascii_lowercase();
                let id = a.shown_events("", names).into_iter().find(|(_, e)| event_name(e, names).to_ascii_lowercase().contains(&want) || e["name"].as_str().is_some_and(|n| n.contains(&want))).and_then(|(_, e)| e["id"].as_u64()).ok_or_else(|| format!("no event {rest}"))?;
                a.event = Some(id);
                a.selected_media = None;
            }
            "edit" => {
                let k: usize = rest.parse().map_err(|_| "edit <k>")?;
                let a = self.audio.as_ref().ok_or("no Audio view")?;
                let (pck, id) = a.media(names).get(k).map(|r| (r.pck.clone(), r.id())).ok_or_else(|| format!("no sound {k}"))?;
                self.edit_wwise_sound(pck, id, cx);
            }
            _ => {
                let path = self.audio.as_ref().and_then(|a| a.package_path(arg)).ok_or_else(|| format!("no sound file {arg} (yet)"))?;
                self.user_audio_open(path, true, cx);
            }
        }
        cx.notify();
        Ok(())
    }

    /// `studio_state.audio`: the list shown, what is open, its events and
    /// sounds as shown (first 60), the editor.
    pub(super) fn audio_state_json(&self) -> J {
        let names = self.prefs.sound_names;
        let Some(a) = &self.audio else { return J::Null };
        let events: Vec<J> = a.shown_events("", names).into_iter().take(60).map(|(b, e)| json!({ "file": b["file"], "name": e["name"], "shown": event_name(e, names), "part": event_part(e), "sounds": e["media"].as_array().map(|m| m.len()) })).collect();
        let media: Vec<J> = a.media(names).iter().enumerate().take(60).map(|(k, r)| json!({ "id": r.id(), "file": file_name(&r.pck), "shown": r.title(k, names), "stem": r.stem(k, names) })).collect();
        json!({
            "mode": format!("{:?}", a.mode),
            "owners": a.owners.as_ref().map(|o| json!({ "champions": o["champions"].as_array().map(|v| v.len()), "items": o["items"].as_array().map(|v| v.len()) })),
            "open": a.open,
            "title": a.title(),
            "files": a.open_paths().iter().map(|p| file_name(p)).collect::<Vec<_>>(),
            "event": a.event,
            "events": events,
            "media": media,
            "names": names,
            "editing": self.ww_sound.as_ref().and_then(|s| s.wwise.as_ref()).map(|(p, id)| json!({ "file": file_name(p), "id": id })),
        })
    }

    fn step_audio_bank(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(a) = &self.audio else { return };
        let shown: Vec<String> = a.shown_rows(cx).into_iter().filter_map(|i| a.row_key(i)).collect();
        if shown.is_empty() {
            return;
        }
        let focus = a.banks_focus.clone();
        let at = a.open.as_ref().and_then(|cur| shown.iter().position(|p| p == cur));
        let next = match (at, by) {
            (_, isize::MIN) => 0,
            (_, isize::MAX) => shown.len() - 1,
            (Some(i), d) => (i as isize + d).clamp(0, shown.len() as isize - 1) as usize,
            (None, d) if d < 0 => shown.len() - 1,
            (None, _) => 0,
        };
        a.banks_scroll.scroll_to_item(next, ScrollStrategy::Nearest);
        if at != Some(next) {
            self.user_audio_open(shown[next].clone(), false, cx);
        }
        focus.focus(window, cx);
    }

    fn step_audio_event(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let names = self.prefs.sound_names;
        let Some(a) = &self.audio else { return };
        let ef = a.events_filter.read(cx).value().to_ascii_lowercase();
        let choices: Vec<Option<u64>> = std::iter::once(None).chain(a.shown_events(&ef, names).into_iter().map(|(_, e)| Some(e["id"].as_u64().unwrap_or(0)))).collect();
        let at = choices.iter().position(|c| *c == a.event);
        let next = match (at, by) {
            (_, isize::MIN) => 0,
            (_, isize::MAX) => choices.len() - 1,
            (Some(i), d) => (i as isize + d).clamp(0, choices.len() as isize - 1) as usize,
            (None, _) => 0,
        };
        let Some(a) = &mut self.audio else { return };
        a.event = choices[next];
        a.selected_media = None;
        let focus = a.events_focus.clone();
        focus.focus(window, cx);
        cx.notify();
    }

    fn step_audio_media(&mut self, by: isize, ids: &[u64], window: &mut Window, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        let Some(a) = &mut self.audio else { return };
        let at = a.selected_media.and_then(|cur| ids.iter().position(|&id| id == cur));
        let next = match (at, by) {
            (_, isize::MIN) => 0,
            (_, isize::MAX) => ids.len() - 1,
            (Some(i), d) => (i as isize + d).clamp(0, ids.len() as isize - 1) as usize,
            (None, d) if d < 0 => ids.len() - 1,
            (None, _) => 0,
        };
        a.selected_media = Some(ids[next]);
        a.media_scroll.scroll_to_item(next, ScrollStrategy::Nearest);
        let focus = a.media_focus.clone();
        focus.focus(window, cx);
        cx.notify();
    }

    fn render_audio_banks(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let row_h = super::ui::row_h(window);
        let Some(a) = self.audio.as_ref() else { return Vec::new() };
        let banks_focus = a.banks_focus.clone();
        let banks_focused = banks_focus.is_focused(window) || banks_focus.contains_focused(window, cx);
        let shown = a.shown_rows(cx);
        let row = |k: usize, key: String, sel: bool, name: String, cx: &mut Context<Self>| {
            let bf = banks_focus.clone();
            h_flex()
                .id(("pck", k))
                .focusable()
                .w_full()
                .min_h(row_h)
                .py_0p5()
                .px_2()
                .gap_2()
                .text_sm()
                .border_1()
                .border_color(if sel && banks_focused { theme.ring } else { gpui::transparent_black() })
                .focus_visible(|s| s.border_color(theme.ring))
                .cursor_pointer()
                .when(sel, |d| d.bg(theme.list_active))
                .when(!sel, |d| d.hover(|s| s.bg(theme.list_hover)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.user_audio_open(key.clone(), true, cx);
                    bf.focus(window, cx);
                }))
                .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name))
        };
        match a.mode {
            ListMode::Files => range
                .filter_map(|k| shown.get(k).map(|&i| (k, &a.packages[i])))
                .map(|(k, (name, path, size, folder))| {
                    let sel = a.open.as_deref() == Some(path.as_str());
                    let is_dirty = self.dirty.contains(path);
                    row(k, path.clone(), sel, name.clone(), cx)
                        .when_some(folder.clone(), |d, f| d.child(super::ui::folder_mark(("pck-folder", k), f, &theme)))
                        .when(is_dirty, |d| {
                            d.child(
                                h_flex()
                                    .flex_none()
                                    .gap_1()
                                    .child(div().size(px(6.0)).rounded_full().bg(theme.warning))
                                    .child(div().text_xs().text_color(theme.warning).child("edited")),
                            )
                        })
                        .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(crate::model::human_size(*size)))
                        .into_any_element()
                })
                .collect(),
            mode => {
                let list = a.owner_list(mode);
                let kind = if mode == ListMode::Items { "item" } else { "champion" };
                range
                    .filter_map(|k| shown.get(k).map(|&i| (k, &list[i])))
                    .map(|(k, o)| {
                        let key = owner_key(kind, o["id"].as_u64().unwrap_or(0));
                        let sel = a.open.as_deref() == Some(key.as_str());
                        let files = o["banks"].as_array().map(|b| b.len()).unwrap_or(0);
                        let is_dirty = o["banks"].as_array().into_iter().flatten().any(|b| b["path"].as_str().is_some_and(|p| self.dirty.contains(p)));
                        let detail = match mode {
                            ListMode::Items => o["kind"].as_str().unwrap_or("").to_string(),
                            _ => format!("{files} file{}", if files == 1 { "" } else { "s" }),
                        };
                        row(k, key, sel, o["name"].as_str().unwrap_or("").to_string(), cx)
                            .when(is_dirty, |d| d.child(div().flex_none().size(px(6.0)).rounded_full().bg(theme.warning)))
                            .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(detail))
                            .into_any_element()
                    })
                    .collect()
            }
        }
    }

    fn render_audio_media(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let row_h = super::ui::row_h(window);
        let names = self.prefs.sound_names;
        let Some(a) = self.audio.as_ref() else { return Vec::new() };
        let media_focus = a.media_focus.clone();
        let media_focused = media_focus.is_focused(window) || media_focus.contains_focused(window, cx);
        let playing = self.player.playing();
        let several = a.open_paths().len() > 1;
        let media = a.media(names);
        range
            .filter_map(|k| media.get(k).map(|m| (k, m)))
            .map(|(k, row)| {
                let m = row.m;
                let id = row.id();
                let now = playing && self.ww_playing == Some(id);
                let sel_m = a.selected_media == Some(id);
                let editing = self.wwise_editing(&row.pck, id);
                let (p1, p2, p3, p4) = (row.pck.clone(), row.pck.clone(), row.pck.clone(), row.pck.clone());
                let stem = row.stem(k, names);
                let mf = media_focus.clone();
                // Sounds have no names, only ids: their event and role (or length) tell them apart.
                let title = if names && row.event.is_some() { row.title(k, true) } else { format!("{} · {:.2} s", row.title(k, false), m["duration"].as_f64().unwrap_or(0.0)) };
                let detail = format!(
                    "{}id {} · {} ch · {} Hz · {}{}{}",
                    if names && row.event.is_some() { format!("{:.2} s · ", m["duration"].as_f64().unwrap_or(0.0)) } else { String::new() },
                    id,
                    m["channels"],
                    m["sampleRate"],
                    crate::model::human_size(m["bytes"].as_u64().unwrap_or(0)),
                    if m["streamed"].as_bool().unwrap_or(false) { " · streamed" } else { "" },
                    if several { format!(" · {}", file_name(&row.pck).trim_end_matches(".pck")) } else { String::new() }
                );
                div().pb_1().child(
                    h_flex()
                        .id(("am-row", k))
                        .focusable()
                        .min_h(row_h)
                        .gap_2()
                        .p_1p5()
                        .rounded_md()
                        .border_1()
                        .border_color(if sel_m && media_focused { theme.ring } else { gpui::transparent_black() })
                        .focus_visible(|s| s.border_color(theme.ring))
                        .when(sel_m, |d| d.bg(theme.list_active))
                        .when(!sel_m, |d| d.hover(|s| s.bg(theme.list_hover)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(a) = &mut this.audio {
                                a.selected_media = Some(id);
                            }
                            mf.focus(window, cx);
                            cx.notify();
                        }))
                        .child(
                            Button::new(("am-play", k))
                                .xsmall()
                                .selected(now)
                                .icon(Icon::new(if now { IconName::Square } else { IconName::Play }))
                                .label(if now { "Stop" } else { "Play" })
                                .on_click(cx.listener(move |this, _, _, cx| this.wwise_play_in(p1.clone(), id, cx))),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().text_sm().child(title))
                                .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().text_color(theme.muted_foreground).child(detail)),
                        )
                        .child(Button::new(("am-edit", k)).xsmall().ghost().selected(editing).label("Edit").tooltip("Trim, fade, gain, normalise, then re-encode into the sound bank").on_click(cx.listener(move |this, _, _, cx| this.edit_wwise_sound(p4.clone(), id, cx))))
                        .child(Button::new(("am-exp", k)).xsmall().ghost().label("Export…").on_click(cx.listener(move |this, _, _, cx| this.wwise_export_in(p2.clone(), id, stem.clone(), cx))))
                        .child(Button::new(("am-rep", k)).xsmall().ghost().label("Replace…").tooltip("An .ogg, .wav or .wem file, re-encoded to Wwise Vorbis").on_click(cx.listener(move |this, _, _, cx| this.wwise_replace_in(p3.clone(), id, cx)))),
                )
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_audio_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.audio_browser(window, cx);
        let theme = cx.theme().clone();
        let row_h = super::ui::row_h(window);
        let names = self.prefs.sound_names;
        let a = self.audio.as_ref().expect("made above");
        let banks_focus = a.banks_focus.clone();
        let events_focus = a.events_focus.clone();
        let media_focus = a.media_focus.clone();
        let events_focused = events_focus.is_focused(window) || events_focus.contains_focused(window, cx);

        // 1. Sound files, champions or items (left), virtual: a game has over a thousand files.
        let mode = a.mode;
        let rows_count = a.shown_rows(cx).len();
        let files = uniform_list("pck-rows", rows_count, cx.processor(Self::render_audio_banks)).track_scroll(&a.banks_scroll).size_full();
        let (title, count) = match mode {
            ListMode::Files => ("Sound files", a.packages.len()),
            ListMode::Champions => ("Champions", a.owner_list(mode).len()),
            ListMode::Items => ("Items", a.owner_list(mode).len()),
        };
        let mode_button = |id: &'static str, label: &'static str, m: ListMode, tip: &'static str, cx: &mut Context<Self>| {
            Button::new(id).xsmall().ghost().selected(mode == m).label(label).tooltip(tip).on_click(cx.listener(move |this, _, _, cx| this.set_audio_mode(m, cx)))
        };
        let modes = h_flex()
            .px_2()
            .pb_1()
            .gap_1()
            .child(mode_button("audio-files", "Files", ListMode::Files, "Every sound bank (.pck) of the game folder", cx))
            .child(div().relative().child(mode_button("audio-champions", "Champions", ListMode::Champions, "Each champion's voice lines, quick chat, abilities, weapon and skins", cx)).child(crate::driver::probe("audio-champions")))
            .child(mode_button("audio-items", "Items", ListMode::Items, "Sprays and other items that play sounds", cx));
        let list_body: AnyElement = if a.loading && mode == ListMode::Files {
            super::ui::loading("Listing sound banks…", &theme).into_any_element()
        } else if mode != ListMode::Files && a.owners.is_none() {
            super::ui::loading("Reading which sounds each champion and item plays…", &theme).into_any_element()
        } else {
            files.into_any_element()
        };
        let left = v_flex()
            .size_full()
            .child(super::ui::pane_header(title, div().text_xs().font_weight(gpui::FontWeight::NORMAL).child(count.to_string()), &theme))
            .child(modes)
            .child(super::ui::pane_filter(Input::new(&a.filter).small().cleanable(true)))
            .child(
                div()
                    .id("pck-list")
                    .track_focus(&banks_focus)
                    .tab_index(0)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(|s| s.border_color(theme.ring))
                    .flex_1()
                    .min_h_0()
                    .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| match e.keystroke.key.as_str() {
                        "up" => {
                            cx.stop_propagation();
                            this.step_audio_bank(-1, window, cx);
                        }
                        "down" => {
                            cx.stop_propagation();
                            this.step_audio_bank(1, window, cx);
                        }
                        "home" => {
                            cx.stop_propagation();
                            this.step_audio_bank(isize::MIN, window, cx);
                        }
                        "end" => {
                            cx.stop_propagation();
                            this.step_audio_bank(isize::MAX, window, cx);
                        }
                        "enter" | "space" if !e.keystroke.modifiers.modified() => {
                            window.prevent_default();
                            cx.stop_propagation();
                            if this.open_audio_path().is_none() {
                                this.step_audio_bank(0, window, cx);
                            }
                            layout::update(&this.panes, |p| {
                                p.overlay = false;
                                p.second_hidden = false;
                            });
                            if let Some(ef) = this.audio.as_ref().map(|a| a.events_focus.clone()) {
                                ef.focus(window, cx);
                            }
                            cx.notify();
                        }
                        _ => {}
                    }))
                    .child(list_body),
            );

        // 2. Events list (middle), a section per file of a champion's or item's set.
        let ef = a.events_filter.read(cx).value().to_ascii_lowercase();
        let mut events = v_flex();
        let mut all_rows: Vec<(String, u64, String)> = Vec::new();
        if let Some(info) = &a.info {
            all_rows = a.media(names).iter().enumerate().map(|(k, r)| (r.pck.clone(), r.id(), r.stem(k, names))).collect();
            let all_sel = a.event.is_none();
            let ef_all = events_focus.clone();
            let several = a.open_paths().len() > 1;
            events = events.child(
                h_flex()
                    .id("ev-all")
                    .focusable()
                    .min_h(row_h)
                    .py_0p5()
                    .px_2()
                    .gap_2()
                    .text_sm()
                    .border_1()
                    .border_color(if all_sel && events_focused { theme.ring } else { gpui::transparent_black() })
                    .focus_visible(|s| s.border_color(theme.ring))
                    .cursor_pointer()
                    .when(all_sel, |d| d.bg(theme.list_active))
                    .when(!all_sel, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(a) = &mut this.audio {
                            a.event = None;
                            a.selected_media = None;
                        }
                        ef_all.focus(window, cx);
                        cx.notify();
                    }))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(if several { "Every sound" } else { "Every sound file" }))
                    .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(bank_media(info).len().to_string())),
            );
            // A file's section label: its kind and skin in a set, from the owner's list.
            let owner_banks: Vec<J> = a.open.as_deref().and_then(|k| a.owner(k)).and_then(|o| o["banks"].as_array().cloned()).unwrap_or_default();
            let mut last_pck: Option<&str> = None;
            for (k, (b, e)) in a.shown_events(&ef, names).into_iter().enumerate() {
                let pck = b["pck"].as_str().unwrap_or("");
                if several && last_pck != Some(pck) {
                    last_pck = Some(pck);
                    let label = owner_banks.iter().find(|ob| ob["path"].as_str() == Some(pck)).map(sound_names::bank_label).unwrap_or_default();
                    let file = b["file"].as_str().unwrap_or("").to_string();
                    events = events.child(
                        h_flex()
                            .px_2()
                            .pt_2()
                            .pb_0p5()
                            .gap_2()
                            .text_xs()
                            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.foreground).child(if label.is_empty() { file.clone() } else { label }))
                            .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(theme.muted_foreground).child(file)),
                    );
                }
                let name = event_name(e, names);
                let part = if names { event_part(e) } else { None };
                let id = e["id"].as_u64().unwrap_or(0);
                let n = e["media"].as_array().map(|m| m.len()).unwrap_or(0);
                let active_ev = a.event == Some(id);
                let ef_row = events_focus.clone();
                events = events.child(
                    h_flex()
                        .id(("ev", k))
                        .focusable()
                        .min_h(row_h)
                        .py_0p5()
                        .px_2()
                        .gap_2()
                        .text_sm()
                        .border_1()
                        .border_color(if active_ev && events_focused { theme.ring } else { gpui::transparent_black() })
                        .focus_visible(|s| s.border_color(theme.ring))
                        .cursor_pointer()
                        .when(active_ev, |d| d.bg(theme.list_active))
                        .when(!active_ev, |d| d.hover(|s| s.bg(theme.list_hover)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(a) = &mut this.audio {
                                a.event = Some(id);
                                a.selected_media = None;
                            }
                            ef_row.focus(window, cx);
                            cx.notify();
                        }))
                        .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name))
                        .when_some(part, |d, p| d.child(div().flex_none().max_w(px(150.0)).overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().text_color(theme.muted_foreground).child(p)))
                        .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(if n == 0 { "no sound".to_string() } else { format!("{n}") })),
                );
            }
        }
        let open_path = a.open.clone();
        let open_err = a.open_error.clone();
        let has_info = a.info.is_some();
        let selected_event = a.event;
        let side_shown = self.panes.get().side();
        let events_body: AnyElement = if open_path.is_none() {
            let empty_copy = if side_shown {
                "Pick a sound file, champion or item on the left: its events (what the game triggers) and the sounds they play show here, to listen to, edit, export or replace.".to_string()
            } else {
                format!("Open the sound bank list ({}): its events (what the game triggers) and the sounds they play show here, to listen to, edit, export or replace.", super::commands::key_hint("⌘B"))
            };
            super::ui::empty(IconName::AudioLines, "No sound file open", empty_copy, &theme)
                .when(!side_shown, |d| {
                    d.child(
                        div()
                            .relative()
                            .child(
                                Button::new("browse-sound-banks")
                                    .small()
                                    .icon(Icon::new(IconName::PanelLeft))
                                    .label("Browse sound banks…")
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pane(false, cx))),
                            )
                            .child(crate::driver::probe("browse-sound-banks")),
                    )
                })
                .into_any_element()
        } else if let Some(err) = open_err {
            let retry = open_path.clone().unwrap_or_default();
            super::ui::error("Could not read sound bank", super::ui::explain_error(&err), &theme)
                .child(Button::new("audio-retry").small().label("Try again").on_click(cx.listener(move |this, _, _, cx| this.audio_open(retry.clone(), cx))))
                .into_any_element()
        } else if !has_info {
            super::ui::loading("Reading the sound files…", &theme).into_any_element()
        } else {
            events.into_any_element()
        };
        let middle = v_flex()
            .size_full()
            .child(super::ui::pane_header(a.title().filter(|_| has_info).map(|t| format!("Events · {t}")).unwrap_or_else(|| "Events".into()), div(), &theme))
            .child(super::ui::pane_filter(Input::new(&a.events_filter).small().cleanable(true)))
            .child(
                div()
                    .id("ev-list")
                    .track_focus(&events_focus)
                    .tab_index(0)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(|s| s.border_color(theme.ring))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| match e.keystroke.key.as_str() {
                        "up" => {
                            cx.stop_propagation();
                            this.step_audio_event(-1, window, cx);
                        }
                        "down" => {
                            cx.stop_propagation();
                            this.step_audio_event(1, window, cx);
                        }
                        "home" => {
                            cx.stop_propagation();
                            this.step_audio_event(isize::MIN, window, cx);
                        }
                        "end" => {
                            cx.stop_propagation();
                            this.step_audio_event(isize::MAX, window, cx);
                        }
                        _ => {}
                    }))
                    .child(events_body),
            );

        // 3. Sounds list with the editor above it and "Export all…" (right).
        let shown: Vec<(String, u64, String)> = a.media(names).iter().enumerate().map(|(k, r)| (r.pck.clone(), r.id(), r.stem(k, names))).collect();
        let shown_ids: Vec<u64> = shown.iter().map(|r| r.1).collect();
        let list: AnyElement = if has_info && shown_ids.is_empty() {
            div().p_2().text_sm().text_color(theme.muted_foreground).child("This event plays no sound file of these banks (a stop, a state change, or media of another bank).").into_any_element()
        } else {
            uniform_list("am-rows", shown_ids.len(), cx.processor(Self::render_audio_media)).track_scroll(&a.media_scroll).p_2().size_full().into_any_element()
        };

        let can_export_all = !all_rows.is_empty();
        let export_all_tip = if open_path.is_none() {
            if side_shown { "Pick a sound bank on the left first" } else { "Pick a sound bank first" }
        } else if all_rows.is_empty() {
            "This sound bank has no sounds to export"
        } else {
            "Export every sound to a folder (.wav, .ogg or .wem)"
        };
        let set_title = a.title().unwrap_or_default();
        let export_btn = {
            let me = cx.entity().downgrade();
            let every = all_rows.clone();
            let ev_rows = shown.clone();
            let has_event_filter = selected_event.is_some() && !ev_rows.is_empty();
            let set_title = set_title.clone();
            Button::new("audio-export-all")
                .xsmall()
                .icon(Icon::new(IconName::Download))
                .label("Export all…")
                .disabled(!can_export_all)
                .tooltip(export_all_tip)
                .dropdown_menu(move |mut menu, _, _| {
                    if has_event_filter {
                        for (label, ext) in [
                            (format!("Export event sounds as WAV (.wav)… ({})", ev_rows.len()), "wav"),
                            (format!("Export event sounds as Ogg Vorbis (.ogg)… ({})", ev_rows.len()), "ogg"),
                            (format!("Export event sounds as raw Wwise (.wem)… ({})", ev_rows.len()), "wem"),
                        ] {
                            let (me, rows, t) = (me.clone(), ev_rows.clone(), set_title.clone());
                            menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                let _ = me.update(cx, |this, cx| this.wwise_export_all(t.clone(), rows.clone(), ext, cx));
                            }));
                        }
                        menu = menu.separator();
                    }
                    for (label, ext) in [
                        (format!("Export every sound as WAV (.wav)… ({})", every.len()), "wav"),
                        (format!("Export every sound as Ogg Vorbis (.ogg)… ({})", every.len()), "ogg"),
                        (format!("Export every sound as raw Wwise (.wem)… ({})", every.len()), "wem"),
                    ] {
                        let (me, rows, t) = (me.clone(), every.clone(), set_title.clone());
                        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.wwise_export_all(t.clone(), rows.clone(), ext, cx));
                        }));
                    }
                    menu
                })
        };

        let sounds_toolbar = super::ui::toolbar(&theme)
            .gap_2()
            .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child("Sounds"))
            .when(has_info, |d| d.child(div().text_xs().text_color(theme.muted_foreground).child(format!("{}", shown_ids.len()))))
            .child(div().flex_1())
            .child(div().relative().flex_none().child(export_btn).child(crate::driver::probe("audio-export-all")));

        // The editor, on a sound of the files open.
        let open_paths = a.open_paths();
        let edited = self.ww_sound.as_ref().and_then(|s| s.wwise.clone()).filter(|(p, _)| open_paths.iter().any(|o| super::project::same_path(o, p)));
        let edited_label = edited.map(|(_, id)| a.media(names).iter().enumerate().find(|(_, r)| r.id() == id).map(|(k, r)| r.title(k, names)).unwrap_or_else(|| format!("Sound {id}")));
        let editor = edited_label.and_then(|label| {
            let body = self.render_sound_slot(super::sound::Slot::Wwise, cx)?;
            Some(
                v_flex()
                    .flex_none()
                    .gap_1()
                    .p_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(format!("Editing {label}")))
                            .child(Button::new("ww-edit-close").xsmall().ghost().icon(Icon::new(IconName::Close)).tooltip("Close the editor").on_click(cx.listener(|this, _, _, cx| {
                                this.player.stop();
                                this.ww_sound = None;
                                cx.notify();
                            }))),
                    )
                    .child(body),
            )
        });

        let key_rows = shown;
        let right = v_flex()
            .size_full()
            .child(sounds_toolbar)
            .children(editor)
            .child(
                div()
                    .id("am-list")
                    .track_focus(&media_focus)
                    .tab_index(0)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(|s| s.border_color(theme.ring))
                    .flex_1()
                    .min_h_0()
                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| match e.keystroke.key.as_str() {
                        "up" => {
                            cx.stop_propagation();
                            this.step_audio_media(-1, &shown_ids, window, cx);
                        }
                        "down" => {
                            cx.stop_propagation();
                            this.step_audio_media(1, &shown_ids, window, cx);
                        }
                        "home" => {
                            cx.stop_propagation();
                            this.step_audio_media(isize::MIN, &shown_ids, window, cx);
                        }
                        "end" => {
                            cx.stop_propagation();
                            this.step_audio_media(isize::MAX, &shown_ids, window, cx);
                        }
                        "enter" | "space" => {
                            let sel = this.audio.as_ref().and_then(|a| a.selected_media);
                            if let Some((pck, id, _)) = key_rows.iter().find(|r| Some(r.1) == sel).or_else(|| key_rows.first()).cloned() {
                                cx.stop_propagation();
                                this.wwise_play_in(pck, id, cx);
                            }
                        }
                        _ => {}
                    }))
                    .child(list),
            );
        let sizes = self.pane_sizes("audio", &[super::ui::SIDE_PANE, 340.0]);
        let me = cx.entity().downgrade();
        layout::group(
            "audio",
            self.panes.get(),
            vec![layout::Part(layout::Role::Side, Some(sizes[0]), left.into_any_element()), layout::Part(layout::Role::Second, Some(sizes[1]), middle.into_any_element()), layout::Part(layout::Role::Main, None, right.into_any_element())],
            Self::remember_panes(me.clone(), "audio"),
            move |_, cx| {
                let _ = me.update(cx, |s, cx| s.close_overlay(cx));
            },
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_keys() {
        assert_eq!(parse_owner_key("champion:2560"), Some(("champion", 2560)));
        assert_eq!(parse_owner_key("item:7"), Some(("item", 7)));
        assert_eq!(parse_owner_key("/games/x:y.pck"), None);
        assert_eq!(parse_owner_key("C:\\game\\WWB_VOX_Nyx.pck"), None);
    }

    #[test]
    fn media_titles() {
        let m = json!({ "id": 573979225u64 });
        let when = json!([{ "kind": "crossfade", "param": 699271803, "paramName": "IsFriendlyWithLocalPawn", "from": 0.75, "to": 1.0 }]);
        let row = MediaRow { m: &m, pck: String::new(), when: Some(&when), event: Some("Ultimate".into()) };
        assert_eq!(row.title(1, true), "Ultimate · heard by you and your team");
        assert_eq!(row.title(1, false), "Sound 2");
        assert_eq!(row.stem(1, true), "Ultimate _ heard by you and your team (573979225)");
        assert_eq!(row.stem(1, false), "573979225");
    }
}
