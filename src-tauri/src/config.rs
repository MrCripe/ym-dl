//! Settings and token storage.
//!
//! The token keeps living in `~/.ym-dl-token` — the file the previous console
//! launcher wrote — so an existing installation needs no migration.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const APP_DIR_QUALIFIER: &str = "";
pub const APP_DIR_ORG: &str = "";
pub const APP_DIR_NAME: &str = "ym-dl";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub username: String,
    pub download_dir: String,
    pub path_pattern: String,
    pub quality: u8,
    pub skip_existing: bool,
    pub embed_cover: bool,
    pub cover_original: bool,
    pub cover_resolution: u32,
    pub lyrics_format: String,
    pub compatibility_level: u8,
    pub only_music: bool,
    pub stick_to_artist: bool,
    pub unsafe_path: bool,
    pub delay: u64,
    pub timeout: u64,
    pub tries: u32,
    pub retry_delay: u64,
    pub extract_flac: bool,
    pub quarantine_lossy: bool,
    pub theme: String,
    pub accent: String,
    pub check_updates: bool,
    pub last_url: String,
    pub concurrent: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            username: String::new(),
            download_dir: String::new(),
            path_pattern: "#track-artist - #title".into(),
            quality: 2,
            skip_existing: true,
            embed_cover: true,
            cover_original: false,
            cover_resolution: 400,
            lyrics_format: "none".into(),
            compatibility_level: 1,
            only_music: true,
            stick_to_artist: false,
            unsafe_path: false,
            delay: 0,
            timeout: 20,
            tries: 6,
            retry_delay: 5,
            extract_flac: true,
            quarantine_lossy: false,
            theme: "system".into(),
            accent: "violet".into(),
            check_updates: true,
            last_url: String::new(),
            concurrent: 2,
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) {
        if self.quality > 2 {
            self.quality = 2;
        }
        if !matches!(self.lyrics_format.as_str(), "none" | "text" | "lrc") {
            self.lyrics_format = "none".into();
        }
        if !matches!(self.theme.as_str(), "system" | "light" | "dark") {
            self.theme = "system".into();
        }
        self.compatibility_level = self.compatibility_level.min(1);
        self.timeout = self.timeout.max(1);
        self.cover_resolution = self.cover_resolution.max(100);
        self.concurrent = self.concurrent.clamp(1, 6);
        // The config file is hand-editable, so every number that drives a wait
        // loop is bounded here: `delay: 999999` would otherwise hang a job.
        self.delay = self.delay.min(300);
        self.retry_delay = self.retry_delay.min(120);
        self.tries = self.tries.min(6);
        if self.path_pattern.trim().is_empty() {
            self.path_pattern = "#track-artist - #title".into();
        }
    }

    pub fn target_dir(&self) -> PathBuf {
        let raw = self.download_dir.trim();
        if raw.is_empty() {
            // Portable build: downloads land next to the exe, not in the profile,
            // because a locked-down profile refuses writes and the job then fails
            // after the track was already resolved.
            return default_download_dir();
        }
        expand_home(raw)
    }
}

pub fn home_dir() -> PathBuf {
    directories::UserDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Where downloads go when the user has not chosen a folder.
///
/// Next to the executable is the portable choice and the only one that works
/// when the profile is locked down; the old `~/Music/lmao` is used only if the
/// exe folder turns out to be read-only (an installed build in Program Files).
pub fn default_download_dir() -> PathBuf {
    if let Some(dir) = exe_dir() {
        let candidate = dir.join("Music");
        if dir_writable(&candidate) {
            return candidate;
        }
    }
    home_dir().join("Music").join("lmao")
}

/// True when `dir` exists and a file can actually be created in it. Creating the
/// directory is not enough: on this kind of machine the folder exists and is
/// listed as writable while every write still fails with access denied.
pub fn dir_writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".ym-dl-write-test");
    match fs::write(&probe, b"1") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Directory of the running executable: the one place that is always writable
/// for a portable build.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Portable data folder: everything the app owns lives next to the exe, so the
/// whole thing can be moved on a flash drive. Falls back to the profile only if
/// the exe folder is read-only.
pub fn portable_dir() -> PathBuf {
    if let Some(dir) = exe_dir() {
        let candidate = dir.join("ym-dl-data");
        if dir_writable(&candidate) {
            return candidate;
        }
    }
    ProjectDirs::from(APP_DIR_QUALIFIER, APP_DIR_ORG, APP_DIR_NAME)
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| home_dir().join(".config").join(APP_DIR_NAME))
}

pub fn config_dir() -> PathBuf {
    portable_dir()
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Portable build: the token travels with the folder, so a copied app stays
/// logged in and nothing is written to the profile.
pub fn token_path() -> PathBuf {
    config_dir().join("token")
}

pub fn expand_home(raw: &str) -> PathBuf {
    let trimmed = raw.trim().trim_matches('"');
    if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
        return home_dir().join(rest);
    }
    PathBuf::from(trimmed)
}

pub fn load_settings() -> Settings {
    let path = config_path();
    let mut settings = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .unwrap_or_default();
    settings.sanitize();
    settings
}

pub fn save_settings(settings: &Settings) -> Result<()> {
    let path = config_path();
    write_atomic(&path, serde_json::to_vec_pretty(settings)?.as_slice())?;
    Ok(())
}

/// The token that shipped with the previous version, kept only to migrate it
/// once into the portable folder.
fn legacy_token_path() -> PathBuf {
    home_dir().join(".ym-dl-token")
}

pub fn read_token() -> String {
    if let Ok(value) = fs::read_to_string(token_path()) {
        return value.trim().to_string();
    }
    // One-time migration: an existing installation keeps its login.
    let legacy = legacy_token_path();
    if let Ok(value) = fs::read_to_string(&legacy) {
        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() && write_token(&trimmed).is_ok() {
            let _ = fs::remove_file(&legacy);
            return trimmed;
        }
        return trimmed;
    }
    std::env::var("YM_TOKEN").unwrap_or_default()
}

pub fn write_token(token: &str) -> Result<()> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return clear_token();
    }
    write_atomic(&token_path(), trimmed.as_bytes())
}

pub fn clear_token() -> Result<()> {
    // Both locations: the portable one and the file an older build used, or a
    // "reset" would silently keep the old login alive.
    let mut last_error = None;
    for path in [token_path(), legacy_token_path()] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => last_error = Some(error),
        }
    }
    match last_error {
        Some(error) => Err(error).context("не удалось удалить файл токена"),
        None => Ok(()),
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("не удалось создать {}", parent.display()))?;
    }
    // A fixed temp name is shared by every writer, so two writes at once left one
    // renaming a file the other had already moved away.
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let ticket = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = path.with_extension(format!("{}.{}.tmp", std::process::id(), ticket));
    fs::write(&temp, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&temp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_clamps_values() {
        let mut settings = Settings {
            quality: 9,
            lyrics_format: "weird".into(),
            theme: "neon".into(),
            timeout: 0,
            cover_resolution: 10,
            concurrent: 99,
            compatibility_level: 7,
            delay: 999_999,
            retry_delay: 99_999,
            tries: 5_000,
            ..Settings::default()
        };
        settings.sanitize();
        assert_eq!(settings.quality, 2);
        assert_eq!(settings.lyrics_format, "none");
        assert_eq!(settings.theme, "system");
        assert_eq!(settings.timeout, 1);
        assert_eq!(settings.cover_resolution, 100);
        assert_eq!(settings.concurrent, 6);
        assert_eq!(settings.compatibility_level, 1);
        // A hand-edited config must not be able to hang a job for days.
        assert_eq!(settings.delay, 300);
        assert_eq!(settings.retry_delay, 120);
        assert_eq!(settings.tries, 6);
    }

    #[test]
    fn default_target_dir_is_writable_and_not_the_profile() {
        // Portable build: downloads go next to the exe, which is the one place
        // that works when the user profile refuses writes.
        let mut settings = Settings::default();
        settings.download_dir.clear();
        let dir = settings.target_dir();
        assert!(dir_writable(&dir), "default download dir must accept writes: {}", dir.display());
        assert!(
            dir.ends_with("Music") || dir.ends_with("Music/lmao") || dir.ends_with("Music\\lmao"),
            "unexpected default: {}",
            dir.display()
        );
    }

    #[test]
    fn portable_dir_is_writable() {
        let dir = portable_dir();
        assert!(dir_writable(&dir), "portable data dir must be writable: {}", dir.display());
    }
}
