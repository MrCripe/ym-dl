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
    /// Ask lrclib.net when Yandex has no lyrics for a track.
    pub lyrics_fallback: bool,
    pub compatibility_level: u8,
    pub only_music: bool,
    pub stick_to_artist: bool,
    pub unsafe_path: bool,
    pub delay: u64,
    pub timeout: u64,
    pub tries: u32,
    pub retry_delay: u64,
    pub extract_flac: bool,
    pub theme: String,
    pub accent: String,
    /// Interface language: `auto` follows the system, otherwise `ru` or `en`.
    pub language: String,
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
            lyrics_fallback: false,
            compatibility_level: 1,
            only_music: true,
            stick_to_artist: false,
            unsafe_path: false,
            delay: 0,
            timeout: 20,
            tries: 6,
            retry_delay: 5,
            extract_flac: true,
            theme: "system".into(),
            accent: "violet".into(),
            language: "auto".into(),
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
        if !matches!(self.language.as_str(), "auto" | "ru" | "en") {
            self.language = "auto".into();
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

/// Where downloads go when the user has not chosen a folder: the system's music
/// folder, on both Windows and Linux.
///
/// If that folder cannot be written to (a locked-down profile), the portable
/// folder next to the executable is used instead, so a download never fails just
/// because the default was unreachable.
pub fn default_download_dir() -> PathBuf {
    // `audio_dir` honours XDG_MUSIC_DIR on Linux and the shell's Music folder on
    // Windows; `home_dir()/Music` would create a second folder on a system that
    // moved it.
    let music = directories::UserDirs::new()
        .and_then(|dirs| dirs.audio_dir().map(Path::to_path_buf))
        .unwrap_or_else(|| home_dir().join("Music"));
    if dir_writable(&music) {
        return music;
    }
    if let Some(dir) = exe_dir() {
        let candidate = dir.join("Music");
        if dir_writable(&candidate) {
            return candidate;
        }
    }
    music
}

/// Folders an earlier build used as its default download location.
///
/// The default moved twice: `~/Music/lmao`, then `~/Music/ym-dl`, then `~/Music`.
/// A user who updates keeps an empty setting, so without this the app would start
/// writing to a new folder and `skip-existing` would no longer see the library
/// that is already on disk.
const LEGACY_DOWNLOAD_DIRS: [&str; 2] = ["ym-dl", "lmao"];

/// Points an untouched setting at the library an earlier build created.
fn adopt_legacy_download_dir(settings: &mut Settings) {
    if !settings.download_dir.trim().is_empty() {
        return;
    }
    for name in LEGACY_DOWNLOAD_DIRS {
        let legacy = home_dir().join("Music").join(name);
        if legacy.is_dir() {
            settings.download_dir = legacy.display().to_string();
            return;
        }
    }
    // A portable build used to download next to the executable.
    if let Some(dir) = exe_dir() {
        let legacy = dir.join("Music");
        if legacy.is_dir() && legacy != home_dir().join("Music") {
            settings.download_dir = legacy.display().to_string();
        }
    }
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

/// True when the app runs from an unpacked portable archive.
///
/// The check must not look at `ym-dl-data`: `portable_dir()` creates that folder
/// itself on the first run, so an installed build would look portable too. The
/// portable archives ship a marker file next to the executable instead.
///
/// The updater installs an NSIS/MSI build into the profile, so a portable copy
/// cannot update itself in place: pressing "Обновить" there would install a second
/// copy somewhere else. The interface says so instead of pretending.
pub fn is_portable() -> bool {
    exe_dir().map(|dir| dir.join("portable").is_file()).unwrap_or(false)
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

/// Where the previous run kept its data.
///
/// The in-app updater installs the new build into the user profile, so after an
/// update the exe sits in a different folder and its `ym-dl-data` — settings and
/// token — is not where the new build looks. This marker lets the new build find
/// the old folder once and take it over.
fn handover_marker() -> Option<PathBuf> {
    ProjectDirs::from(APP_DIR_QUALIFIER, APP_DIR_ORG, APP_DIR_NAME)
        .map(|dirs| dirs.config_dir().join("last-data-dir"))
}

/// Records the current data folder so a build started from somewhere else can
/// adopt it. Called at startup, before anything is read.
pub fn remember_data_dir() {
    let Some(marker) = handover_marker() else { return };
    let current = portable_dir();
    if fs::read_to_string(&marker)
        .map(|saved| saved.trim() == current.display().to_string())
        .unwrap_or(false)
    {
        return;
    }
    if let Some(parent) = marker.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&marker, current.display().to_string());
}

/// Takes over the data folder a previous location used, when this run started
/// from a fresh place and the old one still holds a config.
///
/// This is what makes "update in place" work for a portable copy: after the
/// updater moves the binary, the settings and the token follow it instead of the
/// user being sent back to the sign-in screen.
pub fn adopt_previous_data_dir() {
    let Some(marker) = handover_marker() else { return };
    let Ok(previous) = fs::read_to_string(&marker) else { return };
    let previous = PathBuf::from(previous.trim());
    let current = portable_dir();
    if previous == current {
        return;
    }
    let source = previous.join("config.json");
    let target = current.join("config.json");
    if !source.is_file() || target.is_file() {
        return;
    }
    // Copy rather than move: the old folder may be read-only, and leaving it in
    // place costs nothing while a failed move would lose the settings.
    if fs::copy(&source, &target).is_err() {
        return;
    }
    // The token is not copied: it lives in the system credential store, which does
    // not move with the folder. Copying the old plaintext file would put the secret
    // back on disk for no reason.
    crate::startup_log::write(&format!(
        "данные перенесены из {} в {}",
        previous.display(),
        current.display()
    ));
    remember_data_dir();
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
    adopt_legacy_download_dir(&mut settings);
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

/// Service and account names under which the token lives in the system
/// credential store.
const KEYRING_SERVICE: &str = "ym-dl";
const KEYRING_ACCOUNT: &str = "yandex-music";

/// The system credential store: Windows Credential Manager on Windows, Secret
/// Service on Linux.
///
/// A token is a password to the user's music account, so it belongs where the
/// system keeps passwords rather than in a plain file next to the program. This
/// returns `None` when no store is available — a Linux session without a running
/// Secret Service, or a locked-down container — and the caller then falls back to
/// the file so the app still works.
fn keyring_entry() -> Option<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).ok()
}

fn keyring_read() -> Option<String> {
    let token = keyring_entry()?.get_password().ok()?;
    let trimmed = token.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn keyring_write(token: &str) -> Result<()> {
    keyring_entry()
        .context("системное хранилище паролей недоступно")?
        .set_password(token)
        .context("не удалось сохранить токен в системное хранилище")
}

fn keyring_clear() {
    if let Some(entry) = keyring_entry() {
        let _ = entry.delete_credential();
    }
}

/// Where the token actually ended up.
///
/// The system store is the intended home, but it is not always there: a Linux
/// session without Secret Service, a locked keyring, a stripped container. Writing
/// the token to a file in that case is what keeps the app usable, and the interface
/// says so instead of letting the user believe it is in a password store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenHome {
    /// In the system credential store.
    Store,
    /// In `token_path()` because no store answered.
    File,
    /// No token at all.
    None,
}

pub fn token_home() -> TokenHome {
    if keyring_read().is_some() {
        return TokenHome::Store;
    }
    if token_path().is_file() || legacy_token_path().is_file() {
        return TokenHome::File;
    }
    TokenHome::None
}

pub fn read_token() -> String {
    // The credential store wins: it is where a token is written now.
    if let Some(token) = keyring_read() {
        // A plaintext copy may survive from an older build or a fallback write;
        // it is no longer read, so leaving it behind only leaks the secret.
        let _ = fs::remove_file(token_path());
        let _ = fs::remove_file(legacy_token_path());
        return token;
    }
    if let Ok(value) = fs::read_to_string(token_path()) {
        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() {
            // A file left by an older build: move it into the store so the secret
            // stops sitting on disk in the open.
            if keyring_write(&trimmed).is_ok() {
                let _ = fs::remove_file(token_path());
            }
            return trimmed;
        }
    }
    // One-time migration from the pre-portable location.
    let legacy = legacy_token_path();
    if let Ok(value) = fs::read_to_string(&legacy) {
        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() {
            let _ = write_token(&trimmed);
            let _ = fs::remove_file(&legacy);
            return trimmed;
        }
    }
    std::env::var("YM_TOKEN").unwrap_or_default()
}

pub fn write_token(token: &str) -> Result<()> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return clear_token();
    }
    if let Err(error) = keyring_write(trimmed) {
        // Keep the app usable where no store exists, but say so instead of
        // silently writing the secret to disk.
        crate::startup_log::write(&format!(
            "токен хранится в файле: {error:#} — система без хранилища паролей"
        ));
        return write_atomic(&token_path(), trimmed.as_bytes());
    }
    // Do not leave an older plaintext copy behind.
    let _ = fs::remove_file(token_path());
    Ok(())
}

pub fn clear_token() -> Result<()> {
    keyring_clear();
    // Both file locations too: the portable one and the file an older build used,
    // or a "reset" would silently keep the old login alive.
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
    fn lyrics_are_off_until_asked_for() {
        // Lyrics are an opt-in extra: the default download must not touch them.
        assert_eq!(Settings::default().lyrics_format, "none");
        assert!(!Settings::default().lyrics_fallback);
    }

    #[test]
    fn an_unknown_language_falls_back_to_auto() {
        let mut settings = Settings {
            language: "klingon".into(),
            ..Settings::default()
        };
        settings.sanitize();
        assert_eq!(settings.language, "auto");
    }

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
    fn default_target_dir_is_writable_and_the_music_folder() {
        let mut settings = Settings::default();
        settings.download_dir.clear();
        let dir = settings.target_dir();
        assert!(dir_writable(&dir), "default download dir must accept writes: {}", dir.display());

        // The default must be the music folder itself, never a subfolder of it:
        // an earlier build wrote into `~/Music/ym-dl` and then `~/Music/lmao`, and
        // the difference matters because the FLAC sweep walks the whole folder.
        let name = dir.file_name().map(|value| value.to_string_lossy().to_string());
        assert_eq!(name.as_deref(), Some("Music"), "unexpected default: {}", dir.display());

        let music = directories::UserDirs::new()
            .and_then(|dirs| dirs.audio_dir().map(Path::to_path_buf))
            .unwrap_or_else(|| home_dir().join("Music"));
        if dir_writable(&music) {
            assert_eq!(dir, music, "по умолчанию должна быть сама папка музыки");
        }
        // A locked-down profile falls back to the folder next to the executable;
        // the writability assertion above is what covers that case.
    }

    #[test]
    fn an_explicit_folder_is_never_replaced() {
        // The migration of old defaults must not move a library the user chose.
        let mut settings = Settings {
            download_dir: "D:/Music/mine".into(),
            ..Settings::default()
        };
        adopt_legacy_download_dir(&mut settings);
        assert_eq!(settings.download_dir, "D:/Music/mine");
    }

    #[test]
    fn portable_dir_is_writable() {
        let dir = portable_dir();
        assert!(dir_writable(&dir), "portable data dir must be writable: {}", dir.display());
    }

    /// The file fallback is what keeps the app usable where no password store
    /// answers, so it has to work on its own and must not leave the secret readable
    /// by other accounts.
    #[test]
    fn the_token_file_fallback_is_private() {
        // Under `target/`: the test binary is known to be able to write there,
        // while a CI sandbox may deny creating folders in the system temp dir.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("token-test");
        let path = dir.join("token");
        write_atomic(&path, b"secret-token").expect("the fallback must be able to write");

        assert_eq!(fs::read_to_string(&path).unwrap().trim(), "secret-token");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "token file must not be group/world readable");
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
