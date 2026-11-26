//! Turning image bytes into something the webview can display.
//!
//! Covers arrive as raw bytes from the service and are shown in `<img>` tags. A
//! data URL keeps them out of the asset protocol's scope rules and out of the
//! CSP: the image is already in memory, so nothing needs to be fetched again.

use base64::Engine;

/// Wraps image bytes as a data URL, picking the MIME type from the magic bytes.
pub fn data_url(bytes: &[u8]) -> String {
    let mime = match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', ..] => "image/gif",
        // RIFF also covers WAV and AVI, so the WebP tag has to be checked too.
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        _ => "image/jpeg",
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mime_type_follows_the_magic_bytes() {
        assert!(data_url(&[0x89, b'P', b'N', b'G', 0]).starts_with("data:image/png"));
        assert!(data_url(&[0xFF, 0xD8, 0xFF]).starts_with("data:image/jpeg"));
        assert!(data_url(b"RIFF\0\0\0\0WEBPVP8 ").starts_with("data:image/webp"));
        // A WAV file is RIFF too, and must not be announced as a picture.
        assert!(!data_url(b"RIFF\0\0\0\0WAVEfmt ").starts_with("data:image/webp"));
    }
}
