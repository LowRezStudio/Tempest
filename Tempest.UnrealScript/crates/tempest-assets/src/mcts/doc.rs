//! An editable `.dat` marshal: `assembly.dat` (the game's content database:
//! items, devices, skins, bots, effects, resources… as row sets) or a
//! `Lang_<EXT>.dat` (one `DATA_SET` of messages). Rows are addressed by a
//! path of `(field, index)` pairs from the root, so nested row sets (an
//! item's `DATA_SET_ITEM_PROPS`) are edited like top-level ones. Every
//! mutation is journaled for undo.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, Result};

use super::{num_parse, parse_obscured, row_json, value_from_json, value_json, write_obscured, Kind, Marshal, Row, Tokens, Value};

/// A row set inside a document: field tokens and row indices from the root.
pub type RowPath = Vec<(u16, usize)>;

enum Undo {
    Set { path: RowPath, old: Row },
    Insert { path: RowPath },
    Delete { path: RowPath, old: Row },
    Header { old: Row },
}

pub struct Doc {
    pub path: PathBuf,
    pub marshal: Marshal,
    pub dirty: bool,
    undo: Vec<(String, Undo)>,
}

/// Summary of one row set: its columns in first-seen order with how many
/// rows carry each, and the key column.
pub struct TableInfo {
    pub token: u16,
    pub rows: usize,
    pub columns: Vec<(u16, usize)>,
}

impl Doc {
    pub fn open(path: &Path, tokens: &Tokens) -> Result<Doc> {
        let bytes = std::fs::read(path)?;
        let marshal = parse_obscured(&bytes, tokens)?;
        Ok(Doc { path: path.to_path_buf(), marshal, dirty: false, undo: Vec::new() })
    }

    pub fn undo_labels(&self) -> Vec<String> {
        self.undo.iter().rev().map(|(l, _)| l.clone()).collect()
    }

    /// Write the document (obscured) to `out`, or in place.
    pub fn save(&mut self, tokens: &Tokens, out: Option<&Path>) -> Result<PathBuf> {
        let bytes = write_obscured(&self.marshal, tokens);
        let target = out.map(|p| p.to_path_buf()).unwrap_or_else(|| self.path.clone());
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, bytes)?;
        if out.is_none() {
            self.dirty = false;
        }
        Ok(target)
    }

    // ----- navigation ---------------------------------------------------------

    /// The row set a path's last element belongs to (`path` names a row; the
    /// row set holding it is `path[..n-1]` + the last token).
    fn rows_at(&self, parent: &[(u16, usize)], token: u16) -> Result<&Vec<Row>> {
        self.row_at(parent)?.rows(token).ok_or_else(|| Error::Format(format!("this row has no #{token} row set")))
    }

    fn rows_at_mut(&mut self, parent: &[(u16, usize)], token: u16) -> Result<&mut Vec<Row>> {
        self.row_at_mut(parent)?.rows_mut(token).ok_or_else(|| Error::Format(format!("this row has no #{token} row set")))
    }

    pub fn row_at(&self, path: &[(u16, usize)]) -> Result<&Row> {
        let mut row = &self.marshal.root;
        for (token, index) in path {
            let rows = row.rows(*token).ok_or_else(|| Error::Format(format!("field #{token} is not a row set")))?;
            row = rows.get(*index).ok_or_else(|| Error::Format(format!("row {index} is out of range (the set holds {})", rows.len())))?;
        }
        Ok(row)
    }

    pub fn row_at_mut(&mut self, path: &[(u16, usize)]) -> Result<&mut Row> {
        let mut row = &mut self.marshal.root;
        for (token, index) in path {
            let rows = row.rows_mut(*token).ok_or_else(|| Error::Format(format!("field #{token} is not a row set")))?;
            let n = rows.len();
            row = rows.get_mut(*index).ok_or_else(|| Error::Format(format!("row {index} is out of range (the set holds {n})")))?;
        }
        Ok(row)
    }

    /// Columns of a row set (union over its rows, first-seen order).
    pub fn table_info(&self, parent: &[(u16, usize)], token: u16) -> Result<TableInfo> {
        let rows = self.rows_at(parent, token)?;
        let mut columns: Vec<(u16, usize)> = Vec::new();
        let mut seen: HashMap<u16, usize> = HashMap::new();
        for r in rows {
            for (t, _) in &r.entries {
                match seen.get(t) {
                    Some(&i) => columns[i].1 += 1,
                    None => {
                        seen.insert(*t, columns.len());
                        columns.push((*t, 1));
                    }
                }
            }
        }
        Ok(TableInfo { token, rows: rows.len(), columns })
    }

    /// The row sets of the root row (the tables of an assembly) and the
    /// scalar header fields.
    pub fn tables(&self) -> (Vec<u16>, Vec<u16>) {
        let mut tables = Vec::new();
        let mut header = Vec::new();
        for (t, v) in &self.marshal.root.entries {
            match v {
                Value::Rows(_) => tables.push(*t),
                _ => header.push(*t),
            }
        }
        (tables, header)
    }

    // ----- edits ------------------------------------------------------------

    /// Set one field of a row from JSON (a number, string, hex GUID…); a
    /// `null` removes the field.
    pub fn set(&mut self, tokens: &Tokens, path: &[(u16, usize)], field: u16, value: &J) -> Result<()> {
        let old = self.row_at(path)?.clone();
        let new_value = if value.is_null() { None } else { Some(value_from_json(tokens, field, value)?) };
        let row = self.row_at_mut(path)?;
        match new_value {
            Some(v) => row.set(field, v),
            None => {
                row.remove(field);
            }
        }
        if *row == old {
            return Ok(());
        }
        self.undo.push((format!("set {}", tokens.name(field)), Undo::Set { path: path.to_vec(), old }));
        self.dirty = true;
        Ok(())
    }

    /// Set a field from text typed by the user.
    pub fn set_text(&mut self, tokens: &Tokens, path: &[(u16, usize)], field: u16, text: &str) -> Result<()> {
        let v = match tokens.kind(field) {
            Kind::String => json!(text),
            Kind::RowSet => return Err(Error::Format("a row set cannot be typed".into())),
            Kind::Guid | Kind::Blob => json!(text),
            kind => json!(num_parse(kind, text)?),
        };
        // Numbers were parsed into raw bits already: route them straight in.
        if tokens.kind(field).is_numeric() {
            let old = self.row_at(path)?.clone();
            let bits = v.as_u64().unwrap_or(0);
            self.row_at_mut(path)?.set(field, Value::Num(bits));
            if *self.row_at(path)? != old {
                self.undo.push((format!("set {}", tokens.name(field)), Undo::Set { path: path.to_vec(), old }));
                self.dirty = true;
            }
            return Ok(());
        }
        self.set(tokens, path, field, &v)
    }

    /// Replace a whole row from JSON.
    pub fn replace_row(&mut self, tokens: &Tokens, path: &[(u16, usize)], value: &J) -> Result<()> {
        let new_row = super::row_from_json(tokens, value)?;
        let old = std::mem::replace(self.row_at_mut(path)?, new_row);
        self.undo.push(("edit row".into(), Undo::Set { path: path.to_vec(), old }));
        self.dirty = true;
        Ok(())
    }

    /// Insert a row into a row set at `index` (append when `None`): a copy of
    /// `clone` (a row index of the same set) or an empty row.
    pub fn insert(&mut self, tokens: &Tokens, parent: &[(u16, usize)], token: u16, index: Option<usize>, clone: Option<usize>) -> Result<usize> {
        let rows = self.rows_at_mut(parent, token)?;
        let row = match clone {
            Some(i) => rows.get(i).ok_or_else(|| Error::Format(format!("row {i} is out of range")))?.clone(),
            None => Row::default(),
        };
        let at = index.unwrap_or(rows.len()).min(rows.len());
        rows.insert(at, row);
        let mut path = parent.to_vec();
        path.push((token, at));
        self.undo.push((format!("insert {} row", tokens.name(token)), Undo::Insert { path }));
        self.dirty = true;
        Ok(at)
    }

    /// Append a ready-made row to a row set (undoable).
    pub fn insert_row(&mut self, tokens: &Tokens, parent: &[(u16, usize)], token: u16, row: Row) -> Result<usize> {
        let rows = self.rows_at_mut(parent, token)?;
        rows.push(row);
        let at = rows.len() - 1;
        let mut path = parent.to_vec();
        path.push((token, at));
        self.undo.push((format!("add {} row", tokens.name(token)), Undo::Insert { path }));
        self.dirty = true;
        Ok(at)
    }

    pub fn delete(&mut self, tokens: &Tokens, path: &[(u16, usize)]) -> Result<()> {
        let Some(((token, index), parent)) = path.split_last() else {
            return Err(Error::Format("the root row cannot be deleted".into()));
        };
        let rows = self.rows_at_mut(parent, *token)?;
        if *index >= rows.len() {
            return Err(Error::Format(format!("row {index} is out of range")));
        }
        let old = rows.remove(*index);
        self.undo.push((format!("delete {} row", tokens.name(*token)), Undo::Delete { path: path.to_vec(), old }));
        self.dirty = true;
        Ok(())
    }

    /// Set a scalar header field of the root row.
    pub fn set_header(&mut self, tokens: &Tokens, field: u16, value: &J) -> Result<()> {
        let old = self.marshal.root.clone();
        let v = value_from_json(tokens, field, value)?;
        self.marshal.root.set(field, v);
        self.undo.push((format!("set {}", tokens.name(field)), Undo::Header { old }));
        self.dirty = true;
        Ok(())
    }

    /// Undo the last edit; the label of what was undone.
    pub fn undo(&mut self) -> Result<Option<String>> {
        let Some((label, entry)) = self.undo.pop() else { return Ok(None) };
        match entry {
            Undo::Set { path, old } => *self.row_at_mut(&path)? = old,
            Undo::Insert { path } => {
                let ((token, index), parent) = path.split_last().unwrap();
                let rows = self.rows_at_mut(parent, *token)?;
                if *index < rows.len() {
                    rows.remove(*index);
                }
            }
            Undo::Delete { path, old } => {
                let ((token, index), parent) = path.split_last().unwrap();
                let rows = self.rows_at_mut(parent, *token)?;
                rows.insert((*index).min(rows.len()), old);
            }
            Undo::Header { old } => self.marshal.root = old,
        }
        self.dirty = true;
        Ok(Some(label))
    }

    // ----- queries ----------------------------------------------------------

    /// Rows of a set as JSON for a grid: `[index, cells]`, nested row sets
    /// summarised as `{rows: n}`. Optional case-insensitive substring filter
    /// over the text of every scalar cell, and a sort column.
    #[allow(clippy::too_many_arguments)]
    pub fn page(&self, tokens: &Tokens, parent: &[(u16, usize)], token: u16, offset: usize, limit: usize, filter: Option<&str>, sort: Option<(u16, bool)>) -> Result<J> {
        let rows = self.rows_at(parent, token)?;
        let needle = filter.map(|f| f.to_lowercase()).filter(|f| !f.is_empty());
        let mut picked: Vec<usize> = (0..rows.len())
            .filter(|&i| match &needle {
                None => true,
                Some(n) => rows[i].entries.iter().any(|(t, v)| cell_text(tokens, *t, v).to_lowercase().contains(n.as_str())),
            })
            .collect();
        if let Some((col, desc)) = sort {
            let kind = tokens.kind(col);
            picked.sort_by(|&a, &b| {
                let o = compare(kind, rows[a].get(col), rows[b].get(col));
                if desc { o.reverse() } else { o }
            });
        }
        let total = picked.len();
        let page: Vec<J> = picked.iter().skip(offset).take(limit).map(|&i| json!([i, grid_row(tokens, &rows[i])])).collect();
        Ok(json!({ "total": total, "offset": offset, "rows": page }))
    }

    /// Find the rows of a set whose `key` field equals `value`.
    pub fn find(&self, parent: &[(u16, usize)], token: u16, key: u16, value: u64) -> Result<Vec<usize>> {
        let rows = self.rows_at(parent, token)?;
        Ok(rows.iter().enumerate().filter(|(_, r)| r.num(key) == Some(value)).map(|(i, _)| i).collect())
    }
}

/// A row for the grid: scalar cells as JSON, row sets as `{rows: n}`.
pub fn grid_row(tokens: &Tokens, row: &Row) -> J {
    let mut m = serde_json::Map::new();
    for (t, v) in &row.entries {
        let cell = match v {
            Value::Rows(rows) => json!({ "rows": rows.len() }),
            other => value_json(tokens, *t, other),
        };
        m.insert(tokens.name(*t), cell);
    }
    J::Object(m)
}

/// Text of a scalar cell, as shown and searched.
pub fn cell_text(tokens: &Tokens, token: u16, v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Rows(r) => format!("{} rows", r.len()),
        Value::Num(n) => match tokens.kind(token) {
            Kind::Int => (*n as u32 as i32).to_string(),
            Kind::Float => f32::from_bits(*n as u32).to_string(),
            Kind::Double => f64::from_bits(*n).to_string(),
            _ => n.to_string(),
        },
        Value::Guid(g) => g.iter().map(|b| format!("{b:02x}")).collect(),
        Value::Blob(b) => format!("{} bytes", b.len()),
    }
}

fn compare(kind: Kind, a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(Value::Num(x)), Some(Value::Num(y))) => match kind {
            Kind::Int => (*x as u32 as i32).cmp(&(*y as u32 as i32)),
            Kind::Float => f32::from_bits(*x as u32).partial_cmp(&f32::from_bits(*y as u32)).unwrap_or(Ordering::Equal),
            Kind::Double => f64::from_bits(*x).partial_cmp(&f64::from_bits(*y)).unwrap_or(Ordering::Equal),
            _ => x.cmp(y),
        },
        (Some(Value::Str(x)), Some(Value::Str(y))) => x.to_lowercase().cmp(&y.to_lowercase()),
        (Some(Value::Rows(x)), Some(Value::Rows(y))) => x.len().cmp(&y.len()),
        _ => Ordering::Equal,
    }
}

/// Full JSON of a row (nested row sets included).
pub fn full_row(tokens: &Tokens, row: &Row) -> J {
    row_json(tokens, row)
}
