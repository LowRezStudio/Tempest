//! Row deltas for the marshal files (`assembly.dat`, `Lang_<EXT>.dat`):
//! which rows a mod added, changed or removed, keyed by the table's id
//! column, so two mods adding different champions to the same 21 MB
//! `assembly.dat` compose instead of conflicting.
//!
//! Same container as the package deltas (`kind: "mcts"`, no image): the
//! header lists, per top-level row set, its key column and the operations;
//! a row travels as its full JSON (`row_json`), a changed row with the hash
//! of the row it replaces. Tables without a unique id column (none in the
//! game's files so far) are keyed by position.

use std::collections::{HashMap, HashSet};

use md5::{Digest as _, Md5};
use serde_json::{json, Value as J};
use tempest_upk::io::{Error, Result};

use super::{row_from_json, row_json, Marshal, Row, Tokens, Value};
use crate::delta::{Delta, VERSION};

fn hex(h: &[u8]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

fn row_hash(tokens: &Tokens, row: &Row) -> String {
    hex(&Md5::digest(row_json(tokens, row).to_string().as_bytes()))
}

/// The id column of a row set: the first `*_ID` field present in every row
/// with a distinct value per row.
pub fn key_column(tokens: &Tokens, rows: &[Row]) -> Option<u16> {
    let first = rows.first()?;
    for (t, _) in &first.entries {
        if !tokens.name(*t).ends_with("_ID") {
            continue;
        }
        let mut seen = HashSet::with_capacity(rows.len());
        let unique = rows.iter().all(|r| match r.entries.iter().find(|(f, _)| f == t) {
            Some((_, Value::Num(n))) => seen.insert(*n),
            _ => false,
        });
        if unique {
            return Some(*t);
        }
    }
    None
}

fn key_of(row: &Row, key: Option<u16>, index: usize) -> String {
    match key.and_then(|k| row.entries.iter().find(|(t, _)| *t == k)) {
        Some((_, Value::Num(n))) => n.to_string(),
        _ => format!("#{index}"),
    }
}

/// Scalar header fields (everything of the root row that is not a row set).
fn header_fields(tokens: &Tokens, m: &Marshal) -> serde_json::Map<String, J> {
    let mut out = serde_json::Map::new();
    for (t, v) in &m.root.entries {
        if !matches!(v, Value::Rows(_)) {
            out.insert(tokens.name(*t), super::value_json(tokens, *t, v));
        }
    }
    out
}

/// Per table: key column and `key → hash` of every row; plus the header.
pub fn digest(tokens: &Tokens, m: &Marshal) -> J {
    let mut tables = serde_json::Map::new();
    for (t, v) in &m.root.entries {
        let Value::Rows(rows) = v else { continue };
        let key = key_column(tokens, rows);
        let mut map = serde_json::Map::new();
        for (i, r) in rows.iter().enumerate() {
            map.insert(key_of(r, key, i), J::from(row_hash(tokens, r)));
        }
        tables.insert(tokens.name(*t), json!({ "key": key.map(|k| tokens.name(k)), "rows": map }));
    }
    json!({ "format": "tdelta-digest", "version": VERSION, "kind": "mcts", "header": header_fields(tokens, m), "tables": tables })
}

pub enum Base<'a> {
    Marshal(&'a Marshal),
    Digest(&'a J),
}

/// A row of the base: its hash (a digest), or the row itself, hashed only
/// when the edited row differs (most of a marshal is unchanged, and hashing
/// prints every row as JSON first).
enum BaseRow<'a> {
    Hash(&'a str),
    Row(&'a Row),
}

impl BaseRow<'_> {
    fn hash(&self, tokens: &Tokens) -> String {
        match self {
            BaseRow::Hash(h) => h.to_string(),
            BaseRow::Row(r) => row_hash(tokens, r),
        }
    }
}

/// One row set of the base: its key column and rows by key, in file order.
struct BaseTable<'a> {
    key: Option<String>,
    rows: HashMap<String, BaseRow<'a>>,
}

/// The base's row sets in file order and its header, read from a marshal or a digest.
fn base_tables<'a>(tokens: &Tokens, base: &Base<'a>) -> (Vec<(String, BaseTable<'a>)>, serde_json::Map<String, J>) {
    match base {
        Base::Marshal(m) => {
            let mut tables = Vec::new();
            for (t, v) in &m.root.entries {
                let Value::Rows(rows) = v else { continue };
                let key = key_column(tokens, rows);
                let by_key = rows.iter().enumerate().map(|(i, r)| (key_of(r, key, i), BaseRow::Row(r))).collect();
                tables.push((tokens.name(*t), BaseTable { key: key.map(|k| tokens.name(k)), rows: by_key }));
            }
            (tables, header_fields(tokens, m))
        }
        Base::Digest(d) => {
            let tables = d["tables"].as_object().into_iter().flatten().map(|(name, b)| {
                let rows = b["rows"].as_object().map(|m| m.iter().filter_map(|(k, h)| Some((k.clone(), BaseRow::Hash(h.as_str()?)))).collect()).unwrap_or_default();
                (name.clone(), BaseTable { key: b["key"].as_str().map(str::to_string), rows })
            });
            (tables.collect(), d["header"].as_object().cloned().unwrap_or_default())
        }
    }
}

/// What `edited` changed against its base.
pub fn create(tokens: &Tokens, base: Base, edited: &Marshal, target: &str) -> Result<Delta> {
    let (base_tables, base_header) = base_tables(tokens, &base);
    let by_name: HashMap<&str, &BaseTable> = base_tables.iter().map(|(n, b)| (n.as_str(), b)).collect();

    let mut header_ops = serde_json::Map::new();
    for (name, v) in header_fields(tokens, edited) {
        if base_header.get(&name) != Some(&v) {
            header_ops.insert(name, v);
        }
    }

    let no_rows = HashMap::new();
    let mut tables: Vec<J> = Vec::new();
    for (t, v) in &edited.root.entries {
        let Value::Rows(rows) = v else { continue };
        let name = tokens.name(*t);
        let base_table = by_name.get(name.as_str());
        // The base's key column keeps the keys comparable; a new table picks its own.
        let key = match base_table.and_then(|b| b.key.as_deref()) {
            Some(k) => tokens.token(k),
            None => key_column(tokens, rows),
        };
        let base_rows = base_table.map(|b| &b.rows).unwrap_or(&no_rows);
        let mut ops: Vec<J> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for (i, r) in rows.iter().enumerate() {
            let k = key_of(r, key, i);
            match base_rows.get(&k) {
                None => ops.push(json!({ "op": "add", "key": k, "row": row_json(tokens, r) })),
                // Equal rows print (and hash) the same: no need to hash them.
                Some(BaseRow::Row(b)) if *b == r => {}
                Some(b) => {
                    let (bh, h) = (b.hash(tokens), row_hash(tokens, r));
                    if bh != h {
                        ops.push(json!({ "op": "set", "key": k, "base": bh, "row": row_json(tokens, r) }));
                    }
                }
            }
            seen.insert(k);
        }
        let mut deleted: Vec<&String> = base_rows.keys().filter(|k| !seen.contains(*k)).collect();
        deleted.sort();
        for k in deleted {
            ops.push(json!({ "op": "delete", "key": k }));
        }
        if !ops.is_empty() {
            tables.push(json!({ "table": name, "key": key.map(|k| tokens.name(k)), "ops": ops }));
        }
    }
    // Tables the edited file no longer has at all.
    let edited_names: HashSet<String> = edited.root.entries.iter().filter(|(_, v)| matches!(v, Value::Rows(_))).map(|(t, _)| tokens.name(*t)).collect();
    for (name, b) in &base_tables {
        if !edited_names.contains(name) {
            let mut keys: Vec<&String> = b.rows.keys().collect();
            keys.sort();
            tables.push(json!({ "table": name, "key": b.key, "ops": keys.into_iter().map(|k| json!({ "op": "delete", "key": k })).collect::<Vec<_>>() }));
        }
    }

    let header = json!({
        "format": "tdelta",
        "version": VERSION,
        "kind": "mcts",
        "target": target,
        "header": header_ops,
        "tables": tables,
    });
    Ok(Delta { header, image: Vec::new(), ..Default::default() })
}

#[derive(Clone, Debug, Default)]
pub struct Applied {
    pub added: usize,
    pub replaced: usize,
    pub deleted: usize,
    pub header: usize,
    pub drift: Vec<String>,
    pub warnings: Vec<String>,
}

/// Apply a `kind: "mcts"` delta to a marshal.
pub fn apply(tokens: &Tokens, m: &mut Marshal, delta: &Delta) -> Result<Applied> {
    if delta.kind() != "mcts" {
        return Err(Error::Format(format!("not a marshal delta (kind {})", delta.kind())));
    }
    let mut out = Applied::default();
    if let Some(h) = delta.header["header"].as_object() {
        for (name, v) in h {
            let token = tokens.token(name).ok_or_else(|| Error::Format(format!("unknown header field {name}")))?;
            let value = super::value_from_json(tokens, token, v)?;
            match m.root.entries.iter_mut().find(|(t, _)| *t == token) {
                Some(e) => e.1 = value,
                None => m.root.entries.push((token, value)),
            }
            out.header += 1;
        }
    }
    for table in delta.header["tables"].as_array().cloned().unwrap_or_default() {
        let name = table["table"].as_str().unwrap_or("");
        let token = tokens.token(name).ok_or_else(|| Error::Format(format!("unknown table {name}")))?;
        let key = table["key"].as_str().and_then(|k| tokens.token(k));
        let rows = match m.root.entries.iter_mut().find(|(t, _)| *t == token) {
            Some((_, Value::Rows(rows))) => rows,
            Some(_) => return Err(Error::Format(format!("{name} is not a row set"))),
            None => {
                m.root.entries.push((token, Value::Rows(Box::default())));
                match &mut m.root.entries.last_mut().unwrap().1 {
                    Value::Rows(rows) => rows,
                    _ => unreachable!(),
                }
            }
        };
        let mut index: HashMap<String, usize> = rows.iter().enumerate().map(|(i, r)| (key_of(r, key, i), i)).collect();
        for op in table["ops"].as_array().cloned().unwrap_or_default() {
            let k = op["key"].as_str().unwrap_or("").to_string();
            match op["op"].as_str().unwrap_or("") {
                "delete" => match index.remove(&k) {
                    Some(i) => {
                        rows.remove(i);
                        for v in index.values_mut() {
                            if *v > i {
                                *v -= 1;
                            }
                        }
                        out.deleted += 1;
                    }
                    None => out.warnings.push(format!("{name}: row {k} not found, nothing to delete")),
                },
                "set" | "add" => {
                    let row = row_from_json(tokens, &op["row"])?;
                    match index.get(&k) {
                        Some(&i) => {
                            if let Some(base) = op["base"].as_str() {
                                if row_hash(tokens, &rows[i]) != base {
                                    out.drift.push(format!("{name}:{k}"));
                                }
                            }
                            rows[i] = row;
                            out.replaced += 1;
                        }
                        None => {
                            rows.push(row);
                            index.insert(k, rows.len() - 1);
                            out.added += 1;
                        }
                    }
                }
                other => out.warnings.push(format!("{name}: unknown operation {other}")),
            }
        }
    }
    Ok(out)
}

/// Rows two marshal deltas both touch, as `table:key`.
pub fn overlap(a: &Delta, b: &Delta) -> Vec<String> {
    fn keys(d: &Delta) -> HashSet<String> {
        let mut out = HashSet::new();
        for t in d.header["tables"].as_array().cloned().unwrap_or_default() {
            let name = t["table"].as_str().unwrap_or("");
            for op in t["ops"].as_array().cloned().unwrap_or_default() {
                out.insert(format!("{name}:{}", op["key"].as_str().unwrap_or("")));
            }
        }
        if let Some(h) = d.header["header"].as_object() {
            for k in h.keys() {
                out.insert(format!("header:{k}"));
            }
        }
        out
    }
    let ka = keys(a);
    let mut out: Vec<String> = keys(b).into_iter().filter(|k| ka.contains(k)).collect();
    out.sort();
    out
}

/// Rows of `expected` and `result` that differ (round-trip check).
pub fn compare(tokens: &Tokens, expected: &Marshal, result: &Marshal) -> Vec<String> {
    let de = digest(tokens, expected);
    let dr = digest(tokens, result);
    let mut diffs = Vec::new();
    if de["header"] != dr["header"] {
        diffs.push("header differs".to_string());
    }
    let empty = serde_json::Map::new();
    let te = de["tables"].as_object().unwrap_or(&empty);
    let tr = dr["tables"].as_object().unwrap_or(&empty);
    for (name, t) in te {
        let Some(r) = tr.get(name) else {
            diffs.push(format!("{name}: missing"));
            continue;
        };
        let (re, rr) = (t["rows"].as_object().unwrap_or(&empty), r["rows"].as_object().unwrap_or(&empty));
        for (k, h) in re {
            match rr.get(k) {
                None => diffs.push(format!("{name}:{k}: missing")),
                Some(hr) if hr != h => diffs.push(format!("{name}:{k}: differs")),
                _ => {}
            }
        }
        for k in rr.keys() {
            if !re.contains_key(k) {
                diffs.push(format!("{name}:{k}: unexpected"));
            }
        }
    }
    for name in tr.keys() {
        if !te.contains_key(name) {
            diffs.push(format!("{name}: unexpected table"));
        }
    }
    diffs.sort();
    diffs
}
