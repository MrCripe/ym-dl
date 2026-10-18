//! Yandex Music HTTP client.
//!
//! Everything the UI needs goes through this module: account checks, likes,
//! playlists, albums, artists, the signed `get-file-info` call that yields
//! audio URLs, and lyrics.
//!
//! The API throttles aggressively, so every call goes through `request()`, which
//! retries 429/5xx with an exponential backoff capped by `settings.tries`.

use crate::model::*;
use crate::sign;
use anyhow::{anyhow, bail, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

const BASE: &str = "https://api.music.yandex.net";
const TRACK_BATCH: usize = 100;
/// Ceiling for `settings.tries`: the setting allows "forever", the UI must not.
const MAX_ATTEMPTS: u32 = 6;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    token: String,
    tries: u32,
    retry_delay: Duration,
}

impl Client {
    pub fn new(token: &str, timeout: u64, tries: u32, retry_delay: u64) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout.max(1)))
            .connect_timeout(Duration::from_secs(15))
            .user_agent(sign::USER_AGENT)
            .build()
            .context("не удалось создать HTTP-клиент")?;
        Ok(Self {
            http,
            token: token.trim().to_string(),
            tries,
            retry_delay: Duration::from_secs(retry_delay),
        })
    }

    fn mobile_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(value) = format!("OAuth {}", self.token).parse() {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        if let Ok(value) = sign::MOBILE_CLIENT.parse() {
            headers.insert("X-Yandex-Music-Client", value);
        }
        headers
    }

    /// How many times one request may be attempted. `tries == 0` means "retry
    /// forever" in the loader's vocabulary, capped here so a dead network cannot
    /// hang a command for minutes.
    fn attempts(&self) -> u32 {
        if self.tries == 0 {
            MAX_ATTEMPTS
        } else {
            self.tries.min(MAX_ATTEMPTS).max(1)
        }
    }

    /// Runs one request with retries. `build` is called per attempt so signed
    /// parameters get a fresh timestamp every time.
    async fn request<T, F>(&self, build: F) -> Result<T>
    where
        T: DeserializeOwned,
        F: Fn() -> reqwest::RequestBuilder,
    {
        let max_attempts = self.attempts();
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let response = build().send().await;
            match response {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        let text = response.text().await.unwrap_or_default();
                        let envelope: Value = serde_json::from_str(&text)
                            .with_context(|| format!("ответ API не JSON: {}", truncate(&text)))?;
                        let result = envelope.get("result").cloned().unwrap_or(Value::Null);
                        return serde_json::from_value(result)
                            .with_context(|| format!("неожиданная структура ответа: {}", truncate(&text)));
                    }
                    if status.as_u16() == 401 {
                        bail!("API отклонил токен (401). Проверьте токен в настройках.");
                    }
                    let retryable = status.as_u16() == 429 || status.is_server_error();
                    if !retryable || attempt >= max_attempts {
                        let body = response.text().await.unwrap_or_default();
                        bail!("HTTP {}: {}", status.as_u16(), truncate(&body));
                    }
                }
                Err(error) => {
                    if attempt >= max_attempts || error.is_builder() {
                        return Err(anyhow!(describe_network_error(&error)));
                    }
                }
            }
            // ponytail: фиксированная экспонента без джиттера — API не требует большего.
            let backoff = self.retry_delay * 2u32.pow(attempt.saturating_sub(1));
            tokio::time::sleep(backoff.max(Duration::from_millis(400))).await;
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        self.request(|| {
            let mut builder = self.http.get(format!("{BASE}{path}")).headers(self.mobile_headers());
            if !query.is_empty() {
                builder = builder.query(query);
            }
            builder
        })
        .await
    }

    async fn post_form<T: DeserializeOwned>(&self, path: &str, form: Vec<(&str, String)>) -> Result<T> {
        self.request(|| {
            self.http
                .post(format!("{BASE}{path}"))
                .headers(self.mobile_headers())
                .form(&form)
        })
        .await
    }

    pub async fn status(&self) -> Result<Status> {
        self.get("/account/status", &[]).await
    }

    pub async fn likes_tracks(&self, user: Option<&str>) -> Result<Vec<String>> {
        let path = match user.map(str::trim).filter(|value| !value.is_empty()) {
            Some(name) => format!("/users/{name}/likes/tracks"),
            // `0` is not a user id: the server only understands a real uid here.
            None => {
                let uid = self
                    .status()
                    .await
                    .ok()
                    .and_then(|status| status.account)
                    .and_then(|account| value_to_id(&account.uid))
                    .ok_or_else(|| anyhow!("не удалось определить uid аккаунта"))?;
                format!("/users/{uid}/likes/tracks")
            }
        };
        let response: LikesResponse = self.get(&path, &[]).await?;
        Ok(response
            .library
            .tracks
            .iter()
            .filter_map(|track| value_to_id(&track.id))
            .collect())
    }

    pub async fn tracks(&self, ids: &[String]) -> Result<Vec<Track>> {
        let mut result = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(TRACK_BATCH) {
            let form: Vec<(&str, String)> = chunk.iter().map(|id| ("track-ids", id.clone())).collect();
            let mut page: Vec<Track> = self.post_form("/tracks", form).await?;
            result.append(&mut page);
        }
        Ok(result)
    }

    pub async fn playlist(&self, owner: Option<&str>, kind: &str) -> Result<Playlist> {
        match owner {
            Some(owner) => {
                self.get(&format!("/users/{owner}/playlists/{kind}"), &[]).await
            }
            None => self.get(&format!("/playlist/{kind}"), &[]).await,
        }
    }

    pub async fn album(&self, id: &str) -> Result<Album> {
        self.get(&format!("/albums/{id}/with-tracks"), &[]).await
    }

    pub async fn artist_albums(&self, id: &str, page: u32) -> Result<ArtistAlbums> {
        self.get(
            &format!("/artists/{id}/direct-albums"),
            &[("page", page.to_string()), ("page-size", "50".to_string()), ("sort-by", "year".to_string())],
        )
        .await
    }

    pub async fn artist_name(&self, id: &str) -> Option<String> {
        #[derive(serde::Deserialize)]
        struct Brief {
            #[serde(default)]
            artist: Option<Artist>,
        }
        let brief: Result<Brief> = self.get(&format!("/artists/{id}/brief-info"), &[]).await;
        brief.ok().and_then(|b| b.artist).map(|a| a.name).filter(|n| !n.is_empty())
    }

    /// Signed `get-file-info`: the only call that can hand out lossless.
    pub async fn download_info(&self, track_id: &str, quality: &str) -> Result<DownloadInfo> {
        let info: FileInfo = self
            .request(|| {
                let timestamp = sign::now();
                let signature = sign::file_info_sign(track_id, quality, "encraw", timestamp);
                let mut headers = self.mobile_headers();
                if let Ok(value) = sign::DESKTOP_CLIENT.parse() {
                    headers.insert("X-Yandex-Music-Client", value);
                }
                self.http
                    .get(format!("{BASE}/get-file-info"))
                    .headers(headers)
                    .query(&[
                        ("ts", timestamp.to_string()),
                        ("trackId", track_id.to_string()),
                        ("quality", quality.to_string()),
                        ("codecs", sign::CODECS.join(",")),
                        ("transports", "encraw".to_string()),
                        ("sign", signature),
                    ])
            })
            .await?;
        if info.download_info.urls.is_empty() {
            bail!("сервер не вернул ссылку на файл для трека {track_id} ({quality})");
        }
        Ok(info.download_info)
    }

    pub async fn lyrics(&self, track_id: &str, format: &str) -> Result<String> {
        let timestamp = sign::now();
        let signature = sign::request_sign(track_id, timestamp);
        let lyrics: Lyrics = self
            .get(
                &format!("/tracks/{track_id}/lyrics"),
                &[
                    ("format", format.to_string()),
                    ("timeStamp", timestamp.to_string()),
                    ("sign", signature),
                ],
            )
            .await?;
        match lyrics.download_url {
            Some(url) => Ok(self.http.get(url).send().await?.text().await?),
            None => bail!("текст песни недоступен"),
        }
    }

    pub async fn cover(&self, uri: &str, size: &str) -> Result<Vec<u8>> {
        let url = format!("https://{}", uri.replace("%%", size));
        let response = self.http.get(url).send().await?;
        if !response.status().is_success() {
            bail!("обложка не загрузилась: HTTP {}", response.status().as_u16());
        }
        Ok(response.bytes().await?.to_vec())
    }

    /// Raw audio stream for a track. `on_chunk` receives `(downloaded, total)`
    /// and returns `false` to abort, which is how a stop request interrupts a
    /// transfer that is already in flight.
    ///
    /// The server hands out several mirror URLs for one file and has no range
    /// support on them, so a retry starts from zero on the next mirror instead
    /// of hammering a host that just failed.
    pub async fn audio_stream(
        &self,
        urls: &[String],
        mut on_chunk: impl FnMut(u64, u64) -> bool,
    ) -> Result<Vec<u8>> {
        if urls.is_empty() {
            bail!("сервер не вернул ссылку на файл");
        }
        let mut last_error = None;
        for attempt in 1..=self.attempts() {
            let url = &urls[(attempt as usize - 1) % urls.len()];
            match self.stream_once(url, &mut on_chunk).await {
                Ok(Some(body)) => return Ok(body),
                Ok(None) => bail!("загрузка остановлена"),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.attempts() {
                        let backoff = self.retry_delay * attempt;
                        tokio::time::sleep(backoff.max(Duration::from_millis(400))).await;
                    }
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("аудио не загрузилось")))
    }

    /// One transfer attempt. `Ok(None)` means the caller asked to stop.
    ///
    /// ponytail: the whole track is buffered in memory; a lossless file is tens
    /// of megabytes and up to six workers run at once. Streaming straight to the
    /// `.part` file (CTR allows chunk-wise decryption) is the upgrade path if
    /// memory ever becomes the bottleneck.
    async fn stream_once(
        &self,
        url: &str,
        on_chunk: &mut impl FnMut(u64, u64) -> bool,
    ) -> Result<Option<Vec<u8>>> {
        use futures_util::StreamExt;
        let response = self
            .http
            .get(url)
            .headers(self.mobile_headers())
            .timeout(Duration::from_secs(600))
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("аудиосервер ответил HTTP {}", response.status().as_u16());
        }
        let total = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let mut body: Vec<u8> = Vec::with_capacity(total as usize);
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("обрыв соединения при загрузке аудио")?;
            body.extend_from_slice(&chunk);
            if !on_chunk(body.len() as u64, total) {
                return Ok(None);
            }
        }
        Ok(Some(body))
    }

}

/// `/users/{uid}/likes/tracks` nests the list one level deep:
/// `result.library.tracks`. Declared at module scope so the test exercises the
/// same types the request uses instead of a copy.
#[derive(serde::Deserialize, Default)]
struct LikesResponse {
    #[serde(default)]
    library: LikesLibrary,
}

#[derive(serde::Deserialize, Default)]
struct LikesLibrary {
    #[serde(default)]
    tracks: Vec<LikesTrack>,
}

#[derive(serde::Deserialize)]
struct LikesTrack {
    #[serde(default)]
    id: Value,
}

pub fn value_to_id(value: &Value) -> Option<String> {    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn truncate(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= 220 {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(220).collect();
    format!("{head}…")
}

pub fn describe_network_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "Сервер не ответил вовремя. Увеличьте таймаут в настройках.".to_string()
    } else if error.is_connect() {
        "Нет соединения с api.music.yandex.net. Проверьте интернет или VPN.".to_string()
    } else {
        format!("Сетевая ошибка: {error}")
    }
}

// --- account -------------------------------------------------------------

pub async fn check_token(client: &Client) -> Result<AccountInfo> {
    let status = client.status().await?;
    let mut info = AccountInfo::default();

    if let Some(account) = status.account {
        info.uid = value_to_id(&account.uid).unwrap_or_default();
        info.login = account.login.unwrap_or_default();
        info.name = account
            .display_name
            .or(account.full_name)
            .unwrap_or_default();
        info.family_member = account.non_owner_family_member;
    }
    info.has_plus = status.plus.map(|plus| plus.has_plus).unwrap_or(false);
    if let Some(permissions) = status.permissions {
        info.permissions = permissions.values.join(", ");
        info.expires = permissions.until.unwrap_or_default();
    }
    if let Some(subscription) = status.subscription {
        if let Some(first) = subscription.auto_renewable.into_iter().next() {
            info.product_id = first
                .product
                .and_then(|product| product.product_id)
                .unwrap_or_default();
            if info.expires.is_empty() {
                info.expires = first.expires.unwrap_or_default();
            }
        }
    }
    info.note = status.advertisement.unwrap_or_default();

    // A real request is the only honest lossless check: `hasPlus` alone does not
    // prove the music service applies the subscription.
    let (probe_id, probe_label) = probe_track(client).await;
    info.probe_track = probe_label;
    match client.download_info(&probe_id, "lossless").await {
        Ok(details) => {
            info.probe_codec = details.codec.clone();
            info.probe_quality = details.quality.clone();
            info.probe_bitrate = details.bitrate;
            info.lossless_ok = Some(matches!(details.codec.as_str(), "flac" | "flac-mp4"));
        }
        Err(error) => {
            info.lossless_ok = None;
            if info.note.is_empty() {
                info.note = format!("Не удалось проверить lossless: {error}");
            }
        }
    }
    Ok(info)
}

async fn probe_track(client: &Client) -> (String, String) {
    if let Ok(ids) = client.likes_tracks(None).await {
        if let Some(first) = ids.first() {
            return (first.clone(), "любимый трек".to_string());
        }
    }
    ("53404".to_string(), "проверочный трек".to_string())
}

// --- sources -------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum LinkKind {
    Track(String),
    Album(String),
    Playlist { owner: Option<String>, kind: String },
    Artist(String),
}

pub fn parse_link(text: &str) -> Result<LinkKind> {
    let raw = text.trim();
    if raw.is_empty() {
        bail!("Вставьте ссылку на трек, альбом, плейлист или исполнителя.");
    }
    if raw.chars().all(|c| c.is_ascii_digit()) {
        return Ok(LinkKind::Track(raw.to_string()));
    }
    let path = if let Some(index) = raw.find("music.yandex") {
        let rest = &raw[index..];
        match rest.find('/') {
            Some(slash) => rest[slash..].split(['?', '#']).next().unwrap_or("").to_string(),
            None => String::new(),
        }
    } else if raw.starts_with('/') {
        raw.split(['?', '#']).next().unwrap_or("").to_string()
    } else {
        raw.split(['?', '#']).next().unwrap_or("").to_string()
    };

    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let mut index = 0;
    while index < segments.len() {
        match segments[index] {
            "track" if index + 1 < segments.len() => {
                return Ok(LinkKind::Track(segments[index + 1].to_string()))
            }
            "album" if index + 1 < segments.len() => {
                let album = segments[index + 1].to_string();
                if segments.get(index + 2) == Some(&"track") && index + 3 < segments.len() {
                    return Ok(LinkKind::Track(segments[index + 3].to_string()));
                }
                return Ok(LinkKind::Album(album));
            }
            "artist" if index + 1 < segments.len() => {
                return Ok(LinkKind::Artist(segments[index + 1].to_string()))
            }
            "users" if index + 2 < segments.len() && segments[index + 2] == "playlists" => {
                if index + 3 < segments.len() {
                    return Ok(LinkKind::Playlist {
                        owner: Some(segments[index + 1].to_string()),
                        kind: segments[index + 3].to_string(),
                    });
                }
            }
            "playlists" if index + 1 < segments.len() => {
                return Ok(LinkKind::Playlist {
                    owner: None,
                    kind: segments[index + 1].to_string(),
                })
            }
            _ => index += 1,
        }
    }
    bail!("Не понял ссылку. Нужна ссылка вида music.yandex.ru/album/… или /users/…/playlists/…")
}

pub async fn resolve_source(
    client: &Client,
    text: &str,
    only_music: bool,
    stick_to_artist: bool,
    progress: impl Fn(usize, usize),
) -> Result<SourceInfo> {
    match parse_link(text)? {
        LinkKind::Track(id) => {
            let tracks = client.tracks(std::slice::from_ref(&id)).await?;
            let track = tracks
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("Трек не найден или недоступен."))?;
            let item = track_item(&track);
            Ok(SourceInfo {
                kind: "track".into(),
                title: item.title.clone(),
                subtitle: item.artist.clone(),
                tracks: vec![item],
            })
        }
        LinkKind::Album(id) => {
            let album = client.album(&id).await?;
            let subtitle = format!(
                "{} · {} · {} треков",
                artist_line(&album.artists),
                album.year.unwrap_or_default(),
                album.track_count.unwrap_or(0)
            );
            let tracks = album
                .volumes
                .iter()
                .flatten()
                .map(track_item)
                .collect::<Vec<_>>();
            Ok(SourceInfo {
                kind: "album".into(),
                title: album.title,
                subtitle,
                tracks,
            })
        }
        LinkKind::Playlist { owner, kind } => {
            let playlist = client.playlist(owner.as_deref(), &kind).await?;
            let mut tracks: Vec<TrackItem> = playlist
                .tracks
                .iter()
                .filter_map(|entry| entry.track.clone())
                .map(|track| track_item(&track))
                .collect();
            let missing: Vec<String> = playlist
                .tracks
                .iter()
                .filter(|entry| entry.track.is_none())
                .filter_map(|entry| value_to_id(&entry.id))
                .collect();
            if !missing.is_empty() {
                let total = missing.len();
                for (index, chunk) in missing.chunks(TRACK_BATCH).enumerate() {
                    let fetched = client.tracks(chunk).await?;
                    tracks.extend(fetched.iter().map(track_item));
                    progress(((index + 1) * TRACK_BATCH).min(total), total);
                }
            }
            let owner_label = owner
                .clone()
                .or_else(|| playlist.owner.and_then(|o| o.login.or(o.name)))
                .unwrap_or_default();
            Ok(SourceInfo {
                kind: "playlist".into(),
                title: playlist.title,
                subtitle: format!("{owner_label} · {} треков", tracks.len()),
                tracks,
            })
        }
        LinkKind::Artist(id) => {
            let name = client
                .artist_name(&id)
                .await
                .unwrap_or_else(|| format!("Исполнитель {id}"));
            let mut album_ids: Vec<String> = Vec::new();
            let mut page = 0u32;
            loop {
                let page_info = client.artist_albums(&id, page).await?;
                for album in &page_info.albums {
                    let Some(album_id) = value_to_id(&album.id) else { continue };
                    if !album.available {
                        continue;
                    }
                    if only_music && album.meta_type.as_deref().unwrap_or("music") != "music" {
                        continue;
                    }
                    if stick_to_artist
                        && album
                            .artists
                            .first()
                            .and_then(|artist| value_to_id(&artist.id))
                            .map(|owner| owner != id)
                            .unwrap_or(false)
                    {
                        continue;
                    }
                    album_ids.push(album_id);
                }
                match page_info.pager {
                    Some(pager) if pager.per_page * (pager.page + 1) < pager.total => page += 1,
                    _ => break,
                }
            }
            let mut tracks: Vec<TrackItem> = Vec::new();
            for (index, album_id) in album_ids.iter().enumerate() {
                if let Ok(album) = client.album(album_id).await {
                    tracks.extend(album.volumes.iter().flatten().map(track_item));
                }
                progress(index + 1, album_ids.len());
            }
            Ok(SourceInfo {
                kind: "artist".into(),
                title: name,
                subtitle: format!("{} альбомов · {} треков", album_ids.len(), tracks.len()),
                tracks,
            })
        }
    }
}

pub async fn likes_source(
    client: &Client,
    username: &str,
    progress: impl Fn(usize, usize),
) -> Result<SourceInfo> {
    let ids = client.likes_tracks(Some(username)).await?;
    if ids.is_empty() {
        bail!("В «Мне нравится» нет треков.");
    }
    let mut tracks = Vec::with_capacity(ids.len());
    let total = ids.len();
    for (index, chunk) in ids.chunks(TRACK_BATCH).enumerate() {
        let fetched = client.tracks(chunk).await?;
        tracks.extend(fetched.iter().map(track_item));
        progress(((index + 1) * TRACK_BATCH).min(total), total);
    }
    let owner = if username.trim().is_empty() {
        String::new()
    } else {
        format!("{} · ", username.trim())
    };
    Ok(SourceInfo {
        kind: "likes".into(),
        title: "Мне нравится".into(),
        subtitle: format!("{owner}{} треков", tracks.len()),
        tracks,
    })
}

pub fn artist_line(artists: &[Artist]) -> String {
    artists
        .iter()
        .map(|artist| artist.name.as_str())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn track_item(track: &Track) -> TrackItem {
    let album = track.albums.first();
    let artist = artist_line(&track.artists);
    TrackItem {
        id: value_to_id(&track.id).unwrap_or_default(),
        title: full_title(&track.title, track.version.as_deref()),
        artist: if artist.is_empty() { "Неизвестный исполнитель".into() } else { artist },
        album: album.map(|a| full_title(&a.title, None)).unwrap_or_default(),
        duration: format_duration(track.duration_ms),
        available: track.available,
        year: album.and_then(|a| a.year),
    }
}

pub fn full_title(title: &str, version: Option<&str>) -> String {
    match version.filter(|value| !value.is_empty()) {
        Some(version) => format!("{title} ({version})"),
        None => title.to_string(),
    }
}

pub fn format_duration(ms: Option<u64>) -> String {
    let Some(ms) = ms else { return String::new() };
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_playlist_link() {
        match parse_link("https://music.yandex.ru/users/MrCriper10/playlists/3").unwrap() {
            LinkKind::Playlist { owner, kind } => {
                assert_eq!(owner.as_deref(), Some("MrCriper10"));
                assert_eq!(kind, "3");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_album_track_link_as_track() {
        match parse_link("https://music.yandex.ru/album/11644078/track/6705392").unwrap() {
            LinkKind::Track(id) => assert_eq!(id, "6705392"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_album_artist_and_bare_id() {
        assert!(matches!(
            parse_link("https://music.yandex.ru/album/294912").unwrap(),
            LinkKind::Album(id) if id == "294912"
        ));
        assert!(matches!(
            parse_link("https://music.yandex.ru/artist/208167").unwrap(),
            LinkKind::Artist(id) if id == "208167"
        ));
        assert!(matches!(parse_link("53404").unwrap(), LinkKind::Track(id) if id == "53404"));
        assert!(parse_link("привет").is_err());
    }

    #[test]
    fn duration_is_formatted() {
        assert_eq!(format_duration(Some(305_600)), "5:05");
        assert_eq!(format_duration(None), "");
    }

    /// The wire shape these tests pin is the one the reference client
    /// (`yandex_music` 3.2.2, `FileInfo.de_json`) expects: `result.downloadInfo`.
    #[test]
    fn file_info_wraps_download_info() {
        let json = serde_json::json!({
            "downloadInfo": {
                "quality": "lossless",
                "codec": "flac-mp4",
                "bitrate": 900,
                "key": "00112233445566778899aabbccddeeff",
                "urls": ["https://example.invalid/1"]
            }
        });
        let info: FileInfo = serde_json::from_value(json).unwrap();
        assert_eq!(info.download_info.codec, "flac-mp4");
        assert_eq!(info.download_info.bitrate, 900);
        assert_eq!(info.download_info.urls.len(), 1);
        assert!(info.download_info.key.is_some());
    }

    #[test]
    fn likes_are_nested_under_library() {
        // Uses the production `LikesResponse`, so reverting the nesting in
        // `likes_tracks` breaks this test.
        let json = serde_json::json!({
            "library": { "tracks": [{ "id": "53404" }, { "id": 42 }] }
        });
        let response: LikesResponse = serde_json::from_value(json).unwrap();
        let ids: Vec<String> = response
            .library
            .tracks
            .iter()
            .filter_map(|track| value_to_id(&track.id))
            .collect();
        assert_eq!(ids, vec!["53404".to_string(), "42".to_string()]);
    }

    #[test]
    fn likes_tolerate_a_missing_library() {
        let response: LikesResponse = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(response.library.tracks.is_empty());
    }

    #[test]
    fn permissions_values_is_a_list_of_strings() {
        // The server sends `values: ["high-quality", ...]`; a `String` field
        // made the whole `/account/status` parse fail and locked the wizard.
        let json = serde_json::json!({
            "account": { "uid": 987853379, "login": "user" },
            "plus": { "hasPlus": true },
            "permissions": { "values": ["high-quality", "library-play"], "until": "2027-01-01" }
        });
        let status: Status = serde_json::from_value(json).unwrap();
        let permissions = status.permissions.unwrap();
        assert_eq!(permissions.values.len(), 2);
        assert_eq!(permissions.values[0], "high-quality");
    }
}
