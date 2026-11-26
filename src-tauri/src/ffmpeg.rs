//! ffmpeg/ffprobe helpers.
//!
//! Yandex serves lossless as FLAC inside an MP4 container, so a finished file
//! can be named `.m4a` while holding a real FLAC stream. `extract_flac` remuxes
//! the stream into a `.flac` file and verifies the result before replacing
//! anything; a failed remux leaves the original untouched.

use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
pub(crate) fn hidden(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(windows))]
pub(crate) fn hidden(command: &mut Command) -> &mut Command {
    command
}

pub fn find(binary: &str) -> Option<PathBuf> {
    let name = if cfg!(windows) { format!("{binary}.exe") } else { binary.to_string() };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(&name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // Common install locations that are not always on PATH.
    let fallbacks = if cfg!(windows) {
        vec![
            PathBuf::from(r"C:\ffmpeg\bin").join(&name),
            PathBuf::from(r"C:\Program Files\ffmpeg\bin").join(&name),
        ]
    } else {
        vec![
            PathBuf::from("/usr/bin").join(&name),
            PathBuf::from("/usr/local/bin").join(&name),
            PathBuf::from("/snap/bin").join(&name),
        ]
    };
    fallbacks.into_iter().find(|path| path.is_file())
}

pub fn ffmpeg_path() -> Option<PathBuf> {
    find("ffmpeg")
}

pub fn ffprobe_path() -> Option<PathBuf> {
    find("ffprobe")
}

pub fn available() -> bool {
    ffmpeg_path().is_some() && ffprobe_path().is_some()
}

/// True when the whole audio stream decodes without a single error.
///
/// Probing is not enough: a container whose chunk offsets no longer match the
/// media still reports a codec name and a duration, yet plays silence or noise.
/// Only a full decode catches that, so every tag rewrite is checked this way
/// before it replaces the original file.
pub fn decodes(path: &Path) -> bool {
    let Some(ffmpeg) = ffmpeg_path() else {
        return false;
    };
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-xerror", "-i"]);
    command.arg(path);
    command.args(["-f", "null", "-"]);
    let Ok(output) = hidden(&mut command).output() else {
        return false;
    };
    output.status.success() && output.stderr.is_empty()
}

/// Returns `"converted"`, `"exists"` or `"failed"`.
pub fn extract_flac(source: &Path) -> String {
    let Some(ffmpeg) = ffmpeg_path() else {
        return "failed".into();
    };
    let target = source.with_extension("flac");
    if target.exists() {
        return if codec_of(&target) == "flac" { "exists".into() } else { "failed".into() };
    }
    let part = source.with_file_name(format!(
        ".{}.part.flac",
        source.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    ));

    let attempts: [Vec<&str>; 2] = [
        vec!["-map", "0:a:0", "-map", "0:v?", "-map_metadata", "0", "-c:a", "copy", "-c:v", "copy", "-disposition:v", "attached_pic", "-f", "flac"],
        vec!["-map", "0:a:0", "-map_metadata", "0", "-vn", "-c:a", "copy", "-f", "flac"],
    ];
    for extra in attempts {
        let mut command = Command::new(&ffmpeg);
        command.args(["-y", "-hide_banner", "-loglevel", "error", "-nostdin", "-i"]);
        command.arg(source);
        command.args(&extra);
        command.arg(&part);
        let ok = hidden(&mut command)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
            && part.is_file()
            && part.metadata().map(|meta| meta.len() > 0).unwrap_or(false)
            && codec_of(&part) == "flac";
        // A file that only looks like FLAC is not enough: the source is deleted
        // right after a successful conversion, so the result has to decode end to
        // end first. A remux that produced a truncated file must not cost the user
        // their only copy of the track.
        if ok && decodes(&part) && std::fs::rename(&part, &target).is_ok() {
            return "converted".into();
        }
        let _ = std::fs::remove_file(&part);
    }
    "failed".into()
}

pub fn codec_of(path: &Path) -> String {
    let Some(ffprobe) = ffprobe_path() else {
        return String::new();
    };
    let mut command = Command::new(ffprobe);
    command.args([
        "-v", "error", "-select_streams", "a:0", "-show_entries", "stream=codec_name",
        "-of", "default=noprint_wrappers=1:nokey=1",
    ]);
    command.arg(path);
    match hidden(&mut command).output() {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase(),
        _ => String::new(),
    }
}

/// Removes the `.m4a` that a FLAC was extracted from.
///
/// The extracted `.flac` holds the same audio, so keeping both just doubles the
/// library size. Called only after the conversion succeeded and the result was
/// decoded end to end.
pub fn drop_source(path: &Path) -> bool {
    std::fs::remove_file(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_is_reported_as_none() {
        assert!(find("definitely-not-a-real-binary-ymdl").is_none());
    }

    #[test]
    fn the_source_m4a_is_removed_after_extraction() {
        // Keeping both `.m4a` and `.flac` doubles the library for no gain, so the
        // source is deleted once the conversion has produced the FLAC.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("ymdl-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("track.m4a");
        std::fs::write(&source, b"data").unwrap();
        assert!(drop_source(&source));
        assert!(!source.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
