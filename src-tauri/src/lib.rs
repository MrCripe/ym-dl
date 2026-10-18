mod api;
mod audio;
mod commands;
mod config;
mod download;
mod ffmpeg;
pub mod live;
mod model;
mod sign;
mod startup_log;
mod updates;

use commands::AppState;
use std::path::PathBuf;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// Every folder worth trying as the WebView2 profile, in portable-first order:
/// next to the exe so the whole app travels as one folder, then the profile, then
/// temp. The first one that both accepts a probe write and yields a window wins.
fn data_dirs(app: &tauri::App) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = config::exe_dir() {
        dirs.push(dir.join("ym-dl-data").join("webview"));
    }
    if let Ok(dir) = app.path().app_local_data_dir() {
        dirs.push(dir.join("webview"));
    }
    dirs.push(std::env::temp_dir().join("ym-dl-webview"));
    dirs
}

/// Builds the single app window.
///
/// WebView2 keeps its profile in a user-data folder and refuses to start when
/// that folder cannot be written — which happens on machines with a redirected
/// or permission-damaged `%LOCALAPPDATA%`. Each candidate is probed first, so a
/// refused folder is skipped instead of aborting the launch.
fn create_window(app: &tauri::App) -> Result<(), String> {
    let mut failures = Vec::new();
    for data_dir in data_dirs(app) {
        if !config::dir_writable(&data_dir) {
            startup_log::report_dir("profile (not writable)", &data_dir);
            failures.push(format!("{} — нет прав на запись", data_dir.display()));
            continue;
        }
        startup_log::report_dir("profile", &data_dir);
        match WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
            .title("ym-dl")
            .inner_size(1180.0, 780.0)
            .min_inner_size(960.0, 640.0)
            .center()
            .decorations(false)
            .data_directory(data_dir.clone())
            .build()
        {
            Ok(_) => {
                startup_log::write("window created");
                return Ok(());
            }
            Err(error) => {
                startup_log::write(&format!("window failed at {}: {error}", data_dir.display()));
                failures.push(format!("{} — {error}", data_dir.display()));
            }
        }
    }
    Err(failures.join("\n"))
}

pub fn run() {
    startup_log::write("--- start ---");
    let settings = config::load_settings();
    startup_log::write(&format!("config: {}", config::config_path().display()));
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new(settings))
        .setup(|app| {
            // A window that never opens must not look like a silent crash: the
            // user gets the real reasons and the path of the log file.
            if let Err(error) = create_window(app) {
                let log = startup_log::path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "ym-dl.log".into());
                app.dialog()
                    .message(format!(
                        "Не удалось открыть окно.\n\n{error}\n\nПодробности в файле:\n{log}"
                    ))
                    .kind(MessageDialogKind::Error)
                    .title("ym-dl не запустился")
                    .blocking_show();
                std::process::exit(1);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::has_token,
            commands::clear_token,
            commands::check_token,
            commands::resolve_source,
            commands::load_likes,
            commands::start_download,
            commands::stop_download,
            commands::check_updates,
            commands::environment,
            commands::post_process,
            commands::probe_lossless,
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить приложение");
}
