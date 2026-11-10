//! Lyrics for a track: Yandex first, lrclib.net when Yandex has none.
//!
//! Yandex stores two variants per track and says which it has in
//! `lyricsInfo`: a plain text and a synced one with `[mm:ss.xx]` timestamps.
//! Asking for `format=LRC` is the only way to get the timings; the plain call
//! returns a bare text with no timestamps at all.

use crate::api::Client;
use crate::model::Track;
use serde::Deserialize;

/// A lyrics text plus where it came from, so the log can say why a track ended
/// up with lrclib lyrics or with none.
pub struct LyricsText {
    pub text: String,
    pub synced: bool,
    pub source: &'static str,
}

impl LyricsText {
    /// True when the text carries `[mm:ss]` timestamps.
    pub fn looks_synced(text: &str) -> bool {
        text.lines().any(|line| {
            let rest = line.trim_start().strip_prefix('[');
            match rest {
                Some(rest) => rest
                    .chars()
                    .next()
                    .map(|first| first.is_ascii_digit())
                    .unwrap_or(false),
                None => false,
            }
        })
    }
}

/// lrclib asks clients to identify themselves; an unnamed client is throttled
/// to roughly half the allowance.
const LRCLIB_AGENT: &str = concat!("ym-dl/", env!("CARGO_PKG_VERSION"), " (https://github.com/MrCripe/ym-dl)");
const LRCLIB_GET: &str = "https://lrclib.net/api/get";
const LRCLIB_SEARCH: &str = "https://lrclib.net/api/search";

/// Minimum gap between lrclib requests, measured across every download at once.
///
/// The API allows roughly 90 requests a minute for an identified client and asks
/// for 200–500 ms between calls during batch work. Downloads run up to six at a
/// time, so without a shared gate a playlist would burst straight into 429 and
/// leave tracks without lyrics.
const LRCLIB_INTERVAL: std::time::Duration = std::time::Duration::from_millis(700);
static LRCLIB_GATE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Waits until the next lrclib request is allowed, then claims the slot.
async fn lrclib_turn() {
    let wait = {
        let mut last = match LRCLIB_GATE.lock() {
            Ok(last) => last,
            // A poisoned gate must not stop lyrics forever; skip the throttle.
            Err(_) => return,
        };
        let now = std::time::Instant::now();
        let wait = match *last {
            Some(previous) => LRCLIB_INTERVAL.saturating_sub(now.duration_since(previous)),
            None => std::time::Duration::ZERO,
        };
        *last = Some(now + wait);
        wait
    };
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
}

/// A response that hit the rate limit carries `Retry-After`; the API treats
/// ignoring it as grounds for a temporary ban.
fn retry_after(response: &reqwest::Response) -> Option<std::time::Duration> {
    if response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
        return None;
    }
    let seconds: u64 = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(std::time::Duration::from_secs(seconds.min(30)))
}

/// lrclib answers in camelCase (`trackName`, `syncedLyrics`). Without the rename
/// every field deserialised to its default, so a record that was there looked
/// empty and the fallback silently gave up.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LrclibRecord {
    #[serde(default)]
    track_name: String,
    #[serde(default)]
    artist_name: String,
    #[serde(default)]
    synced_lyrics: Option<String>,
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

impl LrclibRecord {
    fn into_text(self, want_synced: bool) -> Option<LyricsText> {
        if self.instrumental {
            return None;
        }
        let synced = self.synced_lyrics.filter(|text| !text.trim().is_empty());
        let plain = self.plain_lyrics.filter(|text| !text.trim().is_empty());
        match (want_synced, synced, plain) {
            // Synced lyrics are strictly better: they carry the plain text too.
            (true, Some(synced), _) => Some(LyricsText {
                text: synced,
                synced: true,
                source: "lrclib",
            }),
            (_, _, Some(plain)) => Some(LyricsText {
                text: plain,
                synced: false,
                source: "lrclib",
            }),
            (_, Some(synced), None) => Some(LyricsText {
                text: synced,
                synced: true,
                source: "lrclib",
            }),
            _ => None,
        }
    }
}

fn track_artists(track: &Track) -> String {
    track
        .artists
        .iter()
        .map(|artist| artist.name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn track_album(track: &Track) -> String {
    track
        .albums
        .first()
        .map(|album| album.title.trim().to_string())
        .unwrap_or_default()
}

fn track_duration(track: &Track) -> u64 {
    track.duration_ms.unwrap_or(0) / 1000
}

/// Sends one gated lrclib request and parses the body. `None` covers "no such
/// track" (404), a network failure and an exhausted rate limit alike: none of
/// them should stop a download.
async fn lrclib_json<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
    query: &[(&str, String)],
) -> Option<T> {
    lrclib_turn().await;
    let response = http
        .get(url)
        .header(reqwest::header::USER_AGENT, LRCLIB_AGENT)
        .query(query)
        .send()
        .await
        .ok()?;
    if let Some(wait) = retry_after(&response) {
        crate::startup_log::write(&format!(
            "lrclib просит подождать {} с — жду и не дёргаю дальше",
            wait.as_secs()
        ));
        // The wait is claimed in the shared gate too, so parallel downloads back
        // off together instead of each retrying on its own.
        if let Ok(mut last) = LRCLIB_GATE.lock() {
            *last = Some(std::time::Instant::now() + wait);
        }
        return None;
    }
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

/// Exact lookup by metadata. lrclib answers 404 when it has nothing, which is a
/// normal result, not a failure.
async fn lrclib_get(http: &reqwest::Client, track: &Track) -> Option<LrclibRecord> {
    let artist = track_artists(track);
    let title = track.title.trim().to_string();
    if artist.is_empty() || title.is_empty() {
        return None;
    }
    let mut query = vec![("artist_name", artist), ("track_name", title)];
    let album = track_album(track);
    if !album.is_empty() {
        query.push(("album_name", album));
    }
    let duration = track_duration(track);
    if duration > 0 {
        query.push(("duration", duration.to_string()));
    }
    lrclib_json(http, LRCLIB_GET, &query).await
}

/// Loose search, used when the exact match misses because a title carries a
/// version suffix or the artist is spelled differently.
async fn lrclib_search(http: &reqwest::Client, track: &Track, want_synced: bool) -> Option<LyricsText> {
    let artist = track_artists(track);
    let title = track.title.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let mut records = lrclib_query(http, &title, &artist).await;
    if records.is_empty() && !artist.is_empty() {
        // A featured artist or a different transliteration still matches on title.
        records = lrclib_query(http, &title, "").await;
    }
    let mut best: Option<LrclibRecord> = None;
    for record in records {
        // A cover or a live version carries the same title; only an exact title
        // match is worth embedding, and an artist match beats one without.
        if !record.track_name.eq_ignore_ascii_case(&title) {
            continue;
        }
        let has_wanted = if want_synced {
            record.synced_lyrics.is_some()
        } else {
            record.synced_lyrics.is_some() || record.plain_lyrics.is_some()
        };
        if !has_wanted {
            continue;
        }
        let artist_matches = !artist.is_empty()
            && record
                .artist_name
                .to_ascii_lowercase()
                .contains(&artist.to_ascii_lowercase());
        if artist_matches || best.is_none() {
            best = Some(record);
        }
        if artist_matches {
            break;
        }
    }
    best.and_then(|record| record.into_text(want_synced))
}

async fn lrclib_query(http: &reqwest::Client, title: &str, artist: &str) -> Vec<LrclibRecord> {
    let mut query = vec![("track_name", title.to_string())];
    if !artist.is_empty() {
        query.push(("artist_name", artist.to_string()));
    }
    lrclib_json(http, LRCLIB_SEARCH, &query)
        .await
        .unwrap_or_default()
}

async fn lrclib(http: &reqwest::Client, track: &Track, want_synced: bool) -> Option<LyricsText> {
    if let Some(record) = lrclib_get(http, track).await {
        if let Some(text) = record.into_text(want_synced) {
            return Some(text);
        }
    }
    lrclib_search(http, track, want_synced).await
}

/// Yandex lyrics for `track_id`, synced when the track has a timed version and
/// the caller wants one.
async fn yandex(client: &Client, track: &Track, track_id: &str, want_synced: bool) -> Option<LyricsText> {
    let info = track.lyrics_info.as_ref();
    let has_sync = info.map(|info| info.has_available_sync_lyrics).unwrap_or(false);
    let has_text = info.map(|info| info.has_available_text_lyrics).unwrap_or(false);
    if !has_sync && !has_text && info.is_some() {
        // The API already said there is nothing; skip the request entirely.
        return None;
    }
    let formats: &[&str] = if want_synced && has_sync {
        &["LRC", "TEXT"]
    } else {
        &["TEXT", "LRC"]
    };
    for format in formats {
        if let Ok(text) = client.lyrics(track_id, format).await {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            let synced = LyricsText::looks_synced(trimmed);
            return Some(LyricsText {
                text: trimmed.to_string(),
                synced,
                source: "yandex",
            });
        }
    }
    None
}

/// Removes `[mm:ss.xx]` prefixes, leaving the words. Used when the setting asks
/// for plain text: the API only has a timed version, and writing timestamps into
/// a file the user asked to keep plain would ignore the setting.
fn strip_timings(text: &str) -> String {
    text.lines()
        .map(|line| {
            let mut rest = line.trim_start();
            while let Some(after) = rest.strip_prefix('[') {
                match after.find(']') {
                    // A bracketed run of digits and colons is a timestamp.
                    Some(end)
                        if after[..end]
                            .chars()
                            .all(|ch| ch.is_ascii_digit() || ch == ':' || ch == '.') =>
                    {
                        rest = after[end + 1..].trim_start();
                    }
                    _ => break,
                }
            }
            rest
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Picks the best available lyrics for a track.
///
/// `want_synced` is false for the plain-text setting. `fallback` allows asking
/// lrclib when Yandex has nothing.
pub async fn fetch(
    client: &Client,
    track: &Track,
    track_id: &str,
    want_synced: bool,
    fallback: bool,
) -> Option<LyricsText> {
    let found = match yandex(client, track, track_id, want_synced).await {
        Some(text) => {
            // A timed request that only produced a plain text is still better than
            // nothing, but the fallback may hold the real synced version.
            if text.synced || !want_synced || !fallback {
                Some(text)
            } else {
                lrclib(client.http(), track, true).await.or(Some(text))
            }
        }
        None if fallback => lrclib(client.http(), track, want_synced).await,
        None => None,
    };
    // The setting decides whether timestamps stay, whatever the source had.
    found.map(|mut lyrics| {
        if !want_synced && lyrics.synced {
            lyrics.text = strip_timings(&lyrics.text);
            lyrics.synced = false;
        }
        lyrics
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_timestamps() {
        assert!(LyricsText::looks_synced("[00:12.34] первая строка"));
        assert!(LyricsText::looks_synced("[01:02.5]вторая"));
        assert!(!LyricsText::looks_synced("просто текст без времени"));
        assert!(!LyricsText::looks_synced("[припев]\nстрока"));
    }

    /// The plain-text setting must really drop the timestamps, even when the API
    /// only has a timed version.
    #[test]
    fn plain_setting_strips_timings() {
        let text = "[00:12.34] первая строка\n[00:15.00] вторая\n[припев] без времени";
        assert_eq!(strip_timings(text), "первая строка\nвторая\n[припев] без времени");
    }

    #[test]
    fn stripping_leaves_plain_text_alone() {
        let text = "строка без времени\n[припев]";
        assert_eq!(strip_timings(text), text);
    }

    #[test]
    fn synced_lyrics_win_over_plain() {
        let record = LrclibRecord {
            track_name: "Song".into(),
            artist_name: "Artist".into(),
            synced_lyrics: Some("[00:01.00] timed".into()),
            plain_lyrics: Some("plain".into()),
            instrumental: false,
        };
        let text = record.into_text(true).expect("synced");
        assert!(text.synced);
        assert!(text.text.contains("[00:01.00]"));
    }

    #[test]
    fn an_instrumental_record_yields_nothing() {
        let record = LrclibRecord {
            track_name: "Song".into(),
            artist_name: "Artist".into(),
            synced_lyrics: None,
            plain_lyrics: None,
            instrumental: true,
        };
        assert!(record.into_text(true).is_none());
    }

    /// The API answers in camelCase. Without the rename every field fell back to
    /// its default and a record that was present looked empty, so the fallback
    /// gave up on every track.
    #[test]
    fn reads_the_camel_case_response() {
        let json = r#"{
            "id": 15830,
            "name": "Sliver",
            "trackName": "Sliver",
            "artistName": "Nirvana",
            "albumName": "Nirvana",
            "duration": 134.24,
            "instrumental": false,
            "hasWordSync": false,
            "plainLyrics": "Mom and dad went to a show",
            "syncedLyrics": "[00:07.20] Mom and dad went to a show"
        }"#;
        let record: LrclibRecord = serde_json::from_str(json).expect("ответ разбирается");
        assert_eq!(record.track_name, "Sliver");
        assert_eq!(record.artist_name, "Nirvana");
        let text = record.into_text(true).expect("текст должен найтись");
        assert!(text.synced);
        assert!(text.text.contains("[00:07.20]"));
    }
}
