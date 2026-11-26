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

    pub fn settings(&self) -> Settings {
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
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
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
    // A new download folder must also become readable for playback, or every
    // track in the library suddenly refuses to play.
    crate::grant_audio_scope(&app);
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
    let mut info = api::resolve_source(
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
    .map_err(|error| format!("{error:#}"))?;

    // Preview before download: the cover, the year and what quality the service
    // actually answers for this account. One extra request per resolve is worth
    // it — otherwise the first sign of a problem is a finished download.
    if let Some(first) = info.tracks.first().cloned() {
        info.year = first.year;
        if let Ok(fresh) = client.tracks(std::slice::from_ref(&first.id)).await {
            if let Some(track) = fresh.first() {
                let uri = track
                    .albums
                    .first()
                    .and_then(|album| album.cover_uri.clone())
                    .or_else(|| track.cover_uri.clone());
                if let Some(uri) = uri {
                    if let Ok(bytes) = client.cover(&uri, "200x200").await {
                        info.cover = Some(crate::image::data_url(&bytes));
                    }
                }
                if let Ok(details) = client.download_info(&first.id, "lossless").await {
                    info.quality = details.codec;
                }
            }
        }
    }
    Ok(info)
}

/// Catalogue search, grouped the way the results screen shows it.
#[tauri::command]
pub async fn search_music(
    state: State<'_, AppState>,
    text: String,
) -> Result<SearchResults, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(SearchResults::default());
    }
    let client = state.client().map_err(|error| format!("{error:#}"))?;
    // Four tracks per section fill the two columns the results screen uses, plus
    // one row of each shelf; the cover cap covers all of them.
    client
        .search(trimmed, 8, 8 + 6 + 6 + 6)
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
    batches: Vec<crate::download::Batch>,
) -> Result<(), String> {
    // The queue and the current selection arrive together: the user adds
    // everything first, then presses one button.
    let batches: Vec<crate::download::Batch> = batches
        .into_iter()
        .filter(|batch| !batch.track_ids.is_empty())
        .collect();
    if batches.is_empty() {
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
        let job = Job::new(app.clone(), client, settings, control, batches);
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
        portable: config::is_portable(),
        // Where the token actually ended up, so the settings screen can warn when
        // it is a file on disk rather than a system password store.
        token_file: config::token_home() == config::TokenHome::File,
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    }
}

/// Opens the download folder in the system file manager.
///
/// Called from Rust rather than from the frontend: a path opened through the
/// webview would need a permission scope listing every possible folder, while the
/// backend already knows the configured one.
///
/// The opener plugin's `open_path` goes through `ShellExecute`, which is what a
/// double click does. Starting `explorer.exe` by hand looked equivalent and was
/// not: on a machine where the shell is already running, the second copy fails to
/// initialise its DLLs and Windows shows a 0xc0000142 error box instead of the
/// folder.
#[tauri::command]
pub fn open_download_dir(state: State<'_, AppState>) -> Result<(), String> {
    let dir = state.settings().target_dir();
    std::fs::create_dir_all(&dir).map_err(|error| format!("не удалось создать папку: {error}"))?;
    tauri_plugin_opener::open_path(&dir, None::<&str>)
        .map_err(|error| format!("не удалось открыть папку {}: {error}", dir.display()))
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

// --- library --------------------------------------------------------------

/// Everything already downloaded, ordered and optionally filtered.
#[tauri::command]
pub async fn library_list(
    sort: String,
    descending: bool,
    filter: String,
) -> Result<Vec<crate::library::LibraryTrack>, String> {
    // Async so the work leaves the window thread: a synchronous command runs
    // inside the IPC handler, and opening SQLite there stalls the interface.
    tauri::async_runtime::spawn_blocking(move || {
        crate::library::list(&sort, descending, &filter).map_err(|error| format!("{error:#}"))
    })
    .await
    .map_err(|error| format!("{error}"))?
}

/// Applies edited tags to the file first and to the registry second.
///
/// The file is the source of truth: if writing it fails the row must not claim a
/// change that never reached disk.
#[tauri::command]
pub async fn library_update(
    state: State<'_, AppState>,
    id: i64,
    patch: crate::library::TagPatch,
) -> Result<crate::library::LibraryTrack, String> {
    let level = state.settings().compatibility_level;
    // Rewriting a tag copies the file and may run ffmpeg for an MP4 container.
    tauri::async_runtime::spawn_blocking(move || update_tags_of(id, &patch, level))
        .await
        .map_err(|error| format!("{error}"))?
}

fn update_tags_of(
    id: i64,
    patch: &crate::library::TagPatch,
    compatibility_level: u8,
) -> Result<crate::library::LibraryTrack, String> {
    let entry = crate::library::get(id)
        .map_err(|error| format!("{error:#}"))?
        .ok_or_else(|| "запись не найдена".to_string())?;
    let path = std::path::PathBuf::from(&entry.path);
    if !path.is_file() {
        return Err(format!("файл не найден: {}", path.display()));
    }

    let current = crate::audio::read_file_metadata(&path).map_err(|error| format!("{error:#}"))?;
    let pick = |next: &Option<String>, previous: &str| {
        next.clone().unwrap_or_else(|| previous.to_string())
    };
    let patch = patch.clone();
    let payload = crate::audio::TagPayload {
        title: pick(&patch.title, &current.title),
        artist: vec![pick(&patch.artist, &current.artist)],
        album_artist: vec![pick(&patch.album_artist, &current.album_artist)],
        album: pick(&patch.album, &current.album),
        year: patch.year.or(current.year),
        track_number: patch.track_number.or(current.track_number),
        disc_number: patch.disc_number.or(current.disc_number),
        genre: Some(pick(&patch.genre, &current.genre)),
        isrc: Some(current.isrc.clone()).filter(|code| !code.is_empty()),
        // Carried over from the file: a tag rewrite must not drop them.
        cover: crate::audio::read_cover(&path),
        lyrics: Some(current.lyrics.clone()).filter(|text| !text.is_empty()),
        url: current.comment.clone(),
        compatibility_level,
        _marker: std::marker::PhantomData,
    };
    crate::audio::write_tags(&path, &payload).map_err(|error| format!("{error:#}"))?;

    let updated = crate::library::update_metadata(id, &patch)
        .map_err(|error| format!("{error:#}"))?
        .ok_or_else(|| "запись не найдена".to_string())?;
    Ok(updated)
}

/// Deletes the audio file, its `.lrc` sidecar and the registry row.
///
/// The file goes first: dropping the row before the file is gone leaves an
/// orphan the user cannot remove again, because the interface only knows rows.
#[tauri::command]
pub async fn library_delete(id: i64) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || delete_file_of(id))
        .await
        .map_err(|error| format!("{error}"))?
}

fn delete_file_of(id: i64) -> Result<String, String> {
    let entry = crate::library::get(id)
        .map_err(|error| format!("{error:#}"))?
        .ok_or_else(|| "запись не найдена".to_string())?;
    let path = std::path::PathBuf::from(&entry.path);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        // Already gone: the row is stale, which is exactly what we want to clear.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("не удалось удалить файл: {error}")),
    }
    let _ = std::fs::remove_file(path.with_extension("lrc"));
    crate::library::forget(id).map_err(|error| format!("{error:#}"))?;
    Ok(entry.path)
}

/// Rebuilds the index from the download folder.
#[tauri::command]
pub async fn library_scan(state: State<'_, AppState>) -> Result<crate::library::ScanReport, String> {
    // Walking the folder reads every file's tags; on the window thread that is a
    // visible freeze.
    let root = state.settings().target_dir();
    tauri::async_runtime::spawn_blocking(move || {
        crate::library::scan(&root).map_err(|error| format!("{error:#}"))
    })
    .await
    .map_err(|error| format!("{error}"))?
}

/// Re-downloads one track at the best quality the account is offered.
#[tauri::command]
pub async fn library_upgrade(
    state: State<'_, AppState>,
    id: i64,
) -> Result<crate::download::UpgradeOutcome, String> {
    let entry = crate::library::get(id)
        .map_err(|error| format!("{error:#}"))?
        .ok_or_else(|| "запись не найдена".to_string())?;
    let client = state.client().map_err(|error| format!("{error:#}"))?;
    let settings = state.settings();
    crate::download::upgrade_track(&client, &settings, &entry)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// The embedded cover of one library file, as a data URL.
///
/// Read lazily per row instead of shipping 50 images with the list: only the
/// visible rows ask, and the frontend keeps the answer for the session.
#[tauri::command]
pub async fn library_cover(id: i64) -> Result<Option<String>, String> {
    // Called once per visible row, and each call parses a whole tag block: the
    // one command that must never run on the window thread.
    tauri::async_runtime::spawn_blocking(move || cover_of(id))
        .await
        .map_err(|error| format!("{error}"))?
}

fn cover_of(id: i64) -> Result<Option<String>, String> {
    let entry = crate::library::get(id)
        .map_err(|error| format!("{error:#}"))?
        .ok_or_else(|| "запись не найдена".to_string())?;
    let path = std::path::PathBuf::from(&entry.path);
    if !path.is_file() {
        return Ok(None);
    }
    Ok(crate::audio::read_cover(&path).map(|bytes| crate::image::data_url(&bytes)))
}

/// Replaces the embedded cover with the image at `path`.
#[tauri::command]
pub async fn library_set_cover(
    state: State<'_, AppState>,
    id: i64,
    path: String,
) -> Result<(), String> {
    let level = state.settings().compatibility_level;
    // Copying the file and running ffmpeg is not work for the window thread.
    tauri::async_runtime::spawn_blocking(move || set_cover_of(id, &path, level))
        .await
        .map_err(|error| format!("{error}"))?
}

fn set_cover_of(id: i64, path: &str, compatibility_level: u8) -> Result<(), String> {
    let entry = crate::library::get(id)
        .map_err(|error| format!("{error:#}"))?
        .ok_or_else(|| "запись не найдена".to_string())?;
    let file = std::path::PathBuf::from(&entry.path);
    if !file.is_file() {
        return Err(format!("файл не найден: {}", file.display()));
    }
    // The size is checked before reading: a path chosen by mistake (an ISO, a
    // video) must not be loaded into memory first and rejected afterwards.
    let size = std::fs::metadata(path)
        .map_err(|error| format!("не удалось прочитать картинку: {error}"))?
        .len();
    if size > 12 * 1024 * 1024 {
        return Err("картинка больше 12 МБ".to_string());
    }
    let image = std::fs::read(path)
        .map_err(|error| format!("не удалось прочитать картинку: {error}"))?;
    // Anything that is not a picture would be stored as a broken cover and shown
    // back through `library_cover`, so the format is checked by its magic bytes.
    let is_picture = image.starts_with(&[0x89, b'P', b'N', b'G'])
        || image.starts_with(&[0xFF, 0xD8])
        || image.starts_with(b"GIF8")
        || (image.starts_with(b"RIFF") && image.get(8..12) == Some(b"WEBP"));
    if !is_picture {
        return Err("это не картинка: нужен PNG, JPEG, GIF или WebP".to_string());
    }

    // Everything else in the tag is read back from the file and written again:
    // the tag block is replaced as a whole, so an unread field would be lost.
    let current = crate::audio::read_file_metadata(&file).map_err(|error| format!("{error:#}"))?;
    let payload = crate::audio::TagPayload {
        title: current.title,
        artist: vec![current.artist],
        album_artist: vec![current.album_artist],
        album: current.album,
        year: current.year,
        track_number: current.track_number,
        disc_number: current.disc_number,
        genre: Some(current.genre),
        isrc: Some(current.isrc).filter(|code| !code.is_empty()),
        cover: Some(image),
        lyrics: Some(current.lyrics).filter(|text| !text.is_empty()),
        url: current.comment,
        compatibility_level,
        _marker: std::marker::PhantomData,
    };
    crate::audio::write_tags(&file, &payload).map_err(|error| format!("{error:#}"))
}
