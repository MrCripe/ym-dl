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
    let path = dir.join(format!("{}{extension}", item.id));
    match std::fs::write(&path, &bytes) {
        Ok(()) => lines.push(format!("записано: {}", path.display())),
        Err(error) => {
            lines.push(format!("запись упала: {error}"));
            return LiveReport { ok: false, lines };
        }
    }

    let payload = audio::TagPayload::from_track(track, String::new(), 1);
    match audio::write_tags(&path, &payload) {
        Ok(()) => lines.push("теги записаны".to_string()),
        Err(error) => lines.push(format!("теги не записались: {error:#}")),
    }

    LiveReport { ok: true, lines }
}
