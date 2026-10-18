mod api;
mod audio;
mod commands;
mod config;
mod download;
mod ffmpeg;
mod model;
mod sign;
mod updates;

use commands::AppState;
use std::path::PathBuf;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// Builds the single app window.
///
/// WebView2 keeps its profile in a user-data folder and refuses to start when
/// that folder cannot be written. The standard location is tried first and the
/// temp folder second, so a damaged `%LOCALAPPDATA%` still opens the app.
fn create_window(app: &tauri::App) -> tauri::Result<()> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = app.path().app_local_data_dir() {
        candidates.push(dir.join("webview"));
    }
    candidates.push(std::env::temp_dir().join("ym-dl-webview"));

    let mut last_error = None;
    for data_dir in candidates {
        let _ = std::fs::create_dir_all(&data_dir);
        match WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
            .title("ym-dl")
            .inner_size(1180.0, 780.0)
            .min_inner_size(960.0, 640.0)
            .center()
            .decorations(false)
            .data_directory(data_dir)
            .build()
        {
            Ok(_) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.expect("at least one candidate directory is always tried"))
}

pub fn run() {
    let settings = config::load_settings();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new(settings))
        .setup(|app| {
            // A window that never opens must not look like a silent crash: the
            // user gets the real reason and the folder that could not be written.
            if let Err(error) = create_window(app) {
                app.dialog()
                    .message(format!(
                        "Не удалось открыть окно: {error}\n\n\
                         WebView2 не смог создать профиль в папке данных.\n\
                         Проверьте права на {} и свободное место на диске.",
                        config::home_dir().display()
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
