//! The Assembly view's side pane: the files (`assembly.dat` and each
//! `Lang_<EXT>.dat`), their header fields (edited in place; the game version
//! as major.minor.build.revision) and their tables, grouped by domain or
//! filtered by friendly or raw name.

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, KeyDownEvent, MouseButton, MouseDownEvent, SharedString, Window};
use gpui_component::input::Input;
use gpui_component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;

use super::{header_text, short, Assembly, Edit, Table};
use crate::studio::Place;

/// What a table holds, in words, for the side list (`DATA_SET_ITEMS` → `Items`).
pub fn friendly_table(name: &str) -> Option<&'static str> {
    Some(match name.strip_prefix("DATA_SET_").unwrap_or(name) {
        "ITEMS" => "Items",
        "ITEM_CHILDREN" => "Item contents",
        "DEVICES" => "Abilities, cards and talents",
        "SKINS" => "Skins",
        "BOTS" => "Champions and bots",
        "BOT_SPAWN_TABLES" => "Bot spawns",
        "BOT_BEHAVIORS" => "Bot behaviors",
        "PROJECTILES" => "Projectiles",
        "DEPLOYABLES" => "Deployables",
        "SPECIAL_FX" => "Visual effects",
        "EFFECT_GROUPS" => "Effect groups",
        "RESOURCES" => "Resources (asset paths)",
        "ICONS" => "Icons",
        "VALID_VALUES" => "Value names",
        "TIPS" => "Loading tips",
        "PROPERTIES" => "Properties",
        "MAT_RES_GROUPS" => "Material groups",
        "LOOT_TABLES" => "Loot tables and vendors",
        "LOOT_TABLE_GROUPS" => "Loot table groups",
        "XP_TABLES" => "Experience",
        "LANGUAGES" => "Languages",
        "VGS" => "Quick chat",
        "VOX_CHATTER" => "Voice chatter",
        "ASSEMBLY_MESHES" => "Meshes",
        "MESSAGES" | "DATA_SET" => "Texts",
        _ => return None,
    })
}

/// Human language name for a UE3 locale code or `Lang_<CODE>.dat` path.
pub fn friendly_lang(doc_or_file: &str) -> Option<&'static str> {
    let file = doc_or_file.rsplit(['/', '\\']).next().unwrap_or(doc_or_file);
    let stem = file.strip_suffix(".dat").unwrap_or(file);
    let code = stem.strip_prefix("Lang_").unwrap_or(stem).trim().to_ascii_uppercase();
    Some(match code.as_str() {
        "INT" => "English",
        "FRA" => "French",
        "DEU" => "German",
        "ESN" => "Spanish",
        "ESM" => "Spanish (LatAm)",
        "POL" => "Polish",
        "RUS" => "Russian",
        "POR" => "Portuguese",
        "CHN" => "Chinese",
        "JPN" => "Japanese",
        "KOR" => "Korean",
        "TUR" => "Turkish",
        "THA" => "Thai",
        _ => return None,
    })
}

const TABLE_GROUPS: [&str; 7] = [
    "Champions & abilities",
    "Bots & deployables",
    "Skins & items",
    "Loot & progression",
    "Interface & audio",
    "Effects & assets",
    "Lookup & settings",
];

fn table_group_index(name: &str) -> usize {
    match name.strip_prefix("DATA_SET_").unwrap_or(name) {
        "BOTS" | "DEVICES" | "PROJECTILES" | "EFFECT_GROUPS" => 0,
        "DEPLOYABLES" | "BOT_SPAWN_TABLES" | "BOT_BEHAVIORS" => 1,
        "SKINS" | "ITEMS" | "ITEM_CHILDREN" => 2,
        "LOOT_TABLES" | "LOOT_TABLE_GROUPS" | "XP_TABLES" => 3,
        "ICONS" | "TIPS" | "VGS" | "VOX_CHATTER" => 4,
        "SPECIAL_FX" | "RESOURCES" | "MAT_RES_GROUPS" | "ASSEMBLY_MESHES" => 5,
        _ => 6,
    }
}

fn table_matches(doc_id: &str, table_name: &str, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    table_name.to_ascii_lowercase().contains(query_lower)
        || friendly_table(table_name).is_some_and(|n| n.to_ascii_lowercase().contains(query_lower))
        || (doc_id != "assembly"
            && (doc_id.to_ascii_lowercase().contains(query_lower)
                || friendly_lang(doc_id).is_some_and(|l| l.to_ascii_lowercase().contains(query_lower))))
}

impl Assembly {
    /// The tables the list shows, in visual order: (file, table).
    fn listed_tables(&self, cx: &Context<Self>) -> Vec<(String, String)> {
        let f = self.table_filter.read(cx).value().trim().to_ascii_lowercase();
        let mut out = Vec::new();
        for d in &self.docs {
            if f.is_empty() && d.id == "assembly" {
                for g in 0..TABLE_GROUPS.len() {
                    for t in d.tables.iter().filter(|t| table_group_index(&t.name) == g) {
                        out.push((d.id.clone(), t.name.clone()));
                    }
                }
            } else {
                for t in d.tables.iter().filter(|t| table_matches(&d.id, &t.name, &f)) {
                    out.push((d.id.clone(), t.name.clone()));
                }
            }
        }
        out
    }

    /// ↑ ↓ in the table list: open the next table.
    pub(super) fn step_table(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let all = self.listed_tables(cx);
        if all.is_empty() {
            return;
        }
        let at = if self.path.is_empty() { all.iter().position(|(d, t)| Some(d) == self.doc.as_ref() && Some(t) == self.table.as_ref()) } else { None };
        let next = match at {
            Some(i) => (i as isize + by).clamp(0, all.len() as isize - 1) as usize,
            None => 0,
        };
        if at == Some(next) {
            return;
        }
        let (doc, table) = all[next].clone();
        self.open_table(doc, table, window, cx);
        self.tables_focus.focus(window, cx);
    }

    fn table_menu(&mut self, doc: String, table: String, path: String, menu: PopupMenu, cx: &mut Context<Self>) -> PopupMenu {
        let me = cx.entity().downgrade();
        type Act = dyn Fn(&mut Assembly, &mut Window, &mut Context<Assembly>);
        let item = |label: &str, f: Box<Act>| {
            let me = me.clone();
            let f: std::rc::Rc<Act> = f.into();
            PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                let f = f.clone();
                let _ = me.update(cx, |this, cx| f(this, window, cx));
            })
        };
        let (d1, t1, d2, t2, t3) = (doc.clone(), table.clone(), doc, table.clone(), table.clone());
        menu.label(table)
            .item(item("Open", Box::new(move |a, w, cx| a.open_table(d1.clone(), t1.clone(), w, cx))))
            .item(item("Open in a new tab", Box::new(move |a, _, cx| a.open_in_tab(Place::Assembly { file: d2.clone(), path: Vec::new(), table: Some(t2.clone()), row: None }, cx))))
            .separator()
            .item(item("Copy the table name", Box::new(move |a, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(t3.clone()));
                a.set_status(format!("Copied {t3}"), cx);
            })))
            .item(item(if cfg!(target_os = "macos") { "Show the file in Finder" } else { "Show the file in the file manager" }, Box::new(move |_, _, cx| cx.reveal_path(std::path::Path::new(&path)))))
    }

    fn render_table_row(&self, doc_id: &str, doc_path: &str, k: usize, t: &Table, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let side_focused = self.tables_focus.is_focused(window) || self.tables_focus.contains_focused(window, cx);
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let friendly = friendly_table(&t.name);
        let active = self.doc.as_deref() == Some(doc_id) && self.table.as_deref() == Some(t.name.as_str()) && self.path.is_empty();
        let (doc, table) = (doc_id.to_string(), t.name.clone());
        let (doc2, table2) = (doc_id.to_string(), t.name.clone());
        let (doc3, table3, path3) = (doc_id.to_string(), t.name.clone(), doc_path.to_string());
        let probe = format!("table-{}", t.name.to_ascii_lowercase());
        let tip = format!("{} · {}", t.name, if t.columns == 1 { "1 column".to_string() } else { format!("{} columns", t.columns) });
        let me = cx.entity();
        h_flex()
            .id(SharedString::from(format!("t-{doc_id}-{k}")))
            .focusable()
            .relative()
            .min_h(crate::studio::ui::row_h(window))
            .py_0p5()
            .px_2()
            .gap_2()
            .text_xs()
            .border_1()
            .border_color(if active && side_focused { theme.ring } else { gpui::transparent_black() })
            .focus_visible(|s| s.border_color(theme.ring))
            .cursor_pointer()
            .when(active, |e| e.bg(theme.list_active))
            .when(!active, |e| e.hover(|s| s.bg(theme.list_hover)))
            .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                if e.modifiers().secondary(cx) {
                    this.open_in_tab(Place::Assembly { file: doc.clone(), path: Vec::new(), table: Some(table.clone()), row: None }, cx);
                } else {
                    this.open_table(doc.clone(), table.clone(), window, cx);
                    this.tables_focus.focus(window, cx);
                }
            }))
            .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _: &MouseDownEvent, _, cx| this.open_in_tab(Place::Assembly { file: doc2.clone(), path: Vec::new(), table: Some(table2.clone()), row: None }, cx)))
            .child(match friendly {
                Some(n) => div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_sm()
                    .child(n)
                    .into_any_element(),
                None => div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .font_family(mono.clone())
                    .child(short(&t.name).to_string())
                    .into_any_element(),
            })
            .child(div().flex_none().font_family(mono).text_color(muted).child(t.rows.to_string()))
            .child(crate::driver::probe(probe))
            .context_menu(move |menu, _, cx| me.update(cx, |this, cx| this.table_menu(doc3.clone(), table3.clone(), path3.clone(), menu, cx)))
            .into_any_element()
    }

    pub(super) fn render_side(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let f = self.table_filter.read(cx).value().trim().to_ascii_lowercase();
        let mut list = v_flex();
        for k_doc in 0..self.docs.len() {
            let doc_id = self.docs[k_doc].id.clone();
            let doc_path = self.docs[k_doc].path.clone();
            let doc_fn = self.docs[k_doc].function.clone();
            let doc_dirty = self.docs[k_doc].dirty;
            let doc_label = self.docs[k_doc].label();
            if !f.is_empty() && !self.docs[k_doc].tables.iter().any(|t| table_matches(&doc_id, &t.name, &f)) {
                continue;
            }
            let friendly_doc = if doc_id == "assembly" {
                "Game data".to_string()
            } else if let Some(lang) = friendly_lang(&doc_id) {
                format!("{lang} texts")
            } else {
                format!("{doc_id} texts")
            };
            let fn_part = if doc_fn.is_empty() || doc_fn == "0x0" { String::new() } else { format!(" · marshal {doc_fn}") };
            let dirty_part = if doc_dirty { " · changed in your mod" } else { "" };
            let tip = format!("{doc_path}{fn_part}{dirty_part} (click to show or hide header fields)");
            let open = self.headers_open.contains(&doc_id);
            let (id, key_id) = (doc_id.clone(), doc_id.clone());
            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("doc-{doc_id}")))
                    .focusable()
                    .tab_index(0)
                    .px_2()
                    .py_1()
                    .gap_1p5()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(|s| s.border_color(theme.ring))
                    .bg(theme.secondary.opacity(0.6))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.secondary))
                    .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                        if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                            window.prevent_default();
                            cx.stop_propagation();
                            if !this.headers_open.remove(&key_id) {
                                this.headers_open.insert(key_id.clone());
                            }
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.headers_open.remove(&id) {
                            this.headers_open.insert(id.clone());
                        }
                        cx.notify();
                    }))
                    .child(Icon::new(if doc_id == "assembly" { IconName::Database } else { IconName::Languages }).size_3().text_color(muted))
                    .child(div().flex_none().whitespace_nowrap().child(friendly_doc))
                    .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family(mono.clone()).font_weight(gpui::FontWeight::NORMAL).text_color(muted).child(doc_label))
                    .child(div().flex_1().min_w_1())
                    .when(doc_dirty, |e| {
                        e.child(
                            h_flex()
                                .flex_none()
                                .gap_1()
                                .child(div().size(px(6.0)).flex_none().rounded_full().bg(theme.warning))
                                .child(div().font_weight(gpui::FontWeight::NORMAL).text_color(theme.warning).child("edited")),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .px_1p5()
                            .py(px(1.0))
                            .rounded_sm()
                            .border_1()
                            .border_color(if open { theme.border } else { gpui::transparent_black() })
                            .when(open, |d| d.bg(theme.background.opacity(0.6)))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .text_xs()
                            .text_color(muted)
                            .child(if open { "Hide header" } else { "Header" }),
                    ),
            );
            // The header fields, when unfolded (hidden while the tables are filtered).
            if f.is_empty() && open {
                let headers = self.docs[k_doc].header.clone();
                for (k, (field, value)) in headers.iter().enumerate() {
                    let shown = header_text(field, value);
                    let editing = matches!(&self.edit, Some(Edit::Header { doc, field: fl }) if doc == &doc_id && fl == field);
                    let (doc, field2, draft) = (doc_id.clone(), field.clone(), shown.clone());
                    let name_tip = field.clone();
                    let tip = if field == "GAME_VERSION_RAW" { format!("{} (click to edit as major.minor.build.rev)", super::cell_text(value)) } else { format!("{shown} (click to edit)") };
                    list = list.child(
                        h_flex()
                            .gap_2()
                            .px_2()
                            .py_0p5()
                            .text_xs()
                            .font_family(mono.clone())
                            .child(
                                div()
                                    .id(SharedString::from(format!("hdrn-{doc_id}-{k}")))
                                    .w(px(124.0))
                                    .flex_none()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_color(muted)
                                    .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(name_tip.clone()).build(window, cx))
                                    .child(field.clone()),
                            )
                            .child(if editing {
                                div().key_context("AsmEdit").flex_1().min_w_0().child(Input::new(&self.edit_input).xsmall()).into_any_element()
                            } else {
                                let (key_doc, key_field, key_draft) = (doc.clone(), field2.clone(), draft.clone());
                                div()
                                    .id(SharedString::from(format!("hdr-{doc_id}-{k}")))
                                    .focusable()
                                    .tab_index(0)
                                    .flex_1()
                                    .min_w_0()
                                    .min_h(px(16.0))
                                    .px_1()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(gpui::transparent_black())
                                    .focus_visible(|s| s.border_color(theme.ring))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .cursor_text()
                                    .hover(|s| s.bg(theme.list_hover))
                                    .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                        if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                            window.prevent_default();
                                            cx.stop_propagation();
                                            this.start_edit(Edit::Header { doc: key_doc.clone(), field: key_field.clone() }, key_draft.clone(), key_draft.clone(), true, window, cx);
                                        }
                                    }))
                                    .on_click(cx.listener(move |this, _, window, cx| this.start_edit(Edit::Header { doc: doc.clone(), field: field2.clone() }, draft.clone(), draft.clone(), true, window, cx)))
                                    .child(shown)
                                    .into_any_element()
                            }),
                    );
                }
            }
            let tables: Vec<(usize, Table)> = self.docs[k_doc]
                .tables
                .iter()
                .enumerate()
                .map(|(k, t)| (k, Table { name: t.name.clone(), rows: t.rows, columns: t.columns }))
                .collect();
            if f.is_empty() && doc_id == "assembly" {
                for (g, group_title) in TABLE_GROUPS.iter().enumerate() {
                    let group_items: Vec<&(usize, Table)> = tables.iter().filter(|(_, t)| table_group_index(&t.name) == g).collect();
                    if group_items.is_empty() {
                        continue;
                    }
                    list = list.child(div().px_2().child(crate::studio::ui::section(*group_title, &theme)));
                    for (k, t) in group_items {
                        list = list.child(self.render_table_row(&doc_id, &doc_path, *k, t, window, cx));
                    }
                }
            } else {
                for (k, t) in &tables {
                    if !table_matches(&doc_id, &t.name, &f) {
                        continue;
                    }
                    list = list.child(self.render_table_row(&doc_id, &doc_path, *k, t, window, cx));
                }
            }
        }
        v_flex()
            .size_full()
            .child(
                crate::studio::ui::pane_header(
                    "Tables",
                    div().text_xs().font_weight(gpui::FontWeight::NORMAL).child(if self.docs.len() == 1 { "1 file".to_string() } else { format!("{} files", self.docs.len()) }),
                    &theme,
                ),
            )
            .child(crate::studio::ui::pane_filter(Input::new(&self.table_filter).small().cleanable(true)))
            .child(
                div()
                    .id("tables")
                    .track_focus(&self.tables_focus)
                    .tab_index(0)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(|s| s.border_color(theme.ring))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_groups_stay_within_four_items() {
        let all = [
            "DATA_SET_ITEMS",
            "DATA_SET_ITEM_CHILDREN",
            "DATA_SET_DEVICES",
            "DATA_SET_SKINS",
            "DATA_SET_BOTS",
            "DATA_SET_BOT_SPAWN_TABLES",
            "DATA_SET_BOT_BEHAVIORS",
            "DATA_SET_PROJECTILES",
            "DATA_SET_DEPLOYABLES",
            "DATA_SET_SPECIAL_FX",
            "DATA_SET_EFFECT_GROUPS",
            "DATA_SET_RESOURCES",
            "DATA_SET_ICONS",
            "DATA_SET_VALID_VALUES",
            "DATA_SET_TIPS",
            "DATA_SET_PROPERTIES",
            "DATA_SET_MAT_RES_GROUPS",
            "DATA_SET_LOOT_TABLES",
            "DATA_SET_LOOT_TABLE_GROUPS",
            "DATA_SET_XP_TABLES",
            "DATA_SET_LANGUAGES",
            "DATA_SET_VGS",
            "DATA_SET_VOX_CHATTER",
            "DATA_SET_ASSEMBLY_MESHES",
            "DATA_SET_EXTRA_UNKNOWN",
        ];
        let mut counts = [0usize; TABLE_GROUPS.len()];
        for t in all {
            counts[table_group_index(t)] += 1;
        }
        for (g, &n) in counts.iter().enumerate() {
            assert!(n <= 4, "group {} ({}) has {n} items", g, TABLE_GROUPS[g]);
        }
    }

    #[test]
    fn maps_ue3_language_codes() {
        assert_eq!(friendly_lang("INT"), Some("English"));
        assert_eq!(friendly_lang("Lang_INT.dat"), Some("English"));
        assert_eq!(friendly_lang("/ChaosGame/Localization/ESM/Lang_ESM.dat"), Some("Spanish (LatAm)"));
        assert_eq!(friendly_lang("FRA"), Some("French"));
        assert_eq!(friendly_lang("DEU"), Some("German"));
        assert_eq!(friendly_lang("ESN"), Some("Spanish"));
        assert_eq!(friendly_lang("POL"), Some("Polish"));
        assert_eq!(friendly_lang("RUS"), Some("Russian"));
        assert_eq!(friendly_lang("POR"), Some("Portuguese"));
        assert_eq!(friendly_lang("CHN"), Some("Chinese"));
        assert_eq!(friendly_lang("JPN"), Some("Japanese"));
        assert_eq!(friendly_lang("KOR"), Some("Korean"));
        assert_eq!(friendly_lang("TUR"), Some("Turkish"));
        assert_eq!(friendly_lang("THA"), Some("Thai"));
        assert_eq!(friendly_lang("XYZ"), None);
    }

    #[test]
    fn filters_tables_by_friendly_and_raw_names() {
        assert!(table_matches("assembly", "DATA_SET_DEVICES", "abilities"));
        assert!(table_matches("assembly", "DATA_SET_DEVICES", "devices"));
        assert!(table_matches("FRA", "MESSAGES", "french"));
        assert!(!table_matches("assembly", "DATA_SET_BOTS", "abilities"));
    }
}
