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
            return home_dir().join("Music").join("lmao");
        }
        expand_home(raw)
    }
}

pub fn home_dir() -> PathBuf {
    directories::UserDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_dir() -> PathBuf {
    if let Some(dirs) = ProjectDirs::from(APP_DIR_QUALIFIER, APP_DIR_ORG, APP_DIR_NAME) {
        return dirs.config_dir().to_path_buf();
    }
    home_dir().join(".config").join(APP_DIR_NAME)
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn token_path() -> PathBuf {
    home_dir().join(".ym-dl-token")
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

pub fn read_token() -> String {
    fs::read_to_string(token_path())
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|_| std::env::var("YM_TOKEN").unwrap_or_default())
}

pub fn write_token(token: &str) -> Result<()> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return clear_token();
    }
    write_atomic(&token_path(), trimmed.as_bytes())
}

pub fn clear_token() -> Result<()> {
    match fs::remove_file(token_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("не удалось удалить файл токена"),
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("не удалось создать {}", parent.display()))?;
    }
    let temp = path.with_extension("tmp");
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
    fn default_target_dir_ends_with_music_lmao() {
        let mut settings = Settings::default();
        settings.download_dir.clear();
        assert!(settings.target_dir().ends_with("Music/lmao") || settings.target_dir().ends_with("Music\\lmao"));
    }
}
