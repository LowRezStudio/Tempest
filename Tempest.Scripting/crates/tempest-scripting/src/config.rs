//! Runtime configuration and per-mod config files.
//!
//! The game folder is found from the executable (`<game>/Binaries/Win64/` or `Win32/`)
//! or `TEMPEST_GAME_DIR`. Inside it:
//!
//! * `ChaosGame/Config/tempest.toml`: the runtime settings (written as a
//!   commented template on first run; `TEMPEST_SCRIPT_CONFIG` overrides the path)
//! * `mods/<name>/`: dev mods, loaded like installed ones and hot-reloaded
//! * `ChaosGame/Config/mods/<mod-id>.toml`: one config file per installed
//!   mod (a dev mod's is the `config.toml` in its folder), read with
//!   `tempest.config()` and written with `tempest.save_config()`
//!
//! Environment variables keep working and win over the file:
//! `TEMPEST_SCRIPT_LOG`, `TEMPEST_SCRIPT_CONSOLE=0`, `TEMPEST_SCRIPT_DIR`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const TEMPLATE: &str = r#"# Tempest.Scripting runtime settings. Edit and restart the game.
# Environment variables (TEMPEST_SCRIPT_LOG, TEMPEST_SCRIPT_CONSOLE=0, TEMPEST_SCRIPT_DIR)
# override what is set here.

[runtime]
# Log file. Default: %TEMP%\tempest-scripting.log
# log = "C:/temp/tempest-scripting.log"
# Open the console window (also a Lua REPL).
console = false
# LuaJIT's trace compiler. Turn off to debug a suspected JIT issue.
jit = true
# Log every engine call made from Lua (obj:Func(), tempest.new) before and
# after it runs. Noisy; for finding which call crashes the game.
trace_calls = false

[dev]
# Folder of dev mods, relative to the game folder (or absolute). Every subfolder
# with a scripts/ directory (or .lua files) is a mod; manifest.toml is optional
# and the folder name is the id when it is absent. A dev mod with the same id
# as an installed mod replaces it.
mods = "mods"
# Reload a dev mod when one of its files is saved, and any mod when its config
# file changes.
hot_reload = true
# Also reload a mod when its config file is edited by hand. Files written by
# the mod itself (tempest.save_config) never trigger a reload either way.
reload_on_config = true
# Quiet time after the last change before reloading, in milliseconds.
hot_reload_delay_ms = 300

[mods]
# Where per-mod config files live: <configs>/<mod-id>.toml, relative to the
# game folder (or absolute). tempest.config() reads them, tempest.save_config()
# writes them.
configs = "ChaosGame/Config/mods"

[ui]
# Scale of the in-game UI (fonts and paddings). 1 = Dear ImGui's default.
scale = 1.0
"#;

#[derive(Debug, Clone)]
pub struct Config {
    pub path: Option<PathBuf>,
    pub game_dir: Option<PathBuf>,
    pub log: Option<String>,
    pub console: bool,
    pub jit: bool,
    /// Log every Lua-initiated engine call (crash triage).
    pub trace_calls: bool,
    pub dev_mods: String,
    pub hot_reload: bool,
    pub reload_on_config: bool,
    pub hot_reload_delay_ms: u64,
    pub mod_configs: String,
    pub ui_scale: f32,
    /// What happened while loading, logged once the log exists.
    pub notes: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            path: None,
            game_dir: None,
            log: None,
            console: true,
            jit: true,
            trace_calls: false,
            dev_mods: "mods".into(),
            hot_reload: true,
            reload_on_config: true,
            hot_reload_delay_ms: 300,
            mod_configs: "ChaosGame/Config/mods".into(),
            ui_scale: 1.0,
            notes: Vec::new(),
        }
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// The runtime configuration, loaded on first use.
pub fn get() -> &'static Config {
    CONFIG.get_or_init(load)
}

/// `TEMPEST_GAME_DIR`, else two levels above the executable
/// (`<game>/Binaries/Win64/Paladins.exe` or `Win32`) when that looks like the game.
pub fn game_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("TEMPEST_GAME_DIR") {
        if !d.is_empty() {
            return Some(PathBuf::from(d));
        }
    }
    let exe = std::env::current_exe().ok()?;
    let root = exe.parent()?.parent()?.parent()?.to_path_buf();
    (root.join("ChaosGame").is_dir() || root.join("Binaries").is_dir()).then_some(root)
}

fn resolve(game: Option<&Path>, p: &str) -> Option<PathBuf> {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        return Some(path);
    }
    game.map(|g| g.join(path))
}

/// `<game>/mods` (or whatever `[dev] mods` says).
pub fn dev_mods_dir() -> Option<PathBuf> {
    let c = get();
    resolve(c.game_dir.as_deref(), &c.dev_mods)
}

/// `<game>/ChaosGame/Config/mods` (or whatever `[mods] configs` says).
pub fn mod_configs_dir() -> Option<PathBuf> {
    let c = get();
    resolve(c.game_dir.as_deref(), &c.mod_configs)
}

/// `<configs>/<mod-id>.toml`.
pub fn mod_config_path(id: &str) -> Option<PathBuf> {
    mod_configs_dir().map(|d| d.join(format!("{id}.toml")))
}

fn load() -> Config {
    let mut c = Config {
        game_dir: game_dir(),
        ..Config::default()
    };
    let path = match std::env::var("TEMPEST_SCRIPT_CONFIG") {
        Ok(p) if !p.is_empty() => Some(PathBuf::from(p)),
        _ => c
            .game_dir
            .as_ref()
            .map(|g| g.join("ChaosGame").join("Config").join("tempest.toml")),
    };
    let Some(path) = path else {
        c.notes.push("game folder unknown (no TEMPEST_GAME_DIR, executable not under <game>/Binaries/<platform>); using defaults".into());
        return c;
    };
    c.path = Some(path.clone());
    match std::fs::read_to_string(&path) {
        Ok(text) => match text.parse::<toml::Table>() {
            Ok(t) => apply(&mut c, &t),
            Err(e) => c
                .notes
                .push(format!("{}: {e}; using defaults", path.display())),
        },
        Err(_) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match std::fs::write(&path, TEMPLATE) {
                Ok(()) => c
                    .notes
                    .push(format!("wrote default settings to {}", path.display())),
                Err(e) => c
                    .notes
                    .push(format!("cannot write {}: {e}", path.display())),
            }
        }
    }
    c
}

fn apply(c: &mut Config, t: &toml::Table) {
    let section = |name: &str| t.get(name).and_then(|v| v.as_table());
    if let Some(r) = section("runtime") {
        if let Some(s) = r
            .get("log")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            c.log = Some(s.to_string());
        }
        if let Some(b) = r.get("console").and_then(|v| v.as_bool()) {
            c.console = b;
        }
        if let Some(b) = r.get("jit").and_then(|v| v.as_bool()) {
            c.jit = b;
        }
        if let Some(b) = r.get("trace_calls").and_then(|v| v.as_bool()) {
            c.trace_calls = b;
        }
    }
    if let Some(d) = section("dev") {
        if let Some(s) = d.get("mods").and_then(|v| v.as_str()) {
            c.dev_mods = s.to_string();
        }
        if let Some(b) = d.get("hot_reload").and_then(|v| v.as_bool()) {
            c.hot_reload = b;
        }
        if let Some(b) = d.get("reload_on_config").and_then(|v| v.as_bool()) {
            c.reload_on_config = b;
        }
        if let Some(n) = d.get("hot_reload_delay_ms").and_then(|v| v.as_integer()) {
            c.hot_reload_delay_ms = n.max(0) as u64;
        }
    }
    if let Some(m) = section("mods") {
        if let Some(s) = m.get("configs").and_then(|v| v.as_str()) {
            c.mod_configs = s.to_string();
        }
    }
    if let Some(u) = section("ui") {
        if let Some(f) = u
            .get("scale")
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
        {
            if f > 0.0 {
                c.ui_scale = f as f32;
            }
        }
    }
}

/// Editor support for the dev folder: copy the runtime's `types/*.d.lua`
/// next to the dev mods and drop a `.luarc.json` there, once. Returns the
/// files written.
pub fn seed_dev_types(runtime_types: &Path) -> Vec<PathBuf> {
    let mut written = Vec::new();
    let Some(dev) = dev_mods_dir().filter(|d| d.is_dir()) else {
        return written;
    };
    let types = dev.join("types");
    if let Ok(entries) = std::fs::read_dir(runtime_types) {
        for entry in entries.flatten() {
            let src = entry.path();
            if src.extension().and_then(|e| e.to_str()) != Some("lua") {
                continue;
            }
            let dst = types.join(entry.file_name());
            let fresh = match (std::fs::metadata(&src), std::fs::metadata(&dst)) {
                (Ok(s), Ok(d)) => s.len() != d.len() || s.modified().ok() > d.modified().ok(),
                (Ok(_), Err(_)) => true,
                _ => false,
            };
            if fresh && std::fs::create_dir_all(&types).is_ok() && std::fs::copy(&src, &dst).is_ok()
            {
                written.push(dst);
            }
        }
    }
    let luarc = dev.join(".luarc.json");
    if !luarc.exists() && types.is_dir() {
        let text = "{\n  \"runtime.version\": \"LuaJIT\",\n  \"workspace.library\": [\"types\"],\n  \"workspace.checkThirdParty\": false,\n  \"diagnostics.globals\": [\"MOD\", \"shared\"]\n}\n";
        if std::fs::write(&luarc, text).is_ok() {
            written.push(luarc);
        }
    }
    written
}

// ---------------------------------------------------------------------------
// Per-mod config files
// ---------------------------------------------------------------------------

/// Deep-merge `over` into `base` (tables merge, everything else replaces).
pub fn merge(base: &mut toml::Table, over: &toml::Table) {
    for (k, v) in over {
        match (base.get_mut(k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o),
            _ => {
                base.insert(k.clone(), v.clone());
            }
        }
    }
}

fn read_table(path: &Path) -> Result<Option<toml::Table>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse::<toml::Table>()
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Where a mod's editable config lives: a dev mod's own `config.toml`
/// (everything about a dev mod stays in its folder), else
/// `<configs>/<id>.toml`.
pub fn config_file(m: &crate::loader::Mod) -> Option<PathBuf> {
    if m.dev {
        Some(m.root.join("config.toml"))
    } else {
        mod_config_path(&m.id)
    }
}

/// The effective config of a mod: `defaults`, then the mod's shipped
/// `config.toml` (next to its `scripts/`), then, for installed mods, the
/// user's file. The editable file (`config_file`) is created from the merged
/// values when it does not exist, so there is always something to edit.
pub fn load_mod_config(
    m: &crate::loader::Mod,
    defaults: toml::Table,
) -> Result<(toml::Table, Option<PathBuf>), String> {
    let mut merged = defaults;
    if let Some(shipped) = read_table(&m.root.join("config.toml"))? {
        merge(&mut merged, &shipped);
    }
    let path = config_file(m);
    if let Some(path) = &path {
        match read_table(path)? {
            Some(user) => merge(&mut merged, &user),
            None if !merged.is_empty() => {
                write_table(path, &merged)?;
                crate::log::line(
                    "config",
                    &format!("[{}] wrote defaults to {}", m.id, path.display()),
                );
            }
            None => {}
        }
    }
    Ok((merged, path))
}

pub fn write_table(path: &Path, table: &toml::Table) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = toml::to_string_pretty(table).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::watch::expect(path);
    Ok(())
}

/// Save a mod's config (the whole table replaces the file).
pub fn save_mod_config(m: &crate::loader::Mod, table: &toml::Table) -> Result<PathBuf, String> {
    let path = config_file(m).ok_or("game folder unknown; no config directory")?;
    write_table(&path, table)?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Lua <-> TOML
// ---------------------------------------------------------------------------

pub fn to_lua(lua: &mlua::Lua, v: &toml::Value) -> mlua::Result<mlua::Value> {
    use mlua::Value as L;
    Ok(match v {
        toml::Value::String(s) => L::String(lua.create_string(s)?),
        // `Integer` is i32 on the Win32 client; wider values become Numbers.
        toml::Value::Integer(i) => mlua::IntoLua::into_lua(*i, lua)?,
        toml::Value::Float(f) => L::Number(*f),
        toml::Value::Boolean(b) => L::Boolean(*b),
        toml::Value::Datetime(d) => L::String(lua.create_string(d.to_string())?),
        toml::Value::Array(a) => {
            let t = lua.create_table()?;
            for (i, x) in a.iter().enumerate() {
                t.set(i + 1, to_lua(lua, x)?)?;
            }
            L::Table(t)
        }
        toml::Value::Table(m) => {
            let t = lua.create_table()?;
            for (k, x) in m {
                t.set(k.as_str(), to_lua(lua, x)?)?;
            }
            L::Table(t)
        }
    })
}

pub fn from_lua(v: &mlua::Value) -> Result<toml::Value, String> {
    use mlua::Value as L;
    Ok(match v {
        L::Boolean(b) => toml::Value::Boolean(*b),
        L::Integer(i) => toml::Value::Integer(crate::lua_api::to_i64(*i)),
        L::Number(n) => toml::Value::Float(*n),
        L::String(s) => toml::Value::String(s.to_string_lossy().to_string()),
        L::Table(t) => {
            let len = t.raw_len();
            let mut keys = 0usize;
            let mut is_array = true;
            for pair in t.clone().pairs::<L, L>() {
                let (k, _) = pair.map_err(|e| e.to_string())?;
                keys += 1;
                match k {
                    L::Integer(i) if i >= 1 && i as usize <= len => {}
                    _ => is_array = false,
                }
            }
            if is_array && keys == len {
                let mut out = Vec::with_capacity(len);
                for i in 1..=len {
                    out.push(from_lua(&t.get::<L>(i).map_err(|e| e.to_string())?)?);
                }
                toml::Value::Array(out)
            } else {
                let mut out = toml::Table::new();
                for pair in t.clone().pairs::<L, L>() {
                    let (k, x) = pair.map_err(|e| e.to_string())?;
                    let key = match k {
                        L::String(s) => s.to_string_lossy().to_string(),
                        L::Integer(i) => i.to_string(),
                        L::Number(n) => n.to_string(),
                        other => {
                            return Err(format!(
                                "config key must be a string or number, not {}",
                                other.type_name()
                            ))
                        }
                    };
                    if x.is_nil() {
                        continue;
                    }
                    out.insert(key, from_lua(&x)?);
                }
                toml::Value::Table(out)
            }
        }
        L::Nil => return Err("nil cannot be stored in a config".into()),
        other => {
            return Err(format!(
                "{} cannot be stored in a config",
                other.type_name()
            ))
        }
    })
}

pub fn table_from_lua(v: &mlua::Value) -> Result<toml::Table, String> {
    match from_lua(v)? {
        toml::Value::Table(t) => Ok(t),
        _ => Err("config must be a table".into()),
    }
}
