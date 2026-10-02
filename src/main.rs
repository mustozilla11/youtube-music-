mod api;
mod app;
mod player;
mod storage;
mod types;

use std::sync::Arc;

fn main() -> eframe::Result<()> {
    // Multi-threaded tokio runtime for API calls, image downloads, mpv polling
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime başlatılamadı"),
    );

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 680.0])
            .with_min_inner_size([640.0, 480.0])
            .with_title("YouTube Music"),
        ..Default::default()
    };

    eframe::run_native(
        "YouTube Music",
        opts,
        Box::new(move |cc| Ok(Box::new(app::YtMusicApp::new(cc, rt)))),
    )
}
