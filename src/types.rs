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
    RecentlyPlayed,
    Favorites,
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
    GetStreamUrl(Track),
    LoadImage(String),
}

// ── App results (background → UI) ────────────────────────────────────────────

pub enum AppResult {
    SearchResults(Vec<Track>),
    StreamUrlReady(String, Track),
    ThumbnailLoaded(String, Vec<u8>),
    PlayerState {
        position: f64,
        duration: f64,
        paused: bool,
    },
    Error(String),
}
