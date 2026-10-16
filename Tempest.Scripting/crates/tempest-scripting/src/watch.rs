//! Hot reload: a polling watcher over the dev mods folder and the per-mod
//! config directory. A changed `.lua`/`.toml` under `<dev>/<folder>/`
//! reloads that mod; a changed `<configs>/<id>.toml` reloads mod `<id>`.
//! Reloads run on the game thread (queued), after a quiet period so an
//! editor's save burst counts once.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const POLL: Duration = Duration::from_millis(500);

type Snapshot = BTreeMap<PathBuf, (SystemTime, u64)>;

fn watched(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("lua") | Some("toml"))
}

fn scan_into(dir: &Path, depth: usize, out: &mut Snapshot) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_into(&path, depth + 1, out);
        } else if watched(&path) {
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
    delay: Duration,
    last: Snapshot,
    pending: HashSet<String>,
    quiet_since: Option<Instant>,
}

impl Watcher {
    pub fn new(dev: Option<PathBuf>, configs: Option<PathBuf>, delay: Duration) -> Watcher {
        let last = Self::snapshot(dev.as_deref(), configs.as_deref());
        Watcher { dev, configs, delay, last, pending: HashSet::new(), quiet_since: None }
    }

    fn snapshot(dev: Option<&Path>, configs: Option<&Path>) -> Snapshot {
        let mut s = Snapshot::new();
        if let Some(d) = dev {
            scan_into(d, 0, &mut s);
        }
        if let Some(c) = configs {
            scan_into(c, 0, &mut s);
        }
        s
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
        let now = Self::snapshot(self.dev.as_deref(), self.configs.as_deref());
        let mut changed: Vec<PathBuf> = now.iter().filter(|(p, s)| self.last.get(*p) != Some(s)).map(|(p, _)| p.clone()).collect();
        changed.extend(self.last.keys().filter(|p| !now.contains_key(*p)).cloned());
        if !changed.is_empty() {
            for path in &changed {
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
    if dev.is_none() && configs.is_none() {
        return None;
    }
    let delay = Duration::from_millis(cfg.hot_reload_delay_ms);
    let watcher = Watcher::new(dev.clone(), configs.clone(), delay);
    let what = format!(
        "{} ({} files, {} ms quiet period)",
        [dev.as_ref().map(|d| d.display().to_string()), configs.as_ref().map(|c| c.display().to_string())]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", "),
        watcher.tracked(),
        cfg.hot_reload_delay_ms
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
pub fn reload(ids: Vec<String>) {
    let lua = crate::lua();
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
