// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use tauri::scope::Scopes;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_persisted_scope::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            scopes_allow_directory,
            scopes_allow_file,
            scopes_forbid_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
