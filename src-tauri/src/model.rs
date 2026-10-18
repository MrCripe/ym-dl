//! JSON payloads of the Yandex Music API.
//!
//! Not every field is read by the UI, but keeping them documents the wire
//! format and keeps deserialization tolerant of the parts we ignore.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    #[serde(default)]
    pub id: serde_json::Value,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackPosition {
    #[serde(default)]
    pub volume: Option<u32>,
    #[serde(default)]
    pub index: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRef {
    #[serde(default)]
    pub id: serde_json::Value,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub cover_uri: Option<String>,
    #[serde(default)]
    pub track_position: Option<TrackPosition>,
    #[serde(default)]
    pub track_count: Option<u32>,
    #[serde(default)]
    pub artists: Vec<Artist>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub meta_type: Option<String>,
    #[serde(default)]
    pub available: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LyricsInfo {
    #[serde(default)]
    pub has_available_text_lyrics: bool,
    #[serde(default)]
    pub has_available_sync_lyrics: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    #[serde(default)]
    pub id: serde_json::Value,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub artists: Vec<Artist>,
    #[serde(default)]
    pub albums: Vec<AlbumRef>,
    #[serde(default)]
    pub lyrics_info: Option<LyricsInfo>,
    #[serde(default)]
    pub cover_uri: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    #[serde(default)]
    pub id: serde_json::Value,
    #[serde(default)]
    pub track: Option<Track>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub track_count: Option<u32>,
    #[serde(default)]
    pub tracks: Vec<PlaylistEntry>,
    #[serde(default)]
    pub owner: Option<Owner>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub login: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    #[serde(default)]
    pub id: serde_json::Value,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub cover_uri: Option<String>,
    #[serde(default)]
    pub volumes: Vec<Vec<Track>>,
    #[serde(default)]
    pub artists: Vec<Artist>,
    #[serde(default)]
    pub track_count: Option<u32>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub meta_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArtistAlbums {
    #[serde(default)]
    pub albums: Vec<AlbumRef>,
    #[serde(default)]
    pub pager: Option<Pager>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Pager {
    #[serde(default)]
    pub page: u32,
    #[serde(default)]
    pub per_page: u32,
    #[serde(default)]
    pub total: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    #[serde(default)]
    pub uid: serde_json::Value,
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub full_name: Option<String>,
    #[serde(default)]
    pub non_owner_family_member: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Plus {
    #[serde(default)]
    pub has_plus: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    #[serde(default)]
    pub values: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionProduct {
    #[serde(default)]
    pub product_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoRenewable {
    #[serde(default)]
    pub product: Option<SubscriptionProduct>,
    #[serde(default)]
    pub expires: Option<String>,
    #[serde(default)]
    pub finished: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    #[serde(default)]
    pub auto_renewable: Vec<AutoRenewable>,
    #[serde(default)]
    pub can_start_trial: bool,
    #[serde(default)]
    pub had_any_subscription: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(default)]
    pub account: Option<Account>,
    #[serde(default)]
    pub plus: Option<Plus>,
    #[serde(default)]
    pub permissions: Option<Permissions>,
    #[serde(default)]
    pub subscription: Option<Subscription>,
    #[serde(default)]
    pub advertisement: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInfo {
    #[serde(default)]
    pub quality: String,
    #[serde(default)]
    pub codec: String,
    #[serde(default)]
    pub bitrate: u32,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub urls: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    #[serde(default)]
    pub download_url: Option<String>,
}

// --- payloads sent to the frontend ---------------------------------------

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub uid: String,
    pub login: String,
    pub name: String,
    pub has_plus: bool,
    pub family_member: bool,
    pub product_id: String,
    pub permissions: String,
    pub expires: String,
    pub lossless_ok: Option<bool>,
    pub probe_codec: String,
    pub probe_quality: String,
    pub probe_bitrate: u32,
    pub probe_track: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackItem {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: String,
    pub available: bool,
    pub year: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub tracks: Vec<TrackItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackState {
    pub index: usize,
    pub total: usize,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub status: String,
    pub codec: String,
    pub bitrate: u32,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub done: usize,
    pub total: usize,
    pub ok: usize,
    pub failed: usize,
    pub skipped: usize,
    pub running: bool,
    pub stopping: bool,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEntry {
    pub name: String,
    pub installed: String,
    pub latest: String,
    pub newer: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentInfo {
    pub ffmpeg: bool,
    pub ffprobe: bool,
    pub ffmpeg_path: String,
    pub app_version: String,
    pub config_path: String,
    pub token_path: String,
    pub os: String,
}
