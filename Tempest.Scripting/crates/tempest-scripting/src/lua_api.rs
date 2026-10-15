//! The Lua surface: `tempest.*`, `memory.*`, and the `Object` proxy.
//!
//! ```lua
//! local st = tempest.status()                 -- what resolved, and notes if not
//! tempest.on("PostLogin", function(game, p)   -- after the original ran
//!     print(game:full_name(), p.NewPlayer:full_name())
//!     p.NewPlayer:ClientMessage("welcome")    -- call any UFunction
//! end)
//! tempest.before("TakeDamage", function(pawn, p)
//!     p.Damage = 0                            -- edit params in place
//!     return false                            -- or cancel the call entirely
//! end)
//! ```
//!
//! Object properties go through live `UProperty` reflection, so they track
//! patches; unknown names read as `nil` (logged once) and raise on write.

use mlua::{AnyUserData, Function, IntoLua, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};

use crate::memory;
use crate::unreal::{self, Event, Phase, Prop, PropKind};

fn registry_key(id: u64) -> String {
    format!("tempest.sub.{id}")
}

pub fn register(lua: &Lua) -> mlua::Result<()> {
    unreal::set_dispatcher(deliver);

    // `print` → log, so script output lands in the file and the console.
    lua.globals().set(
        "print",
        lua.create_function(|_, args: mlua::MultiValue| {
            let text: Vec<String> = args
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.to_string_lossy().to_string(),
                    other => format!("{other:?}"),
                })
                .collect();
            crate::log::line("lua", &text.join("\t"));
            Ok(())
        })?,
    )?;

    let t = lua.create_table()?;
    t.set("version", env!("CARGO_PKG_VERSION"))?;
    t.set(
        "log",
        lua.create_function(|_, msg: String| {
            crate::log::line("lua", &msg);
            Ok(())
        })?,
    )?;
    t.set("status", lua.create_function(|lua, ()| status(lua))?)?;
    t.set(
        "on",
        lua.create_function(|lua, (spec, cb): (String, Function)| {
            let id = unreal::subscribe(&spec, Phase::After, &crate::loader::current_mod());
            lua.set_named_registry_value(&registry_key(id), cb)?;
            Ok(id)
        })?,
    )?;
    t.set(
        "before",
        lua.create_function(|lua, (spec, cb): (String, Function)| {
            let id = unreal::subscribe(&spec, Phase::Before, &crate::loader::current_mod());
            lua.set_named_registry_value(&registry_key(id), cb)?;
            Ok(id)
        })?,
    )?;
    t.set(
        "queue",
        lua.create_function(|lua, cb: Function| {
            let key = lua.create_registry_value(cb)?;
            unreal::queue(Box::new(move || {
                let lua = crate::lua();
                if let Ok(f) = lua.registry_value::<Function>(&key) {
                    if let Err(e) = f.call::<()>(()) {
                        crate::log::line("lua", &format!("queued job failed: {e}"));
                    }
                }
                let _ = lua.remove_registry_value(key);
            }));
            Ok(())
        })?,
    )?;
    t.set(
        "on_game_thread",
        lua.create_function(|_, ()| Ok(unreal::on_game_thread()))?,
    )?;
    t.set(
        "off",
        lua.create_function(|lua, id: u64| {
            unreal::unsubscribe(id);
            lua.unset_named_registry_value(&registry_key(id))
        })?,
    )?;
    t.set(
        "reload",
        lua.create_function(|lua, only: Option<String>| {
            crate::loader::reload(lua, only.as_deref());
            Ok(())
        })?,
    )?;
    // tempest.mod() -> id of the mod whose script/handler is running
    t.set("mod", lua.create_function(|_, ()| Ok(crate::loader::current_mod()))?)?;
    // Backs tempest.on_key (prelude): hook the game window once.
    t.set(
        "_install_keyhook",
        lua.create_function(|_, ()| {
            crate::input::install().map_err(mlua::Error::external)
        })?,
    )?;
    // tempest.dump_sdk(dir?) -> classes, files
    t.set(
        "dump_sdk",
        lua.create_function(|_, dir: Option<String>| {
            let dir = match dir {
                Some(d) => std::path::PathBuf::from(d),
                None => {
                    let base = crate::loader::dll_dir()
                        .and_then(|d| d.parent().map(|p| p.to_path_buf()))
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    base.join("types").join("sdk")
                }
            };
            let (classes, files) = unreal::dump_sdk(&dir).map_err(mlua::Error::external)?;
            crate::log::line("sdk", &format!("{classes} classes/structs in {files} files -> {}", dir.display()));
            Ok((classes, files))
        })?,
    )?;
    t.set(
        "mods",
        lua.create_function(|_, ()| Ok(crate::loader::mods().into_iter().map(|m| m.id).collect::<Vec<_>>()))?,
    )?;
    // Cross-mod values: `shared` is a plain global table every mod sees;
    // export/import add a log trail and a clear "not provided" message.
    t.set(
        "export",
        lua.create_function(|lua, (name, value): (String, Value)| {
            let shared: Table = lua.globals().get("shared")?;
            if !shared.get::<Value>(name.as_str())?.is_nil() {
                crate::log::line("lua", &format!("[{}] export `{name}` replaces an earlier value", crate::loader::current_mod()));
            }
            shared.set(name.as_str(), value)?;
            let providers: Table = match shared.get::<Value>("__providers")? {
                Value::Table(t) => t,
                _ => {
                    let t = lua.create_table()?;
                    shared.set("__providers", t.clone())?;
                    t
                }
            };
            providers.set(name.as_str(), crate::loader::current_mod())?;
            Ok(())
        })?,
    )?;
    t.set(
        "import",
        lua.create_function(|lua, name: String| {
            let shared: Table = lua.globals().get("shared")?;
            let v: Value = shared.get(name.as_str())?;
            if v.is_nil() {
                crate::log::line(
                    "lua",
                    &format!("[{}] import `{name}`: nothing exported under that name yet (load order: mods run alphabetically by id)", crate::loader::current_mod()),
                );
            }
            Ok(v)
        })?,
    )?;
    t.set(
        "objects",
        lua.create_function(|lua, limit: Option<usize>| {
            let out = lua.create_table()?;
            for (i, ptr) in unreal::objects().take(limit.unwrap_or(64)).enumerate() {
                out.set(i + 1, Object(ptr))?;
            }
            Ok(out)
        })?,
    )?;
    t.set(
        "find_object",
        lua.create_function(|_, full: String| Ok(unreal::find_object(&full).map(Object)))?,
    )?;
    t.set(
        "find_class",
        lua.create_function(|_, path: String| Ok(unreal::find_class(&path).map(Object)))?,
    )?;
    t.set(
        "engine",
        lua.create_function(|_, ()| Ok(unreal::engine().map(Object)))?,
    )?;
    // tempest.new("Engine.Console" | classObj, outer, name?) -> Object or nil
    t.set(
        "new",
        lua.create_function(|_, (class, outer, name): (Value, Option<AnyUserData>, Option<String>)| {
            let class = match &class {
                Value::String(s) => unreal::find_class(&s.to_string_lossy()),
                Value::UserData(ud) => ud.borrow::<Object>().ok().map(|o| o.0),
                _ => None,
            };
            let Some(class) = class else {
                crate::log::line("lua", "tempest.new: unknown class");
                return Ok(None);
            };
            let outer = outer.and_then(|ud| ud.borrow::<Object>().ok().map(|o| o.0)).unwrap_or(0);
            match unreal::construct_object(class, outer, name.as_deref()) {
                Ok(p) => Ok(Some(Object(p))),
                Err(e) => {
                    crate::log::line("lua", &format!("tempest.new failed: {e}"));
                    Ok(None)
                }
            }
        })?,
    )?;
    t.set(
        "local_player",
        lua.create_function(|_, ()| Ok(local_player_controller().map(Object)))?,
    )?;
    lua.globals().set("tempest", t)?;
    lua.globals().set("memory", memory_table(lua)?)?;
    run_prelude(lua)
}

/// Built-in Lua layer (`tempest.command` & co). Re-run on every reload
/// because reload drops the subscriptions it depends on.
pub fn run_prelude(lua: &Lua) -> mlua::Result<()> {
    lua.load(include_str!("prelude.lua")).set_name("=prelude").exec()
}

fn status(lua: &Lua) -> mlua::Result<Table> {
    let g = unreal::globals();
    let addr = |v: usize| if v == 0 { Value::Nil } else { Value::Integer(v as i64) };
    let t = lua.create_table()?;
    t.set("gobjects", addr(g.gobjects))?;
    t.set("gnames", addr(g.gnames))?;
    t.set("engine", addr(g.engine_slot))?;
    t.set("process_event", addr(g.process_event))?;
    t.set("xor_mask", g.xor_mask)?;
    t.set("gnames_ok", g.gnames_ok)?;
    t.set("gobjects_ok", g.gobjects_ok)?;
    t.set("hook_installed", g.hook_installed)?;
    t.set("runtime_uc_flags", addr(g.runtime_uc_flags))?;
    t.set("gpropaddr", addr(g.gpropaddr))?;
    t.set("process_internal", addr(g.process_internal))?;
    t.set("script_hook_installed", g.script_hook_installed)?;
    t.set("gmalloc", addr(g.gmalloc))?;
    t.set("gnatives", addr(g.gnatives))?;
    t.set("native_hooks", unreal::native_hooks())?;
    t.set("mods", crate::loader::mods().into_iter().map(|m| m.id).collect::<Vec<_>>())?;
    t.set("key_hook_installed", crate::input::installed())?;
    t.set("game_thread", unreal::game_thread_id())?;
    t.set("object_count", unreal::object_count())?;
    t.set("name_count", unreal::name_count())?;
    t.set("notes", g.notes)?;
    let subs = lua.create_table()?;
    for (id, spec) in unreal::subscriptions() {
        subs.set(id, spec)?;
    }
    t.set("subscriptions", subs)?;
    Ok(t)
}

/// `Engine.GamePlayers[0].Actor`, via reflection.
fn local_player_controller() -> Option<usize> {
    let engine = unreal::engine()?;
    let players = unreal::find_property(unreal::object_class(engine)?, "GamePlayers")?;
    let data: usize = unsafe { memory::read(engine + players.offset)? };
    let count: i32 = unsafe { memory::read(engine + players.offset + unreal::layout::TARRAY_COUNT)? };
    if data == 0 || count <= 0 {
        return None;
    }
    let player: usize = unsafe { memory::read(data)? };
    let actor = unreal::find_property(unreal::object_class(player)?, "Actor")?;
    unsafe { memory::read::<usize>(player + actor.offset) }.filter(|&p| p != 0)
}

// ---------------------------------------------------------------------------
// Event delivery (game thread, after the original ProcessEvent)
// ---------------------------------------------------------------------------

fn deliver(ev: Event) -> bool {
    let lua = crate::lua();
    let Ok(cb) = lua.named_registry_value::<Function>(&registry_key(ev.sub_id)) else {
        return true; // unsubscribed between match and delivery
    };
    let this = match lua.create_userdata(Object(ev.object)) {
        Ok(ud) => Value::UserData(ud),
        Err(_) => return true,
    };
    let params = match lua.create_userdata(Params {
        parms: ev.parms,
        func: ev.function,
    }) {
        Ok(ud) => Value::UserData(ud),
        Err(_) => return true,
    };
    let owner = unreal::subscription_owner(ev.sub_id).unwrap_or_default();
    let result = crate::loader::with_mod(&owner, || cb.call::<Value>((this, params)));
    match result {
        // Only an explicit `false` from a before-handler cancels.
        Ok(Value::Boolean(false)) => ev.phase == Phase::After,
        Ok(_) => true,
        Err(e) => {
            let name = unreal::object_name(ev.function).unwrap_or_default();
            let tag = if owner.is_empty() { String::new() } else { format!("[{owner}] ") };
            crate::log::line("lua", &format!("{tag}{:?} handler for {name} failed: {e}", ev.phase));
            true
        }
    }
}

fn to_lua(lua: &Lua, v: &unreal::Value) -> Value {
    use unreal::Value as V;
    match v {
        V::Nil => Value::Nil,
        V::Int(i) => Value::Integer(*i),
        V::Float(f) => Value::Number(*f),
        V::Bool(b) => Value::Boolean(*b),
        V::Str(s) => lua.create_string(s).map(Value::String).unwrap_or(Value::Nil),
        V::Object(p) => lua.create_userdata(Object(*p)).map(Value::UserData).unwrap_or(Value::Nil),
        V::Vector(x, y, z) => triple(lua, ["x", "y", "z"], [*x as f64, *y as f64, *z as f64]),
        V::Rotator(p, y, r) => triple(lua, ["pitch", "yaw", "roll"], [*p as f64, *y as f64, *r as f64]),
        V::Struct { addr, kind } => lua
            .create_userdata(Struct { addr: *addr, kind: *kind })
            .map(Value::UserData)
            .unwrap_or(Value::Nil),
        V::Array { addr, inner } => lua
            .create_userdata(Array { addr: *addr, inner: *inner })
            .map(Value::UserData)
            .unwrap_or(Value::Nil),
        V::Map { addr, key, value, elem_size } => lua
            .create_userdata(Map { addr: *addr, key: *key, value: *value, elem_size: *elem_size })
            .map(Value::UserData)
            .unwrap_or(Value::Nil),
        V::Addr(a) => Value::Integer(*a as i64),
    }
}

fn triple(lua: &Lua, keys: [&str; 3], vals: [f64; 3]) -> Value {
    let Ok(t) = lua.create_table() else { return Value::Nil };
    for (k, v) in keys.iter().zip(vals) {
        let _ = t.set(*k, v);
    }
    Value::Table(t)
}

/// Lua → property value, guided by the reflected kind.
fn from_lua(kind: PropKind, v: &Value) -> Option<unreal::Value> {
    use unreal::Value as V;
    Some(match (kind, v) {
        (_, Value::Nil) => V::Nil,
        (_, Value::Boolean(b)) => V::Bool(*b),
        (_, Value::Integer(i)) => V::Int(*i),
        (_, Value::Number(n)) => V::Float(*n),
        (_, Value::String(s)) => V::Str(s.to_string_lossy().to_string()),
        (PropKind::Object, Value::UserData(ud)) => V::Object(ud.borrow::<Object>().ok()?.0),
        (PropKind::Struct, Value::UserData(ud)) => {
            let s = ud.borrow::<Struct>().ok()?;
            V::Struct { addr: s.addr, kind: s.kind }
        }
        (PropKind::Struct, Value::Table(t)) => {
            if let (Ok(x), Ok(y), Ok(z)) = (t.get::<f32>("x"), t.get::<f32>("y"), t.get::<f32>("z")) {
                V::Vector(x, y, z)
            } else {
                V::Rotator(t.get("pitch").ok()?, t.get("yaw").ok()?, t.get("roll").ok()?)
            }
        }
        _ => return None,
    })
}

/// Write a Lua value into a reflected property at `base`. Handles the
/// table → struct case (field by field) on top of `from_lua`/`write_prop`.
fn write_field(base: usize, prop: &Prop, name: &str, value: &Value) -> mlua::Result<()> {
    if prop.kind == PropKind::Struct {
        if let Value::Table(t) = value {
            let kind_name = unreal::object_name(prop.ext).unwrap_or_default();
            if kind_name != "Vector" && kind_name != "Rotator" {
                let addr = base + prop.offset;
                for pair in t.clone().pairs::<String, Value>() {
                    let (k, v) = pair?;
                    let fp = unreal::find_property(prop.ext, &k).ok_or_else(|| {
                        mlua::Error::external(format!("struct {kind_name} has no field `{k}`"))
                    })?;
                    write_field(addr, &fp, &k, &v)?;
                }
                return Ok(());
            }
        }
    }
    let v = from_lua(prop.kind, value).ok_or_else(|| {
        mlua::Error::external(format!("cannot assign {} to `{name}` ({:?})", value.type_name(), prop.kind))
    })?;
    unreal::write_prop(base, prop, &v).map_err(mlua::Error::external)
}

// ---------------------------------------------------------------------------
// Struct / Array / Map proxies (live views into game memory)
// ---------------------------------------------------------------------------

/// A struct other than Vector/Rotator: fields read and write in place.
#[derive(Debug, Clone, Copy)]
pub struct Struct {
    addr: usize,
    kind: usize,
}

impl UserData for Struct {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("addr", |_, this, ()| Ok(this.addr as i64));
        m.add_method("size", |_, this, ()| Ok(unreal::struct_size(this.kind)));
        m.add_method("fields", |_, this, ()| Ok(unreal::field_names(this.kind)));
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Struct({} @0x{:x})", unreal::object_name(this.kind).unwrap_or_default(), this.addr))
        });
        m.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            let Value::String(key) = key else { return Ok(Value::Nil) };
            let key = key.to_string_lossy();
            Ok(match unreal::find_property(this.kind, &key) {
                Some(p) => unreal::read_prop(this.addr, &p).map(|v| to_lua(lua, &v)).unwrap_or(Value::Nil),
                None => Value::Nil,
            })
        });
        m.add_meta_method(MetaMethod::NewIndex, |_, this, (key, value): (String, Value)| {
            let p = unreal::find_property(this.kind, &key).ok_or_else(|| {
                mlua::Error::external(format!("struct {} has no field `{key}`", unreal::object_name(this.kind).unwrap_or_default()))
            })?;
            write_field(this.addr, &p, &key, &value)
        });
    }
}

/// A `TArray`: 1-based indexing, `#`, `ipairs`, `add`, `remove`, `clear`.
#[derive(Debug, Clone, Copy)]
pub struct Array {
    addr: usize,
    inner: Prop,
}

impl Array {
    fn elem(&self, i: i64) -> Option<usize> {
        if i < 1 {
            return None;
        }
        unreal::array_elem(self.addr, &self.inner, (i - 1) as usize)
    }
}

impl UserData for Array {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("addr", |_, this, ()| Ok(this.addr as i64));
        m.add_method("kind", |_, this, ()| Ok(format!("{:?}", this.inner.kind)));
        m.add_meta_method(MetaMethod::Len, |_, this, ()| Ok(unreal::array_len(this.addr)));
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Array<{:?}>[{}] @0x{:x}", this.inner.kind, unreal::array_len(this.addr), this.addr))
        });
        m.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            let Value::Integer(i) = key else { return Ok(Value::Nil) };
            Ok(match this.elem(i) {
                Some(e) => unreal::read_prop(e, &this.inner).map(|v| to_lua(lua, &v)).unwrap_or(Value::Nil),
                None => Value::Nil,
            })
        });
        m.add_meta_method(MetaMethod::NewIndex, |_, this, (i, value): (i64, Value)| {
            let e = this.elem(i).ok_or_else(|| mlua::Error::external(format!("index {i} out of range")))?;
            write_field(e, &this.inner, &format!("[{i}]"), &value)
        });
        // arr:add(v?) -> new length; a zeroed element when v is nil.
        m.add_method("add", |_, this, value: Value| {
            let e = unreal::array_push(this.addr, &this.inner).map_err(mlua::Error::external)?;
            if !value.is_nil() {
                write_field(e, &this.inner, "add", &value)?;
            }
            Ok(unreal::array_len(this.addr))
        });
        m.add_method("remove", |_, this, i: i64| {
            if i < 1 {
                return Err(mlua::Error::external("index out of range"));
            }
            unreal::array_remove(this.addr, &this.inner, (i - 1) as usize).map_err(mlua::Error::external)
        });
        m.add_method("clear", |_, this, ()| {
            unreal::array_clear(this.addr, &this.inner);
            Ok(())
        });
        // for i, v in arr:items() do ... end
        m.add_method("items", |lua, this, ()| {
            let arr = *this;
            let f = lua.create_function(move |lua, (_, i): (Value, i64)| {
                let next = i + 1;
                match arr.elem(next) {
                    Some(e) => Ok((Value::Integer(next), unreal::read_prop(e, &arr.inner).map(|v| to_lua(lua, &v)).unwrap_or(Value::Nil))),
                    None => Ok((Value::Nil, Value::Nil)),
                }
            })?;
            Ok((f, Value::Nil, 0i64))
        });
    }
}

/// A `TMap`, read-only: `#`, `map:get(key)`, `map[key]`, `map:pairs()`.
#[derive(Debug, Clone, Copy)]
pub struct Map {
    addr: usize,
    key: Prop,
    value: Prop,
    elem_size: usize,
}

fn key_matches(k: &unreal::Value, lua_key: &Value) -> bool {
    use unreal::Value as V;
    match (k, lua_key) {
        (V::Str(a), Value::String(b)) => a.eq_ignore_ascii_case(&b.to_string_lossy()),
        (V::Int(a), Value::Integer(b)) => a == b,
        (V::Float(a), Value::Number(b)) => a == b,
        (V::Object(a), Value::UserData(ud)) => ud.borrow::<Object>().is_ok_and(|o| o.0 == *a),
        _ => false,
    }
}

impl UserData for Map {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("addr", |_, this, ()| Ok(this.addr as i64));
        m.add_meta_method(MetaMethod::Len, |_, this, ()| {
            Ok(unreal::map_entries(this.addr, &this.key, &this.value, this.elem_size).len())
        });
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Map<{:?},{:?}> @0x{:x}", this.key.kind, this.value.kind, this.addr))
        });
        m.add_method("get", |lua, this, key: Value| {
            Ok(unreal::map_entries(this.addr, &this.key, &this.value, this.elem_size)
                .into_iter()
                .find(|(k, _)| key_matches(k, &key))
                .map(|(_, v)| to_lua(lua, &v))
                .unwrap_or(Value::Nil))
        });
        m.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            Ok(unreal::map_entries(this.addr, &this.key, &this.value, this.elem_size)
                .into_iter()
                .find(|(k, _)| key_matches(k, &key))
                .map(|(_, v)| to_lua(lua, &v))
                .unwrap_or(Value::Nil))
        });
        // map:set(key, value): overwrite the value of an existing key in place.
        // Inserting new keys would need the engine's TSet rehash: not supported.
        m.add_method("set", |_, this, (key, value): (Value, Value)| {
            let wanted = from_lua(this.key.kind, &key)
                .ok_or_else(|| mlua::Error::external(format!("cannot use {} as a {:?} key", key.type_name(), this.key.kind)))?;
            let slot = unreal::map_find_slot(this.addr, &this.key, &this.value, this.elem_size, &wanted)
                .ok_or_else(|| mlua::Error::external("key not present (maps cannot grow from Lua)"))?;
            let p = Prop { offset: 0, ..this.value };
            write_field(slot, &p, "value", &value)
        });
        // for k, v in map:pairs() do ... end  (snapshot)
        m.add_method("pairs", |lua, this, ()| {
            let entries: Vec<(Value, Value)> = unreal::map_entries(this.addr, &this.key, &this.value, this.elem_size)
                .iter()
                .map(|(k, v)| (to_lua(lua, k), to_lua(lua, v)))
                .collect();
            let mut i = 0usize;
            let f = lua.create_function_mut(move |_, ()| {
                let r = entries.get(i).cloned().unwrap_or((Value::Nil, Value::Nil));
                i += 1;
                Ok(r)
            })?;
            Ok(f)
        });
    }
}

// ---------------------------------------------------------------------------
// Object proxy
// ---------------------------------------------------------------------------

/// A live `UObject*`. Methods are lowercase so they never collide with
/// UnrealScript property names (which are capitalised by convention).
#[derive(Debug, Clone, Copy)]
pub struct Object(pub usize);

fn property(obj: usize, key: &str) -> Option<Prop> {
    unreal::find_property(unreal::object_class(obj)?, key)
}

fn function(obj: usize, key: &str) -> Option<usize> {
    unreal::find_function(unreal::object_class(obj)?, key)
}

/// `obj:Func(args...)`: convert args by the function's param kinds, call,
/// and spread `ReturnValue` + out params as results.
fn call_function(lua: &Lua, obj: usize, func: usize, args: &[Value]) -> mlua::Result<mlua::MultiValue> {
    let params = unreal::function_params(func);
    let inputs: Vec<_> = params.iter().filter(|p| !p.is_return()).collect();
    let mut converted = Vec::with_capacity(args.len());
    for (i, v) in args.iter().enumerate() {
        let Some(p) = inputs.get(i) else {
            return Err(mlua::Error::external(format!(
                "too many arguments ({} given, {} expected)",
                args.len(),
                inputs.len()
            )));
        };
        converted.push(from_lua(p.prop.kind, v).ok_or_else(|| {
            mlua::Error::external(format!(
                "arg {} `{}`: cannot pass {} as {:?}",
                i + 1,
                p.name,
                v.type_name(),
                p.prop.kind
            ))
        })?);
    }
    let results = unreal::call_function(obj, func, &converted).map_err(mlua::Error::external)?;
    Ok(mlua::MultiValue::from_vec(results.iter().map(|(_, v)| to_lua(lua, v)).collect()))
}

/// A bound UFunction, produced by `obj.Func`; call it as `obj:Func(...)`.
#[derive(Debug, Clone, Copy)]
pub struct Method {
    obj: usize,
    func: usize,
}

impl UserData for Method {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Call, |lua, this, args: mlua::MultiValue| {
            let mut args = args.into_vec();
            let bound = matches!(args.first(), Some(Value::UserData(ud)) if ud.borrow::<Object>().is_ok_and(|o| o.0 == this.obj));
            if !bound {
                return Err(mlua::Error::external(format!(
                    "call UFunctions with a colon: obj:{}(...)",
                    unreal::object_name(this.func).unwrap_or_default()
                )));
            }
            args.remove(0);
            call_function(lua, this.obj, this.func, &args)
        });
        m.add_method("native", |_, this, ()| Ok(unreal::function_is_native(this.func)));
        m.add_method("params", |lua, this, ()| {
            let t = lua.create_table()?;
            for (i, p) in unreal::function_params(this.func).iter().enumerate() {
                let e = lua.create_table()?;
                e.set("name", p.name.as_str())?;
                e.set("kind", format!("{:?}", p.prop.kind))?;
                e.set("out", p.is_out())?;
                e.set("ret", p.is_return())?;
                t.set(i + 1, e)?;
            }
            Ok(t)
        });
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!(
                "Function({})",
                unreal::object_full_name(this.func).unwrap_or_else(|| "?".into())
            ))
        });
    }
}

/// Live view of a `ProcessEvent` params frame. Index by param name or
/// 1-based position; assignments write into the frame (so a `before`
/// handler changes what the game sees). `ReturnValue` is readable in
/// `after` handlers and writable in a cancelling `before` handler.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    parms: usize,
    func: usize,
}

impl Params {
    fn find(&self, key: &Value) -> Option<unreal::Param> {
        let params = unreal::function_params(self.func);
        match key {
            Value::String(s) => {
                let s = s.to_string_lossy();
                params.into_iter().find(|p| p.name == *s)
            }
            Value::Integer(i) if *i >= 1 => params.into_iter().nth(*i as usize - 1),
            _ => None,
        }
    }
}

impl UserData for Params {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            if let Value::String(s) = &key {
                if &*s.to_string_lossy() == "names" {
                    let names: Vec<String> = unreal::function_params(this.func).into_iter().map(|p| p.name).collect();
                    return names.into_lua(lua);
                }
            }
            Ok(match this.find(&key) {
                Some(p) if this.parms != 0 => unreal::read_prop(this.parms, &p.prop).map(|v| to_lua(lua, &v)).unwrap_or(Value::Nil),
                _ => Value::Nil,
            })
        });
        m.add_meta_method(MetaMethod::NewIndex, |_, this, (key, value): (Value, Value)| {
            let p = this.find(&key).ok_or_else(|| mlua::Error::external(format!("no param {key:?}")))?;
            write_field(this.parms, &p.prop, &p.name, &value)
        });
        m.add_meta_method(MetaMethod::Len, |_, this, ()| Ok(unreal::function_params(this.func).len()));
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            let parts: Vec<String> = unreal::function_params(this.func)
                .iter()
                .map(|p| {
                    let v = if this.parms != 0 { unreal::read_prop(this.parms, &p.prop) } else { None };
                    format!("{}={:?}", p.name, v.unwrap_or(unreal::Value::Nil))
                })
                .collect();
            Ok(format!("Params({})", parts.join(", ")))
        });
    }
}

impl UserData for Object {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("ptr", |_, this, ()| Ok(this.0 as i64));
        m.add_method("name", |_, this, ()| Ok(unreal::object_name(this.0)));
        m.add_method("path", |_, this, ()| Ok(unreal::object_path(this.0)));
        m.add_method("full_name", |_, this, ()| Ok(unreal::object_full_name(this.0)));
        m.add_method("class", |_, this, ()| Ok(unreal::object_class(this.0).map(Object)));
        m.add_method("outer", |_, this, ()| Ok(unreal::object_outer(this.0).map(Object)));
        // obj:is_a("TgGame.TgPlayerController") or obj:is_a(classObject)
        m.add_method("is_a", |_, this, class: Value| {
            Ok(match class {
                Value::String(s) => unreal::is_a_path(this.0, &s.to_string_lossy()),
                Value::UserData(ud) => ud.borrow::<Object>().map(|c| unreal::is_a(this.0, c.0)).unwrap_or(false),
                _ => false,
            })
        });
        // obj:has("PlayerName") — true when the class declares the property or function.
        m.add_method("has", |_, this, key: String| {
            Ok(property(this.0, &key).is_some() || function(this.0, &key).is_some())
        });
        // obj:prop("Name") -> { class, kind, offset, size, ext, ext_name } or nil
        m.add_method("prop", |lua, this, key: String| {
            let Some(class) = unreal::object_class(this.0) else { return Ok(Value::Nil) };
            let Some(field) = unreal::find_property_object(class, &key) else { return Ok(Value::Nil) };
            let t = lua.create_table()?;
            t.set("class", unreal::prop_class_name(field))?;
            t.set("ptr", field as i64)?;
            if let Some(p) = unreal::find_property(class, &key) {
                t.set("kind", format!("{:?}", p.kind))?;
                t.set("offset", p.offset)?;
                t.set("size", p.elem_size)?;
                t.set("ext", p.ext as i64)?;
                t.set("ext_name", unreal::object_name(p.ext))?;
            }
            Ok(Value::Table(t))
        });
        // obj:map("m_Scenes", "name", "object") -> Map proxy for a native map
        // (reflection has no pair types for `var native Map{...}`).
        m.add_method("map", |lua, this, (name, key, value): (String, Option<String>, Option<String>)| {
            let p = property(this.0, &name).ok_or_else(|| mlua::Error::external(format!("no property `{name}`")))?;
            if p.kind != PropKind::Map {
                return Err(mlua::Error::external(format!("`{name}` is not a map property")));
            }
            let key = unreal::synthetic_prop(key.as_deref().unwrap_or("name"))
                .ok_or_else(|| mlua::Error::external("unknown key kind"))?;
            let value = unreal::synthetic_prop(value.as_deref().unwrap_or("object"))
                .ok_or_else(|| mlua::Error::external("unknown value kind"))?;
            Ok(to_lua(lua, &unreal::map_with(this.0 + p.offset, key, value)))
        });
        // obj:call("Func", ...) — explicit form of obj:Func(...).
        m.add_method("call", |lua, this, args: mlua::MultiValue| {
            let mut args = args.into_vec();
            let name = match args.first() {
                Some(Value::String(s)) => s.to_string_lossy().to_string(),
                _ => return Err(mlua::Error::external("call(name, ...) needs a function name")),
            };
            args.remove(0);
            let func = function(this.0, &name).ok_or_else(|| {
                mlua::Error::external(format!(
                    "{} has no function `{name}`",
                    unreal::object_full_name(this.0).unwrap_or_else(|| format!("0x{:x}", this.0))
                ))
            })?;
            call_function(lua, this.0, func, &args)
        });
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!(
                "Object(0x{:x} {})",
                this.0,
                unreal::object_full_name(this.0).unwrap_or_else(|| "?".into())
            ))
        });
        m.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| {
            Ok(other.borrow::<Object>().map(|o| o.0 == this.0).unwrap_or(false))
        });
        m.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            let Value::String(key) = key else { return Ok(Value::Nil) };
            let key = key.to_string_lossy();
            if let Some(p) = property(this.0, &key) {
                return Ok(unreal::read_prop(this.0, &p).map(|v| to_lua(lua, &v)).unwrap_or(Value::Nil));
            }
            match function(this.0, &key) {
                Some(func) => Ok(Value::UserData(lua.create_userdata(Method { obj: this.0, func })?)),
                None => {
                    crate::log::line(
                        "lua",
                        &format!(
                            "{} has no property `{key}`",
                            unreal::object_full_name(this.0).unwrap_or_else(|| format!("0x{:x}", this.0))
                        ),
                    );
                    Ok(Value::Nil)
                }
            }
        });
        m.add_meta_method(MetaMethod::NewIndex, |_, this, (key, value): (String, Value)| {
            let p = property(this.0, &key).ok_or_else(|| {
                mlua::Error::external(format!(
                    "{} has no property `{key}`",
                    unreal::object_full_name(this.0).unwrap_or_else(|| format!("0x{:x}", this.0))
                ))
            })?;
            write_field(this.0, &p, &key, &value)
        });
    }
}

// ---------------------------------------------------------------------------
// memory.*
// ---------------------------------------------------------------------------

fn memory_table(lua: &Lua) -> mlua::Result<Table> {
    let m = lua.create_table()?;
    m.set("base", lua.create_function(|_, ()| Ok(memory::main_module().map(|m| m.base as i64)))?)?;
    m.set("size", lua.create_function(|_, ()| Ok(memory::main_module().map(|m| m.size as i64)))?)?;

    macro_rules! reader {
        ($name:literal, $ty:ty, $conv:ty) => {
            m.set(
                $name,
                lua.create_function(|_, addr: i64| {
                    if addr <= 0 {
                        return Ok(None);
                    }
                    Ok(unsafe { memory::read::<$ty>(addr as usize) }.map(|v| v as $conv))
                })?,
            )?;
        };
    }
    reader!("read_u8", u8, i64);
    reader!("read_u16", u16, i64);
    reader!("read_u32", u32, i64);
    reader!("read_u64", u64, i64);
    reader!("read_i8", i8, i64);
    reader!("read_i16", i16, i64);
    reader!("read_i32", i32, i64);
    reader!("read_i64", i64, i64);
    reader!("read_f32", f32, f64);
    reader!("read_f64", f64, f64);
    reader!("read_ptr", usize, i64);

    macro_rules! writer {
        ($name:literal, $ty:ty, $from:ty) => {
            m.set(
                $name,
                lua.create_function(|_, (addr, v): (i64, $from)| {
                    if addr <= 0 {
                        return Ok(false);
                    }
                    Ok(unsafe { memory::write(addr as usize, v as $ty) })
                })?,
            )?;
        };
    }
    writer!("write_u8", u8, i64);
    writer!("write_u16", u16, i64);
    writer!("write_u32", u32, i64);
    writer!("write_u64", u64, i64);
    writer!("write_i32", i32, i64);
    writer!("write_i64", i64, i64);
    writer!("write_f32", f32, f64);
    writer!("write_f64", f64, f64);

    m.set(
        "read_str",
        lua.create_function(|_, (addr, max): (i64, Option<usize>)| {
            Ok((addr > 0).then(|| memory::read_cstr(addr as usize, max.unwrap_or(256))).flatten())
        })?,
    )?;
    m.set(
        "read_wstr",
        lua.create_function(|_, (addr, max): (i64, Option<usize>)| {
            Ok((addr > 0).then(|| memory::read_wstr(addr as usize, max.unwrap_or(256))).flatten())
        })?,
    )?;
    m.set(
        "scan",
        lua.create_function(|lua, (pattern, limit): (String, Option<usize>)| {
            let out = lua.create_table()?;
            let Some(module) = memory::main_module() else { return Ok(out) };
            let pat = memory::parse_pattern(&pattern).map_err(mlua::Error::external)?;
            for (i, addr) in memory::scan_all(module, &pat, limit.unwrap_or(16)).into_iter().enumerate() {
                out.set(i + 1, addr as i64)?;
            }
            Ok(out)
        })?,
    )?;
    Ok(m)
}
