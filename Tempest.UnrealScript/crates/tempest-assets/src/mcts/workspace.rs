//! The Studio's view of a game's `.dat` marshals: the assembly and every
//! language file of a cooked folder, opened together, addressed by a
//! document id (`assembly`, `INT`, `FRA`…), with the JSON shapes `upk
//! serve` returns.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, Result};

use super::doc::{full_row, Doc, RowPath};
use super::{Kind, Tokens, Value};

pub struct Workspace {
    pub dir: PathBuf,
    pub tokens: Tokens,
    pub docs: Vec<(String, Doc)>,
    /// `MSG_ID` → row index of the message set, per language document, with
    /// the edit count it was built at.
    msg_index: HashMap<String, (usize, HashMap<u64, usize>)>,
    edits: HashMap<String, usize>,
    /// `start..end` new ids are taken from (`mcts.idRange`); none = past the largest in use.
    pub id_range: Option<(u64, u64)>,
}

/// `Lang_<EXT>.dat` files under a `Localization` folder next to the cooked folder.
fn language_files(cooked: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let parent = cooked.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| cooked.to_path_buf());
    for base in [parent.join("Localization"), cooked.join("Localization")] {
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        let mut dirs: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for d in dirs {
            let ext = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let f = d.join(format!("Lang_{ext}.dat"));
            if f.is_file() {
                out.push((ext.to_ascii_uppercase(), f));
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    // INT first: it is the default language and what the editor shows inline.
    out.sort_by_key(|(e, _)| (e != "INT", e.clone()));
    out
}

impl Workspace {
    /// Open `assembly.dat` of the cooked folder and the language files.
    pub fn open(cooked: &Path) -> Result<Workspace> {
        let tokens = Tokens::for_dir(cooked);
        let mut docs = Vec::new();
        let asm = cooked.join("assembly.dat");
        if asm.is_file() {
            docs.push(("assembly".to_string(), Doc::open(&asm, &tokens)?));
        }
        for (ext, path) in language_files(cooked) {
            match Doc::open(&path, &tokens) {
                Ok(d) => docs.push((ext, d)),
                Err(e) => eprintln!("mcts: {}: {e}", path.display()),
            }
        }
        if docs.is_empty() {
            return Err(Error::Format(format!("no assembly.dat or Localization/*/Lang_*.dat next to {}", cooked.display())));
        }
        Ok(Workspace { dir: cooked.to_path_buf(), tokens, docs, msg_index: HashMap::new(), edits: HashMap::new(), id_range: None })
    }

    pub fn doc(&self, id: &str) -> Result<&Doc> {
        self.docs.iter().find(|(k, _)| k == id).map(|(_, d)| d).ok_or_else(|| Error::Format(format!("no document {id}")))
    }

    pub fn doc_mut(&mut self, id: &str) -> Result<&mut Doc> {
        *self.edits.entry(id.to_string()).or_insert(0) += 1;
        self.docs.iter_mut().find(|(k, _)| k == id).map(|(_, d)| d).ok_or_else(|| Error::Format(format!("no document {id}")))
    }

    fn token(&self, name: &str) -> Result<u16> {
        self.tokens.token(name).ok_or_else(|| Error::Format(format!("unknown field {name}")))
    }

    /// A row path from JSON: `[["DATA_SET_ITEMS", 12], ["DATA_SET_ITEM_PROPS", 0]]`.
    pub fn path(&self, v: Option<&J>) -> Result<RowPath> {
        let Some(arr) = v.and_then(|v| v.as_array()) else { return Ok(Vec::new()) };
        arr.iter()
            .map(|e| {
                let pair = e.as_array().filter(|p| p.len() == 2).ok_or_else(|| Error::Format("path element must be [field, index]".into()))?;
                let token = match &pair[0] {
                    J::String(s) => self.token(s)?,
                    J::Number(n) => n.as_u64().unwrap_or(0) as u16,
                    _ => return Err(Error::Format("path field must be a name".into())),
                };
                let index = pair[1].as_u64().ok_or_else(|| Error::Format("path index must be a number".into()))? as usize;
                Ok((token, index))
            })
            .collect()
    }

    fn column_json(&self, token: u16, count: usize) -> J {
        let kind = self.tokens.kind(token);
        json!({ "name": self.tokens.name(token), "token": token, "kind": kind.name(), "count": count })
    }

    /// Summary of one document: header fields and top-level tables.
    pub fn doc_json(&self, id: &str) -> Result<J> {
        let doc = self.doc(id)?;
        let (tables, header) = doc.tables();
        let tables: Vec<J> = tables
            .iter()
            .map(|t| {
                let info = doc.table_info(&[], *t)?;
                Ok(json!({
                    "name": self.tokens.name(*t),
                    "token": t,
                    "rows": info.rows,
                    "columns": info.columns.iter().map(|(c, n)| self.column_json(*c, *n)).collect::<Vec<_>>(),
                }))
            })
            .collect::<Result<_>>()?;
        let header: Vec<J> = header
            .iter()
            .map(|t| {
                let v = doc.marshal.root.get(*t).unwrap();
                json!({ "name": self.tokens.name(*t), "token": t, "kind": self.tokens.kind(*t).name(), "value": super::value_json(&self.tokens, *t, v) })
            })
            .collect();
        Ok(json!({
            "id": id,
            "path": doc.path.to_string_lossy(),
            "function": self.tokens.function_name(doc.marshal.function).map(|s| s.to_string()).unwrap_or_else(|| format!("{:#x}", doc.marshal.function)),
            "flags": doc.marshal.flags,
            "dirty": doc.dirty,
            "undo": doc.undo_labels(),
            "header": header,
            "tables": tables,
        }))
    }

    pub fn summary(&self) -> Result<J> {
        let docs: Vec<J> = self.docs.iter().map(|(id, _)| self.doc_json(id)).collect::<Result<_>>()?;
        Ok(json!({ "tokens": self.tokens.source, "fields": self.tokens.field_count(), "docs": docs }))
    }

    /// Columns of a (possibly nested) row set.
    pub fn table(&self, p: &J) -> Result<J> {
        let doc = self.doc(p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly"))?;
        let path = self.path(p.get("path"))?;
        let table = self.token(p.get("table").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("missing table".into()))?)?;
        let info = doc.table_info(&path, table)?;
        Ok(json!({ "name": self.tokens.name(table), "token": table, "rows": info.rows, "columns": info.columns.iter().map(|(c, n)| self.column_json(*c, *n)).collect::<Vec<_>>() }))
    }

    pub fn rows(&self, p: &J) -> Result<J> {
        let doc = self.doc(p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly"))?;
        let path = self.path(p.get("path"))?;
        let table = self.token(p.get("table").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("missing table".into()))?)?;
        let offset = p.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = p.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
        let filter = p.get("filter").and_then(|v| v.as_str());
        let sort = match p.get("sort").and_then(|v| v.as_str()) {
            Some(col) if !col.is_empty() => Some((self.token(col)?, p.get("desc").and_then(|v| v.as_bool()).unwrap_or(false))),
            _ => None,
        };
        doc.page(&self.tokens, &path, table, offset, limit, filter, sort)
    }

    /// One row in full, with the kind of every field.
    pub fn row(&self, p: &J) -> Result<J> {
        let doc = self.doc(p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly"))?;
        let path = self.path(p.get("path"))?;
        let row = doc.row_at(&path)?;
        let fields: Vec<J> = row
            .entries
            .iter()
            .map(|(t, v)| {
                let kind = self.tokens.kind(*t);
                let value = match v {
                    Value::Rows(rows) => json!({ "rows": rows.len() }),
                    other => super::value_json(&self.tokens, *t, other),
                };
                json!({ "name": self.tokens.name(*t), "token": t, "kind": kind.name(), "value": value })
            })
            .collect();
        Ok(json!({ "fields": fields }))
    }

    /// The whole row as JSON (nested row sets included), for export.
    pub fn row_full(&self, p: &J) -> Result<J> {
        let doc = self.doc(p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly"))?;
        let path = self.path(p.get("path"))?;
        Ok(full_row(&self.tokens, doc.row_at(&path)?))
    }

    pub(crate) fn status(&self, id: &str) -> Result<J> {
        let doc = self.doc(id)?;
        Ok(json!({ "doc": id, "dirty": doc.dirty, "undo": doc.undo_labels() }))
    }

    pub fn set(&mut self, p: &J) -> Result<J> {
        let id = p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly").to_string();
        let path = self.path(p.get("path"))?;
        let field = self.token(p.get("field").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("missing field".into()))?)?;
        let tokens = self.tokens.clone();
        let doc = self.doc_mut(&id)?;
        match p.get("text").and_then(|v| v.as_str()) {
            Some(text) => doc.set_text(&tokens, &path, field, text)?,
            None => doc.set(&tokens, &path, field, p.get("value").unwrap_or(&J::Null))?,
        }
        let row = doc.row_at(&path)?.clone();
        let mut out = self.status(&id)?;
        out["row"] = super::doc::grid_row(&self.tokens, &row);
        Ok(out)
    }

    pub fn set_header(&mut self, p: &J) -> Result<J> {
        let id = p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly").to_string();
        let field = self.token(p.get("field").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("missing field".into()))?)?;
        let tokens = self.tokens.clone();
        let value = match p.get("text").and_then(|v| v.as_str()) {
            Some(text) if tokens.kind(field).is_numeric() => json!(super::num_parse(tokens.kind(field), text)?),
            Some(text) => json!(text),
            None => p.get("value").cloned().unwrap_or(J::Null),
        };
        self.doc_mut(&id)?.set_header(&tokens, field, &value)?;
        self.status(&id)
    }

    pub fn insert(&mut self, p: &J) -> Result<J> {
        let id = p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly").to_string();
        let path = self.path(p.get("path"))?;
        let table = self.token(p.get("table").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("missing table".into()))?)?;
        let index = p.get("index").and_then(|v| v.as_u64()).map(|v| v as usize);
        let clone = p.get("clone").and_then(|v| v.as_u64()).map(|v| v as usize);
        let tokens = self.tokens.clone();
        let at = self.doc_mut(&id)?.insert(&tokens, &path, table, index, clone)?;
        let mut out = self.status(&id)?;
        out["index"] = json!(at);
        Ok(out)
    }

    pub fn delete(&mut self, p: &J) -> Result<J> {
        let id = p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly").to_string();
        let path = self.path(p.get("path"))?;
        let tokens = self.tokens.clone();
        self.doc_mut(&id)?.delete(&tokens, &path)?;
        self.status(&id)
    }

    pub fn undo(&mut self, p: &J) -> Result<J> {
        let id = p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly").to_string();
        let label = self.doc_mut(&id)?.undo()?;
        let mut out = self.status(&id)?;
        out["undone"] = json!(label);
        Ok(out)
    }

    pub fn save(&mut self, p: &J) -> Result<J> {
        let id = p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly").to_string();
        let out = p.get("out").and_then(|v| v.as_str()).map(PathBuf::from);
        let tokens = self.tokens.clone();
        let doc = self.doc_mut(&id)?;
        let written = doc.save(&tokens, out.as_deref())?;
        let mut st = self.status(&id)?;
        st["out"] = json!(written.to_string_lossy());
        Ok(st)
    }

    /// Reload a document from disk, dropping its edits.
    pub fn discard(&mut self, p: &J) -> Result<J> {
        let id = p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly").to_string();
        let path = self.doc(&id)?.path.clone();
        let fresh = Doc::open(&path, &self.tokens)?;
        *self.doc_mut(&id)? = fresh;
        self.status(&id)
    }

    /// Row indices of a top-level table whose key equals a value: for
    /// following references (`*_ID` columns).
    pub fn find(&self, p: &J) -> Result<J> {
        let doc = self.doc(p.get("doc").and_then(|v| v.as_str()).unwrap_or("assembly"))?;
        let table = self.token(p.get("table").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("missing table".into()))?)?;
        let key = self.token(p.get("key").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("missing key".into()))?)?;
        let value = match p.get("value") {
            Some(J::Number(n)) => n.as_u64().or_else(|| n.as_i64().map(|i| i as i32 as u32 as u64)).unwrap_or(0),
            Some(J::String(s)) => super::num_parse(self.tokens.kind(key), s)?,
            _ => return Err(Error::Format("missing value".into())),
        };
        Ok(json!(doc.find(&[], table, key, value)?))
    }

    fn message_index(&mut self, lang: &str) -> Result<&HashMap<u64, usize>> {
        let edits = self.edits.get(lang).copied().unwrap_or(0);
        let stale = self.msg_index.get(lang).map(|(e, _)| *e != edits).unwrap_or(true);
        if stale {
            let doc = self.doc(lang)?;
            let msg_id = self.token("MSG_ID")?;
            let mut map = HashMap::new();
            for set in ["DATA_SET_MSG_TRANSLATIONS", "DATA_SET"] {
                if let Some(rows) = self.tokens.token(set).and_then(|t| doc.marshal.root.rows(t)) {
                    for (i, r) in rows.iter().enumerate() {
                        if let Some(id) = r.num(msg_id) {
                            map.entry(id).or_insert(i);
                        }
                    }
                    break;
                }
            }
            self.msg_index.insert(lang.to_string(), (edits, map));
        }
        Ok(&self.msg_index.get(lang).unwrap().1)
    }

    /// The message set field of a language document.
    fn message_set(&self, lang: &str) -> Result<u16> {
        let doc = self.doc(lang)?;
        for set in ["DATA_SET_MSG_TRANSLATIONS", "DATA_SET"] {
            if let Some(t) = self.tokens.token(set) {
                if doc.marshal.root.rows(t).is_some() {
                    return Ok(t);
                }
            }
        }
        Err(Error::Format(format!("{lang} holds no message set")))
    }

    /// Texts of messages by id, in every language: `{id: {EXT: {text, path}}}`.
    pub fn messages(&mut self, p: &J) -> Result<J> {
        let ids: Vec<u64> = p.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
        let langs: Vec<String> = match p.get("langs").and_then(|v| v.as_array()) {
            Some(a) => a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect(),
            None => self.docs.iter().map(|(k, _)| k.clone()).filter(|k| k != "assembly").collect(),
        };
        let message = self.token("MESSAGE")?;
        let name = self.token("NAME")?;
        let mut out = serde_json::Map::new();
        for lang in langs {
            let set = self.message_set(&lang)?;
            let index = self.message_index(&lang)?.clone();
            let doc = self.doc(&lang)?;
            let rows = doc.marshal.root.rows(set).unwrap();
            for id in &ids {
                if let Some(&i) = index.get(id) {
                    let r = &rows[i];
                    let entry = out.entry(id.to_string()).or_insert_with(|| json!({}));
                    entry[&lang] = json!({ "text": r.str(message).unwrap_or(""), "name": r.str(name).unwrap_or(""), "path": [[self.tokens.name(set), i]] });
                }
            }
        }
        Ok(J::Object(out))
    }

    /// Set the text of a message (by id) in one language.
    pub fn set_message(&mut self, p: &J) -> Result<J> {
        let lang = p.get("lang").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("missing lang".into()))?.to_string();
        let id = p.get("id").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("missing id".into()))?;
        let text = p.get("text").and_then(|v| v.as_str()).ok_or_else(|| Error::Format("missing text".into()))?.to_string();
        let set = self.message_set(&lang)?;
        let index = self.message_index(&lang)?.get(&id).copied();
        let message = self.token("MESSAGE")?;
        let tokens = self.tokens.clone();
        match index {
            Some(i) => {
                self.doc_mut(&lang)?.set(&tokens, &[(set, i)], message, &json!(text))?;
            }
            None => {
                // A new message: clone the first row for its shape, then set id, name and text.
                let msg_id = self.token("MSG_ID")?;
                let name = self.token("NAME")?;
                let new_name = p.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let doc = self.doc_mut(&lang)?;
                let at = doc.insert(&tokens, &[], set, None, Some(0))?;
                let path = [(set, at)];
                doc.set(&tokens, &path, msg_id, &json!(id))?;
                doc.set(&tokens, &path, name, &json!(new_name))?;
                doc.set(&tokens, &path, message, &json!(text))?;
            }
        }
        self.status(&lang)
    }

    /// Every valid value (`DATA_SET_VALID_VALUES`) with its group and INT
    /// text, keyed by `VALUE_ID`: the enumerations the `*_VALUE_ID` columns
    /// refer to.
    pub fn valid_values(&mut self) -> Result<J> {
        let set = self.token("DATA_SET_VALID_VALUES")?;
        let value_id = self.token("VALUE_ID")?;
        let group = self.token("VALID_VALUE_GROUP_ID")?;
        let text_msg = self.token("TEXT_MSG_ID")?;
        let rows: Vec<(u64, u64, u64)> = self
            .doc("assembly")?
            .marshal
            .root
            .rows(set)
            .map(|rows| rows.iter().filter_map(|r| Some((r.num(value_id)?, r.num(group).unwrap_or(0), r.num(text_msg).unwrap_or(0)))).collect())
            .unwrap_or_default();
        let texts: HashMap<u64, String> = if self.docs.iter().any(|(k, _)| k == "INT") {
            let set = self.message_set("INT")?;
            let index = self.message_index("INT")?.clone();
            let message = self.token("MESSAGE")?;
            let doc = self.doc("INT")?;
            let msgs = doc.marshal.root.rows(set).unwrap();
            rows.iter().filter_map(|(_, _, m)| index.get(m).map(|&i| (*m, msgs[i].str(message).unwrap_or("").to_string()))).collect()
        } else {
            HashMap::new()
        };
        let mut out = serde_json::Map::new();
        for (id, g, m) in rows {
            out.insert(id.to_string(), json!({ "group": g, "msgId": m, "text": texts.get(&m).cloned().unwrap_or_default() }));
        }
        Ok(J::Object(out))
    }

    pub fn kinds(&self) -> J {
        json!([Kind::Byte, Kind::Int, Kind::Word, Kind::Dword, Kind::Float, Kind::Double, Kind::Qword, Kind::NetId, Kind::DateTime, Kind::String, Kind::RowSet, Kind::Guid, Kind::Blob].iter().map(|k| k.name()).collect::<Vec<_>>())
    }
}
