//! ffmpeg/ffprobe helpers.
//!
//! Yandex serves lossless as FLAC inside an MP4 container, so a finished file
//! can be named `.m4a` while holding a real FLAC stream. `extract_flac` remuxes
//! the stream into a `.flac` file and verifies the result before replacing
//! anything; a failed remux leaves the original untouched.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const BACKUP_DIR: &str = "_old_m4a";

#[cfg(windows)]
fn hidden(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(windows))]
fn hidden(command: &mut Command) -> &mut Command {
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
        if ok {
            if std::fs::rename(&part, &target).is_ok() {
                return "converted".into();
            }
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

/// Moves a file into the `_old_m4a` backup folder inside `root`.
pub fn quarantine(path: &Path, root: &Path) -> bool {
    let backup = root.join(BACKUP_DIR);
    if std::fs::create_dir_all(&backup).is_err() {
        return false;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.subsec_nanos())
        .unwrap_or(0);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    std::fs::rename(path, backup.join(format!("{stamp:08x}-{name}"))).is_ok()
}

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostProcessStats {
    pub converted: usize,
    pub quarantined: usize,
    pub failed: usize,
}

/// Walks `root` once: extracts FLAC from `.m4a` files and optionally moves
/// lossy leftovers aside so `skip-existing` does not lock them in.
pub fn post_process(root: &Path, quarantine_lossy: bool) -> PostProcessStats {
    let mut stats = PostProcessStats::default();
    if !root.is_dir() {
        return stats;
    }
    let backup = root.join(BACKUP_DIR);
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path != backup {
                    stack.push(path);
                }
                continue;
            }
            let extension = path
                .extension()
                .map(|value| value.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if !matches!(extension.as_str(), "m4a" | "mp3" | "flac") {
                continue;
            }
            let codec = codec_of(&path);
            if extension == "m4a" && codec == "flac" {
                match extract_flac(&path).as_str() {
                    "converted" => {
                        stats.converted += 1;
                        quarantine(&path, root);
                    }
                    "exists" => {
                        quarantine(&path, root);
                    }
                    _ => stats.failed += 1,
                }
            } else if quarantine_lossy && matches!(codec.as_str(), "aac" | "mp3") && quarantine(&path, root) {
                stats.quarantined += 1;
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_is_reported_as_none() {
        assert!(find("definitely-not-a-real-binary-ymdl").is_none());
    }

    #[test]
    fn quarantine_moves_file_into_backup_folder() {
        let root = std::env::temp_dir().join(format!("ymdl-quarantine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("track.m4a");
        std::fs::write(&source, b"data").unwrap();
        assert!(quarantine(&source, &root));
        assert!(!source.exists());
        assert!(root.join(BACKUP_DIR).read_dir().unwrap().next().is_some());
        let _ = std::fs::remove_dir_all(&root);
    }
}
