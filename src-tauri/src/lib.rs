mod api;
mod audio;
mod commands;
mod config;
mod download;
mod ffmpeg;
pub mod live;
mod lyrics;
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

/// Surface colour of the window before the UI paints. Matches `--surface` in the
/// stylesheet, so the frame between window creation and the first paint is the
/// app colour instead of the default white.
fn surface_color(dark: bool) -> tauri::window::Color {
    if dark {
        tauri::window::Color(0x14, 0x12, 0x18, 0xff)
    } else {
        tauri::window::Color(0xfe, 0xf7, 0xff, 0xff)
    }
}

/// The profile folder the main window ended up using. Every later window has to
/// reuse it: a window created without one falls back to `%LOCALAPPDATA%`, which is
/// exactly the folder that is refused on this kind of machine.
static PROFILE_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Folder for the Yandex sign-in window, or `None` when the app has not resolved
/// one yet.
pub fn profile_dir() -> Option<PathBuf> {
    PROFILE_DIR.get().cloned()
}

/// Builds the single app window.
///
/// WebView2 keeps its profile in a user-data folder and refuses to start when
/// that folder cannot be written — which happens on machines with a redirected
/// or permission-damaged `%LOCALAPPDATA%`. Each candidate is probed first, so a
/// refused folder is skipped instead of aborting the launch.
///
/// The window stays hidden until the page has painted its boot screen: showing it
/// earlier is what produced the white flash before the dark UI appeared.
fn create_window(app: &tauri::App, theme: &str) -> Result<(), String> {
    let mut failures = Vec::new();
    // Only an explicit choice is pushed into the page: `system` must stay
    // undecided so the boot screen follows the OS, not a guess made here.
    let script = match theme {
        "dark" | "light" => format!("window.__YMDL_THEME = \"{theme}\";"),
        _ => "window.__YMDL_THEME = null;".to_string(),
    };
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
            .visible(false)
            .background_color(surface_color(theme == "dark"))
            .initialization_script(&script)
            .data_directory(data_dir.clone())
            .on_page_load(|window, payload| {
                if payload.event() == tauri::webview::PageLoadEvent::Finished {
                    let _ = window.show();
                }
            })
            .build()
        {
            Ok(window) => {
                startup_log::write("window created");
                let _ = PROFILE_DIR.set(data_dir.clone());
                // The OS preference is only readable once the window exists. Doing
                // it here, before the first show, keeps a dark desktop from getting
                // a light frame at startup.
                if theme == "system" {
                    if let Ok(tauri::Theme::Dark) = window.theme() {
                        let _ = window.set_background_color(Some(surface_color(true)));
                    }
                }
                // A page that never finishes loading must not leave an invisible
                // window behind: show it anyway after a short grace period.
                let fallback = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let _ = fallback.show();
                });
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
    // Before anything is read: after an in-app update the binary lives in a new
    // folder, and this is what brings the old settings and token across.
    config::adopt_previous_data_dir();
    config::remember_data_dir();
    let settings = config::load_settings();
    let theme = settings.theme.clone();
    startup_log::write(&format!("config: {}", config::config_path().display()));
    // The closure has to own its copy: the builder keeps it for the app lifetime.
    let setup_theme = theme.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::new(settings))
        .setup(move |app| {
            // A window that never opens must not look like a silent crash: the
            // user gets the real reasons and the path of the log file.
            if let Err(error) = create_window(app, &setup_theme) {
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
            commands::login_yandex,
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
