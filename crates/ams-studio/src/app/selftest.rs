//! `ams-studio --selftest <report file>`: builds the program without a window, feeds it made-up data and walks
//! through every menu action, every window, the folder-of-specimens questions and the phase joining, drawing real
//! frames (shapes are tessellated like a screen would) and catching any panic. The same walk runs on Linux and in the
//! Windows build, where a program error would otherwise close the window silently. Uses only made-up files in a
//! temporary folder and a temporary preferences folder.

use super::*;
use ams_core::specset;
use ams_core::ams::{build_record_with, Phase};
use ams_core::tensor::sym;
use egui::RawInput;

#[derive(Debug)]
struct FakeDrop(PathBuf);

impl egui::DroppedFile for FakeDrop {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

struct Harness {
    ctx: egui::Context,
    app: App,
    t: f64,
    report: String,
    fails: usize,
    steps: usize,
    /// Errors the guarded frames caught (message and step), in order.
    panics: Vec<String>,
    seen: u32,
    /// Events for the next frames (the monkey).
    queue: std::collections::VecDeque<Vec<egui::Event>>,
    mods: egui::Modifiers,
    /// Files dropped on the window in the next frame.
    drops: Vec<PathBuf>,
}

fn lcg(seed: &mut u64) -> f64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    ((*seed >> 33) as f64) / ((1u64 << 31) as f64)
}

fn phase(seed: &mut u64, km: f64) -> Phase {
    let (a, b) = (0.05 + 0.1 * lcg(seed), 0.05 * lcg(seed));
    Phase { km, sd: [1e-3, 2e-3, 3e-3], tensor: sym(1.0 + a, 1.0 - a * 0.5, 1.0 - a * 0.5, b, -b * 0.5, b * 0.3), ftest: [5.0, 2.0, 1.5] }
}

fn zero() -> Phase {
    Phase { km: 0.0, sd: [0.0; 3], tensor: [[0.0; 3]; 3], ftest: [0.0; 3] }
}

/// `n` specimens `<id>-A1`.. with both phases (or one phase only).
fn records(id: &str, n: usize, both: bool, seed: &mut u64) -> Vec<u8> {
    let mut out = vec![];
    for i in 0..n {
        let name = format!("{id}-{}{}", (b'A' + (i % 4) as u8) as char, i / 4 + 1);
        let ip = phase(seed, 2e-3 + 1e-4 * i as f64);
        let op = if both { phase(seed, -3e-6) } else { zero() };
        out.extend(build_record_with(&name, &ip, &op, 400.0, 1220.0, "KLY5"));
    }
    out
}

fn ran_file(names: &[String], km: f32, sign: f32, seed: &mut u64) -> Vec<u8> {
    let mut b = vec![b' '; 64];
    b[0] = (names.len() + 2) as u8;
    b[1] = 0;
    b[48..60].copy_from_slice(b"3  0  3  0  ");
    for n in names {
        let mut rec = vec![0u8; 64];
        rec[..12].fill(b' ');
        rec[..n.len().min(12)].copy_from_slice(&n.as_bytes()[..n.len().min(12)]);
        rec[16..20].copy_from_slice(&km.to_le_bytes());
        let t = [sign * (1.05 + 0.1 * lcg(seed)) as f32, sign * 0.97, sign * 0.98, 0.01, 0.0, 0.02];
        for (i, v) in t.iter().enumerate() {
            rec[20 + 4 * i..24 + 4 * i].copy_from_slice(&(*v as f32).to_le_bytes());
        }
        rec[44..46].copy_from_slice(b"SS");
        b.extend(&rec);
    }
    b
}

fn w(p: &Path, bytes: &[u8]) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, bytes);
}

/// The made-up files: own folders, one shared folder with 7-digit IDs, a .ran set with out-of-phase partners, two
/// files per specimen with phase words, and a single pair.
fn make_data(root: &Path) {
    let mut s = 7u64;
    for i in 1..=4 {
        let id = format!("ZZ{i:02}");
        w(&root.join("own").join(format!("{id}_AMS")).join(format!("{id}_use.ams")), &records(&id, 8, true, &mut s));
        if i % 2 == 0 {
            w(&root.join("own").join(format!("{id}_AMS")).join(format!("{id}_raw.ams")), &records(&id, 8, true, &mut s));
        }
    }
    for i in 0..5u32 {
        let id = format!("{}", 1234560 + i);
        w(&root.join("flat7").join(format!("{id}_use.ams")), &records(&id, 6, true, &mut s));
        w(&root.join("flat7").join(format!("{id}_raw.ams")), &records(&id, 6, true, &mut s));
    }
    for i in 1..=3 {
        let id = format!("JE{i:03}");
        let names: Vec<String> = (0..6).map(|k| format!("{}{}", (b'A' + (k % 3) as u8) as char, k / 3 + 1)).collect();
        let tagged: Vec<String> = names.iter().map(|n| format!("{n}_Im")).collect();
        let d = root.join("ran").join(&id);
        w(&d.join(format!("{id}.ran")), &ran_file(&names, 1500.0, 1.0, &mut s));
        w(&d.join(format!("{id}_Im.ran")), &ran_file(&tagged, 2.5, -1.0, &mut s));
    }
    for id in ["PH1", "PH2"] {
        let both = records(id, 6, true, &mut s);
        let ip: Vec<u8> = both.chunks(640).flat_map(|r| {
            let mut r = r.to_vec();
            r[324..468].fill(0);
            r
        }).collect();
        let op: Vec<u8> = both.chunks(640).flat_map(|r| {
            let mut r = r.to_vec();
            let blk: Vec<u8> = r[324..468].to_vec();
            r[180..324].copy_from_slice(&blk);
            r[324..468].fill(0);
            r
        }).collect();
        w(&root.join("phase").join(format!("{id}.ams")), &ip);
        w(&root.join("phase").join(format!("{id}_oop.ams")), &op);
    }
    w(&root.join("single").join("demo.ams"), &records("DEMO", 12, true, &mut s));
}

impl Harness {
    fn frames(&mut self, n: usize, size: Vec2) {
        for _ in 0..n {
            let events = self.queue.pop_front().unwrap_or_default();
            let dropped_files = std::mem::take(&mut self.drops).into_iter().map(|p| std::sync::Arc::new(FakeDrop(p)) as egui::DroppedFileHandle).collect();
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), time: Some(self.t), events, dropped_files, ..Default::default() };
            self.t += 0.02;
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(input, |ui| app.guarded_frame(ui));
            out.textures_delta.clear();
            let _ = self.ctx.tessellate(out.shapes, out.pixels_per_point);
            if self.app.crash.total > self.seen {
                self.seen = self.app.crash.total;
                let m = self.app.crash.notice.take().unwrap_or_else(|| "panic in a frame".into());
                self.panics.push(m);
                self.app.crash.streak = 0;
                self.app.crash.rescue = false;
            }
        }
    }

    fn step(&mut self, name: &str, f: impl FnOnce(&mut App, &egui::Context)) {
        self.steps += 1;
        let before = self.panics.len();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut self.app, &self.ctx)));
        let mut problem = r.err().map(|_| crate::crash::take_last().unwrap_or_else(|| "panic".into()));
        self.frames(4, Vec2::new(1400.0, 900.0));
        if problem.is_none() && self.panics.len() > before {
            problem = Some(self.panics[before..].join(" || "));
        }
        self.app.crash.notice = None;
        match problem {
            None => self.report.push_str(&format!("ok    {name}\n")),
            Some(p) => {
                self.fails += 1;
                self.report.push_str(&format!("FAIL  {name}: {}\n", p.replace('\n', " | ")));
            }
        }
        self.app.close_all_dialogs();
    }

    /// Random clicks, drags, wheel turns, keys and text, plus now and then a menu action or a window, for `n` frames.
    fn monkey(&mut self, name: &str, n: usize, seed: u64, size: Vec2, actions: &[Action]) {
        use egui::{Event, Key, PointerButton};
        self.steps += 1;
        let before = self.panics.len();
        let mut s = seed;
        let mut seen_actions: std::collections::HashSet<String> = std::collections::HashSet::new();
        let keys = [Key::Q, Key::E, Key::W, Key::A, Key::S, Key::D, Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown, Key::Escape, Key::Enter, Key::Tab, Key::Delete, Key::F1, Key::Space, Key::Num1, Key::Num0, Key::Plus, Key::Minus, Key::Z, Key::Y, Key::P, Key::O, Key::N, Key::R, Key::B];
        for i in 0..n {
            let pos = |s: &mut u64| Pos2::new(lcg(s) as f32 * size.x, lcg(s) as f32 * size.y);
            let mods = match (lcg(&mut s) * 10.0) as u32 {
                0 => egui::Modifiers::CTRL,
                1 => egui::Modifiers::SHIFT,
                2 => egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                3 => egui::Modifiers::ALT,
                _ => egui::Modifiers::NONE,
            };
            match (lcg(&mut s) * 10.0) as u32 {
                0..=3 => {
                    let p = pos(&mut s);
                    let b = if lcg(&mut s) < 0.15 { PointerButton::Secondary } else { PointerButton::Primary };
                    self.queue.push_back(vec![Event::PointerMoved(p)]);
                    self.queue.push_back(vec![Event::PointerButton { pos: p, button: b, pressed: true, modifiers: mods }]);
                    self.queue.push_back(vec![Event::PointerButton { pos: p, button: b, pressed: false, modifiers: mods }]);
                }
                4 | 5 => {
                    // a drag: press, a few moves, release
                    let p = pos(&mut s);
                    self.queue.push_back(vec![Event::PointerMoved(p)]);
                    self.queue.push_back(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: mods }]);
                    let mut q = p;
                    for _ in 0..4 {
                        q = Pos2::new((q.x + (lcg(&mut s) as f32 - 0.5) * 200.0).clamp(0.0, size.x), (q.y + (lcg(&mut s) as f32 - 0.5) * 200.0).clamp(0.0, size.y));
                        self.queue.push_back(vec![Event::PointerMoved(q)]);
                    }
                    self.queue.push_back(vec![Event::PointerButton { pos: q, button: PointerButton::Primary, pressed: false, modifiers: mods }]);
                }
                6 => {
                    let p = pos(&mut s);
                    self.queue.push_back(vec![Event::PointerMoved(p), Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: Vec2::new((lcg(&mut s) as f32 - 0.5) * 80.0, (lcg(&mut s) as f32 - 0.5) * 240.0), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: mods }]);
                }
                7 | 8 => {
                    let k = keys[(lcg(&mut s) * keys.len() as f64) as usize % keys.len()];
                    self.queue.push_back(vec![Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: mods }]);
                    self.queue.push_back(vec![Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: mods }]);
                }
                _ => {
                    let t = ["1", "-", "e", "0.5", " ", "x", "\u{e9}", "9999999999"][(lcg(&mut s) * 8.0) as usize % 8];
                    self.queue.push_back(vec![Event::Text(t.to_string())]);
                }
            }
            if i % 37 == 5 && !actions.is_empty() {
                let a = actions[(lcg(&mut s) * actions.len() as f64) as usize % actions.len()];
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.app.run(&self.ctx, a)));
                if r.is_err() {
                    self.panics.push(format!("{} (action {a:?})", crate::crash::take_last().unwrap_or_default()));
                }
            }
            if i % 53 == 11 {
                let slot = (lcg(&mut s) * (W_RECOMB_ASK + 1) as f64) as usize % (W_RECOMB_ASK + 1);
                self.app.open_win(slot);
            }
            // a few frames per random action; the actions the clicks and keys reached are counted
            let last0 = self.app.crash.last.clone();
            for _ in 0..3 {
                self.frames(1, size);
                if self.app.crash.last != last0 {
                    seen_actions.insert(self.app.crash.last.clone());
                }
            }
        }
        self.queue.clear();
        self.frames(2, size);
        if self.panics.len() > before {
            let mut m: Vec<String> = self.panics[before..].to_vec();
            m.sort();
            m.dedup();
            self.fails += 1;
            self.report.push_str(&format!("FAIL  {name}: {} error(s): {}\n", self.panics.len() - before, m.join(" || ").replace('\n', " | ")));
        } else {
            self.report.push_str(&format!("ok    {name} (actions reached by clicks and keys: {})\n", seen_actions.len()));
        }
        self.mods = egui::Modifiers::NONE;
        self.app.close_all_dialogs();
    }

    fn check(&mut self, name: &str, ok: bool, detail: String) {
        self.steps += 1;
        if ok {
            self.report.push_str(&format!("ok    {name}\n"));
        } else {
            self.fails += 1;
            self.report.push_str(&format!("FAIL  {name}: {detail}\n"));
        }
    }
}

/// Writes the made-up files (`--demo-tree <folder>`), for trying the folder and recombine windows by hand.
pub fn write_data(root: &Path) {
    make_data(root);
}

/// Runs the walk; returns the number of failed steps. The report is also written to `out`.
pub fn run(out: &Path) -> usize {
    crate::fdialog::block_dialogs(true);
    // the walk must not ask GitHub for updates or take over a setup from the Synology folder
    std::env::set_var("AMS_STUDIO_NO_UPDATE_CHECK", "1");
    std::env::set_var("AMS_STUDIO_NO_SEED", "1");
    let root = std::env::temp_dir().join(format!("ams-selftest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    make_data(&root);
    // preferences and the crash log of the test go to a folder of their own
    let cfg = root.join("cfg");
    let _ = std::fs::create_dir_all(&cfg);
    std::env::set_var("XDG_CONFIG_HOME", &cfg);
    std::env::set_var("APPDATA", &cfg);
    let ctx = egui::Context::default();
    let app = App::create(&ctx, None);
    let mut h = Harness { ctx, app, t: 0.0, panics: vec![], seen: 0, queue: Default::default(), mods: egui::Modifiers::NONE, drops: vec![], report: format!("AMS Studio {} selftest on {}\n", env!("CARGO_PKG_VERSION"), std::env::consts::OS), fails: 0, steps: 0 };
    let big = Vec2::new(1400.0, 900.0);

    // ---- an empty program in several window sizes and both themes
    h.step("start, no file", |_, _| {});
    for (n, sz) in [("min size 760x480", Vec2::new(760.0, 480.0)), ("full hd", Vec2::new(1920.0, 1080.0)), ("tiny 300x200", Vec2::new(300.0, 200.0))] {
        h.step(&format!("frames at {n}"), |_, _| {});
        h.frames(3, sz);
    }
    h.step("dark theme", |a, c| a.run(c, Action::ToggleTheme));
    h.step("light theme", |a, c| a.run(c, Action::ToggleTheme));

    // ---- open a file and go through every action that needs no file dialog
    let demo = root.join("single").join("demo.ams");
    h.step("open a file", |a, _| a.open_path(demo.clone()));
    h.check("file is open with 12 specimens", h.app.doc.as_ref().map_or(0, |d| d.specs.len()) == 12, format!("{:?}", h.app.doc.as_ref().map(|d| d.specs.len())));
    let actions: Vec<(&str, Action)> = vec![
        ("Edit Data", Action::EditData),
        ("Set Data Classes", Action::SetClasses),
        ("Spec to Geo (Owens)", Action::Owens),
        ("Drill Core to Geo", Action::CoreGeo),
        ("Data table", Action::DataTable),
        ("Group statistics", Action::GroupStats),
        ("Contour plot", Action::Contour),
        ("Factors", Action::Factors),
        ("Orientation parameters", Action::OrientParams),
        ("Colors", Action::Colors),
        ("About", Action::About),
        ("GIS settings", Action::Gis),
        ("GIS locations", Action::GisLocations),
        ("GIS enter coordinates", Action::GisManual),
        ("Toggle GIS pane", Action::ToggleGis),
        ("Toggle GIS pane again", Action::ToggleGis),
        ("Command palette", Action::Palette),
        ("Toggle core viewer", Action::ToggleCoreViewer),
        ("Toggle core viewer again", Action::ToggleCoreViewer),
        ("Toggle file browser", Action::ToggleBrowser),
        ("Toggle file browser again", Action::ToggleBrowser),
        ("Volume correction", Action::Volume),
        ("Export as .ams window", Action::ExportAms),
        ("Recombine", Action::Recombine),
        ("Out-of-phase", Action::Phase(true)),
        ("In-phase", Action::Phase(false)),
        ("Toggle phase", Action::TogglePhase),
        ("Toggle phase again", Action::TogglePhase),
        ("Next specimen", Action::StepSpecimen(1)),
        ("Previous specimen", Action::StepSpecimen(-1)),
        ("Invert selection", Action::InvertSel),
        ("Disregard", Action::Disregard),
        ("Restore all", Action::RestoreAll),
        ("Clear selection", Action::ClearSel),
        ("Add pane", Action::AddPane),
        ("Remove pane", Action::RemovePane),
        ("Reset panes", Action::ResetPanes),
        ("Undo reset panes", Action::UndoResetPanes),
        ("Reload", Action::Reload),
        ("GIS fit", Action::GisFit),
        ("Pop out GIS", Action::PopOutGis),
        ("Pop in GIS", Action::PopOutGis),
    ];
    for (n, a) in actions {
        h.step(&format!("action: {n}"), move |app, c| app.run(c, a));
    }
    for cs in [ams_core::coords::CoordSys::Specimen, ams_core::coords::CoordSys::Geographic, ams_core::coords::CoordSys::Paleo, ams_core::coords::CoordSys::Tectonic, ams_core::coords::CoordSys::Specimen] {
        h.step(&format!("coordinates {cs:?}"), move |app, c| app.run(c, Action::Coord(cs)));
    }
    for t in tools_ui::TOOL_NAMES {
        h.step(&format!("tool: {t}"), move |app, c| app.run(c, Action::Tool(t)));
    }
    // every window slot, with a file and without
    for slot in 0..=W_RECOMB_ASK {
        h.step(&format!("window slot {slot} with a file"), move |app, _| app.open_win(slot));
    }
    h.step("close the file", |a, c| a.run(c, Action::Close));
    for slot in 0..=W_RECOMB_ASK {
        h.step(&format!("window slot {slot} without a file"), move |app, _| app.open_win(slot));
    }

    // ---- hyperspectral parameters on the cubes of the core viewer (a made-up dashboard code and feature table)
    {
        let dep = root.join("hyper").join("working_files_doc").join("Python_doc").join("dependancies");
        let table_dir = root.join("hyper").join("data").join("core_samples_SPECTRAL").join("core_samples_SPECTRAL_extracted");
        let _ = std::fs::create_dir_all(&dep);
        let _ = std::fs::create_dir_all(&table_dir);
        let code = "FEATURES_CSV = os.path.join(_SPEC_OUT, \"ER26PB_spectral_features.csv\")\nCUBE_ROWS = {\n  \"cube_d2200\": (\"D2200\", \"2200_ok\", \"White mica\\nD2200\", \"depth\"),\n  \"cube_w900\": (\"W900_shift\", \"900_ok\", \"Fe3+ pos.\\nW900 shift\", \"nm\"),\n  \"cube_ism\": (\"ISM\", \"ISM_ok\", \"ISM\", \"ISM\"),\n}\nCUBE_ROW_DIVERGING = {\"cube_w900\": [\"#7f0000\", \"#f7f7f7\", \"#7a4a00\"]}\nW900_REF, W900_HALF = 900.0, 30.0\nCUBE_ROW_DIVERGING_HALF = {\"cube_w900\": W900_HALF}\nEDGE_FRAC = 0.24\n";
        w(&dep.join(crate::hyper::MODULE_FILE), code.as_bytes());
        w(&dep.join("hyperspectral_extract_asd_dependancy.py"), b"ISM_MIN_D2200 = 0.05\nOUT_DIR = os.path.join(SPECTRAL_ROOT, \"core_samples_SPECTRAL_extracted\")\n");
        let id = core_pane::spec_id(&root.join("single").join("demo.ams"));
        let mut csv = String::from("specimen,cube,side,D2200,2200_ok,W900,900_ok,ISM,ISM_ok\n");
        for sl in 1..=3 {
            for l in ["A", "B", "C"] {
                for (side, k) in [("BTM", 1.0), ("BK", 0.5), ("L", 0.2)] {
                    csv.push_str(&format!("{id},{l}{sl},{side},{:.3},True,{:.1},True,{:.2},True\n", 0.1 * sl as f64 * k, 880.0 + 10.0 * sl as f64, 1.0 + k));
                }
            }
        }
        w(&table_dir.join("ER26PB_spectral_features.csv"), csv.as_bytes());
        std::env::set_var("AMS_STUDIO_HYPER_DIR", &dep);
        h.step("open the demo file for the core viewer", |a, _| a.open_path(demo.clone()));
        h.app.cv.on = true;
        h.step("core viewer on", |_, _| {});
        h.step("Ctrl+Shift+E asks for the hyperspectral parameters", |a, c| a.run(c, Action::CoreMode(1)));
        for _ in 0..200 {
            if h.app.cv.hy.is_some() || !h.app.cv.hy_err.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.frames(1, big);
        }
        h.frames(3, big);
        let names: Vec<String> = h.app.cv.hy.as_ref().map(|x| x.rows.iter().map(|r| r.key.clone()).collect()).unwrap_or_default();
        h.check("the parameters were read from the code", names == ["cube_d2200", "cube_w900", "cube_ism"], format!("{names:?} {}", h.app.cv.hy_err));
        h.check("the first press went to the first parameter", h.app.cv.mode == 1, format!("mode {}", h.app.cv.mode));
        h.step("the cubes in colour", |_, _| {});
        for want in [2usize, 3, 0, 1] {
            h.step(&format!("next mode -> {want}"), |a, c| a.run(c, Action::CoreMode(1)));
            h.check(&format!("mode {want}"), h.app.cv.mode == want, format!("mode {}", h.app.cv.mode));
        }
        h.step("previous mode", |a, c| a.run(c, Action::CoreMode(-1)));
        h.check("back to photos", h.app.cv.mode == 0, format!("mode {}", h.app.cv.mode));
        // the keyboard: Ctrl+Shift+E while the pointer is over the core viewer
        h.frames(2, big);
        let c = h.app.cv.rect.center();
        let m = egui::Modifiers::CTRL | egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        h.queue.push_back(vec![egui::Event::PointerMoved(c)]);
        h.queue.push_back(vec![egui::Event::ModifiersChanged(m), egui::Event::Key { key: egui::Key::E, physical_key: Some(egui::Key::E), pressed: true, repeat: false, modifiers: m }]);
        h.queue.push_back(vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)]);
        h.step("Ctrl+Shift+E over the core viewer", |_, _| {});
        h.check("the keyboard moved to the next parameter", h.app.cv.mode == 1, format!("mode {}", h.app.cv.mode));
        h.check("the choice is in the preferences", h.app.state_text().contains("core_mode=cube_d2200"), "not saved".to_string());
        std::env::remove_var("AMS_STUDIO_HYPER_DIR");
        h.app.cv.on = false;
        h.app.cv.mode = 0;
    }

    // ---- vein planes from alpha / beta (made-up dashboard code with VEIN_PLANES and a notebook cell with BOREHOLE_CONFIGS)
    {
        let dep = root.join("veins").join("working_files_doc").join("Python_doc").join("dependancies");
        let _ = std::fs::create_dir_all(&dep);
        let id = core_pane::spec_id(&root.join("single").join("demo.ams"));
        let code = format!("VEIN_PLANES = {{\n    \"{id}\": [{{\"name\": \"vein 2 (main)\", \"alpha\": 35, \"beta\": 315, \"zones\": (2, 4)}},\n        {{\"name\": \"vein 3\", \"alpha\": 22.5, \"beta\": 330, \"zones\": (6,)}}],\n}}\n");
        w(&dep.join(crate::hyper::MODULE_FILE), code.as_bytes());
        let cell = format!("BOREHOLE_CONFIGS = {{\n    \"{id}\": {{\n        \"DIP_DIR\": 305.0,\n        \"PLUNGE\": 69.0,\n        \"INTERVALS\": [\n            (0, 32, 45.0)  # turn\n        ]\n    }}\n}}\n");
        let nb = serde_json::json!({ "cells": [{ "cell_type": "code", "source": [cell] }] });
        w(&dep.parent().unwrap().join("dashboard_from_dot_ams.ipynb"), nb.to_string().as_bytes());
        std::env::set_var("AMS_STUDIO_HYPER_DIR", &dep);
        h.app.st_planes.clear();
        h.app.planes_for = None;
        h.step("open the demo file for the vein planes", |a, _| a.open_path(demo.clone()));
        h.check("the dashboard tables were read", h.app.st_dash.as_ref().map_or(false, |d| d.specimen(&id).veins.len() == 2), "not read".to_string());
        h.step("add the veins of the dashboard", |a, _| a.add_dashboard_veins());
        h.check("two vein planes", h.app.st_planes.iter().filter(|q| q.ab.is_some()).count() == 2, format!("{}", h.app.st_planes.len()));
        h.step("add the veins again", |a, _| a.add_dashboard_veins());
        h.check("none twice", h.app.st_planes.iter().filter(|q| q.ab.is_some()).count() == 2, format!("{}", h.app.st_planes.len()));
        let (dd, dp) = h.app.st_planes.first().map_or((0.0, 0.0), |q| (q.dip_dir, q.dip));
        h.check("the plane is the dashboard's", (dd - 87.184).abs() < 0.01 && (dp - 70.858).abs() < 0.01, format!("{dd} {dp}"));
        h.app.st_planes_on = true;
        h.app.coord = ams_core::coords::CoordSys::Geographic;
        h.check("the stereonet draws them", h.app.plane_draws().len() == 2, format!("{}", h.app.plane_draws().len()));
        h.step("the stereonet with vein planes", |_, _| {});
        std::env::remove_var("AMS_STUDIO_HYPER_DIR");
        h.app.st_planes.clear();
        h.app.st_dash = None;
        h.app.st_planes_on = false;
    }

    // ---- every pane into a window of its own and back, also with the shortcut over the pane under the pointer
    {
        h.app.cv.on = true;
        h.app.gis_on = true;
        for id in [PaneId::Stereo, PaneId::Plot(1), PaneId::Plot(2), PaneId::Plot(3), PaneId::Core] {
            h.step(&format!("pop out {id:?}"), move |a, _| a.toggle_pane_pop(id));
            h.check(&format!("{id:?} is in a window of its own"), h.app.popped.iter().any(|(p, _)| *p == id), "not popped".to_string());
            h.step(&format!("dock {id:?}"), move |a, _| a.toggle_pane_pop(id));
            h.check(&format!("{id:?} is back"), !h.app.popped.iter().any(|(p, _)| *p == id), "still popped".to_string());
        }
        // the shortcut acts on the pane under the pointer
        h.frames(3, big);
        let rects = h.app.pane_rects.clone();
        for (id, r) in rects {
            if id == PaneId::Gis {
                continue;
            }
            h.queue.push_back(vec![egui::Event::PointerMoved(r.center())]);
            h.frames(2, big);
            h.step(&format!("Shift+Ctrl+Space over {id:?}"), |a, c| a.run(c, Action::PopOutPane));
            h.check(&format!("{id:?} popped by the shortcut"), h.app.popped.iter().any(|(p, _)| *p == id), "not popped".to_string());
            h.step(&format!("dock {id:?} again"), move |a, _| a.toggle_pane_pop(id));
        }
        h.app.gis_on = false;
        h.app.cv.on = false;
        h.app.popped.clear();
    }

    // ---- folder of specimens: the three layouts of the questions
    for (name, dir, kind, layout, expect_specimens, expect_pairs) in [
        ("own folders (.ams)", "own", specset::Kind::Ams, specset::Layout::OwnFolders, 4usize, 0usize),
        ("own folders, auto", "own", specset::Kind::Ams, specset::Layout::Auto, 4, 0),
        ("one folder with 7-digit IDs", "flat7", specset::Kind::Ams, specset::Layout::OneFolder, 5, 0),
        ("one folder, auto", "flat7", specset::Kind::Ams, specset::Layout::Auto, 5, 0),
        (".ran with out-of-phase files", "ran", specset::Kind::Ran, specset::Layout::OwnFolders, 3, 3),
        ("two .ams files per specimen by name", "phase", specset::Kind::Ams, specset::Layout::OneFolder, 2, 2),
    ] {
        let rootp = root.join(dir);
        let raw = specset::scan_raw(&rootp);
        let opts = specset_ui::SetOpts { kind, layout, phases: specset::Phases::ByName, scheme: specset_ui::SchemeChoice::Auto };
        let b = opts.build(&rootp, &raw);
        let pairs: usize = b.folders.iter().map(|f| f.pairs.len()).sum();
        h.check(&format!("scan {name}: specimens"), b.folders.len() == expect_specimens && pairs == expect_pairs, format!("{} specimens, {pairs} pairs (wanted {expect_specimens}, {expect_pairs})", b.folders.len()));
        // through the dialog: scan thread, questions, list, then open the first main file and walk the set
        let r2 = rootp.clone();
        h.step(&format!("folder dialog: {name}"), move |app, c| {
            app.set_opts = opts;
            app.begin_scan(c, r2);
        });
        for _ in 0..100 {
            if h.app.sd.rx.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            h.frames(1, big);
        }
        h.step(&format!("folder dialog questions: {name}"), |_, _| {});
        h.check(&format!("dialog {name}: asks first"), h.app.sd.ask && !h.app.sd.raw.dirs.is_empty(), "the questions did not show".to_string());
        h.app.sd.ask = false;
        h.app.sd.opts = opts;
        h.app.sd.rebuild(None);
        h.step(&format!("folder dialog list: {name}"), |_, _| {});
        let set = specset_ui::SpecSet { root: rootp.clone(), raw: h.app.sd.raw.clone(), opts, folders: h.app.sd.folders.clone(), ident: h.app.sd.ident.clone(), picks: h.app.sd.picks.clone() };
        let mains = set.mains();
        h.check(&format!("set {name}: main files"), mains.len() == expect_specimens, format!("{} main files of {} specimens, string {:?}", mains.len(), set.folders.len(), set.ident));
        h.app.set = Some(set);
        h.app.win_set = false;
        if let Some(first) = mains.first().cloned() {
            h.step(&format!("open first of {name}"), move |app, _| app.open_path(first));
            let n = h.app.doc.as_ref().map_or(0, |d| d.specs.len());
            let both = h.app.doc.as_ref().map_or(false, |d| d.specs.iter().all(|s| s.outphase.km != 0.0));
            h.check(&format!("{name}: first file has records and both phases"), n > 0 && both, format!("{n} records, both phases {both}"));
        }
        for i in 0..mains.len() {
            h.step(&format!("{name}: next file {}", i + 1), |app, c| app.run(c, Action::Next));
        }
        h.step(&format!("{name}: previous file"), |app, c| app.run(c, Action::Prev));
        h.step(&format!("{name}: select all matched"), |app, _| app.select_all_matched());
        h.step(&format!("{name}: refresh set"), |app, _| app.refresh_set());
        h.step(&format!("{name}: change string dialog"), |app, _| app.change_set_string());
        h.app.set = None;
        h.step(&format!("{name}: close set"), |app, c| app.run(c, Action::Close));
    }

    // ---- the Recombine questions and the phase join
    h.step("recombine question window", |a, _| a.open_win(W_RECOMB_ASK));
    h.step("recombine parts window", |a, _| a.open_win(W_RECOMB));
    let ph = root.join("phase");
    {
        let (ip, op) = (ph.join("PH1.ams"), ph.join("PH1_oop.ams"));
        h.step("join one pair (picked in-phase file finds its partner)", |a, _| {
            a.pj.goal = phase_join_ui::Goal::One;
            a.pj.kind = specset::Kind::Ams;
            a.pj.set_ip(ip.clone());
            a.open_win(W_PHASE);
        });
        let ok = h.app.pj.op.as_deref() == Some(op.as_path()) && matches!(&h.app.pj.preview, Some(Ok((_, m))) if m.joined == 6 && m.from_first_slot == 6);
        h.check("one pair: partner found, six joined", ok, format!("{:?}", h.app.pj.op));
        h.step("join one pair: swapped roles", |a, _| {
            a.pj.swap();
            a.open_win(W_PHASE);
        });
        h.check("swapped roles give a warning", !h.app.pj.warn.is_empty(), "no warning".to_string());
    }
    h.step("join many ran pairs", |a, _| {
        a.pj.goal = phase_join_ui::Goal::Many;
        a.pj.kind = specset::Kind::Ran;
        a.pj.scan_many(root.join("ran"));
        a.open_win(W_PHASE);
    });
    h.check("many: three .ran pairs found", h.app.pj.rows.iter().filter(|r| r.ip.is_some() && r.op.is_some()).count() == 3, format!("{} rows", h.app.pj.rows.len()));
    let outdir = root.join("joined");
    let rows = h.app.pj.rows.clone();
    match phase_join_ui::save_pairs(&rows, &outdir) {
        Ok(rep) => {
            let back: Vec<_> = rep.written.iter().filter_map(|p| ams_core::ams::read_file(p).ok()).collect();
            let full = back.len() == 3 && back.iter().all(|s| s.len() == 6 && s.iter().all(|x| x.outphase.km < 0.0 && x.inphase.km > 0.0));
            h.check("many: saved files hold both phases (out-of-phase Km negative, sign recovered)", full, format!("{} files", back.len()));
        }
        Err(e) => h.check("many: save", false, e),
    }
    h.check("many: refuses a data folder as output", phase_join_ui::save_pairs(&rows, &rows[0].dir).is_err(), "it wrote into the data folder".to_string());
    h.step("a .ran opened directly joins its partner", |a, _| a.open_path(root.join("ran").join("JE001").join("JE001.ran")));
    h.check("the .ran file opened with both phases", h.app.doc.as_ref().map_or(false, |d| d.specs.len() == 6 && d.specs.iter().all(|s| s.outphase.km < 0.0)), format!("{:?}", h.app.doc.as_ref().map(|d| d.note.clone())));
    h.step("an out-of-phase .ran opened by itself opens the pair", |a, _| a.open_path(root.join("ran").join("JE002").join("JE002_Im.ran")));
    h.step("import dialog code path (no dialog): doc note", |_, _| {});

    // ---- coordinates of specimens that share a folder, and the one-time take-over of a setup
    {
        let flat = root.join("flat7");
        let (a, b) = (flat.join("1234560_use.ams"), flat.join("1234561_use.ams"));
        let la = gis_loc::Location::new(500000.0, 5000000.0, ams_core::gis::Crs::Utm { zone: 43, north: true }, "typed");
        let lb = gis_loc::Location::new(510000.0, 5010000.0, ams_core::gis::Crs::Utm { zone: 43, north: true }, "typed");
        h.check("a location can be built", la.is_some() && lb.is_some(), "no location".to_string());
        if let (Some(la), Some(lb)) = (la, lb) {
            let ra = gis_loc::write_for(&a, Some(&la));
            let rb = gis_loc::write_for(&b, Some(&lb));
            h.check("two specimens of one folder are saved", ra.is_ok() && rb.is_ok(), format!("{ra:?} {rb:?}"));
            let (ga, gb) = (gis_loc::read_for(&a), gis_loc::read_for(&b));
            h.check("each specimen reads back its own coordinates", ga.as_ref().map(|l| l.x) == Some(500000.0) && gb.as_ref().map(|l| l.x) == Some(510000.0), format!("{:?} {:?}", ga.map(|l| l.x), gb.map(|l| l.x)));
            let _ = gis_loc::write_for(&a, None);
            let _ = gis_loc::write_for(&b, None);
        }
        // a setup written by one computer into the folder of the specimens is taken over once by another computer that opens it
        let data = root.join("setupdata").join("SynologyDrive").join("AMS");
        let cfg_a = root.join("seedcfg-a");
        let cfg_b = root.join("seedcfg-b");
        for d in [data.join("ER1_AMS"), cfg_a.clone(), cfg_b.clone()] {
            let _ = std::fs::create_dir_all(d);
        }
        let st = format!("dark=1\nwin_w=1000\nset_root={}\n", data.to_string_lossy().replace('\\', "/"));
        w(&cfg_a.join("state.txt"), st.as_bytes());
        let wrote = seed::write_setup(&cfg_a, None, false, "selftest");
        h.check("the setup file is written next to the specimen folders", wrote.is_ok() && data.join(seed::FILE).is_file(), format!("{wrote:?}"));
        h.check("the writing computer does not take it over", seed::take_over(&cfg_a, &[data.clone()], &seed::host_name()) == seed::Taken::No, "it did".to_string());
        w(&cfg_b.join("state.txt"), b"dark=0\nwin_w=700\n");
        let first = seed::take_over(&cfg_b, &[data.join("ER1_AMS")], "another-computer");
        let got = std::fs::read_to_string(cfg_b.join("state.txt")).unwrap_or_default();
        h.check("another computer takes the setup over", matches!(first, seed::Taken::Yes(_)) && got.contains("dark=1") && got.contains("win_w=700"), format!("{first:?} {got:?}"));
        h.check("and only once", seed::take_over(&cfg_b, &[data.clone()], "another-computer") == seed::Taken::No, "twice".to_string());
        // taken over while the program runs: the opened folder holds a setup of another computer (also GIS layers)
        {
            let dir = root.join("livesetup");
            let _ = std::fs::create_dir_all(&dir);
            let layer = dir.join("m.png");
            w(&layer, b"x");
            let lp = layer.to_string_lossy().replace('\\', "/");
            let mut seedv = seed::make_setup(&format!("marker=8\ndark=0\ngis_layers={{\"tree\":[{{\"id\":1,\"kind\":\"map\",\"name\":\"Taken over\",\"visible\":false,\"open\":true}}],\"maps\":[{{\"id\":1,\"path\":\"{lp}\",\"opacity\":0.5}}],\"vectors\":[],\"selected\":null}}\n"), "", &dir, "another-computer", "live-1", "now");
            seedv["from_host"] = serde_json::json!("another-computer");
            w(&dir.join(seed::FILE), serde_json::to_string_pretty(&seedv).unwrap_or_default().as_bytes());
            h.app.gis_on = true;
            std::env::remove_var("AMS_STUDIO_NO_SEED");
            let d2 = dir.clone();
            h.step("a setup file in an opened folder is taken over while running", move |a, _| a.setup_near(&[d2]));
            let names: Vec<String> = h.app.gis.layers.items.iter().filter(|i| !i.kind.is_builtin()).map(|i| i.name.clone()).collect();
            h.check("its layers and preferences are in the running program", (h.app.marker - 8.0).abs() < 1e-3 && names == ["Taken over"], format!("marker {} layers {names:?}", h.app.marker));
            std::env::set_var("AMS_STUDIO_NO_SEED", "1");
            h.app.gis_on = false;
        }
        h.check("a folder without a setup file is left alone", seed::take_over(&cfg_b, &[root.join("flat7")], "x") == seed::Taken::No, "touched".to_string());
    }

    // ---- an ArcGIS project: dropped on the window, and chosen; the file itself is never written
    {
        let dir = root.join("arc");
        let _ = std::fs::create_dir_all(dir.join("data"));
        for f in ["faults.shp", "units.shp", "dem.tif"] {
            w(&dir.join("data").join(f), b"x");
        }
        let doc = r#"{"type":"CIMMapDocument","mapDefinition":{"type":"CIMMap","name":"Map","spatialReference":{"wkid":27700},"layers":["g1","r1"]},"layerDefinitions":[
          {"type":"CIMGroupLayer","name":"Geology","uRI":"g1","visibility":false,"layers":["f1","f2"]},
          {"type":"CIMFeatureLayer","name":"Faults","uRI":"f1","visibility":false,"featureTable":{"definitionExpression":"[TYPE] = 'thrust'","dataConnection":{"type":"CIMStandardDataConnection","workspaceConnectionString":"DATABASE=data","dataset":"faults"}}},
          {"type":"CIMFeatureLayer","name":"Units","uRI":"f2","visibility":false,"featureTable":{"dataConnection":{"type":"CIMStandardDataConnection","workspaceConnectionString":"DATABASE=C:\\other pc\\data","dataset":"units.shp"}}},
          {"type":"CIMRasterLayer","name":"DEM","uRI":"r1","visibility":false,"dataConnection":{"type":"CIMStandardDataConnection","workspaceConnectionString":"DATABASE=data","dataset":"dem.tif"}}]}"#;
        let lyrx = dir.join("project.mapx");
        w(&lyrx, doc.as_bytes());
        let before = std::fs::read(&lyrx).unwrap();
        h.app.gis.layers = Default::default();
        h.app.gis.layer_undo.clear();
        h.drops.push(lyrx.clone());
        h.step("drop an ArcGIS map file on the window", |_, _| {});
        let n = h.app.gis.layers.items.iter().filter(|i| !i.kind.is_builtin()).count();
        h.check("the dropped map file added a group and three layers", n == 4 && h.app.gis_on, format!("{n} layers, GIS on {}", h.app.gis_on));
        let filter = h.app.gis.restore_vecs.values().find(|v| !v.filter.is_empty()).map(|v| v.filter.clone());
        h.check("its definition query came along as a filter", filter.as_deref() == Some("\"TYPE\" = 'thrust'"), format!("{filter:?}"));
        h.step("undo the project import", |a, c| a.run(c, Action::GisUndoLayers));
        let n = h.app.gis.layers.items.iter().filter(|i| !i.kind.is_builtin()).count();
        h.check("undo took the whole block out again", n == 0, format!("{n} left"));
        h.check("the project file was not touched", std::fs::read(&lyrx).unwrap() == before, "it changed".to_string());
        // a damaged ArcGIS file and an unreadable ArcMap file are reported, not fatal
        let bad = dir.join("bad.aprx");
        w(&bad, b"not a zip");
        h.drops.push(bad.clone());
        h.step("drop a damaged .aprx", |_, _| {});
        h.check("the damaged file gave a message", !h.app.gis.map_err.is_empty(), "no message".to_string());
        let badmxd = dir.join("bad.mxd");
        w(&badmxd, b"not a compound document");
        h.drops.push(badmxd);
        h.step("drop a damaged .mxd", |_, _| {});
    }

    // ---- several layers at once: Shift+drag a rectangle over the rows, Delete, Ctrl+Z
    {
        use egui::{Event, Key, PointerButton};
        let dir = root.join("layers");
        let _ = std::fs::create_dir_all(&dir);
        h.app.gis_on = true;
        h.app.gis.show_layers = true;
        h.app.gis.layers = Default::default();
        let mut ids = vec![];
        for i in 0..6 {
            let f = dir.join(format!("m{i}.png"));
            w(&f, b"x");
            let id = h.app.gis.layers.add_map(&format!("Layer {i}"));
            h.app.gis.layers.set_visible(id, false);
            h.app.gis.restore_maps.insert(id, gis_ui::MapSpec { path: f, opacity: 0.5, georef: None, crs: None });
            ids.push(id);
        }
        h.step("layers sidebar with six layers", |_, _| {});
        let rows = h.app.gis.row_hits.clone();
        h.check("the sidebar drew the six rows", rows.len() == 6, format!("{} rows", rows.len()));
        if rows.len() == 6 {
            let shift = egui::Modifiers::SHIFT;
            let (a, b) = (rows[0].1.center(), rows[5].1.center());
            h.queue.push_back(vec![Event::PointerMoved(a)]);
            h.queue.push_back(vec![Event::ModifiersChanged(shift), Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: shift }]);
            for k in 1..=6 {
                let p = Pos2::new(a.x, a.y + (b.y - a.y) * k as f32 / 6.0);
                h.queue.push_back(vec![Event::PointerMoved(p)]);
            }
            h.queue.push_back(vec![Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: shift }, Event::ModifiersChanged(egui::Modifiers::NONE)]);
            h.frames(14, big);
            h.step("shift-drag over every row", |_, _| {});
            h.check("no widget holds the keyboard after the drag", !h.ctx.egui_wants_keyboard_input(), format!("focused: {:?}", h.ctx.memory(|m| m.focused())));
            h.check("the rectangle selected all six layers", h.app.gis.multi.len() == 6, format!("{} selected", h.app.gis.multi.len()));
            h.queue.push_back(vec![Event::PointerMoved(a)]);
            h.queue.push_back(vec![Event::Key { key: Key::Delete, physical_key: Some(Key::Delete), pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
            h.step("Delete removes all selected layers", |_, _| {});
            let left = h.app.gis.layers.items.iter().filter(|i| !i.kind.is_builtin()).count();
            h.check("no layers are left", left == 0, format!("{left} left"));
            h.check("the removal is in the undo list", h.app.gis.layer_undo.len() == 1, format!("{}", h.app.gis.layer_undo.len()));
            let c = egui::Modifiers::CTRL | egui::Modifiers::COMMAND;
            h.queue.push_back(vec![Event::PointerMoved(a)]);
            h.queue.push_back(vec![Event::ModifiersChanged(c), Event::Key { key: Key::Z, physical_key: Some(Key::Z), pressed: true, repeat: false, modifiers: c }]);
            h.queue.push_back(vec![Event::ModifiersChanged(egui::Modifiers::NONE)]);
            h.step("Ctrl+Z puts the layers back", |_, _| {});
            let back = h.app.gis.layers.items.iter().filter(|i| !i.kind.is_builtin()).count();
            h.check("all six layers are back", back == 6 && h.app.gis.restore_maps.len() == 6, format!("{back} back, {} specs", h.app.gis.restore_maps.len()));
        }
        h.app.gis_on = false;
    }

    // ---- every window open at once, in every window size
    h.app.set = None;
    h.step("open the demo file again", |a, _| a.open_path(demo.clone()));
    for (n, sz) in [("200x150", Vec2::new(200.0, 150.0)), ("320x240", Vec2::new(320.0, 240.0)), ("480x320", Vec2::new(480.0, 320.0)), ("760x480", Vec2::new(760.0, 480.0)), ("1000x600", Vec2::new(1000.0, 600.0)), ("1920x1200", Vec2::new(1920.0, 1200.0)), ("3840x2160", Vec2::new(3840.0, 2160.0))] {
        let before = h.panics.len();
        h.steps += 1;
        h.app.gis_on = true;
        h.app.cv.on = true;
        for slot in 0..=W_RECOMB_ASK {
            h.app.open_win(slot);
        }
        for t in 0..tools_ui::TOOL_NAMES.len() {
            h.app.tools.open[t] = true;
        }
        h.frames(6, sz);
        if h.panics.len() > before {
            h.fails += 1;
            let m = h.panics[before..].join(" || ").replace('\n', " | ");
            h.report.push_str(&format!("FAIL  every window at once, {n}: {m}\n"));
        } else {
            h.report.push_str(&format!("ok    every window at once, {n}\n"));
        }
        h.app.close_all_dialogs();
    }

    // ---- random clicks, drags, wheel turns and keys over the whole program
    let all_actions: Vec<Action> = vec![
        Action::Open, Action::OpenSet, Action::Prev, Action::Next, Action::Reload, Action::Import, Action::SaveActive, Action::AppendSel, Action::ExportCsv, Action::ExportSelection,
        Action::EditData, Action::SetClasses, Action::InvertSel, Action::Disregard, Action::RestoreAll, Action::ClearSel, Action::DataTable, Action::Phase(true), Action::Phase(false),
        Action::Coord(ams_core::coords::CoordSys::Geographic), Action::Coord(ams_core::coords::CoordSys::Specimen), Action::GroupStats, Action::Contour, Action::Factors, Action::OrientParams,
        Action::Owens, Action::CoreGeo, Action::Colors, Action::ToggleBrowser, Action::ToggleCoreViewer, Action::Gis, Action::ToggleGis, Action::ToggleLayerBar, Action::PopOutGis, Action::AddPane,
        Action::RemovePane, Action::ResetLayout, Action::ToggleTheme, Action::StepSpecimen(1), Action::StepSpecimen(-1), Action::TogglePhase, Action::ResetPanes, Action::UndoResetPanes,
        Action::Palette, Action::GisLocations, Action::GisManual, Action::GisFit, Action::Recombine, Action::Volume, Action::ExportAms, Action::Tool("GED Editor"), Action::Tool("Deform AMS"),
        Action::Tool("Fabric Overprint"), Action::Tool("Intersection Toolbox"), Action::Tool("Tensor Addition / Subtraction"), Action::Close,
    ];
    let own = root.join("own");
    h.app.set_opts = specset_ui::SetOpts::default();
    h.step("a set for the monkey", |a, c| a.begin_scan(c, own.clone()));
    for _ in 0..50 {
        if h.app.sd.rx.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.frames(1, big);
    }
    h.app.sd.ask = false;
    h.app.sd.rebuild(None);
    if let Some(sd_set) = Some(specset_ui::SpecSet { root: own.clone(), raw: h.app.sd.raw.clone(), opts: h.app.sd.opts, folders: h.app.sd.folders.clone(), ident: h.app.sd.ident.clone(), picks: h.app.sd.picks.clone() }) {
        h.app.set = Some(sd_set);
    }
    h.app.win_set = false;
    h.step("open the demo file for the monkey", |a, _| a.open_path(demo.clone()));
    let rounds: u64 = std::env::var("AMS_STUDIO_SELFTEST_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    for r in 0..rounds {
        let k = r * 100;
        h.app.gis_on = false;
        h.app.cv.on = false;
        h.monkey("monkey 1: 1400x900, a file and a set", 600, 11 + k, big, &all_actions);
        h.monkey("monkey 2: 760x480", 600, 12 + k, Vec2::new(760.0, 480.0), &all_actions);
        h.monkey("monkey 3: 320x240", 300, 13 + k, Vec2::new(320.0, 240.0), &all_actions);
        h.monkey("monkey 4: 1920x1080", 600, 14 + k, Vec2::new(1920.0, 1080.0), &all_actions);
        h.app.gis_on = true;
        h.app.cv.on = true;
        h.monkey("monkey 5: GIS pane and core viewer, 1400x900", 600, 15 + k, big, &all_actions);
    }
    h.step("close everything", |a, c| a.run(c, Action::Close));
    h.app.set = None;
    for r in 0..rounds {
        h.monkey("monkey 6: no file, 1000x700", 300, 16 + r * 100, Vec2::new(1000.0, 700.0), &all_actions);
    }

    // ---- a damaged file must not close the program
    let bad = root.join("bad.ams");
    w(&bad, &[1, 2, 3, 4, 5]);
    h.step("open a damaged file", |a, _| a.open_path(bad.clone()));
    let empty = root.join("empty.ams");
    w(&empty, &[]);
    h.step("open an empty file", |a, _| a.open_path(empty.clone()));
    let badran = root.join("bad.ran");
    w(&badran, &[0u8; 100]);
    h.step("open a damaged .ran file", |a, _| a.open_path(badran.clone()));
    h.step("a panic inside a step is caught and the program goes on", |a, _| {
        a.crash.last = "selftest panic".into();
        let _ = std::panic::catch_unwind(|| panic!("deliberate selftest panic"));
    });
    h.check("the panic hook logged it", std::fs::read_to_string(crate::crash::log_path()).map_or(false, |t| t.contains("deliberate selftest panic")), format!("no entry in {}", crate::crash::log_path().display()));
    h.frames(3, big);

    let summary = format!("\n{} steps, {} failed\n", h.steps, h.fails);
    h.report.push_str(&summary);
    let _ = std::fs::write(out, &h.report);
    let _ = std::fs::remove_dir_all(&root);
    h.fails
}
