use anyhow::{bail, Result};
use reqwest::{header, Client};
use serde_json::{json, Value};

use crate::types::Track;

const API_KEY: &str = "AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";

pub struct YtMusicClient {
    client: Client,
    cookie: Option<String>,
}

impl YtMusicClient {
    pub fn new(cookie: Option<String>) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::REFERER, "https://music.youtube.com/".parse()?);
        headers.insert(header::ORIGIN, "https://music.youtube.com".parse()?);

        let client = Client::builder()
            .user_agent(
                "Mozilla/5.0 (X11; Linux x86_64; rv:125.0) Gecko/20100101 Firefox/125.0",
            )
            .default_headers(headers)
            .build()?;

        Ok(Self { client, cookie })
    }

    fn context(&self) -> Value {
        json!({
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": "1.20240101.01.00",
                "hl": "tr",
                "gl": "TR"
            }
        })
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let url = format!(
            "https://music.youtube.com/youtubei/v1/{path}?key={API_KEY}"
        );
        let mut req = self.client.post(&url).json(&body);
        if let Some(c) = &self.cookie {
            req = req.header("Cookie", c);
        }
        let resp = req.send().await?.json::<Value>().await?;
        Ok(resp)
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Search YouTube Music for tracks matching `query`.
    pub async fn search(&self, query: &str) -> Result<Vec<Track>> {
        let body = json!({
            "context": self.context(),
            "query": query,
            "params": "EgWKAQIIAWoKEAkQBRAKEAMQBA=="
        });
        let resp = self.post("search", body).await?;
        Ok(parse_search_results(&resp))
    }

    /// Get automix/radio queue tracks starting from `video_id`.
    /// Fetches up to ~100 tracks using continuation token.
    pub async fn get_radio_queue(&self, video_id: &str) -> Result<Vec<Track>> {
        let body = json!({
            "context": self.context(),
            "videoId": video_id,
            "playlistId": format!("RDAMVM{video_id}"),
            "params": "wAEB",
            "isAudioOnly": true
        });
        let resp = self.post("next", body).await?;
        let mut tracks = parse_radio_queue(&resp);

        // Fetch continuation batch (second ~50 tracks)
        let token = resp
            .pointer(
                "/contents/singleColumnMusicWatchNextResultsRenderer\
                 /tabbedRenderer/watchNextTabbedResultsRenderer\
                 /tabs/0/tabRenderer/content\
                 /musicQueueRenderer/content\
                 /playlistPanelRenderer/continuations/0\
                 /nextRadioContinuationData/continuation",
            )
            .and_then(|v| v.as_str());

        if let Some(tok) = token {
            let cont_body = json!({
                "context": self.context(),
                "continuation": tok
            });
            if let Ok(cont_resp) = self.post("next", cont_body).await {
                if let Some(items) = cont_resp
                    .pointer("/continuationContents/playlistPanelContinuation/contents")
                    .and_then(Value::as_array)
                {
                    for item in items {
                        if let Some(r) = item.get("playlistPanelVideoRenderer") {
                            if let Some(t) = parse_panel_video(r) {
                                if !tracks.iter().any(|x| x.id == t.id) {
                                    tracks.push(t);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(tracks)
    }

    /// Fetch the user's YouTube Music listen history (requires cookie auth).
    pub async fn get_yt_history(&self) -> Result<Vec<Track>> {
        if self.cookie.is_none() {
            return Ok(vec![]);
        }
        let body = json!({
            "context": self.context(),
            "browseId": "FEmusic_history"
        });
        let resp = self.post("browse", body).await?;
        Ok(parse_browse_tracks(&resp))
    }

    /// Obtain a direct audio stream URL via yt-dlp.
    /// Prefers WebM/Opus (up to 251 kbps) over AAC for better quality.
    pub async fn get_stream_url(id: &str) -> Result<String> {
        let chk = tokio::process::Command::new("which")
            .arg("yt-dlp")
            .output()
            .await?;
        if chk.stdout.is_empty() {
            bail!("yt-dlp bulunamadı. Lütfen kurun: sudo pacman -S yt-dlp");
        }

        let out = tokio::process::Command::new("yt-dlp")
            .args([
                "-g",
                // Prefer Opus/WebM (251 kbps) > AAC/M4A > anything else
                "-f",
                "bestaudio[ext=webm]/bestaudio[ext=m4a]/bestaudio",
                "--no-playlist",
                "--quiet",
                &format!("https://music.youtube.com/watch?v={id}"),
            ])
            .output()
            .await?;

        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if url.is_empty() {
            bail!("Ses akışı alınamadı (yt-dlp boş döndü)");
        }
        Ok(url)
    }
}

// ── JSON parsers ──────────────────────────────────────────────────────────────

/// Parse results from `/youtubei/v1/search`
fn parse_search_results(json: &Value) -> Vec<Track> {
    let mut tracks = Vec::new();

    let sections = json
        .pointer(
            "/contents/tabbedSearchResultsRenderer\
             /tabs/0/tabRenderer/content\
             /sectionListRenderer/contents",
        )
        .and_then(Value::as_array);

    if let Some(secs) = sections {
        for sec in secs {
            if let Some(items) = sec
                .pointer("/musicShelfRenderer/contents")
                .and_then(Value::as_array)
            {
                for item in items {
                    if let Some(t) = parse_responsive_item(item) {
                        tracks.push(t);
                    }
                }
            }
        }
    }

    tracks
}

/// Parse radio/automix queue from `/youtubei/v1/next`
fn parse_radio_queue(json: &Value) -> Vec<Track> {
    let mut tracks = Vec::new();

    let items = json
        .pointer(
            "/contents/singleColumnMusicWatchNextResultsRenderer\
             /tabbedRenderer/watchNextTabbedResultsRenderer\
             /tabs/0/tabRenderer/content\
             /musicQueueRenderer/content\
             /playlistPanelRenderer/contents",
        )
        .or_else(|| {
            json.pointer(
                "/contents/singleColumnMusicWatchNextResultsRenderer\
                 /playlist/playlistPanelRenderer/contents",
            )
        })
        .and_then(Value::as_array);

    if let Some(items) = items {
        for item in items {
            if let Some(r) = item.get("playlistPanelVideoRenderer") {
                if let Some(t) = parse_panel_video(r) {
                    tracks.push(t);
                }
            }
        }
    }

    tracks
}

/// Parse browse results (history, liked songs, etc.) from `/youtubei/v1/browse`
fn parse_browse_tracks(json: &Value) -> Vec<Track> {
    let mut tracks = Vec::new();

    let sections = json
        .pointer(
            "/contents/singleColumnBrowseResultsRenderer\
             /tabs/0/tabRenderer/content\
             /sectionListRenderer/contents",
        )
        .and_then(Value::as_array);

    if let Some(secs) = sections {
        for sec in secs {
            if let Some(items) = sec
                .pointer("/musicShelfRenderer/contents")
                .and_then(Value::as_array)
            {
                for item in items {
                    if let Some(t) = parse_responsive_item(item) {
                        tracks.push(t);
                    }
                }
            }
        }
    }

    tracks
}

// ── Item-level parsers ────────────────────────────────────────────────────────

/// Parse a `musicResponsiveListItemRenderer` (search / history rows)
fn parse_responsive_item(item: &Value) -> Option<Track> {
    let r = item.get("musicResponsiveListItemRenderer")?;

    let id = r
        .pointer(
            "/overlay/musicItemThumbnailOverlayRenderer\
             /content/musicPlayButtonRenderer\
             /playNavigationEndpoint/watchEndpoint/videoId",
        )
        .or_else(|| {
            r.pointer(
                "/flexColumns/0\
                 /musicResponsiveListItemFlexColumnRenderer\
                 /text/runs/0/navigationEndpoint/watchEndpoint/videoId",
            )
        })
        .and_then(|v| v.as_str())
        .map(String::from)?;

    let title = r
        .pointer(
            "/flexColumns/0\
             /musicResponsiveListItemFlexColumnRenderer/text",
        )
        .and_then(first_run_text)?;

    let runs = r
        .pointer(
            "/flexColumns/1\
             /musicResponsiveListItemFlexColumnRenderer/text/runs",
        )
        .and_then(Value::as_array);

    let artist = runs
        .and_then(|r| r.first())
        .and_then(|r| r.get("text"))
        .and_then(|t| t.as_str())
        .map(String::from)
        .unwrap_or_else(|| "Bilinmiyor".into());

    let album = runs
        .and_then(|r| r.get(4))
        .and_then(|r| r.get("text"))
        .and_then(|t| t.as_str())
        .map(String::from);

    let duration_secs = r
        .pointer(
            "/fixedColumns/0\
             /musicResponsiveListItemFixedColumnRenderer\
             /text/runs/0/text",
        )
        .and_then(|v| v.as_str())
        .and_then(parse_duration);

    let thumbnail_url = r
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|a| a.last())
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(fix_url);

    Some(Track { id, title, artist, album, duration_secs, thumbnail_url })
}

/// Parse a `playlistPanelVideoRenderer` (radio/queue rows)
fn parse_panel_video(r: &Value) -> Option<Track> {
    let id = r.get("videoId")?.as_str().map(String::from)?;

    let title = r
        .pointer("/title/runs/0/text")
        .and_then(|v| v.as_str())
        .map(String::from)?;

    let runs = r
        .pointer("/longBylineText/runs")
        .and_then(Value::as_array);

    let artist = runs
        .and_then(|r| r.first())
        .and_then(|r| r.get("text"))
        .and_then(|t| t.as_str())
        .map(String::from)
        .unwrap_or_else(|| "Bilinmiyor".into());

    let album = runs
        .and_then(|r| r.get(2))
        .and_then(|r| r.get("text"))
        .and_then(|t| t.as_str())
        .map(String::from);

    let duration_secs = r
        .pointer("/lengthText/runs/0/text")
        .and_then(|v| v.as_str())
        .and_then(parse_duration);

    let thumbnail_url = r
        .pointer("/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|a| a.last())
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(fix_url);

    Some(Track { id, title, artist, album, duration_secs, thumbnail_url })
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn first_run_text(v: &Value) -> Option<String> {
    v.get("runs")?
        .as_array()?
        .first()?
        .get("text")?
        .as_str()
        .map(String::from)
}

fn fix_url(u: &str) -> String {
    if u.starts_with("//") {
        format!("https:{u}")
    } else {
        u.to_owned()
    }
}

fn parse_duration(s: &str) -> Option<u32> {
    let p: Vec<&str> = s.split(':').collect();
    match p.as_slice() {
        [m, s] => Some(m.parse::<u32>().ok()? * 60 + s.parse::<u32>().ok()?),
        [h, m, s] => Some(
            h.parse::<u32>().ok()? * 3600
                + m.parse::<u32>().ok()? * 60
                + s.parse::<u32>().ok()?,
        ),
        _ => None,
    }
}
