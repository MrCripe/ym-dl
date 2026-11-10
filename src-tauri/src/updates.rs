//! What version is installed and what version is published.
//!
//! The check reads the project's public releases feed. It used to call
//! `api.github.com`, which allows only 60 anonymous requests per hour per IP:
//! after a handful of launches every check failed with "GitHub did not answer"
//! while the UI still showed "up to date". The Atom feed is served by
//! github.com itself, carries the same tags and has no such limit.

use crate::ffmpeg;
use crate::model::UpdateEntry;
use anyhow::Result;
use std::process::Command;
use std::time::Duration;

/// Where this app publishes its releases.
const APP_REPO: &str = "MrCripe/ym-dl";
const RELEASES_FEED: &str = "https://github.com/MrCripe/ym-dl/releases.atom";

fn version_tuple(text: &str) -> Vec<u64> {
    text.split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .take(4)
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

fn newer(latest: &str, installed: &str) -> bool {
    version_tuple(latest) > version_tuple(installed)
}

fn ffmpeg_version() -> String {
    let Some(path) = ffmpeg::ffmpeg_path() else {
        return String::new();
    };
    let mut command = Command::new(path);
    command.arg("-version");
    // Without this a console window flashes on every update check, and the check
    // runs at startup.
    match ffmpeg::hidden(&mut command).output() {
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

/// The newest tag in the releases feed.
///
/// Every tag in the document is collected and the highest version wins, rather
/// than trusting the first one: the feed is ordered by publication date, so a
/// late fix released for an older branch would otherwise hide a newer version.
fn newest_tag(feed: &str) -> Option<String> {
    let marker = "/releases/tag/";
    let mut tags: Vec<String> = Vec::new();
    let mut rest = feed;
    while let Some(start) = rest.find(marker) {
        rest = &rest[start + marker.len()..];
        let end = rest
            .find(|ch: char| ch == '<' || ch == '"' || ch == '\'' || ch.is_whitespace())
            .unwrap_or(rest.len());
        let tag = rest[..end].trim();
        // A tag is a version; anything else is a link to the releases page.
        if !tag.is_empty() && tag.chars().next().map(|ch| ch.is_ascii_digit() || ch == 'v').unwrap_or(false) {
            tags.push(tag.to_string());
        }
    }
    tags.into_iter()
        .max_by(|a, b| version_tuple(a).cmp(&version_tuple(b)))
}

async fn latest_release_tag() -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(concat!("ym-dl/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client.get(RELEASES_FEED).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("HTTP {}", response.status().as_u16());
    }
    let body = response.text().await?;
    newest_tag(&body).ok_or_else(|| anyhow::anyhow!("в ленте релизов нет тегов"))
}

pub async fn check_updates(app_version: &str) -> Vec<UpdateEntry> {
    let mut entries = Vec::new();

    let entry = match latest_release_tag().await {
        Ok(tag) => UpdateEntry {
            name: "Приложение".into(),
            installed: app_version.to_string(),
            newer: newer(&tag, app_version),
            latest: tag,
            checked: true,
            detail: format!("github.com/{APP_REPO}/releases"),
        },
        Err(error) => UpdateEntry {
            name: "Приложение".into(),
            installed: app_version.to_string(),
            newer: false,
            latest: "—".into(),
            checked: false,
            detail: format!("Не удалось узнать новую версию: {error:#}"),
        },
    };
    entries.push(entry);

    // ffmpeg is installed by the user and its builds are date-stamped, so there is
    // nothing to compare against; showing the version is the useful part.
    let installed = ffmpeg_version();
    entries.push(if installed.is_empty() {
        UpdateEntry {
            name: "ffmpeg".into(),
            installed: "не найден".into(),
            latest: "—".into(),
            newer: false,
            checked: true,
            detail: "Без ffmpeg нельзя извлечь настоящий FLAC из .m4a.".into(),
        }
    } else {
        UpdateEntry {
            name: "ffmpeg".into(),
            installed,
            latest: "—".into(),
            newer: false,
            checked: true,
            detail: "Обновляется отдельно от приложения.".into(),
        }
    });

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

    /// The app tags its releases as `v1.0.1` while the running build reports
    /// `1.0.1`, so the same version must not look like an update.
    #[test]
    fn a_tagged_release_of_the_same_version_is_not_an_update() {
        assert!(!newer("v1.0.1", "1.0.1"));
        assert!(newer("v1.0.2", "1.0.1"));
    }

    #[test]
    fn reads_the_first_tag_from_a_release_feed() {
        let feed = r#"<feed>
            <link rel="alternate" href="https://github.com/MrCripe/ym-dl/releases"/>
            <entry><id>tag:github.com,2008:Repository/1/v1.2.0</id>
              <link rel="alternate" href="https://github.com/MrCripe/ym-dl/releases/tag/v1.2.0"/>
            </entry>
            <entry><link href="https://github.com/MrCripe/ym-dl/releases/tag/v1.1.0"/></entry>
        </feed>"#;
        assert_eq!(newest_tag(feed).as_deref(), Some("v1.2.0"));
    }

    /// The feed is ordered by publication date. A fix released for an older branch
    /// after a newer version must not hide that newer version.
    #[test]
    fn the_highest_version_wins_over_the_newest_publication() {
        let feed = r#"<feed>
            <entry><link href="https://github.com/MrCripe/ym-dl/releases/tag/v1.0.5"/></entry>
            <entry><link href="https://github.com/MrCripe/ym-dl/releases/tag/v1.1.0"/></entry>
            <entry><link href="https://github.com/MrCripe/ym-dl/releases/tag/v1.0.9"/></entry>
        </feed>"#;
        assert_eq!(newest_tag(feed).as_deref(), Some("v1.1.0"));
    }

    #[test]
    fn a_feed_without_tags_yields_nothing() {
        assert!(newest_tag("<feed><title>none</title></feed>").is_none());
        // The releases page itself is not a tag.
        assert!(newest_tag(r#"<link href="https://github.com/MrCripe/ym-dl/releases"/>"#).is_none());
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// The check that used to answer "GitHub did not answer" once the 60/hour
    /// anonymous API limit was spent. Run explicitly:
    /// `cargo test --release live_updates -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network"]
    async fn reads_the_real_release_feed() {
        let entries = check_updates("1.1.0").await;
        for entry in &entries {
            println!("{}: у вас {}, новая {}, checked={} ({})",
                entry.name, entry.installed, entry.latest, entry.checked, entry.detail);
        }
        let app = entries.iter().find(|entry| entry.name == "Приложение").expect("запись о приложении");
        assert!(app.checked, "проверка не удалась: {}", app.detail);
        assert!(app.latest.starts_with('v'), "не похоже на тег: {}", app.latest);
    }
}
