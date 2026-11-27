//! Download engine.
//!
//! A job resolves the selected track ids, then downloads them with a bounded
//! number of parallel workers. Per-track state, overall progress and log lines
//! are pushed to the UI as Tauri events; stopping sets a flag that is checked
//! before every track and inside every streamed chunk.

use crate::api::{self, Client};
use crate::audio;
use crate::config::Settings;
use crate::model::{JobProgress, Track, TrackState, UpgradeProgress};
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
/// Library-wide quality upgrade: per-track progress, then the final summary.
pub const EVENT_UPGRADE: &str = "library://upgrade";
pub const EVENT_UPGRADE_FINISHED: &str = "library://upgrade-finished";

const BATCH: usize = 100;
// Every tick clones the whole track state and wakes the webview; at six workers
// 120 ms produced about fifty events a second, which the interface re-rendered
// for nothing. 250 ms is still smoother than the eye needs for a progress bar.
const PROGRESS_INTERVAL_MS: u128 = 250;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLog {
    pub level: String,
    pub message: String,
}

#[derive(Clone)]
pub struct JobControl {
    cancel: Arc<AtomicBool>,
    /// Set while the user has paused the queue. Workers finish the track in hand
    /// and then wait here, so a pause never leaves a half-written file behind.
    paused: Arc<AtomicBool>,
}

impl Default for JobControl {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl JobControl {
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Blocks while paused. Returns `false` when the job was cancelled meanwhile,
    /// which the caller uses to stop instead of waiting for nothing.
    pub async fn wait_while_paused(&self) -> bool {
        while self.is_paused() {
            if self.is_cancelled() {
                return false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
        !self.is_cancelled()
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
    /// Tracks whose file was written without lyrics. The open database is asked
    /// once for all of them after the downloads finish: asking per track during a
    /// big playlist is what pushed lrclib into its rate limit, and a request that
    /// hits the limit leaves the track without lyrics for good.
    missing_lyrics: Mutex<Vec<(usize, PathBuf)>>,
    /// Every file this job wrote, tagged with the batch it came from, so the
    /// closing pass can write one playlist per album or collection.
    written: Mutex<Vec<(String, PathBuf)>>,
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

    /// Remembers a track whose file was written without lyrics, so the closing
    /// pass can ask the open database once for all of them.
    fn defer_lyrics(&self, index: usize, path: &Path) {
        let mut pending = match self.missing_lyrics.lock() {
            Ok(pending) => pending,
            Err(poisoned) => poisoned.into_inner(),
        };
        pending.push((index, path.to_path_buf()));
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

    async fn download_one(
        self: &Arc<Self>,
        index: usize,
        track: &Track,
        target_dir: &Path,
        source_kind: &str,
        batch_title: &str,
    ) {
        if !track.available {
            self.settle(index, "failed", "трек недоступен в регионе", "");
            return;
        }
        let item = api::track_item(track);
        let pattern = self.settings.pattern_for(source_kind);
        let mut base = target_dir.join(audio::base_path(
            pattern,
            track,
            self.settings.unsafe_path,
        ));
        // A template of bare placeholders can render to nothing (an untitled
        // track with no artist); writing to the folder itself would fail.
        if base.as_os_str().is_empty() || base == target_dir {
            base = target_dir.join(&item.id);
        }

        if self.settings.skip_existing {
            let at_path = [".mp3", ".flac", ".m4a"]
                .iter()
                .map(|suffix| PathBuf::from(format!("{}{suffix}", base.display())))
                .find(|candidate| candidate.is_file());
            let known = match crate::library::find_by_track_id(&item.id) {
                // A row whose file was deleted from disk is not a reason to skip.
                Ok(Some(entry)) if Path::new(&entry.path).is_file() => Some(entry),
                _ => None,
            };
            if let Some(existing) = at_path.or_else(|| known.as_ref().map(|e| PathBuf::from(&e.path)))
            {
                self.settle(index, "skipped", "уже скачан", &existing.display().to_string());
                return;
            }
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

        // Keep the real extension last: the tagger and the demuxer both identify the
        // container by it, and `X.m4a.part` reads as an unknown format.
        let temp = audio::part_path(&final_path);
        if let Err(error) = std::fs::write(&temp, &bytes) {
            self.settle(index, "failed", &format!("не удалось записать файл: {error}"), "");
            return;
        }

        if let Err(error) = std::fs::rename(&temp, &final_path) {
            let _ = std::fs::remove_file(&temp);
            self.settle(index, "failed", &format!("не удалось переименовать файл: {error}"), "");
            return;
        }

        if self.settings.extract_flac && codec == "FLAC" && container == "MP4" {
            // Demuxing reads and writes tens of megabytes: keep it off the async
            // runtime so the other workers keep moving.
            let source = final_path.clone();
            let target = final_path.with_extension("flac");
            let outcome = tauri::async_runtime::spawn_blocking(move || {
                crate::mp4::extract_flac(&source, &target)
            })
            .await;
            match outcome {
                Ok(Ok(_)) => {
                    let extracted = final_path.with_extension("flac");
                    // The demuxer can produce a shorter stream from a container
                    // whose sample tables disagree with each other. Deleting the
                    // original first would leave the user with a truncated file
                    // and no way back, so the result is decoded before the .m4a
                    // is dropped.
                    let check = extracted.clone();
                    let plays = tauri::async_runtime::spawn_blocking(move || {
                        crate::transcode::decodes(&check)
                    })
                    .await
                    .unwrap_or(false);
                    if !plays {
                        let _ = std::fs::remove_file(&extracted);
                        self.log(
                            "warn",
                            format!(
                                "FLAC из {} не прошёл проверку; файл оставлен как .m4a",
                                final_path.display()
                            ),
                        );
                    } else {
                        // The FLAC holds the same audio; keeping the .m4a too only
                        // doubles the size of the library.
                        let _ = std::fs::remove_file(&final_path);
                        final_path = extracted;
                        self.log("ok", format!("FLAC извлечён: {}", final_path.display()));
                    }
                }
                Ok(Err(error)) => self.log(
                    "warn",
                    format!(
                        "FLAC не извлечён из {}; файл оставлен как есть: {error:#}",
                        final_path.display()
                    ),
                ),
                Err(error) => self.log(
                    "warn",
                    format!("FLAC не извлечён из {}: {error}", final_path.display()),
                ),
            }
        }

        // Tags go into the file that is actually kept: tagging the m4a before the
        // FLAC extraction meant the fields had to survive a container change, and
        // the m4a muxer refuses a FLAC stream outright.
        if let Err(error) = self.tag(index, track, &final_path, &item.id).await {
            self.log("warn", format!("Теги не записались для «{}»: {error:#}", item.title));
        }
        self.mp3_copy(&final_path).await;

        // The registry is what the library screen reads and what stops the same
        // track from being downloaded again after a template change.
        self.remember(track, &final_path, codec, info.bitrate);

        let label = if info.bitrate > 0 {
            format!("{codec} {} kbps", info.bitrate)
        } else {
            codec.to_string()
        };
        // Remembered for the closing pass: one playlist per batch, and the
        // history row needs to know how many of each outcome there were.
        if let Ok(mut written) = self.written.lock() {
            if !batch_title.is_empty() {
                written.push((batch_title.to_string(), final_path.clone()));
            }
        }
        self.settle(index, "ok", &label, &final_path.display().to_string());
    }

    /// Writes the finished file into the local index.
    ///
    /// Failure here is logged rather than failing the track: the audio is on disk
    /// and playable, and `scan` can rebuild the row later.
    fn remember(&self, track: &Track, path: &Path, codec: &str, bitrate: u32) {
        let item = api::track_item(track);
        let album = track.albums.first();
        let position = album.and_then(|album| album.track_position.as_ref());
        let entry = crate::library::LibraryTrack {
            track_id: item.id.clone(),
            isrc: track.isrc.clone().unwrap_or_default(),
            title: item.title.clone(),
            artist: item.artist.clone(),
            album_artist: album
                .map(|album| {
                    album
                        .artists
                        .iter()
                        .map(|artist| artist.name.clone())
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default(),
            album: album.map(|album| album.title.clone()).unwrap_or_default(),
            genre: album.and_then(|album| album.genre.clone()).unwrap_or_default(),
            year: album.and_then(|album| album.year),
            track_number: position.and_then(|value| value.index),
            disc_number: position.and_then(|value| value.volume),
            duration_ms: track.duration_ms.unwrap_or(0),
            codec: codec.to_string(),
            bitrate,
            path: path.display().to_string(),
            size_bytes: std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0),
            ..Default::default()
        };
        if let Err(error) = crate::library::record(&entry) {
            self.log(
                "warn",
                format!("Не удалось записать трек в библиотеку: {error:#}"),
            );
        }
    }

    async fn tag(&self, index: usize, track: &Track, path: &Path, track_id: &str) -> Result<()> {
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
        strip_extended(&mut payload, self.settings.extended_tags);

        if self.settings.embed_cover {
            payload.cover = cover_bytes(&self.client, &self.settings, track).await;
        }

        if self.settings.lyrics_format != "none" {
            let want_synced = self.settings.lyrics_format == "lrc";
            // Yandex only here. The open database is a separate pass after the
            // job, so parallel downloads never hammer it.
            let found = crate::lyrics::yandex_only(&self.client, track, track_id, want_synced).await;
            match found {
                Some(lyrics) => {
                    // Embedded so the text travels with the file. A sidecar is only
                    // written when the text really carries timestamps: a `.lrc`
                    // without them is worse than none, players show an empty pane.
                    if lyrics.synced {
                        let _ = std::fs::write(path.with_extension("lrc"), &lyrics.text);
                    }
                    payload.lyrics = Some(lyrics.text);
                    if !lyrics.synced {
                        self.log(
                            "info",
                            format!("Текст песни без таймингов: «{}»", track.title),
                        );
                    }
                }
                None => {
                    if self.settings.lyrics_fallback {
                        // Deferred: this track is retried in the closing pass.
                        self.defer_lyrics(index, path);
                    } else {
                        self.log("info", format!("Текста песни нет: «{}»", track.title));
                    }
                }
            }
        }

        audio::write_tags(path, &payload)?;
        // The cover is already in memory: a copy next to the audio is what a
        // phone shows in a folder view without reading the tag block.
        if self.settings.sidecar_cover {
            crate::sidecar::write_cover(path, payload.cover.as_deref());
        }
        Ok(())
    }

    /// Writes the MP3 copy the settings ask for, beside the original.
    ///
    /// The service has no MP3 stream, so this decodes what was downloaded and
    /// encodes it again. A failure is logged and does not fail the track: the
    /// original is on disk and plays.
    async fn mp3_copy(&self, path: &Path) {
        if !self.settings.mp3_copy || path.extension().and_then(|ext| ext.to_str()) == Some("mp3") {
            return;
        }
        // Reserved through `claim` like any other output path: two tracks whose
        // template drops the unique fields would otherwise write the same
        // `song.mp3` and one copy would silently replace the other.
        let target = self.claim(path.with_extension("mp3"));
        if target.is_file() {
            return;
        }
        let source = path.to_path_buf();
        let target_for_job = target.clone();
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            crate::transcode::encode_mp3(&source, &target_for_job)
        })
        .await;
        match outcome {
            Ok(Ok(bytes)) => self.log(
                "info",
                format!("MP3-копия: {} ({bytes} байт)", target.display()),
            ),
            Ok(Err(error)) => self.log(
                "warn",
                format!("MP3-копия не сделана для {}: {error:#}", path.display()),
            ),
            Err(error) => self.log("warn", format!("MP3-копия не сделана: {error}")),
        }
    }
}

/// The album cover at the size the settings ask for, when the track has one.
async fn cover_bytes(client: &Client, settings: &Settings, track: &Track) -> Option<Vec<u8>> {
    let uri = track
        .albums
        .first()
        .and_then(|album| album.cover_uri.clone())
        .or_else(|| track.cover_uri.clone())?;
    let size = if settings.cover_original {
        "orig".to_string()
    } else {
        format!("{}x{}", settings.cover_resolution, settings.cover_resolution)
    };
    client.cover(&uri, &size).await.ok()
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeOutcome {
    /// `upgraded`, `best` or `unavailable`.
    pub status: String,
    pub detail: String,
    pub path: Option<String>,
}

fn is_lossless(codec: &str) -> bool {
    matches!(codec, "flac" | "flac-mp4")
}

/// Whether the offered stream is actually better than the file on disk.
///
/// A codec that is lossless beats one that is not, whatever the numbers say. Two
/// lossy streams are compared by bitrate, with a small margin so that a track
/// already at 192 kbps is not re-downloaded for a nominal 193.
fn is_better(existing_codec: &str, existing_bitrate: u32, new_codec: &str, new_bitrate: u32) -> bool {
    let existing_lossless = is_lossless(existing_codec) || existing_codec.eq_ignore_ascii_case("flac");
    let new_lossless = is_lossless(new_codec);
    if new_lossless && !existing_lossless {
        return true;
    }
    if existing_lossless && !new_lossless {
        return false;
    }
    new_bitrate > existing_bitrate.saturating_add(existing_bitrate / 20)
}

/// Whether two paths name the same file.
///
/// Case matters on Linux and does not on Windows, and `Song.flac` versus
/// `Song.FLAC` is the difference between replacing a track and deleting it.
fn same_path(left: &Path, right: &Path) -> bool {
    if let (Ok(left), Ok(right)) = (left.canonicalize(), right.canonicalize()) {
        return left == right;
    }
    let as_text = |path: &Path| path.to_string_lossy().to_lowercase();
    as_text(left) == as_text(right)
}

/// Re-downloads one library track at the best quality the account is offered and
/// replaces the file in place.
///
/// This is the same pipeline as a normal download, minus the queue: it writes a
/// temporary file next to the old one, extracts FLAC when asked, carries the
/// existing lyrics over, tags the result, and only then removes the old file. A
/// failure at any step leaves the original untouched.
pub async fn upgrade_track(
    client: &Client,
    settings: &Settings,
    entry: &crate::library::LibraryTrack,
) -> Result<UpgradeOutcome> {
    if entry.track_id.trim().is_empty() {
        anyhow::bail!("у этого файла нет идентификатора трека — качество не проверить");
    }
    let old_path = PathBuf::from(&entry.path);
    if !old_path.is_file() {
        anyhow::bail!("файл не найден: {}", old_path.display());
    }

    let info = client.download_info(&entry.track_id, "lossless").await?;
    if !is_better(&entry.codec, entry.bitrate, &info.codec, info.bitrate) {
        return Ok(UpgradeOutcome {
            status: "best".into(),
            detail: if info.bitrate > 0 {
                format!("уже лучшее: {} {} kbps", info.codec, info.bitrate)
            } else {
                format!("уже лучшее: {}", info.codec)
            },
            path: None,
        });
    }

    let track = client
        .tracks(std::slice::from_ref(&entry.track_id))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("трек не найден в сервисе"))?;

    if info.urls.is_empty() {
        anyhow::bail!("сервер не вернул ссылку на файл");
    }
    let body = client.audio_stream(&info.urls, |_, _| true).await?;
    let bytes = match info.key.as_deref().filter(|key| !key.is_empty()) {
        Some(key) => audio::decrypt(body, key)?,
        None => body,
    };

    let (container, codec) = audio::container_name(&info.codec);
    let extension = audio::extension_for(container, codec);
    // Mutable: extracting FLAC moves the result to a `.flac` name, and the file
    // has to land under the extension its contents actually have.
    let mut final_path = old_path.with_extension(extension.trim_start_matches('.'));

    // Lyrics live in the file, not in the registry: read them before the old file
    // is touched, or the upgrade silently drops them.
    let previous = audio::read_file_metadata(&old_path).unwrap_or_default();
    let mut payload = audio::TagPayload::from_track(
        &track,
        format!(
            "https://music.yandex.ru/album/{}/track/{}",
            track
                .albums
                .first()
                .and_then(|album| api::value_to_id(&album.id))
                .unwrap_or_default(),
            entry.track_id
        ),
        settings.compatibility_level,
    );
    strip_extended(&mut payload, settings.extended_tags);
    if !previous.lyrics.is_empty() {
        payload.lyrics = Some(previous.lyrics);
    }
    if settings.embed_cover {
        payload.cover = cover_bytes(client, settings, &track).await;
    }

    // The new audio is staged beside the old file and moved into place only once
    // it is tagged. Writing straight onto `final_path` used to replace the
    // original before tagging ran, so a failed tag cost the user the track.
    // Staged beside the target, not beside the old file: when the old file is
    // already `.flac`, a staging name derived from it is that very file.
    let mut staged = audio::part_path(&final_path);
    std::fs::write(&staged, &bytes)?;

    if settings.extract_flac && codec == "FLAC" && container == "MP4" {
        // A `.flac` left by an interrupted run would be taken for this
        // extraction's result, so it is cleared first.
        let source = staged.clone();
        let target = staged.with_extension("flac");
        let _ = std::fs::remove_file(&target);
        let target_for_job = target.clone();
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            crate::mp4::extract_flac(&source, &target_for_job)
        })
        .await
        .unwrap_or_else(|error| Err(anyhow::anyhow!("{error}")));
        if outcome.is_ok() {
            let _ = std::fs::remove_file(&staged);
            staged = target;
            final_path = final_path.with_extension("flac");
        }
        // A failed extraction leaves the staged `.m4a`: the same audio, only in
        // its original container. Losing the upgrade is better than losing the
        // track, and the next attempt can try again.
    }

    // Tagging and moving are the two steps that can fail halfway; each cleans up
    // its staged file so a `.part` never turns into a phantom library entry.
    if let Err(error) = audio::write_tags(&staged, &payload) {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&staged, &final_path) {
        let _ = std::fs::remove_file(&staged);
        return Err(error.into());
    }

    // The old file goes only when the new one landed at another path. On Windows
    // `Song.FLAC` and `Song.flac` are one file, and comparing the paths as text
    // used to delete the file that had just been written.
    if !same_path(&final_path, &old_path) {
        let _ = std::fs::remove_file(&old_path);
        let old_lrc = old_path.with_extension("lrc");
        if old_lrc.is_file() {
            let _ = std::fs::rename(&old_lrc, final_path.with_extension("lrc"));
        }
    }

    let record = crate::library::LibraryTrack {
        track_id: entry.track_id.clone(),
        isrc: track.isrc.clone().unwrap_or_default(),
        title: payload.title.clone(),
        artist: payload.artist.join("; "),
        album_artist: payload.album_artist.join("; "),
        album: payload.album.clone(),
        genre: payload.genre.clone().unwrap_or_default(),
        year: payload.year,
        track_number: payload.track_number,
        disc_number: payload.disc_number,
        duration_ms: track.duration_ms.unwrap_or(0),
        codec: codec.to_string(),
        bitrate: info.bitrate,
        path: final_path.display().to_string(),
        size_bytes: std::fs::metadata(&final_path).map(|meta| meta.len()).unwrap_or(0),
        ..Default::default()
    };
    // The row is keyed by path, so a different extension would leave the old row
    // pointing at a file that no longer exists. Compared as paths, not as text:
    // a case-only difference is the same file on Windows.
    if !same_path(&final_path, &old_path) {
        let _ = crate::library::forget(entry.id);
    }
    crate::library::record(&record)?;

    Ok(UpgradeOutcome {
        status: "upgraded".into(),
        detail: if info.bitrate > 0 {
            format!("{codec} {} kbps", info.bitrate)
        } else {
            codec.to_string()
        },
        path: Some(final_path.display().to_string()),
    })
}

/// Re-downloads the whole library at the best quality the account is offered.
///
/// One track at a time: every upgrade streams a file and rewrites it in place, so
/// parallel workers would only compete for the same link and the same disk. The
/// stop flag is checked between tracks — never inside one — so no file is left
/// half-written.
pub async fn upgrade_library(
    app: AppHandle,
    client: Client,
    settings: Settings,
    control: JobControl,
    entries: Vec<crate::library::LibraryTrack>,
) {
    let mut progress = UpgradeProgress {
        total: entries.len(),
        running: true,
        ..Default::default()
    };
    let _ = app.emit(EVENT_UPGRADE, progress.clone());

    for entry in &entries {
        if control.is_cancelled() {
            break;
        }
        progress.title = entry.title.clone();
        let _ = app.emit(EVENT_UPGRADE, progress.clone());
        match upgrade_track(&client, &settings, entry).await {
            Ok(outcome) if outcome.status == "upgraded" => progress.upgraded += 1,
            // "best" means the service has nothing better, which is not a failure.
            Ok(_) => progress.best += 1,
            Err(error) => {
                progress.failed += 1;
                crate::startup_log::write(&format!(
                    "[error] Не удалось заменить «{}»: {error:#}",
                    entry.title
                ));
            }
        }
        progress.done += 1;
        let _ = app.emit(EVENT_UPGRADE, progress.clone());
    }

    progress.running = false;
    progress.cancelled = control.is_cancelled();
    progress.title.clear();
    let _ = app.emit(EVENT_UPGRADE_FINISHED, progress);
}

/// One thing the user put in the queue: a selection and where it came from.
///
/// The kind travels with the tracks because each source has its own path
/// template, and a queue mixes likes with albums and single tracks.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub track_ids: Vec<String>,
    pub source_kind: String,
    /// What the user resolved, used for the history row and the playlist name.
    #[serde(default)]
    pub title: String,
}

/// Drops the tags the settings switch off.
///
/// `from_track` fills everything the service gave; turning the switch off here
/// rather than inside it keeps the payload builder free of a settings type.
fn strip_extended(payload: &mut audio::TagPayload<'_>, keep: bool) {
    if keep {
        return;
    }
    payload.label = None;
    payload.explicit = None;
    payload.composer = None;
}

pub struct Job {
    shared: Arc<Shared>,
    batches: Vec<Batch>,
}

impl Job {
    pub fn new(
        app: AppHandle,
        client: Client,
        settings: Settings,
        control: JobControl,
        batches: Vec<Batch>,
    ) -> Self {
        let total = batches.iter().map(|batch| batch.track_ids.len()).sum();
        Self {
            shared: Arc::new(Shared {
                app,
                client,
                settings,
                control,
                counters: Counters::default(),
                states: Mutex::new(Vec::new()),
                total: AtomicUsize::new(total),
                claimed: Mutex::new(std::collections::HashSet::new()),
                missing_lyrics: Mutex::new(Vec::new()),
                written: Mutex::new(Vec::new()),
            }),
            batches,
        }
    }

    pub async fn run(self) -> Result<JobProgress> {
        let shared = self.shared;
        let wanted: usize = self.batches.iter().map(|batch| batch.track_ids.len()).sum();
        if wanted == 0 {
            anyhow::bail!("Ничего не выбрано");
        }

        // Every track keeps the kind of the batch it came from: the path template
        // is chosen per track, not per job.
        let mut tracks: Vec<Track> = Vec::with_capacity(wanted);
        let mut kinds: Vec<String> = Vec::with_capacity(wanted);
        let mut titles: Vec<String> = Vec::with_capacity(wanted);
        for batch in &self.batches {
            for chunk in batch.track_ids.chunks(BATCH) {
                if shared.control.is_cancelled() {
                    break;
                }
                let fetched = shared.client.tracks(chunk).await?;
                kinds.resize(kinds.len() + fetched.len(), batch.source_kind.clone());
                titles.resize(titles.len() + fetched.len(), batch.title.clone());
                tracks.extend(fetched);
                shared.log(
                    "info",
                    format!("Получены данные о треках: {} из {}", tracks.len(), wanted),
                );
            }
        }
        // A batch whose tracks did not arrive leaves `kinds` short; the download
        // loop indexes both, so it is padded to the same length.
        while kinds.len() < tracks.len() {
            kinds.push("track".to_string());
        }
        while titles.len() < tracks.len() {
            titles.push(String::new());
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
                    id: item.id,
                    title: item.title,
                    artist: item.artist,
                    status: "pending".into(),
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
        let kinds = Arc::new(kinds);
        let titles = Arc::new(titles);
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let shared = shared.clone();
            let tracks = tracks.clone();
            let kinds = kinds.clone();
            let titles = titles.clone();
            let next = next.clone();
            let target_dir = target_dir.clone();
            handles.push(tauri::async_runtime::spawn(async move {
                loop {
                    // A paused queue keeps its workers alive but idle, so
                    // resuming continues the same job instead of starting a new one.
                    if !shared.control.wait_while_paused().await {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    if index >= tracks.len() || shared.control.is_cancelled() {
                        break;
                    }
                    let kind = kinds.get(index).map(String::as_str).unwrap_or("track");
                    let title = titles.get(index).cloned().unwrap_or_default();
                    shared
                        .download_one(index, &tracks[index], &target_dir, kind, &title)
                        .await;
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

        // Lyrics from the open database, one pass, after every file is written.
        // Asking per track while six downloads ran at once is what pushed lrclib
        // into its rate limit; a limited request silently left that track with no
        // lyrics at all. One sequential pass keeps the request rate low and gives
        // every track the same chance.
        if shared.settings.lyrics_format != "none" && shared.settings.lyrics_fallback {
            let pending = {
                let list = match shared.missing_lyrics.lock() {
                    Ok(list) => list,
                    Err(poisoned) => poisoned.into_inner(),
                };
                list.clone()
            };
            if !pending.is_empty() && !shared.control.is_cancelled() {
                let want_synced = shared.settings.lyrics_format == "lrc";
                let total = pending.len();
                shared.log(
                    "info",
                    format!(
                        "Ищу тексты в открытой базе lrclib: {total} трек(ов). Это может занять время"
                    ),
                );
                let mut found = 0usize;
                for (index, path) in pending {
                    if shared.control.is_cancelled() {
                        break;
                    }
                    let Some(track) = tracks.get(index) else { continue };
                    let Some(lyrics) =
                        crate::lyrics::from_open_database(&shared.client, track, want_synced).await
                    else {
                        continue;
                    };
                    if lyrics.synced {
                        let _ = std::fs::write(path.with_extension("lrc"), &lyrics.text);
                    }
                    // The mp4 muxer refuses to copy a FLAC stream, so a lossless
                    // file can reject the tag even though everything else worked.
                    // A `.lrc` next to the track is what players read anyway, so
                    // the text is not lost.
                    match audio::write_lyrics(&path, &lyrics.text) {
                        Ok(()) => found += 1,
                        Err(error) => {
                            let sidecar = path.with_extension("lrc");
                            if std::fs::write(&sidecar, &lyrics.text).is_ok() {
                                found += 1;
                                shared.log(
                                    "info",
                                    format!(
                                        "Текст для «{}» записан рядом файлом .lrc: {error:#}",
                                        track.title
                                    ),
                                );
                            } else {
                                shared.log(
                                    "warn",
                                    format!("Текст не записался в «{}»: {error:#}", track.title),
                                );
                            }
                        }
                    }
                }
                if shared.control.is_cancelled() {
                    // A cancelled pass did not finish, so an "N of M" line would
                    // read as if the database had nothing for the rest.
                    shared.log("info", "Поиск текстов прерван вместе с загрузкой");
                } else {
                    shared.log("ok", format!("Тексты из открытой базы: {found} из {total}"));
                }
            }
        }

        // No folder-wide sweep here. Every track this job wrote was already
        // converted on the spot, so walking the download folder with a decoder again
        // only re-examined files the app did not touch — and the folder is now the
        // user's whole music library.

        // Playlists and history, once, after every file is on disk. Per-track
        // writing would either append to the same list concurrently or drop the
        // tracks a worker had not written yet.
        {
            let written = match shared.written.lock() {
                Ok(written) => written.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            };
            if shared.settings.write_m3u {
                // Grouped by the batch the tracks came from, because a playlist
                // is one album or one collection, not the whole job.
                let mut by_title: Vec<(String, Vec<PathBuf>)> = Vec::new();
                for (title, path) in &written {
                    match by_title.iter_mut().find(|(name, _)| name == title) {
                        Some((_, paths)) => paths.push(path.clone()),
                        None => by_title.push((title.clone(), vec![path.clone()])),
                    }
                }
                for (title, paths) in by_title {
                    if let Some(playlist) = crate::sidecar::write_playlist(&title, &paths) {
                        shared.log(
                            "info",
                            format!("Плейлист: {} ({})", playlist.display(), paths.len()),
                        );
                    }
                }
            }

            // One row per job, not per batch: the counters are totals. The title
            // says so when the queue held more than one source, otherwise the row
            // would claim the whole job was that one album.
            let first_title = self
                .batches
                .first()
                .map(|batch| batch.title.clone())
                .unwrap_or_default();
            let title = if self.batches.len() > 1 {
                format!("{first_title} +{}", self.batches.len() - 1)
            } else {
                first_title
            };
            let entry = crate::library::HistoryEntry {
                id: 0,
                title,
                source_kind: self
                    .batches
                    .first()
                    .map(|batch| batch.source_kind.clone())
                    .unwrap_or_default(),
                total: shared.total.load(Ordering::SeqCst),
                ok: shared.counters.ok.load(Ordering::SeqCst),
                skipped: shared.counters.skipped.load(Ordering::SeqCst),
                failed: shared.counters.failed.load(Ordering::SeqCst),
                finished_at: 0,
            };
            // A history row is a convenience, never a reason to fail a job that
            // already wrote its files.
            if let Err(error) = crate::library::record_history(&entry) {
                shared.log("warn", format!("История не записалась: {error:#}"));
            }
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

    /// The upgrade replaces the file in place, so "same file" has to be decided
    /// by the filesystem, not by the text of the path: a case-only difference is
    /// one file on Windows and the difference between replacing and deleting.
    #[test]
    fn a_case_only_difference_is_the_same_file() {
        let upper = PathBuf::from("a/Song.FLAC");
        let lower = PathBuf::from("a/Song.flac");
        assert!(same_path(&upper, &lower));
        assert!(!same_path(&lower, &PathBuf::from("a/Song.m4a")));
    }

    /// The staging file must never be the file it is meant to replace: an old
    /// `.flac` used to produce a staging name equal to the track itself.
    #[test]
    fn staging_never_collides_with_the_target() {
        for name in ["a/Song.flac", "a/Song.m4a", "a/Song.mp3"] {
            let target = PathBuf::from(name);
            assert_ne!(audio::part_path(&target), target, "{name}");
        }
    }
}
