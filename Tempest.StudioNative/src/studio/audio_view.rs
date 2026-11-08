//! The Audio view (⌘4): every Wwise `.pck` of the game folder. Pick a file,
//! then an event (named from the packages' AkEvent exports) to see the
//! media it plays, or browse every medium; each one plays, exports and is
//! replaced from a file (an unsaved edit of the `.pck`, saved like a package).

use gpui::{div, prelude::*, px, AnyElement, Context, Entity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use super::layout;
use gpui_component::{h_flex, v_flex, ActiveTheme, Sizable};
use serde_json::{json, Value as J};

use super::{file_name, Studio};

pub struct AudioBrowser {
    packages: Vec<(String, String, u64)>,
    filter: Entity<InputState>,
    events_filter: Entity<InputState>,
    open: Option<String>,
    info: Option<J>,
    /// The event whose media are shown (None: every medium).
    event: Option<u64>,
    loading: bool,
}

impl AudioBrowser {
    /// Dev: the path of the sound file named `name`.
    pub fn package_path(&self, name: &str) -> Option<String> {
        self.packages.iter().find(|p| p.0.eq_ignore_ascii_case(name)).map(|p| p.1.clone())
    }
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
        self.audio = Some(AudioBrowser { packages: Vec::new(), filter, events_filter, open: None, info: None, event: None, loading: true });
        let call = self.backend.call("wwise.packages", json!({}));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(a) = &mut this.audio {
                    a.loading = false;
                    a.packages = r.map(|r| r.json["packages"].as_array().into_iter().flatten().map(|p| (p["name"].as_str().unwrap_or("").to_string(), p["path"].as_str().unwrap_or("").to_string(), p["size"].as_u64().unwrap_or(0))).collect()).unwrap_or_default();
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn audio_open(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(a) = &mut self.audio else { return };
        a.open = Some(path.clone());
        a.info = None;
        a.event = None;
        let call = self.backend.call("wwise.open", json!({ "path": path }));
        self.set_status(format!("Reading {}… (the first time, every package is indexed for event names)", file_name(&path)), cx);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        if let Some(a) = this.audio.as_mut().filter(|a| a.open.as_deref() == Some(path.as_str())) {
                            a.info = Some(r.json);
                        }
                        this.set_status(file_name(&path), cx);
                    }
                    Err(e) => this.set_status(format!("{}: {e}", file_name(&path)), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_audio_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.audio_browser(window, cx);
        let theme = cx.theme().clone();
        let a = self.audio.as_ref().expect("made above");
        let title = |t: &str| div().h(px(28.0)).px_2().flex().items_center().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(t.to_uppercase());
        // Files.
        let f = a.filter.read(cx).value().to_ascii_lowercase();
        let mut files = v_flex();
        for (k, (name, path, size)) in a.packages.iter().filter(|p| f.is_empty() || p.0.to_ascii_lowercase().contains(&f)).take(800).enumerate() {
            let sel = a.open.as_deref() == Some(path.as_str());
            let p = path.clone();
            files = files.child(
                h_flex()
                    .id(("pck", k))
                    .h(px(22.0))
                    .px_2()
                    .gap_2()
                    .text_sm()
                    .cursor_pointer()
                    .when(sel, |d| d.bg(theme.list_active))
                    .hover(|s| s.bg(theme.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.audio_open(p.clone(), cx)))
                    .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name.clone()))
                    .when(self.dirty.contains(path), |d| d.child(div().size(px(6.0)).rounded_full().bg(theme.warning)))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(crate::model::human_size(*size))),
            );
        }
        let left = v_flex()
            .size_full()
            .child(title(&format!("Sound files ({})", a.packages.len())))
            .child(div().px_1().pb_1().child(Input::new(&a.filter).small()))
            .child(div().id("pck-list").flex_1().min_h_0().overflow_y_scroll().child(if a.loading { div().p_2().text_sm().text_color(theme.muted_foreground).child("Listing…").into_any_element() } else { files.into_any_element() }));
        // Events.
        let ef = a.events_filter.read(cx).value().to_ascii_lowercase();
        let mut events = v_flex();
        let mut media: Vec<J> = Vec::new();
        if let Some(info) = &a.info {
            let all = |b: &J| b["media"].as_array().cloned().unwrap_or_default();
            let mut k = 0usize;
            events = events.child(
                div()
                    .id("ev-all")
                    .px_2()
                    .py_0p5()
                    .text_sm()
                    .cursor_pointer()
                    .when(a.event.is_none(), |d| d.bg(theme.list_active))
                    .hover(|s| s.bg(theme.list_hover))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(a) = &mut this.audio {
                            a.event = None;
                        }
                        cx.notify();
                    }))
                    .child("Every sound file"),
            );
            for b in info["banks"].as_array().into_iter().flatten() {
                for e in b["events"].as_array().into_iter().flatten() {
                    let name = e["name"].as_str().map(str::to_string).unwrap_or_else(|| format!("event {}", e["id"]));
                    if !ef.is_empty() && !name.to_ascii_lowercase().contains(&ef) {
                        continue;
                    }
                    let id = e["id"].as_u64().unwrap_or(0);
                    let n = e["media"].as_array().map(|m| m.len()).unwrap_or(0);
                    k += 1;
                    events = events.child(
                        h_flex()
                            .id(("ev", k))
                            .px_2()
                            .py_0p5()
                            .gap_2()
                            .text_sm()
                            .cursor_pointer()
                            .when(a.event == Some(id), |d| d.bg(theme.list_active))
                            .hover(|s| s.bg(theme.list_hover))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(a) = &mut this.audio {
                                    a.event = Some(id);
                                }
                                cx.notify();
                            }))
                            .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(if n == 0 { "no sound".to_string() } else { format!("{n}") })),
                    );
                    if a.event == Some(id) {
                        let ids: Vec<u64> = e["media"].as_array().into_iter().flatten().filter_map(|m| m.as_u64()).collect();
                        let pool: Vec<J> = info["banks"].as_array().into_iter().flatten().flat_map(all).chain(info["streams"].as_array().cloned().unwrap_or_default()).collect();
                        media = ids.iter().filter_map(|i| pool.iter().find(|m| m["id"].as_u64() == Some(*i)).cloned()).collect();
                    }
                }
            }
            if a.event.is_none() {
                media = info["banks"].as_array().into_iter().flatten().flat_map(all).chain(info["streams"].as_array().cloned().unwrap_or_default()).collect();
            }
        }
        let middle = v_flex()
            .size_full()
            .child(title("Events"))
            .child(div().px_1().pb_1().child(Input::new(&a.events_filter).small()))
            .child(div().id("ev-list").flex_1().min_h_0().overflow_y_scroll().child(if a.open.is_none() { div().p_2().text_sm().text_color(theme.muted_foreground).child("Pick a sound file on the left").into_any_element() } else if a.info.is_none() { div().p_2().text_sm().text_color(theme.muted_foreground).child("Reading…").into_any_element() } else { events.into_any_element() }));
        // Media.
        let pck = a.open.clone().unwrap_or_default();
        let playing = self.player.playing();
        let mut list = v_flex().gap_1().p_2();
        if a.info.is_some() && media.is_empty() {
            list = list.child(div().text_sm().text_color(theme.muted_foreground).child("This event plays no sound file of this bank (a stop, a state change, or media of another bank)."));
        }
        for (k, m) in media.into_iter().take(500).enumerate() {
            let id = m["id"].as_u64().unwrap_or(0);
            let now = playing && self.ww_playing == Some(id);
            let (p1, p2, p3) = (pck.clone(), pck.clone(), pck.clone());
            list = list.child(
                h_flex()
                    .gap_2()
                    .p_1()
                    .rounded_md()
                    .hover(|s| s.bg(theme.list_hover))
                    .child(Button::new(("am-play", k)).xsmall().when(now, |b| b.primary()).label(if now { "Stop" } else { "Play" }).on_click(cx.listener(move |this, _, _, cx| this.wwise_play_in(p1.clone(), id, cx))))
                    .child(v_flex().flex_1().min_w_0().child(div().text_sm().font_family("Menlo").child(id.to_string())).child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                        "{:.2} s · {} ch · {} Hz · {}{}",
                        m["duration"].as_f64().unwrap_or(0.0),
                        m["channels"],
                        m["sampleRate"],
                        crate::model::human_size(m["bytes"].as_u64().unwrap_or(0)),
                        if m["streamed"].as_bool().unwrap_or(false) { " · streamed" } else { "" }
                    ))))
                    .child(Button::new(("am-exp", k)).xsmall().ghost().label("Export…").on_click(cx.listener(move |this, _, _, cx| this.wwise_export_in(p2.clone(), id, cx))))
                    .child(Button::new(("am-rep", k)).xsmall().ghost().label("Replace…").tooltip("An .ogg, .wav or .wem file, re-encoded to Wwise Vorbis").on_click(cx.listener(move |this, _, _, cx| this.wwise_replace_in(p3.clone(), id, cx)))),
            );
        }
        let right = v_flex().size_full().child(title("Sounds")).child(div().id("am-list").flex_1().min_h_0().overflow_y_scroll().child(list));
        let sizes = self.pane_sizes("audio", &[280.0, 340.0]);
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
