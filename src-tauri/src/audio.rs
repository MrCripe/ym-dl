//! Audio post-processing: decryption, file naming, tagging and FLAC extraction.

use crate::model::Track;
use crate::api::{full_title, value_to_id};
use aes::Aes128;
use ctr::cipher::{KeyIvInit, StreamCipher};
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::{Accessor, ItemKey};
use lofty::probe::Probe;
use lofty::tag::{Tag, TagType};
use std::path::{Path, PathBuf};

type Aes128Ctr = ctr::Ctr128BE<Aes128>;

const MAX_NAME_BYTES: usize = 250;

/// Yandex encrypts audio with AES-128-CTR using a 12-byte zero nonce.
///
/// Takes the buffer by value and decrypts in place: a lossless track is tens of
/// megabytes and a copy would double the peak memory of every worker.
pub fn decrypt(mut data: Vec<u8>, key_hex: &str) -> anyhow::Result<Vec<u8>> {
    let key = hex_to_bytes(key_hex)?;
    if key.len() != 16 {
        anyhow::bail!("неверная длина ключа расшифровки: {} байт", key.len());
    }
    let mut cipher = Aes128Ctr::new(key.as_slice().into(), &[0u8; 16].into());
    cipher.apply_keystream(&mut data);
    Ok(data)
}

fn hex_to_bytes(text: &str) -> anyhow::Result<Vec<u8>> {
    let trimmed = text.trim();
    if trimmed.len() % 2 != 0 {
        anyhow::bail!("ключ расшифровки нечётной длины");
    }
    (0..trimmed.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&trimmed[index..index + 2], 16).map_err(Into::into))
        .collect()
}

/// Placeholders mirror `llistochek/yandex-music-downloader` so existing folders
/// keep their layout.
pub fn base_path(pattern: &str, track: &Track, unsafe_path: bool) -> PathBuf {
    let album = track.albums.first();
    let track_artist = track
        .artists
        .first()
        .map(|artist| artist.name.clone())
        .unwrap_or_default();
    let album_artist = album
        .and_then(|album| album.artists.first())
        .map(|artist| artist.name.clone())
        .unwrap_or_default();
    let position = album.and_then(|album| album.track_position.as_ref());
    let index = position.and_then(|value| value.index);
    let volume = position.and_then(|value| value.volume);
    let track_count = album.and_then(|album| album.track_count).unwrap_or(0);
    let pad = track_count.to_string().len().max(2);

    let replacements: [(&str, String); 12] = [
        ("#number-padded", index.map(|value| format!("{value:0pad$}")).unwrap_or_default()),
        ("#number", index.map(|value| value.to_string()).unwrap_or_default()),
        ("#disc-number-padded", volume.map(|value| format!("{value:02}")).unwrap_or_default()),
        ("#disc-number", volume.map(|value| value.to_string()).unwrap_or_default()),
        ("#album-artist", album_artist),
        ("#track-artist", track_artist),
        ("#artist-id", track.artists.first().and_then(|a| value_to_id(&a.id)).unwrap_or_default()),
        ("#album-id", album.and_then(|a| value_to_id(&a.id)).unwrap_or_default()),
        ("#track-id", value_to_id(&track.id).unwrap_or_default()),
        ("#title", full_title(&track.title, track.version.as_deref())),
        ("#album", album.map(|a| full_title(&a.title, None)).unwrap_or_default()),
        ("#year", album.and_then(|a| a.year).map(|y| y.to_string()).unwrap_or_default()),
    ];

    let mut result = pattern.to_string();
    for (placeholder, value) in replacements {
        let safe = sanitize(&value, unsafe_path);
        result = result.replace(placeholder, &safe);
    }
    result
        .split(['/', '\\'])
        .filter(|part| !part.trim().is_empty())
        .fold(PathBuf::new(), |path, part| path.join(trim_name(part)))
}

fn sanitize(value: &str, unsafe_path: bool) -> String {
    let trimmed = value.trim();
    if unsafe_path {
        return trimmed.replace(['/', '\\'], "_");
    }
    trimmed
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ch if (ch as u32) < 0x20 => '_',
            ch => ch,
        })
        .collect::<String>()
        .trim_end_matches(['.', ' '])
        .to_string()
}

fn trim_name(part: &str) -> String {
    if part.len() <= MAX_NAME_BYTES {
        return part.to_string();
    }
    let mut end = MAX_NAME_BYTES;
    while end > 0 && !part.is_char_boundary(end) {
        end -= 1;
    }
    part[..end].to_string()
}

pub fn extension_for(container: &str, codec: &str) -> &'static str {
    match container {
        "MP3" => ".mp3",
        "FLAC" => ".flac",
        "MP4" if codec == "FLAC" => ".m4a",
        "MP4" => ".m4a",
        _ => ".bin",
    }
}

pub fn container_name(codec: &str) -> (&'static str, &'static str) {
    match codec {
        "flac" => ("FLAC", "FLAC"),
        "flac-mp4" => ("MP4", "FLAC"),
        "mp3" => ("MP3", "MP3"),
        _ => ("MP4", "AAC"),
    }
}

pub struct TagPayload<'a> {
    pub title: String,
    pub artist: Vec<String>,
    pub album_artist: Vec<String>,
    pub album: String,
    pub year: Option<i32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub genre: Option<String>,
    pub cover: Option<Vec<u8>>,
    pub lyrics: Option<String>,
    pub url: String,
    pub compatibility_level: u8,
    pub _marker: std::marker::PhantomData<&'a ()>,
}

impl TagPayload<'_> {
    pub fn from_track(track: &Track, url: String, compatibility_level: u8) -> Self {
        let album = track.albums.first();
        let position = album.and_then(|a| a.track_position.as_ref());
        Self {
            title: full_title(&track.title, track.version.as_deref()),
            artist: track
                .artists
                .iter()
                .map(|artist| artist.name.clone())
                .filter(|name| !name.is_empty())
                .collect(),
            album_artist: album
                .map(|a| {
                    a.artists
                        .iter()
                        .map(|artist| artist.name.clone())
                        .filter(|name| !name.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            album: album.map(|a| full_title(&a.title, None)).unwrap_or_default(),
            year: album.and_then(|a| a.year),
            track_number: position.and_then(|p| p.index),
            disc_number: position.and_then(|p| p.volume),
            genre: album.and_then(|a| a.genre.clone()),
            cover: None,
            lyrics: None,
            url,
            compatibility_level,
            _marker: std::marker::PhantomData,
        }
    }
}

pub fn write_tags(path: &Path, payload: &TagPayload<'_>) -> anyhow::Result<()> {
    // lofty guesses the container from the extension. A name it does not
    // recognise makes `read()` fail with UnknownFormat, and then every tag —
    // cover and lyrics included — is silently dropped. Fall back to sniffing the
    // content so a temporary file name can never lose the metadata.
    let probe = Probe::open(path)?;
    let probe = if probe.file_type().is_none() {
        probe.guess_file_type()?
    } else {
        probe
    };
    let mut tagged = probe.read()?;
    let tag_type = tagged.primary_tag_type();
    let tag = tagged.primary_tag_mut();
    let tag = match tag {
        Some(tag) => tag,
        None => {
            tagged.insert_tag(Tag::new(tag_type));
            tagged.primary_tag_mut().expect("tag just inserted")
        }
    };

    tag.set_title(payload.title.clone());
    tag.set_album(payload.album.clone());
    tag.set_artist(join_artists(&payload.artist, payload.compatibility_level));
    if !payload.album_artist.is_empty() {
        tag.insert_text(
            ItemKey::AlbumArtist,
            join_artists(&payload.album_artist, payload.compatibility_level),
        );
    }
    if let Some(year) = payload.year {
        tag.set_year(year as u32);
    }
    if let Some(number) = payload.track_number {
        tag.set_track(number);
    }
    if let Some(disc) = payload.disc_number {
        tag.set_disk(disc);
    }
    if let Some(genre) = payload.genre.clone().filter(|g| !g.is_empty()) {
        tag.set_genre(genre);
    }
    if let Some(lyrics) = payload.lyrics.clone().filter(|l| !l.is_empty()) {
        tag.insert_text(ItemKey::Lyrics, lyrics);
    }
    if !payload.url.is_empty() {
        tag.insert_text(ItemKey::Comment, payload.url.clone());
    }
    if let Some(cover) = payload.cover.clone() {
        let mime = if cover.starts_with(&[0x89, b'P', b'N', b'G']) {
            lofty::picture::MimeType::Png
        } else {
            lofty::picture::MimeType::Jpeg
        };
        let picture = lofty::picture::Picture::new_unchecked(
            lofty::picture::PictureType::CoverFront,
            Some(mime),
            None,
            cover,
        );
        tag.push_picture(picture);
    }
    if tag_type == TagType::Mp4Ilst {
        let _ = tag.insert_text(ItemKey::Unknown("aART".into()), payload.album_artist.join("; "));
    }
    tagged.save_to_path(path, WriteOptions::default())?;
    Ok(())
}

/// Reads back what a finished file actually contains. Used by the live check and
/// by tests: writing tags is only useful if they survive on disk.
pub fn read_tags(path: &Path) -> anyhow::Result<String> {
    let probe = Probe::open(path)?;
    let probe = if probe.file_type().is_none() {
        probe.guess_file_type()?
    } else {
        probe
    };
    let tagged = probe.read()?;
    let Some(tag) = tagged.primary_tag() else {
        anyhow::bail!("в файле нет тега");
    };
    let lyrics = tag.get_string(&ItemKey::Lyrics).unwrap_or("");
    Ok(format!(
        "название={:?} исполнитель={:?} альбом={:?} год={:?} номер={:?} жанр={:?} обложка={} лирикс={}",
        tag.title().unwrap_or_default(),
        tag.artist().unwrap_or_default(),
        tag.album().unwrap_or_default(),
        tag.year().map(|year| year.to_string()).unwrap_or_default(),
        tag.track().map(|track| track.to_string()).unwrap_or_default(),
        tag.genre().unwrap_or_default(),
        tag.pictures().len(),
        if lyrics.is_empty() { "нет" } else { "да" }
    ))
}

fn join_artists(artists: &[String], compatibility_level: u8) -> String {
    if compatibility_level >= 1 && artists.len() > 1 {
        artists.join("; ")
    } else {
        artists.join(", ")
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AlbumRef, Artist, TrackPosition};
    use serde_json::json;

    fn sample_track() -> Track {
        Track {
            id: json!("6705392"),
            title: "Seven Nation Army".into(),
            version: None,
            available: true,
            duration_ms: Some(231_000),
            artists: vec![Artist { id: json!(7), name: "The White Stripes".into() }],
            albums: vec![AlbumRef {
                id: json!(11644078),
                title: "Elephant".into(),
                year: Some(2003),
                release_date: None,
                cover_uri: None,
                track_position: Some(TrackPosition { volume: Some(1), index: Some(1) }),
                track_count: Some(14),
                artists: vec![Artist { id: json!(7), name: "The White Stripes".into() }],
                genre: Some("rock".into()),
                meta_type: Some("music".into()),
                available: true,
            }],
            lyrics_info: None,
            cover_uri: None,
        }
    }

    #[test]
    fn path_pattern_expands_placeholders() {
        let track = sample_track();
        let path = base_path("#album-artist/#album/#number - #title", &track, false);
        let rendered = path.to_string_lossy().replace('\\', "/");
        assert_eq!(rendered, "The White Stripes/Elephant/1 - Seven Nation Army");
    }

    #[test]
    fn unsafe_characters_are_replaced() {
        let mut track = sample_track();
        track.title = "AC/DC: live?".into();
        let path = base_path("#title", &track, false);
        assert_eq!(path.to_string_lossy(), "AC_DC_ live_");
    }

    #[test]
    fn decrypt_is_symmetric() {
        let key = "00112233445566778899aabbccddeeff";
        let plain = b"hello yandex music".to_vec();
        let encrypted = decrypt(plain.clone(), key).unwrap();
        assert_ne!(encrypted, plain);
        assert_eq!(decrypt(encrypted, key).unwrap(), plain);
    }

    #[test]
    fn extension_matches_container() {
        assert_eq!(extension_for("FLAC", "FLAC"), ".flac");
        assert_eq!(extension_for("MP4", "FLAC"), ".m4a");
        assert_eq!(extension_for("MP4", "AAC"), ".m4a");
        assert_eq!(extension_for("MP3", "MP3"), ".mp3");
    }
}

#[cfg(test)]
mod tag_tests {
    use super::*;
    use std::process::Command;

    /// A real MP4 with a real audio stream, so the tagger meets the same file the
    /// download engine produces. Returns `None` when ffmpeg is not installed.
    fn make_m4a(dir: &Path) -> Option<PathBuf> {
        let path = dir.join("source.m4a");
        let status = Command::new("ffmpeg")
            .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
            .args(["-i", "anullsrc=r=44100:cl=stereo", "-t", "0.2"])
            .args(["-c:a", "aac", "-b:a", "64k"])
            .arg(&path)
            .status()
            .ok()?;
        status.success().then_some(path)
    }

    fn tiny_png() -> Vec<u8> {
        // 1x1 PNG, enough for lofty to treat the payload as a real picture.
        const PNG: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H',
            b'D', b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
        ];
        PNG.to_vec()
    }

    /// The bug that lost every tag: the engine tagged a file named `X.m4a.part`,
    /// and lofty guesses the container from the extension.
    #[test]
    fn tags_survive_a_part_suffixed_name() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("ymdl-tags-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let Some(source) = make_m4a(&dir) else {
            eprintln!("ffmpeg недоступен — тест пропущен");
            return;
        };

        // Exactly the temporary name the download engine writes before renaming.
        let temp = dir.join(".track.part.m4a");
        std::fs::copy(&source, &temp).unwrap();

        let payload = TagPayload {
            title: "Название".into(),
            artist: vec!["Артист".into()],
            album_artist: vec!["Артист".into()],
            album: "Альбом".into(),
            year: Some(2026),
            track_number: Some(3),
            disc_number: Some(1),
            genre: Some("rock".into()),
            cover: Some(tiny_png()),
            lyrics: Some("строка текста".into()),
            url: "https://music.yandex.ru/track/1".into(),
            compatibility_level: 1,
            _marker: std::marker::PhantomData,
        };
        write_tags(&temp, &payload).expect("теги должны записаться в .part.m4a");

        let summary = read_tags(&temp).expect("файл должен перечитаться");
        assert!(summary.contains("название=\"Название\""), "{summary}");
        assert!(summary.contains("исполнитель=\"Артист\""), "{summary}");
        assert!(summary.contains("альбом=\"Альбом\""), "{summary}");
        assert!(summary.contains("год=\"2026\""), "{summary}");
        assert!(summary.contains("номер=\"3\""), "{summary}");
        assert!(summary.contains("жанр=\"rock\""), "{summary}");
        assert!(summary.contains("обложка=1"), "обложка потерялась: {summary}");
        assert!(summary.contains("лирикс=да"), "лирикс потерялся: {summary}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
