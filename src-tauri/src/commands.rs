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
pub async fn check_updates(app_version: String) -> Vec<UpdateEntry> {
    updates::check_updates(&app_version).await
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
