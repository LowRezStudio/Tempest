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

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mlua::{AnyUserData, Function, IntoLua, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};

use crate::hooks;
use crate::memory;
use crate::unreal::{self, Event, Phase, Prop, PropKind};

/// A Lua number for a Rust integer: an `Integer` when it fits the VM's
/// integer type (`i32` on the Win32 client, where LuaJIT's `lua_Integer` is
/// `ptrdiff_t`), else a `Number` (exact below 2^53). Addresses always fit.
fn int(v: i64) -> Value {
    match mlua::Integer::try_from(v) {
        Ok(i) => Value::Integer(i),
        Err(_) => Value::Number(v as f64),
    }
}

/// The Rust integer behind a Lua `Integer` (`i32` on the Win32 client).
#[allow(clippy::unnecessary_cast)]
pub(crate) fn to_i64(i: mlua::Integer) -> i64 {
    i as i64
}

fn registry_key(id: u64) -> String {
    format!("tempest.sub.{id}")
}

fn hook_key(id: u64) -> String {
    format!("tempest.hook.{id}")
}

pub fn register(lua: &Lua) -> mlua::Result<()> {
    unreal::set_dispatcher(deliver);
    hooks::set_dispatcher(deliver_address_hook);

    // `print` → log, so script output lands in the file and the console.
    // Arguments go through `tostring` (`__tostring` included), like Lua's own.
    lua.globals().set(
        "print",
        lua.create_function(|_, args: mlua::MultiValue| {
            let text: Vec<String> = args
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.to_string_lossy().to_string(),
                    other => other.to_string().unwrap_or_else(|_| format!("{other:?}")),
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
            let owner = crate::loader::current_mod();
            unreal::queue(Box::new(move || {
                let lua = crate::lua();
                if let Ok(f) = lua.registry_value::<Function>(&key) {
                    if let Err(e) = crate::loader::with_mod(&owner, || f.call::<()>(())) {
                        crate::log::line("lua", &format!("[{owner}] queued job failed: {e}"));
                    }
                }
                let _ = lua.remove_registry_value(key);
            }));
            Ok(())
        })?,
    )?;
    // tempest._with_mod(id, fn, ...) -> fn's results, run as mod `id` (key
    // bindings and console commands dispatch from the prelude, outside any mod).
    t.set(
        "_with_mod",
        lua.create_function(|_, (id, f, args): (String, Function, mlua::MultiValue)| {
            crate::loader::with_mod(&id, || f.call::<mlua::MultiValue>(args))
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
    register_config(lua, &t)?;
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
            // Default: <game>/mods/types/sdk when the dev folder exists (where the
            // runtime also drops its own .d.lua files), else the runtime mod's types/.
            let dir = match dir {
                Some(d) => std::path::PathBuf::from(d),
                None => {
                    let base = crate::config::dev_mods_dir()
                        .filter(|d| d.is_dir())
                        .or_else(|| crate::loader::dll_dir().and_then(|d| d.parent().map(|p| p.to_path_buf())))
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
    // tempest.installed() -> every mod the launcher knows (Lua or not) plus dev mods:
    // { id, name, version, authors, kind, enabled, lua, dll, files, dev, loaded, path }.
    // tempest.mods() above is the cheap view: the Lua mods running, in load order.
    t.set(
        "installed",
        lua.create_function(|lua, ()| {
            let out = lua.create_table()?;
            for (i, m) in crate::loader::installed().into_iter().enumerate() {
                let e = lua.create_table()?;
                e.set("id", m.id)?;
                e.set("name", m.name)?;
                e.set("version", m.version)?;
                e.set("authors", m.authors)?;
                e.set("kind", m.kind)?;
                e.set("enabled", m.enabled)?;
                e.set("lua", m.lua)?;
                e.set("dll", m.dll)?;
                e.set("files", m.files)?;
                e.set("dev", m.dev)?;
                e.set("loaded", m.loaded)?;
                e.set("path", m.path.map(|p| p.display().to_string()))?;
                out.set(i + 1, e)?;
            }
            Ok(out)
        })?,
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
    // tempest.default_object("Engine.PointLight" | classObj) -> the class default object
    t.set(
        "default_object",
        lua.create_function(|_, class: Value| {
            let class = match &class {
                Value::String(s) => unreal::find_class(&s.to_string_lossy()),
                Value::UserData(ud) => ud.borrow::<Object>().ok().map(|o| o.0),
                _ => None,
            };
            Ok(class.and_then(unreal::default_object).map(Object))
        })?,
    )?;
    // Backs tempest.native (prelude): make "Pkg.Class.Func" call into Lua.
    // Two kinds of function qualify: a `dllimport` declaration (no body; the
    // runtime rebinds it to a Lua-native thunk) and an ordinary script
    // function with a stub body (any parameter types; the ProcessInternal
    // detour's cancelling `before` handler is the implementation and the
    // body is the fallback). Returns true when ready, false (with a log
    // line) when the class is not loaded yet; raises for a real native.
    t.set(
        "_bind_native",
        lua.create_function(|_, spec: String| {
            let Some((class_path, name)) = spec.rsplit_once('.') else {
                return Err(mlua::Error::external(format!("tempest.native: `{spec}` must be Package.Class.Func")));
            };
            let Some(class) = unreal::find_class(class_path) else {
                crate::log::line("lua", &format!("tempest.native: class {class_path} is not loaded; `{name}` binds when it is"));
                return Ok(false);
            };
            let Some(func) = unreal::find_function(class, name) else {
                return Err(mlua::Error::external(format!("tempest.native: {class_path} has no function `{name}`")));
            };
            if unreal::is_lua_native(func) {
                return Ok(true);
            }
            if unreal::function_is_lua_bindable(func) {
                unreal::bind_lua_native(func).map_err(|e| mlua::Error::external(format!("tempest.native: {e}")))?;
                return Ok(true);
            }
            if unreal::function_is_native(func) {
                return Err(mlua::Error::external(format!(
                    "tempest.native: {spec} is a native with a C++ implementation; hook it with tempest.before instead, \
                     or declare your own function (`dllimport`, or a script function with a stub body)"
                )));
            }
            Ok(true)
        })?,
    )?;
    // tempest.object(ptr) -> Object proxy for a raw UObject* (nil unless the
    // pointer and its class pointer are readable), for address-hook arguments.
    t.set(
        "object",
        lua.create_function(|_, ptr: i64| {
            let ptr = ptr as usize;
            let ok = ptr != 0
                && memory::is_readable(ptr, unreal::layout::UOBJECT_CLASS + unreal::layout::PTR)
                && unreal::object_class(ptr).is_some_and(|c| memory::is_readable(c, unreal::layout::UOBJECT_CLASS + unreal::layout::PTR));
            Ok(ok.then_some(Object(ptr)))
        })?,
    )?;
    register_ui(lua, &t)?;
    lua.globals().set("tempest", t)?;
    lua.globals().set("memory", memory_table(lua)?)?;
    run_prelude(lua)
}

/// Built-in Lua layer (`tempest.command` & co, then the `ui` module). Re-run
/// on every reload because reload drops the subscriptions it depends on.
pub fn run_prelude(lua: &Lua) -> mlua::Result<()> {
    lua.load(include_str!("prelude.lua")).set_name("=prelude").exec()?;
    lua.load(include_str!("ui.lua")).set_name("=ui").exec()
}

/// `tempest.config()` & co: the mod's own config file, and the runtime settings.
fn register_config(lua: &Lua, t: &Table) -> mlua::Result<()> {
    // tempest.config(defaults?) -> table: defaults <- <mod>/config.toml <- <configs>/<id>.toml;
    // the user's file is created from the merged values when missing.
    t.set(
        "config",
        lua.create_function(|lua, defaults: Option<Table>| {
            let id = crate::loader::current_mod();
            if id.is_empty() {
                return Err(mlua::Error::external("tempest.config: no mod is running (call it from a mod script or handler)"));
            }
            let defaults = match defaults {
                Some(d) => crate::config::table_from_lua(&Value::Table(d)).map_err(mlua::Error::external)?,
                None => toml::Table::new(),
            };
            let m = crate::loader::find(&id).ok_or_else(|| mlua::Error::external(format!("tempest.config: mod `{id}` is not loaded")))?;
            let (merged, _) = crate::config::load_mod_config(&m, defaults).map_err(mlua::Error::external)?;
            crate::config::to_lua(lua, &toml::Value::Table(merged))
        })?,
    )?;
    // tempest.save_config(table) -> path
    t.set(
        "save_config",
        lua.create_function(|_, table: Table| {
            let id = crate::loader::current_mod();
            if id.is_empty() {
                return Err(mlua::Error::external("tempest.save_config: no mod is running"));
            }
            let table = crate::config::table_from_lua(&Value::Table(table)).map_err(mlua::Error::external)?;
            let m = crate::loader::find(&id).ok_or_else(|| mlua::Error::external(format!("tempest.save_config: mod `{id}` is not loaded")))?;
            let path = crate::config::save_mod_config(&m, &table).map_err(mlua::Error::external)?;
            Ok(path.display().to_string())
        })?,
    )?;
    t.set(
        "config_path",
        lua.create_function(|_, ()| {
            Ok(crate::loader::find(&crate::loader::current_mod()).and_then(|m| crate::config::config_file(&m)).map(|p| p.display().to_string()))
        })?,
    )?;
    t.set("game_dir", lua.create_function(|_, ()| Ok(crate::config::get().game_dir.as_ref().map(|p| p.display().to_string())))?)?;
    // tempest.settings() -> the runtime settings (read-only copy)
    t.set(
        "settings",
        lua.create_function(|lua, ()| {
            let c = crate::config::get();
            let s = lua.create_table()?;
            s.set("path", c.path.as_ref().map(|p| p.display().to_string()))?;
            s.set("game_dir", c.game_dir.as_ref().map(|p| p.display().to_string()))?;
            s.set("log", crate::log::path())?;
            s.set("console", c.console)?;
            s.set("jit", c.jit)?;
            s.set("dev_mods", crate::config::dev_mods_dir().map(|p| p.display().to_string()))?;
            s.set("hot_reload", c.hot_reload)?;
            s.set("reload_on_config", c.reload_on_config)?;
            s.set("hot_reload_delay_ms", c.hot_reload_delay_ms)?;
            s.set("mod_configs", crate::config::mod_configs_dir().map(|p| p.display().to_string()))?;
            s.set("ui_scale", c.ui_scale)?;
            Ok(s)
        })?,
    )?;
    Ok(())
}

/// `tempest._ui_*`: the Rust side of the `ui` module (ui.lua), plus the
/// generated cimgui declarations in `tempest._imgui`.
fn register_ui(lua: &Lua, t: &Table) -> mlua::Result<()> {
    let spec: Table = lua.load(include_str!("imgui_cdef.lua")).set_name("=imgui_cdef").eval()?;
    t.set("_imgui", spec)?;
    t.set(
        "_ui_syms",
        lua.create_function(|lua, ()| {
            let out = lua.create_table()?;
            for (name, addr) in crate::imgui_syms::table() {
                out.set(name, addr as i64)?;
            }
            Ok(out)
        })?,
    )?;
    crate::overlay::set_scale(crate::config::get().ui_scale);
    if let Some(game) = &crate::config::get().game_dir {
        crate::overlay::set_ini(&game.join("ChaosGame").join("Config").join("tempest-ui.ini").display().to_string());
    }
    t.set("_ui_ready", lua.create_function(|_, ()| Ok(crate::overlay::ready()))?)?;
    // Fonts: _ui_add_font(path|"", size, merge, {first, last, ...}?) -> ImFont* address or nil
    t.set(
        "_ui_add_font",
        lua.create_function(|_, (path, size, merge, ranges): (Option<String>, f32, Option<bool>, Option<Vec<u16>>)| {
            let f = crate::overlay::add_font(path.as_deref().unwrap_or(""), size, merge.unwrap_or(false), ranges.as_deref().unwrap_or(&[]));
            Ok((f != 0).then_some(f as i64))
        })?,
    )?;
    t.set(
        "_ui_set_default_font",
        lua.create_function(|_, font: i64| {
            crate::overlay::set_default_font(font as usize);
            Ok(())
        })?,
    )?;
    t.set("_ui_fonts_dirty", lua.create_function(|_, ()| Ok(crate::overlay::fonts_dirty()))?)?;
    // Images: _ui_load_image(path) -> id, w, h; _ui_load_image_rgba(bytes, w, h) -> id
    t.set(
        "_ui_load_image",
        lua.create_function(|_, path: String| {
            Ok(match crate::overlay::load_image(&path) {
                Some((id, w, h)) => (int(id as i64), int(w as i64), int(h as i64)),
                None => (Value::Nil, Value::Nil, Value::Nil),
            })
        })?,
    )?;
    t.set(
        "_ui_load_image_rgba",
        lua.create_function(|_, (bytes, w, h): (mlua::String, i32, i32)| Ok(crate::overlay::load_image_rgba(&bytes.as_bytes(), w, h)))?,
    )?;
    t.set("_ui_texture", lua.create_function(|_, id: i32| Ok(crate::overlay::texture_id(id) as i64))?)?;
    t.set(
        "_ui_free_texture",
        lua.create_function(|_, id: i32| {
            crate::overlay::free_texture(id);
            Ok(())
        })?,
    )?;
    t.set("_ui_headless_upload", lua.create_function(|_, ()| Ok(crate::overlay::headless_upload()))?)?;
    t.set("_ui_backend", lua.create_function(|_, ()| Ok(crate::overlay::backend()))?)?;
    t.set("_ui_hwnd", lua.create_function(|_, ()| Ok(crate::overlay::hwnd() as i64))?)?;
    t.set("_ui_stats", lua.create_function(|_, ()| Ok(crate::overlay::stats()))?)?;
    t.set(
        "_ui_capture",
        lua.create_function(|_, on: bool| {
            crate::overlay::set_capture(on);
            Ok(())
        })?,
    )?;
    t.set("_ui_capturing", lua.create_function(|_, ()| Ok(crate::overlay::capturing()))?)?;
    t.set("_ui_want", lua.create_function(|_, ()| Ok(crate::overlay::want()))?)?;
    t.set(
        "_ui_scale",
        lua.create_function(|_, s: Option<f32>| {
            if let Some(s) = s {
                crate::overlay::set_scale(s);
            }
            Ok(crate::overlay::scale())
        })?,
    )?;
    // Run one draw handler under ImGui's error recovery; returns the error text.
    t.set(
        "_ui_protect",
        lua.create_function(|_, (f, owner): (Function, Option<String>)| {
            Ok(crate::overlay::run_handler(&f, owner.as_deref().unwrap_or("")))
        })?,
    )?;
    // Tests / diagnostics: a backend-less context, and an immediate frame.
    t.set("_ui_headless", lua.create_function(|_, (w, h): (f32, f32)| Ok(crate::overlay::init_headless(w, h)))?)?;
    t.set("_ui_build_frame", lua.create_function(|_, ()| Ok(crate::overlay::build_frame()))?)?;
    Ok(())
}

fn status(lua: &Lua) -> mlua::Result<Table> {
    let g = unreal::globals();
    let addr = |v: usize| if v == 0 { Value::Nil } else { int(v as i64) };
    let t = lua.create_table()?;
    t.set("arch", crate::arch())?;
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
    t.set("address_hooks", address_hooks(lua)?)?;
    t.set("lua_natives", unreal::lua_natives())?;
    t.set("key_hook_installed", crate::input::installed())?;
    t.set("ui_backend", crate::overlay::backend())?;
    t.set("ui_ready", crate::overlay::ready())?;
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

// ---------------------------------------------------------------------------
// Address hooks (memory.hook): the handler chain for one intercepted call
// ---------------------------------------------------------------------------

/// `{ {id, addr, mod}, ... }` for `memory.hooks()` and `tempest.status()`.
fn address_hooks(lua: &Lua) -> mlua::Result<Table> {
    let out = lua.create_table()?;
    for (i, h) in hooks::list().iter().enumerate() {
        let e = lua.create_table()?;
        e.set("id", h.id)?;
        e.set("addr", h.addr as i64)?;
        e.set("mod", h.owner.as_str())?;
        e.set("args", h.kinds.iter().map(|k| format!("{k:?}").to_ascii_lowercase()).collect::<Vec<_>>())?;
        e.set("conv", h.conv.name())?;
        out.set(i + 1, e)?;
    }
    Ok(out)
}

/// A float argument for `memory.call` / `original(...)`: `memory.f32(x)`,
/// `memory.f64(x)`.
#[derive(Debug, Clone, Copy)]
pub struct FloatArg {
    value: f64,
    kind: hooks::ArgKind,
}

impl UserData for FloatArg {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(format!("{}({})", if this.kind == hooks::ArgKind::F32 { "f32" } else { "f64" }, this.value)));
    }
}

/// A Lua value as a machine word: integers, booleans and Object proxies
/// (their pointer). `None` for anything else.
fn to_word(v: &Value) -> Option<usize> {
    match v {
        Value::Integer(n) => Some(*n as usize),
        Value::Number(n) => Some(*n as i64 as usize),
        Value::Boolean(b) => Some(*b as usize),
        Value::UserData(ud) => ud.borrow::<Object>().ok().map(|o| o.0),
        _ => None,
    }
}

/// Place argument `i` of a call at `loc`: a `memory.f32/f64` value as the
/// float it is, anything word-like as a machine word, a plain number as
/// whatever `kind` says.
fn place_arg(regs: &mut hooks::Regs, i: usize, loc: hooks::Loc, kind: hooks::ArgKind, v: &Value) -> Result<(), String> {
    if let Value::UserData(ud) = v {
        if let Ok(f) = ud.borrow::<FloatArg>() {
            // On x86 a float lives on the stack, an f64 across two dwords: it
            // only fits where the hook declared one.
            if cfg!(target_pointer_width = "32") && kind == hooks::ArgKind::Int && (f.kind == hooks::ArgKind::F64 || !matches!(loc, hooks::Loc::Stack(_))) {
                return Err(format!("argument {} is declared int; declare it {:?} in opts.args to pass a float", i + 1, f.kind));
            }
            regs.set(loc, f.kind, f.value);
            return Ok(());
        }
    }
    match (kind, v) {
        (hooks::ArgKind::Int, _) => {
            let w = to_word(v).ok_or_else(|| format!("argument {} must be an integer, boolean, Object or memory.f32/f64 value, not {}", i + 1, v.type_name()))?;
            regs.set_word(loc, w);
            Ok(())
        }
        (k, Value::Integer(n)) => {
            regs.set(loc, k, *n as f64);
            Ok(())
        }
        (k, Value::Number(n)) => {
            regs.set(loc, k, *n);
            Ok(())
        }
        (k, other) => Err(format!("argument {} is declared {k:?} and must be a number, not {}", i + 1, other.type_name())),
    }
}

/// `conv` option of `memory.hook` / `memory.call`: the x86 calling
/// convention (`"cdecl"` default, `"stdcall"`, `"thiscall"`, `"fastcall"`);
/// accepted and ignored on x64.
fn parse_conv(v: Option<String>) -> mlua::Result<hooks::Conv> {
    match v {
        None => Ok(hooks::Conv::default()),
        Some(name) => hooks::Conv::parse(&name).ok_or_else(|| mlua::Error::external(format!("unknown calling convention `{name}` (cdecl, stdcall, thiscall, fastcall)"))),
    }
}

/// `args` option of `memory.hook`: a count (that many integers) or a list
/// of kind names (`"int"`, `"f32"`, `"f64"`, `"ptr"`).
fn parse_kinds(v: Option<Value>) -> mlua::Result<Vec<hooks::ArgKind>> {
    match v {
        None | Some(Value::Nil) => Ok(vec![hooks::ArgKind::Int; 4]),
        Some(Value::Integer(n)) => Ok(vec![hooks::ArgKind::Int; n.max(0) as usize]),
        Some(Value::Number(n)) => Ok(vec![hooks::ArgKind::Int; n.max(0.0) as usize]),
        Some(Value::Table(t)) => {
            let mut kinds = Vec::new();
            for pair in t.sequence_values::<String>() {
                let name = pair?;
                kinds.push(hooks::ArgKind::parse(&name).ok_or_else(|| mlua::Error::external(format!("memory.hook: unknown argument kind `{name}` (int, ptr, f32, f64)")))?);
            }
            Ok(kinds)
        }
        Some(other) => Err(mlua::Error::external(format!("memory.hook: `args` must be a count or a list of kinds, not {}", other.type_name()))),
    }
}

fn parse_ret(v: Option<String>) -> mlua::Result<hooks::ArgKind> {
    match v {
        None => Ok(hooks::ArgKind::Int),
        Some(name) => hooks::ArgKind::parse(&name).ok_or_else(|| mlua::Error::external(format!("unknown result kind `{name}` (int, f32, f64)"))),
    }
}

/// Entry from the detour thunk: run the chain from its newest handler.
fn deliver_address_hook(call: hooks::Call) -> hooks::Ret {
    let regs = call.regs;
    run_hook_level(crate::lua(), Arc::new(call), 0, regs)
}

/// Run handler `level` of the chain with `regs`. The handler gets
/// `(original, a1, ..., aN)` typed by its declared kinds; `original(...)`
/// runs the rest of the chain (nil or missing arguments keep the incoming
/// ones) and is only valid while the handler runs. The handler's numeric
/// result is the call's result (in XMM0 for a float `ret`); `nil` means
/// "whatever `original` returned", or a plain pass-through when it was
/// never called.
fn run_hook_level(lua: &Lua, call: Arc<hooks::Call>, level: usize, regs: hooks::Regs) -> hooks::Ret {
    let Some(h) = call.handlers.get(level).cloned() else {
        return hooks::call_original(call.slot, &regs);
    };
    let Ok(cb) = lua.named_registry_value::<Function>(&hook_key(h.id)) else {
        return run_hook_level(lua, call, level + 1, regs); // unhooked since the snapshot
    };
    let alive = Arc::new(AtomicBool::new(true));
    let forwarded: Arc<Mutex<Option<hooks::Ret>>> = Arc::new(Mutex::new(None));
    let original = {
        let (call, alive, forwarded, kinds, locs) = (call.clone(), alive.clone(), forwarded.clone(), h.kinds.clone(), h.locs.clone());
        lua.create_function(move |lua, over: mlua::Variadic<Value>| {
            if !alive.load(Ordering::Acquire) {
                return Err(mlua::Error::external("original() is only valid while the hook handler runs"));
            }
            let mut merged = regs;
            for (i, v) in over.iter().enumerate().take(locs.len()) {
                if v.is_nil() {
                    continue;
                }
                let kind = kinds.get(i).copied().unwrap_or(hooks::ArgKind::Int);
                place_arg(&mut merged, i, locs[i], kind, v).map_err(|e| mlua::Error::external(format!("original(): {e}")))?;
            }
            let r = run_hook_level(lua, call.clone(), level + 1, merged);
            *forwarded.lock().unwrap() = Some(r);
            Ok(match h.ret {
                hooks::ArgKind::Int => int(r.int as i64),
                k => Value::Number(k.decode(r.fbits)),
            })
        })
    };
    let Ok(original) = original else {
        return run_hook_level(lua, call, level + 1, regs);
    };
    let mut lua_args: Vec<Value> = Vec::with_capacity(1 + h.kinds.len());
    lua_args.push(Value::Function(original));
    for (kind, loc) in h.kinds.iter().zip(&h.locs) {
        lua_args.push(match kind {
            hooks::ArgKind::Int => int(regs.word(*loc) as i64),
            k => Value::Number(regs.get(*loc, *k)),
        });
    }
    let result = crate::loader::with_mod(&h.owner, || cb.call::<Value>(mlua::MultiValue::from_vec(lua_args)));
    alive.store(false, Ordering::Release);
    let forwarded = *forwarded.lock().unwrap();
    let pass_through = |lua: &Lua| forwarded.unwrap_or_else(|| run_hook_level(lua, call.clone(), level + 1, regs));
    let tag = if h.owner.is_empty() { String::new() } else { format!("[{}] ", h.owner) };
    match result {
        Ok(Value::Nil) => pass_through(lua),
        Ok(v) => {
            let as_number = match &v {
                Value::Integer(n) => Some(*n as f64),
                Value::Number(n) => Some(*n),
                _ => None,
            };
            match (h.ret, as_number, to_word(&v)) {
                (hooks::ArgKind::Int, _, Some(w)) => hooks::Ret { int: w, fbits: 0 },
                (k, Some(n), _) if k != hooks::ArgKind::Int => {
                    let bits = k.encode(n);
                    hooks::Ret { int: bits as usize, fbits: bits }
                }
                _ => {
                    crate::log::line("lua", &format!("{tag}hook #{} on 0x{:x} returned a {}; expected a number or nil", h.id, h.addr, v.type_name()));
                    pass_through(lua)
                }
            }
        }
        Err(e) => {
            crate::log::line("lua", &format!("{tag}hook #{} on 0x{:x} failed: {e}", h.id, h.addr));
            pass_through(lua)
        }
    }
}

fn to_lua(lua: &Lua, v: &unreal::Value) -> Value {
    use unreal::Value as V;
    match v {
        V::Nil => Value::Nil,
        V::Int(i) => int(*i),
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
        V::Addr(a) => int(*a as i64),
        V::List(items) => match lua.create_table() {
            Ok(t) => {
                for (i, item) in items.iter().enumerate() {
                    let _ = t.set(i + 1, to_lua(lua, item));
                }
                Value::Table(t)
            }
            Err(_) => Value::Nil,
        },
        V::Record(fields) => match lua.create_table() {
            Ok(t) => {
                for (k, v) in fields {
                    let _ = t.set(k.as_str(), to_lua(lua, v));
                }
                Value::Table(t)
            }
            Err(_) => Value::Nil,
        },
    }
}

/// A Lua value with no property kind to guide it (array elements, struct
/// fields given as tables): tables become Vector/Rotator, List or Record.
fn from_lua_any(v: &Value) -> Option<unreal::Value> {
    use unreal::Value as V;
    Some(match v {
        Value::Nil => V::Nil,
        Value::Boolean(b) => V::Bool(*b),
        Value::Integer(i) => V::Int(to_i64(*i)),
        Value::Number(n) => V::Float(*n),
        Value::String(s) => V::Str(s.to_string_lossy().to_string()),
        Value::UserData(ud) => {
            if let Ok(o) = ud.borrow::<Object>() {
                V::Object(o.0)
            } else if let Ok(s) = ud.borrow::<Struct>() {
                V::Struct { addr: s.addr, kind: s.kind }
            } else if let Ok(a) = ud.borrow::<Array>() {
                V::Array { addr: a.addr, inner: a.inner }
            } else {
                return None;
            }
        }
        Value::Table(t) => {
            if let (Ok(x), Ok(y), Ok(z)) = (t.get::<f32>("x"), t.get::<f32>("y"), t.get::<f32>("z")) {
                return Some(V::Vector(x, y, z));
            }
            if let (Ok(p), Ok(y), Ok(r)) = (t.get::<i32>("pitch"), t.get::<i32>("yaw"), t.get::<i32>("roll")) {
                return Some(V::Rotator(p, y, r));
            }
            let len = t.raw_len();
            if len > 0 || t.clone().pairs::<Value, Value>().next().is_none() {
                let mut items = Vec::with_capacity(len);
                for i in 1..=len {
                    items.push(from_lua_any(&t.get::<Value>(i).ok()?)?);
                }
                V::List(items)
            } else {
                let mut fields = Vec::new();
                for pair in t.clone().pairs::<String, Value>() {
                    let (k, v) = pair.ok()?;
                    fields.push((k, from_lua_any(&v)?));
                }
                V::Record(fields)
            }
        }
        _ => return None,
    })
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
        (_, Value::Integer(i)) => V::Int(to_i64(*i)),
        (_, Value::Number(n)) => V::Float(*n),
        (_, Value::String(s)) => V::Str(s.to_string_lossy().to_string()),
        (PropKind::Object, Value::UserData(ud)) => V::Object(ud.borrow::<Object>().ok()?.0),
        (PropKind::Struct, Value::UserData(ud)) => {
            let s = ud.borrow::<Struct>().ok()?;
            V::Struct { addr: s.addr, kind: s.kind }
        }
        (PropKind::Struct, Value::Table(_)) | (PropKind::Array, Value::Table(_)) => from_lua_any(v)?,
        (PropKind::Array, Value::UserData(ud)) => {
            let a = ud.borrow::<Array>().ok()?;
            V::Array { addr: a.addr, inner: a.inner }
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

    /// `(iter, nil, 0)` for `ipairs(arr)` / `arr:items()`: a stateless
    /// 1-based walk that re-reads the live element each step.
    fn iterator(self, lua: &Lua) -> mlua::Result<(Function, Value, i64)> {
        let f = lua.create_function(move |lua, (_, i): (Value, i64)| {
            let next = i + 1;
            match self.elem(next) {
                Some(e) => Ok((int(next), unreal::read_prop(e, &self.inner).map(|v| to_lua(lua, &v)).unwrap_or(Value::Nil))),
                None => Ok((Value::Nil, Value::Nil)),
            }
        })?;
        Ok((f, Value::Nil, 0))
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
            Ok(match this.elem(to_i64(i)) {
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
        // for i, v in arr:items() do ... end  — also what `ipairs(arr)` uses:
        // LuaJIT's ipairs only accepts userdata through `__ipairs`.
        m.add_method("items", |lua, this, ()| this.iterator(lua));
        m.add_meta_method(MetaMethod::IPairs, |lua, this, ()| this.iterator(lua));
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
        (V::Int(a), Value::Integer(b)) => *a == to_i64(*b),
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
        // for k, v in map:pairs() do ... end  (snapshot); `pairs(map)` too.
        m.add_method("pairs", |lua, this, ()| this.iterator(lua));
        m.add_meta_method(MetaMethod::Pairs, |lua, this, ()| Ok((this.iterator(lua)?, Value::Nil, Value::Nil)));
    }
}

impl Map {
    /// Snapshot iterator over the entries at call time.
    fn iterator(&self, lua: &Lua) -> mlua::Result<Function> {
        let entries: Vec<(Value, Value)> = unreal::map_entries(self.addr, &self.key, &self.value, self.elem_size)
            .iter()
            .map(|(k, v)| (to_lua(lua, k), to_lua(lua, v)))
            .collect();
        let mut i = 0usize;
        lua.create_function_mut(move |_, ()| {
            let r = entries.get(i).cloned().unwrap_or((Value::Nil, Value::Nil));
            i += 1;
            Ok(r)
        })
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
                // FName semantics: `p.PlayerId` finds the `PlayerID` parameter.
                let s = s.to_string_lossy();
                params.into_iter().find(|p| p.name.eq_ignore_ascii_case(&s))
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
        // obj:prop("Name") -> { class, ptr, kind, offset, size, ext, ext_name, bitmask } or nil
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
                if p.kind == PropKind::Bool {
                    t.set("bitmask", p.bitmask)?;
                }
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

/// How often one call site may report unreadable addresses.
const BAD_READ_REPORT_EVERY: Duration = Duration::from_secs(10);

/// `file:line` for a `debug.getinfo` source and line. Mod scripts are named
/// by full path; the folder and file name say enough next to the mod id.
fn lua_site(source: Option<&str>, line: Option<i64>) -> String {
    let (Some(source), Some(line)) = (source, line) else { return "?".into() };
    let file = match source.strip_prefix('@') {
        Some(path) => {
            let mut tail: Vec<&str> = path.rsplit(['\\', '/']).take(2).collect();
            tail.reverse();
            tail.join("/")
        }
        None => source.trim_start_matches('=').to_string(),
    };
    format!("{file}:{line}")
}

/// Logs that a script read an unreadable address (`call` got nil): the mod
/// and the Lua line, at most once per call site every `BAD_READ_REPORT_EVERY`,
/// with how many it left out in between. The prelude's wrappers pass the
/// site: Rust callbacks run on a Lua thread of their own and cannot see it.
fn report_bad_read(call: &str, addr: i64, site: &str) {
    // "mod|site" -> (last report, reads left out since)
    type Sites = HashMap<String, (Option<Instant>, u32)>;
    static SITES: Mutex<Option<Sites>> = Mutex::new(None);
    let owner = crate::loader::current_mod();
    let now = Instant::now();
    let left_out = {
        let mut sites = SITES.lock().unwrap();
        let (last, left_out) = sites.get_or_insert_default().entry(format!("{owner}|{site}")).or_insert((None, 0));
        if last.is_some_and(|t| now.duration_since(t) < BAD_READ_REPORT_EVERY) {
            *left_out += 1;
            return;
        }
        *last = Some(now);
        std::mem::take(left_out)
    };
    let owner = if owner.is_empty() { String::new() } else { format!("[{owner}] ") };
    let more = if left_out > 0 { format!(" ({left_out} more there since the last report)") } else { String::new() };
    crate::log::line("lua", &format!("{owner}memory.{call}(0x{addr:x}) at {site}: address not readable, returned nil{more}"));
}

fn memory_table(lua: &Lua) -> mlua::Result<Table> {
    let m = lua.create_table()?;
    m.set("base", lua.create_function(|_, ()| Ok(memory::main_module().map(|m| m.base as i64)))?)?;
    m.set("size", lua.create_function(|_, ()| Ok(memory::main_module().map(|m| m.size as i64)))?)?;

    // Script addresses are unchecked: the readers return nil where a plain read
    // would fault (the prelude wraps them to log where that happened).
    macro_rules! reader {
        ($name:literal, $ty:ty, $conv:ty) => {
            m.set(
                $name,
                lua.create_function(|_, addr: i64| {
                    if addr <= 0 {
                        return Ok(None);
                    }
                    Ok(memory::try_read::<$ty>(addr as usize).map(|v| v as $conv))
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

    // memory.readable(addr, len): every byte committed and readable.
    m.set(
        "readable",
        lua.create_function(|_, (addr, len): (i64, Option<usize>)| {
            let (addr, len) = (addr as usize, len.unwrap_or(1).max(1));
            Ok(addr != 0 && (buffers::owns(addr, len) || memory::is_readable(addr, len)))
        })?,
    )?;
    m.set(
        "read_str",
        lua.create_function(|_, (addr, max): (i64, Option<usize>)| {
            Ok((addr > 0).then(|| memory::try_read_cstr(addr as usize, max.unwrap_or(256))).flatten())
        })?,
    )?;
    m.set(
        "read_wstr",
        lua.create_function(|_, (addr, max): (i64, Option<usize>)| {
            Ok((addr > 0).then(|| memory::try_read_wstr(addr as usize, max.unwrap_or(256))).flatten())
        })?,
    )?;
    // memory._report_unreadable(call, addr, source, line): the prelude's read wrappers.
    m.set(
        "_report_unreadable",
        lua.create_function(|_, (call, addr, source, line): (String, i64, Option<String>, Option<i64>)| {
            report_bad_read(&call, addr, &lua_site(source.as_deref(), line));
            Ok(())
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

    // memory.hook(addr, fn, opts?) -> id. fn(original, a1..aN); opts.args = N
    // integers (default 4, max 8) or a list of kinds {"int", "f32", "f64"};
    // opts.ret = "int" (default) | "f32" | "f64"; opts.conv = the x86 calling
    // convention ("cdecl" default, "stdcall", "thiscall", "fastcall").
    m.set(
        "hook",
        lua.create_function(|lua, (addr, cb, opts): (i64, Function, Option<Table>)| {
            let (kinds, ret, conv) = match &opts {
                Some(t) => (parse_kinds(t.get::<Option<Value>>("args")?)?, parse_ret(t.get::<Option<String>>("ret")?)?, parse_conv(t.get::<Option<String>>("conv")?)?),
                None => (vec![hooks::ArgKind::Int; 4], hooks::ArgKind::Int, hooks::Conv::default()),
            };
            let id = hooks::add(addr as usize, &crate::loader::current_mod(), kinds, ret, conv)
                .map_err(|e| mlua::Error::external(format!("memory.hook: {e}")))?;
            lua.set_named_registry_value(&hook_key(id), cb)?;
            Ok(id)
        })?,
    )?;
    m.set(
        "unhook",
        lua.create_function(|lua, id: u64| {
            let found = hooks::remove(id);
            lua.unset_named_registry_value(&hook_key(id))?;
            Ok(found)
        })?,
    )?;
    m.set("hooks", lua.create_function(|lua, ()| address_hooks(lua))?)?;
    // memory.call(addr, ...) -> result; integer/boolean/Object/memory.f32/f64
    // args (max 8). A trailing table { ret = "f32" | "f64", conv = "thiscall" }
    // types the result and names the x86 calling convention.
    m.set(
        "call",
        lua.create_function(|_, (addr, args): (i64, mlua::Variadic<Value>)| {
            let mut args: Vec<Value> = args.into_iter().collect();
            let (ret, conv) = match args.last() {
                Some(Value::Table(t)) => {
                    let r = (parse_ret(t.get::<Option<String>>("ret")?)?, parse_conv(t.get::<Option<String>>("conv")?)?);
                    args.pop();
                    r
                }
                _ => (hooks::ArgKind::Int, hooks::Conv::default()),
            };
            if args.len() > hooks::MAX_ARGS {
                return Err(mlua::Error::external(format!("memory.call: at most {} arguments", hooks::MAX_ARGS)));
            }
            // Floats are the memory.f32/f64 values; everything else a word.
            let kinds: Vec<hooks::ArgKind> = args
                .iter()
                .map(|v| match v {
                    Value::UserData(ud) => ud.borrow::<FloatArg>().map(|f| f.kind).unwrap_or(hooks::ArgKind::Int),
                    _ => hooks::ArgKind::Int,
                })
                .collect();
            let locs = hooks::locate(&kinds, conv).map_err(|e| mlua::Error::external(format!("memory.call: {e}")))?;
            let mut regs = hooks::Regs::default();
            for (i, v) in args.iter().enumerate() {
                if v.is_nil() {
                    continue;
                }
                place_arg(&mut regs, i, locs[i], kinds[i], v).map_err(|e| mlua::Error::external(format!("memory.call: {e}")))?;
            }
            let shape = hooks::Shape::new(&kinds, conv, ret);
            Ok(hooks::call_raw(addr as usize, &regs, &shape).map(|r| match ret {
                hooks::ArgKind::Int => int(r.int as i64),
                k => Value::Number(k.decode(r.fbits)),
            }))
        })?,
    )?;
    // memory.f32(x) / memory.f64(x): a float argument for memory.call / original().
    m.set("f32", lua.create_function(|_, v: f64| Ok(FloatArg { value: v, kind: hooks::ArgKind::F32 }))?)?;
    m.set("f64", lua.create_function(|_, v: f64| Ok(FloatArg { value: v, kind: hooks::ArgKind::F64 }))?)?;

    // Scratch buffers for call arguments: memory.alloc(n) -> addr (zeroed),
    // memory.free(addr), memory.write_str(addr, s) / write_wstr(addr, s) (NUL-terminated).
    m.set(
        "alloc",
        lua.create_function(|_, size: usize| {
            buffers::alloc(size).map(|a| a as i64).ok_or_else(|| mlua::Error::external("memory.alloc: size must be 1..=16 MiB"))
        })?,
    )?;
    m.set(
        "free",
        lua.create_function(|_, addr: i64| Ok(buffers::free(addr as usize)))?,
    )?;
    m.set(
        "write_str",
        lua.create_function(|_, (addr, s): (i64, mlua::String)| {
            let bytes = s.as_bytes();
            Ok(write_bytes(addr as usize, &bytes, 1))
        })?,
    )?;
    m.set(
        "write_wstr",
        lua.create_function(|_, (addr, s): (i64, String)| {
            let units: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            Ok(write_bytes(addr as usize, &units, 2))
        })?,
    )?;
    Ok(m)
}

/// `bytes` then `nul` zero bytes at `addr`; `false` when the destination is
/// neither one of our buffers nor committed memory (so a bad address fails
/// instead of faulting).
fn write_bytes(addr: usize, bytes: &[u8], nul: usize) -> bool {
    let len = bytes.len() + nul;
    if addr == 0 || !(buffers::owns(addr, len) || memory::is_readable(addr, len)) {
        return false;
    }
    // SAFETY: `len` bytes at `addr` are ours or committed, checked just above.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len());
        std::ptr::write_bytes((addr + bytes.len()) as *mut u8, 0, nul);
    }
    true
}

/// Heap buffers handed to Lua by address (`memory.alloc` / `memory.free`).
mod buffers {
    use std::alloc::{alloc_zeroed, dealloc, Layout};
    use std::collections::HashMap;
    use std::sync::Mutex;

    static LIVE: Mutex<Option<HashMap<usize, Layout>>> = Mutex::new(None);
    const MAX: usize = 16 << 20;

    pub fn alloc(size: usize) -> Option<usize> {
        if size == 0 || size > MAX {
            return None;
        }
        let layout = Layout::from_size_align(size, 16).ok()?;
        // SAFETY: non-zero size; freed in `free` with the same layout.
        let p = unsafe { alloc_zeroed(layout) } as usize;
        if p == 0 {
            return None;
        }
        LIVE.lock().unwrap().get_or_insert_with(HashMap::new).insert(p, layout);
        Some(p)
    }

    /// Is `[addr, addr+len)` inside a live buffer?
    pub fn owns(addr: usize, len: usize) -> bool {
        LIVE.lock().unwrap().as_ref().is_some_and(|m| {
            m.iter().any(|(&p, l)| addr >= p && addr + len <= p + l.size())
        })
    }

    pub fn free(addr: usize) -> bool {
        let layout = LIVE.lock().unwrap().as_mut().and_then(|m| m.remove(&addr));
        match layout {
            // SAFETY: `addr` came from `alloc` with this layout and is removed from LIVE first.
            Some(l) => {
                unsafe { dealloc(addr as *mut u8, l) };
                true
            }
            None => false,
        }
    }
}
