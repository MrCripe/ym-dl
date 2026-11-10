//! Live end-to-end check against the real Yandex Music API.
//!
//! Run explicitly, it is not part of the normal suite because it needs a token
//! and network access:
//!
//! ```text
//! cargo test --release --test live_download -- --ignored --nocapture
//! ```
//!
//! It follows the exact path the "Начать" button takes — resolve the track,
//! fetch `get-file-info`, download the audio, decrypt it — and prints what each
//! step returned, so a failure names the step instead of surfacing as a generic
//! UI toast.

use ym_dl_lib::live::run_live_check;

#[tokio::test]
#[ignore = "needs a real token in ~/.ym-dl-token and network access"]
async fn downloads_a_real_track_end_to_end() {
    let report = run_live_check("53404", 2).await;
    for line in &report.lines {
        println!("{line}");
    }
    assert!(report.ok, "live download failed: {}", report.lines.join(" | "));
}

/// A track Yandex has synced lyrics for, which is what the LRC option is for.
#[tokio::test]
#[ignore = "needs a real token in ~/.ym-dl-token and network access"]
async fn a_track_with_lyrics_gets_its_timings() {
    let report = run_live_check("114784199", 2).await;
    for line in &report.lines {
        println!("{line}");
    }
    assert!(report.ok, "live download failed: {}", report.lines.join(" | "));
    let lyrics = report
        .lines
        .iter()
        .find(|line| line.starts_with("лирикс:"))
        .cloned()
        .unwrap_or_default();
    assert!(
        lyrics.contains("с таймингами"),
        "тайминги не попали в файл: {}",
        report.lines.join(" | ")
    );
}

/// Yandex has no text for this track, so the lrclib fallback is what supplies it.
#[tokio::test]
#[ignore = "needs a real token in ~/.ym-dl-token and network access"]
async fn the_lrclib_fallback_covers_a_track_yandex_lacks() {
    let report = run_live_check("96109", 2).await;
    for line in &report.lines {
        println!("{line}");
    }
    let lyrics = report
        .lines
        .iter()
        .find(|line| line.starts_with("лирикс:"))
        .cloned()
        .unwrap_or_else(|| "лирикса нет вовсе".into());
    assert!(
        lyrics.contains("lrclib"),
        "резервный источник не сработал: {lyrics} | {}",
        report.lines.join(" | ")
    );
}
