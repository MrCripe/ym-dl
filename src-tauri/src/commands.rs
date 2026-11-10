//! Tauri commands: the whole surface the UI talks to.

use crate::api::{self, Client};
use crate::config::{self, Settings};
use crate::download::{Job, JobControl};
use crate::ffmpeg;
use crate::model::*;
use crate::startup_log;
use crate::updates;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    settings: Mutex<Settings>,
    client: Mutex<Option<Client>>,
    job: Mutex<Option<JobControl>>,
    running: AtomicBool,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: Mutex::new(settings),
            client: Mutex::new(None),
            job: Mutex::new(None),
            running: AtomicBool::new(false),
        }
    }

    fn settings(&self) -> Settings {
        self.settings.lock().expect("settings lock").clone()
    }

    fn client(&self) -> Result<Client> {
        let token = config::read_token();
        if token.is_empty() {
            anyhow::bail!("Токен не задан. Вставьте токен Яндекс.Музыки.");
        }
        let settings = self.settings();
        let mut slot = self.client.lock().expect("client lock");
        if slot.is_none() {
            *slot = Some(Client::new(
                &token,
                settings.timeout,
                settings.tries,
                settings.retry_delay,
            )?);
        }
        Ok(slot.as_ref().expect("client just set").clone())
    }

    fn reset_client(&self) {
        if let Ok(mut slot) = self.client.lock() {
            *slot = None;
        }
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    let mut sanitized = settings;
    sanitized.sanitize();
    // The lock is held across the write so an autosave and the username fill-in
    // cannot interleave: without it one of them writes a stale copy of the other
    // field and the edit is lost.
    {
        let mut slot = state.settings.lock().expect("settings lock");
        config::save_settings(&sanitized).map_err(|error| format!("{error:#}"))?;
        *slot = sanitized.clone();
    }
    state.reset_client();
    Ok(sanitized)
}

#[tauri::command]
pub fn has_token() -> bool {
    !config::read_token().is_empty()
}

#[tauri::command]
pub fn clear_token(state: State<'_, AppState>) -> Result<(), String> {
    config::clear_token().map_err(|error| format!("{error:#}"))?;
    state.reset_client();
    Ok(())
}

#[tauri::command]
pub async fn check_token(state: State<'_, AppState>, token: String) -> Result<AccountInfo, String> {
    let trimmed = token.trim().to_string();
    if trimmed.is_empty() {
        return Err("Вставьте токен Яндекс.Музыки.".into());
    }
    let settings = state.settings();
    let client = Client::new(&trimmed, settings.timeout, settings.tries, settings.retry_delay)
        .map_err(|error| format!("{error:#}"))?;
    let info = api::check_token(&client)
        .await
        .map_err(|error| format!("{error:#}"))?;
    config::write_token(&trimmed).map_err(|error| format!("{error:#}"))?;
    state.reset_client();
    remember_username(&state, &info);
    Ok(info)
}

/// Yandex's own web client. A personal OAuth application cannot be registered
/// any more, so the implicit flow through this client id is the only way to sign
/// in without the user digging a token out of DevTools.
const OAUTH_CLIENT: &str = "23cabbbdc6cd418abb4b39c32c41195d";
const LOGIN_WINDOW: &str = "yandex-login";

/// Reads `access_token` out of the address Yandex lands on. The token arrives in
/// the URL fragment: `…/#access_token=AQAA…&token_type=bearer`.
fn token_from_url(url: &str) -> Option<String> {
    // Parsed rather than split by hand: the token is in the fragment, and only the
    // parser can tell a fragment from a query string or a path segment that
    // happens to contain the same text.
    let parsed = tauri::Url::parse(url).ok()?;
    let fragment = parsed.fragment()?;
    let token = fragment
        .split('&')
        .find_map(|pair| pair.strip_prefix("access_token="))?
        .trim();
    // Real tokens are long and URL-safe; anything else is a redirect we should
    // keep following.
    if token.len() > 30 && token.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
        Some(token.to_string())
    } else {
        None
    }
}

/// Opens the Yandex sign-in page in its own window and finishes as soon as the
/// token appears in the address, so the user never copies anything.
#[tauri::command]
pub async fn login_yandex(app: AppHandle, state: State<'_, AppState>) -> Result<AccountInfo, String> {
    if let Some(existing) = app.get_webview_window(LOGIN_WINDOW) {
        let _ = existing.close();
    }
    let target = format!("https://oauth.yandex.ru/authorize?response_type=token&client_id={OAUTH_CLIENT}");
    let address: tauri::Url = target
        .parse()
        .map_err(|error| format!("адрес входа: {error}"))?;

    // A window has to be created on the main thread; this command runs on a
    // worker, so the build is handed over and its result comes back through a
    // channel.
    let (built, built_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let builder_app = app.clone();
    // The sign-in window must use the same WebView2 profile folder as the main
    // one. Without it WebView2 falls back to %LOCALAPPDATA%, which is refused on
    // machines with a redirected profile — the window then fails with os error 5.
    let profile = crate::profile_dir();
    app.run_on_main_thread(move || {
        let mut builder = tauri::WebviewWindowBuilder::new(
            &builder_app,
            LOGIN_WINDOW,
            tauri::WebviewUrl::External(address),
        )
        .title("Вход в Яндекс")
        .inner_size(520.0, 720.0)
        .min_inner_size(420.0, 560.0)
        .center();
        if let Some(profile) = profile {
            builder = builder.data_directory(profile.join("login"));
        }
        let result = builder
            .build()
            .map(|_| ())
            .map_err(|error| format!("окно входа не открылось: {error}"));
        let _ = built.send(result);
    })
    .map_err(|error| format!("не удалось открыть окно входа: {error}"))?;
    // The build happens on the main thread; a timeout keeps a stuck event loop
    // from leaving the button on "Жду вход в Яндекс…" forever.
    built_rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|_| "окно входа не создалось за 15 секунд".to_string())??;

    // The token lands in the fragment, which does not always raise a navigation
    // event, so the address is polled instead of hooked.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let token = loop {
        let Some(window) = app.get_webview_window(LOGIN_WINDOW) else {
            return Err("окно входа закрыто — вход не выполнен".into());
        };
        if std::time::Instant::now() > deadline {
            let _ = window.close();
            return Err("вход не завершён за 5 минут".into());
        }
        if let Ok(current) = window.url() {
            if let Some(token) = token_from_url(current.as_str()) {
                let _ = window.close();
                break token;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    };
    startup_log::write("вход через Яндекс: токен получен");

    // Stored before the account is verified: the sign-in window is already closed,
    // so a network hiccup during the check would otherwise throw away a token the
    // user did just obtain and make them log in again.
    config::write_token(&token).map_err(|error| format!("токен не сохранился: {error:#}"))?;

    let settings = state.settings();
    let client = Client::new(&token, settings.timeout, settings.tries, settings.retry_delay)
        .map_err(|error| format!("{error:#}"))?;
    let info = api::check_token(&client)
        .await
        .map_err(|error| format!("токен сохранён, но аккаунт не подтвердился: {error:#}"))?;
    state.reset_client();
    remember_username(&state, &info);
    Ok(info)
}

/// Fills the "Скачать любимые" username from the account, so the user does not
/// have to copy it by hand. A login is preferred; a token without one gets the
/// uid, which the likes endpoint accepts just as well.
fn remember_username(state: &State<'_, AppState>, info: &AccountInfo) {
    let name = if !info.login.is_empty() {
        info.login.clone()
    } else {
        info.uid.clone()
    };
    if name.is_empty() {
        return;
    }
    // Read, modify and write under one lock, otherwise a concurrent autosave
    // overwrites this with a copy taken before the username was known.
    let Ok(mut slot) = state.settings.lock() else {
        return;
    };
    if !slot.username.trim().is_empty() {
        return;
    }
    slot.username = name;
    if let Err(error) = config::save_settings(&slot) {
        startup_log::write(&format!("имя пользователя не сохранилось: {error:#}"));
    }
}

#[tauri::command]
pub async fn resolve_source(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<SourceInfo, String> {
    let client = state.client().map_err(|error| format!("{error:#}"))?;
    let settings = state.settings();
    // An artist link fetches one album at a time, which takes minutes on a big
    // catalogue; the UI needs to see that it is moving, not frozen.
    let progress_app = app.clone();
    api::resolve_source(
        &client,
        &url,
        settings.only_music,
        settings.stick_to_artist,
        move |done, total| {
            let _ = tauri::Emitter::emit(
                &progress_app,
                "job://resolve",
                ResolveProgress { done, total },
            );
        },
    )
    .await
    .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub async fn load_likes(state: State<'_, AppState>) -> Result<SourceInfo, String> {
    let client = state.client().map_err(|error| format!("{error:#}"))?;
    let settings = state.settings();
    api::likes_source(&client, &settings.username, |_, _| {})
        .await
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub fn start_download(
    app: AppHandle,
    state: State<'_, AppState>,
    track_ids: Vec<String>,
) -> Result<(), String> {
    if track_ids.is_empty() {
        return Err("Ничего не выбрано.".into());
    }
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("Загрузка уже идёт.".into());
    }
    let client = match state.client() {
        Ok(client) => client,
        Err(error) => {
            state.running.store(false, Ordering::SeqCst);
            return Err(format!("{error:#}"));
        }
    };
    let control = JobControl::default();
    if let Ok(mut slot) = state.job.lock() {
        *slot = Some(control.clone());
    }
    let settings = state.settings();
    let running = app.clone();
    tauri::async_runtime::spawn(async move {
        let job = Job::new(app.clone(), client, settings, control, track_ids);
        if let Err(error) = job.run().await {
            let _ = tauri::Emitter::emit(
                &app,
                crate::download::EVENT_LOG,
                crate::download::JobLog {
                    level: "error".into(),
                    message: format!("{error:#}"),
                },
            );
            let _ = tauri::Emitter::emit(
                &app,
                crate::download::EVENT_FINISHED,
                JobProgress {
                    done: 0,
                    total: 0,
                    ok: 0,
                    failed: 0,
                    skipped: 0,
                    running: false,
                    stopping: false,
                    cancelled: false,
                    error: true,
                },
            );
        }
        if let Some(state) = running.try_state::<AppState>() {
            state.running.store(false, Ordering::SeqCst);
            if let Ok(mut slot) = state.job.lock() {
                *slot = None;
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn stop_download(state: State<'_, AppState>) -> bool {
    match state.job.lock() {
        Ok(slot) => match slot.as_ref() {
            Some(control) => {
                control.stop();
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}

#[tauri::command]
pub async fn check_updates() -> Vec<UpdateEntry> {
    // The running version is a compile-time constant; taking it from the UI meant
    // a check could run against the placeholder "0.0.0" before the UI had loaded.
    updates::check_updates(env!("CARGO_PKG_VERSION")).await
}

#[tauri::command]
pub fn environment() -> EnvironmentInfo {
    EnvironmentInfo {
        ffmpeg: ffmpeg::ffmpeg_path().is_some(),
        ffprobe: ffmpeg::ffprobe_path().is_some(),
        ffmpeg_path: ffmpeg::ffmpeg_path()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        config_path: config::config_path().display().to_string(),
        token_path: config::token_path().display().to_string(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    }
}

#[tauri::command]
pub async fn post_process(state: State<'_, AppState>) -> Result<ffmpeg::PostProcessStats, String> {
    let settings = state.settings();
    let dir = settings.target_dir();
    if !ffmpeg::available() {
        return Err("Нужны ffmpeg и ffprobe.".into());
    }
    Ok(tauri::async_runtime::spawn_blocking(move || {
        ffmpeg::post_process(&dir, settings.quarantine_lossy)
    })
    .await
    .map_err(|error| format!("{error}"))?)
}

#[tauri::command]
pub async fn probe_lossless(state: State<'_, AppState>) -> Result<AccountInfo, String> {
    let client = state.client().map_err(|error| format!("{error:#}"))?;
    let info = api::check_token(&client)
        .await
        .map_err(|error| format!("{error:#}"))?;
    // Startup runs this, so the username is filled even for an existing install.
    remember_username(&state, &info);
    Ok(info)
}

#[cfg(test)]
mod login_tests {
    use super::*;

    /// The token arrives in the fragment, which is why the address is parsed
    /// instead of a redirect callback being registered.
    #[test]
    fn reads_a_token_from_the_redirect_address() {
        let url = "https://music.yandex.ru/#access_token=AQAAAAYc1234567890abcdefghijklmnop&token_type=bearer&expires_in=31535645";
        assert_eq!(
            token_from_url(url).as_deref(),
            Some("AQAAAAYc1234567890abcdefghijklmnop")
        );
    }

    #[test]
    fn ignores_pages_that_are_not_the_redirect() {
        assert!(token_from_url("https://oauth.yandex.ru/authorize?response_type=token").is_none());
        assert!(token_from_url("https://music.yandex.ru/").is_none());
        // A truncated or placeholder value must not be treated as a token.
        assert!(token_from_url("https://x/#access_token=short").is_none());
        assert!(token_from_url("https://x/#access_token=/bad/path/aaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_none());
    }
}
