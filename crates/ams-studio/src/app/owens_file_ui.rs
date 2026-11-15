//! Spec to Geo (Owens) with the values read from a spreadsheet or csv, for the open file ("single") or for the
//! specimen folders of its parent tree ("multiple"). See `ams_core::owens_table` for the layout of the table.
//!
//! Single rotates the data of the open file in memory. Multiple rotates the main files of the specimen folders in
//! memory too: the rotation of each file is remembered (`App::pending`) and applied when the file is opened.
//! Nothing is saved by Rotate; the files are selected in the file browser and written with File > Export as .ams.

use super::{owens_head, App, W_OWENS};
use ams_core::ams;
use ams_core::coords::owens_rotation;
use ams_core::owens_table::{self, Table, TableError, Via, ROLES};
use ams_core::specset::{self, Pick};
use egui::{Align, Color32, Layout, Pos2, RichText, Stroke, Vec2};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Stage {
    /// The manual input of the Owens window.
    #[default]
    Manual,
    /// "This file" / "Multiple specimens" is being chosen.
    Scope,
    /// For "Multiple": which files get the correction (selected in the file tree, or picked in a file dialog).
    Targets,
    /// A file is chosen: layout guide, list, Rotate.
    File,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Scope {
    Single,
    #[default]
    Multiple,
}

/// One file the correction goes to, with the row of the table every record gets.
pub(super) struct Target {
    /// Specimen (folder) name; for a single file the file name.
    pub label: String,
    pub path: Option<PathBuf>,
    /// Folder relative to the parent tree (for the mirrored output).
    pub rel_dir: String,
    pub file_name: String,
    pub records: Vec<(String, Option<(usize, Via)>)>,
}

#[derive(Default)]
pub(super) struct OwensFile {
    pub stage: Stage,
    pub scope: Option<Scope>,
    /// The select boxes of the first two steps.
    pub scope_pick: Scope,
    /// Second step: the file dialog (true) or the selection in the file tree (false).
    pub targets_dialog: bool,
    /// The pointer rested on the tree choice: everything but the tree stays dark until the next step.
    pub tree_latch: bool,
    pub path: Option<PathBuf>,
    pub sheet: String,
    pub table: Option<Table>,
    pub error: Option<TableError>,
    pub plan: Vec<Target>,
    /// Parent folder of a multiple correction and the identifier of the main files.
    pub root: Option<PathBuf>,
    /// The files chosen for a multiple correction (tree selection or file dialog); `None` = every specimen folder of the parent tree.
    pub chosen: Option<Vec<PathBuf>>,
    pub ident: String,
    pub note: String,
    pub msg: String,
    /// What the last correction from a file was, so that *Undo last correction* can name it.
    pub last: Option<Scope>,
    /// The rotations that were pending before the last multiple correction (for Undo).
    pub undo_pending: Option<std::collections::HashMap<PathBuf, Vec<Option<ams_core::tensor::Mat3>>>>,
    /// The Owens-rotated marks before the last multiple correction (for Undo).
    pub undo_done: Option<std::collections::HashMap<PathBuf, Vec<bool>>>,
    /// The edited files that were kept aside, as they were before the last multiple correction.
    pub undo_stash: Vec<(PathBuf, Vec<u8>, Vec<ams::Specimen>)>,
    /// The open file was one of the targets of the last multiple correction.
    pub open_rotated: bool,
}

impl Scope {
    fn word(self) -> &'static str {
        match self {
            Scope::Single => "single",
            Scope::Multiple => "multiple",
        }
    }
}

/// Reads the first sheet (xlsx / xls / ods) or the csv that holds a usable table.
fn load_table(path: &Path) -> (Option<Table>, Option<TableError>, String) {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let fail = |m: String| (None, Some(TableError { message: m, found: Default::default() }), String::new());
    if ext == "csv" || ext == "txt" || ext == "tsv" {
        return match std::fs::read(path) {
            Ok(b) => match owens_table::from_grid(&owens_table::grid_from_csv(&String::from_utf8_lossy(&b))) {
                Ok(t) => (Some(t), None, String::new()),
                Err(e) => (None, Some(e), String::new()),
            },
            Err(e) => fail(format!("Could not read the file: {e}")),
        };
    }
    use calamine::{open_workbook_auto, Data, Reader};
    let mut wb = match open_workbook_auto(path) {
        Ok(w) => w,
        Err(e) => return fail(format!("Could not open the spreadsheet: {e}")),
    };
    let mut first_err: Option<(TableError, String)> = None;
    for name in wb.sheet_names().to_vec() {
        let Ok(range) = wb.worksheet_range(&name) else { continue };
        let grid: Vec<Vec<String>> = range
            .rows()
            .map(|r| {
                r.iter()
                    .map(|c| match c {
                        Data::Empty => String::new(),
                        Data::Float(f) => f.to_string(),
                        Data::Int(i) => i.to_string(),
                        other => other.to_string(),
                    })
                    .collect()
            })
            .collect();
        match owens_table::from_grid(&grid) {
            Ok(t) => return (Some(t), None, name),
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some((e, name));
                }
            }
        }
    }
    match first_err {
        Some((e, n)) => (None, Some(e), n),
        None => fail("The spreadsheet has no readable sheet.".into()),
    }
}

/// Names a file may be listed under in the table: with and without the main-file identifier, and its folder.
fn ids_for(file_name: &str, folder: &str, ident: &str) -> Vec<String> {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s).to_string();
    let mut v = vec![stem.clone(), folder.rsplit('/').next().unwrap_or(folder).to_string()];
    if !ident.is_empty() {
        if let Some(p) = stem.to_lowercase().find(&ident.to_lowercase()) {
            let cut = format!("{}{}", &stem[..p], &stem[p + ident.len()..]);
            v.push(cut.trim_matches(|c: char| c == '_' || c == '-' || c == ' ').to_string());
        }
    }
    v
}

impl App {
    /// Back to the very beginning of the Owens window: nothing of a "from file" correction in progress is kept (the
    /// way back from a correction that was already done stays).
    pub(super) fn reset_owens_file(&mut self) {
        self.of.stage = Stage::Manual;
        self.of.scope = None;
        self.of.path = None;
        self.of.sheet.clear();
        self.of.table = None;
        self.of.error = None;
        self.of.plan.clear();
        self.of.chosen = None;
        self.of.root = None;
        self.of.ident.clear();
        self.of.note.clear();
        self.of.msg.clear();
    }

    /// Closes the Owens window (Cancel, Esc, the close button) and starts over next time.
    pub(super) fn close_owens(&mut self) {
        self.win_owens = false;
        self.reset_owens_file();
    }

    /// Chooses the file (a dialog), reads it and lists what it would do.
    pub(super) fn of_select_file(&mut self) {
        let mut dlg = rfd::FileDialog::new().add_filter("Spreadsheet or csv", &["xlsx", "xlsm", "xls", "ods", "csv"]).set_title("Orientation values for the Owens correction");
        if let Some(p) = self.of.path.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(p);
        } else if self.browser_dir.is_dir() {
            dlg = dlg.set_directory(&self.browser_dir);
        }
        // headless test hook: AMS_STUDIO_PICK_FILE names the file instead of the dialog
        let picked = std::env::var_os("AMS_STUDIO_PICK_FILE").map(PathBuf::from).or_else(|| dlg.pick_file());
        let Some(p) = picked else {
            if self.of.path.is_none() {
                self.of.stage = Stage::Manual;
            }
            return;
        };
        let (t, e, sheet) = load_table(&p);
        self.of.path = Some(p);
        self.of.sheet = sheet;
        self.of.table = t;
        self.of.error = e;
        self.of.msg.clear();
        self.of.stage = Stage::File;
        self.of_build_plan();
    }

    /// Which records of which files get which row.
    pub(super) fn of_build_plan(&mut self) {
        self.of.plan.clear();
        self.of.note.clear();
        let Some(table) = self.of.table.clone() else { return };
        let Some(scope) = self.of.scope else { return };
        let Some(d) = &self.doc else { return };
        match scope {
            Scope::Single => {
                let file_name = d.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let folder = d.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let names: Vec<String> = d.specs.iter().map(|s| s.name.clone()).collect();
                let ident = self.set.as_ref().map(|s| s.ident.clone()).unwrap_or_else(|| self.last_ident.clone());
                let a = owens_table::assign(&table, &names, &ids_for(&file_name, &folder, &ident));
                self.of.plan.push(Target { label: file_name.clone(), path: Some(d.path.clone()), rel_dir: String::new(), file_name, records: names.into_iter().zip(a).collect()});
            }
            Scope::Multiple if self.of.chosen.is_some() => {
                // the files that were chosen in the file tree or in a file dialog
                let files = self.of.chosen.clone().unwrap_or_default();
                let ident = self.set.as_ref().map(|s| s.ident.clone()).unwrap_or_else(|| self.last_ident.clone());
                let root = self.set.as_ref().map(|s| s.root.clone());
                self.of.root = root.clone();
                self.of.ident = ident.clone();
                for path in files {
                    let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                    let rel = root.as_ref().and_then(|r| dir.strip_prefix(r).ok()).map(|p| p.display().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                    let names: Vec<String> = std::fs::read(&path).ok().and_then(|b| ams::parse(&b).ok()).map(|s| s.into_iter().map(|x| x.name).collect()).unwrap_or_default();
                    if names.is_empty() {
                        continue;
                    }
                    let a = owens_table::assign(&table, &names, &ids_for(&file_name, &rel, &ident));
                    self.of.plan.push(Target { label: rel.clone(), path: Some(path), rel_dir: rel, file_name, records: names.into_iter().zip(a).collect() });
                }
            }
            Scope::Multiple => {
                // the folders and main files: the open specimen set, else the parent of this file's folder
                let (root, folders, picks, ident) = if let Some(s) = &self.set {
                    (s.root.clone(), s.folders.clone(), s.picks.clone(), s.ident.clone())
                } else if let Some(root) = d.path.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()) {
                    let folders = specset::scan(&root);
                    let det = specset::detect(&folders, Some(&self.last_ident));
                    if det.needed && det.ident.is_none() {
                        self.of.root = Some(root);
                        self.of.note = "The main file of each specimen could not be told apart in the folders next to this file's folder. Open the folder of specimens first (File > Open Folder of Specimens), choose the identifier there, then come back.".into();
                        return;
                    }
                    let ident = det.ident.clone().unwrap_or_default();
                    let picks = specset::matches(&folders, &ident);
                    (root, folders, picks, ident)
                } else {
                    self.of.note = "This file is not inside a parent folder.".into();
                    return;
                };
                self.of.root = Some(root.clone());
                self.of.ident = ident.clone();
                for (f, p) in folders.iter().zip(&picks) {
                    let leaf = f.name.rsplit('/').next().unwrap_or(&f.name);
                    if leaf == "_geo" || leaf == "_raw" || leaf == "_corr" {
                        continue; // exported results
                    }
                    let Some(k) = p.file() else { continue };
                    let path = f.dir.join(&f.files[k]);
                    let file_name = f.files[k].clone();
                    let names: Vec<String> = std::fs::read(&path).ok().and_then(|b| ams::parse(&b).ok()).map(|s| s.into_iter().map(|x| x.name).collect()).unwrap_or_default();
                    if names.is_empty() {
                        continue;
                    }
                    let a = owens_table::assign(&table, &names, &ids_for(&file_name, &f.name, &ident));
                    self.of.plan.push(Target { label: f.name.clone(), path: Some(path), rel_dir: f.name.clone(), file_name, records: names.into_iter().zip(a).collect() });
                }
                let _ = Pick::None;
            }
        }
    }

    /// Specimens (files) that get at least one record rotated, and the records.
    fn of_counts(&self) -> (usize, usize) {
        let files = self.of.plan.iter().filter(|t| t.records.iter().any(|r| r.1.is_some())).count();
        let recs = self.of.plan.iter().map(|t| t.records.iter().filter(|r| r.1.is_some()).count()).sum();
        (files, recs)
    }

    fn of_apply(&mut self) -> bool {
        let Some(table) = self.of.table.clone() else { return false };
        let Some(scope) = self.of.scope else { return false };
        let mats = |t: &Target| -> Vec<(usize, ams_core::tensor::Mat3)> {
            t.records.iter().enumerate().filter_map(|(i, r)| r.1.map(|(ri, _)| (i, { let v = table.rows[ri].vals; owens_rotation(v[0], v[1], v[2], v[3]) }))).collect()
        };
        match scope {
            Scope::Single => {
                let Some(target) = self.of.plan.first() else { return false };
                let jobs = mats(target);
                if jobs.is_empty() {
                    return false;
                }
                if let Err(e) = self.rotate_doc(&jobs) {
                    self.info = Some(e);
                    return false;
                }
                self.of.last = Some(Scope::Single);
                self.of.msg = format!("Rotated {} record(s) of this file. Nothing is saved yet: File > Export as .ams (or Save Active Records As) writes it.", jobs.len());
            }
            Scope::Multiple => {
                // the whole correction is refused when any target holds a sample that is already Owens-rotated
                let mut problems: Vec<String> = vec![];
                for t in &self.of.plan {
                    if let Some(p) = &t.path {
                        let idx: Vec<usize> = mats(t).iter().map(|j| j.0).collect();
                        let names: Vec<String> = t.records.iter().map(|r| r.0.clone()).collect();
                        if let Err(e) = self.owens_check(p, idx.into_iter(), Some(&names)) {
                            problems.push(e.lines().next().unwrap_or("").to_string());
                        }
                    }
                }
                if !problems.is_empty() {
                    let more = if problems.len() > 6 { format!("\n... and {} more file(s)", problems.len() - 6) } else { String::new() };
                    self.info = Some(format!("{}{}\nNothing was rotated.", problems.iter().take(6).cloned().collect::<Vec<_>>().join("\n"), more));
                    return false;
                }
                self.of.undo_done = Some(self.owens_done.clone());
                self.of.undo_pending = Some(self.pending.clone());
                self.of.undo_stash = self.stash.iter().map(|(p, d)| (p.clone(), d.raw.clone(), d.specs.clone())).collect();
                self.of.open_rotated = false;
                let open_path = self.doc.as_ref().filter(|d| !d.synthetic).map(|d| d.path.clone());
                let (mut files, mut recs) = (0, 0);
                let targets: Vec<(PathBuf, usize, Vec<(usize, ams_core::tensor::Mat3)>)> = self.of.plan.iter().filter_map(|t| t.path.clone().map(|p| (p, t.records.len(), mats(t)))).filter(|(_, _, j)| !j.is_empty()).collect();
                for (path, n, jobs) in targets {
                    recs += jobs.len();
                    files += 1;
                    if open_path.as_ref() == Some(&path) {
                        let _ = self.rotate_doc(&jobs); // the open file is rotated on screen too (checked above)
                        self.of.open_rotated = true;
                    } else {
                        self.add_pending(&path, n, &jobs);
                    }
                }
                self.of.last = Some(Scope::Multiple);
                self.of.msg = format!("Rotated {recs} record(s) of {files} file(s) in memory. Nothing is saved yet: select the files in the browser and use File > Export as .ams.");
            }
        }
        true
    }

    /// Undo of the last correction from a file: the open file's data (single) or the pending rotations (multiple).
    pub(super) fn of_undo(&mut self) {
        match self.of.last {
            Some(Scope::Single) => {
                self.undo_doc_rotation();
                self.of.msg = "The last correction of this file was undone.".into();
            }
            Some(Scope::Multiple) => {
                if self.of.open_rotated {
                    self.undo_doc_rotation();
                    self.of.open_rotated = false;
                }
                if let Some(p) = self.of.undo_pending.take() {
                    self.pending = p;
                }
                if let Some(d) = self.of.undo_done.take() {
                    self.owens_done = d;
                }
                for (path, raw, specs) in std::mem::take(&mut self.of.undo_stash) {
                    if let Some(d) = self.stash.get_mut(&path) {
                        d.raw = raw;
                        d.specs = specs;
                    }
                }
                self.of.msg = "The last multiple correction was undone: the rotations were taken back.".into();
            }
            None => {}
        }
        self.of.last = None;
    }

    /// The Owens window while a table from a file is being used.
    pub(super) fn owens_file_window(&mut self, ctx: &egui::Context) {
        let dark = self.dark;
        let accent = if dark { Color32::from_rgb(106, 160, 255) } else { Color32::from_rgb(47, 111, 222) };
        let ok = Color32::from_rgb(70, 170, 90);
        let bad = Color32::from_rgb(214, 90, 90);
        let warn = Color32::from_rgb(214, 150, 40);
        let now = ctx.input(|i| i.time);
        let toast = self.owens_toast.clone();
        let mut old = self.owens_old;
        let stage = self.of.stage;
        let scope = self.of.scope;
        let last = self.of.last;
        let (files_n, recs_n) = self.of_counts();
        let (mut open, mut cancel, mut switched) = (true, false, false);
        let (mut back, mut select, mut rotate, mut undo) = (false, false, false, false);
        let mut back_arrow = false;
        let mut scope_pick = self.of.scope_pick;
        let mut targets_dialog = self.of.targets_dialog;
        let mut latch = self.of.tree_latch && stage == Stage::Targets;
        let (mut next, mut quick_all) = (false, false);
        let n_tree = self.tree_sel.iter().filter(|p| crate::doc::is_ams(p)).count();
        // roomier than the manual input: the questions and the checks of the file need space to breathe
        let cr = ctx.content_rect();
        // it opens to the right of the file tree, so the tree stays free for selecting files
        let tree_right = if self.tree_rect.is_positive() { self.tree_rect.max.x + 14.0 } else { 8.0 };
        let win_w = (cr.width() - tree_right - 12.0).clamp(620.0, 960.0).min(cr.width() - 16.0);
        let win_size = Vec2::new(win_w, (cr.height() - 60.0).clamp(640.0, 880.0));
        let win_x = tree_right.min(cr.max.x - win_w - 8.0).max(8.0);
        self.base_window(ctx, "Specimen to Geographic Rotation (Owens' method)", W_OWENS, win_size).default_pos(Pos2::new(win_x, 40.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            super::tools_ui::pop_style(ui, 1.3);
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 14.0);
            ui.spacing_mut().button_padding = Vec2::new(12.0, 6.0);
            let (sw, bk) = owens_head(ui, &mut old, &toast, now, accent, stage != Stage::Manual, stage == Stage::Manual);
            switched = sw;
            back_arrow = bk;
            ui.separator();
            // ---- the question comes first, the choice under it
            match stage {
                Stage::Scope => {
                    ui.label(RichText::new("Where should the values from the file go?").strong().size(19.0));
                    ui.add_space(2.0);
                }
                Stage::Targets => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Which files should get the correction?").strong().size(19.0));
                        super::info_icon(ui, "Select the files in the file tree on the left, then press Next.\nShift+click selects a range, only the files the identifier matched (the bold main files).\nCtrl+Shift+click selects every file of the range, the grayed ones too.\nCtrl+click adds or removes single files.");
                    });
                    ui.add_space(2.0);
                }
                Stage::File => {
                    // the prompt first, the button under it
                    let has_values = self.of.table.is_some() || self.of.error.is_some();
                    ui.label(RichText::new(if has_values { "Spreadsheet or csv with the values (choose another one below):" } else { "Which spreadsheet or csv holds the values (strike and dip per specimen)?" }).strong().size(19.0));
                    ui.add_space(2.0);
                }
                _ => {}
            }
            // ---- the choices are select boxes; the step is left with Next at the bottom right
            if stage == Stage::Scope || stage == Stage::Targets {
                let top = ui.cursor().top();
                if stage == Stage::Scope {
                    // several specimens first: that is what a table of values is for
                    ui.radio_value(&mut scope_pick, Scope::Multiple, RichText::new("Multiple specimens from this parent tree").size(16.0)).on_hover_text("Go through the specimen folders next to this file's folder (or the open set of specimens)");
                    ui.radio_value(&mut scope_pick, Scope::Single, RichText::new("This file (the file that is open now)").size(16.0)).on_hover_text("Correct the file that is open now");
                } else {
                    let label = if latch { format!("Shift/Ctrl select from left pane (n={n_tree})") } else { format!("The files selected in the file tree (n={n_tree})") };
                    let row = ui.horizontal(|ui| {
                        ui.radio_value(&mut targets_dialog, false, RichText::new(label).size(16.0));
                        quick_all = ui.button("Quick select all string matched").on_hover_text("Selects the main file of every specimen folder in the file tree (the bold, string-matched ones)").clicked();
                    });
                    // resting the pointer on this choice darkens everything but the tree; it stays dark until the next step
                    if ui.input(|i| i.pointer.hover_pos()).map_or(false, |p| row.response.rect.contains(p)) && !targets_dialog {
                        latch = true;
                    }
                    ui.radio_value(&mut targets_dialog, true, RichText::new("Choose files in a file dialog (it opens when you press Next)").size(16.0)).on_hover_text("The classic file window: pick one or several .ams files");
                    if targets_dialog {
                        latch = false;
                    }
                }
                // Cancel on the left and Next on the right, at the bottom of the step
                let used = ui.cursor().top() - top;
                ui.add_space((250.0 - used).max(10.0));
                ui.horizontal(|ui| {
                    ui.set_width(ui.available_width());
                    back = ui.button("Cancel").clicked();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let ok = stage == Stage::Scope || targets_dialog || n_tree > 0;
                        let next_btn = egui::Button::new(RichText::new("Next  \u{2192}").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(130.0, 38.0));
                        next = ui.add_enabled(ok, next_btn).on_disabled_hover_text("Select files in the file tree first (or choose the file dialog)").clicked();
                    });
                });
                return;
            }
            // ---- the values file: the prompt above, its button under it
            ui.horizontal(|ui| {
                select = ui.add_sized([240.0, 38.0], egui::Button::new(RichText::new("Select file ...").strong())).on_hover_text("Choose another spreadsheet or csv").clicked();
                if let Some(l) = last {
                    undo = ui.add(egui::Button::new(RichText::new(format!("Undo last correction ({})", l.word())).small()).small().fill(Color32::TRANSPARENT).stroke(ui.visuals().widgets.inactive.bg_stroke)).clicked();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| back = ui.small_button("Back to manual input").clicked());
            });
            let sc = scope.unwrap_or(Scope::Single);
            let fname = self.of.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&fname).strong());
                if !self.of.sheet.is_empty() {
                    ui.label(RichText::new(format!("sheet \u{201c}{}\u{201d}", self.of.sheet)).weak().small());
                }
                ui.label(RichText::new(match sc {
                    Scope::Single => "\u{b7} corrects this file".to_string(),
                    Scope::Multiple => match &self.of.chosen {
                        Some(c) => format!("\u{b7} corrects the {} chosen file(s)", c.len()),
                        None => format!("\u{b7} corrects the specimens of {}", self.of.root.as_ref().map(|r| r.display().to_string()).unwrap_or_default()),
                    },
                }).weak().small());
            });

            // ---- the files are chosen, the values file is not yet
            if self.of.table.is_none() && self.of.error.is_none() {
                if let Some(c) = &self.of.chosen {
                    ui.label(RichText::new(format!("{} file(s) will get the correction:", c.len())).weak().small());
                    for f in c.iter().take(10) {
                        ui.label(RichText::new(f.display().to_string()).monospace().small());
                    }
                    if c.len() > 10 {
                        ui.label(RichText::new(format!("... and {} more", c.len() - 10)).weak().small());
                    }
                } else if self.of.scope == Some(Scope::Single) {
                    ui.label(RichText::new("The open file will get the correction.").weak().small());
                } else {
                    ui.label(RichText::new("Every specimen folder of the parent tree will get the correction.").weak().small());
                }
            }
            // ---- how the file has to look, checked against the file
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("How the file has to look").strong());
                    // the long explanation is a pop-up on the (i), not text in the window
                    let how = match sc {
                    Scope::Single => "Specimen = a record name of this file (ZZ01-A1) or the name of the file / its folder (applies to every record). A file with a single row applies to every record.",
                    Scope::Multiple => "Specimen = the specimen folder or main file name (applies to every record of its main file) or a record name. Rotate only turns the data in memory; nothing is saved. Select the files in the browser and use File > Export as .ams: the corrected file goes into a _geo folder inside the specimen folder.",
                    };
                    super::info_icon(ui, &format!("{}\n\n{}", "One row per specimen. The header can be in any of the first 20 rows; any sheet of a workbook is tried. Also accepted: FS FD DS DD, \u{201c}Strike (LHR)\u{201d} (left-hand rule), \u{201c}Dip Direction\u{201d}, \u{201c}Sample ID\u{201d}. Strikes: index finger along the strike, thumb to the dip direction (0\u{b0} dips to 90\u{b0}); \u{201c}LHR\u{201d} is used as it stands, a dip direction is turned into a strike. Same for the drill table.", how));
                });
                let found = self.of.table.as_ref().map(|t| t.found.clone()).or_else(|| self.of.error.as_ref().map(|e| e.found.clone()));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                    for (i, r) in ROLES.iter().enumerate() {
                        let (fill, tag) = match &found {
                            Some(f) if f[i].is_some() => (ok.gamma_multiply(0.35), format!("\u{2713} {}", f[i].clone().unwrap_or_default())),
                            Some(_) => (bad.gamma_multiply(0.35), "\u{2717} missing".to_string()),
                            None => (accent.gamma_multiply(0.30), String::new()),
                        };
                        egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(8, 4)).corner_radius(4.0).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(r.label()).strong());
                                super::info_icon(ui, match r {
                                    owens_table::Role::Specimen => "Which specimen the row belongs to: a record name from the .ams file (ZZ01-A1), or the name of the file or its folder, which then applies to every record of that file. Headers such as Specimen, Sample ID or Name are found.",
                                    owens_table::Role::FieldStrike => "The strike you measured in the field, with the compass, on a marked reference plane (bedding, a vein, a foliation) before the sample was taken out, 0-360 deg. Convention: index finger along the strike, thumb towards the dip direction. A header with LHR (left-hand rule) is used as it stands; a Dip Direction column is converted to a strike.",
                                    owens_table::Role::FieldDip => "The dip you measured in the field on that same plane before sampling: the angle down from the horizontal, 0-90 deg.",
                                    owens_table::Role::TableStrike => "The strike of that same marked plane measured on the hand sample while it lay on the drilling table, before the core was drilled out, 0-360 deg. Same convention as the field strike.",
                                    owens_table::Role::TableDip => "The dip of that same marked plane on the hand sample on the drilling table, measured before drilling: the angle down from the horizontal, 0-90 deg.",
                                });
                            });
                            if !tag.is_empty() {
                                ui.label(RichText::new(tag).small());
                            }
                        });
                    }
                });
                ui.label(RichText::new("example row:  ZZ01-A1 | 30 | 20 | 45 | 10").monospace().small().weak());
                if sc == Scope::Multiple && self.of.chosen.is_none() {
                    ui.separator();
                    ui.columns(2, |cols| {
                        let mono = |s: String| RichText::new(s).monospace().small();
                        cols[0].label(RichText::new("Required folder structure").strong().small());
                        cols[0].label(mono("parent folder/\n\u{251c}\u{2500} SPECIMEN-1/\n\u{2502}  \u{251c}\u{2500} SPECIMEN-1_use.ams  \u{25cf}\n\u{2502}  \u{2514}\u{2500} other versions.ams\n\u{251c}\u{2500} SPECIMEN-2/\n\u{2502}  \u{2514}\u{2500} SPECIMEN-2_use.ams  \u{25cf}\n\u{2514}\u{2500} \u{2026}".to_string()));
                        cols[0].label(RichText::new("\u{25cf} main file of the specimen").weak().small());
                        cols[0].add_space(4.0);
                        cols[0].label(RichText::new("After Export as .ams").strong().small());
                        cols[0].label(mono("SPECIMEN-1/\n\u{251c}\u{2500} SPECIMEN-1_use.ams  (stays)\n\u{251c}\u{2500} _geo/  SPECIMEN-1_use_geo.ams\n\u{2514}\u{2500} other files stay here".to_string()));
                        let root_name = self.of.root.as_ref().and_then(|r| r.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or("parent folder".into());
                        cols[1].label(RichText::new("Found under your parent folder").strong().small());
                        let mut s = format!("{root_name}/\n");
                        let shown: Vec<&Target> = self.of.plan.iter().take(4).collect();
                        for (k, t) in shown.iter().enumerate() {
                            let last_one = k + 1 == shown.len() && self.of.plan.len() <= 4;
                            s += &format!("{} {}/\n", if last_one { "\u{2514}\u{2500}" } else { "\u{251c}\u{2500}" }, t.rel_dir);
                            s += &format!("{}  \u{2514}\u{2500} {}  \u{25cf}\n", if last_one { " " } else { "\u{2502}" }, t.file_name);
                        }
                        if self.of.plan.len() > 4 {
                            s += &format!("\u{2514}\u{2500} \u{2026} {} more\n", self.of.plan.len() - 4);
                        }
                        if self.of.plan.is_empty() {
                            s += "(no specimen folders with a main file)\n";
                        }
                        cols[1].label(mono(s));
                        if !self.of.ident.is_empty() {
                            cols[1].label(RichText::new(format!("main file = name contains \u{201c}{}\u{201d}", self.of.ident)).weak().small());
                        }
                    });
                }
            });
            if !self.of.note.is_empty() {
                ui.label(RichText::new(&self.of.note).color(warn));
            }

            // ---- the file could not be used
            if let Some(e) = &self.of.error {
                ui.label(RichText::new(&e.message).color(bad).strong());
                ui.label(RichText::new("Fix the file (or pick another one with Select file ...); the columns marked above are the ones that are missing.").weak().small());
            }
            // ---- what would be corrected
            if let Some(t) = &self.of.table {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(match sc {
                        Scope::Single => "Records of this file that get the correction".to_string(),
                        Scope::Multiple => "Specimens that get the correction".to_string(),
                    }).strong());
                    let total = match sc {
                        Scope::Single => self.of.plan.first().map_or(0, |p| p.records.len()),
                        Scope::Multiple => self.of.plan.len(),
                    };
                    let n = if sc == Scope::Single { recs_n } else { files_n };
                    ui.label(RichText::new(format!("{n} of {total}")).small().color(if n == total && total > 0 { ok } else { warn }));
                    if t.skipped > 0 {
                        ui.label(RichText::new(format!("{} row(s) of the file skipped (a number was missing)", t.skipped)).small().color(warn));
                    }
                });
                let h = (ui.available_height() - 104.0).max(120.0);
                egui::ScrollArea::both().id_salt("of-list").max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("of-list-grid").striped(true).num_columns(8).spacing([14.0, 5.0]).show(ui, |ui| {
                        let heads: &[&str] = if sc == Scope::Single { &["#", "Record", "FS", "FD", "DS", "DD", "Row of the file"] } else { &["#", "Specimen", "Main file", "Records", "FS", "FD", "DS", "DD"] };
                        for h in heads {
                            ui.label(RichText::new(*h).strong());
                        }
                        ui.end_row();
                        let fmt = |v: f64| format!("{v:.1}");
                        let mut k = 0;
                        if sc == Scope::Single {
                            for tg in &self.of.plan {
                                for (name, a) in &tg.records {
                                    k += 1;
                                    ui.label(RichText::new(k.to_string()).weak());
                                    ui.label(name);
                                    match a {
                                        Some((ri, via)) => {
                                            for v in t.rows[*ri].vals {
                                                ui.label(fmt(v));
                                            }
                                            ui.label(RichText::new(format!("{}{}", t.rows[*ri].name, match via { Via::Record => "", Via::Specimen => "  (whole file)", Via::OnlyRow => "  (only row)" })).small().weak());
                                        }
                                        None => {
                                            for _ in 0..4 {
                                                ui.label(RichText::new("\u{2014}").weak());
                                            }
                                            ui.label(RichText::new("no row for this record: not rotated").small().color(bad));
                                        }
                                    }
                                    ui.end_row();
                                }
                            }
                        } else {
                            for tg in &self.of.plan {
                                k += 1;
                                let n_rot = tg.records.iter().filter(|r| r.1.is_some()).count();
                                ui.label(RichText::new(k.to_string()).weak());
                                ui.label(&tg.label);
                                ui.label(RichText::new(&tg.file_name).small());
                                ui.label(RichText::new(format!("{n_rot} of {}", tg.records.len())).color(if n_rot == tg.records.len() { ok } else if n_rot == 0 { bad } else { warn }));
                                let rows: Vec<usize> = tg.records.iter().filter_map(|r| r.1.map(|x| x.0)).collect();
                                match rows.first() {
                                    Some(&r0) if rows.iter().all(|r| *r == r0) => {
                                        for v in t.rows[r0].vals {
                                            ui.label(fmt(v));
                                        }
                                    }
                                    Some(_) => {
                                        for _ in 0..4 {
                                            ui.label(RichText::new("varies").weak().small());
                                        }
                                    }
                                    None => {
                                        ui.label(RichText::new("no row in the file: skipped").small().color(bad));
                                        for _ in 0..3 {
                                            ui.label("");
                                        }
                                    }
                                }
                                ui.end_row();
                            }
                            if self.of.plan.is_empty() {
                                ui.label("");
                                ui.label(RichText::new("No specimen folder with a main file was found.").color(bad));
                                ui.end_row();
                            }
                        }
                    });
                });
            }
            // ---- actions
            ui.horizontal(|ui| {
                let go = egui::Button::new(RichText::new("Rotate").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(110.0, 0.0));
                let ready = self.of.table.is_some() && recs_n > 0;
                rotate = ui.add_enabled(ready, go).on_hover_text(match sc {
                    Scope::Single => "Rotate the records of the open file",
                    Scope::Multiple => "Rotate the main files in memory (nothing is written)",
                }).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| cancel = ui.button("Cancel").clicked());
            });
            if !self.of.msg.is_empty() {
                ui.label(RichText::new(&self.of.msg).color(ok).small());
            }
        });
        if switched {
            // the manual / new / classic switch takes the window back to the manual input
            self.owens_old = old;
            self.of.stage = Stage::Manual;
            self.save_state();
        }
        if back_arrow {
            // one step back through the questions
            self.of.stage = match (stage, scope) {
                (Stage::File, Some(Scope::Multiple)) => Stage::Targets,
                (Stage::File, _) | (Stage::Targets, _) => Stage::Scope,
                (Stage::Scope, _) => Stage::Manual,
                (st, _) => st,
            };
            self.of.plan.clear();
        }
        self.of.scope_pick = scope_pick;
        self.of.targets_dialog = targets_dialog;
        self.of.tree_latch = latch && stage == Stage::Targets && !targets_dialog;
        if quick_all {
            self.select_all_matched();
        }
        if next && stage == Stage::Scope {
            // the first question is answered: for several specimens the files come next, for this file the values
            self.of.scope = Some(scope_pick);
            self.of.chosen = None;
            self.of.plan.clear();
            self.of.msg.clear();
            self.of.table = None;
            self.of.error = None;
            self.of.path = None;
            self.of.stage = if scope_pick == Scope::Multiple { Stage::Targets } else { Stage::File };
            self.of.tree_latch = false;
        }
        if next && stage == Stage::Targets {
            let files: Option<Vec<PathBuf>> = if targets_dialog {
                let mut dlg = rfd::FileDialog::new().add_filter("AMS files", &["ams"]).set_title("Files that get the Owens correction");
                if self.browser_dir.is_dir() {
                    dlg = dlg.set_directory(&self.browser_dir);
                }
                // headless test hook: AMS_STUDIO_PICK_AMS lists the files (separated by ;) instead of the dialog
                std::env::var("AMS_STUDIO_PICK_AMS").ok().map(|v| v.split(';').map(PathBuf::from).collect::<Vec<_>>()).or_else(|| dlg.pick_files()).filter(|v| !v.is_empty())
            } else {
                Some(self.tree_sel.iter().filter(|p| crate::doc::is_ams(p)).cloned().collect())
            };
            if let Some(v) = files {
                // the files are chosen; the values file is the next step (a values file that was in use stays)
                self.of.scope = Some(Scope::Multiple);
                self.of.chosen = Some(v);
                self.of.plan.clear();
                self.of.msg.clear();
                self.of.stage = Stage::File;
                self.of.tree_latch = false;
                if self.of.table.is_some() {
                    self.of_build_plan();
                }
            }
        }
        if back {
            self.of.stage = Stage::Manual;
        }
        if select {
            self.of_select_file();
        }
        if undo {
            self.of_undo();
        }
        if rotate {
            // a refused correction (a sample that is already Owens-rotated) leaves the list as it is
            let done = self.of_apply();
            if !done {
                // nothing changed
            } else if self.of.scope == Some(Scope::Multiple) {
                // the rotations are held; a second Rotate on the same list would rotate again
                self.of.plan.clear();
                self.of.note = "Done. Choose Select file ... to prepare another correction.".into();
            } else {
                self.of_build_plan();
            }
        }
        // while the pointer is over "the files selected in the file tree" everything but the tree is dimmed
        self.tree_hint = self.of.stage == Stage::Targets && self.of.tree_latch && self.tree_rect.is_positive();
        if self.tree_hint {
            let t = self.tree_rect;
            let scr = ctx.content_rect();
            let dim = Color32::from_black_alpha(165);
            let pa = ctx.layer_painter(egui::LayerId::background());
            pa.rect_filled(egui::Rect::from_min_max(Pos2::new(t.max.x, scr.min.y), scr.max), 0.0, dim);
            pa.rect_filled(egui::Rect::from_min_max(scr.min, Pos2::new(t.max.x, t.min.y)), 0.0, dim);
            pa.rect_filled(egui::Rect::from_min_max(Pos2::new(scr.min.x, t.max.y), Pos2::new(t.max.x, scr.max.y)), 0.0, dim);
            pa.rect_stroke(t.shrink(1.0), 4.0, Stroke::new(2.5, accent), egui::StrokeKind::Inside);
            ctx.request_repaint();
        }
        if !(open && !cancel) {
            self.tree_hint = false;
            self.close_owens();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up workbook that uses the lab template's headers (Block Sample/Sub spec, Strike (LHR), Dip, ...).
    #[test]
    fn reads_a_workbook_with_the_lab_headers() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/orient_lab_layout.xlsx");
        let (t, e, sheet) = load_table(&p);
        let t = t.unwrap_or_else(|| panic!("{:?}", e));
        assert_eq!(sheet, "Output");
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].name, "ZZ01");
        // Strike (LHR) as the lab uses it (thumb to the dip direction): the numbers stay
        assert_eq!(t.rows[0].vals, [10.0, 40.0, 350.0, 30.0]);
    }

    #[test]
    fn a_csv_and_a_missing_file_are_handled() {
        let dir = std::env::temp_dir().join(format!("of-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("o.csv");
        std::fs::write(&p, "Specimen,FS,FD,DS,DD\nA,1,2,3,4\n").unwrap();
        assert_eq!(load_table(&p).0.unwrap().rows[0].vals, [1.0, 2.0, 3.0, 4.0]);
        let (t, e, _) = load_table(&dir.join("nope.xlsx"));
        assert!(t.is_none() && e.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identifiers_for_a_file() {
        assert_eq!(ids_for("ZZ01_use.ams", "A/ZZ01", "_use"), ["ZZ01_use", "ZZ01", "ZZ01"]);
    }
}
