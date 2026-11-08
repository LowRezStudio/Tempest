//! `mcts.choices`: the rows a wizard offers to pick, by their names in game
//! (INT texts) rather than ids: champions (bots), a champion's devices
//! (abilities, cards, talents: devices whose item row names the bot), items
//! and skin collections. Filtered by a query over name and id.

use std::collections::HashMap;

use serde_json::{json, Value as J};

use tempest_upk::io::{Error, Result};

use super::workspace::Workspace;
use super::Row;

impl Workspace {
    /// `mcts.choices {kind: "bot"|"device"|"item"|"collection", bot?, query?, limit?}` →
    /// `[{id, name, detail}]`, named rows first.
    pub fn choices(&mut self, p: &J) -> Result<J> {
        let kind = p.get("kind").and_then(|v| v.as_str()).unwrap_or("item").to_string();
        let bot = p.get("bot").and_then(|v| v.as_u64());
        let query = p.get("query").and_then(|v| v.as_str()).unwrap_or("").trim().to_lowercase();
        let limit = p.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
        if kind == "collection" {
            let all = self.collections(&json!({ "bot": bot }))?;
            let mut out: Vec<J> = all["collections"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| {
                    let parts: Vec<String> = c["parts"].as_array().into_iter().flatten().filter_map(|p| p["kind"].as_str().map(str::to_string)).collect();
                    json!({ "id": c["item"], "name": c["name"], "detail": format!("{}{}", parts.join(", "), if c["active"].as_bool() == Some(true) { "" } else { " · inactive" }) })
                })
                .collect();
            filter(&mut out, &query, limit);
            return Ok(J::Array(out));
        }
        let values = self.valid_values().unwrap_or(J::Null);
        let value_text = |v: Option<u64>| v.and_then(|v| values[v.to_string()]["text"].as_str().map(str::to_string)).unwrap_or_default();
        let lk = self.lookup()?;
        let tok = |n: &str| self.tokens.token(n).ok_or_else(|| Error::Format(format!("no field {n}")));
        let (name_t, ref_bot_t, type_t) = (tok("NAME_MSG_ID")?, tok("REF_BOT_ID")?, tok("ITEM_TYPE_VALUE_ID")?);
        let text = |r: &Row| r.num(name_t).and_then(|m| lk.messages.get(&m)).map(|s| s.to_string()).unwrap_or_default();
        let ref_name = self.tokens.token("REFERENCE_NAME");
        let mut out: Vec<J> = Vec::new();
        match kind.as_str() {
            "bot" => {
                // Champions are the bots the Champion Vendor (loot table 475) lists: its entries' items name them.
                let mut champions: HashMap<u64, bool> = HashMap::new();
                if let (Ok(Some((_, vendor))), Ok(entries_t), Ok(item_t)) = (self.loot_entries(475), tok("DATA_SET_LOOT_TABLE_ITEMS"), tok("ITEM_ID")) {
                    for e in vendor.rows(entries_t).map(|v| v.as_slice()).unwrap_or(&[]) {
                        if let Some(b) = e.num(item_t).and_then(|i| lk.items.get(&i)).and_then(|(_, r)| r.num(ref_bot_t)) {
                            champions.insert(b, true);
                        }
                    }
                }
                for (&id, (_, r)) in &lk.bots {
                    let name = text(r);
                    let reference = ref_name.and_then(|t| r.str(t)).unwrap_or("").to_string();
                    let shown = if name.is_empty() { reference.clone() } else { name };
                    out.push(json!({ "id": id, "name": shown, "detail": if champions.contains_key(&id) { format!("champion · {reference}") } else { format!("bot · {reference}") } }));
                }
                out.sort_by_key(|o| (!o["detail"].as_str().unwrap_or("").starts_with("champion"), o["name"].as_str().unwrap_or("").to_lowercase()));
            }
            "device" => {
                let devices_t = tok("DATA_SET_DEVICES")?;
                let id_t = tok("DEVICE_ID")?;
                let slot_t = self.tokens.token("SLOT_USED_VALUE_ID");
                let rows = self.doc("assembly")?.marshal.root.rows(devices_t).map(|v| v.as_slice()).unwrap_or(&[]);
                for d in rows {
                    let Some(id) = d.num(id_t) else { continue };
                    let item = lk.items.get(&id).map(|(_, r)| *r);
                    let owner = item.and_then(|r| r.num(ref_bot_t)).or_else(|| d.num(ref_bot_t));
                    if bot.is_some() && owner != bot {
                        continue;
                    }
                    let name = item.map(text).unwrap_or_default();
                    let slot = value_text(slot_t.and_then(|t| d.num(t)));
                    let ty = value_text(item.and_then(|r| r.num(type_t)));
                    out.push(json!({ "id": id, "name": name, "detail": join(&[ty, slot]) }));
                }
                out.sort_by_key(|o| (o["name"].as_str().unwrap_or("").is_empty(), o["name"].as_str().unwrap_or("").to_lowercase()));
            }
            _ => {
                for (&id, (_, r)) in &lk.items {
                    if bot.is_some() && r.num(ref_bot_t) != bot {
                        continue;
                    }
                    let owner = r.num(ref_bot_t).and_then(|b| lk.bots.get(&b)).map(|(_, b)| text(b)).unwrap_or_default();
                    let ty = value_text(r.num(type_t));
                    out.push(json!({ "id": id, "name": text(r), "detail": join(&[ty, owner]) }));
                }
                out.sort_by_key(|o| (o["name"].as_str().unwrap_or("").is_empty(), o["name"].as_str().unwrap_or("").to_lowercase()));
            }
        }
        filter(&mut out, &query, limit);
        Ok(J::Array(out))
    }
}

/// Keep the rows whose name, detail or id contain `query`, at most `limit`.
fn filter(out: &mut Vec<J>, query: &str, limit: usize) {
    if !query.is_empty() {
        out.retain(|o| o["name"].as_str().unwrap_or("").to_lowercase().contains(query) || o["detail"].as_str().unwrap_or("").to_lowercase().contains(query) || o["id"].to_string().contains(query));
    }
    out.truncate(limit);
}

fn join(parts: &[String]) -> String {
    parts.iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" · ")
}
