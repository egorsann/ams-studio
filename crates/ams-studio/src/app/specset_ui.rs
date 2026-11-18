//! "Open Folder of Specimens": a parent folder with one sub-folder per specimen. The identifier of the main
//! `.ams` file per specimen (the author's end in `_use`) is detected, shown as a sentence with the files it
//! picks, and can be replaced by a string the user types. See `ams_core::specset`.

use super::{App, W_SET};
use ams_core::specset::{self, Built, Detection, IdScheme, Kind, Layout as FileLayout, Phases, Pick, RawScan, SpecFolder};
use egui::{Align, Color32, Layout, RichText, Sense, Ui, Vec2};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

/// What the user said about how the files are stored (the questions before the list of main files).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum SchemeChoice {
    Auto,
    Digits(usize),
    FirstWord,
    Chars(usize),
}

impl SchemeChoice {
    pub fn scheme(self) -> Option<IdScheme> {
        match self {
            SchemeChoice::Auto => None,
            SchemeChoice::Digits(n) => Some(IdScheme::Digits(n)),
            SchemeChoice::FirstWord => Some(IdScheme::FirstWord),
            SchemeChoice::Chars(n) => Some(IdScheme::Chars(n)),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct SetOpts {
    pub kind: Kind,
    pub layout: FileLayout,
    pub phases: Phases,
    pub scheme: SchemeChoice,
}

impl Default for SetOpts {
    fn default() -> Self {
        SetOpts { kind: Kind::Ams, layout: FileLayout::Auto, phases: Phases::ByName, scheme: SchemeChoice::Auto }
    }
}

impl SetOpts {
    pub fn to_text(&self) -> String {
        let k = match self.kind { Kind::Ams => "ams", Kind::Ran => "ran" };
        let l = match self.layout { FileLayout::Auto => "auto", FileLayout::OwnFolders => "own", FileLayout::OneFolder => "one" };
        let p = match self.phases { Phases::Off => "off", Phases::ByName => "name", Phases::Yes => "yes" };
        let s = match self.scheme {
            SchemeChoice::Auto => "auto".to_string(),
            SchemeChoice::Digits(n) => format!("d{n}"),
            SchemeChoice::FirstWord => "first".to_string(),
            SchemeChoice::Chars(n) => format!("c{n}"),
        };
        format!("{k}|{l}|{p}|{s}")
    }

    pub fn from_text(t: &str) -> Option<SetOpts> {
        let v: Vec<&str> = t.trim().split('|').collect();
        if v.len() != 4 {
            return None;
        }
        Some(SetOpts {
            kind: if v[0] == "ran" { Kind::Ran } else { Kind::Ams },
            layout: match v[1] { "own" => FileLayout::OwnFolders, "one" => FileLayout::OneFolder, _ => FileLayout::Auto },
            phases: match v[2] { "off" => Phases::Off, "yes" => Phases::Yes, _ => Phases::ByName },
            scheme: match v[3] {
                "first" => SchemeChoice::FirstWord,
                s if s.starts_with('d') => s[1..].parse().map(SchemeChoice::Digits).unwrap_or(SchemeChoice::Auto),
                s if s.starts_with('c') => s[1..].parse().map(SchemeChoice::Chars).unwrap_or(SchemeChoice::Auto),
                _ => SchemeChoice::Auto,
            },
        })
    }

    /// The specimens these answers give for a scan.
    pub fn build(&self, root: &Path, raw: &RawScan) -> Built {
        let mut b = specset::build(root, raw, self.kind, self.layout, self.scheme.scheme(), self.phases);
        if self.phases == Phases::Yes {
            // two files per specimen and no phase word in the names: the one with the small Km is the out-of-phase one
            specset::pair_two_by_content(&mut b.folders, &|p: &Path| {
                let bytes = std::fs::read(p).ok()?;
                if crate::doc::is_ran(p) {
                    ams_core::import::ran_to_records(&bytes).ok()
                } else {
                    Some(bytes)
                }
            });
        }
        b
    }
}

/// The confirmed set shown in the file browser.
pub(super) struct SpecSet {
    pub root: PathBuf,
    pub raw: RawScan,
    pub opts: SetOpts,
    pub folders: Vec<SpecFolder>,
    /// The identifier the user confirmed (empty when every folder holds a single file).
    pub ident: String,
    pub picks: Vec<Pick>,
}

impl SpecSet {
    /// Main files in folder order (specimens without a unique match are left out).
    pub fn mains(&self) -> Vec<PathBuf> {
        self.folders.iter().zip(&self.picks).filter_map(|(f, p)| p.file().map(|i| f.dir.join(&f.files[i]))).collect()
    }
}

#[derive(Default)]
pub(super) struct SetDialog {
    pub scanning: bool,
    pub root: PathBuf,
    pub folders: Vec<SpecFolder>,
    pub detection: Option<Detection>,
    /// The string whose matches are listed ("" = none).
    pub ident: String,
    /// The user has asked to search for another string.
    pub custom: bool,
    pub input: String,
    pub picks: Vec<Pick>,
    pub rx: Option<Receiver<RawScan>>,
    /// Every .ams / .ran file found under the folder, before the questions are answered.
    pub raw: RawScan,
    /// How the files are stored (answers to the first questions).
    pub opts: SetOpts,
    /// The questions are showing (first step); off = the list of main files.
    pub ask: bool,
    /// What the answers gave: shared folders split by ID, and the scheme that did it.
    pub split_dirs: usize,
    pub split_scheme: Option<IdScheme>,
    pub n_pairs: usize,
    /// The string was typed by the user, not detected.
    pub typed: bool,
    /// The scan is the silent restore of the folder that was open last time.
    pub restore: bool,
}

impl SetDialog {
    fn rematch(&mut self) {
        self.picks = specset::matches(&self.folders, &self.ident);
    }

    /// Builds the specimens from the answers, finds the identifier of the main file and matches it.
    pub fn rebuild(&mut self, hint: Option<&str>) {
        let b = self.opts.build(&self.root, &self.raw);
        self.split_dirs = b.split_dirs;
        self.split_scheme = b.scheme;
        self.n_pairs = b.folders.iter().map(|f| f.pairs.len()).sum();
        let det = specset::detect(&b.folders, hint);
        self.ident = det.ident.clone().unwrap_or_default();
        self.input = self.ident.clone();
        self.typed = false;
        self.folders = b.folders;
        self.detection = Some(det);
        self.rematch();
    }
}

fn file_list(f: &SpecFolder, idx: &[usize]) -> String {
    idx.iter().map(|&i| f.files[i].as_str()).collect::<Vec<_>>().join(", ")
}

impl App {
    /// Asks for the parent folder and starts scanning it.
    pub(super) fn open_folder_of_specimens(&mut self, ctx: &egui::Context) {
        let mut dlg = crate::fdialog::FileDialog::new().set_title("Open Folder of Specimens (the parent of the specimen folders)");
        if self.browser_dir.is_dir() {
            dlg = dlg.set_directory(&self.browser_dir);
        }
        let Some(root) = dlg.pick_folder() else { return };
        self.begin_scan(ctx, root);
    }

    pub(super) fn begin_scan(&mut self, ctx: &egui::Context, root: PathBuf) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (r2, c2) = (root.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(specset::scan_raw(&r2));
            c2.request_repaint();
        });
        self.sd = SetDialog { scanning: true, root, rx: Some(rx), opts: self.set_opts, ask: true, ..Default::default() };
        self.open_win(W_SET);
    }

    /// Opens the folder set that was open when the program was closed, without asking: the folders are scanned in
    /// the background and the identifier used last is applied. The dialog only appears if nothing matches.
    pub(super) fn restore_set(&mut self, ctx: &egui::Context, root: PathBuf, file: Option<PathBuf>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (r2, c2) = (root.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(specset::scan_raw(&r2));
            c2.request_repaint();
        });
        self.sd = SetDialog { scanning: true, root, rx: Some(rx), restore: true, opts: self.set_opts, ..Default::default() };
        self.restore_file = file;
    }

    /// Called every frame: finishes the silent restore when the scan is done.
    pub(super) fn poll_restore(&mut self, ctx: &egui::Context) {
        if !self.sd.restore {
            return;
        }
        let Some(rx) = &self.sd.rx else { return };
        let raw = match rx.try_recv() {
            Ok(f) => f,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
                return;
            }
            Err(_) => {
                self.sd = SetDialog::default();
                return;
            }
        };
        let root = self.sd.root.clone();
        let opts = self.set_opts;
        self.sd = SetDialog::default();
        let built = opts.build(&root, &raw);
        let folders = built.folders;
        if folders.is_empty() {
            return; // the folder is gone or empty: start as usual
        }
        let det = specset::detect(&folders, Some(&self.last_ident));
        let ident = det.ident.clone().unwrap_or_default();
        let picks = specset::matches(&folders, &ident);
        let matched = picks.iter().filter(|p| p.file().is_some()).count();
        if matched == 0 {
            // nothing to go by: let the user choose, as when opening the folder by hand
            self.sd = SetDialog { root, ident, picks, detection: Some(det), folders, raw, opts, ..Default::default() };
            self.open_win(W_SET);
            return;
        }
        let set = SpecSet { root: root.clone(), raw, opts, folders, ident, picks };
        let mains = set.mains();
        let file = self.restore_file.take().filter(|p| p.is_file()).or_else(|| mains.first().cloned());
        self.browser_dir = root;
        self.set = Some(set);
        if let Some(p) = file {
            self.open_path(p);
        }
        self.save_state();
    }

    /// Reads the folders of the open set again (after files were moved or written) with the same identifier.
    pub(super) fn refresh_set(&mut self) {
        if let Some(s) = &mut self.set {
            s.raw = specset::scan_raw(&s.root);
            s.folders = s.opts.build(&s.root, &s.raw).folders;
            s.picks = specset::matches(&s.folders, &s.ident);
        }
    }

    /// Re-opens the dialog on the folders of the current set, to try another string.
    pub(super) fn change_set_string(&mut self) {
        let Some(s) = &self.set else { return };
        let det = specset::detect(&s.folders, Some(&self.last_ident));
        self.sd = SetDialog {
            root: s.root.clone(),
            raw: s.raw.clone(),
            opts: s.opts,
            folders: s.folders.clone(),
            detection: Some(det),
            ident: s.ident.clone(),
            custom: true,
            input: s.ident.clone(),
            typed: true,
            ..Default::default()
        };
        self.sd.rematch();
        self.open_win(W_SET);
    }

    /// The out-of-phase file of a specimen whose phases are two files, as the open set found them.
    pub(super) fn partner_in_set(&self, p: &Path) -> Option<PathBuf> {
        let s = self.set.as_ref()?;
        s.folders.iter().find_map(|f| f.pairs.iter().find(|(m, _)| f.dir.join(m) == p).map(|(_, o)| f.dir.join(o)))
    }

    /// Previous / next main file, position and count: through the set when the file belongs to it.
    pub(super) fn neighbours_of(&self, path: &Path) -> (Option<PathBuf>, Option<PathBuf>, usize, usize) {
        if let Some(s) = &self.set {
            let mains = s.mains();
            if let Some(i) = mains.iter().position(|p| p == path) {
                return (i.checked_sub(1).map(|j| mains[j].clone()), mains.get(i + 1).cloned(), i + 1, mains.len());
            }
        }
        crate::doc::neighbours(path)
    }

    pub(super) fn specset_window(&mut self, ctx: &egui::Context) {
        if !self.win_set {
            return;
        }
        let mut sd = std::mem::take(&mut self.sd);
        // scan result arrives from the thread
        if let Some(rx) = &sd.rx {
            match rx.try_recv() {
                Ok(raw) => {
                    sd.rx = None;
                    sd.scanning = false;
                    // the questions start from what the folder holds: .ams unless there are only .ran files
                    let (n_ams, n_ran) = raw.counts();
                    if n_ams == 0 && n_ran > 0 {
                        sd.opts.kind = Kind::Ran;
                    } else if n_ran == 0 {
                        sd.opts.kind = Kind::Ams;
                    }
                    sd.raw = raw;
                    sd.rebuild(Some(&self.last_ident));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(120));
                }
                Err(_) => {
                    sd.rx = None;
                    sd.scanning = false;
                }
            }
        }
        let dark = self.dark;
        let hint = self.last_ident.clone();
        let warn = Color32::from_rgb(214, 150, 40);
        let bad = Color32::from_rgb(214, 90, 90);
        let ok = Color32::from_rgb(70, 170, 90);
        let (mut open, mut use_it, mut cancel, mut match_now, mut back_to_detected, mut toggle_custom) = (true, false, false, false, false, false);
        let scr = ctx.content_rect();
        let size = Vec2::new((scr.width() - 24.0).clamp(640.0, 900.0), (scr.height() - 56.0).clamp(560.0, 780.0));
        self.base_window(ctx, "Open Folder of Specimens", W_SET, size).max_size(Vec2::new((scr.width() - 16.0).max(420.0), (scr.height() - 48.0).max(360.0))).open(&mut open).resizable(true).show(ctx, |ui| {
            super::tools_ui::pop_style(ui, 1.2);
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 14.0);
            ui.spacing_mut().button_padding = Vec2::new(12.0, 6.0);
            ui.label(RichText::new(sd.root.display().to_string()).weak().small());
            if sd.scanning {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Looking for .ams files in the sub-folders ...");
                });
                return;
            }
            if sd.raw.dirs.is_empty() && sd.folders.is_empty() {
                ui.label(RichText::new("No .ams or .ran files were found under this folder.").color(bad).strong().size(18.0));
                ui.label("Pick the folder that holds your specimens: either one sub-folder per specimen, or all the data files next to each other.");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| cancel = ui.button("Close").clicked());
                return;
            }
            if sd.ask {
                let (go, no) = ask_ui(ui, &mut sd, dark, &hint);
                cancel = no;
                if go {
                    sd.ask = false;
                    sd.rebuild(Some(&hint));
                }
                return;
            }
            if sd.folders.is_empty() {
                ui.label(RichText::new("With these answers no specimens were found.").color(bad).strong().size(18.0));
                ui.label("Go back and try another file type or another way the files are stored.");
                ui.horizontal(|ui| {
                    if ui.button("Back to the questions").clicked() {
                        sd.ask = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| cancel = ui.button("Close").clicked());
                });
                return;
            }
            let n_files: usize = sd.folders.iter().map(|f| f.files.len()).sum();
            let det = sd.detection.clone().unwrap_or(Detection { ident: None, multi: 0, covered: 0, needed: false });
            let matched = sd.picks.iter().filter(|p| p.file().is_some()).count();

            // ---- the question first, what the program found under it
            ui.horizontal(|ui| {
                ui.label(RichText::new("Which file is the main file of each specimen?").strong().size(19.0));
                super::info_icon(ui, &format!(
                    "Every specimen folder can hold several .ams files; the main one is found by a string its name contains (for example _use). The program looks for a string the main files share.\nUpper and lower case do not matter, and a folder with a single .ams file always gives that file.\nCheck the list below that every specimen shows the file you consider its main file.{}",
                    if det.needed && !sd.ident.trim().is_empty() && !sd.typed { format!("\nFound in {} of the {} folders that hold more than one file.", det.covered, det.multi) } else { String::new() }
                ));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("Specimens (n={}) with .{} files (n={n_files})", sd.folders.len(), sd.opts.kind.ext())).weak());
                if sd.split_dirs > 0 {
                    ui.label(RichText::new(format!("\u{b7} files in one folder told apart by {}", sd.split_scheme.map_or("their names".to_string(), |s| s.describe()))).weak());
                }
                if sd.n_pairs > 0 {
                    ui.label(RichText::new(format!("\u{b7} in- and out-of-phase files joined (n={})", sd.n_pairs)).weak());
                }
                if ui.small_button("Change how the files are stored ...").on_hover_text("Back to the questions: one folder or one folder per specimen, .ams or .ran, split phases").clicked() {
                    sd.ask = true;
                }
            });
            egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if !det.needed {
                    ui.label(RichText::new("Every specimen folder holds exactly one .ams file, so no identifier is needed.").strong());
                } else if sd.ident.trim().is_empty() {
                    ui.label(RichText::new("No identifier was found that the main files share.").strong().color(warn));
                    ui.label("The .ams files in the specimen folders are named differently from folder to folder, so the program cannot tell which one is the main file of each specimen. Please rename the main file of every specimen so that they share one string (for example SPECIMEN_use.ams) before using AMS Studio, or tell the program which string to look for.");
                } else if sd.typed {
                    ui.label(RichText::new(format!("You asked for \u{201c}{}\u{201d}: it picks one file in {} of {} specimens.", sd.ident.trim(), matched, sd.folders.len())).strong());
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("\u{201c}{}\u{201d}", sd.ident)).strong().size(18.0));
                        ui.label(RichText::new("is the identifier of the main .ams file in each specimen folder.").strong());
                    });
                }
                ui.add_space(4.0);
                toggle_custom = ui.add(egui::Button::new(if sd.custom { "Hide the other-string search" } else { "Search for another string ..." }).min_size(Vec2::new(240.0, 34.0))).on_hover_text("Type a part of the file name that marks the main file, and see which files it picks").clicked();
                if sd.custom {
                    // the prompt above its field
                    ui.label(RichText::new("The main file's name contains:").strong());
                    ui.horizontal(|ui| {
                        let r = ui.add(egui::TextEdit::singleline(&mut sd.input).desired_width(260.0).hint_text("e.g. _final").margin(egui::Margin::symmetric(8, 6)));
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        match_now = ui.add(egui::Button::new(RichText::new("Match").strong()).min_size(Vec2::new(100.0, 34.0))).clicked() || enter;
                        if let Some(d) = &det.ident {
                            back_to_detected = ui.add(egui::Button::new(format!("Back to \u{201c}{d}\u{201d}")).min_size(Vec2::new(0.0, 34.0))).clicked();
                        }
                    });
                }
            });

            // ---- the files
            ui.horizontal(|ui| {
                ui.label(RichText::new("Files that would be opened").strong().size(17.0));
                // the one coloured figure of the window: green when every specimen has its main file, orange otherwise
                ui.label(RichText::new(format!("(n={matched} of {} matched)", sd.folders.len())).strong().color(if matched == sd.folders.len() { ok } else { warn }));
            });
            let h = (ui.available_height() - 76.0).max(140.0);
            egui::ScrollArea::both().max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("set-grid").striped(true).num_columns(4).min_col_width(60.0).spacing([20.0, 8.0]).show(ui, |ui| {
                    for hd in ["#", "Specimen folder", "Main file", "Note"] {
                        ui.label(RichText::new(hd).strong());
                    }
                    ui.end_row();
                    for (i, (f, p)) in sd.folders.iter().zip(&sd.picks).enumerate() {
                        ui.label(RichText::new((i + 1).to_string()).weak());
                        ui.label(&f.name);
                        let what = sd.ident.trim();
                        match p {
                            Pick::Only(k) => {
                                ui.label(&f.files[*k]);
                                ui.label(RichText::new("the only .ams file in this folder").weak().small());
                            }
                            Pick::Match(k) => {
                                ui.label(&f.files[*k]);
                                ui.label(RichText::new(format!("the one of {} .ams files whose name contains \u{201c}{what}\u{201d}", f.files.len())).weak().small());
                            }
                            Pick::None => {
                                ui.label(RichText::new("no main file").color(bad));
                                ui.label(RichText::new(format!("no name contains \u{201c}{what}\u{201d} (files here: {})", file_list(f, &(0..f.files.len()).collect::<Vec<_>>()))).weak().small());
                            }
                            Pick::Several(ix) => {
                                ui.label(RichText::new("several candidates").color(warn));
                                ui.label(RichText::new(format!("{} names contain \u{201c}{what}\u{201d}: {} (use a more specific string)", ix.len(), file_list(f, ix))).weak().small());
                            }
                        }
                        ui.end_row();
                    }
                });
            });
            // ---- Cancel on the left, the way forward at the bottom right
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                cancel = ui.add(egui::Button::new("Cancel").min_size(Vec2::new(0.0, 38.0))).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if matched == sd.folders.len() { "Use these files".to_string() } else { format!("Use the matched files (n={matched})") };
                    let go = egui::Button::new(RichText::new(label).strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(190.0, 38.0));
                    let r = ui.add_enabled(matched > 0, go).on_hover_text("Show the specimens in the file browser and open the first main file");
                    use_it = r.clicked();
                    if matched < sd.folders.len() {
                        super::info_icon(ui, "Specimens without a match are listed in the browser but skipped by Previous / Next.");
                    }
                });
            });
        });
        if toggle_custom {
            sd.custom = !sd.custom;
            if sd.custom && sd.input.is_empty() {
                sd.input = sd.ident.clone();
            }
        }
        if match_now {
            sd.ident = sd.input.trim().to_string();
            sd.typed = true;
            sd.rematch();
        }
        if back_to_detected {
            sd.ident = sd.detection.as_ref().and_then(|d| d.ident.clone()).unwrap_or_default();
            sd.input = sd.ident.clone();
            sd.typed = false;
            sd.rematch();
        }
        if use_it {
            let set = SpecSet { root: sd.root.clone(), raw: sd.raw.clone(), opts: sd.opts, folders: sd.folders.clone(), ident: sd.ident.trim().to_string(), picks: sd.picks.clone() };
            self.set_opts = sd.opts;
            if !set.ident.is_empty() {
                self.last_ident = set.ident.clone();
            }
            let first = set.mains().first().cloned();
            self.browser_dir = set.root.clone();
            self.set = Some(set);
            self.save_state();
            if let Some(p) = first {
                self.open_path(p);
            }
            open = false;
        }
        if cancel {
            open = false;
        }
        self.win_set = open;
        self.sd = sd;
    }

    /// The file browser while a set is active: every specimen folder with its `.ams` files, the main one in bold.
    /// Quick select: the main file (the string-matched, bold one) of every specimen folder goes into the tree selection.
    /// Folders of exported results (`_geo`, `_raw`, `_corr`, `<specimen folder>_geo`) are left out.
    /// The main file of every specimen folder of the file tree (result folders left out).
    pub(super) fn tree_specimen_files(&self) -> Vec<PathBuf> {
        let Some(set) = &self.set else { return vec![] };
        let mut v: Vec<PathBuf> = vec![];
        for (f, p) in set.folders.iter().zip(&set.picks) {
            let leaf = f.name.rsplit('/').next().unwrap_or(&f.name);
            if ams_core::specset::is_result_folder(leaf) {
                continue;
            }
            if let Some(m) = p.file() {
                v.push(f.dir.join(&f.files[m]));
            }
        }
        v
    }

    pub(super) fn select_all_matched(&mut self) {
        let v = self.tree_specimen_files();
        self.tree_anchor = v.first().cloned();
        self.tree_sel = v;
        self.last_focus = super::Focus::Files;
    }

    pub(super) fn set_browser_ui(&mut self, ui: &mut Ui) {
        let Some(set) = &self.set else { return };
        let cur = self.doc.as_ref().map(|d| d.path.clone());
        let mut open: Option<PathBuf> = None;
        let (mut change, mut close, mut export) = (false, false, false);
        ui.strong("Specimens");
        ui.horizontal(|ui| {
            change = ui.small_button("Change string ...").on_hover_text("Search for another identifier of the main file").clicked();
            close = ui.small_button("Close set").on_hover_text("Back to the plain folder browser").clicked();
        });
        ui.horizontal(|ui| {
            export = ui.small_button("Save as .ams ...").on_hover_text("Save the selected files (the open one by default) or every edited file into _geo folders").clicked();
            let n_sel = self.tree_sel.len();
            let n_rot = self.pending.len();
            ui.label(RichText::new(format!("{n_sel} selected{}", if n_rot > 0 { format!(" \u{b7} {n_rot} rotated") } else { String::new() })).small().weak());
        });
        ui.label(RichText::new(set.root.display().to_string()).small().weak());
        let matched = set.picks.iter().filter(|p| p.file().is_some()).count();
        let sentence = if set.ident.is_empty() { "one .ams file per specimen".to_string() } else { format!("main file: \u{201c}{}\u{201d}", set.ident) };
        ui.label(RichText::new(format!("{sentence}  \u{b7}  {matched} of {}", set.folders.len())).small().weak());
        ui.separator();
        let open_all = set.folders.len() <= 40;
        // the current file changed (Shift+Q / Shift+E, a click): open its folder and scroll to it once
        let moved = self.tree_cur != cur;
        let gray = ui.visuals().weak_text_color();
        let accent = if self.dark { Color32::from_rgb(106, 160, 255) } else { Color32::from_rgb(59, 130, 246) };
        let dim = ui.visuals().text_color().gamma_multiply(0.55);
        // the files in the order they are listed, for a range selection with Shift
        let flat: Vec<PathBuf> = set
            .folders
            .iter()
            .zip(&set.picks)
            .flat_map(|(f, p)| {
                let mut order: Vec<usize> = (0..f.files.len()).collect();
                if let Some(m) = p.file() {
                    order.retain(|k| *k != m);
                    order.insert(0, m);
                }
                order.into_iter().map(|k| f.dir.join(&f.files[k])).collect::<Vec<_>>()
            })
            .collect();
        // the files the identifier matched (the bold main file of each folder); the others are the grayed ones
        let main_files: std::collections::HashSet<PathBuf> = set.folders.iter().zip(&set.picks).filter_map(|(f, p)| p.file().map(|m| f.dir.join(&f.files[m]))).collect();
        let mut sel = self.tree_sel.clone();
        let mut anchor = self.tree_anchor.clone();
        let mods = ui.input(|i| i.modifiers);
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            for (fi, (f, p)) in set.folders.iter().zip(&set.picks).enumerate() {
                let main = p.file();
                let has_cur = cur.as_ref().map_or(false, |c| f.files.iter().any(|n| f.dir.join(n) == *c));
                // like TX Studio: a folder icon, and the folder name in gray so the files stand out
                let title = match p {
                    Pick::Only(_) | Pick::Match(_) => RichText::new(format!("\u{1F4C1}  {}", f.name)).color(gray),
                    Pick::None => RichText::new(format!("\u{26a0} \u{1F4C1}  {}", f.name)).color(Color32::from_rgb(214, 90, 90)),
                    Pick::Several(_) => RichText::new(format!("\u{26a0} \u{1F4C1}  {}", f.name)).color(Color32::from_rgb(214, 150, 40)),
                };
                let id = ui.make_persistent_id(("set-folder", fi, &set.root));
                let force = moved && has_cur;
                let mut ch = egui::CollapsingHeader::new(title).id_salt(id).default_open(open_all || has_cur);
                if force {
                    ch = ch.open(Some(true));
                }
                ch.show(ui, |ui| {
                    // the file the identifier picked comes first; the others follow in a darker shade
                    let mut order: Vec<usize> = (0..f.files.len()).collect();
                    if let Some(m) = main {
                        order.retain(|k| *k != m);
                        order.insert(0, m);
                    }
                    for k in order {
                        let n = &f.files[k];
                        let path = f.dir.join(n);
                        let is_main = main == Some(k);
                        let is_cur = cur.as_ref() == Some(&path);
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width().max(120.0), 22.0), Sense::click());
                        if is_cur {
                            // the active file: a clear background so it can be found at a glance
                            ui.painter().rect_filled(rect, 4.0, accent.gamma_multiply(if self.dark { 0.38 } else { 0.28 }));
                            ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())), 2.0, accent);
                        } else if sel.contains(&path) {
                            ui.painter().rect_filled(rect, 4.0, accent.gamma_multiply(if self.dark { 0.20 } else { 0.14 }));
                        } else if resp.hovered() {
                            ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.6));
                        }
                        // rotated but not saved (\u{21bb}) / exported with its rotation (\u{2713})
                        let edited = self.pending.contains_key(&path) || self.stash.contains_key(&path) || (is_cur && self.doc.as_ref().map_or(false, |d| d.modified));
                        let rotated = self.pending.contains_key(&path);
                        let mark = if edited && self.exported.contains(&path) { Some(("\u{2713}", Color32::from_rgb(70, 170, 90), "Edits saved to _geo")) } else if rotated { Some(("\u{21bb}", Color32::from_rgb(214, 150, 40), "Rotated, not saved: select it and use Save as .ams")) } else if edited { Some(("\u{270e}", Color32::from_rgb(214, 150, 40), "Edited, not saved: select it and use Save as .ams")) } else { None };
                        if let Some((sym, col, _)) = mark {
                            ui.painter().text(egui::Pos2::new(rect.max.x - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, sym, egui::FontId::proportional(14.0), col);
                        }
                        let color = if is_main || is_cur { ui.visuals().strong_text_color() } else { dim };
                        let text = if is_main { RichText::new(n).strong().color(color) } else { RichText::new(n).color(color) };
                        let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Truncate), rect.width() - 12.0, egui::TextStyle::Body);
                        ui.painter().galley(egui::Pos2::new(rect.min.x + 8.0, rect.center().y - galley.size().y / 2.0), galley, color);
                        if is_cur && moved {
                            resp.scroll_to_me(None);
                        }
                        if resp.clicked() {
                            if mods.shift {
                                // a range from the anchor to this file
                                let a_ix = anchor.as_ref().and_then(|x| flat.iter().position(|y| y == x)).unwrap_or(0);
                                let b_ix = flat.iter().position(|y| *y == path).unwrap_or(0);
                                let (lo, hi) = (a_ix.min(b_ix), a_ix.max(b_ix));
                                let range: Vec<PathBuf> = flat[lo..=hi].to_vec();
                                if mods.ctrl || mods.command {
                                    // Ctrl+Shift: every file of the range, the grayed ones too, added to the selection
                                    for r in range {
                                        if !sel.contains(&r) {
                                            sel.push(r);
                                        }
                                    }
                                } else {
                                    // Shift: only the files the identifier matched
                                    sel = range.into_iter().filter(|r| main_files.contains(r)).collect();
                                }
                            } else if mods.ctrl || mods.command {
                                // a single file in or out of the selection
                                if let Some(pos) = sel.iter().position(|y| *y == path) {
                                    sel.remove(pos);
                                } else {
                                    sel.push(path.clone());
                                }
                                anchor = Some(path.clone());
                            } else {
                                sel = vec![path.clone()];
                                anchor = Some(path.clone());
                                open = Some(path.clone());
                            }
                        }
                        let tip = match (is_main, mark) {
                            (true, Some((_, _, t))) => format!("Main file of this specimen. {t}"),
                            (true, None) => "Main file of this specimen".to_string(),
                            (false, Some((_, _, t))) => t.to_string(),
                            (false, None) => String::new(),
                        };
                        if !tip.is_empty() {
                            resp.on_hover_text(tip);
                        }
                    }
                    if matches!(p, Pick::None | Pick::Several(_)) {
                        ui.label(RichText::new("No single main file: use Change string ...").small().weak());
                    }
                });
            }
        });
        self.tree_cur = cur.clone();
        self.tree_sel = sel;
        self.tree_anchor = anchor;
        if export {
            self.open_export();
        }
        if change {
            self.change_set_string();
        }
        if close {
            self.set = None;
            self.save_state();
        }
        if let Some(p) = open {
            self.open_path(p);
        }
    }
}


/// One answer of a question as a card: a radio dot, the title, a short example, and the explanation behind a circled (i).
fn card(ui: &mut Ui, selected: bool, title: &str, example: &str, tip: &str, dark: bool) -> bool {
    let accent = if dark { Color32::from_rgb(106, 160, 255) } else { Color32::from_rgb(47, 111, 222) };
    let line = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let mut clicked = false;
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 30.0).max(160.0);
        let r = egui::Frame::new()
            .fill(if selected { accent.gamma_multiply(if dark { 0.22 } else { 0.12 }) } else { Color32::TRANSPARENT })
            .stroke(Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { accent } else { line }))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(w - 26.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(if selected { "\u{25cf}" } else { "\u{25cb}" }).color(accent).size(18.0));
                    ui.label(RichText::new(title).strong().size(16.0));
                });
                if !example.is_empty() {
                    ui.label(RichText::new(example).monospace().small().weak());
                }
            });
        let resp = ui.interact(r.response.rect, ui.make_persistent_id(("ask-card", title)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        clicked = resp.clicked();
        super::info_icon(ui, tip);
    });
    clicked
}

fn chip(ui: &mut Ui, selected: bool, text: &str) -> bool {
    ui.add(egui::Button::selectable(selected, RichText::new(text).size(15.0)).min_size(Vec2::new(0.0, 32.0))).clicked()
}

use egui::Stroke;

/// The questions before the list of main files: where the files lie, which file type, whether the phases are split and
/// how the specimen ID is written. Returns (continue, cancel).
fn ask_ui(ui: &mut Ui, sd: &mut SetDialog, dark: bool, _hint: &str) -> (bool, bool) {
    let (n_ams, n_ran) = sd.raw.counts();
    let before = sd.opts;
    ui.horizontal(|ui| {
        ui.label(RichText::new("How are your specimen files stored?").strong().size(20.0));
        super::info_icon(ui, "Three short questions so that AMS Studio finds the right file of every specimen. The answers are remembered for the next time you open a folder.\nThe pre-selected answers are what the program saw in the names; if they are right, press Continue.");
    });
    ui.label(RichText::new(format!("Found: .ams files (n={n_ams}), .ran files (n={n_ran}) in folders (n={})", sd.raw.dirs.len())).weak());

    ui.label(RichText::new("1.  Where are your files?").strong().size(16.0));
    if card(ui, sd.opts.layout == FileLayout::OwnFolders, "Each specimen has its own folder", "parent/ER01/ER01_use.ams    parent/ER02/ER02_use.ams", "One folder per specimen, with that specimen's data files inside (a folder with a single file also counts). The folder is the specimen; the file names only matter when a folder holds several files.", dark) {
        sd.opts.layout = FileLayout::OwnFolders;
    }
    if card(ui, sd.opts.layout == FileLayout::OneFolder, "All files are in one folder", "parent/1234567.ams    parent/1234568.ams", "All specimens lie next to each other in the same folder. AMS Studio tells the specimens apart by the specimen ID in the file name, for example a 7-digit number, and treats the files with the same ID as one specimen.", dark) {
        sd.opts.layout = FileLayout::OneFolder;
    }
    if card(ui, sd.opts.layout == FileLayout::Auto, "Not sure: look at the names", "mixed, or a bit of both", "AMS Studio looks at the file names of every folder: a folder whose files carry several different specimen IDs is split by ID, a folder whose files all belong to one ID stays one specimen.", dark) {
        sd.opts.layout = FileLayout::Auto;
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("2.  Which file type are your specimens in?").strong().size(16.0));
        super::info_icon(ui, ".ams is what Anisoft 6 and the Kappabridge software write. .ran is the older Anisoft 4.2 format: it holds one phase per file, the out-of-phase data of a specimen are a second file (for example X.ran and X_Im.ran). AMS Studio reads both and joins the two files of a split specimen when it opens it. Nothing is ever written into your data folders.");
    });
    ui.horizontal(|ui| {
        if chip(ui, sd.opts.kind == Kind::Ams, &format!(".ams  (n={n_ams})")) {
            sd.opts.kind = Kind::Ams;
        }
        if chip(ui, sd.opts.kind == Kind::Ran, &format!(".ran  (n={n_ran})")) {
            sd.opts.kind = Kind::Ran;
        }
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("3.  Are in-phase and out-of-phase data in separate files?").strong().size(16.0));
        super::info_icon(ui, "Some labs export the in-phase and the out-of-phase part of a specimen as two files. \"Look at the names\" joins files whose names say it (X and X_Im, X_in and X_out, X and X_oop). \"Yes\" also pairs two files of a specimen whose names say nothing, by their numbers: the file with the much smaller susceptibility is the out-of-phase one. \"No\" leaves every file as it is.");
    });
    ui.horizontal(|ui| {
        if chip(ui, sd.opts.phases == Phases::Off, "No, one file has both") {
            sd.opts.phases = Phases::Off;
        }
        if chip(ui, sd.opts.phases == Phases::ByName, "Look at the names") {
            sd.opts.phases = Phases::ByName;
        }
        if chip(ui, sd.opts.phases == Phases::Yes, "Yes, two files per specimen") {
            sd.opts.phases = Phases::Yes;
        }
    });

    if sd.opts.layout != FileLayout::OwnFolders {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("4.  How is the specimen ID written in a file name?").strong().size(16.0));
            super::info_icon(ui, "Used to tell the specimens apart in a folder that holds many. \"Detect\" tries a 7-digit number first (1234567_use.ams), then other digit counts, then the first part of the name (ER26PB002_use.ams). Choose another if the list that follows groups the files wrongly.");
        });
        let label = match sd.opts.scheme {
            SchemeChoice::Auto => "Detect it from the names".to_string(),
            SchemeChoice::Digits(n) => format!("A number with {n} digits"),
            SchemeChoice::FirstWord => "The first part of the name".to_string(),
            SchemeChoice::Chars(n) => format!("The first {n} characters"),
        };
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("ask-scheme").selected_text(label).width(260.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut sd.opts.scheme, SchemeChoice::Auto, "Detect it from the names");
                ui.selectable_value(&mut sd.opts.scheme, SchemeChoice::Digits(7), "A 7-digit number");
                if !matches!(sd.opts.scheme, SchemeChoice::Digits(7) | SchemeChoice::Digits(0)) && matches!(sd.opts.scheme, SchemeChoice::Digits(_)) {
                    ui.label("(another digit count: set below)");
                }
                if ui.selectable_label(matches!(sd.opts.scheme, SchemeChoice::Digits(n) if n != 7), "A number with another digit count").clicked() {
                    sd.opts.scheme = SchemeChoice::Digits(6);
                }
                ui.selectable_value(&mut sd.opts.scheme, SchemeChoice::FirstWord, "The first part of the name");
                if ui.selectable_label(matches!(sd.opts.scheme, SchemeChoice::Chars(_)), "The first N characters").clicked() {
                    sd.opts.scheme = SchemeChoice::Chars(9);
                }
            });
            match &mut sd.opts.scheme {
                SchemeChoice::Digits(n) if *n != 7 => {
                    ui.add(egui::DragValue::new(n).range(3..=14).suffix(" digits"));
                }
                SchemeChoice::Chars(n) => {
                    ui.add(egui::DragValue::new(n).range(2..=30).suffix(" characters"));
                }
                _ => {}
            }
        });
    }

    // what the answers give, live
    ui.add_space(6.0);
    let b = sd.opts.build(&sd.root, &sd.raw);
    let _ = before;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("With these answers: specimens (n={})", b.folders.len())).strong());
        if b.split_dirs > 0 {
            ui.label(format!("\u{b7} shared folders split by ID (n={}), ID = {}", b.split_dirs, b.scheme.map_or("the names".to_string(), |s| s.describe())));
        }
        let pairs: usize = b.folders.iter().map(|f| f.pairs.len()).sum();
        if pairs > 0 {
            ui.label(format!("\u{b7} specimens with in- and out-of-phase files joined (n={pairs})"));
        }
    });

    let (mut go, mut cancel) = (false, false);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.set_width(ui.available_width());
        cancel = ui.add(egui::Button::new("Cancel").min_size(Vec2::new(0.0, 38.0))).clicked();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let blue = egui::Button::new(RichText::new("Continue").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(160.0, 38.0));
            go = ui.add_enabled(!b.folders.is_empty(), blue).on_hover_text("Show the specimens and the file that is the main one of each").clicked();
        });
    });
    (go, cancel)
}
