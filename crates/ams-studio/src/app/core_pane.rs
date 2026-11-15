//! View > Enable core viewer: a pane above the plots with the cube photos on the core. After the option is ticked a
//! window asks for the folder; the photos are searched for below it, however many folders down they are (a folder whose
//! name contains a string, `cube_crops` by default, with cube photos `<ID>_<letter><slice>.png` in it). The photos are
//! decoded into memory by background threads and appear in their places as they arrive; nothing is written to disk.

use super::{info_icon, tools_ui, App, W_COREVIEW};
use crate::core_view::{self, Cube};
use egui::{Align, Color32, ColorImage, Layout, Pos2, Rect, RichText, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

/// Longest side of a decoded photo in memory; the crops are about 1000 px.
const TEX_PX: u32 = 256;
const DEFAULT_NEEDLE: &str = "cube_crops";

/// A folder with cube photos found below the folder the user pointed at.
#[derive(Clone)]
pub(super) struct Found {
    pub dir: PathBuf,
    /// Path below the search folder.
    pub rel: String,
    /// Cube photos in it.
    pub n: usize,
    /// The specimen id the photos are named after (the part before `_<letter><slice>`), when they agree.
    pub id: String,
}

type ScanResult = (Vec<Found>, Vec<Found>, Option<String>);

pub(super) struct CoreViewState {
    pub on: bool,
    /// The folder the search starts in (what the user picked), and the string the cube folder's name contains.
    pub root: Option<PathBuf>,
    pub needle: String,
    /// The cube photo folder in use.
    pub dir: Option<PathBuf>,
    pub cubes: Vec<Cube>,
    photos: BTreeMap<(u32, char), PathBuf>,
    textures: HashMap<(u32, char), TextureHandle>,
    pub view: core_view::View,
    specimen: String,
    /// The file the cubes were laid out for, and its number of records (a change redoes the layout).
    for_file: Option<(PathBuf, usize)>,
    rx: Option<Receiver<((u32, char), ColorImage)>>,
    total: usize,
    loaded: usize,
    // the window
    pub win: bool,
    scan: Option<Receiver<ScanResult>>,
    /// The search was started without the window (a new specimen): pick the best folder when it ends.
    auto: bool,
    found: Vec<Found>,
    by_content: Vec<Found>,
    suggestion: Option<String>,
    pick: usize,
    pub note: String,
    /// Height of the pane as a fraction of the plot area.
    pub frac: f32,
    pub export: bool,
    /// What the pointer was over last (the pane's title).
    pub hover: String,
}

impl Default for CoreViewState {
    fn default() -> Self {
        CoreViewState {
            on: false,
            root: None,
            needle: DEFAULT_NEEDLE.to_string(),
            dir: None,
            cubes: vec![],
            photos: BTreeMap::new(),
            textures: HashMap::new(),
            view: core_view::View::default(),
            specimen: String::new(),
            for_file: None,
            rx: None,
            total: 0,
            loaded: 0,
            win: false,
            scan: None,
            auto: false,
            found: vec![],
            by_content: vec![],
            suggestion: None,
            pick: 0,
            note: String::new(),
            frac: 0.28,
            export: true,
            hover: String::new(),
        }
    }
}

/// `(letter, slice)` of a photo name like `DC26AB002_A12` (the stem), with the part before it.
fn parse_photo(stem: &str) -> Option<(String, char, u32)> {
    let (id, tail) = stem.rsplit_once('_')?;
    let mut ch = tail.chars();
    let letter = ch.next()?.to_ascii_uppercase();
    let digits: String = ch.collect();
    if !matches!(letter, 'A'..='D') || digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((id.to_string(), letter, digits.parse().ok()?))
}

/// The cube photos directly in `dir`: (specimen id, letter, slice, path).
fn photos_in(dir: &Path) -> Vec<(String, char, u32, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if !p.is_file() || !p.extension().map_or(false, |x| x.eq_ignore_ascii_case("png")) {
                return None;
            }
            let stem = p.file_stem()?.to_string_lossy().to_string();
            let (id, l, s) = parse_photo(&stem)?;
            Some((id, l, s, p))
        })
        .collect()
}

fn most_common(ids: impl Iterator<Item = String>) -> String {
    let mut c: HashMap<String, usize> = HashMap::new();
    for i in ids {
        *c.entry(i).or_default() += 1;
    }
    c.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map(|x| x.0).unwrap_or_default()
}

/// Walks below `root` (any depth up to 14, hidden folders skipped). Returns the folders whose name contains `needle`
/// and that hold cube photos, every folder that holds at least three cube photos, and a string to suggest when no
/// name contains `needle`: what the names of those folders have in common once the specimen id is taken off.
fn scan(root: &Path, needle: &str) -> ScanResult {
    let needle = needle.trim().to_lowercase();
    let (mut named, mut content) = (vec![], vec![]);
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        visited += 1;
        if visited > 60_000 {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut n = 0usize;
        let mut ids: Vec<String> = vec![];
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if !name.starts_with('.') && depth < 14 {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().map_or(false, |x| x.eq_ignore_ascii_case("png")) {
                if let Some((id, _, _)) = p.file_stem().and_then(|s| parse_photo(&s.to_string_lossy())) {
                    n += 1;
                    ids.push(id);
                }
            }
        }
        if n >= 1 {
            let rel = dir.strip_prefix(root).map(|r| r.display().to_string()).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| ".".into());
            let f = Found { dir: dir.clone(), rel, n, id: most_common(ids.into_iter()) };
            let dir_name = dir.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !needle.is_empty() && dir_name.contains(&needle) {
                named.push(f.clone());
            }
            if n >= 3 {
                content.push(f);
            }
        }
    }
    named.sort_by(|a, b| a.rel.cmp(&b.rel));
    content.sort_by(|a, b| a.rel.cmp(&b.rel));
    // a string to suggest: the folder names without the specimen id in front of them
    let mut stripped: Vec<String> = vec![];
    for f in &content {
        let name = f.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let s = name.strip_prefix(&f.id).unwrap_or(&name).trim_start_matches('_').to_string();
        if !s.is_empty() {
            stripped.push(s);
        }
    }
    let suggestion = if stripped.is_empty() { None } else { Some(most_common(stripped.into_iter())) };
    (named, content, suggestion)
}

/// The specimen id of a file: its name up to the first `_` (`DC26AB002_use.ams` -> `DC26AB002`).
fn spec_id(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    stem.split('_').next().unwrap_or("").to_string()
}

impl CoreViewState {
    /// The best folder for a specimen: the one whose photos are named after it, then one whose folder name has the id.
    fn best(found: &[Found], id: &str) -> usize {
        let id_l = id.to_lowercase();
        found
            .iter()
            .position(|f| !id_l.is_empty() && f.id.to_lowercase() == id_l)
            .or_else(|| found.iter().position(|f| !id_l.is_empty() && f.dir.display().to_string().to_lowercase().contains(&id_l)))
            .unwrap_or(0)
    }
}

impl App {
    /// Starts the search for the cube folder below the saved root (in the background).
    fn cv_start_scan(&mut self, auto: bool) {
        let Some(root) = self.cv.root.clone() else { return };
        let needle = self.cv.needle.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(scan(&root, &needle));
        });
        self.cv.scan = Some(rx);
        self.cv.auto = auto;
        self.cv.found.clear();
        self.cv.by_content.clear();
        self.cv.suggestion = None;
    }

    /// View > Enable core viewer: ask for the folder (the search starts at once when a folder was chosen before).
    pub(super) fn cv_toggle(&mut self) {
        if self.cv.on {
            self.cv.on = false;
            self.cv.textures.clear();
            self.cv.rx = None;
            self.cv.win = false;
            self.save_state();
            return;
        }
        self.cv.on = true;
        self.cv.note.clear();
        self.cv.dir = None;
        self.cv.for_file = None;
        self.open_win(W_COREVIEW);
        if self.cv.root.is_none() {
            // start where the open file is: its folder and up
            self.cv.root = self.doc.as_ref().filter(|d| !d.synthetic).and_then(|d| d.path.parent().map(|p| p.to_path_buf()));
        }
        self.cv_start_scan(false);
        self.save_state();
    }

    /// Decodes the photos of the folder in the background and lays the cubes out for the open file.
    fn cv_load(&mut self, dir: PathBuf) {
        let id = self.doc.as_ref().map(|d| spec_id(&d.path)).unwrap_or_default();
        let all = photos_in(&dir);
        // only the photos named after this specimen when there are any (a folder may hold several)
        let mine: Vec<_> = if all.iter().any(|p| p.0.eq_ignore_ascii_case(&id)) { all.iter().filter(|p| p.0.eq_ignore_ascii_case(&id)).collect() } else { all.iter().collect() };
        self.cv.specimen = mine.first().map(|p| p.0.clone()).unwrap_or(id);
        self.cv.photos = mine.iter().map(|p| ((p.2, p.1), p.3.clone())).collect();
        self.cv.dir = Some(dir);
        self.cv.textures.clear();
        self.cv.for_file = None;
        self.cv_layout();
        // the workers: four threads take the photos in turn and send them as small pictures
        let specimen = self.cv.specimen.clone();
        let items: Vec<((u32, char), PathBuf, i32)> = self.cv.cubes.iter().filter_map(|c| c.photo.clone().map(|p| ((c.slice, c.letter), p, core_view::photo_turn(&specimen, c.letter, c.slice)))).collect();
        self.cv.total = items.len();
        self.cv.loaded = 0;
        let (tx, rx) = channel();
        self.cv.rx = Some(rx);
        let items = Arc::new(items);
        let next = Arc::new(AtomicUsize::new(0));
        for _ in 0..4 {
            let (items, next, tx) = (items.clone(), next.clone(), tx.clone());
            std::thread::spawn(move || loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some((key, path, turn)) = items.get(i) else { break };
                let Ok(img) = image::open(path) else { continue };
                let mut rgba = img.thumbnail(TEX_PX, TEX_PX).to_rgba8();
                rgba = match turn.rem_euclid(360) {
                    90 => image::imageops::rotate270(&rgba),
                    180 => image::imageops::rotate180(&rgba),
                    270 => image::imageops::rotate90(&rgba),
                    _ => rgba,
                };
                let ci = ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                if tx.send((*key, ci)).is_err() {
                    break;
                }
            });
        }
    }

    /// The layout of the cubes for the open file (names of its records, the photos found).
    fn cv_layout(&mut self) {
        let Some(d) = &self.doc else { return };
        let names: Vec<String> = d.specs.iter().map(|s| s.name.clone()).collect();
        self.cv.cubes = core_view::build(&self.cv.specimen, &names, &self.cv.photos);
        self.cv.for_file = Some((d.path.clone(), d.specs.len()));
    }

    /// Every frame: new photos become textures, a new file gets its layout (and its own cube folder), the search ends.
    pub(super) fn cv_poll(&mut self, ctx: &egui::Context) {
        if !self.cv.on {
            return;
        }
        if let Some(rx) = &self.cv.rx {
            let mut n = 0;
            while n < 6 {
                match rx.try_recv() {
                    Ok((key, img)) => {
                        let t = ctx.load_texture(format!("cube-{}{}", key.1, key.0), img, TextureOptions::LINEAR);
                        self.cv.textures.insert(key, t);
                        self.cv.loaded += 1;
                        n += 1;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        ctx.request_repaint_after(std::time::Duration::from_millis(40));
                        break;
                    }
                    Err(_) => {
                        self.cv.rx = None;
                        break;
                    }
                }
            }
            if n == 6 {
                ctx.request_repaint();
            }
        }
        // the search finished
        if let Some(rx) = &self.cv.scan {
            match rx.try_recv() {
                Ok((named, content, sugg)) => {
                    self.cv.scan = None;
                    let id = self.doc.as_ref().map(|d| spec_id(&d.path)).unwrap_or_default();
                    self.cv.suggestion = sugg;
                    self.cv.found = if named.is_empty() { vec![] } else { named };
                    self.cv.by_content = content;
                    self.cv.pick = CoreViewState::best(&self.cv.found, &id);
                    if self.cv.auto {
                        self.cv.auto = false;
                        match self.cv.found.get(self.cv.pick).cloned() {
                            Some(f) if !id.is_empty() && f.id.eq_ignore_ascii_case(&id) => self.cv_load(f.dir),
                            _ => {
                                self.cv.note = format!("No cube photos named after {id} were found below the chosen folder.");
                                self.cv.win = true;
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(80)),
                Err(_) => self.cv.scan = None,
            }
        }
        // another file: lay the cubes out again; its cube folder is searched below the same root
        let cur = self.doc.as_ref().filter(|d| !d.synthetic).map(|d| (d.path.clone(), d.specs.len()));
        if self.cv.dir.is_some() && cur.is_some() && cur != self.cv.for_file {
            let new_file = cur.as_ref().map(|c| &c.0) != self.cv.for_file.as_ref().map(|c| &c.0);
            let id = cur.as_ref().map(|c| spec_id(&c.0)).unwrap_or_default();
            if new_file && !id.eq_ignore_ascii_case(&self.cv.specimen) && self.cv.root.is_some() && self.cv.scan.is_none() && !self.cv.win {
                self.cv.for_file = cur;
                self.cv_start_scan(true);
            } else {
                self.cv_layout();
            }
        }
    }

    /// The window that asks for the folder with the cube photos.
    pub(super) fn cv_window(&mut self, ctx: &egui::Context) {
        if !self.cv.win {
            return;
        }
        let dark = self.dark;
        let id = self.doc.as_ref().map(|d| spec_id(&d.path)).unwrap_or_default();
        let (mut open, mut cancel, mut load, mut choose, mut search) = (true, false, false, false, false);
        let mut use_sugg = false;
        let scr = ctx.content_rect();
        let size = Vec2::new((scr.width() - 40.0).clamp(560.0, 820.0), (scr.height() - 60.0).clamp(480.0, 720.0));
        let scanning = self.cv.scan.is_some();
        let mut pick = self.cv.pick;
        let mut needle = self.cv.needle.clone();
        self.base_window(ctx, "Core viewer", W_COREVIEW, size).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.2);
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 14.0);
            ui.spacing_mut().button_padding = Vec2::new(12.0, 6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Which folder holds the cube photos?").strong().size(19.0));
                info_icon(ui, "Pick a folder; the cube photos are searched for below it, however many folders down they are. A cube folder is a folder whose name contains the string below (cube_crops by default) and that holds photos named like DC26AB002_A12.png (specimen id, cube letter, slice). If no folder name contains the string, the folders that hold cube photos are listed and a string is suggested. The photos are only read: they are decoded into memory, nothing is written.");
            });
            ui.horizontal(|ui| {
                choose = ui.add(egui::Button::new("Choose folder ...").min_size(Vec2::new(170.0, 34.0))).clicked();
                ui.label(RichText::new(self.cv.root.as_ref().map(|r| r.display().to_string()).unwrap_or_else(|| "no folder chosen yet".into())).monospace().small());
            });
            ui.label(RichText::new("The name of the cube folder contains:").strong());
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut needle).desired_width(240.0).hint_text(DEFAULT_NEEDLE).margin(egui::Margin::symmetric(8, 6)));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                search = ui.add_enabled(self.cv.root.is_some(), egui::Button::new(RichText::new("Search").strong()).min_size(Vec2::new(100.0, 34.0))).clicked() || enter;
            });
            if scanning {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Looking for cube photos below the folder ...");
                });
            } else if self.cv.root.is_some() {
                let (list, from_content) = if self.cv.found.is_empty() { (&self.cv.by_content, true) } else { (&self.cv.found, false) };
                if list.is_empty() {
                    ui.label(RichText::new("No folder with cube photos was found below this folder.").color(Color32::from_rgb(214, 90, 90)).strong());
                    ui.label(RichText::new("The photos are named like DC26AB002_A12.png. Choose a folder above the cube folder.").weak().small());
                } else {
                    if from_content {
                        ui.label(RichText::new(format!("No folder name contains \u{201c}{}\u{201d}. These folders hold cube photos:", needle.trim())).color(Color32::from_rgb(214, 150, 40)).strong());
                        if let Some(s) = &self.cv.suggestion {
                            if use_sugg_button(ui, s) {
                                use_sugg = true;
                            }
                        }
                    } else {
                        ui.label(RichText::new(format!("Cube folders found (n={})", list.len())).strong());
                    }
                    egui::ScrollArea::vertical().max_height((ui.available_height() - 90.0).max(120.0)).auto_shrink([false, false]).show(ui, |ui| {
                        for (k, f) in list.iter().enumerate() {
                            let mine = !id.is_empty() && f.id.eq_ignore_ascii_case(&id);
                            ui.horizontal_wrapped(|ui| {
                                ui.radio_value(&mut pick, k, RichText::new(&f.rel).size(15.0));
                                ui.label(RichText::new(format!("(n={} photos of {})", f.n, f.id)).weak().small());
                                if mine {
                                    ui.label(RichText::new("matches the open file").strong().small());
                                }
                            });
                        }
                    });
                }
            }
            if !self.cv.note.is_empty() {
                ui.label(RichText::new(&self.cv.note).color(Color32::from_rgb(214, 150, 40)));
            }
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                cancel = ui.add(egui::Button::new("Cancel").min_size(Vec2::new(0.0, 38.0))).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let n = if self.cv.found.is_empty() { self.cv.by_content.len() } else { self.cv.found.len() };
                    let go = egui::Button::new(RichText::new("Load the cube photos").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(210.0, 38.0));
                    load = ui.add_enabled(n > 0 && !scanning, go).clicked();
                });
            });
        });
        self.cv.pick = pick;
        if use_sugg {
            if let Some(s) = self.cv.suggestion.clone() {
                needle = s;
                search = true;
            }
        }
        self.cv.needle = needle;
        if choose {
            let mut dlg = rfd::FileDialog::new().set_title("Folder that holds (or is above) the cube photos");
            if let Some(r) = self.cv.root.as_ref().filter(|r| r.is_dir()) {
                dlg = dlg.set_directory(r);
            }
            // headless test hook: AMS_STUDIO_PICK_DIR names the folder instead of the dialog
            let picked = std::env::var_os("AMS_STUDIO_PICK_DIR").map(PathBuf::from).or_else(|| dlg.pick_folder());
            if let Some(p) = picked {
                self.cv.root = Some(p);
                search = true;
            }
        }
        if search {
            self.cv.note.clear();
            self.cv_start_scan(false);
        }
        if load {
            let list = if self.cv.found.is_empty() { self.cv.by_content.clone() } else { self.cv.found.clone() };
            if let Some(f) = list.get(self.cv.pick).cloned() {
                self.cv_load(f.dir);
                self.cv.win = false;
                self.save_state();
                return;
            }
        }
        if cancel || !open {
            self.cv.win = false;
            // nothing was loaded: the viewer stays off
            if self.cv.dir.is_none() {
                self.cv.on = false;
                self.save_state();
            }
        }
    }

    /// The pane itself, above the plots.
    pub(super) fn cv_pane(&mut self, ui: &mut Ui, rect: Rect, th: &crate::plots::Theme) -> crate::plots::PlotOut {
        let visuals = ui.visuals().clone();
        ui.painter().rect(rect, 8.0, if th.dark { th.canvas() } else { visuals.extreme_bg_color }, Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
        let header = Rect::from_min_size(rect.min + Vec2::new(6.0, 4.0), Vec2::new(rect.width() - 12.0, 24.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(header).layout(Layout::left_to_right(Align::Center)));
        let (mut change, mut reset, mut close) = (false, false, false);
        child.menu_button("\u{25b8}", |ui| {
            if ui.button("Choose another folder ...").clicked() {
                change = true;
                ui.close();
            }
            if ui.button("Reset zoom").clicked() {
                reset = true;
                ui.close();
            }
            if ui.button("Close the core viewer").clicked() {
                close = true;
                ui.close();
            }
        });
        let title = if self.cv.hover.is_empty() { format!("Core viewer {}", self.cv.specimen) } else { self.cv.hover.clone() };
        let loading = self.cv.total > 0 && self.cv.loaded < self.cv.total;
        let right = if loading { format!("loading photos (n={} of {})", self.cv.loaded, self.cv.total) } else { format!("cubes (n={}), photos (n={})", self.cv.cubes.len(), self.cv.photos.len()) };
        child.add_sized([(header.width() - 34.0 - 190.0).max(40.0), 18.0], egui::Label::new(if self.cv.hover.is_empty() { RichText::new(title).weak() } else { RichText::new(title) }).truncate());
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(right).weak().small());
        });
        if change {
            self.cv.win = true;
            self.open_win(W_COREVIEW);
            self.cv_start_scan(false);
        }
        if reset {
            self.cv.view.reset();
        }
        if close {
            self.cv_toggle();
        }
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 4.0, header.max.y + 2.0), Pos2::new(rect.max.x - 4.0, rect.max.y - 4.0));
        let Some(d) = &self.doc else { return Default::default() };
        let out = core_view::draw(ui, body, &self.cv.cubes, &self.cv.textures, &d.selected, &d.active, &mut self.cv.view, th, &self.cv.specimen);
        self.cv.hover = out.hover.clone().unwrap_or_default();
        out
    }
}

fn use_sugg_button(ui: &mut Ui, s: &str) -> bool {
    ui.add(egui::Button::new(format!("Search for \u{201c}{s}\u{201d} instead")).min_size(Vec2::new(0.0, 32.0))).clicked()
}
