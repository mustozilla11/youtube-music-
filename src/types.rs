use serde::{Deserialize, Serialize};

// ── Track ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: Option<u32>,
    pub thumbnail_url: Option<String>,
}

// ── Tabs ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Tab {
    Search,
    Queue,
    RecentlyPlayed,
    Favorites,
}

// ── Repeat Mode ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off, // Sıradan devam et (Default)
    All, // Bütün listeyi tekrarla
    One, // Bu şarkıyı tekrarla
}

// ── Config ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Raw browser cookie string for authenticated YouTube Music requests.
    pub cookie: Option<String>,
}

// ── Worker messages (UI → background) ────────────────────────────────────────

pub enum WorkerMsg {
    Search(String),
    PlayTrack(Track),
    NextTrack,
    PrevTrack,
    LoadImage(String),
    FetchRadio(String),
    FetchYtHistory,
}

// ── App results (background → UI) ────────────────────────────────────────────

pub enum AppResult {
    SearchResults(Vec<Track>),
    TrackStarted(Track),
    ThumbnailLoaded(String, Vec<u8>),
    RadioQueue(Vec<Track>),
    YtHistory(Vec<Track>),
    Error(String),
}
