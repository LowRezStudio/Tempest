//! Deltas for Wwise file packages (`kind: "pck"`): what a modded `.pck`
//! changed relative to the game's own, so a mod that replaces one song ships
//! that song instead of the whole 150 MB package, and two mods changing
//! different sounds of the same package compose.
//!
//! Same container as the package deltas (no image): the header lists the
//! operations, each with a `path` (what the launcher compares for conflicts),
//! the MD5 of what it replaces (`base`, for drift) and where its payload is
//! in the delta's streams (`stream: [s, offset, len]`). Paths:
//!
//! * `stream:<id>`, `external:<id>`: a streamed or external media file, whole.
//! * `bank:<id>`: a soundbank, whole (added, removed, or not decomposable).
//! * `bank:<id>/hirc:<object id>`: one hierarchy object (payload: its kind
//!   byte and body).
//! * `bank:<id>/media:<media id>`: one embedded media file (`DIDX`/`DATA`).
//! * `bank:<id>/chunk:<TAG>`: any other bank chunk (`BKHD`, `STID`, …).
//!
//! An id is followed by `@<language id>` for a language other than 0 (the
//! same id may exist once per language). Table entries keep the order the
//! game's lookup needs (sorted by id, then language): additions are inserted
//! in place. A new hierarchy object goes before the first object that refers
//! to it, so a child still comes before its parent as the bank cooker writes
//! them.

use std::collections::{BTreeMap, HashMap};

use md5::{Digest as _, Md5};
use serde_json::{json, Map, Value as J};
use tempest_upk::io::{Error, Result};

use super::akpk::{Entry, Package};
use super::bnk::{kind_name, Bank, Object};
use crate::delta::{Applied, Delta, VERSION};

fn hex(h: &[u8]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash(b: &[u8]) -> String {
    hex(&Md5::digest(b))
}

/// The three tables of a package, by the name paths use.
const TABLES: [&str; 3] = ["bank", "stream", "external"];

fn table<'a>(p: &'a Package, name: &str) -> &'a Vec<Entry> {
    match name {
        "bank" => &p.banks,
        "stream" => &p.streams,
        _ => &p.externals,
    }
}

fn table_mut<'a>(p: &'a mut Package, name: &str) -> &'a mut Vec<Entry> {
    match name {
        "bank" => &mut p.banks,
        "stream" => &mut p.streams,
        _ => &mut p.externals,
    }
}

/// `id` or `id@language`.
fn id_key(id: u64, language: u32) -> String {
    if language == 0 { id.to_string() } else { format!("{id}@{language}") }
}

fn entry_path(t: &str, e: &Entry) -> String {
    format!("{t}:{}", id_key(e.id, e.language))
}

/// `(table, id, language)` of an entry path.
fn parse_entry_path(path: &str) -> Option<(&str, u64, u32)> {
    let (t, rest) = path.split_once(':')?;
    let (id, lang) = match rest.split_once('@') {
        Some((i, l)) => (i.parse().ok()?, l.parse().ok()?),
        None => (rest.parse().ok()?, 0),
    };
    TABLES.contains(&t).then_some((t, id, lang))
}

/// A bank's parts by sub-path (`hirc:…`, `media:…`, `chunk:…`) with their
/// payloads and display class, in bank order; `None` when the bank does not
/// parse or a part appears twice (then it is handled whole).
fn bank_parts(bytes: &[u8]) -> Option<Vec<(String, Vec<u8>, String)>> {
    let bank = Bank::parse(bytes).ok()?;
    if bank.to_bytes() != bytes {
        return None;
    }
    let mut out: Vec<(String, Vec<u8>, String)> = Vec::new();
    for (tag, body) in &bank.chunks {
        match tag {
            b"DIDX" | b"DATA" => {}
            b"HIRC" => {
                for o in bank.objects() {
                    let mut payload = Vec::with_capacity(1 + o.body.len());
                    payload.push(o.kind);
                    payload.extend_from_slice(&o.body);
                    out.push((format!("hirc:{}", o.id), payload, format!("Wwise {}", kind_name(o.kind))));
                }
            }
            _ => out.push((format!("chunk:{}", String::from_utf8_lossy(tag)), body.clone(), "Wwise bank chunk".into())),
        }
    }
    // Every object must have been read (a truncated HIRC is handled whole).
    // Embedded media are compared by content: a media change rebuilds
    // DIDX/DATA in the cooker's 16-byte layout, the same data either way.
    let hirc_n = bank.chunks.iter().find(|(t, _)| t == b"HIRC").map(|(_, b)| u32::from_le_bytes(b.get(0..4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize);
    if hirc_n.is_some_and(|n| n != bank.objects().len()) {
        return None;
    }
    for (id, d) in bank.media() {
        out.push((format!("media:{id}"), d.to_vec(), "Wwise media".into()));
    }
    let mut seen = std::collections::HashSet::new();
    out.iter().all(|(p, _, _)| seen.insert(p.clone())).then_some(out)
}

/// Hashes of every entry and, for banks, of every part: what [`create`]
/// compares against. Small enough to keep instead of a copy of the package.
pub fn digest(p: &Package) -> J {
    let mut entries = Map::new();
    let mut parts = Map::new();
    for t in TABLES {
        for e in table(p, t) {
            let path = entry_path(t, e);
            entries.insert(path.clone(), J::from(hash(&e.data)));
            if t == "bank" {
                if let Some(list) = bank_parts(&e.data) {
                    let m: Map<String, J> = list.iter().map(|(k, b, _)| (k.clone(), J::from(hash(b)))).collect();
                    parts.insert(path, J::Object(m));
                }
            }
        }
    }
    json!({ "format": "tdelta-digest", "version": VERSION, "kind": "pck", "entries": entries, "parts": parts })
}

/// Whether `d` is a `.pck` digest [`create`] can use (an older Studio only
/// marked such files, without hashes).
pub fn is_digest(d: &J) -> bool {
    d["kind"] == "pck" && d["entries"].is_object()
}

pub enum Base<'a> {
    Package(&'a Package),
    Digest(&'a J),
}

/// Ops and their payloads, collected into one stream.
struct Builder {
    ops: Vec<J>,
    stream: Vec<u8>,
}

impl Builder {
    fn push(&mut self, op: &str, path: String, class: &str, base: Option<&str>, payload: Option<&[u8]>) {
        let mut o = json!({ "op": op, "path": path, "class": class });
        if let Some(b) = base {
            o["base"] = J::from(b);
        }
        if let Some(p) = payload {
            o["stream"] = json!([0, self.stream.len(), p.len()]);
            o["size"] = J::from(p.len());
            self.stream.extend_from_slice(p);
        }
        self.ops.push(o);
    }
}

/// What `edited` changed against its base.
pub fn create(base: Base, edited: &Package, target: &str) -> Result<Delta> {
    let base_digest = match base {
        Base::Package(p) => digest(p),
        Base::Digest(d) => d.clone(),
    };
    if !is_digest(&base_digest) {
        return Err(Error::Format("the original of this .pck was not recorded with its hashes".into()));
    }
    let empty = Map::new();
    let base_entries = base_digest["entries"].as_object().unwrap_or(&empty);
    let base_parts = base_digest["parts"].as_object().unwrap_or(&empty);
    let mut b = Builder { ops: Vec::new(), stream: Vec::new() };
    let mut seen = std::collections::HashSet::new();
    for t in TABLES {
        let class = match t {
            "bank" => "Wwise bank",
            "stream" => "Wwise streamed media",
            _ => "Wwise external media",
        };
        for e in table(edited, t) {
            let path = entry_path(t, e);
            seen.insert(path.clone());
            let h = hash(&e.data);
            match base_entries.get(&path).and_then(|v| v.as_str()) {
                None => b.push("add", path, class, None, Some(&e.data)),
                Some(bh) if bh == h => {}
                Some(bh) => {
                    // A bank both sides can take apart: only the parts that changed.
                    let parts = if t == "bank" { base_parts.get(&path).and_then(|v| v.as_object()).zip(bank_parts(&e.data)) } else { None };
                    match parts {
                        Some((old, new)) => {
                            let mut kept = std::collections::HashSet::new();
                            for (i, (k, bytes, cls)) in new.iter().enumerate() {
                                kept.insert(k.clone());
                                let ph = hash(bytes);
                                match old.get(k).and_then(|v| v.as_str()) {
                                    None => {
                                        b.push("add", format!("{path}/{k}"), cls, None, Some(bytes));
                                        // A new object goes before the next object the base already had.
                                        if k.starts_with("hirc:") {
                                            if let Some(next) = new[i + 1..].iter().map(|(n, _, _)| n).find(|n| n.starts_with("hirc:") && old.contains_key(*n)) {
                                                b.ops.last_mut().unwrap()["before"] = J::from(next.trim_start_matches("hirc:"));
                                            }
                                        }
                                    }
                                    Some(oh) if oh == ph => {}
                                    Some(oh) => b.push("set", format!("{path}/{k}"), cls, Some(oh), Some(bytes)),
                                }
                            }
                            let mut gone: Vec<(&String, &J)> = old.iter().filter(|(k, _)| !kept.contains(*k)).collect();
                            gone.sort_by(|a, b| a.0.cmp(b.0));
                            for (k, oh) in gone {
                                b.push("delete", format!("{path}/{k}"), "Wwise bank part", oh.as_str(), None);
                            }
                        }
                        None => b.push("set", path, class, Some(bh), Some(&e.data)),
                    }
                }
            }
        }
    }
    let mut gone: Vec<(&String, &J)> = base_entries.iter().filter(|(k, _)| !seen.contains(*k)).collect();
    gone.sort_by(|a, b| a.0.cmp(b.0));
    for (k, bh) in gone {
        b.push("delete", k.clone(), "Wwise entry", bh.as_str(), None);
    }
    let header = json!({ "format": "tdelta", "version": VERSION, "kind": "pck", "target": target, "ops": b.ops });
    let streams = if b.stream.is_empty() { Vec::new() } else { vec![b.stream] };
    Ok(Delta { header, image: Vec::new(), streams, ..Default::default() })
}

/// The payload of an op (`stream: [s, offset, len]`).
fn payload<'a>(delta: &'a Delta, op: &J) -> Result<&'a [u8]> {
    let s = op["stream"].as_array().ok_or_else(|| Error::Format(format!("{}: no payload", op["path"])))?;
    let n = |i: usize| s.get(i).and_then(|v| v.as_u64()).map(|v| v as usize);
    let (si, off, len) = (n(0).unwrap_or(0), n(1).unwrap_or(0), n(2).unwrap_or(0));
    delta.streams.get(si).and_then(|st| st.get(off..off + len)).ok_or_else(|| Error::Format(format!("{}: payload outside the delta", op["path"])))
}

/// Insert `e` where the game's lookup expects it (sorted by id, then language).
fn insert_sorted(entries: &mut Vec<Entry>, e: Entry) {
    let at = entries.partition_point(|x| (x.id, x.language) < (e.id, e.language));
    entries.insert(at, e);
}

/// Apply a `kind: "pck"` delta to a package.
pub fn apply(p: &mut Package, delta: &Delta) -> Result<Applied> {
    if delta.kind() != "pck" {
        return Err(Error::Format(format!("not a Wwise package delta (kind {})", delta.kind())));
    }
    let mut out = Applied::default();
    let ops = delta.header["ops"].as_array().cloned().unwrap_or_default();
    // Part ops grouped by bank, applied with one parse of the bank each.
    let mut bank_ops: BTreeMap<String, Vec<J>> = BTreeMap::new();
    for op in &ops {
        let path = op["path"].as_str().unwrap_or("");
        let kind = op["op"].as_str().unwrap_or("");
        if let Some((entry, _)) = path.split_once('/') {
            bank_ops.entry(entry.to_string()).or_default().push(op.clone());
            continue;
        }
        let Some((t, id, language)) = parse_entry_path(path) else {
            out.warnings.push(format!("{path}: unknown entry"));
            continue;
        };
        let entries = table_mut(p, t);
        let at = entries.iter().position(|e| e.id == id && e.language == language);
        match kind {
            "delete" => match at {
                Some(i) => {
                    if op["base"].as_str().is_some_and(|b| b != hash(&entries[i].data)) {
                        out.drift.push(path.to_string());
                    }
                    entries.remove(i);
                    out.deleted += 1;
                }
                None => out.warnings.push(format!("{path}: not in the package, nothing to delete")),
            },
            "add" | "set" => {
                let data = payload(delta, op)?.to_vec();
                match at {
                    Some(i) => {
                        if op["base"].as_str().is_some_and(|b| b != hash(&entries[i].data)) {
                            out.drift.push(path.to_string());
                        }
                        entries[i].data = data;
                        out.replaced += 1;
                    }
                    None => {
                        // New files take the table's block size (the alignment of its files).
                        let block = entries.first().map(|e| e.block).unwrap_or(1);
                        insert_sorted(entries, Entry { id, block, language, data });
                        out.added += 1;
                    }
                }
            }
            other => out.warnings.push(format!("{path}: unknown operation {other}")),
        }
    }
    for (entry, ops) in bank_ops {
        let Some(("bank", id, language)) = parse_entry_path(&entry) else {
            out.warnings.push(format!("{entry}: parts of something that is not a bank"));
            continue;
        };
        let Some(e) = p.banks.iter_mut().find(|e| e.id == id && e.language == language) else {
            out.warnings.push(format!("{entry}: the bank is not in the package; {} changes skipped", ops.len()));
            continue;
        };
        let mut bank = Bank::parse(&e.data)?;
        apply_bank(&mut bank, &entry, &ops, delta, &mut out)?;
        e.data = bank.to_bytes();
    }
    Ok(out)
}

fn apply_bank(bank: &mut Bank, entry: &str, ops: &[J], delta: &Delta, out: &mut Applied) -> Result<()> {
    let mut objects: Option<Vec<Object>> = None;
    let mut media: Option<Vec<(u32, Vec<u8>)>> = None;
    // Additions last: a new object is placed against its neighbours and
    // parents as the delta leaves them.
    let ordered: Vec<&J> = ops.iter().filter(|o| o["op"] != "add").chain(ops.iter().filter(|o| o["op"] == "add")).collect();
    for op in ordered {
        let path = op["path"].as_str().unwrap_or("");
        let sub = path.split_once('/').map(|(_, s)| s).unwrap_or("");
        let kind = op["op"].as_str().unwrap_or("");
        let base = op["base"].as_str();
        let (what, key) = sub.split_once(':').unwrap_or(("", ""));
        let data = if kind == "delete" { None } else { Some(payload(delta, op)?) };
        match what {
            "hirc" => {
                let id: u32 = key.parse().map_err(|_| Error::Format(format!("{path}: bad object id")))?;
                let list = objects.get_or_insert_with(|| bank.objects());
                let at = list.iter().position(|o| o.id == id);
                let current = |o: &Object| {
                    let mut b = vec![o.kind];
                    b.extend_from_slice(&o.body);
                    hash(&b)
                };
                match (data, at) {
                    (None, Some(i)) => {
                        if base.is_some_and(|b| b != current(&list[i])) {
                            out.drift.push(path.to_string());
                        }
                        list.remove(i);
                        out.deleted += 1;
                    }
                    (None, None) => out.warnings.push(format!("{path}: not in the bank, nothing to delete")),
                    (Some(d), at) => {
                        let (&kind_byte, body) = d.split_first().ok_or_else(|| Error::Format(format!("{path}: empty object")))?;
                        let o = Object { kind: kind_byte, id, body: body.to_vec() };
                        match at {
                            Some(i) => {
                                if base.is_some_and(|b| b != current(&list[i])) {
                                    out.drift.push(path.to_string());
                                }
                                list[i] = o;
                                out.replaced += 1;
                            }
                            None => {
                                // Before the object it preceded in the edited bank, else before
                                // the first object that names it (its parent), as the cooker orders them.
                                let needle = id.to_le_bytes();
                                let before = op["before"].as_str().and_then(|b| b.parse::<u32>().ok()).and_then(|b| list.iter().position(|x| x.id == b));
                                let at = before.or_else(|| list.iter().position(|x| x.body.windows(4).any(|w| w == needle))).unwrap_or(list.len());
                                list.insert(at, o);
                                out.added += 1;
                            }
                        }
                    }
                }
            }
            "media" => {
                let id: u32 = key.parse().map_err(|_| Error::Format(format!("{path}: bad media id")))?;
                let list = media.get_or_insert_with(|| bank.media().into_iter().map(|(i, d)| (i, d.to_vec())).collect());
                let at = list.iter().position(|(i, _)| *i == id);
                match (data, at) {
                    (None, Some(i)) => {
                        if base.is_some_and(|b| b != hash(&list[i].1)) {
                            out.drift.push(path.to_string());
                        }
                        list.remove(i);
                        out.deleted += 1;
                    }
                    (None, None) => out.warnings.push(format!("{path}: not in the bank, nothing to delete")),
                    (Some(d), Some(i)) => {
                        if base.is_some_and(|b| b != hash(&list[i].1)) {
                            out.drift.push(path.to_string());
                        }
                        list[i].1 = d.to_vec();
                        out.replaced += 1;
                    }
                    (Some(d), None) => {
                        // The index is sorted by id, as the bank cooker writes it.
                        let at = list.partition_point(|(i, _)| *i < id);
                        list.insert(at, (id, d.to_vec()));
                        out.added += 1;
                    }
                }
            }
            "chunk" => {
                let tag: [u8; 4] = key.as_bytes().try_into().map_err(|_| Error::Format(format!("{path}: bad chunk tag")))?;
                let at = bank.chunks.iter().position(|(t, _)| *t == tag);
                match (data, at) {
                    (None, Some(i)) => {
                        if base.is_some_and(|b| b != hash(&bank.chunks[i].1)) {
                            out.drift.push(path.to_string());
                        }
                        bank.chunks.remove(i);
                        out.deleted += 1;
                    }
                    (None, None) => out.warnings.push(format!("{path}: not in the bank, nothing to delete")),
                    (Some(d), Some(i)) => {
                        if base.is_some_and(|b| b != hash(&bank.chunks[i].1)) {
                            out.drift.push(path.to_string());
                        }
                        bank.chunks[i].1 = d.to_vec();
                        out.replaced += 1;
                    }
                    (Some(d), None) => {
                        bank.chunks.push((tag, d.to_vec()));
                        out.added += 1;
                    }
                }
            }
            _ => out.warnings.push(format!("{path} ({entry}): unknown part")),
        }
    }
    if let Some(list) = media {
        bank.set_media(&list);
    }
    if let Some(list) = objects {
        bank.set_objects(&list);
    }
    Ok(())
}

/// Entries and bank parts of `expected` and `result` that differ (round-trip check).
pub fn compare(expected: &Package, result: &Package) -> Vec<String> {
    let (de, dr) = (digest(expected), digest(result));
    let empty = Map::new();
    let mut diffs = Vec::new();
    let both = |a: &J, b: &J, prefix: &str, diffs: &mut Vec<String>| {
        let (a, b) = (a.as_object().unwrap_or(&empty), b.as_object().unwrap_or(&empty));
        for (k, h) in a {
            match b.get(k) {
                None => diffs.push(format!("{prefix}{k}: missing")),
                Some(x) if x != h => diffs.push(format!("{prefix}{k}: differs")),
                _ => {}
            }
        }
        for k in b.keys().filter(|k| !a.contains_key(*k)) {
            diffs.push(format!("{prefix}{k}: unexpected"));
        }
    };
    both(&de["entries"], &dr["entries"], "", &mut diffs);
    diffs.sort();
    diffs
}

/// Counts of a pck delta's ops: `(added, changed, deleted)`.
pub fn counts(d: &Delta) -> (usize, usize, usize) {
    let mut c = (0, 0, 0);
    for op in d.header["ops"].as_array().cloned().unwrap_or_default() {
        match op["op"].as_str().unwrap_or("") {
            "add" => c.0 += 1,
            "delete" => c.2 += 1,
            _ => c.1 += 1,
        }
    }
    c
}

/// One line per op for the cooker's preview (`{op, what}`), at most `max`.
pub fn describe(d: &Delta, max: usize) -> Vec<J> {
    d.header["ops"]
        .as_array()
        .map(|ops| {
            ops.iter()
                .take(max)
                .map(|o| {
                    let size = o["size"].as_u64().map(|n| format!(" ({n} bytes)")).unwrap_or_default();
                    json!({ "op": o["op"], "what": format!("{} {}{size}", o["class"].as_str().unwrap_or(""), o["path"].as_str().unwrap_or("")) })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Which delta paths two pck deltas both touch.
pub fn overlap(a: &Delta, b: &Delta) -> Vec<String> {
    let paths = |d: &Delta| -> HashMap<String, ()> { d.header["ops"].as_array().map(|o| o.iter().filter_map(|x| Some((x["path"].as_str()?.to_string(), ()))).collect()).unwrap_or_default() };
    let pa = paths(a);
    let mut out: Vec<String> = paths(b).into_keys().filter(|k| pa.contains_key(k)).collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank(id: u32, objects: &[(u8, u32, &[u8])], media: &[(u32, &[u8])]) -> Vec<u8> {
        let mut b = Bank { version: 120, id, chunks: vec![(*b"BKHD", {
            let mut h = 120u32.to_le_bytes().to_vec();
            h.extend_from_slice(&id.to_le_bytes());
            h.extend_from_slice(&[0; 8]);
            h
        })] };
        b.set_media(&media.iter().map(|(i, d)| (*i, d.to_vec())).collect::<Vec<_>>());
        b.set_objects(&objects.iter().map(|(k, i, body)| Object { kind: *k, id: *i, body: body.to_vec() }).collect::<Vec<_>>());
        b.chunks.push((*b"STID", vec![1, 2, 3, 4]));
        b.to_bytes()
    }

    fn package(banks: Vec<Vec<u8>>, streams: &[(u64, &[u8])]) -> Package {
        let mut p = Package::default();
        p.version = 1;
        p.banks = banks.into_iter().enumerate().map(|(i, data)| Entry { id: 100 + i as u64, block: 1, language: 0, data }).collect();
        p.streams = streams.iter().map(|(id, d)| Entry { id: *id, block: 1, language: 0, data: d.to_vec() }).collect();
        p
    }

    #[test]
    fn round_trip_parts_and_streams() {
        let base = package(vec![bank(7, &[(11, 1, b"track one"), (10, 2, b"segment \x01\x00\x00\x00")], &[(50, b"embedded")])], &[(10, b"stream ten"), (30, b"stream thirty")]);
        // Change one object, add one before its parent, replace embedded media,
        // add a stream between two others and drop one.
        let edited = package(vec![bank(7, &[(11, 3, b"new track"), (11, 1, b"track ONE"), (10, 2, b"segment \x03\x00\x00\x00")], &[(50, b"replaced media")])], &[(10, b"stream ten"), (20, b"stream twenty")]);
        let d = create(Base::Package(&base), &edited, "ChaosGame/CookedPCConsole/Test.pck").unwrap();
        let d = Delta::from_bytes(&d.to_bytes()).unwrap();
        let paths: Vec<&str> = d.header["ops"].as_array().unwrap().iter().map(|o| o["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"bank:100/hirc:1"), "{paths:?}");
        assert!(paths.contains(&"bank:100/hirc:3"));
        assert!(paths.contains(&"bank:100/media:50"));
        assert!(paths.contains(&"stream:20"));
        assert!(paths.contains(&"stream:30"));
        assert!(!paths.iter().any(|p| p.contains("stream:10") || p.contains("chunk:")));
        let mut result = base.clone();
        let a = apply(&mut result, &d).unwrap();
        assert!(a.drift.is_empty() && a.warnings.is_empty(), "{a:?}");
        assert_eq!(result.to_bytes(), edited.to_bytes());
        assert!(compare(&edited, &result).is_empty());
    }

    #[test]
    fn digest_base_and_drift() {
        let base = package(vec![bank(7, &[(11, 1, b"a")], &[])], &[(5, b"five")]);
        let edited = package(vec![bank(7, &[(11, 1, b"b")], &[])], &[(5, b"FIVE")]);
        let dg = digest(&base);
        let d = create(Base::Digest(&dg), &edited, "x.pck").unwrap();
        // Another mod changed the stream first: the delta still wins, as drift.
        let mut other = package(vec![bank(7, &[(11, 1, b"a")], &[])], &[(5, b"other")]);
        let a = apply(&mut other, &d).unwrap();
        assert_eq!(a.drift, vec!["stream:5".to_string()]);
        assert_eq!(other.to_bytes(), edited.to_bytes());
    }
}
