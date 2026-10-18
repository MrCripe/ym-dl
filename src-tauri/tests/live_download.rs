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
