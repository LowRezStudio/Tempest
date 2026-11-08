//! The command palette (⌘K, ⌘P, ⌘⇧O, ⌘⇧F): one field, a mode by prefix like
//! the Electron Studio's: `>` commands, no prefix packages, `@` objects of
//! the open package, `#` objects of every package (`workspace.search`), `+`
//! properties to add. It is also the picker of every "choose an object"
//! question (`Pick`): a package to copy into, a material for a slot, a
//! texture for a parameter, searched over every package and filtered by
//! class. The lists are filtered here (capped), not by the widget, so a
//! 65k-export package or an asynchronous search stays fast.

use gpui::{div, prelude::*, px, Context, Entity, SharedString, Window};
use gpui_component::command::{Command, CommandItem, CommandState};
use gpui_component::{h_flex, ActiveTheme};
use serde_json::json;

use super::commands::key_hint;
use super::{Cmd, Place, Studio};

/// Rows shown at most (the widget lays every item out).
const SHOWN: usize = 200;

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
}

impl Pick {
    fn placeholder(&self) -> &'static str {
        match self {
            Pick::CopyTo => "The package to copy it into",
            Pick::ReplaceWith => "An object of the same class in another package",
            Pick::SlotMaterial(_) => "A material or material instance, e.g. mic_ or a champion's name",
            Pick::Texture(_) => "A texture of any package",
        }
    }

    /// The class the search keeps (`isA`), for object picks.
    fn is_a(&self, studio: &Studio) -> Option<String> {
        match self {
            Pick::CopyTo => None,
            Pick::ReplaceWith => studio.selection_class(),
            Pick::SlotMaterial(_) => Some("MaterialInterface".into()),
            Pick::Texture(_) => Some("Texture".into()),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Item {
    Command(Cmd),
    /// Index into `Studio::packages`.
    Package(usize),
    /// Export of the open package.
    Export(usize),
    /// A search result in any package.
    Hit { package: String, path: String, index: usize, object: String, class: String },
    /// A property the selected object's class declares and the object does not store.
    AddProperty { name: String, kind: String, ty: String },
}

pub struct Palette {
    pub state: Entity<CommandState>,
    pub items: Vec<Item>,
    /// Counts searches so a late answer to an older query is dropped.
    seq: u64,
    pub note: Option<SharedString>,
    /// `+`: what can be added to the selected object (`object.missing`, read once).
    missing: Option<Vec<(String, String, String)>>,
    pub pick: Option<Pick>,
}

impl Studio {
    pub(super) fn open_palette(&mut self, prefix: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette_with(prefix, None, window, cx);
    }

    pub(super) fn open_picker(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette_with("", Some(pick), window, cx);
    }

    fn open_palette_with(&mut self, prefix: &str, pick: Option<Pick>, window: &mut Window, cx: &mut Context<Self>) {
        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |s, cx| {
            s.set_query(prefix.to_string(), window, cx);
            s.focus(window, cx);
        });
        self.palette = Some(Palette { state, items: Vec::new(), seq: 0, note: None, missing: None, pick });
        self.palette_query(prefix.to_string(), cx);
    }

    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    /// `workspace.search` into the palette's items (dropped when a newer query ran).
    fn search(&mut self, query: String, is_a: Option<String>, cx: &mut Context<Self>) {
        let Some(p) = &mut self.palette else { return };
        p.items.clear();
        if query.len() < 2 {
            p.note = Some("Type two characters or more to search every package".into());
            return;
        }
        p.seq += 1;
        let seq = p.seq;
        let call = self.backend.call("workspace.search", json!({ "query": query, "limit": SHOWN, "isA": is_a }));
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
                        p.items = r.json["results"]
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
                        if r.json["truncated"].as_bool().unwrap_or(false) {
                            p.note = Some(format!("First {SHOWN} matches: type more to narrow it down").into());
                        } else if p.items.is_empty() {
                            p.note = Some("Nothing matches".into());
                        }
                    }
                    Err(e) => p.note = Some(e.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn palette_query(&mut self, q: String, cx: &mut Context<Self>) {
        let enabled: Vec<Cmd> = Cmd::ALL.iter().copied().filter(|c| c.enabled(self)).collect();
        let (recent, pinned) = (self.recent_packages(), self.pinned());
        let Some(p) = &mut self.palette else { return };
        p.note = None;
        let has = |text: &str, f: &str| f.is_empty() || text.to_ascii_lowercase().contains(f);
        if let Some(pick) = p.pick.clone() {
            if matches!(pick, Pick::CopyTo) {
                let f = q.trim().to_ascii_lowercase();
                let here = self.open.as_ref().map(|t| t.path.clone());
                p.items = (0..self.packages.len()).filter(|&i| Some(&self.packages[i].path) != here.as_ref() && !self.packages[i].script && has(&self.packages[i].name, &f)).take(SHOWN).map(Item::Package).collect();
            } else {
                let is_a = pick.is_a(self);
                self.search(q.trim().to_string(), is_a, cx);
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
                    let call = self.backend.call("object.missing", json!({ "path": t.path, "index": i }));
                    cx.spawn(async move |this, cx| {
                        let r = call.await;
                        let _ = this.update(cx, |this, cx| {
                            let Some(p) = &mut this.palette else { return };
                            p.missing = Some(r.map(|r| r.json.as_array().into_iter().flatten().map(|m| (m["name"].as_str().unwrap_or_default().to_string(), m["kind"].as_str().unwrap_or_default().to_string(), m["type"].as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default());
                            let q = p.state.read(cx).query(cx).to_string();
                            this.palette_query(if q.starts_with('+') { q } else { "+".into() }, cx);
                        });
                    })
                    .detach();
                }
            }
        } else if let Some(rest) = q.strip_prefix('>') {
            let f = rest.trim().to_ascii_lowercase();
            p.items = enabled.into_iter().filter(|c| has(c.info().title, &f) || has(c.info().group, &f)).map(Item::Command).collect();
        } else if let Some(rest) = q.strip_prefix('@') {
            let f = rest.trim().to_ascii_lowercase();
            p.items = match &self.open {
                Some(t) => (0..t.exports.len()).filter(|&i| has(&t.exports[i].name, &f)).take(SHOWN).map(Item::Export).collect(),
                None => {
                    p.note = Some("No package is open".into());
                    Vec::new()
                }
            };
        } else if let Some(rest) = q.strip_prefix('#') {
            self.search(rest.trim().to_string(), None, cx);
        } else {
            let f = q.trim().to_ascii_lowercase();
            // Opened lately first (with no query, only those and the pinned ones lead).
            let rank = |path: &str| recent.iter().position(|r| r == path).or_else(|| pinned.iter().position(|r| r == path).map(|k| 100 + k)).unwrap_or(usize::MAX);
            let mut all: Vec<usize> = (0..self.packages.len()).filter(|&i| has(&self.packages[i].name, &f)).collect();
            all.sort_by_key(|&i| rank(&self.packages[i].path));
            p.items = all.into_iter().take(SHOWN).map(Item::Package).collect();
            if p.items.is_empty() && !f.is_empty() {
                p.note = Some("No package by that name: start with # to search objects of every package, > for commands".into());
            }
        }
        cx.notify();
    }

    pub(super) fn palette_confirm(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((item, pick)) = self.palette.as_ref().and_then(|p| p.items.get(row).cloned().map(|i| (i, p.pick.clone()))) else { return };
        self.close_palette(window, cx);
        if let Some(pick) = pick {
            self.picked(pick, item, window, cx);
            return;
        }
        match item {
            Item::Command(c) => self.run(c, window, cx),
            Item::Package(i) => {
                let path = self.packages[i].path.clone();
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
            Item::AddProperty { name, .. } => self.add_property(name, cx),
        }
    }

    /// The picker's answer.
    fn picked(&mut self, pick: Pick, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        match (pick, item) {
            (Pick::CopyTo, Item::Package(i)) => {
                let dst = self.packages[i].path.clone();
                self.copy_to(dst, cx);
            }
            (Pick::ReplaceWith, Item::Hit { path, index, .. }) => self.replace_with(path, index, window, cx),
            (Pick::SlotMaterial(slot), Item::Hit { path, index, .. }) => self.set_slot_material(slot, path, index, cx),
            (Pick::Texture(name), Item::Hit { path, index, .. }) => self.set_texture(name, path, index, cx),
            _ => {}
        }
    }

    pub(super) fn render_palette(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let theme = cx.theme().clone();
        let p = self.palette.as_ref()?;
        let open = self.open.as_ref();
        let items: Vec<CommandItem> = p
            .items
            .iter()
            .map(|it| {
                let (label, detail): (String, String) = match it {
                    Item::Command(c) => {
                        let info = c.info();
                        let keys = if info.keys.is_empty() { String::new() } else { format!("  {}", key_hint(info.keys)) };
                        (format!("{}: {}", info.group, info.title), format!("{}{keys}", info.detail))
                    }
                    Item::Package(i) => (self.packages[*i].name.clone(), crate::model::human_size(self.packages[*i].size)),
                    Item::Export(i) => open.map(|t| (t.exports[*i].name.clone(), t.exports[*i].class.clone())).unwrap_or_default(),
                    Item::Hit { package, object, class, .. } => (object.clone(), format!("{class} · {package}")),
                    Item::AddProperty { name, kind, ty } => (name.clone(), if ty.is_empty() { kind.clone() } else { format!("{kind} {ty}") }),
                };
                let muted = theme.muted_foreground;
                CommandItem::new().label(label.clone()).child(move |_, _| h_flex().w_full().gap_2().child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(label.clone())).child(div().text_xs().text_color(muted).whitespace_nowrap().overflow_hidden().text_ellipsis().max_w(px(320.0)).child(detail.clone())))
            })
            .collect();
        let me = cx.entity();
        let (on_query, on_confirm, on_cancel) = (me.clone(), me.clone(), me);
        let note = p.note.clone();
        let placeholder = p.pick.as_ref().map(|k| k.placeholder()).unwrap_or("Package · > commands · @ objects of this package · # search every package");
        let palette = Command::new(&p.state)
            .filterable(false)
            .items(items)
            .placeholder(placeholder)
            .max_h(px(440.0))
            .on_query(move |q, _, cx| on_query.update(cx, |s, cx| s.palette_query(q.to_string(), cx)))
            .on_confirm(move |ix, window, cx| on_confirm.update(cx, |s, cx| s.palette_confirm(ix.row, window, cx)))
            .on_cancel(move |window, cx| on_cancel.update(cx, |s, cx| s.close_palette(window, cx)))
            .when_some(note, |c, n| c.footer(move |_, _, _| div().px_2().py_1().text_xs().child(n.clone()).into_any_element()));
        Some(
            div()
                .id("palette-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .items_start()
                .justify_center()
                .bg(gpui::black().opacity(0.35))
                .pt(px(80.0))
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, window, cx| this.close_palette(window, cx)))
                .child(div().id("palette").w(px(680.0)).h_auto().bg(theme.popover).rounded_lg().shadow_lg().on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(palette)),
        )
    }
}
