//! Spec to Geo (Owens) with the values read from a spreadsheet or csv, for the open file ("single") or for the
//! specimen folders of its parent tree ("multiple"). See `ams_core::owens_table` for the layout of the table.
//!
//! Single rotates the data of the open file in memory (like the manual input; *Save Active Records As* keeps it).
//! Multiple changes the file tree, after the user has been told: in each specimen folder the source file is moved
//! into a new sub-folder `_raw` and the corrected file `<name>_corr.ams` is written into a new sub-folder `_corr`;
//! other files stay in the specimen folder. Nothing is overwritten, and *Undo* puts everything back.

use super::{owens_head, App, W_OWENS};
use ams_core::ams;
use ams_core::coords::owens_rotation;
use ams_core::owens_table::{self, Table, TableError, Via, ROLES};
use ams_core::specset::{self, Pick};
use egui::{Align, Color32, Layout, RichText, Vec2};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Stage {
    /// The manual input of the Owens window.
    #[default]
    Manual,
    /// "This file" / "Multiple specimens" is being chosen.
    Scope,
    /// A file is chosen: layout guide, list, Rotate.
    File,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    Single,
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
    pub path: Option<PathBuf>,
    pub sheet: String,
    pub table: Option<Table>,
    pub error: Option<TableError>,
    pub plan: Vec<Target>,
    /// Parent folder of a multiple correction and the identifier of the main files.
    pub root: Option<PathBuf>,
    pub ident: String,
    pub note: String,
    pub msg: String,
    /// What the last correction from a file was, so that *Undo last correction* can name it.
    pub last: Option<Scope>,
    /// What the last multiple correction did, to undo it.
    pub ops: Vec<MoveOp>,
    /// The "files will be moved" prompt is showing.
    pub confirm: bool,
}

/// One specimen of a multiple correction: the source moved to `raw`, the corrected file written to `corr`.
pub(super) struct MoveOp {
    pub orig: PathBuf,
    pub raw: PathBuf,
    pub corr: PathBuf,
    pub corr_len: u64,
    pub new_dirs: Vec<PathBuf>,
}

/// Name of the corrected file: the source's name with `_corr` before the extension.
fn corr_name(file_name: &str) -> String {
    match file_name.rsplit_once('.') {
        Some((s, e)) => format!("{s}_corr.{e}"),
        None => format!("{file_name}_corr"),
    }
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
                self.of.plan.push(Target { label: file_name.clone(), path: Some(d.path.clone()), rel_dir: String::new(), file_name, records: names.into_iter().zip(a).collect() });
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
                    if leaf == "_raw" || leaf == "_corr" {
                        continue; // results of an earlier correction
                    }
                    let Some(k) = p.file() else { continue };
                    let path = f.dir.join(&f.files[k]);
                    let names: Vec<String> = std::fs::read(&path).ok().and_then(|b| ams::parse(&b).ok()).map(|s| s.into_iter().map(|x| x.name).collect()).unwrap_or_default();
                    if names.is_empty() {
                        continue;
                    }
                    let a = owens_table::assign(&table, &names, &ids_for(&f.files[k], &f.name, &ident));
                    self.of.plan.push(Target { label: f.name.clone(), path: Some(path), rel_dir: f.name.clone(), file_name: f.files[k].clone(), records: names.into_iter().zip(a).collect() });
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

    fn of_apply(&mut self) {
        let Some(table) = self.of.table.clone() else { return };
        let Some(scope) = self.of.scope else { return };
        match scope {
            Scope::Single => {
                let Some(target) = self.of.plan.first() else { return };
                let jobs: Vec<(usize, [f64; 4])> = target.records.iter().enumerate().filter_map(|(i, r)| r.1.map(|(ri, _)| (i, table.rows[ri].vals))).collect();
                if jobs.is_empty() {
                    return;
                }
                if let Some(d) = &mut self.doc {
                    self.owens_undo = Some((d.raw.clone(), d.specs.clone()));
                    for (i, v) in &jobs {
                        d.rotate_specimen(*i, &owens_rotation(v[0], v[1], v[2], v[3]));
                    }
                    self.dirty = true;
                    self.of.last = Some(Scope::Single);
                    self.of.msg = format!("Rotated {} record(s) of this file. Use File > Save Active Records As to keep them.", jobs.len());
                }
            }
            Scope::Multiple => {
                let (mut ops, mut recs, mut skipped) = (vec![], 0usize, vec![]);
                for t in &self.of.plan {
                    let Some(src) = &t.path else { continue };
                    if t.records.iter().all(|r| r.1.is_none()) {
                        continue;
                    }
                    let Some(dir) = src.parent() else { continue };
                    let (raw_dir, corr_dir) = (dir.join("_raw"), dir.join("_corr"));
                    let (raw, corr) = (raw_dir.join(&t.file_name), corr_dir.join(corr_name(&t.file_name)));
                    if raw.exists() || corr.exists() {
                        skipped.push(format!("{} (already corrected)", t.label));
                        continue;
                    }
                    let Ok(mut bytes) = std::fs::read(src) else {
                        skipped.push(format!("{} (could not be read)", t.label));
                        continue;
                    };
                    let mut n = 0;
                    for (i, (_, a)) in t.records.iter().enumerate() {
                        if let (Some((ri, _)), Some(rec)) = (a, bytes.get_mut(i * ams::RECORD_LEN..(i + 1) * ams::RECORD_LEN)) {
                            let v = table.rows[*ri].vals;
                            ams::rotate_record(rec, &owens_rotation(v[0], v[1], v[2], v[3]));
                            n += 1;
                        }
                    }
                    let mut new_dirs = vec![];
                    let mut failed: Option<String> = None;
                    for d in [&raw_dir, &corr_dir] {
                        if !d.exists() {
                            match std::fs::create_dir(d) {
                                Ok(()) => new_dirs.push(d.clone()),
                                Err(e) => {
                                    failed = Some(format!("{}: {e}", d.display()));
                                    break;
                                }
                            }
                        }
                    }
                    if failed.is_none() {
                        // the corrected file first; the source is only moved once that is on disk
                        if let Err(e) = std::fs::write(&corr, &bytes) {
                            failed = Some(format!("{}: {e}", corr.display()));
                        } else if let Err(e) = std::fs::rename(src, &raw) {
                            let _ = std::fs::remove_file(&corr);
                            failed = Some(format!("moving {}: {e}", src.display()));
                        }
                    }
                    match failed {
                        Some(e) => {
                            for d in new_dirs.iter().rev() {
                                let _ = std::fs::remove_dir(d);
                            }
                            skipped.push(format!("{} ({e})", t.label));
                        }
                        None => {
                            recs += n;
                            ops.push(MoveOp { orig: src.clone(), raw, corr, corr_len: bytes.len() as u64, new_dirs });
                        }
                    }
                }
                let files = ops.len();
                // the open file may have been moved: show its corrected version instead
                let reopen = self.doc.as_ref().and_then(|d| ops.iter().find(|o| o.orig == d.path)).map(|o| o.corr.clone());
                self.of.ops = ops;
                self.refresh_set();
                if let Some(p) = reopen {
                    self.open_path(p);
                }
                self.of.last = Some(Scope::Multiple);
                self.of.msg = format!("Rotated {recs} record(s) in {files} file(s): the sources are now in _raw folders, the corrected files (_corr.ams) in _corr folders.");
                if !skipped.is_empty() {
                    self.of.msg += &format!(" Skipped: {}.", skipped.join("; "));
                }
            }
        }
    }

    /// Undo of the last correction from a file: the open file's data (single) or the written copies (multiple).
    pub(super) fn of_undo(&mut self) {
        match self.of.last {
            Some(Scope::Single) => {
                if let (Some(d), Some((raw, specs))) = (&mut self.doc, self.owens_undo.take()) {
                    d.raw = raw;
                    d.specs = specs;
                    self.dirty = true;
                    self.of.msg = "The last correction of this file was undone.".into();
                }
            }
            Some(Scope::Multiple) => {
                let (mut restored, mut left) = (0, 0);
                let reopen = self.doc.as_ref().and_then(|d| self.of.ops.iter().find(|o| o.corr == d.path || o.raw == d.path)).map(|o| o.orig.clone());
                for op in std::mem::take(&mut self.of.ops).into_iter().rev() {
                    // the corrected file goes only if it is still what was written; the source goes back if its place is free
                    let corr_ok = !op.corr.exists() || (std::fs::metadata(&op.corr).map_or(false, |m| m.len() == op.corr_len) && std::fs::remove_file(&op.corr).is_ok());
                    let back_ok = !op.orig.exists() && std::fs::rename(&op.raw, &op.orig).is_ok();
                    if corr_ok && back_ok {
                        restored += 1;
                    } else {
                        left += 1;
                    }
                    for d in op.new_dirs.iter().rev() {
                        let _ = std::fs::remove_dir(d);
                    }
                }
                self.refresh_set();
                if let Some(p) = reopen.filter(|p| p.exists()) {
                    self.open_path(p);
                }
                self.of.msg = format!("The last multiple correction was undone: {restored} file(s) put back{}.", if left > 0 { format!(", {left} could not be (a file was changed or is in the way)") } else { String::new() });
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
        let (mut single, mut multiple, mut back, mut select, mut rotate, mut undo) = (false, false, false, false, false, false);
        let confirm = self.of.confirm;
        let (mut do_apply, mut cancel_confirm) = (false, false);
        let first_target = self.of.plan.iter().find(|t| t.records.iter().any(|r| r.1.is_some())).map(|t| (t.path.clone(), t.file_name.clone()));
        self.base_window(ctx, "Specimen to Geographic Rotation (Owens' method)", W_OWENS, Vec2::new(700.0, 780.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            super::tools_ui::pop_style(ui, 1.3);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            switched = owens_head(ui, &mut old, &toast, now, accent);
            ui.separator();
            // ---- row 2: the same place as the "From file ..." button of the manual window
            ui.horizontal(|ui| {
                if stage == Stage::Scope {
                    single = ui.add_sized([210.0, 26.0], egui::Button::new(RichText::new("This file").strong())).on_hover_text("Correct the file that is open now").clicked();
                    multiple = ui.add_sized([330.0, 26.0], egui::Button::new(RichText::new("Multiple specimens from this parent tree").strong())).on_hover_text("Go through the specimen folders next to this file's folder (or the open set of specimens)").clicked();
                    back = ui.small_button("Cancel").clicked();
                } else {
                    select = ui.add_sized([210.0, 26.0], egui::Button::new("Select file ...")).on_hover_text("Choose another spreadsheet or csv").clicked();
                    if let Some(l) = last {
                        undo = ui.add(egui::Button::new(RichText::new(format!("Undo last correction ({})", l.word())).small()).small().fill(Color32::TRANSPARENT).stroke(ui.visuals().widgets.inactive.bg_stroke)).clicked();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| back = ui.small_button("Back to manual input").clicked());
                }
            });
            if stage == Stage::Scope {
                ui.label(RichText::new("Where should the values from the file go?").weak().small());
                return;
            }
            if confirm {
                // the file tree is going to change: say how, and ask
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("This changes the file tree").strong().color(warn));
                    ui.label(format!("{files_n} specimen folder(s) are affected. In each of them:"));
                    ui.label("\u{2022} the source file (the main file) is MOVED into a new sub-folder  _raw");
                    ui.label("\u{2022} the corrected file, named like the source with  _corr  added, is written into a new sub-folder  _corr");
                    ui.label("\u{2022} other files in the specimen folder stay where they are");
                    ui.label("\u{2022} nothing is overwritten or deleted, and Undo last correction (multiple) puts everything back");
                    if let Some((Some(p), name)) = &first_target {
                        let dir = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
                        ui.add_space(4.0);
                        ui.label(RichText::new("Example:").strong().small());
                        ui.label(RichText::new(format!("{dir}/{name}\n   \u{2192} moved to  {dir}/_raw/{name}\n   \u{2192} corrected file  {dir}/_corr/{}", corr_name(name))).monospace().small());
                    }
                });
                ui.horizontal(|ui| {
                    let go = egui::Button::new(RichText::new("Move the files and write the corrected ones").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) });
                    do_apply = ui.add(go).clicked();
                    cancel_confirm = ui.button("Cancel").clicked();
                });
                return;
            }
            let sc = scope.unwrap_or(Scope::Single);
            let fname = self.of.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&fname).strong());
                if !self.of.sheet.is_empty() {
                    ui.label(RichText::new(format!("sheet \u{201c}{}\u{201d}", self.of.sheet)).weak().small());
                }
                ui.label(RichText::new(match sc {
                    Scope::Single => "\u{b7} corrects this file".to_string(),
                    Scope::Multiple => format!("\u{b7} corrects the specimens of {}", self.of.root.as_ref().map(|r| r.display().to_string()).unwrap_or_default()),
                }).weak().small());
            });

            // ---- how the file has to look, checked against the file
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("How the file has to look").strong());
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
                                    owens_table::Role::Specimen => "Name of the specimen: a record name of the file (ZZ01-A1) or the name of the file / its folder, which then applies to every record. Headers such as Specimen, Sample ID or Name are found.",
                                    owens_table::Role::FieldStrike => "Strike (0-360 deg) of the reference plane in the field. Used as a right-hand-rule strike; a header with LHR (left-hand rule) or Dip Direction is converted.",
                                    owens_table::Role::FieldDip => "Dip (0-90 deg) of that plane in the field, measured down from the horizontal.",
                                    owens_table::Role::TableStrike => "Strike (0-360 deg) of the same plane on the hand specimen lying on the drilling table. Same convention as the field strike.",
                                    owens_table::Role::TableDip => "Dip (0-90 deg) of the same plane on the drilling table.",
                                });
                            });
                            if !tag.is_empty() {
                                ui.label(RichText::new(tag).small());
                            }
                        });
                    }
                });
                ui.label(RichText::new("example row:  ZZ01-A1 | 30 | 20 | 45 | 10").monospace().small().weak());
                ui.label(RichText::new("One row per specimen. The header can be in any of the first 20 rows; any sheet of a workbook is tried. Also accepted: FS FD DS DD, \u{201c}Strike (LHR)\u{201d} (left-hand rule), \u{201c}Dip Direction\u{201d}, \u{201c}Sample ID\u{201d}. Strikes: index finger along the strike, thumb to the dip direction (0\u{b0} dips to 90\u{b0}); \u{201c}LHR\u{201d} is used as it stands, a dip direction is turned into a strike. Same for the drill table.").weak().small());
                ui.label(RichText::new(match sc {
                    Scope::Single => "Specimen = a record name of this file (ZZ01-A1) or the name of the file / its folder (applies to every record). A file with a single row applies to every record.",
                    Scope::Multiple => "Specimen = the specimen folder or main file name (applies to every record of its main file) or a record name. The source file of each specimen is moved into a new _raw folder and the corrected file (<name>_corr.ams) is written into a new _corr folder, both inside the specimen folder.",
                }).weak().small());
                if sc == Scope::Multiple {
                    ui.separator();
                    ui.columns(2, |cols| {
                        let mono = |s: String| RichText::new(s).monospace().small();
                        cols[0].label(RichText::new("Required folder structure").strong().small());
                        cols[0].label(mono("parent folder/\n\u{251c}\u{2500} SPECIMEN-1/\n\u{2502}  \u{251c}\u{2500} SPECIMEN-1_use.ams  \u{25cf}\n\u{2502}  \u{2514}\u{2500} other versions.ams\n\u{251c}\u{2500} SPECIMEN-2/\n\u{2502}  \u{2514}\u{2500} SPECIMEN-2_use.ams  \u{25cf}\n\u{2514}\u{2500} \u{2026}".to_string()));
                        cols[0].label(RichText::new("\u{25cf} main file of the specimen").weak().small());
                        cols[0].add_space(4.0);
                        cols[0].label(RichText::new("After the correction").strong().small());
                        cols[0].label(mono("SPECIMEN-1/\n\u{251c}\u{2500} _raw/   SPECIMEN-1_use.ams\n\u{251c}\u{2500} _corr/  SPECIMEN-1_use_corr.ams\n\u{2514}\u{2500} other files stay here".to_string()));
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
                    Scope::Multiple => "Write the corrected copies into a new folder next to the parent folder",
                }).clicked();
                if sc == Scope::Multiple && ready {
                    ui.label(RichText::new("moves each source into _raw and writes <name>_corr.ams into _corr (you are asked first)").weak().small());
                }
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
        if single || multiple {
            self.of.scope = Some(if single { Scope::Single } else { Scope::Multiple });
            self.of.path = None;
            self.of.table = None;
            self.of.error = None;
            self.of.plan.clear();
            self.of.msg.clear();
            self.of_select_file();
        }
        if back {
            self.of.stage = Stage::Manual;
            self.of.confirm = false;
        }
        if select {
            self.of_select_file();
        }
        if undo {
            self.of_undo();
        }
        if rotate {
            if self.of.scope == Some(Scope::Multiple) {
                self.of.confirm = true;
            } else {
                self.of_apply();
                self.of_build_plan();
            }
        }
        if do_apply {
            self.of.confirm = false;
            self.of_apply();
            // the files have been moved: the list of what to correct is empty until a file is selected again,
            // so that a second Rotate cannot hit the files that are left in the specimen folders
            self.of.plan.clear();
            self.of.note = "Done. Choose Select file ... to prepare another correction.".into();
        }
        if cancel_confirm {
            self.of.confirm = false;
        }
        self.win_owens = open && !cancel;
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
