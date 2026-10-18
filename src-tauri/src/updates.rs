//! Update checks: the app itself (when a repository is configured) and ffmpeg.

use crate::ffmpeg;
use crate::model::UpdateEntry;
use anyhow::Result;
use std::process::Command;
use std::time::Duration;

const FFMPEG_RELEASES: &str = "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/latest";

fn version_tuple(text: &str) -> Vec<u64> {
    text.split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .take(4)
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

fn newer(latest: &str, installed: &str) -> bool {
    // BtbN publishes ffmpeg as date-stamped `autobuild-YYYY-MM-DD-…` tags. Those
    // digits are a date, not a version: comparing them with an installed `9.0.2`
    // reports an update on every single launch.
    if latest.trim().to_ascii_lowercase().starts_with("autobuild") {
        return false;
    }
    version_tuple(latest) > version_tuple(installed)
}

fn ffmpeg_version() -> String {
    let Some(path) = ffmpeg::ffmpeg_path() else {
        return String::new();
    };
    let mut command = Command::new(path);
    command.arg("-version");
    match command.output() {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .nth(2)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

async fn latest_ffmpeg_tag() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("ym-dl")
        .build()
        .ok()?;
    let response = client.get(FFMPEG_RELEASES).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: serde_json::Value = response.json().await.ok()?;
    value
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .map(|tag| tag.to_string())
}

async fn latest_release_tag(repo: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("ym-dl")
        .build()?;
    let response = client
        .get(format!("https://api.github.com/repos/{repo}/releases/latest"))
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("HTTP {}", response.status().as_u16());
    }
    let value: serde_json::Value = response.json().await?;
    Ok(value
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .unwrap_or_default()
        .to_string())
}

pub async fn check_updates(app_version: &str, repo: &str) -> Vec<UpdateEntry> {
    let mut entries = Vec::new();

    if repo.trim().is_empty() {
        entries.push(UpdateEntry {
            name: "Приложение".into(),
            installed: app_version.to_string(),
            latest: "—".into(),
            newer: false,
            detail: "Репозиторий для проверки обновлений не задан в настройках.".into(),
        });
    } else {
        match latest_release_tag(repo.trim()).await {
            Ok(tag) if !tag.is_empty() => entries.push(UpdateEntry {
                name: "Приложение".into(),
                installed: app_version.to_string(),
                latest: tag.clone(),
                newer: newer(&tag, app_version),
                detail: format!("github.com/{repo}"),
            }),
            Ok(_) => entries.push(UpdateEntry {
                name: "Приложение".into(),
                installed: app_version.to_string(),
                latest: "нет релизов".into(),
                newer: false,
                detail: format!("github.com/{repo}"),
            }),
            Err(error) => entries.push(UpdateEntry {
                name: "Приложение".into(),
                installed: app_version.to_string(),
                latest: "недоступно".into(),
                newer: false,
                detail: format!("GitHub не ответил: {error:#}"),
            }),
        }
    }

    let installed = ffmpeg_version();
    if installed.is_empty() {
        entries.push(UpdateEntry {
            name: "ffmpeg".into(),
            installed: "не найден".into(),
            latest: "—".into(),
            newer: false,
            detail: "Без ffmpeg нельзя извлечь настоящий FLAC из .m4a.".into(),
        });
    } else {
        match latest_ffmpeg_tag().await {
            Some(tag) => {
                // The BtbN tags are dates, so "newer" is unknowable here; say so
                // instead of guessing and nagging.
                let dated = tag.to_ascii_lowercase().starts_with("autobuild");
                entries.push(UpdateEntry {
                    name: "ffmpeg".into(),
                    installed: installed.clone(),
                    latest: tag.clone(),
                    newer: newer(&tag, &installed),
                    detail: if dated {
                        "github.com/BtbN/FFmpeg-Builds · сборки датируются, автосравнение недоступно".into()
                    } else {
                        "github.com/BtbN/FFmpeg-Builds".into()
                    },
                })
            }
            None => entries.push(UpdateEntry {
                name: "ffmpeg".into(),
                installed,
                latest: "недоступно".into(),
                newer: false,
                detail: "GitHub не ответил.".into(),
            }),
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(newer("6.1.2", "6.1.1"));
        assert!(newer("7.0", "6.9.9"));
        assert!(!newer("6.1.1", "6.1.1"));
        assert!(!newer("6.1.0", "6.1.1"));
    }

    #[test]
    fn version_tuple_ignores_prefixes() {
        assert_eq!(version_tuple("autobuild-2026-01-01-12-00"), vec![2026, 1, 1, 12]);
        assert_eq!(version_tuple("n7.1"), vec![7, 1]);
    }

    /// The ffmpeg check used to claim an update on every launch, because the
    /// BtbN tag's year beats any real version number.
    #[test]
    fn autobuild_tag_is_not_newer_than_a_real_version() {
        assert!(!newer("autobuild-2026-10-01-12-00", "9.0.2"));
        assert!(!newer("AUTOBUILD-2026-10-01-12-00", "7.1"));
        assert!(newer("9.0.3", "9.0.2"));
    }
}
