//! Audio post-processing: decryption, file naming, tagging and FLAC extraction.

use crate::model::Track;
use crate::api::{full_title, value_to_id};
use aes::Aes128;
use ctr::cipher::{KeyIvInit, StreamCipher};
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::{Accessor, ItemKey};
use lofty::probe::Probe;
use lofty::tag::Tag;
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
    if !trimmed.len().is_multiple_of(2) {
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
        .fold(PathBuf::new(), |path, part| {
            path.join(avoid_reserved(&trim_name(part)))
        })
}

/// Names Windows reserves for devices, with or without an extension.
///
/// A track called "CON" or "NUL" cannot be created there: the write fails with a
/// message about the device rather than about the name.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn avoid_reserved(name: &str) -> String {
    let stem = name.split('.').next().unwrap_or(name).trim().to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// A single path component with everything a filesystem rejects replaced.
///
/// Public because the playlist file is named after the album, and that name comes
/// from the service just like a track title does.
pub fn sanitize_file_name(value: &str) -> String {
    sanitize(value, false)
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
    /// International Standard Recording Code, when the service provides one.
    /// Yandex usually does not, so this stays empty more often than not.
    pub isrc: Option<String>,
    /// Record label, from the album.
    pub label: Option<String>,
    /// `1` when the service flags the track as explicit, `0` when it is clean,
    /// `None` when the service said nothing and the tag stays empty.
    pub explicit: Option<bool>,
    /// Composer or lyricist, when the service credits one.
    pub composer: Option<String>,
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
            isrc: track.isrc.clone(),
            label: album
                .map(|album| {
                    album
                        .labels
                        .iter()
                        .map(|label| label.name.clone())
                        .filter(|name| !name.is_empty())
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .filter(|labels| !labels.is_empty()),
            // The track carries its own flag; the album's warning applies when
            // the track has none, which is how the service marks a whole record.
            // No flag at all leaves the tag empty rather than claiming "clean".
            explicit: (track.explicit || track.content_warning.is_some())
                .then_some(track.explicit || track.content_warning.as_deref() == Some("explicit")),
            composer: None,
            cover: None,
            lyrics: None,
            url,
            compatibility_level,
            _marker: std::marker::PhantomData,
        }
    }
}

/// Temporary name for a track that is still being written: hidden, marked as a
/// partial file, and ending with the real extension so the tagger and ffprobe
/// still identify the container.
pub fn part_path(final_path: &Path) -> PathBuf {
    let extension = final_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    let stem = final_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    final_path.with_file_name(format!(".{stem}.part{extension}"))
}

/// Writes every tag the payload carries into the file.
///
/// MP4 used to go through ffmpeg because an older lofty rewrote the `moov` atom
/// without shifting the `stco` chunk offsets, which left the file unplayable.
/// Lofty 0.22 updates those offsets itself, so every container is handled here
/// and the app needs no external binary.
pub fn write_tags(path: &Path, payload: &TagPayload<'_>) -> anyhow::Result<()> {
    write_tags_lofty(path, payload)
}

/// Writes only the lyrics tag into a file that is already tagged, leaving every
/// other field alone.
///
/// The lrclib lookup runs as one pass after the downloads finish, so the files are
/// already written and tagged by then. Re-running the full tag pass would re-mux
/// every file and re-download covers for nothing.
pub fn write_lyrics(path: &Path, text: &str) -> anyhow::Result<()> {
    write_lyrics_lofty(path, text)
}

fn write_lyrics_lofty(path: &Path, text: &str) -> anyhow::Result<()> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "track".into());
    let scratch = path.with_file_name(format!(".{stem}.lyrics{extension}"));
    std::fs::copy(path, &scratch)?;
    let result = (|| -> anyhow::Result<()> {
        let mut tagged = open_tagged(&scratch)?;
        let tag_type = tagged.primary_tag_type();
        if tagged.primary_tag().is_none() {
            tagged.insert_tag(Tag::new(tag_type));
        }
        let tag = tagged.primary_tag_mut().expect("tag just ensured");
        tag.insert_text(ItemKey::Lyrics, text.to_string());
        tagged.save_to_path(&scratch, WriteOptions::default())?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            crate::mp4::repair_chunk_offsets(path, &scratch)?;
            if !crate::transcode::decodes(&scratch) {
                let _ = std::fs::remove_file(&scratch);
                anyhow::bail!("файл после записи текста перестал читаться");
            }
            std::fs::rename(&scratch, path)?;
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&scratch);
            Err(error)
        }
    }
}

fn write_tags_lofty(path: &Path, payload: &TagPayload<'_>) -> anyhow::Result<()> {
    // Work on a copy: if the tagger damages the container the original audio is
    // still intact and can be kept instead of shipping a file that will not play.
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "track".into());
    let scratch = path.with_file_name(format!(".{stem}.tagging{extension}"));
    std::fs::copy(path, &scratch)?;
    let result = tag_in_place_lofty(&scratch, payload);
    match result {
        Ok(()) => {
            // An MP4 whose `moov` sits before `mdat` has its chunk offsets left
            // pointing at the old audio position after the tag block grows.
            crate::mp4::repair_chunk_offsets(path, &scratch)?;
            if !crate::transcode::decodes(&scratch) {
                let _ = std::fs::remove_file(&scratch);
                anyhow::bail!("файл после записи тегов перестал читаться");
            }
            std::fs::rename(&scratch, path)?;
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&scratch);
            Err(error)
        }
    }
}

/// Opens a file for tagging. lofty guesses the container from the extension, and
/// a name it does not recognise makes `read()` fail with UnknownFormat — which
/// silently dropped every tag when tracks were written as `X.m4a.part`. Sniffing
/// the content covers that case.
fn open_tagged(path: &Path) -> anyhow::Result<lofty::file::TaggedFile> {
    let probe = Probe::open(path)?;
    let probe = if probe.file_type().is_none() {
        probe.guess_file_type()?
    } else {
        probe
    };
    Ok(probe.read()?)
}

pub fn tag_in_place_lofty(path: &Path, payload: &TagPayload<'_>) -> anyhow::Result<()> {
    let mut tagged = open_tagged(path)?;
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
    if let Some(isrc) = payload.isrc.clone().filter(|code| !code.is_empty()) {
        tag.insert_text(ItemKey::Isrc, isrc);
    }
    if let Some(label) = payload.label.clone().filter(|text| !text.is_empty()) {
        tag.insert_text(ItemKey::Label, label);
    }
    if let Some(composer) = payload.composer.clone().filter(|text| !text.is_empty()) {
        tag.insert_text(ItemKey::Composer, composer);
    }
    if let Some(explicit) = payload.explicit {
        // The tag is a free-form string across formats; `1` is what players read.
        tag.insert_text(
            ItemKey::Unknown("ITUNESADVISORY".into()),
            if explicit { "1".to_string() } else { "0".to_string() },
        );
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
        // `push_picture` appends, and readers take the first picture as the
        // cover: without dropping the old one, "change the cover" kept showing
        // the previous image and grew the file by one picture per save.
        tag.remove_picture_type(lofty::picture::PictureType::CoverFront);
        tag.push_picture(picture);
    }
    tagged.save_to_path(path, WriteOptions::default())?;
    Ok(())
}

/// What a finished file on disk says about itself.
///
/// This is how the library is rebuilt from the folder: the tags written at
/// download time are the only record left if the database is lost or the user
/// moves their music somewhere else.
#[derive(Debug, Clone, Default)]
pub struct FileMetadata {
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub genre: String,
    pub isrc: String,
    pub label: String,
    pub composer: String,
    pub explicit: Option<bool>,
    pub year: Option<i32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_ms: u64,
    pub codec: String,
    pub bitrate: u32,
    /// Stream shape, needed to build a STREAMINFO block when a container does
    /// not carry one and to set up the MP3 encoder.
    pub sample_rate: u32,
    pub channels: u32,
    pub bits_per_sample: u32,
    /// The Yandex page URL written into the comment, when the file has one.
    pub comment: String,
    /// Lyrics already embedded in the file. A quality upgrade rewrites the tags
    /// and must carry them over, or the new file would lose them.
    pub lyrics: String,
}

/// Reads tags and stream properties from an audio file.
pub fn read_file_metadata(path: &Path) -> anyhow::Result<FileMetadata> {
    let tagged = open_tagged(path)?;
    let properties = tagged.properties();
    let file_type = tagged.file_type();
    let codec = if format!("{file_type:?}").contains("Flac") {
        "FLAC".to_string()
    } else if format!("{file_type:?}").contains("Mpeg") {
        "MP3".to_string()
    } else if format!("{file_type:?}").contains("Mp4") {
        // MP4 does not say in its tag block whether it holds AAC or FLAC; the
        // sample entry does, so that is read from the container. The old check
        // reported every `.m4a` as AAC, understating a lossless file.
        crate::mp4::codec_of_file(path)
            .map(|codec| codec.name().to_string())
            .unwrap_or_else(|| "AAC".to_string())
    } else {
        format!("{file_type:?}")
    };

    let mut meta = FileMetadata {
        duration_ms: properties.duration().as_millis() as u64,
        bitrate: properties.audio_bitrate().unwrap_or(0),
        codec,
        sample_rate: properties.sample_rate().unwrap_or(0),
        channels: properties.channels().unwrap_or(0) as u32,
        bits_per_sample: properties.bit_depth().map(u32::from).unwrap_or(0),
        ..Default::default()
    };

    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(meta);
    };
    let text = |key: ItemKey| tag.get_string(&key).unwrap_or("").to_string();
    meta.title = text(ItemKey::TrackTitle);
    meta.artist = text(ItemKey::TrackArtist);
    meta.album_artist = text(ItemKey::AlbumArtist);
    meta.album = text(ItemKey::AlbumTitle);
    meta.genre = text(ItemKey::Genre);
    meta.isrc = text(ItemKey::Isrc);
    meta.label = text(ItemKey::Label);
    meta.composer = text(ItemKey::Composer);
    meta.explicit = tag
        .get_string(&ItemKey::Unknown("ITUNESADVISORY".into()))
        .map(|value| value.trim() == "1");
    meta.comment = text(ItemKey::Comment);
    meta.lyrics = text(ItemKey::Lyrics);
    meta.year = tag.year().and_then(|year| i32::try_from(year).ok());
    meta.track_number = tag.track();
    meta.disc_number = tag.disk();
    Ok(meta)
}

/// The embedded cover of a file, if it has one.
///
/// Editing a tag rewrites the whole tag block, and a writer that is not given a
/// picture drops the one that was there. Reading it back first is what keeps a
/// cover from disappearing the moment someone fixes a typo in a title.
pub fn read_cover(path: &Path) -> Option<Vec<u8>> {
    let tagged = open_tagged(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    tag.pictures().first().map(|picture| picture.data().to_vec())
}

/// The Yandex track id hidden in a page URL, e.g.
/// `https://music.yandex.ru/album/1/track/42` → `42`.
pub fn track_id_from_url(url: &str) -> Option<String> {
    let marker = "/track/";
    let rest = &url[url.find(marker)? + marker.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

/// Reads back what a finished file actually contains. Used by the live check and
/// by tests: writing tags is only useful if they survive on disk.
pub fn read_tags(path: &Path) -> anyhow::Result<String> {
    let tagged = open_tagged(path)?;
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
            artists: vec![Artist {
                id: json!(7),
                name: "The White Stripes".into(),
                cover_uri: None,
                cover: None,
                op_image: None,
            }],
            albums: vec![AlbumRef {
                id: json!(11644078),
                title: "Elephant".into(),
                year: Some(2003),
                release_date: None,
                cover_uri: None,
                track_position: Some(TrackPosition { volume: Some(1), index: Some(1) }),
                track_count: Some(14),
                artists: vec![Artist {
                    id: json!(7),
                    name: "The White Stripes".into(),
                    cover_uri: None,
                    cover: None,
                    op_image: None,
                }],
                genre: Some("rock".into()),
                meta_type: Some("music".into()),
                available: true,
                labels: Vec::new(),
            }],
            lyrics_info: None,
            cover_uri: None,
            isrc: None,
            explicit: false,
            content_warning: None,
        }
    }

    /// Tagging must not move the audio.
    ///
    /// The tagger grows `moov` and pushes `mdat` down the file; on a real 37 MB
    /// download it left every chunk offset behind, so the file decoded to noise
    /// and the tags were dropped. The faststart fixture is the same layout:
    /// `moov` before `mdat`, which is what the service sends.
    #[test]
    fn tagging_keeps_the_audio_of_a_faststart_mp4() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("tag-faststart-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures");
        let source = fixtures.join("flac-mp4-faststart.m4a");
        let target = dir.join("tagged.m4a");
        std::fs::copy(&source, &target).unwrap();

        let before = dir.join("before.flac");
        crate::mp4::extract_flac(&source, &before).unwrap();

        let payload = TagPayload {
            title: "Probe".into(),
            artist: vec!["Someone".into()],
            album_artist: Vec::new(),
            album: "Album".into(),
            year: Some(2024),
            track_number: Some(1),
            disc_number: Some(1),
            genre: Some("rock".into()),
            isrc: None,
            label: None,
            explicit: None,
            composer: None,
            cover: None,
            lyrics: Some("[00:01.00] line".into()),
            url: "https://example.com".into(),
            compatibility_level: 1,
            _marker: std::marker::PhantomData,
        };
        write_tags(&target, &payload).expect("the tag write must succeed");

        assert!(crate::transcode::decodes(&target), "the tagged file must decode");
        let summary = read_tags(&target).expect("the tags must be readable");
        assert!(summary.contains("Probe"), "the title did not land: {summary}");

        let after = dir.join("after.flac");
        crate::mp4::extract_flac(&target, &after).unwrap();
        assert_eq!(
            std::fs::read(&before).unwrap(),
            std::fs::read(&after).unwrap(),
            "tagging changed the audio stream"
        );
        let _ = std::fs::remove_dir_all(&dir);
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

    /// Real MP4 files, committed under `tests/fixtures` and copied in per test.
    ///
    /// They used to be produced by calling ffmpeg, which meant the tests were
    /// skipped wherever it was not installed — including CI, so the tagger's most
    /// important regression went unchecked. A few kilobytes in the repository make
    /// the tests run everywhere.
    fn fixture(name: &str, dir: &Path) -> PathBuf {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name);
        let target = dir.join(name);
        std::fs::copy(&source, &target).expect("тестовый файл должен скопироваться");
        target
    }

    /// A file the tagger can meet: `aac-front` has `moov` before `mdat`, the
    /// layout Yandex serves and the one a tagger can corrupt by growing `moov`
    /// without shifting the chunk offsets.
    fn make_m4a(dir: &Path, faststart: bool) -> PathBuf {
        let name = if faststart { "aac-front.m4a" } else { "aac-back.m4a" };
        fixture(name, dir)
    }

    /// A FLAC stream inside an MP4 container, which is what a lossless download
    /// produces before the stream is demuxed out.
    fn make_flac_in_mp4(dir: &Path) -> PathBuf {
        fixture("flac-in-mp4.m4a", dir)
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

    fn payload_with_cover() -> TagPayload<'static> {
        TagPayload {
            title: "Название".into(),
            artist: vec!["Артист".into()],
            album_artist: vec!["Артист".into()],
            album: "Альбом".into(),
            year: Some(2026),
            track_number: Some(3),
            disc_number: Some(1),
            genre: Some("rock".into()),
            isrc: None,
            label: None,
            explicit: None,
            composer: None,
            cover: Some(tiny_png()),
            lyrics: Some("строка текста".into()),
            url: "https://music.yandex.ru/track/1".into(),
            compatibility_level: 1,
            _marker: std::marker::PhantomData,
        }
    }

    /// Timed lyrics are the point of the LRC option: the timestamps have to reach
    /// the file, not just some lyrics text.
    #[test]
    fn lrc_timestamps_are_stored_verbatim() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("ymdl-lrc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = make_flac_in_mp4(&dir);
        let flac = source.with_extension("flac");
        crate::mp4::extract_flac(&source, &flac).expect("FLAC должен извлечься из mp4");

        let mut payload = payload_with_cover();
        payload.lyrics = Some("[00:12.34] первая строка\n[00:15.00] вторая строка".into());
        write_tags(&flac, &payload).expect("теги должны записаться");

        let tagged = open_tagged(&flac).expect("файл должен открыться");
        let text = tagged
            .primary_tag()
            .and_then(|tag| tag.get_string(&ItemKey::Lyrics))
            .unwrap_or("");
        assert!(text.contains("[00:12.34]"), "тайминги потерялись: {text}");
        assert!(text.contains("вторая строка"), "текст потерялся: {text}");

        let _ = std::fs::remove_dir_all(&dir);
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
        let source = make_m4a(&dir, false);

        // Exactly the temporary name the download engine writes before renaming.
        let temp = dir.join(".track.part.m4a");
        std::fs::copy(&source, &temp).unwrap();
        write_tags(&temp, &payload_with_cover()).expect("теги должны записаться в .part.m4a");

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

    /// Yandex serves `moov` before `mdat` with no padding between them. A tagger
    /// that grows `moov` without shifting the chunk offsets leaves a file that
    /// still reports a codec and a duration but plays noise, so the guard that
    /// rejects such a file is what keeps the audio intact.
    #[test]
    fn a_shifted_media_atom_is_rejected() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("ymdl-play-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = make_m4a(&dir, true);
        let intact = dir.join("intact.m4a");
        std::fs::copy(&source, &intact).unwrap();
        assert!(crate::transcode::decodes(&intact), "целый файл должен декодироваться");

        // Reproduce the damage: push the media payload forward the way an
        // unpatched chunk-offset table does, leaving every sample misaligned.
        let broken = dir.join("broken.m4a");
        let bytes = std::fs::read(&intact).unwrap();
        let media = find_atom(&bytes, b"mdat").expect("в файле должен быть mdat");
        let mut damaged = Vec::with_capacity(bytes.len() + 329);
        damaged.extend_from_slice(&bytes[..media]);
        damaged.extend_from_slice(&[0u8; 329]);
        damaged.extend_from_slice(&bytes[media..]);
        std::fs::write(&broken, &damaged).unwrap();
        assert!(
            !crate::transcode::decodes(&broken),
            "сдвинутый mdat должен считаться битым"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Lossless downloads arrive as FLAC inside MP4 and are demuxed to a real
    /// `.flac` before tagging. Tagging the m4a first meant the fields had to
    /// survive a container change, so the tagger works on the final FLAC — and
    /// that is what this covers.
    #[test]
    fn a_flac_keeps_cover_and_fields() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("ymdl-flac-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = make_flac_in_mp4(&dir);
        let flac = source.with_extension("flac");
        crate::mp4::extract_flac(&source, &flac).expect("FLAC должен извлечься из mp4");
        assert!(flac.is_file());

        write_tags(&flac, &payload_with_cover()).expect("теги должны записаться в flac");

        let summary = read_tags(&flac).expect("flac должен перечитаться");
        assert!(summary.contains("название=\"Название\""), "{summary}");
        assert!(summary.contains("исполнитель=\"Артист\""), "{summary}");
        assert!(summary.contains("альбом=\"Альбом\""), "{summary}");
        assert!(summary.contains("год=\"2026\""), "{summary}");
        assert!(summary.contains("жанр=\"rock\""), "{summary}");
        assert!(summary.contains("обложка=1"), "обложка потерялась: {summary}");
        assert!(summary.contains("лирикс=да"), "лирикс потерялся: {summary}");
        assert!(crate::transcode::decodes(&flac), "flac после тегов не декодируется");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Byte offset of a top-level MP4 atom, or `None` when it is absent.
    fn find_atom(bytes: &[u8], name: &[u8; 4]) -> Option<usize> {        let mut offset = 0usize;
        while offset + 8 <= bytes.len() {
            let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
            if size < 8 {
                return None;
            }
            if &bytes[offset + 4..offset + 8] == name {
                return Some(offset);
            }
            offset = offset.checked_add(size)?;
        }
        None
    }
}
