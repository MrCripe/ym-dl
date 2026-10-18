//! Startup log.
//!
//! The app can fail before any window exists (WebView2 needs a writable profile
//! folder), and a message box alone does not say *which* folder was refused. The
//! log is written next to the executable, the one place a portable build can
//! always write, so a failed start still leaves evidence behind.

use std::io::Write;
use std::path::PathBuf;

pub fn path() -> Option<PathBuf> {
    crate::config::exe_dir().map(|dir| dir.join("ym-dl.log"))
}

/// Appends one line. Never fails loudly: logging must not be able to break the
/// app it is supposed to explain.
pub fn write(message: &str) {
    let Some(path) = path() else { return };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "[{stamp}] {message}");
    }
}

/// Records whether a directory can really be written to, and why not.
pub fn report_dir(label: &str, dir: &std::path::Path) {
    let created = std::fs::create_dir_all(dir);
    let probe = dir.join(".ym-dl-write-test");
    let written = std::fs::write(&probe, b"1");
    if written.is_ok() {
        let _ = std::fs::remove_file(&probe);
    }
    write(&format!(
        "{label}: {} create={} write={}",
        dir.display(),
        match created {
            Ok(()) => "ok".to_string(),
            Err(error) => format!("{error}"),
        },
        match written {
            Ok(()) => "ok".to_string(),
            Err(error) => format!("{error}"),
        }
    ));
}
