//! `wwise.owners`' assembly side: which Wwise events the game's content
//! plays. Rows reach sounds through special FX (`DATA_SET_SPECIAL_FX_SOUNDS`,
//! `AK_EVENT_RES_ID` → the event's resource path, `WW_PC_Nyx.Nyx_SP_…_Play`),
//! and FX through any `*FX_ID` field, assembly meshes (`*ASM_ID`, whose
//! `DATA_SET_ASM_MESH_FXS` rows name the device slot that triggers them),
//! effect groups, projectiles and deployables. Followed from:
//!
//! * a champion's bot row (its meshes), its devices (abilities, weapon,
//!   cards, talents, emotes: devices whose item names the bot) and skins
//!   (`DATA_SET_SKIN_FX_XREF` swaps FX), each event labelled with the part;
//! * every other item (sprays, containers…) on its own.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value as J};

use tempest_upk::io::Result;

use super::workspace::Workspace;
use super::Row;

/// What a field leads to.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Link {
    Fx,
    Asm,
    EffectGroup,
    Projectile,
    Deployable,
    Event,
}

/// One event reached from a row: the event's resource path, the FX row's
/// name and the part of the owner it was reached from.
#[derive(Clone, Debug, PartialEq)]
pub struct Reached {
    pub event: String,
    pub fx: String,
    pub part: String,
}

struct Graph<'a> {
    links: HashMap<u16, Link>,
    fx: HashMap<u64, &'a Row>,
    asm: HashMap<u64, &'a Row>,
    groups: HashMap<u64, &'a Row>,
    projectiles: HashMap<u64, &'a Row>,
    deployables: HashMap<u64, &'a Row>,
    resources: HashMap<u64, &'a str>,
    name_t: Option<u16>,
    slot_t: Option<u16>,
}

impl Graph<'_> {
    /// Events under `row`; `slots` names the parts a mesh FX row's
    /// `SLOT_VALUE_ID` stands for (a champion's device slots).
    fn walk(
        &self,
        row: &Row,
        part: &str,
        fx_name: &str,
        slots: &HashMap<u64, String>,
        seen: &mut HashSet<(Link, u64)>,
        out: &mut Vec<Reached>,
    ) {
        let part = match self
            .slot_t
            .and_then(|t| row.num(t))
            .and_then(|s| slots.get(&s))
        {
            Some(p) => p.as_str(),
            None => part,
        };
        for (token, value) in &row.entries {
            match value {
                super::Value::Rows(rows) => {
                    for r in rows {
                        self.walk(r, part, fx_name, slots, seen, out);
                    }
                }
                super::Value::Num(id) if *id != 0 => {
                    let Some(&link) = self.links.get(token) else {
                        continue;
                    };
                    if link == Link::Event {
                        if let Some(e) = self.resources.get(id) {
                            if !out.iter().any(|r| r.event.eq_ignore_ascii_case(e)) {
                                out.push(Reached {
                                    event: e.to_string(),
                                    fx: fx_name.to_string(),
                                    part: part.to_string(),
                                });
                            }
                        }
                        continue;
                    }
                    let table = match link {
                        Link::Fx => &self.fx,
                        Link::Asm => &self.asm,
                        Link::EffectGroup => &self.groups,
                        Link::Projectile => &self.projectiles,
                        _ => &self.deployables,
                    };
                    let Some(next) = table.get(id) else { continue };
                    if !seen.insert((link, *id)) {
                        continue;
                    }
                    let name = if link == Link::Fx {
                        self.name_t.and_then(|t| next.str(t)).unwrap_or(fx_name)
                    } else {
                        fx_name
                    };
                    self.walk(next, part, name, slots, seen, out);
                }
                _ => {}
            }
        }
    }
}

impl Workspace {
    fn graph(&self) -> Result<Graph<'_>> {
        let doc = self.doc("assembly")?;
        let index = |table: &str, key: &str| -> HashMap<u64, &Row> {
            let (Some(t), Some(k)) = (self.tokens.token(table), self.tokens.token(key)) else {
                return HashMap::new();
            };
            doc.marshal
                .root
                .rows(t)
                .map(|v| v.as_slice())
                .unwrap_or(&[])
                .iter()
                .filter_map(|r| Some((r.num(k)?, r)))
                .collect()
        };
        let mut links = HashMap::new();
        for token in 0..self.tokens.field_count() as u16 {
            let name = self.tokens.name(token);
            let link = if name == "AK_EVENT_RES_ID" {
                Link::Event
            } else if name.ends_with("FX_ID")
                && !matches!(
                    name.as_str(),
                    "FX_SOUND_ID" | "ASM_MESH_FX_ID" | "FROM_FX_ID"
                )
            {
                // A skin's `FROM_FX_ID` is the champion's FX it replaces.
                Link::Fx
            } else if name.ends_with("ASM_ID") {
                Link::Asm
            } else if name.ends_with("EFFECT_GROUP_ID") {
                Link::EffectGroup
            } else if name.ends_with("PROJECTILE_ID") {
                Link::Projectile
            } else if name.ends_with("DEPLOYABLE_ID") {
                Link::Deployable
            } else {
                continue;
            };
            links.insert(token, link);
        }
        let name_t = self.tokens.token("NAME");
        let mut resources = HashMap::new();
        if let (Some(t), Some(id_t), Some(n)) = (
            self.tokens.token("DATA_SET_RESOURCES"),
            self.tokens.token("RES_ID"),
            name_t,
        ) {
            for r in doc
                .marshal
                .root
                .rows(t)
                .map(|v| v.as_slice())
                .unwrap_or(&[])
            {
                if let (Some(id), Some(s)) = (r.num(id_t), r.str(n)) {
                    resources.insert(id, s);
                }
            }
        }
        Ok(Graph {
            links,
            fx: index("DATA_SET_SPECIAL_FX", "FX_ID"),
            asm: index("DATA_SET_ASSEMBLY_MESHES", "ASM_ID"),
            groups: index("DATA_SET_EFFECT_GROUPS", "EFFECT_GROUP_ID"),
            projectiles: index("DATA_SET_PROJECTILES", "DEVICE_PROJECTILE_ID"),
            deployables: index("DATA_SET_DEPLOYABLES", "DEPLOYABLE_ID"),
            resources,
            name_t,
            slot_t: self.tokens.token("SLOT_VALUE_ID"),
        })
    }

    /// Champions (the bots the Champion Vendor, loot table 475, lists) and
    /// items with the Wwise events they play: `{champions: [{id, name,
    /// reference, events: [{event, fx, part, skin}]}], items:
    /// [{id, name, kind, events}]}`. `part` is the device (ability, weapon,
    /// card…) or skin an event was reached from, `"Champion"` otherwise.
    pub fn sound_owners(&mut self) -> Result<J> {
        let values = self.valid_values().unwrap_or(J::Null);
        let lk = self.lookup()?;
        let g = self.graph()?;
        let doc = self.doc("assembly")?;
        let tok = |n: &str| self.tokens.token(n);
        let (name_t, ref_bot_t, type_t, item_t) = (
            tok("NAME_MSG_ID"),
            tok("REF_BOT_ID"),
            tok("ITEM_TYPE_VALUE_ID"),
            tok("ITEM_ID"),
        );
        let text = |r: &Row| {
            name_t
                .and_then(|t| r.num(t))
                .and_then(|m| lk.messages.get(&m))
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        let item_name = |id: u64| lk.items.get(&id).map(|(_, r)| text(r)).unwrap_or_default();
        let rows = |table: &str| {
            tok(table)
                .and_then(|t| doc.marshal.root.rows(t))
                .map(|v| v.as_slice())
                .unwrap_or(&[])
        };
        let (device_t, slot_used_t, bot_devices_t, ref_name_t) = (
            tok("DEVICE_ID"),
            tok("SLOT_USED_VALUE_ID"),
            tok("DATA_SET_BOT_DEVICES"),
            tok("REFERENCE_NAME"),
        );

        // Devices and skins by the bot their item names.
        let owner = |id: u64| {
            lk.items
                .get(&id)
                .and_then(|(_, r)| ref_bot_t.and_then(|t| r.num(t)))
        };
        let mut devices: HashMap<u64, Vec<&Row>> = HashMap::new();
        for d in rows("DATA_SET_DEVICES") {
            let id = device_t.and_then(|t| d.num(t)).unwrap_or(0);
            if let Some(b) = owner(id).or_else(|| ref_bot_t.and_then(|t| d.num(t))) {
                devices.entry(b).or_default().push(d);
            }
        }
        let mut skins: HashMap<u64, Vec<&Row>> = HashMap::new();
        let mut skin_items: HashSet<u64> = HashSet::new();
        for s in rows("DATA_SET_SKINS") {
            let id = item_t.and_then(|t| s.num(t)).unwrap_or(0);
            skin_items.insert(id);
            if let Some(b) = owner(id) {
                skins.entry(b).or_default().push(s);
            }
        }

        let mut champion_ids: HashSet<u64> = HashSet::new();
        if let (Ok(Some((_, vendor))), Some(entries_t), Some(item_t)) = (
            self.loot_entries(475),
            tok("DATA_SET_LOOT_TABLE_ITEMS"),
            item_t,
        ) {
            for e in vendor.rows(entries_t).map(|v| v.as_slice()).unwrap_or(&[]) {
                if let Some(b) = e.num(item_t).and_then(owner) {
                    champion_ids.insert(b);
                }
            }
        }
        let reached_json = |r: &Reached, skin: Option<&str>| json!({ "event": r.event, "fx": r.fx, "part": r.part, "skin": skin });
        let mut champions = Vec::new();
        for (&bot, (_, row)) in lk.bots.iter().filter(|(b, _)| champion_ids.contains(b)) {
            let mine = devices.get(&bot).map(|v| v.as_slice()).unwrap_or(&[]);
            // The slots of the bot's own devices name the mesh FX they trigger.
            let mut slots: HashMap<u64, String> = HashMap::new();
            for bd in bot_devices_t
                .and_then(|t| row.rows(t))
                .map(|v| v.as_slice())
                .unwrap_or(&[])
            {
                if let (Some(slot), Some(dev)) = (
                    slot_used_t.and_then(|t| bd.num(t)),
                    device_t.and_then(|t| bd.num(t)),
                ) {
                    let n = item_name(dev);
                    if !n.is_empty() {
                        slots.insert(slot, n);
                    }
                }
            }
            let mut seen = HashSet::new();
            let mut events = Vec::new();
            let mut out = Vec::new();
            g.walk(row, "Champion", "", &slots, &mut seen, &mut out);
            for d in mine {
                let id = device_t.and_then(|t| d.num(t)).unwrap_or(0);
                let n = item_name(id);
                g.walk(
                    d,
                    if n.is_empty() { "Champion" } else { &n },
                    "",
                    &slots,
                    &mut seen,
                    &mut out,
                );
            }
            events.extend(out.iter().map(|r| reached_json(r, None)));
            for s in skins.get(&bot).map(|v| v.as_slice()).unwrap_or(&[]) {
                let n = item_name(item_t.and_then(|t| s.num(t)).unwrap_or(0));
                let mut out = Vec::new();
                // A skin's swaps lead to FX already seen through the champion: its own `seen`.
                g.walk(s, &n, "", &slots, &mut HashSet::new(), &mut out);
                events.extend(out.iter().map(|r| reached_json(r, Some(&n))));
            }
            if events.is_empty() {
                continue;
            }
            let name = text(row);
            let reference = ref_name_t
                .and_then(|t| row.str(t))
                .unwrap_or("")
                .to_string();
            champions.push(json!({ "id": bot, "name": if name.is_empty() { reference.clone() } else { name }, "reference": reference, "events": events }));
        }
        champions.sort_by_key(|c| c["name"].as_str().unwrap_or("").to_lowercase());

        // Other items: on their own (sprays, containers, mounts…).
        let mut items = Vec::new();
        for (&id, (_, row)) in &lk.items {
            if owner(id).is_some() || skin_items.contains(&id) {
                continue;
            }
            let mut out = Vec::new();
            g.walk(row, "", "", &HashMap::new(), &mut HashSet::new(), &mut out);
            if out.is_empty() {
                continue;
            }
            let kind = type_t
                .and_then(|t| row.num(t))
                .and_then(|v| values[v.to_string()]["text"].as_str().map(str::to_string))
                .unwrap_or_default();
            let name = text(row);
            items.push(json!({ "id": id, "name": if name.is_empty() { format!("item {id}") } else { name }, "kind": kind, "events": out.iter().map(|r| reached_json(r, None)).collect::<Vec<_>>() }));
        }
        items.sort_by_key(|i| {
            (
                i["kind"].as_str().unwrap_or("").to_lowercase(),
                i["name"].as_str().unwrap_or("").to_lowercase(),
            )
        });
        Ok(json!({ "champions": champions, "items": items }))
    }
}
