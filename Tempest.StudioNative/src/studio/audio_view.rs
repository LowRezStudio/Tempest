//! The Audio view (⌘4): every Wwise `.pck` of the game folder. Pick a file,
//! then an event (named from the packages' AkEvent exports) to see the
//! media it plays, or browse every medium; each one plays, exports (singly or
//! all at once) and is replaced from a file (an unsaved edit of the `.pck`,
//! saved like a package).

use std::ops::Range;

use gpui::{div, prelude::*, px, uniform_list, AnyElement, Context, Entity, FocusHandle, KeyDownEvent, ScrollStrategy, UniformListScrollHandle, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Selectable, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{file_name, layout, Studio, Tone};

pub struct AudioBrowser {
    /// Name, path, size and subfolder (`English(US)` for voice lines) of each `.pck`.
    packages: Vec<(String, String, u64, Option<String>)>,
    filter: Entity<InputState>,
    events_filter: Entity<InputState>,
    banks_focus: FocusHandle,
    events_focus: FocusHandle,
    media_focus: FocusHandle,
    open: Option<String>,
    open_error: Option<String>,
    info: Option<J>,
    /// The event whose media are shown (None: every medium).
    event: Option<u64>,
    selected_media: Option<u64>,
    loading: bool,
    banks_scroll: UniformListScrollHandle,
    media_scroll: UniformListScrollHandle,
}

impl AudioBrowser {
    /// The sounds listed: the picked event's, or every one of the bank.
    fn media(&self) -> Vec<&J> {
        let Some(info) = &self.info else { return Vec::new() };
        let pool = bank_media(info);
        let Some(ev) = self.event else { return pool };
        let by_id: std::collections::HashMap<u64, &J> = pool.into_iter().filter_map(|m| Some((m["id"].as_u64()?, m))).collect();
        info["banks"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|b| b["events"].as_array().into_iter().flatten())
            .find(|e| e["id"].as_u64() == Some(ev))
            .map(|e| e["media"].as_array().into_iter().flatten().filter_map(|i| by_id.get(&i.as_u64()?).copied()).collect())
            .unwrap_or_default()
    }

    /// Indices into `packages` of the sound files the filter lets through.
    fn shown_banks(&self, cx: &gpui::App) -> Vec<usize> {
        let f = self.filter.read(cx).value().to_ascii_lowercase();
        (0..self.packages.len()).filter(|&i| f.is_empty() || self.packages[i].0.to_ascii_lowercase().contains(&f)).collect()
    }

    /// Dev: the path of the sound file named `name`.
    pub fn package_path(&self, name: &str) -> Option<String> {
        self.packages
            .iter()
            .find(|p| p.1 == name || p.0.eq_ignore_ascii_case(name) || file_name(&p.1).eq_ignore_ascii_case(name))
            .map(|p| p.1.clone())
    }

    pub(super) fn clear_open(&mut self) {
        self.open = None;
        self.open_error = None;
        self.info = None;
        self.event = None;
        self.selected_media = None;
    }
}

impl AudioBrowser {
    /// The events filter once a sound file is open, else the files filter (⌘F).
    pub(super) fn filter_input(&self) -> Entity<InputState> {
        if self.open.is_some() { self.events_filter.clone() } else { self.filter.clone() }
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
            filter,
            events_filter,
            banks_focus: cx.focus_handle(),
            events_focus: cx.focus_handle(),
            media_focus: cx.focus_handle(),
            open: None,
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

    pub(super) fn open_audio_path(&self) -> Option<&str> {
        self.audio.as_ref()?.open.as_deref()
    }

    /// Re-read the open `.pck` metadata after a replacement, undo, or redo without resetting event/media selection.
    pub(super) fn refresh_audio_open(&mut self, pck: &str, cx: &mut Context<Self>) {
        let Some(open_path) = self.audio.as_ref().and_then(|a| a.open.clone()).filter(|o| super::project::same_path(o, pck)) else { return };
        let call = self.backend.call("wwise.open", json!({ "path": open_path }));
        cx.spawn(async move |this, cx| {
            if let Ok(r) = call.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(a) = this.audio.as_mut().filter(|a| a.open.as_deref() == Some(open_path.as_str())) {
                        a.info = Some(r.json);
                        a.open_error = None;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn audio_open(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(a) = &mut self.audio else { return };
        a.open = Some(path.clone());
        a.open_error = None;
        a.info = None;
        a.event = None;
        a.selected_media = None;
        let call = self.backend.call("wwise.open", json!({ "path": path }));
        self.set_status(format!("Reading {}… (the first time, every package is indexed for event names)", file_name(&path)), cx);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        if let Some(a) = this.audio.as_mut().filter(|a| a.open.as_deref() == Some(path.as_str())) {
                            a.info = Some(r.json);
                            a.open_error = None;
                        }
                        this.set_status(file_name(&path), cx);
                    }
                    Err(e) => {
                        if let Some(a) = this.audio.as_mut().filter(|a| a.open.as_deref() == Some(path.as_str())) {
                            a.open_error = Some(e.clone());
                        }
                        this.toast(Tone::Error, super::ui::explain_error(&format!("Could not open {}: {e}", file_name(&path))), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn user_audio_open(&mut self, path: String, close_drawer: bool, cx: &mut Context<Self>) {
        if close_drawer {
            layout::update(&self.panes, |p| p.overlay = false);
        }
        self.record(super::Place::Audio(Some(path.clone())));
        self.audio_open(path, cx);
    }

    fn step_audio_bank(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(a) = &self.audio else { return };
        let shown: Vec<String> = a.shown_banks(cx).into_iter().map(|i| a.packages[i].1.clone()).collect();
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
        let Some(a) = &mut self.audio else { return };
        let Some(info) = &a.info else { return };
        let ef = a.events_filter.read(cx).value().to_ascii_lowercase();
        let mut choices: Vec<Option<u64>> = vec![None];
        for b in info["banks"].as_array().into_iter().flatten() {
            for e in b["events"].as_array().into_iter().flatten() {
                let name = e["name"].as_str().map(str::to_string).unwrap_or_else(|| format!("event {}", e["id"]));
                if ef.is_empty() || name.to_ascii_lowercase().contains(&ef) {
                    choices.push(Some(e["id"].as_u64().unwrap_or(0)));
                }
            }
        }
        if choices.is_empty() {
            return;
        }
        let at = choices.iter().position(|c| *c == a.event);
        let next = match (at, by) {
            (_, isize::MIN) => 0,
            (_, isize::MAX) => choices.len() - 1,
            (Some(i), d) => (i as isize + d).clamp(0, choices.len() as isize - 1) as usize,
            (None, _) => 0,
        };
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
        let shown = a.shown_banks(cx);
        range
            .filter_map(|k| shown.get(k).map(|&i| (k, &a.packages[i])))
            .map(|(k, (name, path, size, folder))| {
                let sel = a.open.as_deref() == Some(path.as_str());
                let is_dirty = self.dirty.contains(path);
                let p = path.clone();
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
                        this.user_audio_open(p.clone(), true, cx);
                        bf.focus(window, cx);
                    }))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name.clone()))
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
            .collect()
    }

    fn render_audio_media(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let row_h = super::ui::row_h(window);
        let Some(a) = self.audio.as_ref() else { return Vec::new() };
        let media_focus = a.media_focus.clone();
        let media_focused = media_focus.is_focused(window) || media_focus.contains_focused(window, cx);
        let pck = a.open.clone().unwrap_or_default();
        let playing = self.player.playing();
        let media = a.media();
        range
            .filter_map(|k| media.get(k).map(|m| (k, *m)))
            .map(|(k, m)| {
                let id = m["id"].as_u64().unwrap_or(0);
                let now = playing && self.ww_playing == Some(id);
                let sel_m = a.selected_media == Some(id);
                let (p1, p2, p3) = (pck.clone(), pck.clone(), pck.clone());
                let mf = media_focus.clone();
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
                        // Sounds have no names, only ids: their length tells them apart.
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().text_sm().child(format!("Sound {} · {:.2} s", k + 1, m["duration"].as_f64().unwrap_or(0.0))))
                                .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().text_color(theme.muted_foreground).child(format!(
                                    "id {} · {} ch · {} Hz · {}{}",
                                    id,
                                    m["channels"],
                                    m["sampleRate"],
                                    crate::model::human_size(m["bytes"].as_u64().unwrap_or(0)),
                                    if m["streamed"].as_bool().unwrap_or(false) { " · streamed" } else { "" }
                                ))),
                        )
                        .child(Button::new(("am-exp", k)).xsmall().ghost().label("Export…").on_click(cx.listener(move |this, _, _, cx| this.wwise_export_in(p2.clone(), id, cx))))
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
        let a = self.audio.as_ref().expect("made above");
        let banks_focus = a.banks_focus.clone();
        let events_focus = a.events_focus.clone();
        let media_focus = a.media_focus.clone();
        let events_focused = events_focus.is_focused(window) || events_focus.contains_focused(window, cx);

        // 1. Sound banks list (left), virtual: a game has over a thousand.
        let banks_count = a.shown_banks(cx).len();
        let files = uniform_list("pck-rows", banks_count, cx.processor(Self::render_audio_banks)).track_scroll(&a.banks_scroll).size_full();
        let left = v_flex()
            .size_full()
            .child(super::ui::pane_header("Sound files", div().text_xs().font_weight(gpui::FontWeight::NORMAL).child(a.packages.len().to_string()), &theme))
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
                    .child(if a.loading {
                        super::ui::loading("Listing sound banks…", &theme).into_any_element()
                    } else {
                        files.into_any_element()
                    }),
            );

        // 2. Events list (middle).
        let ef = a.events_filter.read(cx).value().to_ascii_lowercase();
        let mut events = v_flex();
        let mut all_bank_ids: Vec<u64> = Vec::new();
        if let Some(info) = &a.info {
            let pool = bank_media(info);
            all_bank_ids = pool.iter().filter_map(|m| m["id"].as_u64()).collect();
            let mut k = 0usize;
            let all_sel = a.event.is_none();
            let ef_all = events_focus.clone();
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
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child("Every sound file"))
                    .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(pool.len().to_string())),
            );
            for b in info["banks"].as_array().into_iter().flatten() {
                for e in b["events"].as_array().into_iter().flatten() {
                    let name = e["name"].as_str().map(str::to_string).unwrap_or_else(|| format!("event {}", e["id"]));
                    if !ef.is_empty() && !name.to_ascii_lowercase().contains(&ef) {
                        continue;
                    }
                    let id = e["id"].as_u64().unwrap_or(0);
                    let n = e["media"].as_array().map(|m| m.len()).unwrap_or(0);
                    let active_ev = a.event == Some(id);
                    let ef_row = events_focus.clone();
                    k += 1;
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
                            .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(if n == 0 { "no sound".to_string() } else { format!("{n}") })),
                    );
                }
            }
        }
        let open_path = a.open.clone();
        let open_err = a.open_error.clone();
        let has_info = a.info.is_some();
        let selected_event = a.event;
        let side_shown = self.panes.get().side();
        let events_body: AnyElement = if open_path.is_none() {
            let empty_copy = if side_shown {
                "Pick a sound file on the left: its events (what the game triggers) and the sounds they play show here, to listen to, export or replace.".to_string()
            } else {
                format!("Open the sound bank list ({}): its events (what the game triggers) and the sounds they play show here, to listen to, export or replace.", super::commands::key_hint("⌘B"))
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
            let retry_path = open_path.clone().unwrap_or_default();
            super::ui::error("Could not read sound bank", super::ui::explain_error(&err), &theme)
                .child(Button::new("audio-retry").small().label("Try again").on_click(cx.listener(move |this, _, _, cx| this.audio_open(retry_path.clone(), cx))))
                .into_any_element()
        } else if !has_info {
            super::ui::loading("Reading the sound file…", &theme).into_any_element()
        } else {
            events.into_any_element()
        };
        let middle = v_flex()
            .size_full()
            .child(super::ui::pane_header("Events", div(), &theme))
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

        // 3. Sounds list + toolbar with batch "Export all…" (right).
        let pck = open_path.unwrap_or_default();
        let shown_ids: Vec<u64> = a.media().iter().filter_map(|m| m["id"].as_u64()).collect();
        let list: AnyElement = if has_info && shown_ids.is_empty() {
            div().p_2().text_sm().text_color(theme.muted_foreground).child("This event plays no sound file of this bank (a stop, a state change, or media of another bank).").into_any_element()
        } else {
            uniform_list("am-rows", shown_ids.len(), cx.processor(Self::render_audio_media)).track_scroll(&a.media_scroll).p_2().size_full().into_any_element()
        };

        let can_export_all = !pck.is_empty() && !all_bank_ids.is_empty();
        let export_all_tip = if pck.is_empty() {
            if side_shown { "Pick a sound bank on the left first" } else { "Pick a sound bank first" }
        } else if all_bank_ids.is_empty() {
            "This sound bank has no sounds to export"
        } else {
            "Export every sound of this bank to a folder (.wav, .ogg or .wem)"
        };
        let export_btn = {
            let me = cx.entity().downgrade();
            let pck_menu = pck.clone();
            let bank_ids = all_bank_ids.clone();
            let ev_ids = shown_ids.clone();
            let has_event_filter = selected_event.is_some() && !ev_ids.is_empty();
            Button::new("audio-export-all")
                .xsmall()
                .icon(Icon::new(IconName::Download))
                .label("Export all…")
                .disabled(!can_export_all)
                .tooltip(export_all_tip)
                .dropdown_menu(move |mut menu, _, _| {
                    if has_event_filter {
                        for (label, ext) in [
                            (format!("Export event sounds as WAV (.wav)… ({})", ev_ids.len()), "wav"),
                            (format!("Export event sounds as Ogg Vorbis (.ogg)… ({})", ev_ids.len()), "ogg"),
                            (format!("Export event sounds as raw Wwise (.wem)… ({})", ev_ids.len()), "wem"),
                        ] {
                            let (me, pck, ids) = (me.clone(), pck_menu.clone(), ev_ids.clone());
                            menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                let _ = me.update(cx, |this, cx| this.wwise_export_all(pck.clone(), ids.clone(), ext, cx));
                            }));
                        }
                        menu = menu.separator();
                    }
                    for (label, ext) in [
                        (format!("Export entire bank as WAV (.wav)… ({})", bank_ids.len()), "wav"),
                        (format!("Export entire bank as Ogg Vorbis (.ogg)… ({})", bank_ids.len()), "ogg"),
                        (format!("Export entire bank as raw Wwise (.wem)… ({})", bank_ids.len()), "wem"),
                    ] {
                        let (me, pck, ids) = (me.clone(), pck_menu.clone(), bank_ids.clone());
                        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.wwise_export_all(pck.clone(), ids.clone(), ext, cx));
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

        let key_ids = shown_ids;
        let key_pck = pck;
        let right = v_flex()
            .size_full()
            .child(sounds_toolbar)
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
                            this.step_audio_media(-1, &key_ids, window, cx);
                        }
                        "down" => {
                            cx.stop_propagation();
                            this.step_audio_media(1, &key_ids, window, cx);
                        }
                        "home" => {
                            cx.stop_propagation();
                            this.step_audio_media(isize::MIN, &key_ids, window, cx);
                        }
                        "end" => {
                            cx.stop_propagation();
                            this.step_audio_media(isize::MAX, &key_ids, window, cx);
                        }
                        "enter" | "space" => {
                            if let Some(id) = this.audio.as_ref().and_then(|a| a.selected_media).or_else(|| key_ids.first().copied()) {
                                if !key_pck.is_empty() {
                                    cx.stop_propagation();
                                    this.wwise_play_in(key_pck.clone(), id, cx);
                                }
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
