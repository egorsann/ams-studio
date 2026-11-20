// no console window on Windows release builds
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod core_view;
mod fdialog;
mod crash;
mod doc;
mod hyper;
mod hyper_py;
mod icon;
mod icons;
mod plots;
mod util;
mod veins;

fn main() -> eframe::Result {
    crash::install();
    // `--write-setup [folder] [--force]`: writes this computer's setup as AMS_STUDIO_SETUP_dependancy.json into the folder of the
    // specimens, for another computer with the same data to take over once
    {
        let a: Vec<String> = std::env::args().collect();
        if let Some(i) = a.iter().position(|x| x == "--write-setup") {
            let folder = a.get(i + 1).filter(|x| !x.starts_with("--")).map(std::path::PathBuf::from);
            let r = app::write_setup_cli(folder, a.iter().any(|x| x == "--force"));
            match &r {
                Ok(m) => println!("{m}"),
                Err(e) => eprintln!("{e}"),
            }
            std::process::exit(if r.is_ok() { 0 } else { 1 });
        }
    }
    // `--cube-size <side mm> [--comment D=text]... <.ams file or specimen folder>...`: keeps the cube size (and comments on cubes that
    // differ) with each specimen, in its dependency file; `--cube-size off <...>` removes it. The .ams files are not touched.
    {
        let a: Vec<String> = std::env::args().collect();
        if let Some(i) = a.iter().position(|x| x == "--cube-size") {
            let side = a.get(i + 1).map(|s| s.as_str()).unwrap_or("");
            let mut comments = vec![];
            let mut files: Vec<std::path::PathBuf> = vec![];
            let mut k = i + 2;
            while k < a.len() {
                if a[k] == "--comment" {
                    if let Some((l, t)) = a.get(k + 1).and_then(|c| c.split_once('=')) {
                        comments.push((l.trim().to_uppercase(), t.trim().to_string()));
                    }
                    k += 2;
                    continue;
                }
                let p = std::path::PathBuf::from(&a[k]);
                // a folder stands for its first .ams file
                let f = if p.is_dir() {
                    let mut v: Vec<_> = std::fs::read_dir(&p).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|e| e.extension().map_or(false, |x| x.eq_ignore_ascii_case("ams"))).collect()).unwrap_or_default();
                    v.sort();
                    v.into_iter().next()
                } else {
                    Some(p)
                };
                match f {
                    Some(f) => files.push(f),
                    None => eprintln!("no .ams file in {}", a[k]),
                }
                k += 1;
            }
            let cs = side.parse::<f64>().ok().filter(|s| *s > 0.0).map(|side_mm| ams_core::depjson::CubeSize { side_mm, comments });
            if cs.is_none() && side != "off" {
                eprintln!("usage: ams-studio --cube-size <side in mm | off> [--comment D=text] <.ams file or folder>...");
                std::process::exit(2);
            }
            let mut bad = 0;
            for f in &files {
                match ams_core::depjson::write_cube_size(f, cs.as_ref()) {
                    Ok(()) => println!("{}: {}", ams_core::depjson::for_source(f).display(), cs.as_ref().map_or("cube size removed".to_string(), |c| format!("{} mm cubes, {} cm3", c.side_mm, (c.volume_cm3() * 1e4).round() / 1e4))),
                    Err(e) => {
                        bad += 1;
                        eprintln!("{}: {e}", f.display());
                    }
                }
            }
            std::process::exit(if bad == 0 && !files.is_empty() { 0 } else { 1 });
        }
    }
    // `--demo-tree <folder>`: writes made-up specimen folders (own folders, one folder with 7-digit IDs, .ran pairs, split phases)
    {
        let a: Vec<String> = std::env::args().collect();
        if let Some(i) = a.iter().position(|x| x == "--demo-tree") {
            app::demo_tree(std::path::Path::new(a.get(i + 1).map(|s| s.as_str()).unwrap_or("ams-demo-tree")));
            return Ok(());
        }
    }
    // `--selftest <report file>`: walks through every menu and window without opening one (see app/selftest.rs). It runs
    // on a thread with the 1 MB stack of a Windows main thread, so a stack overflow shows up on every system.
    {
        let a: Vec<String> = std::env::args().collect();
        if let Some(i) = a.iter().position(|x| x == "--selftest") {
            let out = std::path::PathBuf::from(a.get(i + 1).cloned().unwrap_or_else(|| "ams-studio-selftest.txt".into()));
            let stack: usize = std::env::var("AMS_STUDIO_SELFTEST_STACK").ok().and_then(|v| v.parse().ok()).unwrap_or(1 << 20);
            let o2 = out.clone();
            let fails = std::thread::Builder::new().stack_size(stack).spawn(move || app::selftest(&o2)).ok().and_then(|h| h.join().ok());
            match fails {
                Some(0) => std::process::exit(0),
                Some(_) => std::process::exit(1),
                None => {
                    let _ = std::fs::write(&out, "the self-test itself stopped on an error (see crash.log)\n");
                    std::process::exit(2);
                }
            }
        }
    }
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
    if positional.iter().any(|a| a == "--reset-windows") {
        app::reset_window_positions();
        positional.retain(|a| a != "--reset-windows");
    }
    let open = positional.first().map(std::path::PathBuf::from).filter(|p| p.exists());
    let (size, maximized) = app::saved_window();
    let mut viewport = egui::ViewportBuilder::default()
            .with_inner_size(size.unwrap_or([1280.0, 820.0]))
            .with_maximized(maximized)
            .with_min_inner_size([760.0, 480.0])
            .with_icon(std::sync::Arc::new(egui::IconData { rgba: icon::rgba(256), width: 256, height: 256 }))
            .with_title("AMS Studio")
            .with_app_id("ams-studio");
    // the window reopens where it was (the same screen, when the same screens are connected)
    if let Some(p) = app::saved_window_pos() {
        viewport = viewport.with_position(p);
    }
    let opts = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("AMS Studio", opts, Box::new(move |cc| Ok(Box::new(app::App::new(cc, open.clone())))))
}
