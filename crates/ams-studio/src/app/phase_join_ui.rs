//! Edit > Recombine: the first question ("one or multiple full datasets?"), then the joining of in-phase and
//! out-of-phase files (`.ams` or Anisoft 4.2 `.ran`) into full `.ams` files. The pure steps (reading, pairing,
//! joining, saving) are plain functions so they can be tested without a window; the windows only call them.
//! The data folders are never written to: results go to a file or folder the user chooses.

use super::{tools_ui, App, W_PHASE, W_RECOMB, W_RECOMB_ASK};
use ams_core::phases::{self, MergeReport};
use ams_core::specset::{self, Kind, RawScan};
use egui::{Align, Color32, Layout, RichText, Sense, Stroke, Ui, Vec2};
use std::path::{Path, PathBuf};

/// What the user is combining.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Goal {
    /// One in-phase file and one out-of-phase file into one full dataset.
    #[default]
    One,
    /// Many pairs, one full file per pair.
    Many,
    /// The parts of one dataset that were split by specimens (the former Recombine window).
    Parts,
}

#[derive(Clone, Debug)]
pub(super) struct PairRow {
    pub key: String,
    pub dir: PathBuf,
    pub ip: Option<PathBuf>,
    pub op: Option<PathBuf>,
}

#[derive(Default)]
pub(super) struct PhaseJoin {
    pub goal: Goal,
    pub kind: Kind,
    pub ip: Option<PathBuf>,
    pub op: Option<PathBuf>,
    /// The join of the two chosen files, and what it did (or why it failed).
    pub preview: Option<Result<(Vec<u8>, MergeReport), String>>,
    pub warn: String,
    pub root: Option<PathBuf>,
    pub rows: Vec<PairRow>,
    pub by_content: bool,
    pub msg: String,
}

/// The records of one data file: `.ams` as it is, `.ran` converted (no partner is looked for).
pub(super) fn records_of(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if crate::doc::is_ran(path) {
        ams_core::import::ran_to_records(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    } else {
        if bytes.is_empty() || bytes.len() % ams_core::ams::RECORD_LEN != 0 {
            return Err(format!("{}: not an .ams file ({} bytes is not a whole number of 640-byte records)", path.display(), bytes.len()));
        }
        Ok(bytes)
    }
}

/// Joins the two files. Their roles are as given; `Err` when one cannot be read.
pub(super) fn join_files(ip: &Path, op: &Path) -> Result<(Vec<u8>, MergeReport), String> {
    let a = records_of(ip)?;
    let b = records_of(op)?;
    phases::merge_phases(&a, &b)
}

/// Pairs the files of every folder of a scan: by name, and (when asked) the two files of a folder that say nothing in
/// their names by their numbers.
pub(super) fn find_pairs(raw: &RawScan, kind: Kind, by_content: bool) -> Vec<PairRow> {
    let mut out = vec![];
    for (dir, all) in &raw.dirs {
        let files: Vec<&String> = all.iter().filter(|n| kind.has(n)).collect();
        if files.is_empty() {
            continue;
        }
        let stems: Vec<String> = files.iter().map(|f| f.rsplit_once('.').map_or((*f).clone(), |(s, _)| s.to_string())).collect();
        let pairs = phases::pair_files(&stems);
        let mut rows: Vec<PairRow> = pairs.iter().map(|p| PairRow { key: p.key.clone(), dir: dir.clone(), ip: p.ip.map(|i| dir.join(files[i])), op: p.op.map(|i| dir.join(files[i])) }).collect();
        if by_content && files.len() == 2 && rows.iter().all(|r| r.ip.is_none() || r.op.is_none()) {
            let (a, b) = (dir.join(files[0]), dir.join(files[1]));
            if let (Ok(x), Ok(y)) = (records_of(&a), records_of(&b)) {
                if let Some(k) = phases::guess_out_of_phase(&x, &y) {
                    let (ip, op) = if k == 1 { (a, b) } else { (b, a) };
                    let key = stems[0].clone();
                    rows = vec![PairRow { key: phases::split_phase_word(&key).0, dir: dir.clone(), ip: Some(ip), op: Some(op) }];
                }
            }
        }
        out.extend(rows);
    }
    out
}

/// What saving the pairs did.
#[derive(Debug, Default)]
pub(super) struct SaveReport {
    pub written: Vec<PathBuf>,
    pub records: usize,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

fn clean_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ' | '(' | ')') { c } else { '_' }).collect();
    t.trim().trim_matches('.').to_string()
}

/// Writes one full `.ams` per complete pair into `out_dir` (made if missing). `out_dir` must not be one of the source
/// folders: the data folders stay untouched.
pub(super) fn save_pairs(rows: &[PairRow], out_dir: &Path) -> Result<SaveReport, String> {
    if rows.iter().any(|r| r.dir == out_dir) {
        return Err("Choose another folder for the results: the folders that hold your data files are left untouched.".to_string());
    }
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let mut rep = SaveReport::default();
    let mut used: Vec<String> = vec![];
    for r in rows {
        let (Some(ip), Some(op)) = (&r.ip, &r.op) else {
            rep.skipped.push(format!("{}: {}", r.key, if r.ip.is_none() { "no in-phase file" } else { "no out-of-phase file" }));
            continue;
        };
        match join_files(ip, op) {
            Ok((bytes, m)) => {
                let mut name = clean_name(&r.key);
                if name.is_empty() {
                    name = "specimens".to_string();
                }
                if used.contains(&name.to_lowercase()) {
                    // two folders with the same specimen name: the folder name tells them apart
                    name = format!("{}_{}", clean_name(&r.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()), name);
                }
                used.push(name.to_lowercase());
                let p = out_dir.join(format!("{name}.ams"));
                std::fs::write(&p, &bytes).map_err(|e| format!("{}: {e}", p.display()))?;
                rep.records += m.joined;
                if !m.ip_only.is_empty() || !m.op_only.is_empty() {
                    rep.warnings.push(format!("{name}: {} specimen(s) without an out-of-phase partner, {} out-of-phase record(s) without an in-phase partner", m.ip_only.len(), m.op_only.len()));
                }
                rep.written.push(p);
            }
            Err(e) => rep.skipped.push(format!("{}: {e}", r.key)),
        }
    }
    Ok(rep)
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

impl PhaseJoin {
    /// Joins the two chosen files for the preview and checks that the roles look right.
    pub fn refresh_one(&mut self) {
        self.preview = None;
        self.warn.clear();
        let (Some(ip), Some(op)) = (self.ip.clone(), self.op.clone()) else { return };
        if ip == op {
            self.preview = Some(Err("The in-phase and the out-of-phase file are the same file.".to_string()));
            return;
        }
        if let (Ok(a), Ok(b)) = (records_of(&ip), records_of(&op)) {
            if phases::guess_out_of_phase(&a, &b) == Some(0) {
                self.warn = "The file chosen as out-of-phase has the much larger susceptibility, so the two are probably the wrong way round.".to_string();
            }
        }
        self.preview = Some(join_files(&ip, &op));
    }

    /// The in-phase file was chosen: its out-of-phase partner is looked for by name.
    pub fn set_ip(&mut self, p: PathBuf) {
        if self.op.is_none() {
            self.op = crate::doc::find_partner(&p);
        }
        self.ip = Some(p);
        self.refresh_one();
    }

    pub fn swap(&mut self) {
        std::mem::swap(&mut self.ip, &mut self.op);
        self.refresh_one();
    }

    pub fn scan_many(&mut self, root: PathBuf) {
        let raw = specset::scan_raw(&root);
        self.rows = find_pairs(&raw, self.kind, self.by_content);
        self.root = Some(root);
    }
}

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
                ui.label(RichText::new(example).monospace().small().weak());
            });
        clicked = ui.interact(r.response.rect, ui.make_persistent_id(("pj-card", title)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
        super::info_icon(ui, tip);
    });
    clicked
}

fn blue(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }
}

impl App {
    pub(super) fn open_recombine(&mut self) {
        self.open_win(W_RECOMB_ASK);
    }

    /// The first window of Recombine: one or multiple full datasets, and the file type.
    pub(super) fn recombine_ask_window(&mut self, ctx: &egui::Context) {
        if !self.win_recomb_ask {
            return;
        }
        let dark = self.dark;
        let (mut open, mut go, mut cancel) = (true, false, false);
        let mut pj = std::mem::take(&mut self.pj);
        self.base_window(ctx, "Recombine", W_RECOMB_ASK, Vec2::new(700.0, 560.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.2);
            ui.spacing_mut().item_spacing = Vec2::new(10.0, 12.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Are you combining in- and out-of-phase data into one or multiple full .ams datasets?").strong().size(19.0));
                super::info_icon(ui, "A full .ams file holds the in-phase and the out-of-phase measurement of every specimen. Some exports keep the two phases in separate files (an in-phase file and an out-of-phase file, or Anisoft 4.2's X.ran and X_Im.ran). This tool joins them again. Your original files are never changed: the full files are saved where you say.");
            });
            if card(ui, pj.goal == Goal::One, "One full dataset", "in-phase file  +  out-of-phase file   ->   one full .ams", "You have one file with the in-phase data of your specimens and one file with their out-of-phase data (the same specimens). You pick the two files and get one full .ams.", dark) {
                pj.goal = Goal::One;
            }
            if card(ui, pj.goal == Goal::Many, "Multiple full datasets", "folder with X.ran + X_Im.ran, Y.ran + Y_Im.ran ...   ->   X.ams, Y.ams ...", "You have many pairs of files, for example one in-phase and one out-of-phase file per specimen, spread over a folder or its sub-folders. AMS Studio finds the pairs by their names and saves one full .ams for each.", dark) {
                pj.goal = Goal::Many;
            }
            if card(ui, pj.goal == Goal::Parts, "Neither: I am joining the parts of one split dataset", "part1.ams (specimens A-F)  +  part2.ams (G-Z)   ->   one .ams", "The files hold different specimens of one measurement series, each with both phases, and you want them in one file in a sensible order. This is the Recombine Split AMS Files window.", dark) {
                pj.goal = Goal::Parts;
            }
            if pj.goal != Goal::Parts {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Which file type are your files?").strong().size(16.0));
                    super::info_icon(ui, ".ams is written by the Kappabridge software and Anisoft 6. .ran is the older Anisoft 4.2 format with one phase per file. Both are read; the result is always a .ams file.");
                });
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::selectable(pj.kind == Kind::Ams, RichText::new(".ams").size(15.0)).min_size(Vec2::new(90.0, 32.0))).clicked() {
                        pj.kind = Kind::Ams;
                    }
                    if ui.add(egui::Button::selectable(pj.kind == Kind::Ran, RichText::new(".ran").size(15.0)).min_size(Vec2::new(90.0, 32.0))).clicked() {
                        pj.kind = Kind::Ran;
                    }
                });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                cancel = ui.add(egui::Button::new("Cancel").min_size(Vec2::new(0.0, 38.0))).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    go = ui.add(egui::Button::new(RichText::new("Continue").strong().color(Color32::WHITE)).fill(blue(dark)).min_size(Vec2::new(160.0, 38.0))).clicked();
                });
            });
        });
        if go {
            open = false;
            match pj.goal {
                Goal::Parts => self.open_win(W_RECOMB),
                _ => {
                    pj.msg.clear();
                    self.open_win(W_PHASE);
                }
            }
        }
        if cancel {
            open = false;
        }
        self.win_recomb_ask = open;
        self.pj = pj;
    }

    fn pick_data_file(&self, title: &str, kind: Kind, hint: Option<&Path>) -> Option<PathBuf> {
        let mut dlg = crate::fdialog::FileDialog::new().set_title(title);
        dlg = match kind {
            Kind::Ams => dlg.add_filter("AMS files (*.ams)", &["ams"]),
            Kind::Ran => dlg.add_filter("RAN files (*.ran)", &["ran"]),
        };
        let start = hint.and_then(|h| h.parent().map(|p| p.to_path_buf())).filter(|p| p.is_dir()).or_else(|| Some(self.browser_dir.clone()).filter(|p| p.is_dir()));
        if let Some(d) = start {
            dlg = dlg.set_directory(d);
        }
        dlg.pick_file()
    }

    /// The window that joins the files, for one pair or many.
    pub(super) fn phase_join_window(&mut self, ctx: &egui::Context) {
        if !self.win_phase {
            return;
        }
        let dark = self.dark;
        let ok = Color32::from_rgb(70, 170, 90);
        let warn = Color32::from_rgb(214, 150, 40);
        let bad = Color32::from_rgb(214, 90, 90);
        let mut pj = std::mem::take(&mut self.pj);
        let (mut open, mut back) = (true, false);
        let mut do_pick_ip = false;
        let mut do_pick_op = false;
        let mut do_swap = false;
        let mut do_save_one = false;
        let mut do_open_one = false;
        let mut do_pick_root = false;
        let mut do_save_many = false;
        let mut rescan = false;
        let cr = ctx.content_rect();
        let size = Vec2::new((cr.width() - 24.0).clamp(560.0, 940.0), (cr.height() - 56.0).clamp(420.0, 720.0));
        let title = if pj.goal == Goal::One { "Join in-phase and out-of-phase files into one full .ams" } else { "Join many pairs of files into full .ams files" };
        self.base_window(ctx, "Join phases", W_PHASE, size).max_size(Vec2::new((cr.width() - 16.0).max(420.0), (cr.height() - 48.0).max(360.0))).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.15);
            ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).strong().size(19.0));
                super::info_icon(ui, "The in-phase file gives the in-phase tensor of every specimen, the out-of-phase file gives the out-of-phase tensor. Specimens are matched by name (a tag such as _Im on the out-of-phase names is ignored). The full records keep everything else from the in-phase file. Nothing is written until you press Save, and never into the folder of your data files when saving many.");
            });
            ui.label(RichText::new(format!("File type: .{}   ({})", pj.kind.ext(), if pj.goal == Goal::One { "one pair" } else { "many pairs" })).weak());
            if pj.goal == Goal::One {
                ui.label(RichText::new("1.  Pick the in-phase file").strong());
                ui.horizontal(|ui| {
                    do_pick_ip = ui.button("Browse ...").clicked();
                    ui.label(pj.ip.as_ref().map_or("(none chosen)".to_string(), |p| file_name(p)));
                });
                ui.label(RichText::new("2.  Pick the out-of-phase file").strong());
                ui.horizontal(|ui| {
                    do_pick_op = ui.button("Browse ...").clicked();
                    ui.label(pj.op.as_ref().map_or("(none chosen)".to_string(), |p| file_name(p)));
                    if pj.ip.is_some() && pj.op.is_some() {
                        do_swap = ui.button("Swap the two").on_hover_text("In case the in-phase and the out-of-phase file are the wrong way round").clicked();
                    }
                    super::info_icon(ui, "When you pick the in-phase file first, the out-of-phase file is looked for next to it by name (X and X_Im, X_in and X_out, X and X_oop).");
                });
                if !pj.warn.is_empty() {
                    ui.label(RichText::new(format!("\u{26a0} {}", pj.warn)).color(warn));
                }
                match &pj.preview {
                    Some(Ok((_, m))) => {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Specimens joined (n={})", m.joined)).strong().color(if m.ip_only.is_empty() && m.op_only.is_empty() { ok } else { warn }));
                            super::info_icon(ui, "How many specimens got both an in-phase and an out-of-phase tensor.");
                        });
                        if m.by_position {
                            ui.label(RichText::new("The names of the two files do not match, so the records were joined in file order (both files have the same number of records).").color(warn));
                        }
                        if m.from_first_slot > 0 {
                            ui.label(RichText::new(format!("Out-of-phase numbers taken from the first data block of the second file (n={}): that is where a split export keeps them.", m.from_first_slot)).weak());
                        }
                        if !m.ip_only.is_empty() {
                            ui.label(RichText::new(format!("In-phase records without an out-of-phase partner (n={}): {}", m.ip_only.len(), m.ip_only.iter().take(8).cloned().collect::<Vec<_>>().join(", "))).color(warn));
                        }
                        if !m.op_only.is_empty() {
                            ui.label(RichText::new(format!("Out-of-phase records without an in-phase partner (n={}), left out: {}", m.op_only.len(), m.op_only.iter().take(8).cloned().collect::<Vec<_>>().join(", "))).color(warn));
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            do_save_one = ui.add(egui::Button::new(RichText::new("Save full .ams ...").strong().color(Color32::WHITE)).fill(blue(dark)).min_size(Vec2::new(170.0, 36.0))).clicked();
                            do_open_one = ui.add(egui::Button::new("Open without saving").min_size(Vec2::new(0.0, 36.0))).on_hover_text("Shows the joined data in the program; use File > Save Active Records As later to keep it").clicked();
                        });
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).color(bad));
                    }
                    None => {
                        ui.label(RichText::new("Pick both files to see what the join gives.").weak());
                    }
                }
            } else {
                ui.label(RichText::new("1.  Pick the folder that holds the pairs (its sub-folders are searched too)").strong());
                ui.horizontal(|ui| {
                    do_pick_root = ui.button("Browse ...").clicked();
                    ui.label(pj.root.as_ref().map_or("(none chosen)".to_string(), |p| p.display().to_string()));
                });
                ui.horizontal(|ui| {
                    let c = ui.checkbox(&mut pj.by_content, "Also pair two files whose names do not say which is which").changed();
                    super::info_icon(ui, "For a folder with exactly two data files and no phase word in their names, the file with the much smaller susceptibility is taken as the out-of-phase one.");
                    rescan = c;
                });
                if pj.root.is_some() {
                    let ready = pj.rows.iter().filter(|r| r.ip.is_some() && r.op.is_some()).count();
                    ui.label(RichText::new(format!("Pairs found (n={ready}) of files (n={})", pj.rows.len())).strong().color(if ready == pj.rows.len() && ready > 0 { ok } else { warn }));
                    let h = (ui.available_height() - 64.0).max(100.0);
                    egui::ScrollArea::both().max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                        egui::Grid::new("pj-grid").striped(true).num_columns(4).spacing([16.0, 6.0]).show(ui, |ui| {
                            for h in ["#", "Specimen", "In-phase file", "Out-of-phase file"] {
                                ui.label(RichText::new(h).strong());
                            }
                            ui.end_row();
                            for (i, r) in pj.rows.iter().enumerate() {
                                ui.label(RichText::new((i + 1).to_string()).weak());
                                ui.label(&r.key);
                                ui.label(r.ip.as_ref().map_or(RichText::new("none").color(bad), |p| RichText::new(file_name(p))));
                                ui.label(r.op.as_ref().map_or(RichText::new("none").color(bad), |p| RichText::new(file_name(p))));
                                ui.end_row();
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        do_save_many = ui.add_enabled(ready > 0, egui::Button::new(RichText::new(format!("Save full .ams files (n={ready}) ...")).strong().color(Color32::WHITE)).fill(blue(dark)).min_size(Vec2::new(240.0, 36.0))).clicked();
                        super::info_icon(ui, "You are asked for a folder to put the results in. It must not be one of the folders that hold your data files.");
                    });
                }
            }
            if !pj.msg.is_empty() {
                ui.separator();
                ui.label(RichText::new(&pj.msg).small());
            }
            ui.horizontal(|ui| {
                back = ui.button("\u{2190} Back to the first question").clicked();
            });
        });
        // ---- actions
        if do_pick_ip {
            if let Some(p) = self.pick_data_file("The in-phase file", pj.kind, pj.ip.as_deref()) {
                pj.op = None;
                pj.set_ip(p);
            }
        }
        if do_pick_op {
            if let Some(p) = self.pick_data_file("The out-of-phase file", pj.kind, pj.ip.as_deref().or(pj.op.as_deref())) {
                pj.op = Some(p);
                pj.refresh_one();
            }
        }
        if do_swap {
            pj.swap();
        }
        if do_save_one || do_open_one {
            if let (Some(Ok((bytes, m))), Some(ip)) = (pj.preview.clone(), pj.ip.clone()) {
                if do_open_one {
                    match crate::doc::Doc::simulated(&format!("{} + {} (joined)", file_name(&ip), pj.op.as_ref().map(|p| file_name(p)).unwrap_or_default()), bytes) {
                        Ok(d) => {
                            self.set_doc(d);
                            pj.msg = format!("Opened the joined data (n={} specimens). Nothing was written.", m.joined);
                        }
                        Err(e) => pj.msg = e,
                    }
                } else {
                    let key = phases::split_phase_word(&ip.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()).0;
                    let mut dlg = crate::fdialog::FileDialog::new().add_filter("AMS files", &["ams"]).set_title("Save the full .ams file").set_file_name(format!("{key}_full.ams"));
                    if let Some(d) = ip.parent().filter(|d| d.is_dir()) {
                        dlg = dlg.set_directory(d);
                    }
                    if let Some(mut p) = dlg.save_file() {
                        if p.extension().is_none() {
                            p.set_extension("ams");
                        }
                        // never over one of the two source files
                        if Some(&p) == pj.ip.as_ref() || Some(&p) == pj.op.as_ref() {
                            pj.msg = "That would overwrite one of the two source files. Choose another name.".to_string();
                        } else {
                            pj.msg = match std::fs::write(&p, &bytes) {
                                Ok(()) => format!("Saved {} (n={} specimens).", p.display(), m.joined),
                                Err(e) => format!("Could not save {}: {e}", p.display()),
                            };
                        }
                    }
                }
            }
        }
        if do_pick_root {
            let mut dlg = crate::fdialog::FileDialog::new().set_title("The folder that holds the pairs of files");
            if self.browser_dir.is_dir() {
                dlg = dlg.set_directory(&self.browser_dir);
            }
            if let Some(r) = dlg.pick_folder() {
                pj.msg.clear();
                pj.scan_many(r);
            }
        }
        if rescan {
            if let Some(r) = pj.root.clone() {
                pj.scan_many(r);
            }
        }
        if do_save_many {
            let mut dlg = crate::fdialog::FileDialog::new().set_title("Folder for the full .ams files (not one of your data folders)");
            if let Some(r) = &pj.root {
                if let Some(parent) = r.parent().filter(|d| d.is_dir()) {
                    dlg = dlg.set_directory(parent);
                }
            }
            if let Some(out) = dlg.pick_folder() {
                pj.msg = match save_pairs(&pj.rows, &out) {
                    Ok(rep) => {
                        let mut t = format!("Saved full .ams files (n={}) with specimens (n={}) into {}.", rep.written.len(), rep.records, out.display());
                        if !rep.skipped.is_empty() {
                            t += &format!("\nNot saved (n={}): {}", rep.skipped.len(), rep.skipped.iter().take(6).cloned().collect::<Vec<_>>().join("; "));
                        }
                        if !rep.warnings.is_empty() {
                            t += &format!("\nNote: {}", rep.warnings.iter().take(4).cloned().collect::<Vec<_>>().join("; "));
                        }
                        t
                    }
                    Err(e) => e,
                };
            }
        }
        if back {
            self.win_phase = false;
            self.pj = pj;
            self.open_win(W_RECOMB_ASK);
            return;
        }
        self.win_phase = open;
        self.pj = pj;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ams_core::ams::{build_record_with, parse, Phase};
    use ams_core::tensor::sym;

    fn ph(km: f64, d: f64) -> Phase {
        Phase { km, sd: [0.0; 3], tensor: sym(d, d + 0.1, 3.0 - 2.0 * d - 0.1, 0.01, 0.0, 0.0), ftest: [0.0; 3] }
    }
    fn zero() -> Phase {
        Phase { km: 0.0, sd: [0.0; 3], tensor: [[0.0; 3]; 3], ftest: [0.0; 3] }
    }
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pj-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn many_pairs_in_folders_become_full_files_elsewhere() {
        let root = tmp("many");
        for (d, n) in [("ZZ01", "ZZ01"), ("ZZ02", "ZZ02")] {
            std::fs::create_dir_all(root.join(d)).unwrap();
            let mut a = build_record_with(&format!("{n}-A1"), &ph(2e-3, 1.0), &zero(), 0.0, 0.0, "");
            a.extend(build_record_with(&format!("{n}-B1"), &ph(2e-3, 0.9), &zero(), 0.0, 0.0, ""));
            let mut b = build_record_with(&format!("{n}-A1_Im"), &ph(-1e-6, 1.1), &zero(), 0.0, 0.0, "");
            b.extend(build_record_with(&format!("{n}-B1_Im"), &ph(-2e-6, 1.0), &zero(), 0.0, 0.0, ""));
            std::fs::write(root.join(d).join(format!("{n}.ams")), a).unwrap();
            std::fs::write(root.join(d).join(format!("{n}_Im.ams")), b).unwrap();
        }
        let raw = specset::scan_raw(&root);
        let rows = find_pairs(&raw, Kind::Ams, false);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.ip.is_some() && r.op.is_some()));
        let out = tmp("many-out");
        let rep = save_pairs(&rows, &out).unwrap();
        assert_eq!((rep.written.len(), rep.records), (2, 4));
        let s = parse(&std::fs::read(out.join("ZZ01.ams")).unwrap()).unwrap();
        assert_eq!(s[1].name, "ZZ01-B1");
        assert!((s[1].outphase.km + 2e-6).abs() < 1e-12 && (s[1].inphase.km - 2e-3).abs() < 1e-9);
        // never into a source folder
        assert!(save_pairs(&rows, &root.join("ZZ01")).is_err());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn two_files_without_phase_words_are_paired_by_their_numbers() {
        let root = tmp("content");
        let a = build_record_with("S1", &ph(2e-3, 1.0), &zero(), 0.0, 0.0, "");
        let b = build_record_with("S1", &ph(-3e-6, 1.0), &zero(), 0.0, 0.0, "");
        std::fs::write(root.join("first.ams"), a).unwrap();
        std::fs::write(root.join("second.ams"), b).unwrap();
        let raw = specset::scan_raw(&root);
        assert!(find_pairs(&raw, Kind::Ams, false).iter().all(|r| r.op.is_none()));
        let rows = find_pairs(&raw, Kind::Ams, true);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].ip.as_ref().unwrap().ends_with("first.ams") && rows[0].op.as_ref().unwrap().ends_with("second.ams"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_picked_in_phase_file_finds_its_partner_and_roles_are_checked() {
        let root = tmp("one");
        let a = build_record_with("S1", &ph(2e-3, 1.0), &zero(), 0.0, 0.0, "");
        let b = build_record_with("S1", &ph(-3e-6, 1.0), &zero(), 0.0, 0.0, "");
        std::fs::write(root.join("S1.ams"), a).unwrap();
        std::fs::write(root.join("S1_oop.ams"), b).unwrap();
        let mut pj = PhaseJoin::default();
        pj.set_ip(root.join("S1.ams"));
        assert_eq!(pj.op.as_ref().map(|p| file_name(p)), Some("S1_oop.ams".to_string()));
        assert!(matches!(&pj.preview, Some(Ok((_, m))) if m.joined == 1));
        assert!(pj.warn.is_empty());
        pj.swap();
        assert!(!pj.warn.is_empty(), "the wrong way round is noticed");
        let _ = std::fs::remove_dir_all(&root);
    }
}
