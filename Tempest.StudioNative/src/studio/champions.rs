//! The Champions view: the game's champions by their names in game, and for
//! one champion everything a mod changes about them (skins, abilities, cards
//! and talents, their game files), each a click away from where it is edited.

use gpui::{div, prelude::*, px, AnyElement, Context, Entity, SharedString, Window};
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
    selected: Option<u64>,
    /// The selected champion's devices and skin collections, once read.
    devices: Option<Vec<J>>,
    skins: Option<Vec<J>>,
    /// Lists shown whole ("skins", "devices"); the others stop at `SHORT`.
    whole: Vec<&'static str>,
}

/// Rows a list shows before its Show all.
const SHORT: usize = 8;

impl Champions {
    pub(super) fn filter_input(&self) -> Entity<InputState> {
        self.filter.clone()
    }

    /// Champions by package prefix (`BOMBKING` → Bomb King), to name packages.
    pub(super) fn by_prefix(&self) -> impl Iterator<Item = (String, &Champion)> {
        self.list.iter().map(|c| (prefix(&c.reference), c))
    }
}

/// `Bomb King`/`BombKing` → `BOMBKING`: how packages start.
pub fn prefix(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase()
}

/// What a champion's package holds, from its name (`CASSIE_LOBBY_ASSETS_SF`).
pub fn package_role(package: &str, prefix: &str) -> Option<&'static str> {
    let rest = package.strip_prefix(prefix)?.strip_prefix('_')?.to_ascii_uppercase();
    Some(match rest.as_str() {
        "ASSETS_SF" => "In-match models, textures, effects and sounds",
        "LOBBY_ASSETS_SF" => "Lobby and menu models",
        "UI_ASSETS_SF" => "Interface: icons and portraits",
        "TRANS_ASSETS_SF" => "Transition assets",
        _ => "Game files",
    })
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
        self.champions = Some(Champions { list: Vec::new(), loading: true, error: None, filter, selected: None, devices: None, skins: None, whole: Vec::new() });
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let _ = backend.call("mcts.open", json!({})).await;
            let r = backend.call("mcts.choices", json!({ "kind": "bot", "limit": 1000 })).await;
            let _ = this.update(cx, |this, cx| {
                let Some(c) = &mut this.champions else { return };
                c.loading = false;
                match r {
                    Ok(r) => {
                        c.list = r.json.as_array().into_iter().flatten().filter_map(|b| {
                            let reference = b["detail"].as_str()?.strip_prefix("champion · ")?.to_string();
                            Some(Champion { id: b["id"].as_u64()?, name: b["name"].as_str().unwrap_or(&reference).to_string(), reference })
                        }).collect();
                    }
                    Err(e) => c.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn select_champion(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(c) = &mut self.champions else { return };
        if c.selected == Some(id) {
            return;
        }
        c.selected = Some(id);
        c.whole.clear();
        c.devices = None;
        c.skins = None;
        let (devices, skins) = (self.backend.call("mcts.choices", json!({ "kind": "device", "bot": id, "limit": 500 })), self.backend.call("mcts.collections", json!({ "bot": id })));
        cx.spawn(async move |this, cx| {
            let (d, s) = futures::join!(devices, skins);
            let _ = this.update(cx, |this, cx| {
                let Some(c) = this.champions.as_mut().filter(|c| c.selected == Some(id)) else { return };
                c.devices = Some(d.map(|r| r.json.as_array().cloned().unwrap_or_default()).unwrap_or_default());
                c.skins = Some(s.map(|r| r.json["collections"].as_array().cloned().unwrap_or_default()).unwrap_or_default());
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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

    pub(super) fn render_champions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.champions_view(window, cx);
        let theme = cx.theme().clone();
        let c = self.champions.as_ref().expect("made above");
        let f = c.filter.read(cx).value().trim().to_lowercase();
        let mut list = v_flex().p_1();
        for (k, ch) in c.list.iter().filter(|ch| f.is_empty() || ch.name.to_lowercase().contains(&f) || ch.reference.to_lowercase().contains(&f)).enumerate() {
            let id = ch.id;
            let active = c.selected == Some(id);
            list = list.child(
                h_flex()
                    .id(("champ", k))
                    .relative()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .rounded_md()
                    .text_sm()
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.list_active))
                    .when(!active, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_champion(id, cx)))
                    .child(div().flex_none().size(px(22.0)).rounded_full().bg(theme.secondary).flex().items_center().justify_center().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).child(ch.name.chars().next().map(|c| c.to_string()).unwrap_or_default()))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(ch.name.clone()))
                    .child(crate::driver::probe(format!("champ-{}", prefix(&ch.reference).to_lowercase()))),
            );
        }
        let body: AnyElement = if c.loading {
            ui::loading("Reading the champions…", &theme).into_any_element()
        } else if let Some(e) = &c.error {
            ui::error("The champions could not be read", e.clone(), &theme).into_any_element()
        } else {
            div().id("champ-list").size_full().overflow_y_scroll().child(list).into_any_element()
        };
        let side = v_flex().size_full().child(ui::pane_header("Champions", div().text_xs().child(c.list.len().to_string()), &theme)).child(div().px_1().pb_1().child(Input::new(&c.filter).small().cleanable(true))).child(div().flex_1().min_h_0().child(body));
        let main = self.render_champion(window, cx);
        let sizes = self.pane_sizes("champions", &[240.0]);
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
            return ui::empty(IconName::Users, "Pick a champion", "Their skins, abilities, cards, talents and game files show here, each one click away from where you change it.", &theme).into_any_element();
        };
        let section = |title: &str, count: Option<usize>| {
            h_flex().pt_6().pb_2().gap_2().child(div().text_base().font_weight(gpui::FontWeight::SEMIBOLD).child(title.to_string())).when_some(count, |d, n| d.child(div().text_sm().text_color(theme.muted_foreground).child(n.to_string())))
        };
        let row = |id: SharedString, title: String, detail: String| {
            h_flex().id(id.clone()).relative().child(crate::driver::probe(id)).gap_3().px_3().py_2().rounded_md().cursor_pointer().hover(|s| s.bg(theme.list_hover)).child(v_flex().flex_1().min_w_0().child(div().text_sm().child(title)).when(!detail.is_empty(), |d| d.child(div().text_xs().text_color(theme.muted_foreground).child(detail)))).child(Icon::new(IconName::ArrowUpRight).small().text_color(theme.muted_foreground))
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
                    skins = skins.child(row(SharedString::from(format!("skin-{k}")), name, detail).on_click(cx.listener(move |this, _, window, cx| this.open_row("DATA_SET_ITEMS", "ITEM_ID", item, window, cx))));
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
                    let name = d["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("Device {id}"));
                    devices = devices.child(row(SharedString::from(format!("dev-{k}")), name, d["detail"].as_str().unwrap_or("").to_string()).on_click(cx.listener(move |this, _, window, cx| this.open_row("DATA_SET_DEVICES", "DEVICE_ID", id, window, cx))));
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
            files = files.child(row(SharedString::from(format!("file-{k}")), package_role(&pkg.name, &p).unwrap_or("").to_string(), pkg.name.clone()).on_click(cx.listener(move |this, _, _, cx| this.user_open_package(path.clone(), cx))));
        }
        if n_files == 0 {
            files = files.child(ui::hint(format!("No package of this folder starts with {p}_."), &theme));
        }
        let (n_skins, n_devices) = (c.skins.as_ref().map(Vec::len), c.devices.as_ref().map(Vec::len));
        let id = ch.id;
        let header = h_flex()
            .gap_4()
            .child(div().flex_none().size(px(56.0)).rounded_lg().bg(theme.primary.opacity(0.15)).flex().items_center().justify_center().text_2xl().font_weight(gpui::FontWeight::BOLD).text_color(theme.primary).child(ch.name.chars().next().map(|c| c.to_string()).unwrap_or_default()))
            .child(v_flex().flex_1().min_w_0().child(div().text_2xl().font_weight(gpui::FontWeight::BOLD).child(ch.name.clone())).child(div().text_xs().text_color(theme.muted_foreground).child(format!("{} · bot {}", ch.reference, ch.id))))
            .child(Button::new("champ-row").small().ghost().icon(Icon::new(IconName::Database)).label("Open in game data").tooltip("The champion's row: health, speed, role and every other field").on_click(cx.listener(move |this, _, window, cx| this.open_row("DATA_SET_BOTS", "BOT_ID", id, window, cx))))
            .child(Button::new("champ-new-skin").small().icon(Icon::new(IconName::Shirt)).label("New skin…").on_click(cx.listener(|this, _, window, cx| this.open_wizard(Some(super::wizard::Kind::Skin), None, window, cx))));
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
                    .child(section("Skins", n_skins))
                    .child(ui::hint("Each opens its row in the game data. To change how a skin looks, open the game files below and replace its textures or meshes.", &theme).pb_2())
                    .child(skins)
                    .child(section("Abilities, cards and talents", n_devices))
                    .child(ui::hint("Damage, cooldowns and other numbers are fields of each one's row in the game data.", &theme).pb_2())
                    .child(devices)
                    .child(section("Game files", Some(n_files)))
                    .child(files),
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
    }
}
