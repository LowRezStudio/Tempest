//! Hot reload: a polling watcher over the dev mods folder, the per-mod
//! config directory and the launcher's mod registry. A changed `.lua`/`.toml`
//! under `<dev>/<folder>/` reloads that mod; a changed `<configs>/<id>.toml`
//! reloads mod `<id>`; a Lua-only mod the launcher installed, updated or removed
//! (the registry is written last, so it is the signal, and the scripts still
//! pass the loader's integrity checks) is reloaded or stopped.
//! Reloads run on the game thread (queued), after a quiet period so an
//! editor's save burst counts once. Files the runtime writes itself
//! (`tempest.save_config`, generated defaults) are announced through
//! [`expect`] so they never count as edits.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use crate::loader::InstalledScripts;

const POLL: Duration = Duration::from_millis(500);
/// An update writes the registry twice, around unpacking the mod.
const REGISTRY_QUIET: Duration = Duration::from_secs(1);

type Stamp = (SystemTime, u64);
type Snapshot = BTreeMap<PathBuf, Stamp>;

/// Files written by the runtime itself, with the stamp they got: the next
/// change the watcher sees on them with that stamp is not an edit.
static EXPECTED: Mutex<Option<HashMap<PathBuf, Stamp>>> = Mutex::new(None);

fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len()))
}

/// Tell the watcher the runtime just wrote `path`.
pub fn expect(path: &Path) {
    if let Some(stamp) = stamp_of(path) {
        EXPECTED.lock().unwrap().get_or_insert_default().insert(path.to_path_buf(), stamp);
    }
}

fn watched(path: &Path, configs: bool) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some("lua") => true,
        Some("toml") => configs,
        _ => false,
    }
}

fn scan_into(dir: &Path, depth: usize, configs: bool, out: &mut Snapshot) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_into(&path, depth + 1, configs, out);
        } else if watched(&path, configs) {
            // Fresh metadata: a directory listing can report a stale write
            // time on Windows while an editor still holds the file.
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.insert(path, (mtime, meta.len()));
        }
    }
}

/// The watcher state: what was seen last, what changed, and since when
/// nothing changed. `poll` is the whole algorithm, so tests can drive it.
pub struct Watcher {
    dev: Option<PathBuf>,
    configs: Option<PathBuf>,
    /// Do `.toml` changes (dev mods' config.toml, the configs dir) reload?
    reload_on_config: bool,
    delay: Duration,
    last: Snapshot,
    pending: HashSet<String>,
    quiet_since: Option<Instant>,
    registry: Option<PathBuf>,
    registry_changed: Option<Instant>,
    installed: HashMap<String, InstalledScripts>,
}

/// Which mods changed between two reads of the registry: those to reload (or stop,
/// when gone) and those that need a restart because they are more than scripts,
/// before or after the change.
fn registry_changes(before: &HashMap<String, InstalledScripts>, after: &HashMap<String, InstalledScripts>) -> (Vec<String>, Vec<String>) {
    let mut reload = Vec::new();
    let mut restart = Vec::new();
    let ids: HashSet<&String> = before.keys().chain(after.keys()).collect();
    for id in ids {
        let (old, new) = (before.get(id), after.get(id));
        if old == new {
            continue;
        }
        if old.is_none_or(|s| s.lua_only) && new.is_none_or(|s| s.lua_only) {
            reload.push(id.clone());
        } else {
            restart.push(id.clone());
        }
    }
    reload.sort();
    restart.sort();
    (reload, restart)
}

impl Watcher {
    pub fn new(dev: Option<PathBuf>, configs: Option<PathBuf>, reload_on_config: bool, delay: Duration) -> Watcher {
        let configs = if reload_on_config { configs } else { None };
        let last = Self::snapshot(dev.as_deref(), configs.as_deref(), None, reload_on_config);
        Watcher { dev, configs, reload_on_config, delay, last, pending: HashSet::new(), quiet_since: None, registry: None, registry_changed: None, installed: HashMap::new() }
    }

    /// Also follow what the launcher does to installed mods, through its registry file.
    pub fn watch_registry(mut self, registry: PathBuf) -> Watcher {
        if let Some(stamp) = stamp_of(&registry) {
            self.last.insert(registry.clone(), stamp);
        }
        self.registry = Some(registry);
        self.installed = crate::loader::installed_scripts().unwrap_or_default();
        self
    }

    fn snapshot(dev: Option<&Path>, configs: Option<&Path>, registry: Option<&Path>, reload_on_config: bool) -> Snapshot {
        let mut s = Snapshot::new();
        if let Some(d) = dev {
            scan_into(d, 0, reload_on_config, &mut s);
        }
        if let Some(c) = configs {
            scan_into(c, 0, true, &mut s);
        }
        if let Some((r, stamp)) = registry.and_then(|r| Some((r, stamp_of(r)?))) {
            s.insert(r.to_path_buf(), stamp);
        }
        s
    }

    /// The installed mods to reload now that the registry has settled.
    fn installed_changes(&mut self) -> Vec<String> {
        // Half written: the write that finishes it changes the stamp again.
        let Some(now) = crate::loader::installed_scripts() else { return Vec::new() };
        let (reload, restart) = registry_changes(&self.installed, &now);
        self.installed = now;
        for id in &restart {
            crate::log::line("hot", &format!("{id} changed; it is more than scripts, so it applies at the next launch"));
        }
        let dev: Vec<String> = crate::loader::mods().into_iter().filter(|m| m.dev).map(|m| m.id).collect();
        let reload: Vec<String> = reload.into_iter().filter(|id| !dev.iter().any(|d| d.eq_ignore_ascii_case(id))).collect();
        if reload.is_empty() {
            return reload;
        }
        crate::log::line("hot", &format!("the launcher changed {}", reload.join(", ")));
        crate::loader::with_dependants(reload)
    }

    pub fn tracked(&self) -> usize {
        self.last.len()
    }

    /// Which mod a changed file belongs to: a config file's stem, or the dev
    /// folder's mod (by its manifest id when it has one). Files in folders
    /// that are not mods (`types/`) belong to nobody.
    fn owner(&self, path: &Path) -> Option<String> {
        if let Some(c) = &self.configs {
            if path.parent() == Some(c.as_path()) {
                return path.file_stem().map(|s| s.to_string_lossy().to_string());
            }
        }
        let d = self.dev.as_ref()?;
        let rel = path.strip_prefix(d).ok()?;
        let folder = rel.components().next()?.as_os_str().to_string_lossy().to_string();
        crate::loader::mods()
            .into_iter()
            .find(|m| m.dev && m.root.file_name().is_some_and(|n| n.to_string_lossy() == folder))
            .map(|m| m.id)
    }

    /// One poll: rescan, note changes, and return the mods ready to reload
    /// (none while files are still changing or the quiet period runs).
    pub fn poll(&mut self) -> Vec<String> {
        let now = Self::snapshot(self.dev.as_deref(), self.configs.as_deref(), self.registry.as_deref(), self.reload_on_config);
        let mut changed: Vec<PathBuf> = now.iter().filter(|(p, s)| self.last.get(*p) != Some(s)).map(|(p, _)| p.clone()).collect();
        changed.extend(self.last.keys().filter(|p| !now.contains_key(*p)).cloned());
        if !changed.is_empty() {
            let mut expected = EXPECTED.lock().unwrap();
            for path in &changed {
                if self.registry.as_ref() == Some(path) {
                    self.registry_changed = Some(Instant::now());
                    continue;
                }
                // The runtime's own write (save_config, generated defaults): not an edit.
                if let Some(e) = expected.as_mut() {
                    if e.get(path).is_some_and(|stamp| now.get(path) == Some(stamp)) {
                        e.remove(path);
                        continue;
                    }
                }
                match self.owner(path) {
                    Some(id) => {
                        if self.pending.insert(id.clone()) {
                            crate::log::line("hot", &format!("{} changed ({id})", path.display()));
                        }
                    }
                    None => crate::log::line("hot", &format!("{} changed (not a mod file; ignored)", path.display())),
                }
            }
            self.quiet_since = Some(Instant::now());
            self.last = now;
            return Vec::new();
        }
        if self.registry_changed.is_some_and(|t| t.elapsed() >= self.delay.max(REGISTRY_QUIET)) {
            self.registry_changed = None;
            let ready = self.installed_changes();
            if !ready.is_empty() {
                return ready;
            }
        }
        match self.quiet_since {
            Some(t) if t.elapsed() >= self.delay && !self.pending.is_empty() => {
                self.quiet_since = None;
                self.pending.drain().collect()
            }
            _ => Vec::new(),
        }
    }
}

/// Start the watcher thread when hot reload is on and there is something
/// to watch. Returns what it watches, for the log.
pub fn start() -> Option<String> {
    let cfg = crate::config::get();
    if !cfg.hot_reload {
        return None;
    }
    let dev = crate::config::dev_mods_dir().filter(|d| d.is_dir());
    let configs = crate::config::mod_configs_dir();
    // TEMPEST_SCRIPT_DIR replaces mod discovery, so installed mods are not running.
    let registry = crate::loader::mods_json_path().filter(|_| std::env::var_os("TEMPEST_SCRIPT_DIR").is_none());
    if dev.is_none() && configs.is_none() && registry.is_none() {
        return None;
    }
    let delay = Duration::from_millis(cfg.hot_reload_delay_ms);
    let mut watcher = Watcher::new(dev.clone(), configs.clone(), cfg.reload_on_config, delay);
    if let Some(registry) = registry.clone() {
        watcher = watcher.watch_registry(registry);
    }
    let what = format!(
        "{} ({} files, {} ms quiet period{})",
        [dev.as_ref().map(|d| d.display().to_string()), configs.as_ref().filter(|_| cfg.reload_on_config).map(|c| c.display().to_string()), registry.as_ref().map(|r| r.display().to_string())]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", "),
        watcher.tracked(),
        cfg.hot_reload_delay_ms,
        if cfg.reload_on_config { "" } else { ", config files excluded" }
    );
    std::thread::spawn(move || run(watcher));
    Some(what)
}

fn run(mut watcher: Watcher) {
    loop {
        std::thread::sleep(POLL);
        let ready = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| watcher.poll())) {
            Ok(ids) => ids,
            Err(_) => {
                crate::log::line("hot", "watcher panicked; hot reload stopped");
                return;
            }
        };
        if !ready.is_empty() {
            crate::log::line("hot", &format!("reloading {} at the next event", ready.join(", ")));
            crate::unreal::queue(Box::new(move || reload(ready)));
        }
    }
}

/// Game thread: reload each mod and tell the player.
pub fn reload(mut ids: Vec<String>) {
    let lua = crate::lua();
    // Dependencies before the mods that use them; mods that are gone only stop.
    let order = crate::loader::mods();
    ids.sort_by_key(|id| order.iter().position(|m| m.id.eq_ignore_ascii_case(id)).map_or(0, |at| at + 1));
    for id in ids {
        crate::loader::reload(lua, Some(&id));
        let msg = format!("hot reload: {id}");
        if let Ok(t) = lua.globals().get::<mlua::Table>("tempest") {
            if let Ok(f) = t.get::<mlua::Function>("toast") {
                let _ = f.call::<()>(msg.as_str());
            }
        }
    }
}
