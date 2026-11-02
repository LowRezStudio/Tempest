//! Champion skins as the client sells and lists them (8.1): loot table 1076
//! "Inventory Vendor - Collections". Each entry is a **collection**: its
//! `ITEM_ID` is a container item (`ITEM_TYPE_VALUE_ID` 1634, `REF_BOT_ID` =
//! the champion, named like the skin, "Obsidian Evie") and its
//! `SUB_LOOT_TABLE_ID` a loot table of type 1455 ("SLT - Collection - Evie -
//! Obsidian") holding the parts: body skin (10230), head (15170), weapon
//! (15041), sometimes a voice pack, and often an entitlement item (11403,
//! "ENT Evie Mastery Obsidian", also the entry's `REQUIRED_ITEM_ID` on some
//! entries; its `DATA_SET_ITEM_PROPS` prop 2143 holds the entry's
//! `LOOT_TABLE_ITEM_ID` and its `REF_ITEM_ID` a container of the set).
//!
//! The native `UIDataChampions` code of the 8.1 client reads table 0x434
//! from the assembly and groups its entries per champion
//! (`m_PerChampionCollections`), and resolves a collection item to its skin
//! through the entry's sub loot table, picking the item of type 10230
//! (`sub_141595550`, `sub_1414c6770`). Default skins are not in 1076 (they
//! sit in "SLT - Collection/Bundle - <champion> - Default" tables).

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, Result};

use super::workspace::Workspace;
use super::{Row, Value};

/// "Inventory Vendor - Collections".
pub const COLLECTION_VENDOR: u64 = 1076;
const CONTAINER: u64 = 1634;
const SKIN: u64 = 10230;
const ENTITLEMENT: u64 = 11403;
/// `DATA_SET_ITEM_PROPS` prop of an entitlement naming its vendor entry.
const PROP_VENDOR_ENTRY: u64 = 2143;
/// `LOOT_TABLE_TYPE_VALUE_ID` of a collection's sub loot table.
const SUB_TABLE_TYPE: u64 = 1455;

/// What an item type is called in the dialogs.
fn part_kind(ty: u64) -> &'static str {
    match ty {
        SKIN => "skin",
        15170 => "head",
        15041 => "weapon",
        10637 => "voice",
        ENTITLEMENT => "entitlement",
        CONTAINER => "collection",
        _ => "item",
    }
}

/// `old` with `from` replaced by `to` (whole words, case kept as typed), or
/// None when `from` does not occur.
fn swap(text: &str, from: &str, to: &str) -> Option<String> {
    if from.is_empty() || !text.contains(from) {
        return None;
    }
    Some(text.replace(from, to).split_whitespace().collect::<Vec<_>>().join(" "))
}

/// The theme of a collection name: the name without the champion's ("Obsidian
/// Evie" → "Obsidian").
fn theme(name: &str, champion: &str) -> String {
    if champion.is_empty() {
        return name.to_string();
    }
    name.replace(champion, " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A part name for the new collection: the template's theme and champion
/// swapped for the new ones ("Obsidian Pigtails" → "Frost Pigtails"), else
/// the new name before the old text.
pub fn rename(text: &str, old_theme: &str, new_theme: &str, old_champ: &str, new_champ: &str) -> String {
    let mut out = text.to_string();
    let mut changed = false;
    if old_theme != new_theme {
        if let Some(s) = swap(&out, old_theme, new_theme) {
            out = s;
            changed = true;
        }
    } else {
        changed = true;
    }
    if !old_champ.is_empty() && old_champ != new_champ {
        if let Some(s) = swap(&out, old_champ, new_champ) {
            out = s;
            changed = true;
        }
    }
    if changed {
        out
    } else {
        format!("{new_theme} {text}").trim().to_string()
    }
}

struct Lookup<'a> {
    items: HashMap<u64, (usize, &'a Row)>,
    tables: HashMap<u64, (usize, &'a Row)>,
    bots: HashMap<u64, (usize, &'a Row)>,
    messages: HashMap<u64, &'a str>,
}

struct Entry {
    row: Row,
}

impl Workspace {
    fn num_of(&self, row: &Row, field: &str) -> Option<u64> {
        row.num(self.tokens.token(field)?)
    }

    /// The 1076 table's row index and its entries.
    fn collection_vendor(&self) -> Result<(usize, Vec<Row>)> {
        let ti = self.find_row("DATA_SET_LOOT_TABLES", "LOOT_TABLE_ID", COLLECTION_VENDOR)?.ok_or_else(|| Error::Format("this assembly has no loot table 1076 (Inventory Vendor - Collections)".into()))?;
        let entries = self.row_of("DATA_SET_LOOT_TABLES", ti)?.rows(self.t("DATA_SET_LOOT_TABLE_ITEMS")?).cloned().unwrap_or_default();
        Ok((ti, entries))
    }

    fn vendor_entry(&self, item: u64) -> Result<(usize, Entry)> {
        let (ti, entries) = self.collection_vendor()?;
        let item_t = self.t("ITEM_ID")?;
        let index = entries.iter().position(|e| e.num(item_t) == Some(item)).ok_or_else(|| Error::Format(format!("item {item} is not a collection of loot table 1076")))?;
        Ok((ti, Entry { row: entries[index].clone() }))
    }

    fn item_row(&self, id: u64) -> Result<Option<Row>> {
        Ok(match self.find_row("DATA_SET_ITEMS", "ITEM_ID", id)? {
            Some(i) => Some(self.row_of("DATA_SET_ITEMS", i)?),
            None => None,
        })
    }

    fn item_name(&self, row: &Row) -> String {
        self.message_text(self.num_of(row, "NAME_MSG_ID")).unwrap_or_default()
    }

    /// The champion name (INT) of a bot.
    fn bot_name(&self, bot: Option<u64>) -> String {
        let Some(bot) = bot else { return String::new() };
        let Ok(Some(i)) = self.find_row("DATA_SET_BOTS", "BOT_ID", bot) else { return String::new() };
        let Ok(row) = self.row_of("DATA_SET_BOTS", i) else { return String::new() };
        self.message_text(self.num_of(&row, "NAME_MSG_ID"))
            .or_else(|| self.tokens.token("REFERENCE_NAME").and_then(|t| row.str(t)).map(|s| s.to_string()))
            .unwrap_or_default()
    }

    /// The entries of a loot table by id, with its row index.
    fn loot_entries(&self, table: u64) -> Result<Option<(usize, Row)>> {
        Ok(match self.find_row("DATA_SET_LOOT_TABLES", "LOOT_TABLE_ID", table)? {
            Some(i) => Some((i, self.row_of("DATA_SET_LOOT_TABLES", i)?)),
            None => None,
        })
    }

    fn parts_json(&self, sub: Option<&Row>) -> Result<Vec<J>> {
        let Some(sub) = sub else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for e in sub.rows(self.t("DATA_SET_LOOT_TABLE_ITEMS")?).map(|v| v.as_slice()).unwrap_or(&[]) {
            let Some(id) = self.num_of(e, "ITEM_ID") else { continue };
            let row = self.item_row(id)?;
            let ty = row.as_ref().and_then(|r| self.num_of(r, "ITEM_TYPE_VALUE_ID")).unwrap_or(0);
            out.push(json!({
                "item": id,
                "type": ty,
                "kind": part_kind(ty),
                "name": row.as_ref().map(|r| self.item_name(r)).unwrap_or_default(),
                "exists": row.is_some(),
            }));
        }
        Ok(out)
    }

    /// Lookups the listing needs, built once (the per-row finders scan).
    fn lookup(&self) -> Result<Lookup<'_>> {
        let doc = self.doc("assembly")?;
        let index = |table: &str, key: &str| -> Result<HashMap<u64, (usize, &Row)>> {
            let k = self.t(key)?;
            Ok(doc.marshal.root.rows(self.t(table)?).map(|v| v.as_slice()).unwrap_or(&[]).iter().enumerate().filter_map(|(i, r)| Some((r.num(k)?, (i, r)))).collect())
        };
        let mut messages = HashMap::new();
        if let (Ok(int), Some(set), Some(id_t), Some(text_t)) = (self.doc("INT"), self.tokens.token("DATA_SET"), self.tokens.token("MSG_ID"), self.tokens.token("MESSAGE")) {
            for r in int.marshal.root.rows(set).map(|v| v.as_slice()).unwrap_or(&[]) {
                if let (Some(id), Some(text)) = (r.num(id_t), r.str(text_t)) {
                    messages.insert(id, text);
                }
            }
        }
        Ok(Lookup { items: index("DATA_SET_ITEMS", "ITEM_ID")?, tables: index("DATA_SET_LOOT_TABLES", "LOOT_TABLE_ID")?, bots: index("DATA_SET_BOTS", "BOT_ID")?, messages })
    }

    /// `mcts.collections {bot?}`: the collections of loot table 1076 (all, or
    /// one champion's) with their parts, and the champion skins (type 10230)
    /// no collection holds.
    pub fn collections(&self, p: &J) -> Result<J> {
        let only = p.get("bot").and_then(|v| v.as_u64());
        let (ti, entries) = self.collection_vendor()?;
        let lk = self.lookup()?;
        let t = |n: &str| self.t(n);
        let (active_t, item_t, ref_bot_t, type_t, name_t, sub_t, entry_t, req_t) = (t("ACTIVE_FLAG")?, t("ITEM_ID")?, t("REF_BOT_ID")?, t("ITEM_TYPE_VALUE_ID")?, t("NAME_MSG_ID")?, t("SUB_LOOT_TABLE_ID")?, t("LOOT_TABLE_ITEM_ID")?, t("REQUIRED_ITEM_ID")?);
        let entries_t = t("DATA_SET_LOOT_TABLE_ITEMS")?;
        let name_of = |r: &Row| r.num(name_t).and_then(|m| lk.messages.get(&m)).map(|s| s.to_string()).unwrap_or_default();
        let ref_name_t = self.tokens.token("REFERENCE_NAME");
        let bot_name = |b: u64| lk.bots.get(&b).map(|(_, r)| {
            let n = name_of(r);
            if n.is_empty() { ref_name_t.and_then(|t| r.str(t)).unwrap_or("").to_string() } else { n }
        }).unwrap_or_default();
        let mut held: HashSet<u64> = HashSet::new();
        let mut list = Vec::new();
        let mut bots: HashMap<u64, String> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            let item = e.num(item_t).unwrap_or(0);
            let row = lk.items.get(&item);
            let bot = row.and_then(|(_, r)| r.num(ref_bot_t));
            let sub_id = e.num(sub_t);
            let sub = sub_id.and_then(|s| lk.tables.get(&s));
            let mut parts = Vec::new();
            for pe in sub.and_then(|(_, r)| r.rows(entries_t)).map(|v| v.as_slice()).unwrap_or(&[]) {
                let Some(id) = pe.num(item_t) else { continue };
                held.insert(id);
                let pr = lk.items.get(&id);
                let ty = pr.and_then(|(_, r)| r.num(type_t)).unwrap_or(0);
                parts.push(json!({ "item": id, "type": ty, "kind": part_kind(ty), "name": pr.map(|(_, r)| name_of(r)).unwrap_or_default(), "exists": pr.is_some() }));
            }
            if only.is_some() && bot != only {
                continue;
            }
            if let Some(b) = bot {
                bots.entry(b).or_insert_with(|| bot_name(b));
            }
            list.push(json!({
                "path": [["DATA_SET_LOOT_TABLES", ti], ["DATA_SET_LOOT_TABLE_ITEMS", i]],
                "entry": e.num(entry_t),
                "item": item,
                "itemPath": row.map(|(r, _)| json!([["DATA_SET_ITEMS", r]])),
                "bot": bot,
                "name": row.map(|(_, r)| name_of(r)).unwrap_or_default(),
                "active": e.str(active_t) == Some("y"),
                "subTable": sub_id,
                "subPath": sub.map(|(i, _)| json!([["DATA_SET_LOOT_TABLES", i]])),
                "parts": parts,
                "required": e.num(req_t),
            }));
        }
        // Skins that no collection holds.
        let mut loose = Vec::new();
        let obt_t = self.tokens.token("OBTAINABILITY_VALUE_ID");
        for (i, r) in self.doc("assembly")?.marshal.root.rows(t("DATA_SET_ITEMS")?).map(|v| v.as_slice()).unwrap_or(&[]).iter().enumerate() {
            if r.num(type_t) != Some(SKIN) {
                continue;
            }
            let Some(id) = r.num(item_t) else { continue };
            let bot = r.num(ref_bot_t);
            if held.contains(&id) || (only.is_some() && bot != only) {
                continue;
            }
            if let Some(b) = bot {
                bots.entry(b).or_insert_with(|| bot_name(b));
            }
            loose.push(json!({ "item": id, "bot": bot, "name": name_of(r), "obtainability": obt_t.and_then(|t| r.num(t)), "path": [["DATA_SET_ITEMS", i]] }));
        }
        let bots: serde_json::Map<String, J> = bots.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        Ok(json!({
            "table": COLLECTION_VENDOR,
            "index": ti,
            "total": entries.len(),
            "active": entries.iter().filter(|e| e.str(active_t) == Some("y")).count(),
            "collections": list,
            "loose": loose,
            "bots": bots,
        }))
    }

    /// `mcts.collection {item}`: one collection, with the names a new one
    /// made from it would give its parts (`suggest(name, bot)` runs in the
    /// Studio from `theme` and `champion`).
    pub fn collection(&self, p: &J) -> Result<J> {
        let item = p.get("item").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("missing item".into()))?;
        let (_, entry) = self.vendor_entry(item)?;
        let row = self.item_row(item)?.ok_or_else(|| Error::Format(format!("no item {item}")))?;
        let bot = self.num_of(&row, "REF_BOT_ID");
        let name = self.item_name(&row);
        let champion = self.bot_name(bot);
        let sub = match self.num_of(&entry.row, "SUB_LOOT_TABLE_ID") {
            Some(s) => self.loot_entries(s)?,
            None => None,
        };
        let sub_name = sub.as_ref().and_then(|(_, r)| self.message_text(self.num_of(r, "NAME_MSG_ID"))).unwrap_or_default();
        Ok(json!({
            "item": item,
            "bot": bot,
            "name": name,
            "champion": champion,
            "theme": theme(&name, &champion),
            "subTable": sub.as_ref().and_then(|(_, r)| self.num_of(r, "LOOT_TABLE_ID")),
            "subName": sub_name,
            "parts": self.parts_json(sub.as_ref().map(|(_, r)| r))?,
        }))
    }

    /// A fresh `LOOT_TABLE_ITEM_ID` counter and `LOOT_TABLE_ID`.
    fn loot_ids(&self) -> Result<(u64, u64)> {
        let entry = self.after(self.max_nested("DATA_SET_LOOT_TABLES", "LOOT_TABLE_ITEM_ID")?)?;
        let table = self.after(self.max_of("DATA_SET_LOOT_TABLES", "LOOT_TABLE_ID")?)?;
        Ok((entry, table))
    }

    /// Point an entitlement copy at its new collection: `REF_ITEM_ID` and the
    /// prop naming the vendor entry.
    fn retarget_entitlement(&mut self, item: u64, collection: u64, entry_id: u64, old_entry_id: Option<u64>) -> Result<()> {
        let ii = self.find_row("DATA_SET_ITEMS", "ITEM_ID", item)?.ok_or_else(|| Error::Format(format!("no item {item}")))?;
        let items_t = self.t("DATA_SET_ITEMS")?;
        let path = vec![(items_t, ii)];
        let tokens = self.tokens.clone();
        let ref_item_t = self.t("REF_ITEM_ID")?;
        let row = self.row_of("DATA_SET_ITEMS", ii)?;
        if row.num(ref_item_t).is_some() {
            self.doc_mut("assembly")?.set(&tokens, &path, ref_item_t, &json!(collection))?;
        }
        let props_t = self.t("DATA_SET_ITEM_PROPS")?;
        let prop_t = self.t("PROP_ID")?;
        let value_t = self.t("VALUE")?;
        if let Some(props) = row.rows(props_t) {
            for (k, pr) in props.iter().enumerate() {
                if pr.num(prop_t) != Some(PROP_VENDOR_ENTRY) {
                    continue;
                }
                let current = match value_json_f64(&tokens, value_t, pr.get(value_t)) {
                    Some(v) => v as u64,
                    None => 0,
                };
                if old_entry_id.is_none() || old_entry_id == Some(current) {
                    let mut p = path.clone();
                    p.push((props_t, k));
                    self.doc_mut("assembly")?.set(&tokens, &p, value_t, &json!(entry_id as f64))?;
                }
            }
        }
        Ok(())
    }

    /// Append a sub loot table (type 1455) holding `items`, shaped like
    /// `like` (its entries are the templates by position, else its last
    /// one); its path and id.
    fn add_sub_table(&mut self, like: Option<&Row>, table_id: u64, name: &str, items: &[(u64, Option<usize>)], next_entry: &mut u64) -> Result<(usize, u64)> {
        let entries_t = self.t("DATA_SET_LOOT_TABLE_ITEMS")?;
        let table_t = self.t("LOOT_TABLE_ID")?;
        let item_t = self.t("ITEM_ID")?;
        let entry_t = self.t("LOOT_TABLE_ITEM_ID")?;
        let active_t = self.t("ACTIVE_FLAG")?;
        let templates: Vec<Row> = like.and_then(|r| r.rows(entries_t).cloned()).unwrap_or_default();
        let mut row = match like {
            Some(r) => {
                let mut r = r.clone();
                r.entries.retain(|(t, _)| *t != entries_t);
                r
            }
            None => {
                let mut r = Row::default();
                r.set(self.t("LOOT_TABLE_TYPE_VALUE_ID")?, Value::Num(SUB_TABLE_TYPE));
                r
            }
        };
        Self::set_num(&mut row, table_t, table_id);
        let msg = self.next_ids()?.msg;
        let mut texts = HashMap::new();
        texts.insert("INT".to_string(), name.to_string());
        self.new_message(msg, "Loot_Table_name", &texts)?;
        Self::set_num(&mut row, self.t("NAME_MSG_ID")?, msg);
        let mut out = Vec::new();
        for (item, from) in items {
            let mut e = match from.and_then(|i| templates.get(i)).or(templates.last()) {
                Some(t) => t.clone(),
                None => {
                    let mut r = Row::default();
                    r.set(self.t("QUANTITY")?, Value::Num(1));
                    r.set(active_t, Value::Str("y".into()));
                    r
                }
            };
            Self::set_num(&mut e, table_t, table_id);
            Self::set_num(&mut e, item_t, *item);
            Self::set_num(&mut e, entry_t, *next_entry);
            *next_entry += 1;
            e.set(active_t, Value::Str("y".into()));
            out.push(e);
        }
        row.set(entries_t, Value::Rows(out));
        let at = self.add_row("DATA_SET_LOOT_TABLES", row)?;
        Ok((at, table_id))
    }

    /// Append an entry to 1076 shaped like `like` (or the last active entry).
    fn add_collection_entry(&mut self, like: Option<&Row>, item: u64, sub: u64, required: Option<u64>, entry_id: u64) -> Result<J> {
        let (ti, entries) = self.collection_vendor()?;
        let tables_t = self.t("DATA_SET_LOOT_TABLES")?;
        let entries_t = self.t("DATA_SET_LOOT_TABLE_ITEMS")?;
        let active_t = self.t("ACTIVE_FLAG")?;
        let required_t = self.t("REQUIRED_ITEM_ID")?;
        let own_msgs: Vec<u16> = ["NAME_MSG_ID", "DESC_MSG_ID", "SHORT_DESC_MSG_ID"].iter().filter_map(|f| self.tokens.token(f)).collect();
        let mut row = like.cloned().or_else(|| entries.iter().rev().find(|e| e.str(active_t) == Some("y")).cloned()).unwrap_or_default();
        row.entries.retain(|(t, _)| !own_msgs.contains(t));
        Self::set_num(&mut row, self.t("LOOT_TABLE_ID")?, COLLECTION_VENDOR);
        Self::set_num(&mut row, self.t("ITEM_ID")?, item);
        Self::set_num(&mut row, self.t("SUB_LOOT_TABLE_ID")?, sub);
        Self::set_num(&mut row, self.t("LOOT_TABLE_ITEM_ID")?, entry_id);
        match required {
            Some(r) => Self::set_num(&mut row, required_t, r),
            None => row.entries.retain(|(t, _)| *t != required_t),
        }
        row.set(active_t, Value::Str("y".into()));
        let tokens = self.tokens.clone();
        let at = self.doc_mut("assembly")?.insert_row(&tokens, &[(tables_t, ti)], entries_t, row)?;
        Ok(json!([["DATA_SET_LOOT_TABLES", ti], ["DATA_SET_LOOT_TABLE_ITEMS", at]]))
    }

    /// `mcts.create {kind: "skin"}`: a new collection cloned from `template`
    /// (a collection item of 1076): the container item, a copy of every part
    /// (skin rows included) named from `names` (`{"<old item id>": "name"}`)
    /// or by swapping the theme and champion, a new sub loot table and the
    /// 1076 entry; every part on `bot` when given.
    pub(super) fn create_skin(&mut self, template: u64, name: &str, bot: Option<u64>, names: &HashMap<u64, String>, ids: &mut super::create::NextIds) -> Result<J> {
        let (_, entry) = self.vendor_entry(template)?;
        let coll = self.item_row(template)?.ok_or_else(|| Error::Format(format!("no item {template}")))?;
        let old_bot = self.num_of(&coll, "REF_BOT_ID");
        let old_name = self.item_name(&coll);
        let old_champ = self.bot_name(old_bot);
        let new_champ = if bot.is_some() && bot != old_bot { self.bot_name(bot) } else { old_champ.clone() };
        let old_theme = theme(&old_name, &old_champ);
        let new_theme = theme(name, &new_champ);
        let rename = |text: &str| rename(text, &old_theme, &new_theme, &old_champ, &new_champ);
        let sub = self.num_of(&entry.row, "SUB_LOOT_TABLE_ID").map(|s| self.loot_entries(s)).transpose()?.flatten();
        let (mut next_entry, table_id) = self.loot_ids()?;
        let entry_id = next_entry;
        next_entry += 1;
        let old_entry_id = self.num_of(&entry.row, "LOOT_TABLE_ITEM_ID");

        // The container.
        let made = self.create_item(template, name, "", "", bot, None, ids)?;
        let collection = made["itemId"].as_u64().unwrap_or(0);
        let mut parts = vec![json!({ "from": template, "item": collection, "kind": "collection", "name": name, "path": made["item"] })];

        // The parts, in sub table order.
        let entries_t = self.t("DATA_SET_LOOT_TABLE_ITEMS")?;
        let sub_entries: Vec<Row> = sub.as_ref().and_then(|(_, r)| r.rows(entries_t).cloned()).unwrap_or_default();
        let mut map: HashMap<u64, u64> = HashMap::new();
        map.insert(template, collection);
        let mut new_items: Vec<(u64, Option<usize>)> = Vec::new();
        for (k, e) in sub_entries.iter().enumerate() {
            let Some(old) = self.num_of(e, "ITEM_ID") else { continue };
            if let Some(n) = map.get(&old) {
                new_items.push((*n, Some(k)));
                continue;
            }
            let Some(row) = self.item_row(old)? else {
                // A part missing from the items: keep the reference.
                new_items.push((old, Some(k)));
                continue;
            };
            let ty = self.num_of(&row, "ITEM_TYPE_VALUE_ID").unwrap_or(0);
            let text = names.get(&old).cloned().unwrap_or_else(|| rename(&self.item_name(&row)));
            let made = self.create_item(old, &text, "", "", bot, None, ids)?;
            let id = made["itemId"].as_u64().unwrap_or(0);
            if ty == ENTITLEMENT {
                self.retarget_entitlement(id, collection, entry_id, old_entry_id)?;
            }
            map.insert(old, id);
            new_items.push((id, Some(k)));
            parts.push(json!({ "from": old, "item": id, "kind": part_kind(ty), "name": text, "path": made["item"], "skin": made["skin"] }));
        }
        let sub_name = sub.as_ref().and_then(|(_, r)| self.message_text(self.num_of(r, "NAME_MSG_ID"))).map(|s| rename(&s)).unwrap_or_else(|| format!("SLT - Collection - {new_champ} - {new_theme}"));
        let (sub_at, _) = self.add_sub_table(sub.as_ref().map(|(_, r)| r), table_id, &sub_name, &new_items, &mut next_entry)?;
        let required = self.num_of(&entry.row, "REQUIRED_ITEM_ID").map(|r| map.get(&r).copied().unwrap_or(r));
        let vendor = self.add_collection_entry(Some(&entry.row), collection, table_id, required, entry_id)?;
        Ok(json!({
            "itemId": collection,
            "collection": made_path(&parts),
            "item": made_path(&parts),
            "parts": parts,
            "subTable": table_id,
            "subPath": [["DATA_SET_LOOT_TABLES", sub_at]],
            "vendor": vendor,
            "messages": [],
        }))
    }

    /// `mcts.collectionAdd {items, name, like?}`: a collection around existing
    /// items (skins without one, say): a container named `name` on the first
    /// item's champion (a copy of `like`'s container, else of a collection of
    /// that champion, else of any), a sub loot table holding the items and the
    /// 1076 entry. No entitlement is made.
    pub fn collection_add(&mut self, p: &J) -> Result<J> {
        let items: Vec<u64> = p.get("items").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
        if items.is_empty() {
            return Err(Error::Format("no items".into()));
        }
        let first = self.item_row(items[0])?.ok_or_else(|| Error::Format(format!("no item {}", items[0])))?;
        let bot = self.num_of(&first, "REF_BOT_ID");
        let name = p.get("name").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| self.item_name(&first));
        let (_, entries) = self.collection_vendor()?;
        let item_t = self.t("ITEM_ID")?;
        let active_t = self.t("ACTIVE_FLAG")?;
        let like = p.get("like").and_then(|v| v.as_u64());
        let mut template: Option<Row> = like.and_then(|l| entries.iter().find(|e| e.num(item_t) == Some(l)).cloned());
        if template.is_none() {
            for e in entries.iter().rev().filter(|e| e.str(active_t) == Some("y")) {
                let Some(id) = e.num(item_t) else { continue };
                if self.item_row(id)?.and_then(|r| self.num_of(&r, "REF_BOT_ID")) == bot {
                    template = Some(e.clone());
                    break;
                }
            }
        }
        let template = template.or_else(|| entries.iter().rev().find(|e| e.str(active_t) == Some("y")).cloned()).ok_or_else(|| Error::Format("loot table 1076 has no entry to copy".into()))?;
        let container = self.num_of(&template, "ITEM_ID").unwrap_or(0);
        let mut ids = self.next_ids()?;
        let made = self.create_item(container, &name, "", "", bot, None, &mut ids)?;
        let collection = made["itemId"].as_u64().unwrap_or(0);
        let (mut next_entry, table_id) = self.loot_ids()?;
        let entry_id = next_entry;
        next_entry += 1;
        let like_sub = self.num_of(&template, "SUB_LOOT_TABLE_ID").map(|s| self.loot_entries(s)).transpose()?.flatten();
        let champion = self.bot_name(bot);
        let sub_name = format!("SLT - Collection - {champion} - {}", theme(&name, &champion));
        let list: Vec<(u64, Option<usize>)> = items.iter().map(|i| (*i, None)).collect();
        // Entries copy the template's first non-entitlement part.
        let first_part = like_sub.as_ref().and_then(|(_, r)| {
            r.rows(self.tokens.token("DATA_SET_LOOT_TABLE_ITEMS")?).and_then(|rows| {
                rows.iter().position(|e| self.num_of(e, "ITEM_ID").and_then(|id| self.item_row(id).ok().flatten()).and_then(|r| self.num_of(&r, "ITEM_TYPE_VALUE_ID")) != Some(ENTITLEMENT))
            })
        });
        let list: Vec<(u64, Option<usize>)> = list.into_iter().map(|(i, _)| (i, first_part)).collect();
        let (sub_at, _) = self.add_sub_table(like_sub.as_ref().map(|(_, r)| r), table_id, &sub_name, &list, &mut next_entry)?;
        let vendor = self.add_collection_entry(Some(&template), collection, table_id, None, entry_id)?;
        Ok(json!({ "itemId": collection, "item": made["item"], "subTable": table_id, "subPath": [["DATA_SET_LOOT_TABLES", sub_at]], "vendor": vendor }))
    }
}

fn made_path(parts: &[J]) -> J {
    parts.first().map(|p| p["path"].clone()).unwrap_or(J::Null)
}

/// A float/number field's value as f64.
fn value_json_f64(tokens: &super::Tokens, token: u16, v: Option<&Value>) -> Option<f64> {
    super::value_json(tokens, token, v?).as_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_parts() {
        assert_eq!(rename("Obsidian Pigtails", "Obsidian", "Frost", "Evie", "Evie"), "Frost Pigtails");
        assert_eq!(rename("ENT Evie Mastery Obsidian", "Obsidian", "Frost", "Evie", "Evie"), "ENT Evie Mastery Frost");
        assert_eq!(rename("Ice Staff", "Obsidian", "Frost", "Evie", "Evie"), "Frost Ice Staff");
        assert_eq!(rename("SLT - Collection - Evie - Obsidian", "Obsidian", "Frost", "Evie", "Maeve"), "SLT - Collection - Maeve - Frost");
        assert_eq!(theme("Obsidian Evie", "Evie"), "Obsidian");
    }
}
