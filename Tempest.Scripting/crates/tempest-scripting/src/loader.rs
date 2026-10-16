//! Mod discovery, sandboxed script loading, and reload.
//!
//! Where scripts come from (first match wins for the single-directory cases):
//!
//! 1. `$TEMPEST_SCRIPT_DIR` — one mod, id `local`.
//! 2. `<dll dir>/scripts` — drop-in next to the DLL, id `local`.
//! 3. The V2 layout: this DLL lives at `<game>/.tempest/v2/mods/<id>/dlls/`,
//!    so every sibling `<game>/.tempest/v2/mods/*/scripts/` is a mod. Mods
//!    marked disabled in `<game>/.tempest/mods.json` are skipped (the DLL's
//!    own mod is always loaded).
//! 4. Dev mods: every folder of `<game>/mods/` (`[dev] mods` in the runtime
//!    settings) with a `scripts/` directory or `.lua` files, manifest
//!    optional. They are hot-reloaded on save and a dev mod with the id of
//!    an installed mod replaces it.
//!
//! Each mod runs in its own environment: globals it sets stay private,
//! reads fall through to the real globals (`tempest`, `memory`, `shared`,
//! the standard library). `require("a.b")` resolves `scripts/a/b.lua` inside
//! the same mod and shares that environment; names with no such file go to
//! the real `require` (`ffi`, `bit`, `jit`, ...). Scripts run alphabetically,
//! mods alphabetically by id.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mlua::{Lua, Table, Value};

#[derive(Debug, Clone)]
pub struct Mod {
    pub id: String,
    pub scripts: PathBuf,
    /// The mod folder (manifest.toml, config.toml, scripts/).
    pub root: PathBuf,
    /// Ids this mod wants loaded before it (`[scripting] after` in manifest.toml).
    pub after: Vec<String>,
    /// From the dev folder (hot-reloaded, overrides an installed mod of the same id).
    pub dev: bool,
}

impl Mod {
    fn new(id: impl Into<String>, scripts: PathBuf) -> Mod {
        let root = scripts.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| scripts.clone());
        Mod { id: id.into(), scripts, root, after: Vec::new(), dev: false }
    }
}

#[derive(serde::Deserialize, Default)]
struct Manifest {
    #[serde(default)]
    r#mod: ModSection,
    #[serde(default)]
    scripting: ScriptingSection,
}

#[derive(serde::Deserialize, Default)]
struct ModSection {
    #[serde(default)]
    id: String,
}

#[derive(serde::Deserialize, Default)]
struct ScriptingSection {
    #[serde(default)]
    after: Vec<String>,
}

/// `(id, after)` from the folder's manifest.toml; empty when absent or broken.
fn manifest(mod_root: &Path) -> (String, Vec<String>) {
    let Ok(text) = std::fs::read_to_string(mod_root.join("manifest.toml")) else { return (String::new(), Vec::new()) };
    match toml::from_str::<Manifest>(&text) {
        Ok(m) => (m.r#mod.id, m.scripting.after.into_iter().map(|s| s.to_ascii_lowercase()).collect()),
        Err(e) => {
            crate::log::line("loader", &format!("{}: manifest.toml: {e}", mod_root.display()));
            (String::new(), Vec::new())
        }
    }
}

/// Dev mods from `<game>/mods` (or `[dev] mods`): `<dir>/<folder>/scripts/`,
/// or the folder itself when it holds `.lua` files directly.
fn dev_mods() -> Vec<Mod> {
    let mut out = Vec::new();
    let Some(dir) = crate::config::dev_mods_dir() else { return out };
    let Ok(entries) = std::fs::read_dir(&dir) else { return out };
    for entry in entries.flatten() {
        let root = entry.path();
        let folder = entry.file_name().to_string_lossy().to_string();
        // `types/` holds the editor stubs the runtime drops here; dot-folders are editors' own.
        if !root.is_dir() || folder == "types" || folder.starts_with('.') {
            continue;
        }
        let scripts = if root.join("scripts").is_dir() {
            root.join("scripts")
        } else if list_scripts(&root).is_empty() {
            continue;
        } else {
            root.clone()
        };
        let (id, after) = manifest(&root);
        let id = if id.is_empty() { folder } else { id };
        out.push(Mod { id, scripts, root, after, dev: true });
    }
    out
}

/// Alphabetical by id, then moved so every `after` dependency precedes its
/// dependant (stable topological order; cycles are logged and broken).
fn order_mods(mut mods: Vec<Mod>) -> Vec<Mod> {
    mods.sort_by_key(|m| m.id.to_ascii_lowercase());
    let ids: HashSet<String> = mods.iter().map(|m| m.id.to_ascii_lowercase()).collect();
    for m in &mods {
        for dep in &m.after {
            if !ids.contains(dep) {
                crate::log::line("loader", &format!("mod {}: `after = {dep}` is not installed/enabled", m.id));
            }
        }
    }
    let mut out: Vec<Mod> = Vec::new();
    let mut remaining = mods;
    while !remaining.is_empty() {
        let placed: HashSet<String> = out.iter().map(|m| m.id.to_ascii_lowercase()).collect();
        let pos = remaining.iter().position(|m| {
            m.after.iter().all(|d| placed.contains(d) || !ids.contains(d))
        });
        match pos {
            Some(i) => out.push(remaining.remove(i)),
            None => {
                let m = remaining.remove(0);
                crate::log::line("loader", &format!("mod {}: dependency cycle, loading it anyway", m.id));
                out.push(m);
            }
        }
    }
    out
}

/// Mod whose script (or handler) is currently executing; `""` outside.
static CURRENT: Mutex<String> = Mutex::new(String::new());

pub fn current_mod() -> String {
    CURRENT.lock().unwrap().clone()
}

/// Run `f` with the current mod set to `id`, restoring the previous value.
pub fn with_mod<R>(id: &str, f: impl FnOnce() -> R) -> R {
    let prev = std::mem::replace(&mut *CURRENT.lock().unwrap(), id.to_string());
    let r = f();
    *CURRENT.lock().unwrap() = prev;
    r
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
#[allow(non_snake_case)]
struct ModRecord {
    #[serde(alias = "id")]
    Id: String,
    #[serde(alias = "enabled", default = "yes")]
    Enabled: bool,
    #[serde(alias = "kind", default)]
    Kind: String,
}

fn yes() -> bool {
    true
}

/// Ids (lowercase) of enabled V2 mods, or `None` when the file is absent
/// or unreadable (then every mod folder counts).
fn enabled_ids(mods_json: &Path) -> Option<HashSet<String>> {
    let text = std::fs::read_to_string(mods_json).ok()?;
    let records: Vec<ModRecord> = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            crate::log::line("loader", &format!("{}: {e}; loading every mod folder", mods_json.display()));
            return None;
        }
    };
    Some(
        records
            .into_iter()
            .filter(|r| r.Enabled && r.Kind.eq_ignore_ascii_case("v2"))
            .map(|r| r.Id.to_ascii_lowercase())
            .collect(),
    )
}

pub fn mods() -> Vec<Mod> {
    if let Ok(dir) = std::env::var("TEMPEST_SCRIPT_DIR") {
        return vec![Mod::new("local", PathBuf::from(dir))];
    }
    let mut out = Vec::new();
    if let Some(dll) = dll_dir() {
        let local = dll.join("scripts");
        if local.is_dir() {
            out.push(Mod::new("local", local));
        }
        if let Some((mod_root, mods_root)) = dll.parent().and_then(|m| m.parent().map(|r| (m, r))) {
            let own = mod_root.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase());
            let enabled = mods_root
                .parent()
                .and_then(|v2| v2.parent())
                .and_then(|tempest| enabled_ids(&tempest.join("mods.json")));
            if let Ok(entries) = std::fs::read_dir(mods_root) {
                for entry in entries.flatten() {
                    let dir = entry.path();
                    let scripts = dir.join("scripts");
                    if !scripts.is_dir() {
                        continue;
                    }
                    let id = entry.file_name().to_string_lossy().to_string();
                    let lower = id.to_ascii_lowercase();
                    let is_own = own.as_deref() == Some(lower.as_str());
                    if !is_own && enabled.as_ref().is_some_and(|set| !set.contains(&lower)) {
                        crate::log::line("loader", &format!("mod {id}: disabled, skipped"));
                        continue;
                    }
                    let (_, after) = manifest(&dir);
                    out.push(Mod { id, scripts, root: dir, after, dev: false });
                }
            }
        }
    }
    for dev in dev_mods() {
        // A dev mod with an installed mod's id replaces it (the summary line says so).
        out.retain(|m| !m.id.eq_ignore_ascii_case(&dev.id));
        out.push(dev);
    }
    if out.is_empty() {
        out.push(Mod::new("local", PathBuf::from("scripts")));
    }
    order_mods(out)
}

/// The loaded mod with this id, if any.
pub fn find(id: &str) -> Option<Mod> {
    mods().into_iter().find(|m| m.id.eq_ignore_ascii_case(id))
}

fn list_scripts(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "lua"))
        // `*.d.lua` are LuaLS stubs, never scripts.
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(".d.lua")))
        .collect();
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Sandboxed execution
// ---------------------------------------------------------------------------

/// A fresh environment for `m`: private globals, `MOD`, and a `require`
/// scoped to the mod's `scripts/` directory (falling back to the global one
/// for built-in modules such as `ffi` and `bit`).
fn mod_env(lua: &Lua, m: &Mod) -> mlua::Result<Table> {
    let env = lua.create_table()?;
    let mt = lua.create_table()?;
    mt.set("__index", lua.globals())?;
    env.set_metatable(Some(mt));
    env.set("MOD", m.id.as_str())?;
    env.set("_ENV_MOD", m.id.as_str())?;

    let loaded = lua.create_table()?;
    let dir = m.scripts.clone();
    let id = m.id.clone();
    let env_for_require = env.clone();
    let global_require: Option<mlua::Function> = lua.globals().get("require").ok();
    env.set(
        "require",
        lua.create_function(move |lua, name: String| {
            if let Ok(v) = loaded.get::<Value>(name.as_str()) {
                if !v.is_nil() {
                    return Ok(v);
                }
            }
            let path = dir.join(format!("{}.lua", name.replace('.', "/")));
            if !path.is_file() {
                if let Some(req) = &global_require {
                    return req.call::<Value>(name.as_str());
                }
            }
            let src = std::fs::read_to_string(&path)
                .map_err(|_| mlua::Error::external(format!("module '{name}' not found ({})", path.display())))?;
            let result: Value = with_mod(&id, || {
                lua.load(&src)
                    .set_name(format!("@{}", path.display()))
                    .set_environment(env_for_require.clone())
                    .call(())
            })?;
            let v = if result.is_nil() { Value::Boolean(true) } else { result };
            loaded.set(name.as_str(), v.clone())?;
            Ok(v)
        })?,
    )?;
    Ok(env)
}

/// Run every script of one mod in a fresh environment. Errors are logged
/// per file and attributed to the mod.
pub fn run_mod(lua: &Lua, m: &Mod) {
    let scripts = list_scripts(&m.scripts);
    crate::log::line(
        "loader",
        &format!("mod {}{}: {} script(s) in {}", m.id, if m.dev { " (dev)" } else { "" }, scripts.len(), m.scripts.display()),
    );
    let env = match mod_env(lua, m) {
        Ok(e) => e,
        Err(e) => {
            crate::log::line("loader", &format!("mod {}: cannot create environment: {e}", m.id));
            return;
        }
    };
    for path in scripts {
        let name = path.display().to_string();
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                crate::log::line("loader", &format!("[{}] {name}: cannot read: {e}", m.id));
                continue;
            }
        };
        let r = with_mod(&m.id, || {
            lua.load(&src)
                .set_name(format!("@{name}"))
                .set_environment(env.clone())
                .exec()
        });
        match r {
            Ok(()) => crate::log::line("loader", &format!("[{}] ran {name}", m.id)),
            Err(e) => crate::log::line("loader", &format!("[{}] {name}: {e}", m.id)),
        }
    }
}

/// Run every mod. `shared` starts empty.
pub fn run_all(lua: &Lua) {
    if let Ok(t) = lua.create_table() {
        let _ = lua.globals().set("shared", t);
    }
    // dllimport functions of the loaded script packages call into Lua from
    // now on (zero results until a tempest.native handler claims them).
    crate::unreal::bind_dll_imports();
    let mods = mods();
    crate::log::line(
        "loader",
        &format!(
            "{} mod(s): {}",
            mods.len(),
            mods.iter().map(|m| if m.dev { format!("{} (dev)", m.id) } else { m.id.clone() }).collect::<Vec<_>>().join(", ")
        ),
    );
    for m in &mods {
        run_mod(lua, m);
    }
}

/// Reload one mod (its subscriptions, address hooks and commands go first)
/// or, with `None`, everything (prelude included, `shared` reset).
pub fn reload(lua: &Lua, only: Option<&str>) {
    match only {
        Some(id) => {
            let Some(m) = mods().into_iter().find(|m| m.id.eq_ignore_ascii_case(id)) else {
                crate::log::line("loader", &format!("reload: no mod `{id}`"));
                return;
            };
            for sub in crate::unreal::clear_subscriptions_of(&m.id) {
                let _ = lua.unset_named_registry_value(&format!("tempest.sub.{sub}"));
            }
            for hook in crate::hooks::clear_of(&m.id) {
                let _ = lua.unset_named_registry_value(&format!("tempest.hook.{hook}"));
            }
            if let Ok(forget) = lua.globals().get::<Table>("tempest").and_then(|t| t.get::<mlua::Function>("_forget")) {
                let _ = forget.call::<()>(m.id.as_str());
            }
            crate::log::line("loader", &format!("reload mod {}", m.id));
            run_mod(lua, &m);
        }
        None => {
            for id in crate::unreal::clear_subscriptions() {
                let _ = lua.unset_named_registry_value(&format!("tempest.sub.{id}"));
            }
            for id in crate::hooks::clear_all() {
                let _ = lua.unset_named_registry_value(&format!("tempest.hook.{id}"));
            }
            crate::log::line("loader", "reload all");
            if let Err(e) = crate::lua_api::run_prelude(lua) {
                crate::log::line("loader", &format!("prelude failed: {e}"));
            }
            run_all(lua);
        }
    }
}

/// Directory containing this DLL (Windows); `None` on the host build.
pub fn dll_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    unsafe {
        const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;
        let mut module: *mut std::ffi::c_void = std::ptr::null_mut();
        let anchor = dll_dir as *const () as *const u16;
        if GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, anchor, &mut module) == 0 {
            return None;
        }
        let mut buf = [0u16; 1024];
        let n = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize;
        if n == 0 {
            return None;
        }
        PathBuf::from(String::from_utf16_lossy(&buf[..n]))
            .parent()
            .map(|p| p.to_path_buf())
    }
    #[cfg(not(windows))]
    None
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut *mut std::ffi::c_void) -> i32;
    fn GetModuleFileNameW(module: *mut std::ffi::c_void, buf: *mut u16, len: u32) -> u32;
}
