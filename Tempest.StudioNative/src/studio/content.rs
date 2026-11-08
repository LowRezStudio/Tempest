//! Assembly helpers for making new game content: **New content** (a device,
//! card, item, champion or skin cloned from a template with fresh ids and
//! new texts in every language, `mcts.create`, ids from the user's id range
//! when one is set) and **Champion vendor** (the vendor's 60 buttons, and
//! champions whose Character Class item no vendor entry lists) and **Skins**
//! (each champion's skin collections with their parts: open one in the grid,
//! copy it as a new skin, or wrap a skin no collection holds in a new one).

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{div, prelude::*, px, App, Context, Entity, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme, Sizable, WindowExt};
use serde_json::{json, Value as J};

use super::assembly::Assembly;
use super::dialogs::footer;
use super::Tone;

const KINDS: &[(&str, &str, &str)] = &[
    ("device", "Ability", "a device (ability) of a champion: the devices and items rows, on the bot and slot you give"),
    ("card", "Card / talent", "a card or talent: a device too, on the bot and slot you give"),
    ("item", "Item", "an item (and its skin row)"),
    ("champion", "Champion", "a bot with a device per slot, Character Class and Weapon items, a default skin and a Champion Vendor entry"),
    ("skin", "Skin", "a copy of a skin collection (loot table 1076 entry): every part with its skin row"),
];

fn input(window: &mut Window, cx: &mut App, value: String, placeholder: &str) -> Entity<InputState> {
    let placeholder = placeholder.to_string();
    cx.new(|cx| {
        let mut s = InputState::new(window, cx).placeholder(placeholder);
        s.set_value(value, window, cx);
        s
    })
}

impl Assembly {
    /// The New content dialog, with the selected row as the template when it fits.
    pub(super) fn new_content(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (kind, template) = self.template_guess();
        self.new_content_with(kind, template, window, cx);
    }

    /// The New content dialog with `kind` chosen and `template` filled in.
    pub(super) fn new_content_with(&mut self, kind: &str, template: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let kind = Rc::new(RefCell::new(kind.to_string()));
        let template = input(window, cx, template.map(|t| t.to_string()).unwrap_or_default(), "the id of the row to copy");
        let name = input(window, cx, String::new(), "its name in game");
        let desc = input(window, cx, String::new(), "its description (optional)");
        let bot = input(window, cx, String::new(), "BOT_ID of the champion it goes on (devices, cards)");
        let slot = input(window, cx, String::new(), "slot (devices, cards; optional)");
        let range = input(window, cx, String::new(), "your id range, e.g. 9000000-9099999 (optional)");
        let me = cx.entity().downgrade();
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                name.update(cx, |s, cx| s.focus(window, cx));
            }
            let theme = cx.theme().clone();
            let current = kind.borrow().clone();
            let mut kinds = h_flex().gap_1();
            for (id, label, _) in KINDS {
                let k = kind.clone();
                let b = Button::new(SharedString::from(format!("nc-{id}"))).small().label(*label).on_click(move |_, _, cx| {
                    *k.borrow_mut() = id.to_string();
                    cx.refresh_windows();
                });
                kinds = kinds.child(if current == *id { b.primary() } else { b.ghost() });
            }
            let what = KINDS.iter().find(|k| k.0 == current).map(|k| k.2).unwrap_or_default();
            let row = |label: &'static str, i: &Entity<InputState>| h_flex().gap_2().child(div().w(px(110.0)).text_xs().text_color(theme.muted_foreground).child(label)).child(div().flex_1().child(Input::new(i).small()));
            let device = current == "device" || current == "card";
            let (me, t2, n2, d2, b2, s2, r2, k2) = (me.clone(), template.clone(), name.clone(), desc.clone(), bot.clone(), slot.clone(), range.clone(), kind.clone());
            dialog
                .title("New content")
                .w(px(640.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(kinds)
                        .child(div().text_xs().text_color(theme.muted_foreground).child(format!("Makes {what}, copied from the template with fresh ids; its texts are new messages in every language.")))
                        .child(row("Template id", &template))
                        .child(row("Name", &name))
                        .child(row("Description", &desc))
                        .when(device, |d| d.child(row("Champion", &bot)).child(row("Slot", &slot)))
                        .child(row("Id range", &range)),
                )
                .footer(footer("Create", false, true, move |window, cx| {
                    let num = |e: &Entity<InputState>, cx: &App| e.read(cx).value().trim().parse::<i64>().ok();
                    let Some(t) = num(&t2, cx) else { return };
                    let n = n2.read(cx).value().trim().to_string();
                    if n.is_empty() {
                        return;
                    }
                    let range = r2.read(cx).value().to_string();
                    let range = range.split_once('-').and_then(|(a, b)| Some((a.trim().parse::<i64>().ok()?, b.trim().parse::<i64>().ok()?)));
                    let params = json!({ "kind": *k2.borrow(), "template": t, "name": n, "description": d2.read(cx).value().to_string(), "bot": num(&b2, cx), "slot": num(&s2, cx) });
                    window.close_dialog(cx);
                    let _ = me.update(cx, |this, cx| this.create(params, range, cx));
                }))
        });
    }

    fn create(&mut self, params: J, range: Option<(i64, i64)>, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            if let Some((a, b)) = range {
                if let Err(e) = backend.call("mcts.idRange", json!({ "start": a, "end": b })).await {
                    let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Id range: {e}"), cx));
                    return;
                }
            }
            let r = backend.call("mcts.create", params).await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    let j = &r.json;
                    let ids = [("device", &j["deviceId"]), ("item", &j["itemId"]), ("bot", &j["botId"])].iter().filter_map(|(k, v)| v.as_i64().map(|v| format!("{k} {v}"))).collect::<Vec<_>>().join(", ");
                    this.toast(Tone::Success, format!("Made a new {} ({ids}); unsaved", j["kind"].as_str().unwrap_or_default()), cx);
                    this.reopen(cx);
                    // Show the main row made.
                    let path = [&j["bot"], &j["device"], &j["item"], &j["subPath"]].into_iter().find(|p| p.is_array()).cloned();
                    if let Some((t, i)) = path.and_then(|p| p.as_array().and_then(|a| a.first()).and_then(|f| Some((f[0].as_str()?.to_string(), f[1].as_u64()? as usize)))) {
                        this.visit("assembly".into(), Some(t), Some(i), cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("Could not create it: {e}"), cx),
            });
        })
        .detach();
    }

    /// The Champion vendor: listed entries against the 60 buttons, and the champions missing.
    pub(super) fn champion_vendor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let call = self.backend.call("mcts.vendor", json!({}));
        let me = cx.entity().downgrade();
        let handle = window.window_handle();
        cx.spawn(async move |_, cx| {
            let r = call.await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let v = match r {
                    Ok(r) => r.json,
                    Err(e) => {
                        let _ = me.update(cx, |this, cx| this.toast(Tone::Error, e, cx));
                        return;
                    }
                };
                let me = me.clone();
                window.open_dialog(cx, move |dialog, _, cx| {
                    let theme = cx.theme().clone();
                    let mut list = v_flex().gap_1();
                    let missing = v["missing"].as_array().cloned().unwrap_or_default();
                    if missing.is_empty() {
                        list = list.child(div().text_sm().text_color(theme.muted_foreground).child("Every champion's Character Class item has an entry."));
                    }
                    for (k, m) in missing.into_iter().enumerate() {
                        let bot = m["bot"].as_i64().unwrap_or(0);
                        let me = me.clone();
                        list = list.child(
                            h_flex()
                                .gap_2()
                                .text_sm()
                                .child(div().flex_1().child(format!("{} ({})", m["name"].as_str().unwrap_or_default(), m["reference"].as_str().unwrap_or_default())))
                                .child(div().text_xs().text_color(theme.muted_foreground).child(format!("bot {bot}, item {}", m["item"])))
                                .child(Button::new(("vend-add", k)).xsmall().label("Add").on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = me.update(cx, |this, cx| this.vendor_add(bot, cx));
                                })),
                        );
                    }
                    dialog
                        .title("Champion vendor")
                        .w(px(620.0))
                        .child(
                            v_flex()
                                .gap_2()
                                .child(div().text_sm().child(format!("{} active entries of {} (the in-match champion select has 60 buttons)", v["active"], v["total"])))
                                .child(div().text_xs().text_color(theme.muted_foreground).child("The client lists champions from this loot table's active entries, not from the bots: a champion without an entry never shows up."))
                                .child(div().id("vendor").max_h(px(360.0)).overflow_y_scroll().child(list)),
                        )
                });
            });
        })
        .detach();
    }

    fn vendor_add(&mut self, bot: i64, cx: &mut Context<Self>) {
        let call = self.backend.call("mcts.vendorAdd", json!({ "bot": bot }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.toast(Tone::Success, format!("Bot {bot} is in the Champion vendor (item {}); unsaved", r.json["item"]), cx);
                    this.reopen(cx);
                    if let Some((t, i)) = r.json["vendor"].as_array().and_then(|a| a.first()).and_then(|f| Some((f[0].as_str()?.to_string(), f[1].as_u64()? as usize))) {
                        this.visit("assembly".into(), Some(t), Some(i), cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("Could not add it: {e}"), cx),
            });
        })
        .detach();
    }

    /// The Skins dialog: champions on the left, their collections and loose skins on the right.
    pub(super) fn skins(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let call = self.backend.call("mcts.collections", json!({}));
        let me = cx.entity().downgrade();
        let handle = window.window_handle();
        cx.spawn(async move |_, cx| {
            let r = call.await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let v = match r {
                    Ok(r) => r.json,
                    Err(e) => {
                        let _ = me.update(cx, |this, cx| this.toast(Tone::Error, e, cx));
                        return;
                    }
                };
                let mut bots: Vec<(u64, String)> = v["bots"].as_object().into_iter().flatten().filter_map(|(k, n)| Some((k.parse().ok()?, n.as_str().unwrap_or_default().to_string()))).collect();
                bots.sort_by_key(|b| b.1.to_ascii_lowercase());
                let chosen = Rc::new(RefCell::new(bots.first().map(|b| b.0)));
                let filter = input(window, cx, String::new(), "Filter champions");
                let me = me.clone();
                window.open_dialog(cx, move |dialog, _, cx| {
                    let theme = cx.theme().clone();
                    let f = filter.read(cx).value().to_ascii_lowercase();
                    let current = *chosen.borrow();
                    let count = |b: u64| v["collections"].as_array().into_iter().flatten().filter(|c| c["bot"].as_u64() == Some(b)).count();
                    let mut side = v_flex().gap_0p5();
                    for (k, (b, name)) in bots.iter().enumerate().filter(|(_, (_, n))| f.is_empty() || n.to_ascii_lowercase().contains(&f)) {
                        let (b, ch) = (*b, chosen.clone());
                        side = side.child(
                            h_flex()
                                .id(("skin-bot", k))
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .text_sm()
                                .cursor_pointer()
                                .when(current == Some(b), |d| d.bg(theme.list_active))
                                .hover(|s| s.bg(theme.list_hover))
                                .on_click(move |_, _, cx| {
                                    *ch.borrow_mut() = Some(b);
                                    cx.refresh_windows();
                                })
                                .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(if name.is_empty() { format!("bot {b}") } else { name.clone() }))
                                .child(div().text_xs().text_color(theme.muted_foreground).child(count(b).to_string())),
                        );
                    }
                    let first_path = |p: &J| p.as_array().and_then(|a| a.first()).and_then(|f| Some((f[0].as_str()?.to_string(), f[1].as_u64()? as usize)));
                    let mut main = v_flex().gap_2();
                    let collections: Vec<J> = v["collections"].as_array().into_iter().flatten().filter(|c| c["bot"].as_u64() == current).cloned().collect();
                    if collections.is_empty() {
                        main = main.child(div().text_sm().text_color(theme.muted_foreground).child("No collection lists a skin of this champion."));
                    }
                    for (k, c) in collections.iter().enumerate() {
                        let item = c["item"].as_i64().unwrap_or(0);
                        let parts = c["parts"].as_array().cloned().unwrap_or_default();
                        let mut part_list = h_flex().flex_wrap().gap_1();
                        for p in &parts {
                            let missing = !p["exists"].as_bool().unwrap_or(true);
                            part_list = part_list.child(
                                div()
                                    .px_1()
                                    .rounded_sm()
                                    .text_xs()
                                    .bg(theme.secondary)
                                    .text_color(if missing { theme.danger } else { theme.muted_foreground })
                                    .child(format!("{}: {}", p["kind"].as_str().unwrap_or("part"), p["name"].as_str().filter(|n| !n.is_empty()).map(|n| n.to_string()).unwrap_or_else(|| format!("item {}", p["item"])))),
                            );
                        }
                        let (me1, me2) = (me.clone(), me.clone());
                        let open = first_path(&c["itemPath"]).or_else(|| first_path(&c["path"]));
                        main = main.child(
                            v_flex()
                                .gap_1()
                                .p_2()
                                .rounded_md()
                                .border_1()
                                .border_color(theme.border)
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(div().flex_1().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(c["name"].as_str().filter(|n| !n.is_empty()).map(|n| n.to_string()).unwrap_or_else(|| format!("item {item}"))))
                                        .when(!c["active"].as_bool().unwrap_or(false), |d| d.child(div().text_xs().text_color(theme.warning).child("inactive")))
                                        .child(div().text_xs().text_color(theme.muted_foreground).child(format!("item {item}")))
                                        .child(Button::new(("skin-open", k)).xsmall().ghost().label("Open").tooltip("Show the collection's item row in the grid").on_click(move |_, window, cx| {
                                            window.close_dialog(cx);
                                            if let Some((t, i)) = open.clone() {
                                                let _ = me1.update(cx, |this, cx| this.visit("assembly".into(), Some(t), Some(i), cx));
                                            }
                                        }))
                                        .child(Button::new(("skin-copy", k)).xsmall().primary().label("Make a copy…").tooltip("A new skin: this collection with every part copied, fresh ids and your names").on_click(move |_, window, cx| {
                                            window.close_dialog(cx);
                                            let _ = me2.update(cx, |this, cx| this.new_content_with("skin", Some(item), window, cx));
                                        })),
                                )
                                .child(part_list),
                        );
                    }
                    let loose: Vec<J> = v["loose"].as_array().into_iter().flatten().filter(|c| c["bot"].as_u64() == current).cloned().collect();
                    if !loose.is_empty() {
                        main = main.child(div().pt_2().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("SKINS NO COLLECTION HOLDS (the client never lists them)"));
                        for (k, l) in loose.iter().enumerate() {
                            let item = l["item"].as_i64().unwrap_or(0);
                            let name = l["name"].as_str().unwrap_or_default().to_string();
                            let (me1, me2) = (me.clone(), me.clone());
                            let open = first_path(&l["path"]);
                            main = main.child(
                                h_flex()
                                    .gap_2()
                                    .text_sm()
                                    .child(div().flex_1().child(if name.is_empty() { format!("item {item}") } else { name.clone() }))
                                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!("item {item}")))
                                    .child(Button::new(("loose-open", k)).xsmall().ghost().label("Open").on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        if let Some((t, i)) = open.clone() {
                                            let _ = me1.update(cx, |this, cx| this.visit("assembly".into(), Some(t), Some(i), cx));
                                        }
                                    }))
                                    .child(Button::new(("loose-wrap", k)).xsmall().label("Put in a new collection").tooltip("A collection (container item, loot table, vendor entry) holding this skin, so the client lists it").on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let name = name.clone();
                                        let _ = me2.update(cx, |this, cx| this.collection_add(item, name, cx));
                                    })),
                            );
                        }
                    }
                    dialog
                        .title("Skins")
                        .w(px(860.0))
                        .child(
                            v_flex()
                                .gap_2()
                                .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                                    "The client lists skins per champion from the collections of loot table {} ({} active of {}): a container item whose loot table holds the skin, head, weapon and entitlement.",
                                    v["table"], v["active"], v["total"]
                                )))
                                .child(
                                    h_flex()
                                        .items_start()
                                        .gap_3()
                                        .child(v_flex().w(px(220.0)).gap_1().child(Input::new(&filter).small()).child(div().id("skin-bots").h(px(440.0)).overflow_y_scroll().child(side)))
                                        .child(div().id("skin-main").flex_1().h(px(472.0)).overflow_y_scroll().child(main)),
                                ),
                        )
                });
            });
        })
        .detach();
    }

    fn collection_add(&mut self, item: i64, name: String, cx: &mut Context<Self>) {
        let call = self.backend.call("mcts.collectionAdd", json!({ "items": [item], "name": name }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.toast(Tone::Success, format!("Made a collection for item {item} (item {}); unsaved", r.json["item"]), cx);
                    this.reopen(cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not make the collection: {e}"), cx),
            });
        })
        .detach();
    }

    /// Export mod: a launcher mod folder with the assembly changes as row deltas, the
    /// private server's rows for a champion and script stubs for new devices (`mcts.scaffold`).
    pub(super) fn export_mod(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (kind, guess) = self.template_guess();
        let id = input(window, cx, "my-content".into(), "mod id: lowercase, digits, dashes");
        let name = input(window, cx, "My content".into(), "the mod's name");
        let version = input(window, cx, "0.1.0".into(), "version");
        let author = input(window, cx, String::new(), "author");
        let bot = input(window, cx, guess.filter(|_| kind == "champion").map(|b| b.to_string()).unwrap_or_default(), "BOT_ID of a new champion (optional): writes paserver.json");
        let devices = input(window, cx, guess.filter(|_| kind == "device").map(|b| b.to_string()).unwrap_or_default(), "DEVICE_IDs, comma separated (optional): script stubs");
        let all = Rc::new(std::cell::Cell::new(false));
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let row = |label: &'static str, i: &Entity<InputState>| h_flex().gap_2().child(div().w(px(110.0)).text_xs().text_color(theme.muted_foreground).child(label)).child(div().flex_1().child(Input::new(i).small()));
            let (set, flag) = (all.clone(), all.clone());
            let (me, i2, n2, v2, a2, b2, d2) = (me.clone(), id.clone(), name.clone(), version.clone(), author.clone(), bot.clone(), devices.clone());
            dialog
                .title("Export mod folder")
                .w(px(620.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_xs().text_color(theme.muted_foreground).child("Writes a launcher mod folder: manifest.toml, the rows this session added or changed in assembly.dat and the language files as deltas (the launcher applies them over the game's files, so other content mods keep working), paserver.json for a new champion, and for new devices UnrealScript class stubs with Lua handlers. Build it with the Mod view or `tempest mod pack`."))
                        .child(row("Id", &id))
                        .child(row("Name", &name))
                        .child(row("Version", &version))
                        .child(row("Author", &author))
                        .child(row("Champion", &bot))
                        .child(row("Devices", &devices))
                        .child(gpui_component::checkbox::Checkbox::new("all-langs").label("Every language file, even unchanged ones").checked(set.get()).on_click(move |v, window, _| {
                            set.set(*v);
                            window.refresh();
                        })),
                )
                .footer(footer("Choose a folder…", false, true, move |window, cx| {
                    let text = |e: &Entity<InputState>, cx: &App| e.read(cx).value().trim().to_string();
                    let id = text(&i2, cx);
                    if id.is_empty() {
                        return;
                    }
                    let params = json!({
                        "id": id,
                        "name": text(&n2, cx),
                        "version": text(&v2, cx),
                        "author": text(&a2, cx),
                        "bot": text(&b2, cx).parse::<u64>().ok(),
                        "devices": text(&d2, cx).split(|c: char| c == ',' || c.is_whitespace()).filter_map(|d| d.parse::<u64>().ok()).collect::<Vec<_>>(),
                        "allLanguages": flag.get(),
                    });
                    window.close_dialog(cx);
                    let _ = me.update(cx, |this, cx| this.write_mod(params, cx));
                }))
        });
    }

    fn write_mod(&mut self, mut params: J, cx: &mut Context<Self>) {
        let pick = cx.prompt_for_paths(gpui::PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Folder to write the mod folder in".into()) });
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut dirs))) = pick.await else { return };
            let Some(parent) = dirs.pop() else { return };
            let dir = parent.join(params["id"].as_str().unwrap_or("mod"));
            params["dir"] = json!(dir.to_string_lossy());
            let r = backend.call("mcts.scaffold", params).await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    this.toast(Tone::Success, format!("Wrote {} files in {}", r.json["files"].as_array().map(|f| f.len()).unwrap_or(0), dir.display()), cx);
                    cx.reveal_path(&dir);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not write the mod folder: {e}"), cx),
            });
        })
        .detach();
    }
}
