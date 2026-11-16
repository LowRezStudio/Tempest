//! The game folders the Studio can find on its own, so a first run does not
//! start with a folder picker: the Tempest Launcher's instances (the one
//! played last first), then Steam's Paladins.

use std::path::{Path, PathBuf};

/// A game folder found on this computer.
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    /// What to call it: the Launcher instance's name, or "Paladins (Steam)".
    pub label: String,
    /// The game version when known ("8.1").
    pub version: Option<String>,
    /// The game's root folder (the one holding ChaosGame).
    pub path: String,
}

/// Every game found, the Launcher's last played first; folders without
/// cooked packages are left out.
pub fn find() -> Vec<Game> {
    let mut out = launcher_dir().and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok()).map(|s| launcher_games(&s)).unwrap_or_default();
    for lib in steam_libraries() {
        let path = lib.join("steamapps").join("common").join("Paladins");
        out.push(Game { label: "Paladins (Steam)".into(), version: None, path: path.to_string_lossy().to_string() });
    }
    let mut seen = Vec::new();
    out.retain(|g| {
        let ok = Path::new(&g.path).join("ChaosGame").join("CookedPCConsole").is_dir() && !seen.contains(&g.path);
        seen.push(g.path.clone());
        ok
    });
    out
}

/// The Launcher's profile folder (`com.lowrezstudio.tempest`).
fn launcher_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support/com.lowrezstudio.tempest"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("com.lowrezstudio.tempest"))
    } else {
        home.map(|h| h.join(".local/share/com.lowrezstudio.tempest"))
    }
}

/// The instances of the Launcher's `settings.json` (`instances:<id>` keys
/// holding JSON with `label`, `version` and `path`), the last launched first.
pub fn launcher_games(settings: &str) -> Vec<Game> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(settings) else { return Vec::new() };
    let last = map.get("lastLaunchedInstanceId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut games: Vec<(bool, Game)> = Vec::new();
    for (key, value) in &map {
        let Some(id) = key.strip_prefix("instances:") else { continue };
        let inst = match value {
            serde_json::Value::String(s) => serde_json::from_str(s).unwrap_or(serde_json::Value::Null),
            v => v.clone(),
        };
        let Some(path) = inst["path"].as_str().filter(|p| !p.is_empty()) else { continue };
        let label = inst["label"].as_str().filter(|l| !l.is_empty()).unwrap_or("Paladins").to_string();
        games.push((id == last, Game { label, version: inst["version"].as_str().map(str::to_string), path: path.to_string() }));
    }
    // Last played first, then the newest versions.
    games.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| version_key(&b.1.version).cmp(&version_key(&a.1.version))));
    games.into_iter().map(|(_, g)| g).collect()
}

fn version_key(v: &Option<String>) -> Vec<u32> {
    v.as_deref().unwrap_or("").split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Steam's library folders (`steamapps/libraryfolders.vdf`), the default one first.
fn steam_libraries() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let roots: Vec<PathBuf> = if cfg!(target_os = "macos") {
        home.into_iter().map(|h| h.join("Library/Application Support/Steam")).collect()
    } else if cfg!(windows) {
        ["ProgramFiles(x86)", "ProgramFiles"].iter().filter_map(std::env::var_os).map(|p| PathBuf::from(p).join("Steam")).collect()
    } else {
        home.into_iter().flat_map(|h| [h.join(".local/share/Steam"), h.join(".steam/steam")]).collect()
    };
    let mut out = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        out.push(root.clone());
        if let Ok(vdf) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
            out.extend(vdf_paths(&vdf).into_iter().map(PathBuf::from));
        }
    }
    out.dedup();
    out
}

/// The `"path"` values of a `libraryfolders.vdf`.
pub fn vdf_paths(vdf: &str) -> Vec<String> {
    vdf.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("\"path\"")?.trim();
            Some(rest.trim_matches('"').replace("\\\\", "\\"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_launcher_instances_last_played_first() {
        let settings = serde_json::json!({
            "lastLaunchedInstanceId": "b",
            "instances:a": serde_json::json!({ "label": "Bird of Prey", "version": "0.57", "path": "/g/0.57" }).to_string(),
            "instances:b": serde_json::json!({ "label": "Wekono's Wake", "version": "8.1", "path": "/g/8.1" }).to_string(),
            "instances:c": serde_json::json!({ "label": "Banners Fall", "version": "7.2", "path": "/g/7.2" }).to_string(),
            "instances:d": serde_json::json!({ "label": "No path" }).to_string(),
            "other": 1,
        })
        .to_string();
        let g = launcher_games(&settings);
        assert_eq!(g.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(), ["Wekono's Wake", "Banners Fall", "Bird of Prey"]);
        assert_eq!(g[0].version.as_deref(), Some("8.1"));
        assert!(launcher_games("not json").is_empty());
    }

    #[test]
    fn reads_steam_library_paths() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\Games\"\n\t}\n}\n";
        assert_eq!(vdf_paths(vdf), ["C:\\Program Files (x86)\\Steam", "D:\\Games"]);
    }
}
