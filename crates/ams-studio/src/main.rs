// no console window on Windows release builds
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod core_view;
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
    // switches for a computer without a graphics card (see About): --backend dx12|vulkan|gl (wgpu's backend),
    // --max-fps N (0 = no limit); anything else is the file or folder to open
    let mut positional: Vec<String> = vec![];
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let take = |name: &str, a: &str, it: &mut dyn Iterator<Item = String>| -> Option<String> {
            if let Some(v) = a.strip_prefix(&format!("{name}=")) {
                Some(v.to_string())
            } else if a == name {
                it.next()
            } else {
                None
            }
        };
        if let Some(v) = take("--backend", &a, &mut it) {
            let b = match v.to_lowercase().as_str() {
                "gl" | "opengl" => "opengl".to_string(),
                other => other.to_string(),
            };
            std::env::set_var("WGPU_BACKEND", b);
        } else if let Some(v) = take("--max-fps", &a, &mut it) {
            if let Ok(n) = v.parse::<u32>() {
                app::set_max_fps(n);
            }
        } else {
            positional.push(a);
        }
    }
    let open = positional.first().map(std::path::PathBuf::from).filter(|p| p.exists());
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
