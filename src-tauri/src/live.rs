//! Live self-check of the download path, used by the integration test.
//!
//! The app can fail at four different points on the way from a track id to a
//! finished file, and a UI toast hides which one. This walks the same steps and
//! records each result, so a failure names its step.

use crate::api::{self, Client};
use crate::audio;
use crate::config;

pub struct LiveReport {
    pub ok: bool,
    pub lines: Vec<String>,
}

/// Runs the real path for one track. `quality` is 0 = lq, 1 = nq, 2 = lossless.
pub async fn run_live_check(track_id: &str, quality: u8) -> LiveReport {
    let mut lines = Vec::new();
    let token = config::read_token();
    if token.is_empty() {
        lines.push("токен не найден в ~/.ym-dl-token".to_string());
        return LiveReport { ok: false, lines };
    }
    lines.push(format!("токен: {} символов", token.len()));

    let client = match Client::new(&token, 20, 3, 2) {
        Ok(client) => client,
        Err(error) => {
            lines.push(format!("клиент не создался: {error:#}"));
            return LiveReport { ok: false, lines };
        }
    };

    let account = match api::check_token(&client).await {
        Ok(account) => {
            lines.push(format!(
                "аккаунт: uid={} plus={} lossless_ok={:?} codec={} bitrate={}",
                account.uid,
                account.has_plus,
                account.lossless_ok,
                account.probe_codec,
                account.probe_bitrate
            ));
            account
        }
        Err(error) => {
            lines.push(format!("check_token упал: {error:#}"));
            return LiveReport { ok: false, lines };
        }
    };
    if !account.note.is_empty() {
        lines.push(format!("примечание API: {}", account.note));
    }

    let tracks = match client.tracks(std::slice::from_ref(&track_id.to_string())).await {
        Ok(tracks) => tracks,
        Err(error) => {
            lines.push(format!("tracks упал: {error:#}"));
            return LiveReport { ok: false, lines };
        }
    };
    let Some(track) = tracks.first() else {
        lines.push("сервер не вернул трек".to_string());
        return LiveReport { ok: false, lines };
    };
    let item = api::track_item(track);
    lines.push(format!("трек: {} — {}", item.artist, item.title));
    if !item.available {
        lines.push("трек помечен как недоступный".to_string());
        return LiveReport { ok: false, lines };
    }

    let quality_name = match quality {
        0 => "lq",
        1 => "nq",
        _ => "lossless",
    };
    let info = match client.download_info(&item.id, quality_name).await {
        Ok(info) => {
            lines.push(format!(
                "get-file-info: codec={} bitrate={} urls={} key={}",
                info.codec,
                info.bitrate,
                info.urls.len(),
                info.key.as_deref().map(|key| key.len()).unwrap_or(0)
            ));
            info
        }
        Err(error) => {
            lines.push(format!("get-file-info упал: {error:#}"));
            return LiveReport { ok: false, lines };
        }
    };

    let body = match client.audio_stream(&info.urls, |_, _| true).await {
        Ok(body) => {
            lines.push(format!("аудио скачано: {} байт", body.len()));
            body
        }
        Err(error) => {
            lines.push(format!("audio_stream упал: {error:#}"));
            return LiveReport { ok: false, lines };
        }
    };

    let bytes = match info.key.as_deref().filter(|key| !key.is_empty()) {
        Some(key) => match audio::decrypt(body, key) {
            Ok(bytes) => {
                lines.push("расшифровано".to_string());
                bytes
            }
            Err(error) => {
                lines.push(format!("расшифровка упала: {error:#}"));
                return LiveReport { ok: false, lines };
            }
        },
        None => {
            lines.push("ключ не пришёл — файл без шифрования".to_string());
            body
        }
    };

    let (container, codec) = audio::container_name(&info.codec);
    let extension = audio::extension_for(container, codec);
    lines.push(format!("контейнер: {container}, кодек: {codec}, расширение: {extension}"));

    // Write to the workspace, never to the user's music folder: this is a probe.
    let dir = config::exe_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("live-check");
    let _ = std::fs::create_dir_all(&dir);
    let final_path = dir.join(format!("{}{extension}", item.id));
    // Exactly the naming the download engine uses: the real extension has to stay
    // last or the tagger cannot identify the container.
    let temp = final_path.with_file_name(format!(".{}.part{extension}", item.id));
    if let Err(error) = std::fs::write(&temp, &bytes) {
        lines.push(format!("запись упала: {error}"));
        return LiveReport { ok: false, lines };
    }
    lines.push(format!("записано: {}", temp.display()));

    let mut payload = audio::TagPayload::from_track(
        track,
        format!("https://music.yandex.ru/track/{track_id}"),
        1,
    );

    // Cover, same call the download engine makes.
    if let Some(uri) = track
        .albums
        .first()
        .and_then(|album| album.cover_uri.clone())
        .or_else(|| track.cover_uri.clone())
    {
        match client.cover(&uri, "400x400").await {
            Ok(bytes) => {
                lines.push(format!("обложка скачана: {} байт", bytes.len()));
                payload.cover = Some(bytes);
            }
            Err(error) => lines.push(format!("обложка не скачалась: {error:#}")),
        }
    } else {
        lines.push("у трека нет coverUri".to_string());
    }

    // Lyrics, same call the download engine makes. "No lyrics found" is a normal
    // server answer for a track without a text, not a failure.
    match client.lyrics(track_id, "TEXT").await {
        Ok(text) => {
            lines.push(format!("лирикс скачан: {} символов", text.chars().count()));
            payload.lyrics = Some(text);
        }
        Err(error) if format!("{error:#}").contains("No lyrics found") => {
            lines.push("лирикса у трека нет на сервере — это нормально".to_string());
        }
        Err(error) => lines.push(format!("лирикс не скачался: {error:#}")),
    }

    if let Err(error) = audio::write_tags(&temp, &payload) {
        lines.push(format!("теги не записались: {error:#}"));
        return LiveReport { ok: false, lines };
    }
    if let Err(error) = std::fs::rename(&temp, &final_path) {
        lines.push(format!("переименование упало: {error}"));
        return LiveReport { ok: false, lines };
    }

    // Read the result back: writing tags is only useful if they are there.
    let check = audio::read_tags(&final_path);
    match check {
        Ok(summary) => {
            lines.push(format!("проверка файла: {summary}"));
            // Cover must always land; lyrics only when the track has a text.
            if !summary.contains("обложка=1") {
                return LiveReport { ok: false, lines };
            }
            let had_lyrics = payload.lyrics.is_some();
            if had_lyrics && !summary.contains("лирикс=да") {
                return LiveReport { ok: false, lines };
            }
            // Tags are worthless on a file that no longer plays.
            if !crate::ffmpeg::decodes(&final_path) {
                lines.push("файл после записи тегов не декодируется".to_string());
                return LiveReport { ok: false, lines };
            }
            lines.push("файл декодируется целиком".to_string());
        }
        Err(error) => {
            lines.push(format!("файл не перечитался: {error:#}"));
            return LiveReport { ok: false, lines };
        }
    }

    LiveReport { ok: true, lines }
}
