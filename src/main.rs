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

    let title = format!("YouTube Music v{}", env!("CARGO_PKG_VERSION"));
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 680.0])
            .with_min_inner_size([640.0, 480.0])
            .with_title(&title),
        ..Default::default()
    };

    eframe::run_native(
        &title,
        opts,
        Box::new(move |cc| Ok(Box::new(app::YtMusicApp::new(cc, rt)))),
    )
}
