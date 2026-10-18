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
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Writes and removes a probe file, returning the outcome.
fn probe(dir: &std::path::Path) -> std::io::Result<()> {
    let path = dir.join("probe.txt");
    std::fs::create_dir_all(dir)?;
    std::fs::write(&path, "ok")?;
    std::fs::remove_file(&path)
}

/// Builds the single app window.
///
/// WebView2 aborts with "catastrophic failure" when its user data folder does
/// not exist yet, so the folder is created here and passed explicitly instead
/// of relying on the default `%LOCALAPPDATA%\dev.ymdl.app` location.
fn create_window(app: &tauri::App) -> tauri::Result<()> {
    let base = app.path().app_local_data_dir()?;
    let roaming = app.path().app_data_dir()?;
    eprintln!("[diag] local write: {:?}", probe(&base));
    eprintln!("[diag] roaming write: {:?}", probe(&roaming));
    let data_dir = base.join("webview");
    std::fs::create_dir_all(&data_dir).map_err(|err| {
        tauri::Error::Io(std::io::Error::new(
            err.kind(),
            format!("не удалось создать {}: {err}", data_dir.display()),
        ))
    })?;
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("ym-dl")
        .inner_size(1180.0, 780.0)
        .min_inner_size(960.0, 640.0)
        .center()
        .decorations(false)
        .data_directory(data_dir)
        .build()?;
    Ok(())
}

pub fn run() {
    let settings = config::load_settings();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new(settings))
        .setup(|app| {
            create_window(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::has_token,
            commands::save_token,
            commands::clear_token,
            commands::check_token,
            commands::resolve_source,
            commands::load_likes,
            commands::start_download,
            commands::stop_download,
            commands::is_downloading,
            commands::check_updates,
            commands::environment,
            commands::post_process,
            commands::preview_path,
            commands::probe_lossless,
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить приложение");
}
