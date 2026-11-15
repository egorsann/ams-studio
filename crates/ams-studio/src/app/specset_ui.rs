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
    /// The scan is the silent restore of the folder that was open last time.
    pub restore: bool,
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

    /// Opens the folder set that was open when the program was closed, without asking: the folders are scanned in
    /// the background and the identifier used last is applied. The dialog only appears if nothing matches.
    pub(super) fn restore_set(&mut self, ctx: &egui::Context, root: PathBuf, file: Option<PathBuf>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (r2, c2) = (root.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(specset::scan(&r2));
            c2.request_repaint();
        });
        self.sd = SetDialog { scanning: true, root, rx: Some(rx), restore: true, ..Default::default() };
        self.restore_file = file;
    }

    /// Called every frame: finishes the silent restore when the scan is done.
    pub(super) fn poll_restore(&mut self, ctx: &egui::Context) {
        if !self.sd.restore {
            return;
        }
        let Some(rx) = &self.sd.rx else { return };
        let folders = match rx.try_recv() {
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
        self.sd = SetDialog::default();
        if folders.is_empty() {
            return; // the folder is gone or empty: start as usual
        }
        let det = specset::detect(&folders, Some(&self.last_ident));
        let ident = det.ident.clone().unwrap_or_default();
        let picks = specset::matches(&folders, &ident);
        let matched = picks.iter().filter(|p| p.file().is_some()).count();
        if matched == 0 {
            // nothing to go by: let the user choose, as when opening the folder by hand
            self.sd = SetDialog { root, ident, picks, detection: Some(det), folders, ..Default::default() };
            self.open_win(W_SET);
            return;
        }
        let set = SpecSet { root: root.clone(), folders, ident, picks };
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
            s.folders = specset::scan(&s.root);
            s.picks = specset::matches(&s.folders, &s.ident);
        }
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
                                    for r in range {
                                        if !sel.contains(&r) {
                                            sel.push(r);
                                        }
                                    }
                                } else {
                                    sel = range;
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
