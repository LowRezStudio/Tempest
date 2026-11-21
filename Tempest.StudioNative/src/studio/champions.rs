//! The Champions view: the game's champions by their names in game, and for
//! one champion everything a mod changes about them (skins, abilities, cards
//! and talents, their game files), each a click away from where it is edited.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{div, img, prelude::*, px, AnyElement, Context, Entity, FocusHandle, Image, ImageFormat, KeyDownEvent, ObjectFit, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{layout, ui, Place, Studio, Tone};

/// A champion of the champion vendor.
#[derive(Clone)]
pub struct Champion {
    pub id: u64,
    pub name: String,
    /// The row's reference name (`BombKing`): its packages are `BOMBKING_*`.
    pub reference: String,
}

pub struct Champions {
    list: Vec<Champion>,
    loading: bool,
    error: Option<String>,
    filter: Entity<InputState>,
    list_focus: FocusHandle,
    selected: Option<u64>,
    /// The selected champion's devices and skin collections, once read.
    devices: Option<Vec<J>>,
    skins: Option<Vec<J>>,
    /// Lists shown whole ("skins", "devices"); the others stop at `SHORT`.
    whole: Vec<&'static str>,
    /// Icons of the selected champion's skins, by collection item; the default skin's is the portrait.
    icons: HashMap<u64, Arc<Image>>,
    default_skin: Option<u64>,
    /// The skin unfolded, and each skin's own files once read (or why they could not be).
    open_skin: Option<u64>,
    skin_files: HashMap<u64, Result<Vec<SkinFile>, String>>,
}

/// An object a skin has of its own (its skin referencer's, less the champion's base ones).
#[derive(Clone)]
pub struct SkinFile {
    pub package: String,
    pub index: usize,
    pub path: String,
    pub class: String,
}

/// The classes a skin's look is made of, in the order they are listed (textures and materials first for reskinning).
const LOOK_CLASSES: [&str; 7] = ["texture2d", "materialinstanceconstant", "material", "materialinstancetimevarying", "skeletalmesh", "staticmesh", "particlesystem"];

/// Rows a list shows before its Show all.
const SHORT: usize = 8;

/// Non-champion vendor entries (such as the shared mount) sit in an "Other" section at the bottom.
fn is_other_entry(ch: &Champion) -> bool {
    let r = ch.reference.trim();
    let n = ch.name.trim();
    r.eq_ignore_ascii_case("horse") || r.eq_ignore_ascii_case("mount") || n.eq_ignore_ascii_case("horse")
}

impl Champions {
    pub(super) fn all(&self) -> &[Champion] {
        &self.list
    }

    pub(super) fn selected_id(&self) -> Option<u64> {
        self.selected
    }

    pub(super) fn clear_selection(&mut self) {
        self.selected = None;
        self.whole.clear();
        self.devices = None;
        self.skins = None;
        self.icons.clear();
        self.default_skin = None;
        self.open_skin = None;
        self.skin_files.clear();
    }

    pub(super) fn filter_input(&self) -> Entity<InputState> {
        self.filter.clone()
    }

    /// Champions by package prefix (`BOMBKING` → Bomb King), to name packages.
    pub(super) fn by_prefix(&self) -> impl Iterator<Item = (String, &Champion)> {
        self.list.iter().map(|c| (prefix(&c.reference), c))
    }

    /// Plain-English `"Champion · Role"` label for a champion's package.
    pub(super) fn package_label(&self, pkg_name: &str) -> Option<String> {
        self.by_prefix().find_map(|(pre, ch)| package_role(pkg_name, &pre).map(|role| format!("{} · {role}", ch.name)))
    }
}

/// `Bomb King`/`BombKing` → `BOMBKING`: how packages start.
pub fn prefix(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase()
}

/// What a champion's package holds, from its name (`CASSIE_LOBBY_ASSETS_SF`).
pub fn package_role(package: &str, prefix: &str) -> Option<&'static str> {
    let up = package.to_ascii_uppercase();
    let rest = up.strip_prefix(&prefix.to_ascii_uppercase())?.strip_prefix('_')?;
    Some(match rest {
        "ASSETS_SF" => "In-match models, textures, effects and sounds",
        "LOBBY_ASSETS_SF" => "Lobby and menu models",
        "UI_ASSETS_SF" => "Interface: icons and portraits",
        "TRANS_ASSETS_SF" => "Transition assets",
        _ => "Game files",
    })
}

/// Cold-start `"Champion · Role"` label derived from a champion-style package
/// name (`CASSIE_ASSETS_SF`) before `mcts.choices` has finished warming up.
pub(super) fn fallback_package_label(pkg_name: &str) -> Option<String> {
    let base = pkg_name.strip_suffix(".upk").or_else(|| pkg_name.strip_suffix(".UPK")).unwrap_or(pkg_name);
    let up = base.to_ascii_uppercase();
    let stem = ["_LOBBY_ASSETS_SF", "_UI_ASSETS_SF", "_TRANS_ASSETS_SF", "_ASSETS_SF"]
        .into_iter()
        .find_map(|suf| up.strip_suffix(suf))?;
    if stem.is_empty()
        || stem.ends_with("UI")
        || matches!(
            stem,
            "GENERALUI" | "HUD" | "GLOBAL" | "LOBBY" | "LOBBYUI" | "GAME" | "GAMEUI" | "SPECTATOR" | "LOADING" | "LOADINGSCREEN" | "COMMON" | "SHARED" | "GENERIC" | "DEVICE" | "DEVICES" | "ITEM" | "ITEMS"
        )
    {
        return None;
    }
    let role = package_role(base, stem)?;
    let champ = match stem {
        "BOMBKING" => "Bomb King".to_string(),
        "MALDAMBA" => "Mal'Damba".to_string(),
        "SHALIN" => "Sha Lin".to_string(),
        "BETTYLABOMBA" => "Betty la Bomba".to_string(),
        "VII" => "VII".to_string(),
        _ => {
            let words: Vec<String> = stem
                .split('_')
                .filter(|w| !w.is_empty())
                .map(|w| {
                    let mut chars = w.chars();
                    match chars.next() {
                        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str().to_ascii_lowercase()),
                        None => String::new(),
                    }
                })
                .collect();
            if words.is_empty() {
                return None;
            }
            words.join(" ")
        }
    };
    Some(format!("{champ} · {role}"))
}

impl Studio {
    /// The view's state, made (and the champions read) the first time it shows.
    pub(super) fn champions_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.champions.is_some() {
            return;
        }
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter champions"));
        cx.subscribe(&filter, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        self.champions = Some(Champions {
            list: Vec::new(),
            loading: true,
            error: None,
            filter,
            list_focus: cx.focus_handle(),
            selected: None,
            devices: None,
            skins: None,
            whole: Vec::new(),
            icons: HashMap::new(),
            default_skin: None,
            open_skin: None,
            skin_files: HashMap::new(),
        });
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let _ = backend.call("mcts.open", json!({})).await;
            let r = backend.call("mcts.choices", json!({ "kind": "bot", "limit": 1000 })).await;
            let _ = this.update(cx, |this, cx| {
                let mut need_icons = None;
                if let Some(c) = &mut this.champions {
                    c.loading = false;
                    match r {
                        Ok(r) => {
                            let mut list: Vec<Champion> = r
                                .json
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|b| {
                                    let reference = b["detail"].as_str()?.strip_prefix("champion · ")?.to_string();
                                    Some(Champion { id: b["id"].as_u64()?, name: b["name"].as_str().unwrap_or(&reference).to_string(), reference })
                                })
                                .collect();
                            list.sort_by_key(|ch| (is_other_entry(ch), ch.name.to_lowercase()));
                            c.list = list;
                            need_icons = c.selected.filter(|_| c.skins.is_some() && c.icons.is_empty());
                        }
                        Err(e) => c.error = Some(e),
                    }
                }
                this.sync_tab();
                if let Some(bot) = need_icons {
                    this.load_skin_icons(bot, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn select_champion(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(c) = &mut self.champions else { return };
        if let Some(name) = c.list.iter().find(|ch| ch.id == id).map(|ch| ch.name.clone()) {
            self.status = name.into();
        }
        if c.selected == Some(id) {
            return;
        }
        c.selected = Some(id);
        c.whole.clear();
        c.devices = None;
        c.skins = None;
        c.icons.clear();
        c.default_skin = None;
        c.open_skin = None;
        c.skin_files.clear();
        let (devices, skins) = (self.backend.call("mcts.choices", json!({ "kind": "device", "bot": id, "limit": 500 })), self.backend.call("mcts.collections", json!({ "bot": id })));
        cx.spawn(async move |this, cx| {
            let (d, s) = futures::join!(devices, skins);
            let _ = this.update(cx, |this, cx| {
                let Some(c) = this.champions.as_mut().filter(|c| c.selected == Some(id)) else { return };
                c.devices = Some(d.map(|r| r.json.as_array().cloned().unwrap_or_default()).unwrap_or_default());
                c.skins = Some(s.map(|r| r.json["collections"].as_array().cloned().unwrap_or_default()).unwrap_or_default());
                this.load_skin_icons(id, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn user_select_champion(&mut self, id: u64, close_drawer: bool, cx: &mut Context<Self>) {
        if close_drawer {
            layout::update(&self.panes, |p| p.overlay = false);
        }
        self.record(Place::Champions(Some(id as i64)));
        self.select_champion(id, cx);
    }

    /// Move the champion selection in the sidebar with ↑/↓/Home/End.
    fn step_champion(&mut self, step: isize, cx: &mut Context<Self>) {
        let Some(c) = &self.champions else { return };
        let f = c.filter.read(cx).value().trim().to_lowercase();
        let shown: Vec<u64> = c
            .list
            .iter()
            .filter(|ch| f.is_empty() || ch.name.to_lowercase().contains(&f) || ch.reference.to_lowercase().contains(&f))
            .map(|ch| ch.id)
            .collect();
        if shown.is_empty() {
            return;
        }
        let at = c.selected.and_then(|id| shown.iter().position(|&x| x == id));
        let next = match step {
            isize::MIN => 0,
            isize::MAX => shown.len() - 1,
            s => at.map(|a| (a as isize + s).clamp(0, shown.len() as isize - 1) as usize).unwrap_or(0),
        };
        self.user_select_champion(shown[next], step == 0, cx);
    }

    /// The selected champion's package named `<PREFIX>_<suffix>`.
    fn champion_package(&self, suffix: &str) -> Option<String> {
        let c = self.champions.as_ref()?;
        let ch = c.list.iter().find(|ch| Some(ch.id) == c.selected)?;
        let name = format!("{}_{suffix}", prefix(&ch.reference));
        self.packages.iter().find(|p| p.name.eq_ignore_ascii_case(&name)).map(|p| p.path.clone())
    }

    /// The skins' icons (`mcts.icons`: item → icon texture), read from the champion's UI package.
    fn load_skin_icons(&mut self, bot: u64, cx: &mut Context<Self>) {
        let Some(items) = self.champions.as_ref().and_then(|c| c.skins.as_ref()).map(|s| s.iter().filter_map(|s| s["item"].as_u64()).collect::<Vec<_>>()) else { return };
        let Some(ui) = self.champion_package("UI_ASSETS_SF") else { return };
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let Ok(r) = backend.call("mcts.icons", json!({ "items": items, "bot": bot })).await else { return };
            let default_skin = r.json["defaultSkin"].as_u64();
            let _ = this.update(cx, |this, cx| {
                if let Some(c) = this.champions.as_mut().filter(|c| c.selected == Some(bot)) {
                    c.default_skin = default_skin;
                    cx.notify();
                }
            });
            // The default skin first: it is the portrait.
            let mut wanted: Vec<(u64, String)> = r.json["items"].as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_ascii_lowercase()))).collect();
            wanted.sort_by_key(|(k, _)| Some(*k) != default_skin);
            for (item, texture) in wanted {
                // `SkinIcon_Cassie.Collection_Cassie_Default`: the cooked package keeps the first part as a group.
                let Ok(p) = backend.call("texture.preview", json!({ "path": ui, "object": texture, "maxSize": 128 })).await else { continue };
                let Some(png) = p.bin(&p.json["png"]).map(|b| b.to_vec()) else { continue };
                let alive = this.update(cx, |this, cx| {
                    let Some(c) = this.champions.as_mut().filter(|c| c.selected == Some(bot)) else { return false };
                    c.icons.insert(item, Arc::new(Image::from_bytes(ImageFormat::Png, png)));
                    cx.notify();
                    true
                });
                if !matches!(alive, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    /// Unfold a skin: the objects its referencer in `<PREFIX>_ASSETS_SF` has
    /// beyond the champion's base ones (`skinreferencer<part item>` less `botreferencer<bot>`).
    fn toggle_skin(&mut self, item: u64, parts: Vec<u64>, cx: &mut Context<Self>) {
        let Some(c) = &mut self.champions else { return };
        if c.open_skin == Some(item) {
            c.open_skin = None;
            cx.notify();
            return;
        }
        c.open_skin = Some(item);
        let Some(bot) = c.selected else { return };
        cx.notify();
        if c.skin_files.contains_key(&item) {
            return;
        }
        let Some(assets) = self.champion_package("ASSETS_SF") else {
            if let Some(c) = &mut self.champions {
                c.skin_files.insert(item, Err("The champion's game files are not in this folder.".into()));
            }
            return;
        };
        // Cooked packages keep their objects under a group named after them: `CASSIE_ASSETS_SF` → `cassie_assets`.
        let group = super::file_name(&assets).trim_end_matches(".upk").to_ascii_lowercase().trim_end_matches("_sf").to_string();
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let refs = |r: Result<crate::backend::Reply, String>| -> Vec<(i64, String, String)> {
                r.ok().map(|r| r.json["references"].as_array().into_iter().flatten().filter_map(|x| Some((x["ref"].as_i64()?, x["path"].as_str()?.to_string(), x["class"].as_str().unwrap_or("").to_ascii_lowercase()))).collect()).unwrap_or_default()
            };
            let base: std::collections::HashSet<i64> = refs(backend.call("object.deps", json!({ "path": assets, "object": format!("{group}.botreferencer{bot}") })).await).into_iter().map(|r| r.0).collect();
            let mut own: Vec<SkinFile> = Vec::new();
            let mut found = false;
            for part in parts {
                let r = backend.call("object.deps", json!({ "path": assets, "object": format!("{group}.skinreferencer{part}") })).await;
                found |= r.is_ok();
                for (re, path, class) in refs(r) {
                    // Exports of this package only (imports live elsewhere), of the classes a look is made of.
                    if re > 0 && !base.contains(&re) && LOOK_CLASSES.contains(&class.as_str()) && !own.iter().any(|f| f.index == (re - 1) as usize) {
                        own.push(SkinFile { package: assets.clone(), index: (re - 1) as usize, path, class });
                    }
                }
            }
            own.sort_by_key(|f| (LOOK_CLASSES.iter().position(|c| *c == f.class).unwrap_or(99), f.path.clone()));
            let _ = this.update(cx, |this, cx| {
                if let Some(c) = this.champions.as_mut().filter(|c| c.selected == Some(bot)) {
                    c.skin_files.insert(item, if found { Ok(own) } else { Err("The game files do not list this skin's objects.".into()) });
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Open the game data at the row of `table` whose `key` is `id`.
    fn open_row(&mut self, table: &'static str, key: &'static str, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let call = self.backend.call("mcts.find", json!({ "doc": "assembly", "table": table, "key": key, "value": id }));
        cx.spawn_in(window, async move |this, cx| {
            let r = call.await;
            let _ = this.update_in(cx, |this, window, cx| match r.ok().and_then(|r| r.json.as_array().and_then(|a| a.first()).and_then(|x| x.as_u64())) {
                Some(row) => {
                    let place = Place::Assembly { file: "assembly".into(), path: Vec::new(), table: Some(table.to_string()), row: Some(row as usize) };
                    this.record(place.clone());
                    this.go(place, window, cx);
                }
                None => this.toast(Tone::Warning, format!("No row with {key} {id} in the game data"), cx),
            });
        })
        .detach();
    }

    /// A skin unfolded: the textures, materials and models it has of its own.
    fn render_skin_files(&self, item: u64, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let c = self.champions.as_ref().expect("the view is shown");
        let body = v_flex().ml(px(52.0)).mb_2().gap_0p5();
        let files = match c.skin_files.get(&item) {
            None => return body.child(div().h(px(40.0)).child(ui::loading("Finding its files…", &theme))).into_any_element(),
            Some(Err(e)) => return body.child(ui::hint(e.clone(), &theme)).into_any_element(),
            Some(Ok(f)) => f,
        };
        if files.is_empty() {
            let mut body = body.child(ui::hint("It uses the champion's own models and textures (Look, above), with its own colours or parts set in the game data.", &theme));
            if let Some(pkg) = self.champion_package("ASSETS_SF") {
                body = body.child(
                    h_flex().pt_1().child(
                        Button::new(SharedString::from(format!("skin-base-pkg-{item}")))
                            .xsmall()
                            .ghost()
                            .label("Open champion's textures & models →")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let place = Place::package(pkg.clone(), None);
                                this.record(place.clone());
                                this.go(place, window, cx);
                            })),
                    ),
                );
            }
            return body.into_any_element();
        }
        let mut body = body.child(ui::hint("To reskin: open a Texture 2D below, click Export to save a PNG, edit it, then click Replace.", &theme).pb_1());
        for (k, f) in files.iter().enumerate().take(60) {
            let (package, key_pkg, index) = (f.package.clone(), f.package.clone(), f.index);
            let edited = self.dirty.contains(&f.package);
            let short_name = f.path.rsplit('.').next().unwrap_or(&f.path).to_string();
            let class_label = super::pretty_class(&f.class);
            body = body.child(
                h_flex()
                    .id(SharedString::from(format!("skin-file-{k}")))
                    .focusable()
                    .tab_index(0)
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(|s| s.border_color(theme.ring))
                    .text_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                        if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                            window.prevent_default();
                            cx.stop_propagation();
                            let place = Place::package(key_pkg.clone(), Some(index));
                            this.record(place.clone());
                            this.go(place, window, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let place = Place::package(package.clone(), Some(index));
                        this.record(place.clone());
                        this.go(place, window, cx);
                    }))
                    .child(Icon::new(super::class_icon(&f.class)).small().text_color(theme.muted_foreground))
                    .child(div().max_w(px(260.0)).flex_none().overflow_hidden().text_ellipsis().whitespace_nowrap().font_weight(gpui::FontWeight::MEDIUM).child(short_name))
                    .child(ui::badge(None, class_label, theme.muted_foreground, &theme))
                    .when(edited, |d| d.child(ui::badge(None, "edited", theme.warning, &theme)))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_right().font_family(ui::mono()).text_xs().text_color(theme.muted_foreground).child(f.path.clone()))
                    .child(Icon::new(IconName::ArrowUpRight).xsmall().text_color(theme.muted_foreground)),
            );
        }
        if files.len() > 60 {
            body = body.child(ui::hint(format!("and {} more in the game files", files.len() - 60), &theme));
        }
        body.into_any_element()
    }

    pub(super) fn render_champions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.champions_view(window, cx);
        let theme = cx.theme().clone();
        let c = self.champions.as_ref().expect("made above");
        let f = c.filter.read(cx).value().trim().to_lowercase();
        let list_focus = c.list_focus.clone();
        let list_focused = list_focus.is_focused(window) || list_focus.contains_focused(window, cx);
        let playable_total = c.list.iter().filter(|ch| !is_other_entry(ch)).count();
        let shown: Vec<&Champion> = c.list.iter().filter(|ch| f.is_empty() || ch.name.to_lowercase().contains(&f) || ch.reference.to_lowercase().contains(&f)).collect();
        let mut list = v_flex();
        let mut other_header_shown = false;
        for (k, ch) in shown.into_iter().enumerate() {
            if is_other_entry(ch) && !other_header_shown {
                other_header_shown = true;
                list = list.child(ui::section("Other", &theme).px_2());
            }
            let id = ch.id;
            let active = c.selected == Some(id);
            let (lf, lf_key) = (list_focus.clone(), list_focus.clone());
            list = list.child(
                h_flex()
                    .id(("champ", k))
                    .focusable()
                    .relative()
                    .min_h(super::ui::row_h(window))
                    .py_0p5()
                    .px_2()
                    .gap_2()
                    .text_sm()
                    .cursor_pointer()
                    .border_1()
                    .border_color(if active && list_focused { theme.ring } else { gpui::transparent_black() })
                    .focus_visible(|s| s.border_color(theme.ring))
                    .when(active, |d| d.bg(theme.list_active))
                    .when(!active, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                        if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                            window.prevent_default();
                            cx.stop_propagation();
                            lf_key.focus(window, cx);
                            this.user_select_champion(id, true, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        lf.focus(window, cx);
                        this.user_select_champion(id, true, cx);
                    }))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(ch.name.clone()))
                    .child(crate::driver::probe(format!("champ-{}", prefix(&ch.reference).to_lowercase()))),
            );
        }
        let body: AnyElement = if c.loading {
            ui::loading("Reading the champions…", &theme).into_any_element()
        } else if let Some(e) = &c.error {
            ui::error("The champions could not be read", ui::explain_error(e), &theme)
                .child(
                    Button::new("champ-retry")
                        .small()
                        .icon(Icon::new(IconName::RefreshCw))
                        .label("Try again")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.champions = None;
                            this.champions_view(window, cx);
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            div()
                .id("champ-list")
                .track_focus(&list_focus)
                .tab_index(0)
                .size_full()
                .border_1()
                .border_color(gpui::transparent_black())
                .focus_visible(|s| s.border_color(theme.ring))
                .overflow_y_scroll()
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| match e.keystroke.key.as_str() {
                    "up" if !e.keystroke.modifiers.modified() => {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.step_champion(-1, cx);
                    }
                    "down" if !e.keystroke.modifiers.modified() => {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.step_champion(1, cx);
                    }
                    "home" if !e.keystroke.modifiers.modified() => {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.step_champion(isize::MIN, cx);
                    }
                    "end" if !e.keystroke.modifiers.modified() => {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.step_champion(isize::MAX, cx);
                    }
                    "enter" | "space" if !e.keystroke.modifiers.modified() => {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.step_champion(0, cx);
                    }
                    _ => {}
                }))
                .child(list)
                .into_any_element()
        };
        let side = v_flex().size_full().child(ui::pane_header("Champions", div().text_xs().child(playable_total.to_string()), &theme)).child(ui::pane_filter(Input::new(&c.filter).small().cleanable(true))).child(div().flex_1().min_h_0().child(body));
        let main = self.render_champion(window, cx);
        let sizes = self.pane_sizes("champions", &[ui::SIDE_PANE]);
        let me = cx.entity().downgrade();
        layout::group(
            "champions",
            self.panes.get(),
            vec![layout::Part(layout::Role::Side, Some(sizes[0]), side.into_any_element()), layout::Part(layout::Role::Main, None, main)],
            Self::remember_panes(me.clone(), "champions"),
            move |_, cx| {
                let _ = me.update(cx, |s, cx| s.close_overlay(cx));
            },
            cx,
        )
    }

    /// One champion: skins, abilities and cards, game files.
    fn render_champion(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let c = self.champions.as_ref().expect("made by render_champions");
        let Some(ch) = c.selected.and_then(|id| c.list.iter().find(|ch| ch.id == id)).cloned() else {
            if c.loading && c.selected.is_some() {
                return ui::loading("Reading the champions…", &theme).into_any_element();
            }
            let side_shown = self.panes.get().side();
            let body = if side_shown {
                "Their skins, abilities, cards, talents and game files show here, each one click away from where you change it.".to_string()
            } else {
                format!("Open the champion list ({}) to inspect skins, abilities, cards, talents and game files.", super::commands::key_hint("⌘B"))
            };
            return ui::empty(IconName::Users, "Pick a champion", body, &theme)
                .when(!side_shown, |d| {
                    d.child(
                        div()
                            .relative()
                            .child(
                                Button::new("browse-champions")
                                    .small()
                                    .icon(Icon::new(IconName::PanelLeft))
                                    .label("Browse champions…")
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pane(false, cx))),
                            )
                            .child(crate::driver::probe("browse-champions")),
                    )
                })
                .into_any_element();
        };
        let section = |title: &str, count: Option<usize>| {
            h_flex().pt_6().pb_2().gap_2().child(div().text_base().font_weight(gpui::FontWeight::SEMIBOLD).child(title.to_string())).when_some(count, |d, n| d.child(div().text_sm().text_color(theme.muted_foreground).child(n.to_string())))
        };
        type RowAction = Rc<dyn Fn(&mut Studio, &mut Window, &mut Context<Studio>)>;
        let row = |id: SharedString,
                   title: String,
                   detail: String,
                   mono_detail: bool,
                   edited: bool,
                   cx: &mut Context<Self>,
                   activate: RowAction| {
            let act_key = activate.clone();
            h_flex()
                .id(id.clone())
                .focusable()
                .tab_index(0)
                .relative()
                .child(crate::driver::probe(id))
                .gap_3()
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .border_color(gpui::transparent_black())
                .focus_visible(|s| s.border_color(theme.ring))
                .cursor_pointer()
                .hover(|s| s.bg(theme.list_hover))
                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                    if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                        window.prevent_default();
                        cx.stop_propagation();
                        act_key(this, window, cx);
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| activate(this, window, cx)))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().child(title))
                        .when(!detail.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .when(mono_detail, |s| s.font_family(ui::mono()))
                                    .child(detail),
                            )
                        }),
                )
                .when(edited, |d| d.child(ui::badge(None, "edited", theme.warning, &theme)))
                .child(Icon::new(IconName::ArrowUpRight).small().text_color(theme.muted_foreground))
        };
        // Skins.
        let mut skins = v_flex().gap_0p5();
        match &c.skins {
            None => skins = skins.child(div().h(px(60.0)).child(ui::loading("Reading the skins…", &theme))),
            Some(list) if list.is_empty() => skins = skins.child(ui::hint("No skin collection lists this champion.", &theme)),
            Some(list) => {
                let all = c.whole.contains(&"skins");
                for (k, s) in list.iter().enumerate().take(if all { usize::MAX } else { SHORT }) {
                    let item = s["item"].as_u64().unwrap_or(0);
                    let parts: Vec<&str> = s["parts"].as_array().into_iter().flatten().filter_map(|p| p["kind"].as_str()).collect();
                    let mut detail = parts.join(", ");
                    if s["active"].as_bool() == Some(false) {
                        detail.push_str(" · not sold");
                    }
                    let name = s["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("Skin {item}"));
                    let part_items: Vec<u64> = s["parts"].as_array().into_iter().flatten().filter_map(|p| p["item"].as_u64()).chain(Some(item)).collect();
                    let key_parts = part_items.clone();
                    let open = c.open_skin == Some(item);
                    let thumb = match c.icons.get(&item) {
                        Some(icon) => img(icon.clone()).size(px(36.0)).rounded_md().object_fit(ObjectFit::Cover).into_any_element(),
                        None => div().size(px(36.0)).rounded_md().bg(theme.secondary).into_any_element(),
                    };
                    let id = SharedString::from(format!("skin-{k}"));
                    skins = skins.child(
                        h_flex()
                            .id(id.clone())
                            .focusable()
                            .tab_index(0)
                            .relative()
                            .child(crate::driver::probe(id))
                            .gap_3()
                            .px_3()
                            .py_1p5()
                            .rounded_md()
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .focus_visible(|s| s.border_color(theme.ring))
                            .cursor_pointer()
                            .when(open, |d| d.bg(theme.list_active))
                            .when(!open, |d| d.hover(|s| s.bg(theme.list_hover)))
                            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.toggle_skin(item, key_parts.clone(), cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_skin(item, part_items.clone(), cx)))
                            .child(thumb)
                            .child(v_flex().flex_1().min_w_0().child(div().text_sm().child(name)).child(div().text_xs().text_color(theme.muted_foreground).child(detail)))
                            .child(Button::new(("skin-row", k)).xsmall().ghost().icon(Icon::new(IconName::Table)).tooltip("Edit its row in the game data").on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_row("DATA_SET_ITEMS", "ITEM_ID", item, window, cx);
                            })))
                            .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).small().text_color(theme.muted_foreground)),
                    );
                    if open {
                        skins = skins.child(self.render_skin_files(item, cx));
                    }
                }
                if !all && list.len() > SHORT {
                    skins = skins.child(show_all("skins", list.len(), cx));
                }
            }
        }
        // Abilities, cards and talents.
        let mut devices = v_flex().gap_0p5();
        match &c.devices {
            None => devices = devices.child(div().h(px(60.0)).child(ui::loading("Reading the abilities…", &theme))),
            Some(list) if list.is_empty() => devices = devices.child(ui::hint("No ability, card or talent names this champion.", &theme)),
            Some(list) => {
                let all = c.whole.contains(&"devices");
                for (k, d) in list.iter().enumerate().take(if all { usize::MAX } else { SHORT }) {
                    let id = d["id"].as_u64().unwrap_or(0);
                    let name = d["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("Ability {id}"));
                    devices = devices.child(row(
                        SharedString::from(format!("dev-{k}")),
                        name,
                        d["detail"].as_str().unwrap_or("").to_string(),
                        false,
                        false,
                        cx,
                        Rc::new(move |this, window, cx| this.open_row("DATA_SET_DEVICES", "DEVICE_ID", id, window, cx)),
                    ));
                }
                if !all && list.len() > SHORT {
                    devices = devices.child(show_all("devices", list.len(), cx));
                }
            }
        }
        // Game files.
        let p = prefix(&ch.reference);
        let mut files = v_flex().gap_0p5();
        let mut n_files = 0;
        for (k, pkg) in self.packages.iter().filter(|e| package_role(&e.name, &p).is_some()).enumerate() {
            n_files += 1;
            let path = pkg.path.clone();
            let edited = self.dirty.contains(&pkg.path);
            files = files.child(row(
                SharedString::from(format!("file-{k}")),
                package_role(&pkg.name, &p).unwrap_or("").to_string(),
                pkg.name.clone(),
                true,
                edited,
                cx,
                Rc::new(move |this, _, cx| this.user_open_package(path.clone(), cx)),
            ));
        }
        if n_files == 0 {
            files = files.child(ui::hint(format!("No package of this folder starts with {p}_."), &theme));
        }
        let (n_skins, n_devices) = (c.skins.as_ref().map(Vec::len), c.devices.as_ref().map(Vec::len));
        let id = ch.id;
        let champ_name = ch.name.clone();
        // The portrait: the icon of the skin collection holding the default skin.
        let default_collection = c.default_skin.and_then(|d| c.skins.as_ref()?.iter().find(|s| s["item"].as_u64() == Some(d) || s["parts"].as_array().is_some_and(|p| p.iter().any(|p| p["item"].as_u64() == Some(d)))).and_then(|s| s["item"].as_u64()));
        let portrait = c.default_skin.and_then(|d| c.icons.get(&d)).or_else(|| default_collection.and_then(|d| c.icons.get(&d))).map(|icon| img(icon.clone()).size(px(56.0)).rounded_lg().object_fit(ObjectFit::Cover));
        let subtitle: AnyElement = if is_other_entry(&ch) {
            div().text_xs().text_color(theme.muted_foreground).child("Shared mount · models, skins and game files").into_any_element()
        } else if ch.reference != ch.name.replace(' ', "") {
            h_flex()
                .gap_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("Champion · internal prefix")
                .child(div().font_family(ui::mono()).child(ch.reference.clone()))
                .into_any_element()
        } else {
            div().text_xs().text_color(theme.muted_foreground).child("Playable champion · models, skins, abilities and cards").into_any_element()
        };
        let header = h_flex()
            .gap_4()
            .children(portrait)
            .child(v_flex().flex_1().min_w_0().child(div().text_2xl().font_weight(gpui::FontWeight::BOLD).child(ch.name.clone())).child(subtitle))
            .child(Button::new("champ-row").small().ghost().icon(Icon::new(IconName::Database)).label("Open in game data").tooltip("The champion's row: health, speed, role and every other field").on_click(cx.listener(move |this, _, window, cx| this.open_row("DATA_SET_BOTS", "BOT_ID", id, window, cx))))
            .when(!is_other_entry(&ch), |d| {
                d.child(Button::new("champ-sounds").small().ghost().icon(Icon::new(IconName::AudioLines)).label("Sounds").tooltip("Every voice line, ability, weapon and skin sound of this champion").on_click(cx.listener(move |this, _, window, cx| {
                    this.set_view(super::View::Audio, window, cx);
                    this.user_audio_open(format!("champion:{id}"), true, cx);
                })))
            })
            .child(Button::new("champ-new-skin").small().icon(Icon::new(IconName::Shirt)).label("New skin…").tooltip("Create a new skin for this champion, step by step").on_click(cx.listener(move |this, _, window, cx| this.open_wizard_for_champion(super::wizard::Kind::Skin, id, champ_name.clone(), window, cx))));
        div()
            .id("champion")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .max_w(px(860.0))
                    .px_8()
                    .py_6()
                    .child(header)
                    .child(section("Look: models, textures and effects", Some(n_files)))
                    .child(ui::hint("Open one, pick a texture, mesh or material, then Replace it with your own file.", &theme).pb_2())
                    .child(files)
                    .child(section("Skins", n_skins))
                    .child(ui::hint("Click a skin to list its textures, materials and 3D models, or click its table button on the right to edit its game data row.", &theme).pb_2())
                    .child(skins)
                    .child(section("Abilities, cards and talents", n_devices))
                    .child(ui::hint("Damage, cooldowns and other numbers are fields of each one's row in the game data.", &theme).pb_2())
                    .child(devices),
            )
            .into_any_element()
    }
}

/// The button showing the rest of a list.
fn show_all(list: &'static str, n: usize, cx: &mut Context<Studio>) -> impl IntoElement {
    div().pl_1().pt_1().child(Button::new(SharedString::from(format!("all-{list}"))).xsmall().ghost().label(format!("Show all {n}")).on_click(cx.listener(move |this, _, _, cx| {
        if let Some(c) = &mut this.champions {
            c.whole.push(list);
        }
        cx.notify();
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_packages_by_champion() {
        assert_eq!(prefix("Bomb King"), "BOMBKING");
        assert_eq!(prefix("Mal'Damba"), "MALDAMBA");
        assert_eq!(package_role("CASSIE_LOBBY_ASSETS_SF", "CASSIE"), Some("Lobby and menu models"));
        assert_eq!(package_role("CASSIE_ASSETS_SF", "CASSIE"), Some("In-match models, textures, effects and sounds"));
        assert_eq!(package_role("CASSIEX_ASSETS_SF", "CASSIE"), None);
        assert_eq!(package_role("CASSIE", "CASSIE"), None);
        assert!(is_other_entry(&Champion { id: 1, name: "Horse".into(), reference: "Horse".into() }));
        assert!(!is_other_entry(&Champion { id: 2, name: "Cassie".into(), reference: "Cassie".into() }));
    }

    #[test]
    fn derives_cold_start_fallback_package_labels() {
        assert_eq!(
            fallback_package_label("CASSIE_ASSETS_SF").as_deref(),
            Some("Cassie · In-match models, textures, effects and sounds")
        );
        assert_eq!(
            fallback_package_label("CASSIE_LOBBY_ASSETS_SF").as_deref(),
            Some("Cassie · Lobby and menu models")
        );
        assert_eq!(
            fallback_package_label("CASSIE_UI_ASSETS_SF").as_deref(),
            Some("Cassie · Interface: icons and portraits")
        );
        assert_eq!(
            fallback_package_label("CASSIE_TRANS_ASSETS_SF").as_deref(),
            Some("Cassie · Transition assets")
        );
        assert_eq!(
            fallback_package_label("BOMBKING_ASSETS_SF").as_deref(),
            Some("Bomb King · In-match models, textures, effects and sounds")
        );
        assert_eq!(fallback_package_label("GENERALUI_ASSETS_SF"), None);
        assert_eq!(fallback_package_label("HUD_ASSETS_SF"), None);
        assert_eq!(fallback_package_label("GLOBAL_ASSETS_SF"), None);
        assert_eq!(fallback_package_label("LOBBYUI_ASSETS_SF"), None);
        assert_eq!(fallback_package_label("GAMEUI_ASSETS_SF"), None);
        assert_eq!(fallback_package_label("LobbyUI_SF"), None);
        assert_eq!(fallback_package_label("ICE_MINES_P"), None);
    }
}
