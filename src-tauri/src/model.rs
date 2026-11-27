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
    /// Only the search result carries this; track and album payloads do not.
    #[serde(default)]
    pub cover_uri: Option<String>,
    /// The search result sends the picture as an object, not as `coverUri`,
    /// which is why artist cards used to show an empty circle.
    #[serde(default)]
    pub cover: Option<SearchCover>,
    /// Plain image URL the service also provides; a fallback when there is no
    /// cover object.
    #[serde(default)]
    pub op_image: Option<String>,
}

/// A record label, sent either as an object or as a bare string.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LabelRef {
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
    /// Record label, written as the `label` tag when the service sends one.
    #[serde(default)]
    pub labels: Vec<LabelRef>,
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
    /// The service flags a track with an explicit content warning.
    #[serde(default)]
    pub explicit: bool,
    /// Whether the whole album is flagged, which applies when the track is not.
    #[serde(default)]
    pub content_warning: Option<String>,
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
    /// Present on the rare track the service ships with one. Yandex does not
    /// expose ISRC for most releases, so an empty value is the normal case.
    #[serde(default)]
    pub isrc: Option<String>,
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
    pub uid: serde_json::Value,
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

/// A page of an artist's own tracks (`/artists/{id}/tracks`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArtistTracks {
    #[serde(default)]
    pub tracks: Vec<Track>,
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
    pub values: Vec<String>,
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

/// `get-file-info` wraps the payload one level deep: `{"downloadInfo": {...}}`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    #[serde(default)]
    pub download_info: DownloadInfo,
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
    pub expires: String,
    pub lossless_ok: Option<bool>,
    pub probe_codec: String,
    pub probe_bitrate: u32,
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

/// One track in the search results, with everything the row shows.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    /// Cover as a data URL, already fetched: the list shows covers immediately
    /// instead of popping them in one by one.
    pub cover: Option<String>,
    /// A normal page address, so picking the row feeds the resolve path.
    pub link: String,
}

/// A card in one of the horizontal shelves: artist, album or playlist.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCard {
    pub title: String,
    pub subtitle: String,
    pub cover: Option<String>,
    pub link: String,
    /// Artists are shown as circles, albums and playlists as squares.
    pub round: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub tracks: Vec<SearchTrack>,
    pub artists: Vec<SearchCard>,
    pub albums: Vec<SearchCard>,
    pub playlists: Vec<SearchCard>,
}

/// `search` wraps every section the same way: `{results: [...]}`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSection<T> {
    #[serde(default)]
    pub results: Vec<T>,
}

/// The search payload has no `result` wrapper: the sections sit at the top level.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPayload {
    #[serde(default)]
    pub tracks: SearchSection<Track>,
    #[serde(default)]
    pub albums: SearchSection<AlbumRef>,
    #[serde(default)]
    pub artists: SearchSection<Artist>,
    #[serde(default)]
    pub playlists: SearchSection<SearchPlaylist>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPlaylist {
    #[serde(default)]
    pub kind: serde_json::Value,
    #[serde(default)]
    pub uid: serde_json::Value,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub owner: Option<Owner>,
    #[serde(default)]
    pub track_count: Option<u32>,
    #[serde(default)]
    pub cover: Option<SearchCover>,
}

/// A playlist cover is either one URI or a collage of up to four.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCover {
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub items_uri: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    /// Preview of what was resolved, shown before anything is downloaded: a
    /// cover as a data URL, the year and what quality the service offers for it.
    pub cover: Option<String>,
    pub year: Option<i32>,
    /// `flac` or `aac`, as the service really answers for this account.
    pub quality: String,
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub tracks: Vec<TrackItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackState {
    pub index: usize,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub status: String,
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
    /// The job itself blew up (as opposed to individual tracks failing), so the UI
    /// must not report a clean finish. Set only when the whole job fails to start.
    pub error: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveProgress {
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEntry {
    /// Stable identifier the UI translates: `app`. A display name sent
    /// from here would stay Russian in an English interface.
    pub kind: String,
    pub name: String,
    pub installed: String,
    pub latest: String,
    pub newer: bool,
    /// False when the newest version could not be determined, so the UI does not
    /// claim "up to date" after a failed check.
    pub checked: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentInfo {
    pub app_version: String,
    pub config_path: String,
    /// True when the app runs from an unpacked portable folder, where the built-in
    /// updater cannot replace the binary in place.
    pub portable: bool,
    /// True when the token is currently sitting in a file, not in the store.
    pub token_file: bool,
    pub os: String,
}
