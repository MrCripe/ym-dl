//! Request signing for the Yandex Music API.
//!
//! Two different keys are in play and they are not interchangeable:
//! * `DEFAULT_SIGN_KEY` signs `track_id + timestamp` and belongs to the mobile app.
//! * `DESKTOP_SIGN_KEY` signs the `get-file-info` parameter string and is only
//!   accepted together with the matching desktop client header. Lossless is
//!   served only to requests signed with the desktop key.

use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_SIGN_KEY: &str = "p93jhgh689SBReK6ghtw62";
pub const DESKTOP_SIGN_KEY: &str = "kzqU4XhfCaY6B6JTHODeq5";
pub const DESKTOP_CLIENT: &str = "YandexMusicDesktopAppWindows/5.95.0";
pub const MOBILE_CLIENT: &str = "YandexMusicAndroid/24023621";
pub const USER_AGENT: &str = "Yandex-Music-API";

pub const CODECS: [&str; 7] = [
    "flac", "flac-mp4", "mp3", "aac", "he-aac", "aac-mp4", "he-aac-mp4",
];

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hmac_b64(key: &str, message: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("hmac accepts any key size");
    mac.update(message.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Signature for `get-file-info`. The trailing `=` is stripped by the server's
/// own implementation, so it is stripped here too.
pub fn file_info_sign(track_id: &str, quality: &str, transport: &str, timestamp: u64) -> String {
    let codecs: String = CODECS.concat();
    let message = format!("{timestamp}{track_id}{quality}{codecs}{transport}");
    let encoded = hmac_b64(DESKTOP_SIGN_KEY, &message);
    encoded.trim_end_matches('=').to_string()
}

/// Signature for the lyrics endpoint (mobile key, full base64).
pub fn request_sign(track_id: &str, timestamp: u64) -> String {
    hmac_b64(DEFAULT_SIGN_KEY, &format!("{track_id}{timestamp}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_info_sign_is_deterministic_and_trimmed() {
        let a = file_info_sign("53404", "lossless", "encraw", 1_700_000_000);
        let b = file_info_sign("53404", "lossless", "encraw", 1_700_000_000);
        assert_eq!(a, b);
        assert!(!a.ends_with('='));
        assert_ne!(a, file_info_sign("53405", "lossless", "encraw", 1_700_000_000));
        assert_ne!(a, file_info_sign("53404", "nq", "encraw", 1_700_000_000));
    }

    #[test]
    fn request_sign_keeps_padding() {
        let value = request_sign("53404", 1_700_000_000);
        assert_eq!(value.len(), 44);
        assert!(value.ends_with('='));
    }
}
