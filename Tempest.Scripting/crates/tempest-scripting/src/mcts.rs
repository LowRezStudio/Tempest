//! `assembly.*` and `lang.*`: read-only queries over the game's MCTS
//! marshals, `ChaosGame/CookedPCConsole/assembly.dat` (items, devices, bots,
//! skins, resources... as `DATA_SET_*` row sets) and
//! `ChaosGame/Localization/<EXT>/Lang_<EXT>.dat` (the messages every
//! `*_MSG_ID` points at).
//!
//! ```lua
//! local item = assembly.find("ITEMS", "ITEM_ID", 7204)  -- "DATA_SET_" is optional
//! print(item.ITEM_ID, lang.message(item.NAME_MSG_ID))
//! for _, d in ipairs(assembly.find_all("DEVICES", { REF_BOT_ID = 2420 })) do ... end
//! ```
//!
//! The files are XOR'd (0x2A) `CMarshal` streams; the full format, and the
//! writer, live in `tempest-assets/src/mcts/mod.rs` (the Studio's side).
//! This is only the reader. Field names come from the token table of the
//! game's own `MctsInterface.dll` (older clients number their fields
//! differently), else from the 8.1 table in `data/mcts-fields.dat`.
//!
//! A file is read and checked on first use (8.1's assembly.dat: about 70 ms)
//! and stays loaded, shared by every mod, until `unload()`. It is kept as
//! its bytes plus the offset of every top-level row (about the file's size
//! in memory: the Win32 client has little to spare); a field is decoded
//! when it is read. A lookup by field builds a hash index of that set and
//! field once. Files are read from the game folder as installed, so mods
//! that changed them are included.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use mlua::{IntoLuaMulti, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};

const OBSCURE: u8 = 0x2A;

/// Field type (`MFIELD_TYPES`, the low byte of the token table's type word).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Other,
    Int,
    Float,
    Double,
    Qword,
}

impl Kind {
    fn from_u8(v: u8) -> Kind {
        match v {
            3 => Kind::Int,
            6 => Kind::Float,
            7 => Kind::Double,
            8 | 9 => Kind::Qword,
            _ => Kind::Other,
        }
    }
}

/// Field names and types by token.
#[derive(Default)]
struct Tokens {
    fields: Vec<Option<(Box<str>, Kind)>>,
    by_name: HashMap<Box<str>, u16>,
}

impl Tokens {
    fn insert(&mut self, token: u16, name: &[u8], ty: u16) {
        let name: Box<str> = String::from_utf8_lossy(name).into();
        let i = token as usize;
        if self.fields.len() <= i {
            self.fields.resize(i + 1, None);
        }
        self.by_name.insert(name.clone(), token);
        self.fields[i] = Some((name, Kind::from_u8(ty as u8)));
    }

    /// The 8.1 table, as `tempest marshal extract-tokens` writes it: `{u16 token BE, u16 type BE, name\0}`.
    fn builtin() -> Tokens {
        let d = include_bytes!("../data/mcts-fields.dat");
        let mut t = Tokens::default();
        let mut pos = 0;
        while pos + 4 < d.len() {
            let token = u16::from_be_bytes([d[pos], d[pos + 1]]);
            let ty = u16::from_be_bytes([d[pos + 2], d[pos + 3]]);
            let Some(end) = d[pos + 4..].iter().position(|&b| b == 0) else { break };
            t.insert(token, &d[pos + 4..pos + 4 + end], ty);
            pos += 4 + end + 1;
        }
        t
    }

    /// The table a client DLL embeds: a gzip blob of `{u16 wire token, u16
    /// type BE, name\0}` holding a `VERSION` entry; the position is the token.
    fn from_binary(bytes: &[u8]) -> Option<Tokens> {
        let mut at = 0;
        while let Some(i) = memchr::memmem::find(&bytes[at..], &[0x1f, 0x8b, 0x08]) {
            let start = at + i;
            at = start + 1;
            let mut blob = Vec::new();
            // The function table (with a `HELLO` entry) is the other blob, and holds names with `VERSION` in them too.
            let gz = flate2::read::GzDecoder::new(&bytes[start..]).read_to_end(&mut blob);
            if gz.is_err() || memchr::memmem::find(&blob, b"HELLO").is_some() || memchr::memmem::find(&blob, b"VERSION").is_none() {
                continue;
            }
            let mut t = Tokens::default();
            let (mut pos, mut token) = (0, 0u16);
            while pos + 4 < blob.len() {
                let ty = u16::from_be_bytes([blob[pos + 2], blob[pos + 3]]);
                let Some(end) = blob[pos + 4..].iter().position(|&b| b == 0) else { break };
                t.insert(token, &blob[pos + 4..pos + 4 + end], ty);
                token = token.wrapping_add(1);
                pos += 4 + end + 1;
            }
            if t.by_name.len() >= 100 {
                return Some(t);
            }
        }
        None
    }

    /// The running game's table, read once.
    fn game() -> Arc<Tokens> {
        static GAME: OnceLock<Arc<Tokens>> = OnceLock::new();
        GAME.get_or_init(|| {
            let found = crate::config::get().game_dir.as_ref().and_then(|g| {
                ["Binaries/Win64/MctsInterface.dll", "Binaries/Win32/MctsInterface.dll"]
                    .iter()
                    .filter_map(|sub| std::fs::read(g.join(sub)).ok())
                    .find_map(|b| Tokens::from_binary(&b))
            });
            Arc::new(found.unwrap_or_else(Tokens::builtin))
        })
        .clone()
    }

    fn name(&self, token: u16) -> Option<&str> {
        self.fields.get(token as usize)?.as_ref().map(|(n, _)| &**n)
    }

    fn kind(&self, token: u16) -> Kind {
        self.fields.get(token as usize).and_then(|f| f.as_ref()).map_or(Kind::Other, |(_, k)| *k)
    }

    fn token(&self, name: &str) -> Option<u16> {
        self.by_name.get(name).copied()
    }
}

// ----------------------------------------------------------------------------
// Reading
// ----------------------------------------------------------------------------

/// A field where it lies in the file; numbers keep their raw bits (a
/// `Float` is the `f32` pattern).
#[derive(Clone, Copy)]
enum Field<'a> {
    Num(u64),
    /// Encoding (1 UTF-8, 2/3 UTF-32, 4 UTF-16, 5 one byte per character) and the bytes.
    Str(u8, &'a [u8]),
    /// Offset of the row set's count.
    Rows(usize),
    Guid(&'a [u8]),
    Blob(&'a [u8]),
}

fn text(encoding: u8, b: &[u8]) -> String {
    match encoding {
        1 => String::from_utf8_lossy(b).into_owned(),
        2 | 3 => b.chunks(4).map(|c| char::from_u32(u32::from_le_bytes(c.try_into().unwrap())).unwrap_or('\u{FFFD}')).collect(),
        4 => char::decode_utf16(b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]]))).map(|c| c.unwrap_or('\u{FFFD}')).collect(),
        _ => b.iter().map(|&c| c as char).collect(),
    }
}

type Res<T> = Result<T, String>;

struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Res<&'a [u8]> {
        let s = self.d.get(self.pos..self.pos + n).ok_or_else(|| format!("truncated at {} (wanted {n} bytes)", self.pos))?;
        self.pos += n;
        Ok(s)
    }

    fn u8(&mut self) -> Res<u8> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Res<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Res<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Res<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    fn count(&mut self) -> Res<usize> {
        let c = self.u16()?;
        Ok(if c == 0xFFFF { self.u32()? as usize } else { c as usize })
    }

    /// Walk the row at `pos`, handing each field to `visit` until it returns
    /// true (then `true`, with `pos` mid-row); else `pos` ends past the row.
    fn row(&mut self, visit: &mut dyn FnMut(u16, Field<'a>) -> bool) -> Res<bool> {
        let count = self.count()?;
        let mut seen = 0;
        while seen < count {
            let at = self.pos;
            let h = self.u16()?;
            let param = (h & 0x3F) as usize;
            // Header `u16`: bits 15..12 the kind, bits 5..0 the parameter
            // (the table is in `tempest-assets`).
            let (token, field) = match h >> 12 {
                kind @ 1..=4 => {
                    if param > 32 {
                        return Err(format!("integer batch of {param} at {at}"));
                    }
                    let tokens = self.bytes(param * 2)?;
                    for t in tokens.chunks(2) {
                        let v = match kind {
                            1 => self.u8()? as u64,
                            2 => self.u16()? as u64,
                            3 => self.u32()? as u64,
                            _ => self.u64()?,
                        };
                        if visit(u16::from_le_bytes([t[0], t[1]]), Field::Num(v)) {
                            return Ok(true);
                        }
                    }
                    seen += param;
                    continue;
                }
                5 => {
                    let t = self.u16()?;
                    let len = self.u16()?;
                    let s = if len & 0x8000 != 0 { Field::Str(4, self.bytes((len & 0x7FFF) as usize * 2)?) } else { Field::Str(5, self.bytes(len as usize)?) };
                    (t, s)
                }
                6 => {
                    let t = self.u16()?;
                    let at = self.pos;
                    for _ in 0..self.count()? {
                        self.row(&mut |_, _| false)?;
                    }
                    (t, Field::Rows(at))
                }
                7 => (self.u16()?, Field::Guid(self.bytes(16)?)),
                8 => {
                    let t = self.u16()?;
                    let n = self.u16()? as usize;
                    (t, Field::Blob(self.bytes(n)?))
                }
                9 => (self.u16()?, Field::Str(4, self.bytes(param.min(3) * 2)?)),
                10 => {
                    let t = self.u16()?;
                    let n = self.u16()? as usize;
                    let unit = match param {
                        1 | 5 => 1,
                        2 | 3 => 4,
                        4 => 2,
                        _ => return Err(format!("string encoding {param} at {at}")),
                    };
                    (t, Field::Str(param as u8, self.bytes(n * unit)?))
                }
                kind => return Err(format!("entry kind {kind} at {at}")),
            };
            if visit(token, field) {
                return Ok(true);
            }
            seen += 1;
        }
        if seen != count {
            return Err(format!("row before {} declares {count} entries but holds {seen}", self.pos));
        }
        Ok(false)
    }

    /// Offsets of the rows of the row set at `pos`.
    fn rows(&mut self) -> Res<Vec<u32>> {
        let n = self.count()?;
        let mut out = Vec::with_capacity(n.min(1 << 20));
        for _ in 0..n {
            out.push(self.pos as u32);
            self.row(&mut |_, _| false)?;
        }
        Ok(out)
    }
}

/// Offset of the root row, after checking the whole marshal parses. Modern
/// header: `u8 flags, u32 function hash`; the legacy one (Paladins 0.x):
/// `u16 function index`. A modern file starts with a zero flags byte, which
/// a legacy index can too, so a failed walk retries the other way.
fn root(plain: &[u8]) -> Res<usize> {
    if plain.len() > u32::MAX as usize {
        return Err("larger than 4 GB".into());
    }
    let attempt = |legacy: bool| -> Res<usize> {
        let root = if legacy { 2 } else { 5 };
        let mut r = Reader { d: plain, pos: root };
        r.row(&mut |_, _| false)?;
        match plain.len() - r.pos {
            0 => Ok(root),
            n => Err(format!("{n} trailing bytes")),
        }
    };
    let modern = plain.first() == Some(&0);
    attempt(!modern).or_else(|e| attempt(modern).map_err(|_| e))
}

// ----------------------------------------------------------------------------
// Loaded files
// ----------------------------------------------------------------------------

/// A lookup key: what Lua compares, so `7204` finds an `INT` and `1.5` a `FLOAT`.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Int(i64),
    /// `f64` bits of a non-integral number.
    Float(u64),
    Str(Box<str>),
}

fn num_key(n: f64) -> Key {
    if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 { Key::Int(n as i64) } else { Key::Float(n.to_bits()) }
}

/// Row offsets by key, for one set and field.
type Index = HashMap<Key, Vec<u32>>;

struct Doc {
    path: PathBuf,
    tokens: Arc<Tokens>,
    /// The file, de-obscured.
    data: Box<[u8]>,
    root: u32,
    /// The top-level row sets in file order, with their row offsets.
    sets: Vec<(u16, Box<[u32]>)>,
    indexes: Mutex<HashMap<(u16, u16), Arc<Index>>>,
}

fn lua_err(e: String) -> mlua::Error {
    mlua::Error::external(e)
}

impl Doc {
    fn open(path: &Path, tokens: Arc<Tokens>) -> Res<Doc> {
        let fail = |e: String| format!("{}: {e}", path.display());
        let mut data = std::fs::read(path).map_err(|e| fail(e.to_string()))?;
        data.iter_mut().for_each(|b| *b ^= OBSCURE);
        let root = root(&data).map_err(fail)?;
        let mut tops = Vec::new();
        Reader { d: &data, pos: root }.row(&mut |t, f| {
            if let Field::Rows(at) = f {
                tops.push((t, at));
            }
            false
        })?;
        let sets = tops.into_iter().map(|(t, at)| Ok((t, Reader { d: &data, pos: at }.rows()?.into()))).collect::<Res<_>>().map_err(fail)?;
        Ok(Doc { path: path.to_path_buf(), tokens, data: data.into(), root: root as u32, sets, indexes: Mutex::default() })
    }

    fn token(&self, name: &str) -> mlua::Result<u16> {
        self.tokens.token(name).ok_or_else(|| mlua::Error::external(format!("unknown field {name:?}")))
    }

    /// Every field of the row at `row`, in file order.
    fn each<'a>(&'a self, row: u32, mut f: impl FnMut(u16, Field<'a>)) -> mlua::Result<()> {
        Reader { d: &self.data, pos: row as usize }
            .row(&mut |t, v| {
                f(t, v);
                false
            })
            .map(drop)
            .map_err(lua_err)
    }

    fn get(&self, row: u32, token: u16) -> mlua::Result<Option<Field<'_>>> {
        let mut found = None;
        Reader { d: &self.data, pos: row as usize }
            .row(&mut |t, v| {
                found = (t == token).then_some(v);
                found.is_some()
            })
            .map_err(lua_err)?;
        Ok(found)
    }

    fn rows_at(&self, at: usize) -> mlua::Result<Vec<u32>> {
        Reader { d: &self.data, pos: at }.rows().map_err(lua_err)
    }

    /// A top-level row set by name; `"ITEMS"` also finds `DATA_SET_ITEMS`.
    fn set(&self, name: &str) -> mlua::Result<(u16, &[u32])> {
        let found = [name.to_string(), format!("DATA_SET_{name}")]
            .into_iter()
            .find_map(|n| self.tokens.token(&n).and_then(|t| self.sets.iter().find(|(s, _)| *s == t)))
            .map(|(t, rows)| (*t, &**rows));
        found.ok_or_else(|| mlua::Error::external(format!("{} has no row set {name:?}", self.path.display())))
    }

    fn index(&self, set: u16, rows: &[u32], field: u16) -> mlua::Result<Arc<Index>> {
        if let Some(i) = self.indexes.lock().unwrap().get(&(set, field)) {
            return Ok(i.clone());
        }
        let mut index = Index::new();
        for &row in rows {
            if let Some(k) = self.get(row, field)?.and_then(|v| self.key(field, v)) {
                index.entry(k).or_default().push(row);
            }
        }
        let index = Arc::new(index);
        self.indexes.lock().unwrap().insert((set, field), index.clone());
        Ok(index)
    }

    fn key(&self, token: u16, v: Field) -> Option<Key> {
        match v {
            Field::Num(n) => Some(match self.tokens.kind(token) {
                Kind::Int => Key::Int(n as u32 as i32 as i64),
                Kind::Float => num_key(f32::from_bits(n as u32) as f64),
                Kind::Double => num_key(f64::from_bits(n)),
                _ if n <= i64::MAX as u64 => Key::Int(n as i64),
                _ => num_key(n as f64),
            }),
            Field::Str(enc, b) => Some(Key::Str(text(enc, b).into())),
            _ => None,
        }
    }

    /// Rows of `set` matching every `(field, key)`, through the index of the first.
    fn matching(&self, set: &str, filter: &[(u16, Key)], first_only: bool) -> mlua::Result<Vec<u32>> {
        let (set_token, rows) = self.set(set)?;
        let Some(((field, key), rest)) = filter.split_first() else {
            return Ok(rows.iter().take(if first_only { 1 } else { usize::MAX }).copied().collect());
        };
        let index = self.index(set_token, rows, *field)?;
        let mut out = Vec::new();
        for &row in index.get(key).map(|v| v.as_slice()).unwrap_or_default() {
            let mut ok = true;
            for (f, k) in rest {
                ok = ok && self.get(row, *f)?.and_then(|v| self.key(*f, v)).as_ref() == Some(k);
            }
            if ok {
                out.push(row);
                if first_only {
                    break;
                }
            }
        }
        Ok(out)
    }
}

/// Every loaded file by path.
static DOCS: Mutex<Vec<Arc<Doc>>> = Mutex::new(Vec::new());
/// Serializes loading, so two threads asking at once read a file once.
static LOADING: Mutex<()> = Mutex::new(());

fn load(path: PathBuf) -> mlua::Result<Arc<Doc>> {
    let cached = || DOCS.lock().unwrap().iter().find(|d| d.path == path).cloned();
    if let Some(d) = cached() {
        return Ok(d);
    }
    let _loading = LOADING.lock().unwrap();
    if let Some(d) = cached() {
        return Ok(d);
    }
    let t = std::time::Instant::now();
    let doc = Arc::new(Doc::open(&path, Tokens::game()).map_err(lua_err)?);
    crate::log::line("mcts", &format!("loaded {} in {} ms", path.display(), t.elapsed().as_millis()));
    DOCS.lock().unwrap().push(doc.clone());
    Ok(doc)
}

fn unload(path: &Path) -> bool {
    let mut docs = DOCS.lock().unwrap();
    let before = docs.len();
    docs.retain(|d| d.path != path);
    docs.len() != before
}

fn game_dir() -> mlua::Result<PathBuf> {
    crate::config::get().game_dir.clone().ok_or_else(|| mlua::Error::external("the game folder is unknown (set TEMPEST_GAME_DIR)"))
}

fn assembly_path() -> mlua::Result<PathBuf> {
    Ok(game_dir()?.join("ChaosGame/CookedPCConsole/assembly.dat"))
}

fn lang_path(ext: &str) -> mlua::Result<PathBuf> {
    let ext = ext.to_ascii_uppercase();
    if ext.is_empty() || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(mlua::Error::external(format!("bad language {ext:?}")));
    }
    Ok(game_dir()?.join(format!("ChaosGame/Localization/{ext}/Lang_{ext}.dat")))
}

/// The game's language: `-language=XXX` on its command line, else `INT`.
fn game_language() -> String {
    std::env::args()
        .find_map(|a| {
            let a = a.trim_start_matches(['-', '/']);
            let (k, v) = a.split_once('=')?;
            k.eq_ignore_ascii_case("language").then(|| v.to_ascii_uppercase())
        })
        .unwrap_or_else(|| "INT".into())
}

// ----------------------------------------------------------------------------
// Lua
// ----------------------------------------------------------------------------

/// One row of a loaded file. Fields read by name (`row.ITEM_ID`): a
/// misspelled name raises, a field the row does not carry is `nil`.
#[derive(Clone)]
struct RowRef {
    doc: Arc<Doc>,
    row: u32,
}

fn rows_value(lua: &Lua, doc: &Arc<Doc>, rows: impl IntoIterator<Item = u32>) -> mlua::Result<Table> {
    lua.create_sequence_from(rows.into_iter().map(|row| RowRef { doc: doc.clone(), row }))
}

fn field_value(lua: &Lua, doc: &Arc<Doc>, token: u16, v: Field) -> mlua::Result<Value> {
    Ok(match v {
        Field::Num(n) => match doc.tokens.kind(token) {
            Kind::Int => Value::Integer(n as u32 as i32 as mlua::Integer),
            Kind::Float => Value::Number(f32::from_bits(n as u32) as f64),
            Kind::Double => Value::Number(f64::from_bits(n)),
            _ if n <= i64::MAX as u64 => crate::lua_api::int(n as i64),
            _ => Value::Number(n as f64),
        },
        Field::Str(enc, b) => Value::String(lua.create_string(text(enc, b))?),
        Field::Rows(at) => Value::Table(rows_value(lua, doc, doc.rows_at(at)?)?),
        Field::Guid(g) => {
            let d1 = u32::from_le_bytes(g[0..4].try_into().unwrap());
            let d2 = u16::from_le_bytes([g[4], g[5]]);
            let d3 = u16::from_le_bytes([g[6], g[7]]);
            let tail: String = g[8..].iter().map(|b| format!("{b:02x}")).collect();
            Value::String(lua.create_string(format!("{d1:08x}-{d2:04x}-{d3:04x}-{}-{}", &tail[..4], &tail[4..]))?)
        }
        Field::Blob(b) => Value::String(lua.create_string(b)?),
    })
}

/// A row as a plain table, row sets included (as arrays of plain tables).
fn plain_row(lua: &Lua, doc: &Arc<Doc>, row: u32) -> mlua::Result<Table> {
    let mut fields = Vec::new();
    doc.each(row, |t, v| fields.push((t, v)))?;
    let t = lua.create_table_with_capacity(0, fields.len())?;
    for (token, v) in fields {
        let name = doc.tokens.name(token).map_or_else(|| format!("#{token}"), str::to_string);
        let value = match v {
            Field::Rows(at) => Value::Table(lua.create_sequence_from(doc.rows_at(at)?.into_iter().map(|r| plain_row(lua, doc, r)).collect::<mlua::Result<Vec<_>>>()?)?),
            v => field_value(lua, doc, token, v)?,
        };
        t.raw_set(name, value)?;
    }
    Ok(t)
}

impl UserData for RowRef {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, name: String| {
            let token = this.doc.token(&name)?;
            match this.doc.get(this.row, token)? {
                Some(v) => field_value(lua, &this.doc, token, v),
                None => Ok(Value::Nil),
            }
        });
        m.add_method("has", |_, this, name: String| Ok(this.doc.get(this.row, this.doc.token(&name)?)?.is_some()));
        m.add_method("fields", |lua, this, ()| plain_row(lua, &this.doc, this.row));
        m.add_meta_method(MetaMethod::Eq, |_, a, b: mlua::UserDataRef<RowRef>| Ok(Arc::ptr_eq(&a.doc, &b.doc) && a.row == b.row));
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            // The id fields come first in most sets (`ITEM_ID`, `MSG_ID`...): enough to tell rows apart.
            let mut ids = Vec::new();
            this.doc.each(this.row, |t, v| {
                if let (Some(name), Some(Key::Int(i))) = (this.doc.tokens.name(t), this.doc.key(t, v)) {
                    if name.ends_with("_ID") && ids.len() < 3 {
                        ids.push(format!("{name}={i}"));
                    }
                }
            })?;
            Ok(format!("Row({})", ids.join(", ")))
        });
    }
}

/// The `(field, key)` pairs of `find(set, field, value)` / `find(set, { FIELD = value, ... })`.
fn filter(doc: &Doc, field: Value, value: Value) -> mlua::Result<Vec<(u16, Key)>> {
    let key = |v: &Value| -> mlua::Result<Key> {
        match v {
            Value::Integer(i) => Ok(Key::Int(crate::lua_api::to_i64(*i))),
            Value::Number(n) => Ok(num_key(*n)),
            Value::Boolean(b) => Ok(Key::Int(*b as i64)),
            Value::String(s) => Ok(Key::Str(s.to_str()?.as_ref().into())),
            other => Err(mlua::Error::external(format!("cannot look a field up by a {}", other.type_name()))),
        }
    };
    match field {
        Value::String(f) => Ok(vec![(doc.token(&f.to_str()?)?, key(&value)?)]),
        Value::Table(t) => {
            let mut out = Vec::new();
            for pair in t.pairs::<String, Value>() {
                let (f, v) = pair?;
                out.push((doc.token(&f)?, key(&v)?));
            }
            // Sorted, so the same filter always goes through the same index.
            out.sort_by_key(|(f, _)| *f);
            Ok(out)
        }
        Value::Nil => Ok(Vec::new()),
        other => Err(mlua::Error::external(format!("expected a field name or a table of fields, got a {}", other.type_name()))),
    }
}

/// `find`, `find_all` and `rows` of one row set, shared by both modules.
fn query(lua: &Lua, doc: &Arc<Doc>, set: &str, field: Value, value: Value, first_only: bool) -> mlua::Result<Value> {
    let filter = filter(doc, field, value)?;
    let rows = doc.matching(set, &filter, first_only)?;
    if first_only {
        return Ok(match rows.first() {
            Some(&row) => Value::UserData(lua.create_userdata(RowRef { doc: doc.clone(), row })?),
            None => Value::Nil,
        });
    }
    Ok(Value::Table(rows_value(lua, doc, rows)?))
}

fn sets(lua: &Lua, doc: &Arc<Doc>) -> mlua::Result<Table> {
    let out = lua.create_table()?;
    for (t, rows) in &doc.sets {
        let e = lua.create_table()?;
        e.set("name", doc.tokens.name(*t).map_or_else(|| format!("#{t}"), str::to_string))?;
        e.set("rows", rows.len())?;
        out.push(e)?;
    }
    Ok(out)
}

/// A top-level field that is not a row set (`DB_VERSION`, `GAME_VERSION_RAW`, `EXT_NAME`...).
fn top(lua: &Lua, doc: &Arc<Doc>, name: &str) -> mlua::Result<Value> {
    let token = doc.token(name)?;
    match doc.get(doc.root, token)? {
        Some(Field::Rows(_)) => Err(mlua::Error::external(format!("{name} is a row set; use rows()"))),
        Some(v) => field_value(lua, doc, token, v),
        None => Ok(Value::Nil),
    }
}

/// `load()` as `true` or `nil, message`, for a mod that wants to check first.
fn try_load(lua: &Lua, path: mlua::Result<PathBuf>) -> mlua::Result<mlua::MultiValue> {
    match path.and_then(load) {
        Ok(_) => (true,).into_lua_multi(lua),
        Err(e) => (Value::Nil, e.to_string()).into_lua_multi(lua),
    }
}

pub fn register(lua: &Lua) -> mlua::Result<()> {
    let asm = lua.create_table()?;
    let doc = || assembly_path().and_then(load);
    asm.set("path", lua.create_function(|_, ()| Ok(assembly_path()?.display().to_string()))?)?;
    asm.set("load", lua.create_function(|lua, ()| try_load(lua, assembly_path()))?)?;
    asm.set("unload", lua.create_function(|_, ()| Ok(unload(&assembly_path()?)))?)?;
    asm.set("sets", lua.create_function(move |lua, ()| sets(lua, &doc()?))?)?;
    asm.set("rows", lua.create_function(move |lua, set: String| query(lua, &doc()?, &set, Value::Nil, Value::Nil, false))?)?;
    asm.set("find", lua.create_function(move |lua, (set, field, value): (String, Value, Value)| query(lua, &doc()?, &set, field, value, true))?)?;
    asm.set("find_all", lua.create_function(move |lua, (set, field, value): (String, Value, Value)| query(lua, &doc()?, &set, field, value, false))?)?;
    asm.set("get", lua.create_function(move |lua, name: String| top(lua, &doc()?, &name))?)?;
    // GAME_VERSION_RAW: four u16, most significant first (8.1.5839.0).
    asm.set(
        "version",
        lua.create_function(move |_, ()| {
            let d = doc()?;
            let Some(t) = d.tokens.token("GAME_VERSION_RAW") else { return Ok(None) };
            Ok(match d.get(d.root, t)? {
                Some(Field::Num(v)) => Some(format!("{}.{}.{}.{}", v >> 48, (v >> 32) & 0xFFFF, (v >> 16) & 0xFFFF, v & 0xFFFF)),
                _ => None,
            })
        })?,
    )?;
    lua.globals().set("assembly", asm)?;

    // Every lang.* function takes the language last, optional: the game's by default.
    let lang = lua.create_table()?;
    let ldoc = |ext: Option<String>| lang_path(&ext.unwrap_or_else(game_language)).and_then(load);
    lang.set("language", lua.create_function(|_, ()| Ok(game_language()))?)?;
    lang.set("path", lua.create_function(|_, ext: Option<String>| Ok(lang_path(&ext.unwrap_or_else(game_language))?.display().to_string()))?)?;
    lang.set("load", lua.create_function(|lua, ext: Option<String>| try_load(lua, lang_path(&ext.unwrap_or_else(game_language))))?)?;
    lang.set("unload", lua.create_function(|_, ext: Option<String>| Ok(unload(&lang_path(&ext.unwrap_or_else(game_language))?)))?)?;
    lang.set(
        "languages",
        lua.create_function(|_, ()| {
            let dir = game_dir()?.join("ChaosGame/Localization");
            let mut out: Vec<String> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|ext| lang_path(ext).is_ok_and(|p| p.is_file()))
                .collect();
            out.sort();
            Ok(out)
        })?,
    )?;
    lang.set(
        "message",
        lua.create_function(move |lua, (id, ext): (Value, Option<String>)| {
            let d = ldoc(ext)?;
            let filter = filter(&d, Value::String(lua.create_string("MSG_ID")?), id)?;
            let Some(&row) = d.matching("DATA_SET", &filter, true)?.first() else { return Ok(Value::Nil) };
            let message = d.token("MESSAGE")?;
            d.get(row, message)?.map_or(Ok(Value::Nil), |v| field_value(lua, &d, message, v))
        })?,
    )?;
    lang.set("rows", lua.create_function(move |lua, ext: Option<String>| query(lua, &ldoc(ext)?, "DATA_SET", Value::Nil, Value::Nil, false))?)?;
    lang.set("find", lua.create_function(move |lua, (field, value, ext): (Value, Value, Option<String>)| query(lua, &ldoc(ext)?, "DATA_SET", field, value, true))?)?;
    lang.set("find_all", lua.create_function(move |lua, (field, value, ext): (Value, Value, Option<String>)| query(lua, &ldoc(ext)?, "DATA_SET", field, value, false))?)?;
    lang.set("get", lua.create_function(move |lua, (name, ext): (String, Option<String>)| top(lua, &ldoc(ext)?, &name))?)?;
    lua.globals().set("lang", lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_table() {
        let t = Tokens::builtin();
        assert!(t.by_name.len() > 3000);
        let id = t.token("ITEM_ID").unwrap();
        assert_eq!(t.name(id), Some("ITEM_ID"));
        assert_eq!(t.kind(t.token("WEIGHT").unwrap()), Kind::Float);
    }

    /// A hand-built marshal: one set of two rows, ints batched, a string.
    #[test]
    fn parses_rows() {
        let t = Tokens::builtin();
        let (set, id, name) = (t.token("DATA_SET_ITEMS").unwrap(), t.token("ITEM_ID").unwrap(), t.token("NAME").unwrap());
        let mut b = vec![0u8, 0, 0, 0, 0];
        b.extend(1u16.to_le_bytes()); // root: one entry
        b.extend((0x6001u16).to_le_bytes());
        b.extend(set.to_le_bytes());
        b.extend(2u16.to_le_bytes()); // two rows
        for (i, s) in [(7u16, "a"), (300, "bé")] {
            b.extend(2u16.to_le_bytes());
            b.extend((0x2001u16).to_le_bytes());
            b.extend(id.to_le_bytes());
            b.extend(i.to_le_bytes());
            b.extend((0xA004u16).to_le_bytes());
            b.extend(name.to_le_bytes());
            let units: Vec<u16> = s.encode_utf16().collect();
            b.extend((units.len() as u16).to_le_bytes());
            units.iter().for_each(|u| b.extend(u.to_le_bytes()));
        }
        let path = std::env::temp_dir().join(format!("tempest-mcts-test-{}.dat", std::process::id()));
        std::fs::write(&path, b.iter().map(|x| x ^ OBSCURE).collect::<Vec<_>>()).unwrap();
        let doc = Doc::open(&path, Arc::new(t)).unwrap();
        let _ = std::fs::remove_file(&path);
        let (_, rows) = doc.set("ITEMS").unwrap();
        assert_eq!(rows.len(), 2);
        assert!(matches!(doc.get(rows[1], id).unwrap(), Some(Field::Num(300))));
        assert!(matches!(doc.get(rows[1], name).unwrap(), Some(Field::Str(e, s)) if text(e, s) == "bé"));
        let hit = doc.matching("DATA_SET_ITEMS", &[(name, Key::Str("bé".into()))], true).unwrap();
        assert_eq!(hit, vec![rows[1]]);
    }
}
