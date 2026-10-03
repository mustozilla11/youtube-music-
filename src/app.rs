use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Color32, FontId, Rounding, RichText, Stroke, TextStyle, Vec2};

use crate::api::YtMusicClient;
use crate::player::Player;
use crate::storage::Storage;
use crate::types::{AppResult, Config, Tab, Track, WorkerMsg};

// ── OLED Black Palette ───────────────────────────────────────────────────────
const BG: Color32      = Color32::BLACK;
const SURF: Color32    = Color32::from_rgb(10, 10, 10);
const SURF2: Color32   = Color32::from_rgb(22, 22, 22);
const BORDER: Color32  = Color32::from_rgb(32, 32, 32);
const ACCENT: Color32  = Color32::from_rgb(255, 0,  48);
const ACCENT2: Color32 = Color32::from_rgb(160, 0,  30);
const TEXT: Color32    = Color32::WHITE;
const TEXT_D: Color32  = Color32::from_rgb(155, 155, 155);
const TEXT_M: Color32  = Color32::from_rgb(80,  80,  80);

// ── Row action ───────────────────────────────────────────────────────────────
#[derive(Clone)]
enum RowAction {
    Play,
    ToggleFav,
}

// ── App state ────────────────────────────────────────────────────────────────
pub struct YtMusicApp {
    tab: Tab,

    // Search
    query: String,
    searching: bool,
    results: Vec<Track>,

    // Persistent data
    history: Vec<Track>,
    favorites: Vec<Track>,

    // Playback queue
    queue: Vec<Track>,
    queue_pos: usize,
    radio_fetched: bool,

    // Player state
    current: Option<Track>,
    playing: bool,
    position: f64,
    duration: f64,
    volume: f64,
    loading_stream: bool,
    prev_position: f64, // for end-of-track detection

    // Seek UX: only send seek command on drag release
    user_seeking: bool,
    seek_target: f64,

    // State polling
    polling: bool,
    last_poll: Instant,

    // Login
    login_open: bool,
    cookie_buf: String,
    config: Config,

    // Error toast
    error: Option<String>,
    error_end: Instant,

    // Channels
    cmd_tx: tokio::sync::mpsc::UnboundedSender<WorkerMsg>,
    res_rx: mpsc::Receiver<AppResult>,
    res_tx: mpsc::Sender<AppResult>,

    // Systems
    player: Arc<Mutex<Option<Player>>>,
    storage: Storage,
    rt: Arc<tokio::runtime::Runtime>,

    // Image cache
    textures: HashMap<String, egui::TextureHandle>,
    img_pending: HashSet<String>,
}

impl YtMusicApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, rt: Arc<tokio::runtime::Runtime>) -> Self {
        let storage = Storage::new().expect("storage init");
        let config = storage.load_config();
        let history = storage.load_history();
        let favorites = storage.load_favorites();

        let player = Arc::new(Mutex::new(match Player::new() {
            Ok(p) => Some(p),
            Err(e) => { eprintln!("[ytmusic] mpv: {e}"); None }
        }));

        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<WorkerMsg>();
        let (res_tx, res_rx)     = mpsc::channel::<AppResult>();

        // Fetch YouTube Music history on startup if logged in
        if config.cookie.is_some() {
            let _ = cmd_tx.send(WorkerMsg::FetchYtHistory);
        }

        // ── Background worker ────────────────────────────────────────────────
        {
            let tx     = res_tx.clone();
            let cookie = config.cookie.clone();
            rt.spawn(async move {
                let api = match YtMusicClient::new(cookie) {
                    Ok(a) => a,
                    Err(e) => {
                        let _ = tx.send(AppResult::Error(format!("API hatası: {e}")));
                        return;
                    }
                };

                while let Some(msg) = cmd_rx.recv().await {
                    let tx = tx.clone();
                    match msg {
                        WorkerMsg::Search(q) => {
                            let r = match api.search(&q).await {
                                Ok(t) => AppResult::SearchResults(t),
                                Err(e) => AppResult::Error(e.to_string()),
                            };
                            let _ = tx.send(r);
                        }
                        WorkerMsg::GetStreamUrl(track) => {
                            let r = match YtMusicClient::get_stream_url(&track.id).await {
                                Ok(u) => AppResult::StreamUrlReady(u, track),
                                Err(e) => AppResult::Error(e.to_string()),
                            };
                            let _ = tx.send(r);
                        }
                        WorkerMsg::LoadImage(url) => {
                            if let Ok(resp) = reqwest::get(&url).await {
                                if let Ok(bytes) = resp.bytes().await {
                                    let _ = tx.send(AppResult::ThumbnailLoaded(url, bytes.to_vec()));
                                }
                            }
                        }
                        WorkerMsg::FetchRadio(vid) => {
                            match api.get_radio_queue(&vid).await {
                                Ok(tracks) if !tracks.is_empty() => {
                                    let _ = tx.send(AppResult::RadioQueue(tracks));
                                }
                                _ => {}
                            }
                        }
                        WorkerMsg::FetchYtHistory => {
                            if let Ok(tracks) = api.get_yt_history().await {
                                if !tracks.is_empty() {
                                    let _ = tx.send(AppResult::YtHistory(tracks));
                                }
                            }
                        }
                    }
                }
            });
        }

        Self {
            tab: Tab::Search,
            query: String::new(),
            searching: false,
            results: vec![],
            history,
            favorites,
            queue: vec![],
            queue_pos: 0,
            radio_fetched: false,
            current: None,
            playing: false,
            position: 0.0,
            duration: 0.0,
            volume: 70.0,
            loading_stream: false,
            prev_position: 0.0,
            user_seeking: false,
            seek_target: 0.0,
            polling: false,
            last_poll: Instant::now(),
            login_open: false,
            cookie_buf: String::new(),
            config,
            error: None,
            error_end: Instant::now(),
            cmd_tx,
            res_rx,
            res_tx,
            player,
            storage,
            rt,
            textures: HashMap::new(),
            img_pending: HashSet::new(),
        }
    }

    // ── OLED Black theme ─────────────────────────────────────────────────────
    fn apply_theme(ctx: &egui::Context) {
        let mut s = (*ctx.style()).clone();
        s.text_styles = [
            (TextStyle::Heading,   FontId::proportional(20.0)),
            (TextStyle::Body,      FontId::proportional(14.0)),
            (TextStyle::Button,    FontId::proportional(13.5)),
            (TextStyle::Small,     FontId::proportional(11.0)),
            (TextStyle::Monospace, FontId::monospace(11.5)),
        ]
        .into();
        s.spacing.item_spacing   = Vec2::new(8.0, 5.0);
        s.spacing.button_padding = Vec2::new(14.0, 7.0);
        ctx.set_style(s);

        let mut v = egui::Visuals::dark();
        v.window_fill      = BG;
        v.panel_fill       = BG;
        v.extreme_bg_color = Color32::from_gray(4);
        v.code_bg_color    = SURF;

        let mk = |fill: Color32, fg: Color32, stroke_col: Color32| egui::style::WidgetVisuals {
            bg_fill:     fill,
            weak_bg_fill: fill,
            bg_stroke:   Stroke::new(1.0_f32, stroke_col),
            rounding:    Rounding::same(8.0),
            fg_stroke:   Stroke::new(1.0_f32, fg),
            expansion:   0.0,
        };

        v.widgets.noninteractive = mk(SURF,   TEXT,   BORDER);
        v.widgets.inactive       = mk(SURF,   TEXT_D, Color32::TRANSPARENT);
        v.widgets.hovered        = mk(SURF2,  TEXT,   ACCENT);
        v.widgets.active         = mk(ACCENT2, TEXT,  ACCENT);
        v.widgets.open           = mk(SURF2,  TEXT,   BORDER);

        v.selection.bg_fill    = Color32::from_rgba_premultiplied(255, 0, 48, 40);
        v.selection.stroke     = Stroke::new(1.0_f32, ACCENT);
        v.slider_trailing_fill = true;
        v.window_rounding      = Rounding::same(10.0);
        v.window_stroke        = Stroke::new(1.0_f32, BORDER);

        ctx.set_visuals(v);
    }

    // ── Async result drain ───────────────────────────────────────────────────
    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(res) = self.res_rx.try_recv() {
            match res {
                AppResult::SearchResults(tracks) => {
                    self.results   = tracks;
                    self.searching = false;
                }

                AppResult::StreamUrlReady(url, track) => {
                    self.loading_stream = false;
                    let guard = self.player.lock().unwrap();
                    if let Some(p) = guard.as_ref() {
                        match p.load_url(&url) {
                            Ok(_) => {
                                drop(guard);
                                // History
                                self.history.retain(|t| t.id != track.id);
                                self.history.insert(0, track.clone());
                                self.history.truncate(50);
                                self.storage.save_history(&self.history);

                                self.current      = Some(track.clone());
                                self.playing      = true;
                                self.prev_position = 0.0;
                                self.position     = 0.0;

                                // Auto-fetch radio queue for this track
                                if !self.radio_fetched {
                                    self.radio_fetched = true;
                                    let _ = self.cmd_tx.send(WorkerMsg::FetchRadio(track.id));
                                }
                            }
                            Err(e) => { drop(guard); self.toast(e.to_string()); }
                        }
                    }
                }

                AppResult::ThumbnailLoaded(url, bytes) => {
                    self.img_pending.remove(&url);
                    if let Ok(img) = image::load_from_memory(&bytes) {
                        let (w, h) = (img.width(), img.height());
                        let rgba = img.to_rgba8();
                        let ci = egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize], rgba.as_raw(),
                        );
                        let tex = ctx.load_texture(&url, ci, egui::TextureOptions::LINEAR);
                        self.textures.insert(url, tex);
                    }
                }

                AppResult::PlayerState { position, duration, paused } => {
                    self.polling = false;

                    // End-of-track detection:
                    // mpv becomes idle → time-pos returns null (→ 0) while duration also goes 0.
                    // We detect this by noticing position dropped to near-zero after a real position.
                    let track_ended = self.current.is_some()
                        && self.prev_position > 20.0  // was significantly into the song
                        && position < 2.0              // jumped back near zero
                        && duration < 2.0              // duration also disappeared
                        && !paused;                    // mpv not manually paused

                    if position > 2.0 {
                        self.prev_position = position;
                    }

                    if !self.user_seeking {
                        self.position = position;
                        self.duration = duration;
                    } else {
                        // While seeking, still update duration but keep slider position
                        self.duration = duration;
                    }
                    self.playing = !paused;

                    if track_ended {
                        self.prev_position = 0.0;
                        self.auto_next();
                    }
                }

                AppResult::RadioQueue(tracks) => {
                    // Append radio tracks that aren't already in the queue
                    for t in tracks {
                        if !self.queue.iter().any(|q| q.id == t.id) {
                            self.queue.push(t);
                        }
                    }
                }

                AppResult::YtHistory(tracks) => {
                    // Merge YT history with local — YT history takes precedence
                    self.history = tracks;
                    self.storage.save_history(&self.history);
                }

                AppResult::Error(e) => {
                    self.searching      = false;
                    self.loading_stream = false;
                    self.polling        = false;
                    self.toast(e);
                }
            }
        }
    }

    // ── mpv state polling ────────────────────────────────────────────────────
    fn poll_player(&mut self) {
        if self.polling
            || self.current.is_none()
            || self.last_poll.elapsed() < Duration::from_millis(600)
        {
            return;
        }
        self.polling   = true;
        self.last_poll = Instant::now();

        let player = self.player.clone();
        let tx     = self.res_tx.clone();
        self.rt.spawn(async move {
            let state = tokio::task::spawn_blocking(move || {
                player.lock().ok()?.as_ref().map(|p| p.get_state())
            })
            .await
            .ok()
            .flatten();

            if let Some((pos, dur, paused)) = state {
                let _ = tx.send(AppResult::PlayerState { position: pos, duration: dur, paused });
            } else {
                // Socket unavailable — mark polling done
                let _ = tx.send(AppResult::PlayerState { position: 0.0, duration: 0.0, paused: true });
            }
        });
    }

    // ── Queue / playback helpers ─────────────────────────────────────────────

    /// Automatically advance to next track in queue (radio or manual next)
    fn auto_next(&mut self) {
        let next = self.queue_pos + 1;
        if next < self.queue.len() {
            self.queue_pos = next;
            let track = self.queue[next].clone();
            self.do_play(track);
        }
    }

    fn next_track(&mut self) {
        self.auto_next();
    }

    fn prev_track(&mut self) {
        // If more than 5 s in, restart current track; otherwise go to previous
        if self.position > 5.0 {
            if let Some(p) = self.player.lock().unwrap().as_ref() {
                let _ = p.seek(0.0);
            }
            self.position     = 0.0;
            self.prev_position = 0.0;
        } else if self.queue_pos > 0 {
            self.queue_pos -= 1;
            let track = self.queue[self.queue_pos].clone();
            self.do_play(track);
        }
    }

    fn play_track(&mut self, track: Track) {
        // Replace queue with just this track; radio will refill it
        self.queue.clear();
        self.queue.push(track.clone());
        self.queue_pos    = 0;
        self.radio_fetched = false;
        self.do_play(track);
    }

    /// Internal: start loading the given track's stream without touching the queue
    fn do_play(&mut self, track: Track) {
        if self.loading_stream { return; }
        self.loading_stream = true;
        self.prev_position  = 0.0;
        if let Some(url) = &track.thumbnail_url { self.queue_img(url); }
        let _ = self.cmd_tx.send(WorkerMsg::GetStreamUrl(track));
    }

    // ── Misc helpers ─────────────────────────────────────────────────────────

    fn toast(&mut self, msg: String) {
        self.error     = Some(msg);
        self.error_end = Instant::now() + Duration::from_secs(5);
    }

    fn queue_img(&mut self, url: &str) {
        if !self.textures.contains_key(url) && self.img_pending.insert(url.to_string()) {
            let _ = self.cmd_tx.send(WorkerMsg::LoadImage(url.to_string()));
        }
    }

    fn prefetch_thumbs(&mut self, tracks: &[Track]) {
        let urls: Vec<_> = tracks
            .iter()
            .filter_map(|t| t.thumbnail_url.clone())
            .filter(|u| !self.textures.contains_key(u) && !self.img_pending.contains(u))
            .collect();
        for u in urls { self.queue_img(&u); }
    }

    fn is_fav(&self, id: &str) -> bool {
        self.favorites.iter().any(|t| t.id == id)
    }

    fn toggle_fav(&mut self, track: Track) {
        if self.is_fav(&track.id) {
            self.favorites.retain(|t| t.id != track.id);
        } else {
            self.favorites.insert(0, track);
        }
        self.storage.save_favorites(&self.favorites);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // UI panels
    // ══════════════════════════════════════════════════════════════════════════

    fn ui_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(52.0);
            ui.add_space(14.0);
            let app_title = format!("▶ YouTube Music v{}", env!("CARGO_PKG_VERSION"));
            ui.label(RichText::new(app_title).color(ACCENT).size(18.0).strong());

            // Queue info (right side)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(14.0);

                let (lbl, col) = if self.config.cookie.is_some() {
                    ("● Hesap", Color32::from_rgb(80, 210, 80))
                } else {
                    ("🔐  Giriş Yap", TEXT_D)
                };

                if ui.add(
                    egui::Button::new(RichText::new(lbl).color(col).size(13.0))
                        .fill(SURF)
                        .stroke(Stroke::new(1.0_f32, BORDER))
                        .rounding(Rounding::same(20.0)),
                ).clicked() {
                    self.login_open = true;
                    self.cookie_buf = self.config.cookie.clone().unwrap_or_default();
                }

                if self.queue.len() > 1 {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(format!("Kuyruk: {} şarkı", self.queue.len()))
                            .color(TEXT_M)
                            .size(11.5),
                    );
                }
            });
        });
    }

    fn ui_tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(42.0);
            ui.add_space(14.0);

            for (t, lbl) in [
                (Tab::Search,         "🔍  Ara"),
                (Tab::RecentlyPlayed, "🕐  Son Dinlenenler"),
                (Tab::Favorites,      "❤  Favoriler"),
            ] {
                let active = self.tab == t;
                let btn = egui::Button::new(
                    RichText::new(lbl)
                        .color(if active { ACCENT } else { TEXT_D })
                        .size(13.5),
                )
                .fill(Color32::TRANSPARENT)
                .stroke(if active { Stroke::new(2.0_f32, ACCENT) } else { Stroke::NONE })
                .rounding(Rounding::ZERO);

                if ui.add(btn).clicked() { self.tab = t; }
                ui.add_space(8.0);
            }
        });
    }

    fn ui_search_tab(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);

        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let avail = ui.available_width();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .desired_width(avail - 100.0)
                    .hint_text("Şarkı, sanatçı veya albüm ara…")
                    .margin(egui::Margin::symmetric(12.0, 8.0))
                    .font(FontId::proportional(14.0)),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

            let click = ui.add(
                egui::Button::new(
                    RichText::new(if self.searching { "⏳" } else { "Ara" }).size(13.0),
                )
                .fill(ACCENT)
                .rounding(Rounding::same(8.0))
                .min_size(Vec2::new(65.0, 36.0)),
            ).clicked();

            if (enter || click) && !self.query.trim().is_empty() && !self.searching {
                self.searching = true;
                let _ = self.cmd_tx.send(WorkerMsg::Search(self.query.clone()));
            }
            ui.add_space(14.0);
        });

        ui.add_space(10.0);

        if self.searching {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.label(RichText::new("Aranıyor…").color(TEXT_D).size(15.0));
            });
            return;
        }

        let results = self.results.clone();
        self.prefetch_thumbs(&results);

        let textures = self.textures.clone();
        let current  = self.current.clone();
        let favs     = self.favorites.clone();
        let playing  = self.playing;
        let mut actions: Vec<(Track, RowAction)> = vec![];

        egui::ScrollArea::vertical().id_source("search_scroll").show(ui, |ui| {
            if results.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(70.0);
                    ui.label(RichText::new("Bir şeyler ara başlamak için 🎵").color(TEXT_M).size(14.0));
                });
                return;
            }
            for track in &results {
                if let Some(a) = track_row(ui, track, &textures, &current, &favs, playing) {
                    actions.push((track.clone(), a));
                }
            }
        });

        for (t, a) in actions {
            match a {
                RowAction::Play      => self.play_track(t),
                RowAction::ToggleFav => self.toggle_fav(t),
            }
        }
    }

    fn ui_list_tab(&mut self, ui: &mut egui::Ui, title: &str, is_favs: bool) {
        let source: Vec<Track> = if is_favs {
            self.favorites.clone()
        } else {
            self.history.clone()
        };
        self.prefetch_thumbs(&source);

        let textures = self.textures.clone();
        let current  = self.current.clone();
        let favs     = self.favorites.clone();
        let playing  = self.playing;
        let mut actions: Vec<(Track, RowAction)> = vec![];

        egui::ScrollArea::vertical().id_source(title).show(ui, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(RichText::new(title).color(TEXT).size(18.0).strong());
            });
            ui.add_space(8.0);

            if source.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(50.0);
                    let hint = if is_favs {
                        "Favori şarkın yok. Bir şarkı çalarken ❤ simgesine bas!"
                    } else {
                        "Henüz dinlediğin şarkı yok."
                    };
                    ui.label(RichText::new(hint).color(TEXT_M).size(13.5));
                });
                return;
            }

            for track in &source {
                if let Some(a) = track_row(ui, track, &textures, &current, &favs, playing) {
                    actions.push((track.clone(), a));
                }
            }
        });

        for (t, a) in actions {
            match a {
                RowAction::Play      => self.play_track(t),
                RowAction::ToggleFav => self.toggle_fav(t),
            }
        }
    }

    fn ui_player_bar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(Color32::from_gray(4))
            .inner_margin(egui::Margin::symmetric(16.0, 10.0))
            .stroke(Stroke::new(1.0_f32, BORDER))
            .show(ui, |ui| {
                ui.set_min_height(82.0);

                if self.loading_stream {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("⏳  Yükleniyor…").color(TEXT_D).size(14.0));
                    });
                    return;
                }

                let Some(track) = self.current.clone() else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("♪  Çalmak için bir şarkı seç").color(TEXT_M).size(13.5));
                    });
                    return;
                };

                ui.horizontal(|ui| {
                    // ── Thumbnail ────────────────────────────────────────────
                    if let Some(url) = &track.thumbnail_url {
                        if let Some(tex) = self.textures.get(url) {
                            let t = tex.clone();
                            ui.add(
                                egui::Image::new(&t)
                                    .fit_to_exact_size(Vec2::splat(56.0))
                                    .rounding(Rounding::same(6.0)),
                            );
                        }
                    }
                    ui.add_space(10.0);

                    // ── Track info ───────────────────────────────────────────
                    ui.vertical(|ui| {
                        ui.set_width(180.0);
                        ui.label(RichText::new(&track.title).color(TEXT).size(13.5).strong());
                        ui.label(RichText::new(&track.artist).color(TEXT_D).size(11.0));

                        // Queue position indicator
                        if self.queue.len() > 1 {
                            ui.label(
                                RichText::new(format!(
                                    "{} / {}",
                                    self.queue_pos + 1,
                                    self.queue.len()
                                ))
                                .color(TEXT_M)
                                .size(10.0),
                            );
                        }
                    });

                    ui.add_space(8.0);

                    // ── Controls + progress ──────────────────────────────────
                    ui.vertical(|ui| {
                        // ⏮ / ⏸▶ / ⏭
                        ui.horizontal(|ui| {
                            let side_pad = (ui.available_width() - 116.0) / 2.0;
                            ui.add_space(side_pad.max(0.0));

                            // Previous
                            let prev_en = self.queue_pos > 0 || self.position > 5.0;
                            if ui.add_enabled(
                                prev_en,
                                egui::Button::new(RichText::new("⏮").size(16.0).color(if prev_en { TEXT } else { TEXT_M }))
                                    .fill(Color32::TRANSPARENT)
                                    .min_size(Vec2::splat(34.0)),
                            ).clicked() {
                                self.prev_track();
                            }

                            ui.add_space(4.0);

                            // Play / Pause
                            let icon = if self.playing { "⏸" } else { "▶" };
                            if ui.add(
                                egui::Button::new(RichText::new(icon).size(18.0).color(TEXT))
                                    .fill(ACCENT)
                                    .rounding(Rounding::same(20.0))
                                    .min_size(Vec2::splat(40.0)),
                            ).clicked() {
                                if let Some(p) = self.player.lock().unwrap().as_ref() {
                                    let _ = p.toggle_pause();
                                    self.playing = !self.playing;
                                }
                            }

                            ui.add_space(4.0);

                            // Next
                            let next_en = self.queue_pos + 1 < self.queue.len();
                            if ui.add_enabled(
                                next_en,
                                egui::Button::new(RichText::new("⏭").size(16.0).color(if next_en { TEXT } else { TEXT_M }))
                                    .fill(Color32::TRANSPARENT)
                                    .min_size(Vec2::splat(34.0)),
                            ).clicked() {
                                self.next_track();
                            }
                        });

                        ui.add_space(4.0);

                        // Progress slider — seek only on drag release to avoid stream interruption
                        ui.horizontal(|ui| {
                            let display = if self.user_seeking { self.seek_target } else { self.position };
                            let mut t = if self.duration > 0.0 {
                                (display / self.duration) as f32
                            } else { 0.0 };

                            ui.label(RichText::new(fmt_time(display)).color(TEXT_M).size(10.0));

                            let slider_w = (ui.available_width() - 68.0).max(40.0);
                            let r = ui.add_sized(
                                [slider_w, 20.0],
                                egui::Slider::new(&mut t, 0.0..=1.0).show_value(false),
                            );

                            // Update visual target while dragging
                            if r.dragged() {
                                self.user_seeking = true;
                                self.seek_target  = t as f64 * self.duration;
                            }
                            // Commit seek only when finger/mouse is released
                            if r.drag_stopped() {
                                self.user_seeking = false;
                                if self.duration > 0.0 {
                                    let target = t as f64 * self.duration;
                                    if let Some(p) = self.player.lock().unwrap().as_ref() {
                                        let _ = p.seek(target);
                                    }
                                    self.position = target;
                                }
                            }

                            ui.label(RichText::new(fmt_time(self.duration)).color(TEXT_M).size(10.0));
                        });
                    });

                    // ── Volume ───────────────────────────────────────────────
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.set_width(112.0);
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("🔊").size(12.0).color(TEXT_D));
                            let r = ui.add_sized(
                                [72.0, 20.0],
                                egui::Slider::new(&mut self.volume, 0.0..=100.0).show_value(false),
                            );
                            if r.changed() {
                                if let Some(p) = self.player.lock().unwrap().as_ref() {
                                    let _ = p.set_volume(self.volume);
                                }
                            }
                        });
                    });
                });
            });
    }

    fn ui_login_dialog(&mut self, ctx: &egui::Context) {
        if !self.login_open { return; }

        egui::Window::new("Giriş Yap")
            .collapsible(false)
            .resizable(true)
            .anchor(egui::Align2::CENTER_CENTER, [0.0; 2])
            .default_width(500.0)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(SURF)
                    .stroke(Stroke::new(1.0_f32, BORDER)),
            )
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("YouTube Music — Cookie ile Giriş").size(16.5).strong().color(TEXT));
                ui.add_space(10.0);

                egui::Frame::none()
                    .fill(SURF2)
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        for step in [
                            "1. Tarayıcında music.youtube.com'a gir ve hesabına giriş yap.",
                            "2. F12 → Network (Ağ) sekmesini aç.",
                            "3. Herhangi bir isteğe tıkla → Request Headers bölümüne bak.",
                            "4. 'Cookie:' satırının tüm değerini kopyala, aşağıya yapıştır.",
                        ] {
                            ui.label(RichText::new(step).color(TEXT_D).size(12.0));
                        }
                    });

                ui.add_space(10.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.cookie_buf)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .hint_text("Cookie değerini buraya yapıştır…")
                        .font(FontId::monospace(11.0)),
                );
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui.add(
                        egui::Button::new("İptal").fill(SURF2).rounding(Rounding::same(8.0))
                    ).clicked() {
                        self.login_open = false;
                    }

                    if self.config.cookie.is_some() {
                        ui.add_space(6.0);
                        if ui.add(
                            egui::Button::new(
                                RichText::new("Çıkış Yap").color(Color32::from_rgb(255, 85, 85))
                            )
                            .fill(SURF2).rounding(Rounding::same(8.0))
                        ).clicked() {
                            self.config.cookie = None;
                            self.storage.save_config(&self.config);
                            self.login_open = false;
                        }
                    }

                    ui.add_space(6.0);
                    let ok = !self.cookie_buf.trim().is_empty();
                    if ui.add_enabled(
                        ok,
                        egui::Button::new(RichText::new("Kaydet").color(TEXT))
                            .fill(if ok { ACCENT } else { SURF2 })
                            .rounding(Rounding::same(8.0))
                            .min_size(Vec2::new(72.0, 28.0)),
                    ).clicked() {
                        self.config.cookie = Some(self.cookie_buf.trim().to_string());
                        self.storage.save_config(&self.config);
                        self.login_open = false;
                        self.toast("Cookie kaydedildi. Geçmişini çekiyorum…".into());
                        let _ = self.cmd_tx.send(WorkerMsg::FetchYtHistory);
                    }
                });
            });
    }
}

// ── Pure track row renderer ───────────────────────────────────────────────────

fn track_row(
    ui: &mut egui::Ui,
    track: &Track,
    textures: &HashMap<String, egui::TextureHandle>,
    current: &Option<Track>,
    favs: &[Track],
    playing: bool,
) -> Option<RowAction> {
    let is_cur = current.as_ref().map(|t| t.id == track.id).unwrap_or(false);
    let is_fav = favs.iter().any(|t| t.id == track.id);
    let mut action = None;

    let bg = if is_cur {
        Color32::from_rgba_premultiplied(255, 0, 48, 20)
    } else {
        SURF
    };

    egui::Frame::none()
        .fill(bg)
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(12.0, 7.0))
        .outer_margin(egui::Margin { left: 14.0, right: 14.0, top: 0.0, bottom: 4.0 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let sz = Vec2::splat(46.0);
                if let Some(url) = &track.thumbnail_url {
                    if let Some(tex) = textures.get(url) {
                        ui.add(
                            egui::Image::new(tex)
                                .fit_to_exact_size(sz)
                                .rounding(Rounding::same(4.0)),
                        );
                    } else {
                        thumb_placeholder(ui, sz);
                    }
                } else {
                    thumb_placeholder(ui, sz);
                }

                ui.add_space(8.0);

                ui.vertical(|ui| {
                    ui.set_min_width(150.0);
                    ui.label(
                        RichText::new(&track.title)
                            .color(if is_cur { ACCENT } else { TEXT })
                            .size(14.0)
                            .strong(),
                    );
                    let mut sub = track.artist.clone();
                    if let Some(al) = &track.album { sub.push_str(&format!(" • {al}")); }
                    ui.label(RichText::new(sub).color(TEXT_D).size(11.5));
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let pi = if is_cur && playing { "⏸" } else { "▶" };
                    if ui.add(
                        egui::Button::new(RichText::new(pi).size(14.0).color(TEXT))
                            .fill(if is_cur { ACCENT2 } else { SURF2 })
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::splat(30.0)),
                    ).clicked() {
                        action = Some(RowAction::Play);
                    }

                    ui.add_space(4.0);

                    if ui.add(
                        egui::Button::new(
                            RichText::new(if is_fav { "❤" } else { "♡" })
                                .color(if is_fav { ACCENT } else { TEXT_M })
                                .size(15.0),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .min_size(Vec2::splat(30.0)),
                    ).clicked() {
                        action = Some(RowAction::ToggleFav);
                    }

                    ui.add_space(4.0);

                    if let Some(s) = track.duration_secs {
                        ui.label(
                            RichText::new(format!("{}:{:02}", s / 60, s % 60))
                                .color(TEXT_M)
                                .size(11.0),
                        );
                    }
                });
            });
        });

    action
}

fn thumb_placeholder(ui: &mut egui::Ui, size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(4.0), SURF2);
    ui.painter().text(
        rect.center(), egui::Align2::CENTER_CENTER,
        "♪", FontId::proportional(20.0), TEXT_M,
    );
}

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

// ── eframe::App ───────────────────────────────────────────────────────────────
impl eframe::App for YtMusicApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        Self::apply_theme(ctx);
        self.drain(ctx);
        self.poll_player();

        if self.error.is_some() && Instant::now() > self.error_end {
            self.error = None;
        }

        // Keep repainting while playing for the progress bar
        if self.playing {
            ctx.request_repaint_after(Duration::from_millis(600));
        }

        egui::TopBottomPanel::top("top")
            .frame(egui::Frame::none().fill(BG).stroke(Stroke::new(1.0_f32, BORDER)))
            .show(ctx, |ui| self.ui_top_bar(ui));

        egui::TopBottomPanel::top("tabs")
            .frame(egui::Frame::none().fill(BG).stroke(Stroke::new(1.0_f32, BORDER)))
            .show(ctx, |ui| self.ui_tab_bar(ui));

        if let Some(err) = self.error.clone() {
            egui::TopBottomPanel::top("toast")
                .frame(
                    egui::Frame::none()
                        .fill(Color32::from_rgb(65, 12, 12))
                        .inner_margin(egui::Margin::symmetric(16.0, 6.0)),
                )
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(format!("⚠  {err}"))
                            .color(Color32::from_rgb(255, 130, 130))
                            .size(12.5),
                    );
                });
        }

        egui::TopBottomPanel::bottom("player")
            .frame(egui::Frame::none())
            .show(ctx, |ui| self.ui_player_bar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG))
            .show(ctx, |ui| {
                if self.tab == Tab::Search {
                    self.ui_search_tab(ui);
                } else if self.tab == Tab::RecentlyPlayed {
                    self.ui_list_tab(ui, "Son Dinlenenler", false);
                } else {
                    self.ui_list_tab(ui, "Favoriler", true);
                }
            });

        self.ui_login_dialog(ctx);
    }
}
