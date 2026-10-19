//! Download engine.
//!
//! A job resolves the selected track ids, then downloads them with a bounded
//! number of parallel workers. Per-track state, overall progress and log lines
//! are pushed to the UI as Tauri events; stopping sets a flag that is checked
//! before every track and inside every streamed chunk.

use crate::api::{self, Client};
use crate::audio;
use crate::config::Settings;
use crate::ffmpeg;
use crate::model::{JobProgress, Track, TrackState};
use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

pub const EVENT_TRACK: &str = "job://track";
pub const EVENT_PROGRESS: &str = "job://progress";
pub const EVENT_LOG: &str = "job://log";
pub const EVENT_FINISHED: &str = "job://finished";

const BATCH: usize = 100;
const PROGRESS_INTERVAL_MS: u128 = 120;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLog {
    pub level: String,
    pub message: String,
}

#[derive(Clone, Default)]
pub struct JobControl {
    cancel: Arc<AtomicBool>,
}

impl JobControl {
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct Counters {
    done: AtomicUsize,
    ok: AtomicUsize,
    failed: AtomicUsize,
    skipped: AtomicUsize,
}

struct Shared {
    app: AppHandle,
    client: Client,
    settings: Settings,
    control: JobControl,
    counters: Counters,
    states: Mutex<Vec<TrackState>>,
    total: AtomicUsize,
    /// Paths already handed out to a track in this job. Two tracks that render
    /// to the same name (same artist/title, or a template without #track-id)
    /// would otherwise overwrite each other.
    claimed: Mutex<std::collections::HashSet<PathBuf>>,
}

impl Shared {
    fn emit_progress(&self, running: bool) -> JobProgress {
        let progress = JobProgress {
            done: self.counters.done.load(Ordering::SeqCst),
            total: self.total.load(Ordering::SeqCst),
            ok: self.counters.ok.load(Ordering::SeqCst),
            failed: self.counters.failed.load(Ordering::SeqCst),
            skipped: self.counters.skipped.load(Ordering::SeqCst),
            running,
            stopping: self.control.is_cancelled(),
            cancelled: false,
            error: false,
        };
        let _ = self.app.emit(EVENT_PROGRESS, progress.clone());
        progress
    }

    fn log(&self, level: &str, message: impl Into<String>) {
        let message = message.into();
        // Mirror to the startup log: a toast disappears, and the log is the only
        // thing left to diagnose a failed download with.
        crate::startup_log::write(&format!("[{level}] {message}"));
        let _ = self.app.emit(
            EVENT_LOG,
            JobLog { level: level.to_string(), message },
        );
    }

    fn update(&self, index: usize, mutate: impl FnOnce(&mut TrackState)) {
        let snapshot = {
            let Ok(mut states) = self.states.lock() else { return };
            let Some(state) = states.get_mut(index) else { return };
            mutate(state);
            state.clone()
        };
        let _ = self.app.emit(EVENT_TRACK, snapshot);
    }

    /// Reserves a unique path for this track, adding " (2)", " (3)" … on a clash.
    fn claim(&self, path: PathBuf) -> PathBuf {
        let mut claimed = match self.claimed.lock() {
            Ok(claimed) => claimed,
            Err(poisoned) => poisoned.into_inner(),
        };
        claim_path(&mut claimed, path)
    }

    fn settle(&self, index: usize, status: &str, message: &str, path: &str) {
        let title = {
            let states = match self.states.lock() {
                Ok(states) => states,
                Err(poisoned) => poisoned.into_inner(),
            };
            states.get(index).map(|state| state.title.clone()).unwrap_or_default()
        };
        if status == "failed" {
            // A toast disappears; the log is what is left afterwards.
            self.log(
                "error",
                format!("«{title}»: {message}{}", if path.is_empty() { String::new() } else { format!(" ({path})") }),
            );
        }
        self.update(index, |state| {
            state.status = status.to_string();
            state.message = message.to_string();
            if !path.is_empty() {
                state.path = path.to_string();
            }
        });
        match status {
            "ok" => {
                self.counters.ok.fetch_add(1, Ordering::SeqCst);
            }
            "skipped" => {
                self.counters.skipped.fetch_add(1, Ordering::SeqCst);
            }
            "failed" => {
                self.counters.failed.fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
        }
    }

    async fn download_one(self: &Arc<Self>, index: usize, track: &Track, target_dir: &Path) {
        if !track.available {
            self.settle(index, "failed", "трек недоступен в регионе", "");
            return;
        }
        let item = api::track_item(track);
        let mut base = target_dir.join(audio::base_path(
            &self.settings.path_pattern,
            track,
            self.settings.unsafe_path,
        ));
        // A template of bare placeholders can render to nothing (an untitled
        // track with no artist); writing to the folder itself would fail.
        if base.as_os_str().is_empty() || base == target_dir {
            base = target_dir.join(&item.id);
        }

        if self.settings.skip_existing
            && [".mp3", ".flac", ".m4a"]
                .iter()
                .any(|suffix| PathBuf::from(format!("{}{suffix}", base.display())).is_file())
        {
            self.settle(index, "skipped", "уже скачан", &base.display().to_string());
            return;
        }

        let quality = match self.settings.quality {
            0 => "lq",
            1 => "nq",
            _ => "lossless",
        };
        let info = match self.client.download_info(&item.id, quality).await {
            Ok(info) => info,
            Err(error) => {
                self.settle(index, "failed", &format!("{error:#}"), "");
                return;
            }
        };
        let (container, codec) = audio::container_name(&info.codec);
        let extension = audio::extension_for(container, codec);
        let mut final_path = self.claim(PathBuf::from(format!("{}{extension}", base.display())));

        if let Some(parent) = final_path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                self.settle(index, "failed", &format!("не удалось создать папку: {error}"), "");
                return;
            }
        }

        self.update(index, |state| {
            state.codec = codec.to_string();
            state.bitrate = info.bitrate;
            state.status = "downloading".into();
        });

        if info.urls.is_empty() {
            self.settle(index, "failed", "сервер не вернул ссылку на файл", "");
            return;
        }

        let shared = self.clone();
        let mut last_emit = Instant::now();
        let body = shared
            .client
            .audio_stream(&info.urls, |done, total| {
                if last_emit.elapsed().as_millis() >= PROGRESS_INTERVAL_MS {
                    last_emit = Instant::now();
                    shared.update(index, |state| {
                        state.done_bytes = done;
                        state.total_bytes = total;
                    });
                }
                !shared.control.is_cancelled()
            })
            .await;

        let body = match body {
            Ok(body) => body,
            Err(error) => {
                if self.control.is_cancelled() {
                    self.settle(index, "cancelled", "остановлено", "");
                } else {
                    self.settle(index, "failed", &format!("{error:#}"), "");
                }
                return;
            }
        };

        let bytes = match info.key.as_deref().filter(|key| !key.is_empty()) {
            Some(key) => match audio::decrypt(body, key) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.settle(index, "failed", &format!("расшифровка не удалась: {error}"), "");
                    return;
                }
            },
            None => body,
        };

        // Keep the real extension last: the tagger and ffprobe both identify the
        // container by it, and `X.m4a.part` reads as an unknown format.
        let temp = audio::part_path(&final_path);
        if let Err(error) = std::fs::write(&temp, &bytes) {
            self.settle(index, "failed", &format!("не удалось записать файл: {error}"), "");
            return;
        }

        if let Err(error) = self.tag(track, &temp, &final_path, &item.id).await {
            self.log("warn", format!("Теги не записались для «{}»: {error:#}", item.title));
        }
        if let Err(error) = std::fs::rename(&temp, &final_path) {
            let _ = std::fs::remove_file(&temp);
            self.settle(index, "failed", &format!("не удалось переименовать файл: {error}"), "");
            return;
        }

        if self.settings.extract_flac && codec == "FLAC" && container == "MP4" && ffmpeg::available()
        {
            // ffmpeg is a blocking child process: keep it off the async runtime.
            let source = final_path.clone();
            let outcome = tauri::async_runtime::spawn_blocking(move || ffmpeg::extract_flac(&source))
                .await
                .unwrap_or_else(|_| "failed".to_string());
            match outcome.as_str() {
                "converted" => {
                    ffmpeg::quarantine(&final_path, target_dir);
                    final_path = final_path.with_extension("flac");
                    self.log("ok", format!("FLAC извлечён: {}", final_path.display()));
                }
                "exists" => {
                    ffmpeg::quarantine(&final_path, target_dir);
                    final_path = final_path.with_extension("flac");
                }
                _ => self.log(
                    "warn",
                    format!("Не удалось извлечь FLAC из {}; оставлен .m4a", final_path.display()),
                ),
            }
        }

        let label = if info.bitrate > 0 {
            format!("{codec} {} kbps", info.bitrate)
        } else {
            codec.to_string()
        };
        self.settle(index, "ok", &label, &final_path.display().to_string());
    }

    async fn tag(&self, track: &Track, path: &Path, final_path: &Path, track_id: &str) -> Result<()> {
        let album_id = track
            .albums
            .first()
            .and_then(|album| api::value_to_id(&album.id))
            .unwrap_or_default();
        let mut payload = audio::TagPayload::from_track(
            track,
            format!("https://music.yandex.ru/album/{album_id}/track/{track_id}"),
            self.settings.compatibility_level,
        );

        if self.settings.embed_cover {
            let uri = track
                .albums
                .first()
                .and_then(|album| album.cover_uri.clone())
                .or_else(|| track.cover_uri.clone());
            if let Some(uri) = uri {
                let size = if self.settings.cover_original {
                    "orig".to_string()
                } else {
                    format!("{}x{}", self.settings.cover_resolution, self.settings.cover_resolution)
                };
                if let Ok(bytes) = self.client.cover(&uri, &size).await {
                    payload.cover = Some(bytes);
                }
            }
        }

        if self.settings.lyrics_format != "none" {
            let format = if self.settings.lyrics_format == "lrc" { "LRC" } else { "TEXT" };
            if let Ok(text) = self.client.lyrics(track_id, format).await {
                if self.settings.lyrics_format == "lrc" {
                    // The tag path is the `.part` file; lyrics belong next to
                    // the final track, not next to the temporary one.
                    let _ = std::fs::write(final_path.with_extension("lrc"), text);
                } else {
                    payload.lyrics = Some(text);
                }
            }
        }

        audio::write_tags(path, &payload)
    }
}

/// Reserves a unique path inside `claimed`, adding " (2)", " (3)" … on a clash.
/// Two tracks that render to the same name would otherwise overwrite each other.
fn claim_path(claimed: &mut std::collections::HashSet<PathBuf>, path: PathBuf) -> PathBuf {
    if claimed.insert(path.clone()) {
        return path;
    }
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let extension = path.extension().map(|ext| ext.to_os_string());
    for counter in 2..1000 {
        let mut candidate = path.with_file_name(format!("{stem} ({counter})"));
        if let Some(extension) = &extension {
            candidate.set_extension(extension);
        }
        if claimed.insert(candidate.clone()) {
            return candidate;
        }
    }
    path
}

pub struct Job {
    shared: Arc<Shared>,
    track_ids: Vec<String>,
}

impl Job {
    pub fn new(
        app: AppHandle,
        client: Client,
        settings: Settings,
        control: JobControl,
        track_ids: Vec<String>,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                app,
                client,
                settings,
                control,
                counters: Counters::default(),
                states: Mutex::new(Vec::new()),
                total: AtomicUsize::new(track_ids.len()),
                claimed: Mutex::new(std::collections::HashSet::new()),
            }),
            track_ids,
        }
    }

    pub async fn run(self) -> Result<JobProgress> {
        let shared = self.shared;
        if self.track_ids.is_empty() {
            anyhow::bail!("Ничего не выбрано");
        }

        let mut tracks: Vec<Track> = Vec::with_capacity(self.track_ids.len());
        for chunk in self.track_ids.chunks(BATCH) {
            if shared.control.is_cancelled() {
                break;
            }
            let fetched = shared.client.tracks(chunk).await?;
            tracks.extend(fetched);
            shared.log(
                "info",
                format!("Получены данные о треках: {} из {}", tracks.len(), self.track_ids.len()),
            );
        }

        shared.total.store(tracks.len(), Ordering::SeqCst);
        {
            // A poisoned lock here would abort the whole process (panic=abort),
            // so the states vector is rebuilt defensively instead.
            let mut states = match shared.states.lock() {
                Ok(states) => states,
                Err(poisoned) => poisoned.into_inner(),
            };
            states.clear();
            for (index, track) in tracks.iter().enumerate() {
                let item = api::track_item(track);
                states.push(TrackState {
                    index,
                    total: tracks.len(),
                    id: item.id,
                    title: item.title,
                    artist: item.artist,
                    album: item.album,
                    status: "pending".into(),
                    codec: String::new(),
                    bitrate: 0,
                    done_bytes: 0,
                    total_bytes: 0,
                    path: String::new(),
                    message: String::new(),
                });
            }
            for state in states.iter() {
                let _ = shared.app.emit(EVENT_TRACK, state.clone());
            }
        }
        shared.emit_progress(true);

        let mut target_dir = shared.settings.target_dir();
        if !crate::config::dir_writable(&target_dir) {
            // A locked-down profile refuses writes to the configured folder. Fall
            // back to the portable folder instead of failing after the tracks were
            // already resolved and shown to the user.
            let fallback = crate::config::default_download_dir();
            shared.log(
                "warn",
                format!(
                    "Нет прав на запись в {} — качаю в {}",
                    target_dir.display(),
                    fallback.display()
                ),
            );
            target_dir = fallback;
            if !crate::config::dir_writable(&target_dir) {
                anyhow::bail!(
                    "нет папки, доступной для записи: {} и {}",
                    target_dir.display(),
                    shared.settings.target_dir().display()
                );
            }
        }

        let next = Arc::new(AtomicUsize::new(0));
        let workers = shared.settings.concurrent.clamp(1, 6) as usize;
        let tracks = Arc::new(tracks);
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let shared = shared.clone();
            let tracks = tracks.clone();
            let next = next.clone();
            let target_dir = target_dir.clone();
            handles.push(tauri::async_runtime::spawn(async move {
                loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    if index >= tracks.len() || shared.control.is_cancelled() {
                        break;
                    }
                    shared.download_one(index, &tracks[index], &target_dir).await;
                    shared.counters.done.fetch_add(1, Ordering::SeqCst);
                    shared.emit_progress(true);

                    // Pause between tracks: the API throttles a burst of requests.
                    // Skipped after the last track and cut short by a stop request,
                    // so finishing never hangs on a long delay.
                    let more = next.load(Ordering::SeqCst) < tracks.len();
                    if more && shared.settings.delay > 0 && !shared.control.is_cancelled() {
                        let delay = std::time::Duration::from_secs(shared.settings.delay);
                        tokio::time::sleep(delay).await;
                    }
                }
            }));
        }
        for handle in handles {
            let _ = handle.await;
        }

        let final_progress = JobProgress {
            done: shared.counters.done.load(Ordering::SeqCst),
            total: shared.total.load(Ordering::SeqCst),
            ok: shared.counters.ok.load(Ordering::SeqCst),
            failed: shared.counters.failed.load(Ordering::SeqCst),
            skipped: shared.counters.skipped.load(Ordering::SeqCst),
            running: false,
            stopping: false,
            cancelled: shared.control.is_cancelled(),
            error: false,
        };
        let _ = shared.app.emit(EVENT_FINISHED, final_progress.clone());

        // Maintenance sweep of the whole folder runs after the job is reported
        // finished: it walks every file with ffprobe and must neither block the
        // async runtime nor delay the "done" the user is waiting for.
        if shared.settings.extract_flac && ffmpeg::available() {
            shared.log("info", "Проверяю папку на настоящий FLAC…");
            let root = target_dir.clone();
            let quarantine_lossy = shared.settings.quarantine_lossy;
            let stats = tauri::async_runtime::spawn_blocking(move || {
                ffmpeg::post_process(&root, quarantine_lossy)
            })
            .await
            .unwrap_or_default();
            if stats.converted > 0 {
                shared.log("ok", format!("Извлечено FLAC из .m4a: {}", stats.converted));
            }
            if stats.quarantined > 0 {
                shared.log(
                    "info",
                    format!("Старые lossy-файлы убраны в {}: {}", ffmpeg::BACKUP_DIR, stats.quarantined),
                );
            }
            if stats.failed > 0 {
                shared.log("warn", format!("Не удалось перепаковать файлов: {}", stats.failed));
            }
        }

        Ok(final_progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_names_get_distinct_paths() {
        let mut claimed = std::collections::HashSet::new();
        let first = claim_path(&mut claimed, PathBuf::from("/music/song.m4a"));
        let second = claim_path(&mut claimed, PathBuf::from("/music/song.m4a"));
        let third = claim_path(&mut claimed, PathBuf::from("/music/song.m4a"));
        assert_eq!(first, PathBuf::from("/music/song.m4a"));
        assert_eq!(second, PathBuf::from("/music/song (2).m4a"));
        assert_eq!(third, PathBuf::from("/music/song (3).m4a"));
    }

    #[test]
    fn claim_keeps_the_extension() {
        let mut claimed = std::collections::HashSet::new();
        claim_path(&mut claimed, PathBuf::from("a/track.flac"));
        let second = claim_path(&mut claimed, PathBuf::from("a/track.flac"));
        assert_eq!(second.extension().and_then(|e| e.to_str()), Some("flac"));
        assert_eq!(second.file_stem().and_then(|s| s.to_str()), Some("track (2)"));
    }
}
