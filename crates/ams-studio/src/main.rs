// no console window on Windows release builds
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod doc;
mod icon;
mod icons;
mod plots;

fn main() -> eframe::Result {
    // developer helper: write the icon files (used to create assets/)
    let mut args = std::env::args().skip(1);
    if let Some(a) = args.next() {
        if a == "--write-icons" {
            let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| "assets".into()));
            std::fs::create_dir_all(&dir).expect("create dir");
            let raw = icon::rgba(256);
            image::save_buffer(dir.join("icon.png"), &raw, 256, 256, image::ColorType::Rgba8).expect("png");
            std::fs::write(dir.join("icon.ico"), icon::ico_bytes(&[256, 64, 48, 32, 16])).expect("ico");
            return Ok(());
        }
    }
    let open = std::env::args().nth(1).map(std::path::PathBuf::from).filter(|p| p.exists());
    let (size, maximized) = app::saved_window();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size.unwrap_or([1280.0, 820.0]))
            .with_maximized(maximized)
            .with_min_inner_size([760.0, 480.0])
            .with_icon(std::sync::Arc::new(egui::IconData { rgba: icon::rgba(256), width: 256, height: 256 }))
            .with_title("AMS Studio")
            .with_app_id("ams-studio"),
        ..Default::default()
    };
    eframe::run_native("AMS Studio", opts, Box::new(move |cc| Ok(Box::new(app::App::new(cc, open.clone())))))
}
