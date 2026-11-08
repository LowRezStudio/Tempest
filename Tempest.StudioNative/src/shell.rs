//! File associations for `.upk` and `.twrk` on Windows and Linux (the macOS
//! app declares them in its Info.plist, `scripts/bundle.sh`). A bundled
//! build registers itself for the current user when it first runs from a
//! place, and again when it was moved: on Windows as an "Open with" choice
//! (the user's default app is left alone), on Linux as a desktop entry with
//! the two MIME types.

#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
const KINDS: &[(&str, &str, &str)] = &[("upk", "application/x-unreal-package", "Unreal Engine 3 package"), ("twrk", "application/x-tempest-workspace", "Tempest Studio workspace")];

/// Register this build if it is a bundle and was not registered from where it is now.
pub fn register() {
    if cfg!(target_os = "macos") || crate::update::install().is_none() {
        return;
    }
    let Some(exe) = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok()) else { return };
    let exe = exe.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let Some(stamp) = crate::studio::state_file("associations") else { return };
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s == exe) {
        return;
    }
    if write(&exe).is_ok() {
        let _ = std::fs::create_dir_all(stamp.parent().unwrap());
        let _ = std::fs::write(stamp, &exe);
    }
}

#[cfg(windows)]
fn write(exe: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let reg = |args: &[&str]| -> Result<(), String> {
        let st = std::process::Command::new("reg").args(args).creation_flags(CREATE_NO_WINDOW).status().map_err(|e| e.to_string())?;
        if st.success() { Ok(()) } else { Err(format!("reg {} failed", args.join(" "))) }
    };
    for (ext, _, desc) in KINDS {
        let prog = format!(r"HKCU\Software\Classes\TempestStudioNative.{ext}");
        reg(&["add", &prog, "/ve", "/d", desc, "/f"])?;
        reg(&["add", &format!(r"{prog}\DefaultIcon"), "/ve", "/d", &format!("\"{exe}\",0"), "/f"])?;
        reg(&["add", &format!(r"{prog}\shell\open\command"), "/ve", "/d", &format!("\"{exe}\" \"%1\""), "/f"])?;
        reg(&["add", &format!(r"HKCU\Software\Classes\.{ext}\OpenWithProgids"), "/v", &format!("TempestStudioNative.{ext}"), "/d", "", "/f"])?;
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn write(exe: &str) -> Result<(), String> {
    use std::path::PathBuf;
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))).ok_or("no home folder")?;
    let icon = std::path::Path::new(exe).with_file_name("icon.png");
    let mime_types: String = KINDS.iter().map(|(_, m, _)| format!("{m};")).collect();
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Tempest Studio Native\nComment=Edit Paladins packages and make mods\nExec=\"{exe}\" %f\nIcon={}\nTerminal=false\nCategories=Development;\nMimeType={mime_types}\n",
        icon.display()
    );
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n");
    for (ext, mime, desc) in KINDS {
        xml += &format!("  <mime-type type=\"{mime}\">\n    <comment>{desc}</comment>\n    <glob pattern=\"*.{ext}\"/>\n  </mime-type>\n");
    }
    xml += "</mime-info>\n";
    let apps = data.join("applications");
    let mime = data.join("mime");
    std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(mime.join("packages")).map_err(|e| e.to_string())?;
    std::fs::write(apps.join("tempest-studio-native.desktop"), desktop).map_err(|e| e.to_string())?;
    std::fs::write(mime.join("packages/tempest-studio-native.xml"), xml).map_err(|e| e.to_string())?;
    // Missing tools only delay the desktop noticing.
    let _ = std::process::Command::new("update-mime-database").arg(&mime).status();
    let _ = std::process::Command::new("update-desktop-database").arg(&apps).status();
    Ok(())
}

#[cfg(target_os = "macos")]
fn write(_exe: &str) -> Result<(), String> {
    Ok(())
}
