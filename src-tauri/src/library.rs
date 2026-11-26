//! Local index of downloaded tracks.
//!
//! One SQLite file next to the settings holds what was downloaded, where it
//! landed and what the file says about itself. It exists for three jobs:
//!
//! 1. The library screen: sorting, filtering, editing tags, deleting files.
//! 2. Not downloading the same track twice, even after the path template
//!    changes — the old check only looked at whether one expected file existed.
//! 3. Deciding whether a track can be replaced by a better quality version.
//!
//! The database is disposable: every field can be rebuilt by reading the tags of
//! the files themselves, which is what `scan` does.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// One downloaded file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryTrack {
    /// Row id, not the Yandex id: two files of the same track are two rows.
    pub id: i64,
    pub track_id: String,
    pub isrc: String,
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub genre: String,
    pub year: Option<i32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_ms: u64,
    pub codec: String,
    pub bitrate: u32,
    pub path: String,
    pub size_bytes: u64,
    pub added_at: i64,
}

/// Editable fields: only what the user can change in the tag editor.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagPatch {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub scanned: usize,
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    /// Rows dropped because the file they pointed at is gone.
    pub removed: usize,
}

/// Set once by a test so the registry does not touch the real library.
static DB_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

pub fn db_path() -> PathBuf {
    DB_OVERRIDE
        .get()
        .cloned()
        .unwrap_or_else(|| crate::config::config_dir().join("library.db"))
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|span| span.as_secs() as i64)
        .unwrap_or(0)
}

/// Schema generation, stored in the database itself as `PRAGMA user_version`.
///
/// The table and its indexes used to be re-created on every `connect()`, which
/// means every row of a scan and every cover read paid for six DDL statements.
/// The marker lives in the file rather than in a process-wide flag: a deleted or
/// replaced database is then rebuilt on the next call instead of answering
/// `no such table` until the app is restarted, and a test that points the
/// registry at a fresh file gets a schema of its own.
const SCHEMA_VERSION: i64 = 1;

/// Opens the database, creating it on first use.
///
/// A connection per call rather than one shared handle: downloads record from
/// worker threads while the UI queries from the command thread, and SQLite
/// already serialises writers. WAL plus a busy timeout covers the overlap with no
/// shared state to lock.
pub fn connect() -> Result<Connection> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("не удалось создать {}", parent.display()))?;
    }
    let connection = Connection::open(&path)
        .with_context(|| format!("не удалось открыть {}", path.display()))?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
    let version: i64 =
        connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(connection);
    }
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         CREATE TABLE IF NOT EXISTS tracks (
             id            INTEGER PRIMARY KEY AUTOINCREMENT,
             track_id      TEXT NOT NULL DEFAULT '',
             isrc          TEXT NOT NULL DEFAULT '',
             title         TEXT NOT NULL DEFAULT '',
             artist        TEXT NOT NULL DEFAULT '',
             album_artist  TEXT NOT NULL DEFAULT '',
             album         TEXT NOT NULL DEFAULT '',
             genre         TEXT NOT NULL DEFAULT '',
             year          INTEGER,
             track_number  INTEGER,
             disc_number   INTEGER,
             duration_ms   INTEGER NOT NULL DEFAULT 0,
             codec         TEXT NOT NULL DEFAULT '',
             bitrate       INTEGER NOT NULL DEFAULT 0,
             path          TEXT NOT NULL UNIQUE,
             size_bytes    INTEGER NOT NULL DEFAULT 0,
             added_at      INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS tracks_track_id ON tracks(track_id);
         CREATE INDEX IF NOT EXISTS tracks_isrc ON tracks(isrc);
         CREATE INDEX IF NOT EXISTS tracks_artist ON tracks(artist);
         CREATE INDEX IF NOT EXISTS tracks_album ON tracks(album);
         CREATE INDEX IF NOT EXISTS tracks_year ON tracks(year);",
    )?;
    connection.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
    Ok(connection)
}

const COLUMNS: &str = "id, track_id, isrc, title, artist, album_artist, album, genre, year,
                       track_number, disc_number, duration_ms, codec, bitrate, path,
                       size_bytes, added_at";

fn to_track(row: &Row<'_>) -> rusqlite::Result<LibraryTrack> {
    Ok(LibraryTrack {
        id: row.get(0)?,
        track_id: row.get(1)?,
        isrc: row.get(2)?,
        title: row.get(3)?,
        artist: row.get(4)?,
        album_artist: row.get(5)?,
        album: row.get(6)?,
        genre: row.get(7)?,
        year: row.get(8)?,
        track_number: row.get(9)?,
        disc_number: row.get(10)?,
        duration_ms: row.get::<_, i64>(11)?.max(0) as u64,
        codec: row.get(12)?,
        bitrate: row.get(13)?,
        path: row.get(14)?,
        size_bytes: row.get::<_, i64>(15)?.max(0) as u64,
        added_at: row.get(16)?,
    })
}

/// Inserts a file, or refreshes the row that already points at the same path.
pub fn record(entry: &LibraryTrack) -> Result<i64> {
    let connection = connect()?;
    let added = if entry.added_at > 0 { entry.added_at } else { now() };
    connection.execute(
        "INSERT INTO tracks (track_id, isrc, title, artist, album_artist, album, genre,
                             year, track_number, disc_number, duration_ms, codec, bitrate,
                             path, size_bytes, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(path) DO UPDATE SET
             track_id = excluded.track_id,
             isrc = excluded.isrc,
             title = excluded.title,
             artist = excluded.artist,
             album_artist = excluded.album_artist,
             album = excluded.album,
             genre = excluded.genre,
             year = excluded.year,
             track_number = excluded.track_number,
             disc_number = excluded.disc_number,
             duration_ms = excluded.duration_ms,
             codec = excluded.codec,
             bitrate = excluded.bitrate,
             size_bytes = excluded.size_bytes",
        params![
            entry.track_id,
            entry.isrc,
            entry.title,
            entry.artist,
            entry.album_artist,
            entry.album,
            entry.genre,
            entry.year,
            entry.track_number,
            entry.disc_number,
            entry.duration_ms as i64,
            entry.codec,
            entry.bitrate,
            entry.path,
            entry.size_bytes as i64,
            added,
        ],
    )?;
    Ok(connection.query_row(
        "SELECT id FROM tracks WHERE path = ?1",
        params![entry.path],
        |row| row.get::<_, i64>(0),
    )?)
}

/// Column for each sort the interface offers.
///
/// The value arrives from the frontend, so it is a lookup rather than text pasted
/// into the query: an unknown name must never become SQL.
fn sort_column(sort: &str) -> &'static str {
    match sort {
        "artist" => "artist COLLATE NOCASE",
        "album" => "album COLLATE NOCASE",
        "year" => "year",
        "title" => "title COLLATE NOCASE",
        "duration" => "duration_ms",
        _ => "added_at",
    }
}

/// Everything in the library, ordered and optionally narrowed by a search word.
pub fn list(sort: &str, descending: bool, filter: &str) -> Result<Vec<LibraryTrack>> {
    let connection = connect()?;
    let column = sort_column(sort);
    let direction = if descending { "DESC" } else { "ASC" };
    // `IS NULL` first keeps tracks with no year or no album from piling up at the
    // top of an ascending sort, which is where SQLite would otherwise put them.
    let query = format!(
        "SELECT {COLUMNS} FROM tracks
         ORDER BY {column} IS NULL, {column} {direction}, title COLLATE NOCASE ASC"
    );
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map([], to_track)?;
    // The word is matched here, not in SQL: `LIKE` and `COLLATE NOCASE` fold the
    // ASCII range only, so a Cyrillic title in capitals never matched a lowercase
    // query. Rust's `to_lowercase` is the full Unicode table, which is what a
    // person typing in the search box expects.
    let needle = filter.trim().to_lowercase();
    let mut result = Vec::new();
    for row in rows {
        let entry = row?;
        if needle.is_empty() || contains(&entry, &needle) {
            result.push(entry);
        }
    }
    Ok(result)
}

/// Whether any of the three visible names contains the (lowercased) word.
fn contains(entry: &LibraryTrack, needle: &str) -> bool {
    [&entry.title, &entry.artist, &entry.album]
        .iter()
        .any(|value| value.to_lowercase().contains(needle))
}

pub fn get(id: i64) -> Result<Option<LibraryTrack>> {
    let connection = connect()?;
    let mut statement =
        connection.prepare(&format!("SELECT {COLUMNS} FROM tracks WHERE id = ?1"))?;
    let mut rows = statement.query_map(params![id], to_track)?;
    Ok(rows.next().transpose()?)
}

/// The first file already downloaded for a Yandex track id.
pub fn find_by_track_id(track_id: &str) -> Result<Option<LibraryTrack>> {
    if track_id.is_empty() {
        return Ok(None);
    }
    let connection = connect()?;
    let mut statement = connection.prepare(&format!(
        "SELECT {COLUMNS} FROM tracks WHERE track_id = ?1 LIMIT 1"
    ))?;
    let mut rows = statement.query_map(params![track_id], to_track)?;
    Ok(rows.next().transpose()?)
}

/// Applies an edited tag to the row and hands back the updated record.
pub fn update_metadata(id: i64, patch: &TagPatch) -> Result<Option<LibraryTrack>> {
    let Some(mut entry) = get(id)? else {
        return Ok(None);
    };
    if let Some(value) = &patch.title {
        entry.title = value.clone();
    }
    if let Some(value) = &patch.artist {
        entry.artist = value.clone();
    }
    if let Some(value) = &patch.album_artist {
        entry.album_artist = value.clone();
    }
    if let Some(value) = &patch.album {
        entry.album = value.clone();
    }
    if let Some(value) = &patch.genre {
        entry.genre = value.clone();
    }
    if let Some(value) = patch.year {
        entry.year = Some(value);
    }
    if let Some(value) = patch.track_number {
        entry.track_number = Some(value);
    }
    if let Some(value) = patch.disc_number {
        entry.disc_number = Some(value);
    }
    connect()?.execute(
        "UPDATE tracks SET title = ?1, artist = ?2, album_artist = ?3, album = ?4,
                          genre = ?5, year = ?6, track_number = ?7, disc_number = ?8
         WHERE id = ?9",
        params![
            entry.title,
            entry.artist,
            entry.album_artist,
            entry.album,
            entry.genre,
            entry.year,
            entry.track_number,
            entry.disc_number,
            id,
        ],
    )?;
    Ok(Some(entry))
}

/// Removes the row and hands back the record it pointed at.
pub fn forget(id: i64) -> Result<Option<LibraryTrack>> {
    let entry = get(id)?;
    if entry.is_some() {
        connect()?.execute("DELETE FROM tracks WHERE id = ?1", params![id])?;
    }
    Ok(entry)
}

/// Audio files the library is allowed to index.
fn is_audio(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("mp3" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wav")
    )
}

/// Rebuilds the index from the files themselves.
///
/// This is both the first-run import of a folder filled before the library
/// existed, and the repair path when the database is lost. A file that cannot be
/// parsed counts as skipped instead of aborting the walk.
pub fn scan(root: &Path) -> Result<ScanReport> {
    let mut report = ScanReport::default();
    if !root.is_dir() {
        return Ok(report);
    }
    // Repair first: rows whose file is gone would otherwise stay in the library
    // forever, and the user would have to delete each one by hand.
    report.removed = prune()?;
    let mut stack = vec![root.to_path_buf()];
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // `symlink_metadata` does not follow links, so a loop cannot make this
            // walk forever.
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_symlink() {
                continue;
            }
            if meta.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // The folders this app itself uses for leftovers and for its own
                // data are not part of the music library.
                if name != "_old_m4a" && name != "ym-dl-data" {
                    stack.push(path);
                }
                continue;
            }
            if is_audio(&path) {
                files.push(path);
            }
        }
    }

    let known: std::collections::HashSet<String> = list("added", true, "")?
        .into_iter()
        .map(|entry| entry.path)
        .collect();

    for path in files {
        report.scanned += 1;
        let display = path.display().to_string();
        let Ok(metadata) = crate::audio::read_file_metadata(&path) else {
            report.skipped += 1;
            continue;
        };
        let entry = LibraryTrack {
            track_id: crate::audio::track_id_from_url(&metadata.comment).unwrap_or_default(),
            isrc: metadata.isrc,
            title: metadata.title,
            artist: metadata.artist,
            album_artist: metadata.album_artist,
            album: metadata.album,
            genre: metadata.genre,
            year: metadata.year,
            track_number: metadata.track_number,
            disc_number: metadata.disc_number,
            duration_ms: metadata.duration_ms,
            codec: metadata.codec,
            bitrate: metadata.bitrate,
            size_bytes: std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0),
            path: display.clone(),
            ..Default::default()
        };
        let existed = known.contains(&display);
        record(&entry)?;
        if existed {
            report.updated += 1;
        } else {
            report.added += 1;
        }
    }
    Ok(report)
}

/// Drops rows whose file is gone, so the library never offers a dead path.
pub fn prune() -> Result<usize> {
    let connection = connect()?;
    let mut statement = connection.prepare(&format!("SELECT {COLUMNS} FROM tracks"))?;
    let entries: Vec<LibraryTrack> = statement
        .query_map([], to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let gone: Vec<i64> = entries
        .into_iter()
        .filter(|entry| !Path::new(&entry.path).is_file())
        .map(|entry| entry.id)
        .collect();
    if gone.is_empty() {
        return Ok(0);
    }
    // One transaction for the whole sweep: a row per statement meant a disk sync
    // per missing file, which is the slowest part of scanning a big folder.
    let transaction = connection.unchecked_transaction()?;
    for id in &gone {
        transaction.execute("DELETE FROM tracks WHERE id = ?1", params![id])?;
    }
    transaction.commit()?;
    Ok(gone.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test drives the whole registry: the database is process-wide, so a
    /// second test would fight this one for the same file.
    #[test]
    fn the_registry_records_lists_edits_and_forgets() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("library-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        DB_OVERRIDE.set(dir.join("library.db")).ok();

        let first = LibraryTrack {
            track_id: "42".into(),
            title: "Song".into(),
            artist: "Band".into(),
            album: "Record".into(),
            year: Some(1991),
            codec: "FLAC".into(),
            bitrate: 900,
            path: "/music/Band/Record/Song.flac".into(),
            ..Default::default()
        };
        let id = record(&first).expect("a track must be recordable");
        assert!(id > 0);

        // Recording the same path twice refreshes instead of duplicating.
        record(&first).unwrap();
        assert_eq!(list("added", true, "").unwrap().len(), 1);

        // The dedupe lookup is what stops a re-download after a template change.
        let found = find_by_track_id("42").unwrap().expect("track id must be found");
        assert_eq!(found.title, "Song");

        // A search word narrows the list, and an unknown sort must not reach SQL.
        assert_eq!(list("artist", false, "ban").unwrap().len(), 1);
        assert!(list("artist", false, "nothing").unwrap().is_empty());
        assert_eq!(list("added_at; DROP TABLE tracks", true, "").unwrap().len(), 1);

        // Case is folded for Cyrillic as well: a capitalised title must answer to
        // a lowercase query, which is where SQLite's own folding gives up.
        let cyrillic = LibraryTrack {
            track_id: "43".into(),
            title: "БЛЭЙД".into(),
            artist: "Исполнитель".into(),
            path: "/music/БЛЭЙД.flac".into(),
            ..Default::default()
        };
        let cyrillic_id = record(&cyrillic).unwrap();
        assert_eq!(list("added", true, "блэйд").unwrap().len(), 1);
        assert_eq!(list("added", true, "исполн").unwrap().len(), 1);
        assert!(list("added", true, "нет такого").unwrap().is_empty());
        forget(cyrillic_id).unwrap();

        let edited = update_metadata(
            id,
            &TagPatch {
                title: Some("Song (Remastered)".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .expect("the row must exist");
        assert_eq!(edited.title, "Song (Remastered)");

        // Forgetting returns the record so the caller knows which file to delete.
        let gone = forget(id).unwrap().expect("the row must exist");
        assert_eq!(gone.path, "/music/Band/Record/Song.flac");
        assert!(list("added", true, "").unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_sort_falls_back_instead_of_reaching_the_query() {
        assert_eq!(sort_column("added_at; DROP TABLE tracks"), "added_at");
        assert_eq!(sort_column("artist"), "artist COLLATE NOCASE");
    }

    #[test]
    fn a_track_id_is_read_back_from_the_page_url() {
        assert_eq!(
            crate::audio::track_id_from_url("https://music.yandex.ru/album/1/track/42").as_deref(),
            Some("42")
        );
        assert_eq!(crate::audio::track_id_from_url("no url here"), None);
    }
}
