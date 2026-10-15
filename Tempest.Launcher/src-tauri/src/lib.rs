// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::scope::Scopes;
use tauri::{Emitter, Manager};

mod child_cleanup;

struct PendingOpenFiles(Mutex<Vec<String>>);

fn tempest_files_from_args(
    args: impl IntoIterator<Item = String>,
    working_directory: Option<&Path>,
) -> Vec<String> {
    args.into_iter()
        .filter_map(|argument| {
            let path = PathBuf::from(argument);
            let path = if path.is_absolute() {
                path
            } else if let Some(working_directory) = working_directory {
                working_directory.join(path)
            } else {
                path
            };

            let is_tempest_file = path.extension().is_some_and(|extension| {
                extension
                    .to_string_lossy()
                    .eq_ignore_ascii_case("tempest")
            });

            (is_tempest_file && path.is_file()).then(|| path.to_string_lossy().into_owned())
        })
        .collect()
}

fn queue_open_files(app: &tauri::AppHandle, files: Vec<String>) {
    if files.is_empty() {
        return;
    }

    let pending = app.state::<PendingOpenFiles>();
    let mut pending = pending
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    pending.extend(files);
    drop(pending);

    // The frontend drains the queue after receiving this notification. Keeping the
    // paths in Rust means a launch event cannot be lost while the webview is loading.
    let _ = app.emit("open-mod-files", ());
}

#[tauri::command]
fn take_pending_open_files(state: tauri::State<'_, PendingOpenFiles>) -> Vec<String> {
    let mut pending = state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    std::mem::take(&mut *pending)
}

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
fn is_process_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return false;
        };
        let mut exit_code = 0;
        let running = unsafe { GetExitCodeProcess(process, &mut exit_code) }.is_ok()
            && exit_code == STILL_ACTIVE.0 as u32;
        unsafe {
            let _ = CloseHandle(process);
        }
        return running;
    }

    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        return result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    }

    #[allow(unreachable_code)]
    false
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

    #[cfg(target_os = "windows")]
    {
        // Set AppUserModelID so Windows identifies this app consistently for taskbar pinning,
        // jump lists, and icon caching across updates.
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
        use windows::core::PCWSTR;
        let app_id: Vec<u16> = "com.lowrezstudio.tempest".encode_utf16().chain(std::iter::once(0)).collect();
        let app_id = PCWSTR::from_raw(app_id.as_ptr());
        let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(app_id) };
    }

    let initial_working_directory = std::env::current_dir().ok();
    let initial_open_files = tempest_files_from_args(
        std::env::args_os().map(|argument| argument.to_string_lossy().into_owned()),
        initial_working_directory.as_deref(),
    );

    tauri::Builder::default()
        // This must be the first plugin so file opens from later processes can be
        // forwarded to the already-running launcher before those processes exit.
        .plugin(tauri_plugin_single_instance::init(
            |app, args, working_directory| {
                let files =
                    tempest_files_from_args(args, Some(Path::new(&working_directory)));
                queue_open_files(app, files);

                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            },
        ))
        .manage(PendingOpenFiles(Mutex::new(initial_open_files)))
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
            take_pending_open_files,
            relaunch,
            trigger_child_cleanup,
            is_process_running,
            which,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
