//! Files written next to the audio: the cover and the playlist.
//!
//! Both are for players and file managers rather than for the app itself. A
//! `cover.jpg` next to a track is what a phone shows in a folder view, and an
//! `.m3u8` is what a NAS or a car stereo reads when it wants the album in order.

use std::path::{Path, PathBuf};

/// The name a cover file gets. Lowercase because the players that look for one
/// on a case-sensitive filesystem all look for this spelling.
pub const COVER_NAME: &str = "cover.jpg";

/// Writes `cover.jpg` beside `track`, keeping the bytes the tag already holds.
///
/// Returns the path written, or `None` when there is no cover to write. A file
/// that is already there is left alone: it may be one the user replaced.
pub fn write_cover(track: &Path, cover: Option<&[u8]>) -> Option<PathBuf> {
    let bytes = cover?;
    if bytes.is_empty() {
        return None;
    }
    let directory = track.parent()?;
    let target = directory.join(COVER_NAME);
    if target.is_file() {
        return Some(target);
    }
    // Written to a temporary name and moved: a half-written image would be
    // picked up by a player and shown as a broken cover.
    let part = directory.join(format!(".{COVER_NAME}.part"));
    std::fs::write(&part, bytes).ok()?;
    std::fs::rename(&part, &target).ok()?;
    Some(target)
}

/// Escapes one path for an M3U line.
///
/// The format is plain text with one path per line; a relative path is resolved
/// against the playlist's own folder, which is what makes an album folder
/// movable.
fn playlist_line(path: &Path, base: &Path) -> String {
    let text = relative_to(path, base);
    // A line starting with '#' is a comment in M3U, so a file named that way
    // would silently drop out of the list.
    if text.starts_with('#') {
        format!("./{text}")
    } else {
        text
    }
}

/// `path` expressed relative to `base`, walking up as far as needed.
///
/// A batch can span folders — a template that puts each album in its own — and
/// the playlist lives in the first track's folder. An absolute line would work
/// only on the machine that wrote it, which defeats a movable library.
fn relative_to(path: &Path, base: &Path) -> String {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let base = std::path::absolute(base).unwrap_or_else(|_| base.to_path_buf());
    let mut from: Vec<_> = base.components().collect();
    let mut to: Vec<_> = path.components().collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    from.drain(..common);
    to.drain(..common);
    let mut parts: Vec<String> = from.iter().map(|_| "..".to_string()).collect();
    parts.extend(to.iter().map(|part| part.as_os_str().to_string_lossy().to_string()));
    parts.join("/")
}

/// Writes an `.m3u8` playlist for the given tracks, or returns `None` when there
/// is nothing to write.
///
/// The playlist goes into the folder the tracks share, named after `title`, so a
/// player that opens it finds the files by relative path.
pub fn write_playlist(title: &str, tracks: &[PathBuf]) -> Option<PathBuf> {
    let first = tracks.first()?;
    let directory = first.parent()?;
    let name = crate::audio::sanitize_file_name(title);
    let name = if name.is_empty() { "playlist".to_string() } else { name };
    let target = directory.join(format!("{name}.m3u8"));

    let mut body = String::from("#EXTM3U\n");
    for track in tracks {
        if !track.is_file() {
            continue;
        }
        body.push_str(&playlist_line(track, directory));
        body.push('\n');
    }
    if body.lines().count() <= 1 {
        return None;
    }
    std::fs::write(&target, body).ok()?;
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("sidecar-test")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_cover_is_written_once_and_never_overwritten() {
        let dir = scratch("cover");
        let track = dir.join("Song.flac");
        std::fs::write(&track, b"audio").unwrap();

        let written = write_cover(&track, Some(b"\xFF\xD8picture")).expect("cover must be written");
        assert_eq!(std::fs::read(&written).unwrap(), b"\xFF\xD8picture");

        // A second run with a different image must keep the first one: it may be
        // the one the user chose by hand.
        let again = write_cover(&track, Some(b"other")).unwrap();
        assert_eq!(std::fs::read(&again).unwrap(), b"\xFF\xD8picture");
        // No temporary file is left behind.
        assert!(!dir.join(".cover.jpg.part").exists());

        assert!(write_cover(&track, None).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_playlist_lists_relative_paths() {
        let dir = scratch("m3u");
        let first = dir.join("01 - One.flac");
        let second = dir.join("02 - Two.flac");
        std::fs::write(&first, b"a").unwrap();
        std::fs::write(&second, b"b").unwrap();

        let playlist = write_playlist("Мой альбом", &[first, second]).expect("playlist written");
        let body = std::fs::read_to_string(&playlist).unwrap();
        assert!(body.starts_with("#EXTM3U\n"), "{body}");
        assert!(body.contains("01 - One.flac"), "{body}");
        assert!(body.contains("02 - Two.flac"), "{body}");
        // A path with a drive letter would make the folder unmovable.
        assert!(!body.contains(':'), "{body}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_playlist_with_no_readable_track_is_not_written() {
        let dir = scratch("m3u-empty");
        let missing = dir.join("gone.flac");
        assert!(write_playlist("nothing", &[missing]).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
