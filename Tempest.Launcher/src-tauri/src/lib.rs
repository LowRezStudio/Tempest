// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use tauri::scope::Scopes;

mod child_cleanup;

#[tauri::command]
fn scopes_allow_directory(
    scopes: tauri::State<'_, Scopes>,
    path: String,
    recursive: bool,
) -> Result<(), String> {
    scopes
        .allow_directory(path, recursive)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn scopes_allow_file(scopes: tauri::State<'_, Scopes>, path: String) -> Result<(), String> {
    scopes.allow_file(path).map_err(|error| error.to_string())
}

#[tauri::command]
fn scopes_forbid_file(scopes: tauri::State<'_, Scopes>, path: String) -> Result<(), String> {
    scopes.forbid_file(path).map_err(|error| error.to_string())
}

#[tauri::command]
fn relaunch(app: tauri::AppHandle) {
    app.restart();
}

#[tauri::command]
fn trigger_child_cleanup() {
    #[cfg(target_os = "windows")]
    child_cleanup::setup();
}

#[tauri::command]
fn which(name: String) -> Result<Option<String>, String> {
    let path = std::env::var_os("PATH").ok_or("PATH environment variable is not set")?;

    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(&name);
        if is_executable(&candidate) {
            return Ok(Some(candidate.to_string_lossy().to_string()));
        }

        #[cfg(target_os = "windows")]
        {
            let candidate_exe = dir.join(format!("{}.exe", name));
            if is_executable(&candidate_exe) {
                return Ok(Some(candidate_exe.to_string_lossy().to_string()));
            }
        }
    }

    Ok(None)
}

#[cfg(target_os = "windows")]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}

#[cfg(not(target_os = "windows"))]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    match std::fs::metadata(path) {
        Ok(metadata) => metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    {
        // AppImage bundles libgbm/libEGL which mismatch with the host's Mesa drivers,
        // causing EGL display creation to fail. We disable DMABuf only inside AppImages.
        if std::env::var("APPIMAGE").is_ok() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

	tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::new().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_persisted_scope::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|_app| {
            #[cfg(not(target_os = "windows"))]
            child_cleanup::setup();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scopes_allow_directory,
            scopes_allow_file,
            scopes_forbid_file,
            relaunch,
            trigger_child_cleanup,
            which,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
