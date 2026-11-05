//! What the launcher does to installed mods reaches the running scripts: the watcher
//! follows its registry, and only mods that are nothing but scripts change live.

use std::path::Path;
use std::time::Duration;

use tempest_scripting::watch::{reload, Watcher};

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Writes a mod folder as the CLI installs it and returns its registry record.
fn install(game: &Path, id: &str, version: &str, script: &str, dependencies: &str, extra_dir: Option<&str>) -> String {
    let m = game.join(".tempest/v2/mods").join(id);
    let _ = std::fs::remove_dir_all(&m);
    std::fs::create_dir_all(m.join("scripts")).unwrap();
    std::fs::write(m.join("scripts/main.lua"), script).unwrap();
    std::fs::write(m.join("manifest.toml"), format!("[mod]\nid = \"{id}\"\ndependencies = [{dependencies}]\n")).unwrap();
    if let Some(dir) = extra_dir {
        std::fs::create_dir_all(m.join(dir)).unwrap();
    }
    let checksums = format!("{} scripts/main.lua\n", sha256_hex(script.as_bytes()));
    std::fs::write(m.join("CHECKSUMS"), &checksums).unwrap();
    format!(
        r#"{{"Id":"{id}","Name":"{id}","Version":"{version}","Enabled":true,"Kind":"V2","InstalledFiles":[],"Signer":"abc","Verified":true,"ManifestHash":"{}"}}"#,
        sha256_hex(checksums.as_bytes())
    )
}

fn write_registry(game: &Path, records: &[&str]) {
    // A file system's timestamps can be coarse; the stamp must differ from the last write.
    std::thread::sleep(Duration::from_millis(30));
    std::fs::write(game.join(".tempest/mods/mods.json"), format!("[{}]", records.join(","))).unwrap();
}

/// The first poll sees the change, the one after the registry's quiet period answers.
fn settled(w: &mut Watcher) -> Vec<String> {
    assert!(w.poll().is_empty(), "a change starts the quiet period");
    std::thread::sleep(Duration::from_millis(1100));
    let mut ready = w.poll();
    ready.sort();
    ready
}

#[test]
fn launcher_changes_reach_the_running_scripts() {
    let game = std::env::temp_dir().join(format!("tempest-installed-reload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&game);
    std::fs::create_dir_all(game.join(".tempest/mods")).unwrap();
    std::fs::create_dir_all(game.join("ChaosGame/Config")).unwrap();
    std::fs::write(game.join("ChaosGame/Config/tempest.toml"), "[dev]\nhot_reload = false\n").unwrap();
    std::env::set_var("TEMPEST_GAME_DIR", &game);

    let lib = install(&game, "a-lib", "1.0.0", "shared.lib_version = 1\n", "", None);
    let user = install(&game, "b-user", "1.0.0", "shared.user_saw = shared.lib_version\n", "\"a-lib>=1.0.0\"", None);
    let ui = install(&game, "c-ui", "1.0.0", "shared.ui_version = 1\n", "", Some("deltas"));
    write_registry(&game, &[&lib, &user, &ui]);

    let lua = tempest_scripting::lua();
    let number = |name: &str| lua.globals().get::<mlua::Table>("shared").unwrap().get::<Option<i64>>(name).unwrap();
    lua.globals().set("shared", lua.create_table().unwrap()).unwrap();
    reload(vec!["a-lib".into(), "b-user".into(), "c-ui".into()]);
    assert_eq!((number("lib_version"), number("user_saw"), number("ui_version")), (Some(1), Some(1), Some(1)));

    let registry = game.join(".tempest/mods/mods.json");
    let mut w = Watcher::new(None, None, false, Duration::from_millis(10)).watch_registry(registry);
    assert!(w.poll().is_empty(), "what was running at the start is not a change");

    // Rewriting the registry without touching the mod (another mod's install) reloads nothing.
    write_registry(&game, &[&lib, &user, &ui]);
    assert!(settled(&mut w).is_empty());

    // An update reloads the mod, and what depends on it sees the new version.
    let lib2 = install(&game, "a-lib", "2.0.0", "shared.lib_version = 2\n", "", None);
    write_registry(&game, &[&lib2, &user, &ui]);
    let ready = settled(&mut w);
    assert_eq!(ready, vec!["a-lib".to_string(), "b-user".to_string()]);
    reload(ready);
    assert_eq!((number("lib_version"), number("user_saw")), (Some(2), Some(2)));

    // A mod that is more than scripts waits for the next launch.
    let ui2 = install(&game, "c-ui", "2.0.0", "shared.ui_version = 2\n", "", Some("deltas"));
    write_registry(&game, &[&lib2, &user, &ui2]);
    assert!(settled(&mut w).is_empty());
    assert_eq!(number("ui_version"), Some(1));

    // A new mod starts, and a removed one stops what it registered.
    let extra = install(&game, "d-extra", "1.0.0", "shared.extra = 1\ntempest.command('extra-cmd', function() end, '')\n", "", None);
    write_registry(&game, &[&lib2, &user, &ui2, &extra]);
    let ready = settled(&mut w);
    assert_eq!(ready, vec!["d-extra".to_string()]);
    reload(ready);
    let has_command = || {
        lua.load("for _, c in ipairs(tempest.commands()) do if c.name == 'extra-cmd' then return true end end return false").eval::<bool>().unwrap()
    };
    assert!(has_command());
    std::fs::remove_dir_all(game.join(".tempest/v2/mods/d-extra")).unwrap();
    write_registry(&game, &[&lib2, &user, &ui2]);
    let ready = settled(&mut w);
    assert_eq!(ready, vec!["d-extra".to_string()]);
    reload(ready);
    assert!(!has_command());

    // A half written registry changes nothing until the write finishes.
    std::thread::sleep(Duration::from_millis(30));
    std::fs::write(game.join(".tempest/mods/mods.json"), "[{\"Id\":").unwrap();
    assert!(settled(&mut w).is_empty());
    write_registry(&game, &[&lib2, &user, &ui2]);
    assert!(settled(&mut w).is_empty());

    let _ = std::fs::remove_dir_all(&game);
}
