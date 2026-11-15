//! "Open Folder of Specimens": a parent folder with one sub-folder per specimen. The identifier of the main
//! `.ams` file per specimen (the author's end in `_use`) is detected, shown as a sentence with the files it
//! picks, and can be replaced by a string the user types. See `ams_core::specset`.

use super::{App, W_SET};
use ams_core::specset::{self, Detection, Pick, SpecFolder};
use egui::{Align, Color32, Layout, RichText, Sense, Ui, Vec2};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

/// The confirmed set shown in the file browser.
pub(super) struct SpecSet {
    pub root: PathBuf,
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
    pub rx: Option<Receiver<Vec<SpecFolder>>>,
    /// The string was typed by the user, not detected.
    pub typed: bool,
}

impl SetDialog {
    fn rematch(&mut self) {
        self.picks = specset::matches(&self.folders, &self.ident);
    }
}

fn file_list(f: &SpecFolder, idx: &[usize]) -> String {
    idx.iter().map(|&i| f.files[i].as_str()).collect::<Vec<_>>().join(", ")
}

impl App {
    /// Asks for the parent folder and starts scanning it.
    pub(super) fn open_folder_of_specimens(&mut self, ctx: &egui::Context) {
        let mut dlg = rfd::FileDialog::new().set_title("Open Folder of Specimens (the parent of the specimen folders)");
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
            let _ = tx.send(specset::scan(&r2));
            c2.request_repaint();
        });
        self.sd = SetDialog { scanning: true, root, rx: Some(rx), ..Default::default() };
        self.open_win(W_SET);
    }

    /// Re-opens the dialog on the folders of the current set, to try another string.
    pub(super) fn change_set_string(&mut self) {
        let Some(s) = &self.set else { return };
        let det = specset::detect(&s.folders, Some(&self.last_ident));
        self.sd = SetDialog {
            root: s.root.clone(),
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
                Ok(folders) => {
                    sd.rx = None;
                    sd.scanning = false;
                    let det = specset::detect(&folders, Some(&self.last_ident));
                    sd.ident = det.ident.clone().unwrap_or_default();
                    sd.folders = folders;
                    sd.detection = Some(det);
                    sd.rematch();
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
        let accent = if dark { Color32::from_rgb(106, 160, 255) } else { Color32::from_rgb(47, 111, 222) };
        let warn = Color32::from_rgb(214, 150, 40);
        let bad = Color32::from_rgb(214, 90, 90);
        let ok = Color32::from_rgb(70, 170, 90);
        let (mut open, mut use_it, mut cancel, mut match_now, mut back_to_detected, mut toggle_custom) = (true, false, false, false, false, false);
        self.base_window(ctx, "Open Folder of Specimens", W_SET, Vec2::new(860.0, 700.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            super::tools_ui::pop_style(ui, 1.15);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            ui.label(RichText::new(sd.root.display().to_string()).weak().small());
            if sd.scanning {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Looking for .ams files in the sub-folders ...");
                });
                return;
            }
            if sd.folders.is_empty() {
                ui.label(RichText::new("No .ams files were found under this folder.").color(bad));
                ui.label("Pick the folder that contains one sub-folder per specimen, each with its .ams files.");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| cancel = ui.button("Close").clicked());
                return;
            }
            let n_files: usize = sd.folders.iter().map(|f| f.files.len()).sum();
            let det = sd.detection.clone().unwrap_or(Detection { ident: None, multi: 0, covered: 0, needed: false });
            ui.label(format!("{} specimen folders with {} .ams files.", sd.folders.len(), n_files));

            // ---- the sentence
            let matched = sd.picks.iter().filter(|p| p.file().is_some()).count();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if !det.needed {
                    ui.label(RichText::new("Every specimen folder holds exactly one .ams file, so no identifier is needed.").strong());
                } else if sd.ident.trim().is_empty() {
                    ui.label(RichText::new("No identifier was found that the main files share.").strong().color(warn));
                    ui.label("The .ams files in the specimen folders are named differently from folder to folder, so the program cannot tell which one is the main file of each specimen. Please rename the main file of every specimen so that they share one string (for example SPECIMEN_use.ams) before using AMS Studio, or tell the program which string to look for.");
                } else if sd.typed {
                    ui.label(RichText::new(format!("You asked for \u{201c}{}\u{201d}: it picks one file in {} of {} specimens.", sd.ident.trim(), matched, sd.folders.len())).strong().color(if matched == sd.folders.len() { ok } else { warn }));
                    ui.label(RichText::new("Check the list below: every specimen should show the file you consider its main file.").weak().small());
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("\u{201c}{}\u{201d}", sd.ident)).strong().color(accent).size(17.0));
                        ui.label(RichText::new("is the identifier of the main .ams file in each specimen folder.").strong());
                    });
                    ui.label(RichText::new(format!("Found in {} of the {} folders that hold more than one file. Check the list below that these are the files you want.", det.covered, det.multi)).weak().small());
                }
                ui.horizontal(|ui| {
                    toggle_custom = ui.button(if sd.custom { "Hide the other-string search" } else { "Search for another string ..." }).on_hover_text("Type a part of the file name that marks the main file, and see which files it picks").clicked();
                });
                if sd.custom {
                    ui.horizontal(|ui| {
                        ui.label("The main file's name contains:");
                        let r = ui.add(egui::TextEdit::singleline(&mut sd.input).desired_width(220.0).hint_text("e.g. _final"));
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        match_now = ui.add(egui::Button::new(RichText::new("Match").strong())).clicked() || enter;
                        if let Some(d) = &det.ident {
                            back_to_detected = ui.small_button(format!("Back to \u{201c}{d}\u{201d}")).clicked();
                        }
                    });
                    ui.label(RichText::new("Upper and lower case do not matter. A folder with a single .ams file always gives that file.").weak().small());
                }
            });

            // ---- the files
            ui.horizontal(|ui| {
                ui.label(RichText::new("Files that would be opened").strong());
                ui.label(RichText::new(format!("{matched} of {} specimens matched", sd.folders.len())).color(if matched == sd.folders.len() { ok } else { warn }).small());
            });
            let h = (ui.available_height() - 56.0).max(140.0);
            egui::ScrollArea::both().max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("set-grid").striped(true).num_columns(4).min_col_width(60.0).spacing([16.0, 5.0]).show(ui, |ui| {
                    for hd in ["#", "Specimen folder", "Main file", "Note"] {
                        ui.label(RichText::new(hd).strong());
                    }
                    ui.end_row();
                    for (i, (f, p)) in sd.folders.iter().zip(&sd.picks).enumerate() {
                        ui.label(RichText::new((i + 1).to_string()).weak());
                        ui.label(&f.name);
                        match p {
                            Pick::Only(k) => {
                                ui.label(&f.files[*k]);
                                ui.label(RichText::new("only .ams file").weak().small());
                            }
                            Pick::Match(k) => {
                                ui.label(RichText::new(&f.files[*k]).color(ok));
                                ui.label(RichText::new(format!("{} files in the folder", f.files.len())).weak().small());
                            }
                            Pick::None => {
                                ui.label(RichText::new("no match").color(bad));
                                ui.label(RichText::new(format!("none of {}", file_list(f, &(0..f.files.len()).collect::<Vec<_>>()))).weak().small());
                            }
                            Pick::Several(ix) => {
                                ui.label(RichText::new("several match").color(warn));
                                ui.label(RichText::new(file_list(f, ix)).weak().small());
                            }
                        }
                        ui.end_row();
                    }
                });
            });
            ui.horizontal(|ui| {
                let label = if matched == sd.folders.len() { "Use these files".to_string() } else { format!("Use the {matched} matched files") };
                let go = egui::Button::new(RichText::new(label).strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(150.0, 0.0));
                use_it = ui.add_enabled(matched > 0, go).on_hover_text("Show the specimens in the file browser and open the first main file").clicked();
                if matched < sd.folders.len() {
                    ui.label(RichText::new("Specimens without a match are listed in the browser but skipped by Previous / Next.").weak().small());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| cancel = ui.button("Cancel").clicked());
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
            let set = SpecSet { root: sd.root.clone(), folders: sd.folders.clone(), ident: sd.ident.trim().to_string(), picks: sd.picks.clone() };
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
    pub(super) fn set_browser_ui(&mut self, ui: &mut Ui) {
        let Some(set) = &self.set else { return };
        let cur = self.doc.as_ref().map(|d| d.path.clone());
        let mut open: Option<PathBuf> = None;
        let (mut change, mut close) = (false, false);
        ui.strong("Specimens");
        ui.horizontal(|ui| {
            change = ui.small_button("Change string ...").on_hover_text("Search for another identifier of the main file").clicked();
            close = ui.small_button("Close set").on_hover_text("Back to the plain folder browser").clicked();
        });
        ui.label(RichText::new(set.root.display().to_string()).small().weak());
        let matched = set.picks.iter().filter(|p| p.file().is_some()).count();
        let sentence = if set.ident.is_empty() { "one .ams file per specimen".to_string() } else { format!("main file: \u{201c}{}\u{201d}", set.ident) };
        ui.label(RichText::new(format!("{sentence}  \u{b7}  {matched} of {}", set.folders.len())).small().weak());
        ui.separator();
        let open_all = set.folders.len() <= 40;
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            for (fi, (f, p)) in set.folders.iter().zip(&set.picks).enumerate() {
                let main = p.file();
                let has_cur = cur.as_ref().map_or(false, |c| f.files.iter().any(|n| f.dir.join(n) == *c));
                let title = match p {
                    Pick::Only(_) | Pick::Match(_) => RichText::new(&f.name).strong(),
                    Pick::None => RichText::new(format!("\u{26a0} {}", f.name)).strong().color(Color32::from_rgb(214, 90, 90)),
                    Pick::Several(_) => RichText::new(format!("\u{26a0} {}", f.name)).strong().color(Color32::from_rgb(214, 150, 40)),
                };
                let id = ui.make_persistent_id(("set-folder", fi, &set.root));
                egui::CollapsingHeader::new(title).id_salt(id).default_open(open_all || has_cur).show(ui, |ui| {
                    // the file the identifier picked comes first; the others follow in a darker shade
                    let mut order: Vec<usize> = (0..f.files.len()).collect();
                    if let Some(m) = main {
                        order.retain(|k| *k != m);
                        order.insert(0, m);
                    }
                    let dim = ui.visuals().text_color().gamma_multiply(0.55);
                    for k in order {
                        let n = &f.files[k];
                        let path = f.dir.join(n);
                        let is_main = main == Some(k);
                        let text = if is_main { RichText::new(format!("\u{25cf} {n}")).strong() } else { RichText::new(format!("   {n}")).color(dim) };
                        let resp = ui.add(egui::Button::selectable(cur.as_ref() == Some(&path), text).frame_when_inactive(false).sense(Sense::click()));
                        if resp.clicked() {
                            open = Some(path);
                        }
                        if is_main {
                            resp.on_hover_text("Main file of this specimen");
                        }
                    }
                    if matches!(p, Pick::None | Pick::Several(_)) {
                        ui.label(RichText::new("No single main file: use Change string ...").small().weak());
                    }
                });
            }
        });
        if change {
            self.change_set_string();
        }
        if close {
            self.set = None;
        }
        if let Some(p) = open {
            self.open_path(p);
        }
    }
}
