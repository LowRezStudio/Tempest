//! Mod discovery, sandboxed script loading, and reload.
//!
//! Where scripts come from (first match wins for the single-directory cases):
//!
//! 1. `$TEMPEST_SCRIPT_DIR` — one mod, id `local`.
//! 2. `<dll dir>/scripts` — drop-in next to the DLL, id `local`.
//! 3. The V2 layout: `<game>/.tempest/v2/mods/*/scripts/` (found next to
//!    this DLL, or from the game folder). Mods marked disabled in the CLI's
//!    `<game>/.tempest/mods/mods.json` are skipped (the DLL's own mod is
//!    always loaded), and so are mods failing integrity: the CLI verifies a
//!    mod's signed CHECKSUMS against its files at launch, and this runtime
//!    re-hashes the scripts it is about to run. `installed()` lists every
//!    record of that registry, Lua or not.
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

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mlua::{Lua, Table, Value};
use sha2::{Digest, Sha256};

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

/// One entry of the CLI's `mods.json` (PascalCase, written by Tempest.CLI).
#[derive(serde::Deserialize, Default)]
#[allow(non_snake_case)]
struct ModRecord {
    #[serde(alias = "id")]
    Id: String,
    #[serde(alias = "name", default)]
    Name: String,
    #[serde(alias = "version", default)]
    Version: String,
    #[serde(alias = "author", default)]
    Author: String,
    #[serde(alias = "authors", default)]
    Authors: Vec<ModAuthor>,
    #[serde(alias = "enabled", default = "yes")]
    Enabled: bool,
    #[serde(alias = "kind", default)]
    Kind: String,
    #[serde(alias = "installedFiles", default)]
    InstalledFiles: Vec<String>,
    #[serde(alias = "signer", default)]
    Signer: String,
    /// The CLI re-verified this mod's files against its signed CHECKSUMS for this launch.
    #[serde(alias = "verified", default)]
    Verified: bool,
    /// SHA-256 of the mod folder's CHECKSUMS content at verification time; pins it here.
    #[serde(alias = "manifestHash", default)]
    ManifestHash: String,
}

#[derive(serde::Deserialize, Default)]
#[allow(non_snake_case)]
struct ModAuthor {
    #[serde(alias = "name", default)]
    Name: String,
}

fn yes() -> bool {
    true
}

/// `<game>/.tempest`: next to the installed mods when this DLL is one of
/// them, else from the game folder.
fn tempest_dir() -> Option<PathBuf> {
    if let Some(m) = mod_dir() {
        // <game>/.tempest/v2/mods/<id>
        if let Some(t) = m.ancestors().nth(3) {
            if t.file_name().is_some_and(|n| n == ".tempest") {
                return Some(t.to_path_buf());
            }
        }
    }
    crate::config::game_dir().map(|g| g.join(".tempest"))
}

/// The folder of the mod this DLL ships in: the DLL lives in `<mod>/dlls/`
/// (legacy), `<mod>/dlls/64/` or `<mod>/dlls/32/` (one per client), so walk
/// up past those. A DLL loaded from anywhere else counts its parent as the mod.
pub fn mod_dir() -> Option<PathBuf> {
    let dir = dll_dir()?;
    let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase());
    let dlls = match name(&dir).as_deref() {
        Some("dlls") => dir.clone(),
        Some("32" | "64") if dir.parent().and_then(|p| name(p)).as_deref() == Some("dlls") => dir.parent()?.to_path_buf(),
        _ => dir.clone(),
    };
    dlls.parent().map(|p| p.to_path_buf())
}

/// The CLI's mod registry (`.tempest/mods/mods.json`; older layouts kept it
/// one level up).
fn mods_json_path() -> Option<PathBuf> {
    let t = tempest_dir()?;
    let new = t.join("mods").join("mods.json");
    if new.is_file() {
        return Some(new);
    }
    let old = t.join("mods.json");
    old.is_file().then_some(old).or(Some(new))
}

fn read_records() -> Option<Vec<ModRecord>> {
    let path = mods_json_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&text) {
        Ok(r) => Some(r),
        Err(e) => {
            crate::log::line("loader", &format!("{}: {e}", path.display()));
            None
        }
    }
}

/// Registry records by lowercase id, plus the enabled-id set. When the registry
/// is absent or unreadable the enabled set is `None` and every mod folder counts
/// (previous behavior); a present CHECKSUMS is still enforced down there.
fn registry() -> (Option<HashSet<String>>, HashMap<String, ModRecord>) {
    match read_records() {
        None => (None, HashMap::new()),
        Some(records) => {
            let mut enabled = HashSet::new();
            let mut by_id = HashMap::new();
            for r in records {
                let lower = r.Id.to_ascii_lowercase();
                if r.Enabled && r.Kind.eq_ignore_ascii_case("v2") {
                    enabled.insert(lower.clone());
                }
                by_id.insert(lower, r);
            }
            (Some(enabled), by_id)
        }
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

/// `<hash> <path>` lines of a package CHECKSUMS; paths keep `/` separators.
fn parse_checksums(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (hash, path) = line.split_once(' ')?;
            let path = path.trim();
            let path = path.strip_prefix('*').unwrap_or(path).trim();
            if hash.len() != 64 || path.is_empty() {
                return None;
            }
            Some((path.to_string(), hash.to_ascii_lowercase()))
        })
        .collect()
}

/// Hashes the file at `path`; `None` when it cannot be read.
fn hash_file(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    Some(sha256_hex(&data))
}

/// `.lua` files under `dir`, recursively, `/`-separated relative to `root`.
/// Same skip rule as script loading: `*.d.lua` editor stubs are never scripts.
fn lua_files(root: &Path, dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if !name.ends_with(".lua") || name.ends_with(".d.lua") {
                    continue;
                }
                if let Ok(rel) = path.strip_prefix(root) {
                    if let Some(rel) = rel.to_str() {
                        out.push(rel.to_string());
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// Shipped scripts must still match the package's CHECKSUMS, and mods the CLI
/// verifies at launch (a `signer` in the registry) must have passed that check.
/// An installed mod without CHECKSUMS can only ever be scriptless; one that
/// gained scripts since is tampering, not a dev folder (`<game>/mods`).
/// Returns the reason when the mod's scripts must not be loaded.
fn verify_installed_scripts(root: &Path, record: Option<&ModRecord>) -> Result<(), String> {
    if let Some(record) = record {
        if !record.Signer.is_empty() && !record.Verified {
            return Err(
                "the launcher did not verify this mod's files for this launch (it failed verification or was re-enabled); install the mod again if you meant to change it"
                    .to_string(),
            );
        }
    }

    let checksums = match std::fs::read_to_string(root.join("CHECKSUMS")) {
        Ok(checksums) => checksums,
        Err(_) => {
            return match record.map(|r| r.Signer.is_empty()).unwrap_or(true) {
                // No registry data, or an unsigned install: a script here is an added file.
                true => {
                    if lua_files(root, &root.join("scripts")).is_empty() {
                        Ok(())
                    } else {
                        Err("there is no CHECKSUMS, yet the mod contains scripts; the mod was modified after install".to_string())
                    }
                }
                false => Err("CHECKSUMS is missing from the mod folder; the mod was modified after install".to_string()),
            };
        }
    };

    if let Some(record) = record {
        if !record.ManifestHash.is_empty() && sha256_hex(checksums.as_bytes()) != record.ManifestHash {
            return Err("CHECKSUMS was regenerated after install; the mod was modified after install".to_string());
        }
    }

    let mut map = parse_checksums(&checksums);
    for relative in lua_files(root, &root.join("scripts")) {
        match map.remove(&relative) {
            None => return Err(format!("{relative} was added after install; the mod was modified after install")),
            Some(expected) => match hash_file(&root.join(&relative)) {
                Some(actual) if actual == expected => {}
                Some(_) => return Err(format!("{relative} was modified after install")),
                None => return Err(format!("{relative} cannot be read: the mod was modified after install")),
            },
        }
    }
    Ok(())
}

/// Every mod the launcher installed, Lua or not, plus the dev folder.
#[derive(Debug, Clone)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
    /// `"V2"`, `"NativePackage"`, `"Asset"`, `"Voice"` (CLI kinds) or `"dev"`.
    pub kind: String,
    pub enabled: bool,
    /// Ships `scripts/` (a Lua mod).
    pub lua: bool,
    /// Ships `dlls/`.
    pub dll: bool,
    /// Game files it installs (content: packages, inis, assets).
    pub files: usize,
    pub dev: bool,
    /// Its scripts are running (enabled Lua mod, or dev).
    pub loaded: bool,
    pub path: Option<PathBuf>,
}

pub fn installed() -> Vec<Installed> {
    let loaded: HashSet<String> = mods().into_iter().map(|m| m.id.to_ascii_lowercase()).collect();
    let v2_root = tempest_dir().map(|t| t.join("v2").join("mods"));
    let mut out = Vec::new();
    for r in read_records().unwrap_or_default() {
        let is_v2 = r.Kind.eq_ignore_ascii_case("v2");
        let path = if is_v2 { v2_root.as_ref().map(|root| root.join(&r.Id)).filter(|p| p.is_dir()) } else { None };
        let lua = path.as_ref().is_some_and(|p| p.join("scripts").is_dir());
        let dll = path.as_ref().is_some_and(|p| {
            std::fs::read_dir(p.join("dlls")).map(|d| d.flatten().any(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("dll")))).unwrap_or(false)
        });
        let mut authors: Vec<String> = r.Authors.iter().map(|a| a.Name.clone()).filter(|n| !n.is_empty()).collect();
        if authors.is_empty() && !r.Author.is_empty() && r.Author != "Unknown" {
            authors.push(r.Author.clone());
        }
        let lower = r.Id.to_ascii_lowercase();
        out.push(Installed {
            loaded: loaded.contains(&lower),
            id: r.Id,
            name: r.Name,
            version: r.Version,
            authors,
            kind: if r.Kind.is_empty() { "unknown".into() } else { r.Kind },
            enabled: r.Enabled,
            lua,
            dll,
            files: r.InstalledFiles.len(),
            dev: false,
            path,
        });
    }
    for m in mods().into_iter().filter(|m| m.dev) {
        // A dev mod replacing an installed id: mark the record instead of listing twice.
        if let Some(rec) = out.iter_mut().find(|r| r.id.eq_ignore_ascii_case(&m.id)) {
            rec.dev = true;
            rec.loaded = true;
            rec.path = Some(m.root.clone());
            continue;
        }
        out.push(Installed {
            id: m.id.clone(),
            name: m.id.clone(),
            version: String::new(),
            authors: Vec::new(),
            kind: "dev".into(),
            enabled: true,
            lua: true,
            dll: false,
            files: 0,
            dev: true,
            loaded: true,
            path: Some(m.root.clone()),
        });
    }
    out
}

pub fn mods() -> Vec<Mod> {
    if let Ok(dir) = std::env::var("TEMPEST_SCRIPT_DIR") {
        return vec![Mod::new("local", PathBuf::from(dir))];
    }
    let mut out = Vec::new();
    let dll = dll_dir();
    if let Some(dll) = &dll {
        let local = dll.join("scripts");
        if local.is_dir() {
            out.push(Mod::new("local", local));
        }
    }
    // Installed V2 mods: <game>/.tempest/v2/mods/*/scripts, enabled ones only
    // (this DLL's own mod always counts) and only when they pass integrity.
    if let Some(mods_root) = tempest_dir().map(|t| t.join("v2").join("mods")) {
        let own = mod_dir()
            .and_then(|m| m.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()));
        let (enabled, by_id) = registry();
        if let Ok(entries) = std::fs::read_dir(&mods_root) {
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
                    continue;
                }
                if let Err(problem) = verify_installed_scripts(&dir, by_id.get(&lower)) {
                    crate::log::line("loader", &format!("[{id}] not loading scripts: {problem}"));
                    continue;
                }
                let (_, after) = manifest(&dir);
                out.push(Mod { id, scripts, root: dir, after, dev: false });
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
    let all = installed();
    if !all.is_empty() {
        let describe = |m: &Installed| {
            let mut tags = Vec::new();
            if m.lua {
                tags.push("lua");
            }
            if m.dll {
                tags.push("dll");
            }
            if m.files > 0 {
                tags.push("content");
            }
            if !m.kind.eq_ignore_ascii_case("v2") && !m.dev {
                tags.push(m.kind.as_str());
            }
            format!("{}{}{}", m.id, if tags.is_empty() { String::new() } else { format!(" [{}]", tags.join(" ")) }, if m.enabled { "" } else { " (disabled)" })
        };
        crate::log::line("loader", &format!("installed: {} mod(s): {}", all.len(), all.iter().map(describe).collect::<Vec<_>>().join(", ")));
    }
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
