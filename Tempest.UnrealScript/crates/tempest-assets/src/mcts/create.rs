//! New content for the game: devices (abilities, weapons, loadout cards and
//! talents are all devices), cosmetic items, and whole champions, created
//! by cloning an existing row with fresh ids and new messages in every
//! language; the rows the private server needs for a champion; and a mod
//! folder with the changed `.dat` files plus a Lua / UnrealScript scaffold
//! for the behaviour of the new devices.
//!
//! What the data model is (8.1): a device is a `DATA_SET_DEVICES` row and a
//! `DATA_SET_ITEMS` row with the same id, pointing at its champion with
//! `REF_BOT_ID`, in a slot (`SLOT_USED_VALUE_ID`: primary weapon, Q, E, F,
//! alt fire, card, talent…) and naming its UnrealScript classes through
//! `FORM_CLASS_RES_ID` / `DEVICE_FORM_CLASS_RES_ID` (resources). Its
//! numbers live in `DATA_SET_DEVICE_MODES` (properties, effect groups). A
//! champion is a `DATA_SET_BOTS` row with one device per slot in
//! `DATA_SET_BOT_DEVICES`, a Character Class item, a Weapon item per
//! weapon, skins and the rest, all on its `REF_BOT_ID`. Names and
//! descriptions are message ids into the language files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, Result};

use super::workspace::Workspace;
use super::{Row, Value};

/// The ids a new row of each kind gets: one past the largest in use.
pub struct NextIds {
    pub item: u64,
    pub device: u64,
    pub bot: u64,
    pub res: u64,
    pub icon: u64,
    pub fx: u64,
    pub msg: u64,
    pub device_mode: u64,
    pub skin: u64,
}

impl Workspace {
    fn t(&self, name: &str) -> Result<u16> {
        self.tokens.token(name).ok_or_else(|| Error::Format(format!("unknown field {name}")))
    }

    /// Does an id count when looking for the largest one in use? With an id
    /// range set, only ids inside it do: ids another mod allocated outside
    /// the range leave this mod's numbering alone.
    fn counts(&self, n: u64) -> bool {
        match self.id_range {
            Some((start, end)) => n >= start && n < end,
            None => true,
        }
    }

    /// The id after `max`: `max + 1`, or the start of the id range when nothing
    /// in the range is used yet; an error when the range is exhausted.
    fn after(&self, max: u64) -> Result<u64> {
        let next = match self.id_range {
            Some((start, _)) => (max + 1).max(start),
            None => max + 1,
        };
        if let Some((start, end)) = self.id_range {
            if next >= end {
                return Err(Error::Format(format!("the id range {start}..{end} is used up; widen it in the settings")));
            }
        }
        Ok(next)
    }

    /// Largest value of `key` in a top-level table of the assembly.
    fn max_of(&self, table: &str, key: &str) -> Result<u64> {
        let doc = self.doc("assembly")?;
        let rows = doc.marshal.root.rows(self.t(table)?).ok_or_else(|| Error::Format(format!("no {table}")))?;
        let k = self.t(key)?;
        Ok(rows.iter().filter_map(|r| r.num(k)).filter(|&n| self.counts(n)).max().unwrap_or(0))
    }

    /// Largest value of a field anywhere inside a table (nested row sets included).
    fn max_nested(&self, table: &str, key: &str) -> Result<u64> {
        fn walk(row: &Row, k: u16, best: &mut u64, counts: &dyn Fn(u64) -> bool) {
            for (t, v) in &row.entries {
                match v {
                    Value::Num(n) if *t == k && counts(*n) => *best = (*best).max(*n),
                    Value::Rows(rows) => rows.iter().for_each(|r| walk(r, k, best, counts)),
                    _ => {}
                }
            }
        }
        let doc = self.doc("assembly")?;
        let rows = doc.marshal.root.rows(self.t(table)?).ok_or_else(|| Error::Format(format!("no {table}")))?;
        let k = self.t(key)?;
        let mut best = 0;
        rows.iter().for_each(|r| walk(r, k, &mut best, &|n| self.counts(n)));
        Ok(best)
    }

    pub fn next_ids(&mut self) -> Result<NextIds> {
        // Messages: past the largest id of every language, and the 1e9+ ids
        // the game uses for valid values and system messages stay untouched.
        let msg_id = self.t("MSG_ID")?;
        let mut msg = 0u64;
        for (id, doc) in &self.docs {
            if id == "assembly" {
                continue;
            }
            for set in ["DATA_SET_MSG_TRANSLATIONS", "DATA_SET"] {
                if let Some(rows) = self.tokens.token(set).and_then(|t| doc.marshal.root.rows(t)) {
                    msg = msg.max(rows.iter().filter_map(|r| r.num(msg_id)).filter(|&v| v < 1_000_000_000 && self.counts(v)).max().unwrap_or(0));
                }
            }
        }
        // Items and devices share one id space (a device's item has its id).
        let item = self.after(self.max_of("DATA_SET_ITEMS", "ITEM_ID")?.max(self.max_of("DATA_SET_DEVICES", "DEVICE_ID")?).max(self.max_of("DATA_SET_SKINS", "SKIN_ID")?))?;
        Ok(NextIds {
            item,
            device: item,
            skin: item,
            bot: self.after(self.max_of("DATA_SET_BOTS", "BOT_ID")?)?,
            res: self.after(self.max_of("DATA_SET_RESOURCES", "RES_ID")?)?,
            icon: self.after(self.max_of("DATA_SET_ICONS", "ICON_ID")?)?,
            fx: self.after(self.max_of("DATA_SET_SPECIAL_FX", "FX_ID")?)?,
            msg: self.after(msg)?,
            device_mode: self.after(self.max_nested("DATA_SET_DEVICES", "DEVICE_MODE_ID")?)?,
        })
    }

    pub fn next_ids_json(&mut self) -> Result<J> {
        let n = self.next_ids()?;
        let range = self.id_range.map(|(s, e)| json!({ "start": s, "end": e }));
        Ok(json!({ "item": n.item, "device": n.device, "bot": n.bot, "res": n.res, "icon": n.icon, "fx": n.fx, "msg": n.msg, "deviceMode": n.device_mode, "skin": n.skin, "range": range }))
    }

    /// `mcts.idRange {start, end}`: new ids come from `start..end` (a per-mod
    /// range, so two mods made with the Studio do not collide); `null` clears it.
    pub fn set_id_range(&mut self, p: &J) -> Result<J> {
        let start = p.get("start").and_then(|v| v.as_u64());
        let end = p.get("end").and_then(|v| v.as_u64());
        self.id_range = match (start, end) {
            (Some(s), Some(e)) if e > s => Some((s, e)),
            (None, None) => None,
            _ => return Err(Error::Format("id range needs start < end".into())),
        };
        self.next_ids_json()
    }

    /// Create a message in every language (`texts` by language, INT text for
    /// the rest); the id used.
    fn new_message(&mut self, id: u64, name: &str, texts: &HashMap<String, String>) -> Result<u64> {
        let langs: Vec<String> = self.docs.iter().map(|(k, _)| k.clone()).filter(|k| k != "assembly").collect();
        let default = texts.get("INT").cloned().or_else(|| texts.values().next().cloned()).unwrap_or_default();
        for lang in langs {
            let text = texts.get(&lang).cloned().unwrap_or_else(|| default.clone());
            self.set_message(&json!({ "lang": lang, "id": id, "text": text, "name": name }))?;
        }
        Ok(id)
    }

    /// Clone a row set entry (nested rows included) replacing every value of
    /// `rewrite` fields and allocating fresh `*_MODE_ID`-style keys.
    fn clone_row(&self, row: &Row, rewrite: &HashMap<u16, u64>, fresh: &mut HashMap<u16, u64>) -> Row {
        let mut out = Row::default();
        for (t, v) in &row.entries {
            let nv = match v {
                Value::Num(n) => {
                    if let Some(r) = rewrite.get(t) {
                        Value::Num(*r)
                    } else if let Some(next) = fresh.get_mut(t) {
                        let id = *next;
                        *next += 1;
                        let _ = n;
                        Value::Num(id)
                    } else {
                        Value::Num(*n)
                    }
                }
                Value::Rows(rows) => Value::Rows(rows.iter().map(|r| self.clone_row(r, rewrite, fresh)).collect()),
                other => other.clone(),
            };
            out.entries.push((*t, nv));
        }
        out
    }

    fn find_row(&self, table: &str, key: &str, value: u64) -> Result<Option<usize>> {
        let t = self.t(table)?;
        let k = self.t(key)?;
        Ok(self.doc("assembly")?.find(&[], t, k, value)?.first().copied())
    }

    fn row_of(&self, table: &str, index: usize) -> Result<Row> {
        let t = self.t(table)?;
        Ok(self.doc("assembly")?.row_at(&[(t, index)])?.clone())
    }

    fn add_row(&mut self, table: &str, row: Row) -> Result<usize> {
        let t = self.t(table)?;
        let tokens = self.tokens.clone();
        self.doc_mut("assembly")?.insert_row(&tokens, &[], t, row)
    }

    fn set_num(row: &mut Row, t: u16, v: u64) {
        row.set(t, Value::Num(v));
    }

    /// Messages of a cloned row: NAME/DESC/SHORT_DESC get new ids with the
    /// given texts (the clone keeps the rest, e.g. LORE).
    fn name_messages(&mut self, row: &mut Row, prefix: &str, name: &str, desc: &str, short: &str, next_msg: &mut u64) -> Result<Vec<u64>> {
        let mut ids = Vec::new();
        for (field, text, suffix) in [("NAME_MSG_ID", name, "name"), ("DESC_MSG_ID", desc, "desc"), ("SHORT_DESC_MSG_ID", short, "short_desc")] {
            let t = self.t(field)?;
            if text.is_empty() {
                continue;
            }
            let id = *next_msg;
            *next_msg += 1;
            let mut texts = HashMap::new();
            texts.insert("INT".to_string(), text.to_string());
            self.new_message(id, &format!("{prefix}_{suffix}"), &texts)?;
            Self::set_num(row, t, id);
            ids.push(id);
        }
        Ok(ids)
    }

    /// Clone a device (and its item) from `template` (a DEVICE_ID).
    #[allow(clippy::too_many_arguments)]
    fn create_device(&mut self, template: u64, name: &str, desc: &str, short: &str, bot: Option<u64>, slot: Option<u64>, ids: &mut NextIds) -> Result<J> {
        let di = self.find_row("DATA_SET_DEVICES", "DEVICE_ID", template)?.ok_or_else(|| Error::Format(format!("no device {template}")))?;
        let new_id = ids.device;
        ids.device += 1;
        ids.item = ids.device;
        let mut rewrite = HashMap::new();
        rewrite.insert(self.t("DEVICE_ID")?, new_id);
        rewrite.insert(self.t("ITEM_ID")?, new_id);
        if let Some(b) = bot {
            rewrite.insert(self.t("REF_BOT_ID")?, b);
        }
        let mut fresh = HashMap::new();
        fresh.insert(self.t("DEVICE_MODE_ID")?, ids.device_mode);
        let mut device = self.clone_row(&self.row_of("DATA_SET_DEVICES", di)?, &rewrite, &mut fresh);
        ids.device_mode = fresh[&self.t("DEVICE_MODE_ID")?];
        if let Some(s) = slot {
            Self::set_num(&mut device, self.t("SLOT_USED_VALUE_ID")?, s);
        }
        let msgs = self.name_messages(&mut device, "ITEM", name, desc, short, &mut ids.msg)?;
        // The item row of the device (same id).
        let item_row = match self.find_row("DATA_SET_ITEMS", "ITEM_ID", template)? {
            Some(ii) => {
                let mut item = self.clone_row(&self.row_of("DATA_SET_ITEMS", ii)?, &rewrite, &mut HashMap::new());
                for (field, id) in [("NAME_MSG_ID", msgs.first()), ("DESC_MSG_ID", msgs.get(1)), ("SHORT_DESC_MSG_ID", msgs.get(2))] {
                    if let Some(id) = id {
                        Self::set_num(&mut item, self.t(field)?, *id);
                    }
                }
                Some(item)
            }
            None => None,
        };
        let dat = self.add_row("DATA_SET_DEVICES", device)?;
        let iat = match item_row {
            Some(r) => Some(self.add_row("DATA_SET_ITEMS", r)?),
            None => None,
        };
        Ok(json!({ "deviceId": new_id, "itemId": new_id, "messages": msgs, "device": [["DATA_SET_DEVICES", dat]], "item": iat.map(|i| json!([["DATA_SET_ITEMS", i]])) }))
    }

    /// Clone an item (and its skin row when it has one) from `template` (an ITEM_ID).
    #[allow(clippy::too_many_arguments)]
    fn create_item(&mut self, template: u64, name: &str, desc: &str, short: &str, bot: Option<u64>, item_type: Option<u64>, ids: &mut NextIds) -> Result<J> {
        let ii = self.find_row("DATA_SET_ITEMS", "ITEM_ID", template)?.ok_or_else(|| Error::Format(format!("no item {template}")))?;
        let new_id = ids.item;
        ids.item += 1;
        ids.device = ids.item;
        let mut rewrite = HashMap::new();
        rewrite.insert(self.t("ITEM_ID")?, new_id);
        rewrite.insert(self.t("SKIN_ID")?, new_id);
        if let Some(b) = bot {
            rewrite.insert(self.t("REF_BOT_ID")?, b);
        }
        let mut item = self.clone_row(&self.row_of("DATA_SET_ITEMS", ii)?, &rewrite, &mut HashMap::new());
        if let Some(t) = item_type {
            Self::set_num(&mut item, self.t("ITEM_TYPE_VALUE_ID")?, t);
        }
        let msgs = self.name_messages(&mut item, "ITEM", name, desc, short, &mut ids.msg)?;
        let skin = match self.find_row("DATA_SET_SKINS", "SKIN_ID", template)? {
            Some(si) => {
                let mut s = self.clone_row(&self.row_of("DATA_SET_SKINS", si)?, &rewrite, &mut HashMap::new());
                for (field, id) in [("NAME_MSG_ID", msgs.first()), ("DESC_MSG_ID", msgs.get(1))] {
                    if let Some(id) = id {
                        Self::set_num(&mut s, self.t(field)?, *id);
                    }
                }
                Some(s)
            }
            None => None,
        };
        let at = self.add_row("DATA_SET_ITEMS", item)?;
        let sat = match skin {
            Some(s) => Some(self.add_row("DATA_SET_SKINS", s)?),
            None => None,
        };
        Ok(json!({ "itemId": new_id, "messages": msgs, "item": [["DATA_SET_ITEMS", at]], "skin": sat.map(|i| json!([["DATA_SET_SKINS", i]])) }))
    }

    /// Clone a champion: the bot row, one new device per slot (clones of
    /// the template's), its Character Class and Weapon items and its
    /// default skin, every one on the new bot id.
    fn create_champion(&mut self, template: u64, name: &str, desc: &str, ids: &mut NextIds) -> Result<J> {
        let bi = self.find_row("DATA_SET_BOTS", "BOT_ID", template)?.ok_or_else(|| Error::Format(format!("no bot {template}")))?;
        let new_bot = ids.bot;
        ids.bot += 1;
        let bot_id_t = self.t("BOT_ID")?;
        let device_id_t = self.t("DEVICE_ID")?;
        let mut rewrite = HashMap::new();
        rewrite.insert(bot_id_t, new_bot);
        rewrite.insert(self.t("REF_BOT_ID")?, new_bot);
        let mut bot = self.clone_row(&self.row_of("DATA_SET_BOTS", bi)?, &rewrite, &mut HashMap::new());
        let msgs = self.name_messages(&mut bot, "BOT", name, desc, "", &mut ids.msg)?;
        let ref_name_t = self.t("REFERENCE_NAME")?;
        bot.set(ref_name_t, Value::Str(name.replace(' ', "")));
        // Devices: one clone per slot, the bot's device list rewritten.
        let mut devices = Vec::new();
        let mut map: HashMap<u64, u64> = HashMap::new();
        let slots: Vec<(u64, u64)> = bot
            .rows(self.t("DATA_SET_BOT_DEVICES")?)
            .map(|rows| rows.iter().filter_map(|r| Some((r.num(device_id_t)?, r.num(self.t("SLOT_USED_VALUE_ID").ok()?).unwrap_or(0)))).collect())
            .unwrap_or_default();
        for (old, _slot) in &slots {
            if self.find_row("DATA_SET_DEVICES", "DEVICE_ID", *old)?.is_none() {
                continue;
            }
            let dname = self.message_text(self.device_msg(*old, "NAME_MSG_ID")?).unwrap_or_else(|| format!("device {old}"));
            let ddesc = self.message_text(self.device_msg(*old, "DESC_MSG_ID")?).unwrap_or_default();
            let created = self.create_device(*old, &format!("{name} {dname}"), &ddesc, "", Some(new_bot), None, ids)?;
            let new_id = created["deviceId"].as_u64().unwrap_or(0);
            map.insert(*old, new_id);
            devices.push(created);
        }
        if let Some(rows) = bot.rows_mut(self.t("DATA_SET_BOT_DEVICES")?) {
            for r in rows.iter_mut() {
                if let Some(old) = r.num(device_id_t) {
                    if let Some(n) = map.get(&old) {
                        Self::set_num(r, device_id_t, *n);
                    }
                }
            }
        }
        let bat = self.add_row("DATA_SET_BOTS", bot)?;
        // Items on the template bot that make it selectable and armed.
        let mut items = Vec::new();
        let item_rows: Vec<Row> = self.doc("assembly")?.marshal.root.rows(self.t("DATA_SET_ITEMS")?).map(|r| r.to_vec()).unwrap_or_default();
        let ref_bot_t = self.t("REF_BOT_ID")?;
        let type_t = self.t("ITEM_TYPE_VALUE_ID")?;
        let ref_dev_t = self.t("REF_DEVICE_ID")?;
        let item_id_t = self.t("ITEM_ID")?;
        const CHARACTER_CLASS: u64 = 10149;
        /// Loot table "Champion Vendor": the client's champion list.
        const CHAMPION_VENDOR: u64 = 475;
        const WEAPON: u64 = 15041;
        const SKIN: u64 = 10230;
        let mut skin_done = false;
        let mut class_item: Option<u64> = None;
        let mut template_class_item: Option<u64> = None;
        for r in item_rows.iter().filter(|r| r.num(ref_bot_t) == Some(template)) {
            let ty = r.num(type_t).unwrap_or(0);
            let keep = ty == CHARACTER_CLASS || ty == WEAPON || (ty == SKIN && !skin_done);
            if !keep {
                continue;
            }
            if ty == SKIN {
                skin_done = true;
            }
            let old = r.num(item_id_t).unwrap_or(0);
            if ty == CHARACTER_CLASS {
                template_class_item = Some(old);
            }
            let iname = self.message_text(r.num(self.t("NAME_MSG_ID")?)).unwrap_or_default();
            let created = self.create_item(old, format!("{name} {iname}").trim(), "", "", Some(new_bot), None, ids)?;
            if ty == WEAPON {
                if let (Some(path), Some(old_dev)) = (created["item"].as_array(), r.num(ref_dev_t)) {
                    if let Some(n) = map.get(&old_dev) {
                        let tokens = self.tokens.clone();
                        let p = self.path(Some(&J::Array(path.clone())))?;
                        self.doc_mut("assembly")?.set(&tokens, &p, ref_dev_t, &json!(n))?;
                    }
                }
            }
            if ty == CHARACTER_CLASS {
                class_item = created["itemId"].as_u64();
            }
            items.push(created);
        }
        // The client builds its champion list (lobby, champion select) from
        // the "Champion Vendor" loot table, not from the bots: one entry per
        // Character Class item. Without an entry the champion exists but is
        // never listed.
        let vendor = match class_item {
            Some(item) => self.add_vendor_entry(CHAMPION_VENDOR, template_class_item, item)?,
            None => None,
        };
        Ok(json!({ "botId": new_bot, "messages": msgs, "bot": [["DATA_SET_BOTS", bat]], "devices": devices, "items": items, "vendor": vendor }))
    }

    /// Add `item` to loot table `table` (`DATA_SET_LOOT_TABLE_ITEMS`), as a
    /// copy of `like`'s entry when that item is in the table (else of the
    /// last entry), active, with a fresh LOOT_TABLE_ITEM_ID and the next
    /// SORT_ORDER. The path of the new entry, or None when the table does
    /// not exist.
    fn add_vendor_entry(&mut self, table: u64, like: Option<u64>, item: u64) -> Result<Option<J>> {
        let Some(ti) = self.find_row("DATA_SET_LOOT_TABLES", "LOOT_TABLE_ID", table)? else { return Ok(None) };
        let tables_t = self.t("DATA_SET_LOOT_TABLES")?;
        let entries_t = self.t("DATA_SET_LOOT_TABLE_ITEMS")?;
        let item_t = self.t("ITEM_ID")?;
        let entry_t = self.t("LOOT_TABLE_ITEM_ID")?;
        let sort_t = self.t("SORT_ORDER")?;
        let active_t = self.t("ACTIVE_FLAG")?;
        let lt = self.row_of("DATA_SET_LOOT_TABLES", ti)?;
        let entries = lt.rows(entries_t).cloned().unwrap_or_default();
        if entries.iter().any(|e| e.num(item_t) == Some(item)) {
            return Ok(None);
        }
        let template = like
            .and_then(|l| entries.iter().find(|e| e.num(item_t) == Some(l)))
            .or_else(|| entries.iter().rev().find(|e| e.str(active_t) == Some("y")))
            .or_else(|| entries.last())
            .cloned();
        // An entry may carry its own name/description messages; those are
        // the template champion's, not the new one's.
        let own_msgs: Vec<u16> = ["NAME_MSG_ID", "DESC_MSG_ID", "SHORT_DESC_MSG_ID"].iter().filter_map(|f| self.tokens.token(f)).collect();
        let mut row = match template {
            Some(r) => {
                let mut r = self.clone_row(&r, &HashMap::new(), &mut HashMap::new());
                r.entries.retain(|(t, _)| !own_msgs.contains(t));
                r
            }
            None => {
                let mut r = Row::default();
                r.set(self.t("LOOT_TABLE_ID")?, Value::Num(table));
                r.set(self.t("QUANTITY")?, Value::Num(1));
                r
            }
        };
        Self::set_num(&mut row, item_t, item);
        Self::set_num(&mut row, entry_t, self.max_nested("DATA_SET_LOOT_TABLES", "LOOT_TABLE_ITEM_ID")? + 1);
        Self::set_num(&mut row, sort_t, entries.iter().filter_map(|e| e.num(sort_t)).max().map(|m| m + 1).unwrap_or(0));
        row.set(active_t, Value::Str("y".into()));
        let tokens = self.tokens.clone();
        let at = self.doc_mut("assembly")?.insert_row(&tokens, &[(tables_t, ti)], entries_t, row)?;
        Ok(Some(json!([["DATA_SET_LOOT_TABLES", ti], ["DATA_SET_LOOT_TABLE_ITEMS", at]])))
    }

    /// `mcts.vendor`: the Champion Vendor loot table (475) against the
    /// champion bots: `{table, total, active, listed: [{bot, item, active}],
    /// missing: [{bot, name, item}]}`. `missing` are the bots of champion
    /// type with a Character Class item that has no entry (an entry is what
    /// makes the client list a champion).
    pub fn vendor(&self) -> Result<J> {
        const CHAMPION: u64 = 10849;
        const CHARACTER_CLASS: u64 = 10149;
        const CHAMPION_VENDOR: u64 = 475;
        let item_t = self.t("ITEM_ID")?;
        let ref_bot_t = self.t("REF_BOT_ID")?;
        let type_t = self.t("ITEM_TYPE_VALUE_ID")?;
        let active_t = self.t("ACTIVE_FLAG")?;
        let doc = self.doc("assembly")?;
        let Some(ti) = self.find_row("DATA_SET_LOOT_TABLES", "LOOT_TABLE_ID", CHAMPION_VENDOR)? else {
            return Ok(json!({ "table": CHAMPION_VENDOR, "index": null, "total": 0, "active": 0, "listed": [], "missing": [] }));
        };
        let entries: Vec<Row> = self.row_of("DATA_SET_LOOT_TABLES", ti)?.rows(self.t("DATA_SET_LOOT_TABLE_ITEMS")?).cloned().unwrap_or_default();
        // Character Class items per bot (a champion can have several; one in
        // the vendor is enough).
        let mut class_of: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut bot_of: HashMap<u64, u64> = HashMap::new();
        for r in doc.marshal.root.rows(self.t("DATA_SET_ITEMS")?).map(|v| v.as_slice()).unwrap_or(&[]) {
            if r.num(type_t) == Some(CHARACTER_CLASS) {
                if let (Some(b), Some(i)) = (r.num(ref_bot_t), r.num(item_t)) {
                    class_of.entry(b).or_default().push(i);
                    bot_of.insert(i, b);
                }
            }
        }
        let listed: Vec<J> = entries
            .iter()
            .map(|e| {
                let item = e.num(item_t).unwrap_or(0);
                json!({ "bot": bot_of.get(&item), "item": item, "active": e.str(active_t) == Some("y") })
            })
            .collect();
        let in_vendor: std::collections::HashSet<u64> = entries.iter().filter_map(|e| e.num(item_t)).collect();
        let bot_id_t = self.t("BOT_ID")?;
        let bot_type_t = self.t("BOT_TYPE_VALUE_ID")?;
        let name_t = self.t("REFERENCE_NAME")?;
        let msg_t = self.t("NAME_MSG_ID")?;
        let mut missing: Vec<J> = Vec::new();
        for b in doc.marshal.root.rows(self.t("DATA_SET_BOTS")?).map(|v| v.as_slice()).unwrap_or(&[]) {
            if b.num(bot_type_t) != Some(CHAMPION) {
                continue;
            }
            let Some(bot) = b.num(bot_id_t) else { continue };
            let Some(items) = class_of.get(&bot) else { continue };
            if items.iter().any(|i| in_vendor.contains(i)) {
                continue;
            }
            let name = self.message_text(b.num(msg_t)).unwrap_or_default();
            missing.push(json!({ "bot": bot, "item": items[0], "reference": b.str(name_t).unwrap_or(""), "name": name }));
        }
        missing.sort_by(|a, b| a["reference"].as_str().unwrap_or("").to_lowercase().cmp(&b["reference"].as_str().unwrap_or("").to_lowercase()));
        Ok(json!({
            "table": CHAMPION_VENDOR,
            "index": ti,
            "total": entries.len(),
            "active": entries.iter().filter(|e| e.str(active_t) == Some("y")).count(),
            "listed": listed,
            "missing": missing,
        }))
    }

    /// `mcts.vendorAdd` `{bot}`: add the bot's Character Class item to the
    /// Champion Vendor; `{bot, item, vendor: path | null, existed}`.
    pub fn vendor_add(&mut self, p: &J) -> Result<J> {
        const CHARACTER_CLASS: u64 = 10149;
        const CHAMPION_VENDOR: u64 = 475;
        let bot = p.get("bot").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("missing bot".into()))?;
        let item_t = self.t("ITEM_ID")?;
        let ref_bot_t = self.t("REF_BOT_ID")?;
        let type_t = self.t("ITEM_TYPE_VALUE_ID")?;
        let item = self
            .doc("assembly")?
            .marshal
            .root
            .rows(self.t("DATA_SET_ITEMS")?)
            .and_then(|rows| rows.iter().find(|r| r.num(ref_bot_t) == Some(bot) && r.num(type_t) == Some(CHARACTER_CLASS)).and_then(|r| r.num(item_t)))
            .ok_or_else(|| Error::Format(format!("bot {bot} has no Character Class item (ITEM_TYPE_VALUE_ID 10149 with REF_BOT_ID {bot}); create one first")))?;
        let vendor = self.add_vendor_entry(CHAMPION_VENDOR, None, item)?;
        Ok(json!({ "bot": bot, "item": item, "vendor": vendor, "existed": vendor.is_none() }))
    }

    fn device_msg(&self, device: u64, field: &str) -> Result<Option<u64>> {
        let Some(i) = self.find_row("DATA_SET_DEVICES", "DEVICE_ID", device)? else { return Ok(None) };
        Ok(self.row_of("DATA_SET_DEVICES", i)?.num(self.t(field)?))
    }

    fn message_text(&self, id: Option<u64>) -> Option<String> {
        let id = id?;
        let doc = self.doc("INT").ok()?;
        let set = self.tokens.token("DATA_SET")?;
        let msg_id = self.tokens.token("MSG_ID")?;
        let message = self.tokens.token("MESSAGE")?;
        doc.marshal.root.rows(set)?.iter().find(|r| r.num(msg_id) == Some(id)).and_then(|r| r.str(message)).map(|s| s.to_string())
    }

    /// `mcts.create`: `{kind: device|card|item|champion, template: <id>,
    /// name, description, shortDescription?, bot?, slot?, itemType?}`.
    pub fn create(&mut self, p: &J) -> Result<J> {
        let kind = p.get("kind").and_then(|v| v.as_str()).unwrap_or("device");
        let template = p.get("template").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("missing template id".into()))?;
        let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("New").to_string();
        let desc = p.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let short = p.get("shortDescription").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let bot = p.get("bot").and_then(|v| v.as_u64());
        let slot = p.get("slot").and_then(|v| v.as_u64());
        let item_type = p.get("itemType").and_then(|v| v.as_u64());
        let mut ids = self.next_ids()?;
        let mut out = match kind {
            "device" | "card" => self.create_device(template, &name, &desc, &short, bot, slot, &mut ids)?,
            "item" => self.create_item(template, &name, &desc, &short, bot, item_type, &mut ids)?,
            "champion" => self.create_champion(template, &name, &desc, &mut ids)?,
            other => return Err(Error::Format(format!("unknown kind {other}"))),
        };
        out["kind"] = json!(kind);
        Ok(out)
    }

    /// The rows the private server (paserver-core) answers with for a
    /// champion to be selectable and owned: `GET_PROFILE_CLASSES` and the
    /// inventory items on the bot.
    pub fn server_rows(&self, p: &J) -> Result<J> {
        let bot = p.get("bot").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("missing bot".into()))?;
        let bi = self.find_row("DATA_SET_BOTS", "BOT_ID", bot)?.ok_or_else(|| Error::Format(format!("no bot {bot}")))?;
        let b = self.row_of("DATA_SET_BOTS", bi)?;
        let res_name = |id: Option<u64>| -> String {
            id.and_then(|id| self.find_row("DATA_SET_RESOURCES", "RES_ID", id).ok().flatten())
                .and_then(|i| self.row_of("DATA_SET_RESOURCES", i).ok())
                .and_then(|r| r.str(self.tokens.token("NAME")?).map(|s| s.to_string()))
                .unwrap_or_default()
        };
        let items: Vec<J> = self
            .doc("assembly")?
            .marshal
            .root
            .rows(self.t("DATA_SET_ITEMS")?)
            .map(|rows| {
                rows.iter()
                    .filter(|r| r.num(self.tokens.token("REF_BOT_ID").unwrap_or(0)) == Some(bot))
                    .map(|r| {
                        json!({
                            "ITEM_ID": r.num(self.tokens.token("ITEM_ID").unwrap_or(0)),
                            "ITEM_TYPE_VALUE_ID": r.num(self.tokens.token("ITEM_TYPE_VALUE_ID").unwrap_or(0)),
                            "ITEM_SUBTYPE_VALUE_ID": r.num(self.tokens.token("ITEM_SUBTYPE_VALUE_ID").unwrap_or(0)).unwrap_or(0),
                            "REF_BOT_ID": bot,
                            "QUANTITY": 1,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let class_item = items.iter().find(|i| i["ITEM_TYPE_VALUE_ID"].as_u64() == Some(10149)).and_then(|i| i["ITEM_ID"].as_u64());
        Ok(json!({
            "profileClass": {
                "PROFILE_ID": bot,
                "BOT_TYPE_VALUE_ID": b.num(self.t("BOT_TYPE_VALUE_ID")?),
                "CLASS_NAME": res_name(b.num(self.t("PAWN_CLASS_RES_ID")?)),
                "PAWN_CLASS_RES_ID": b.num(self.t("PAWN_CLASS_RES_ID")?),
                "CONTROLLER_CLASS_RES_ID": b.num(self.t("CONTROLLER_CLASS_RES_ID")?).unwrap_or(0),
                "CLASS_MSG_ID": b.num(self.t("NAME_MSG_ID")?).map(|v| v.to_string()),
                "DESC_MSG_ID": b.num(self.t("DESC_MSG_ID")?),
                "CURRENCY_VALUE": b.num(self.t("CURRENCY_VALUE")?).unwrap_or(0),
                "LOOT_TABLE_ITEM_ID": class_item,
            },
            "inventory": items,
        }))
    }

    /// Write a mod folder: manifest, the changed `.dat` files under
    /// `files/`, the server rows, and the Lua + UnrealScript scaffold for
    /// the given devices.
    pub fn scaffold(&mut self, p: &J) -> Result<J> {
        let dir = PathBuf::from(p.get("dir").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("missing dir".into()))?);
        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("my-champion").to_string();
        let name = p.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
        let version = p.get("version").and_then(|v| v.as_str()).unwrap_or("0.1.0").to_string();
        let author = p.get("author").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let bot = p.get("bot").and_then(|v| v.as_u64());
        let devices: Vec<u64> = p.get("devices").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
        let all_langs = p.get("allLanguages").and_then(|v| v.as_bool()).unwrap_or(false);
        let pkg = p.get("package").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("Tm{}", camel(&id)));

        std::fs::create_dir_all(dir.join("files"))?;
        let esc = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let mut manifest = format!("[mod]\nid = {}\nname = {}\nversion = {}\n", esc(&id), esc(&name), esc(&version));
        if !author.is_empty() {
            manifest.push_str(&format!("authors = [{}]\n", esc(&author)));
        }
        manifest.push_str(&format!("readme = {}\ndependencies = [\"tempest-scripting>=1.0.0\"]\n", esc("Content rows in assembly.dat and the language files, with a Lua scaffold for the new devices.")));
        std::fs::write(dir.join("manifest.toml"), manifest)?;

        // The .dat files: the assembly, and the languages that changed (or
        // all). A file whose base the Studio recorded ships as a row delta
        // (`deltas/<game path>.tdelta`); the others as whole files.
        let mut written = Vec::new();
        let mut deltas = Vec::new();
        let tokens = self.tokens.clone();
        let cooked = self.dir.clone();
        let game_root = game_root_of(&cooked);
        let use_deltas = p.get("deltas").and_then(|v| v.as_bool()).unwrap_or(true);
        let ids: Vec<String> = self.docs.iter().map(|(k, _)| k.clone()).collect();
        for doc_id in ids {
            let (dirty, path) = {
                let d = self.doc(&doc_id)?;
                (d.dirty, d.path.clone())
            };
            if doc_id != "assembly" && !dirty && !all_langs {
                continue;
            }
            let rel = path.strip_prefix(&game_root).map(|r| r.to_path_buf()).unwrap_or_else(|_| PathBuf::from(path.file_name().unwrap_or_default()));
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let digest = if use_deltas { crate::basestore::read(&path) } else { None };
            if let Some(digest) = digest {
                let doc = self.doc(&doc_id)?;
                let d = super::delta::create(&tokens, super::delta::Base::Digest(&digest), &doc.marshal, &rel_str)?;
                let ops: usize = d.header["tables"].as_array().map(|a| a.iter().map(|t| t["ops"].as_array().map(|o| o.len()).unwrap_or(0)).sum()).unwrap_or(0) + d.header["header"].as_object().map(|h| h.len()).unwrap_or(0);
                if ops == 0 {
                    continue;
                }
                let out = dir.join("deltas").join(format!("{rel_str}.tdelta"));
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                d.write(&out)?;
                written.push(format!("deltas/{rel_str}.tdelta"));
                deltas.push(json!({ "file": rel_str, "ops": ops, "bytes": d.to_bytes().len() }));
            } else {
                let out = dir.join("files").join(&rel);
                self.doc_mut(&doc_id)?.save(&tokens, Some(&out))?;
                written.push(format!("files/{rel_str}"));
            }
        }

        if let Some(b) = bot {
            let rows = self.server_rows(&json!({ "bot": b }))?;
            std::fs::write(dir.join("paserver.json"), serde_json::to_vec_pretty(&rows).unwrap_or_default())?;
            written.push("paserver.json".into());
        }

        // Scaffold: one UnrealScript class pair and Lua natives per device.
        let mut lua = String::new();
        lua.push_str(&format!("-- {name}: behaviour of the new devices, implemented in Lua.\n-- The classes live in src/{pkg}/Classes (compile them with `uscript` into {pkg}.u,\n-- drop the package next to the game's script packages, and name them in the\n-- devices' FORM_CLASS_RES_ID / DEVICE_FORM_CLASS_RES_ID resources).\n-- `dllimport` functions of those classes are bound to the handlers below;\n-- see Tempest.Scripting/examples/10_lua_natives.lua.\n\n"));
        let classes_dir = dir.join("src").join(&pkg).join("Classes");
        std::fs::create_dir_all(&classes_dir)?;
        let mut classes = Vec::new();
        for d in &devices {
            let Some(i) = self.find_row("DATA_SET_DEVICES", "DEVICE_ID", *d)? else { continue };
            let row = self.row_of("DATA_SET_DEVICES", i)?;
            let dname = self.message_text(row.num(self.t("NAME_MSG_ID")?)).unwrap_or_else(|| format!("Device{d}"));
            let cname = camel(&dname);
            let base_form = self.resource_name(row.num(self.t("FORM_CLASS_RES_ID")?)).unwrap_or_else(|| "TgGame.TgDevice".into());
            let base_device_form = self.resource_name(row.num(self.t("DEVICE_FORM_CLASS_RES_ID")?)).filter(|s| s != "None");
            let device_class = format!("TgDevice_{cname}");
            let uc = format!(
                "// Device {d} ({dname}): extends the template's class; override what differs.\n// Functions marked dllimport are implemented in Lua (scripts/main.lua).\nclass {device_class} extends {base} DLLBind(TempestScripting);\n\n// Called by your overrides: e.g. from StartFire / ActivateDevice.\ndllimport final function float LuaOnFire(int DeviceId, float Charge);\n\ndefaultproperties\n{{\n}}\n",
                base = base_form.rsplit('.').next().unwrap_or("TgDevice")
            );
            std::fs::write(classes_dir.join(format!("{device_class}.uc")), uc)?;
            classes.push(device_class.clone());
            if let Some(bdf) = &base_device_form {
                let form_class = format!("TgDeviceForm_{cname}");
                let uc = format!("// Form (client-side visuals/input) of device {d} ({dname}).\nclass {form_class} extends {base};\n\ndefaultproperties\n{{\n}}\n", base = bdf.rsplit('.').next().unwrap_or("TgDeviceForm"));
                std::fs::write(classes_dir.join(format!("{form_class}.uc")), uc)?;
                classes.push(form_class);
            }
            lua.push_str(&format!("-- {dname} (device {d})\ntempest.native(\"{pkg}.{device_class}.LuaOnFire\", function(device, p)\n\ttempest.log(string.format(\"{dname} fired (device %d, charge %.2f)\", p.DeviceId, p.Charge))\n\treturn p.Charge\nend)\n\n"));
        }
        lua.push_str("tempest.command(\"mod_status\", function(pc)\n\tpc:ClientMessage(\"mod loaded: ");
        lua.push_str(&name);
        lua.push_str("\")\nend, \"status of this mod\")\n");
        std::fs::create_dir_all(dir.join("scripts"))?;
        std::fs::write(dir.join("scripts").join("main.lua"), lua)?;
        written.push("scripts/main.lua".into());

        let readme = format!(
            "# {name}\n\nGenerated by Tempest Studio.\n\n- `deltas/`: the rows this mod adds or changes in `assembly.dat` and the language files (applied over the game's files by the launcher, so other content mods still work); `files/` holds whole files when the Studio had no record of the original.\n- `paserver.json`: the GET_PROFILE_CLASSES row and inventory items the private server must answer for the champion (bot {bot}).\n- `scripts/main.lua`: Lua handlers for the devices' `dllimport` functions (needs the tempest-scripting mod).\n- `src/{pkg}/Classes/*.uc`: UnrealScript stubs for the new device classes.\n\nBuild the script package:\n\n```\nuscript project init <CookedPCConsole> <project>   # once\n# copy src/{pkg} into the project, then\nuscript project build <project> --package {pkg}\n```\n\nthen put `{pkg}.u` under `files/ChaosGame/CookedPCConsole/` and point the devices' FORM_CLASS_RES_ID / DEVICE_FORM_CLASS_RES_ID resources at `{pkg}.<class>` (Assembly tab, DATA_SET_RESOURCES). Pack with `tempest mod pack <this folder> <out.tempest>`.\n\nClasses: {classes}\n",
            bot = bot.map(|b| b.to_string()).unwrap_or_else(|| "none".into()),
            classes = classes.join(", ")
        );
        std::fs::write(dir.join("README.md"), readme)?;
        written.push("README.md".into());
        Ok(json!({ "dir": dir.to_string_lossy(), "files": written, "deltas": deltas, "classes": classes, "package": pkg }))
    }

    fn resource_name(&self, id: Option<u64>) -> Option<String> {
        let i = self.find_row("DATA_SET_RESOURCES", "RES_ID", id?).ok()??;
        let r = self.row_of("DATA_SET_RESOURCES", i).ok()?;
        r.str(self.tokens.token("NAME")?).map(|s| s.to_string())
    }
}

/// `<game>/ChaosGame/CookedPCConsole` → `<game>`.
fn game_root_of(cooked: &Path) -> PathBuf {
    cooked.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()).unwrap_or_else(|| cooked.to_path_buf())
}

/// `my champion name` → `MyChampionName` (identifier-safe).
fn camel(s: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(if up { c.to_ascii_uppercase() } else { c });
            up = false;
        } else {
            up = true;
        }
    }
    if out.is_empty() {
        out.push_str("Mod");
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, 'M');
    }
    out
}
