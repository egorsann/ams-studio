// no console window on Windows release builds
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod doc;
mod icons;
mod plots;

fn main() -> eframe::Result {
    let open = std::env::args().nth(1).map(std::path::PathBuf::from).filter(|p| p.is_file());
    let (size, maximized) = app::saved_window();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size.unwrap_or([1280.0, 820.0]))
            .with_maximized(maximized)
            .with_min_inner_size([760.0, 480.0])
            .with_title("Anisoft")
            .with_app_id("anisoft-linux"),
        ..Default::default()
    };
    eframe::run_native("Anisoft", opts, Box::new(move |cc| Ok(Box::new(app::App::new(cc, open.clone())))))
}
