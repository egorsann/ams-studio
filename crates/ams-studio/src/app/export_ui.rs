//! File > Save as .ams: writes either the selected files of the browser (the open file by default) or every edited
//! file into a `_geo` folder inside each specimen folder, as `<name>_geo.ams`. The source files stay where they are.
//! Edits (rotations, deleted records, names, volume ...) are never written by themselves; this is where they are saved.

use super::{tools_ui, App, W_EXPORT};
use crate::doc;
use ams_core::ams::{self, RECORD_LEN};
use egui::{Align, Color32, Layout, RichText, Vec2};
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq)]
pub(super) enum Status {
    New,
    /// The output exists already (when it was created).
    Exists(String),
    /// No rotation is held for the file.
    Unchanged,
}

pub(super) struct ExRow {
    pub src: PathBuf,
    pub out: PathBuf,
    pub status: Status,
}

/// One written file, for Undo (a replaced file is kept in memory).
pub(super) struct ExOp {
    pub src: PathBuf,
    pub out: PathBuf,
    pub len: u64,
    pub backup: Option<Vec<u8>>,
    pub new_dir: Option<PathBuf>,
    /// The volume json written next to the file, if any.
    pub json: Option<PathBuf>,
}

#[derive(Default)]
pub(super) struct ExportState {
    pub rows: Vec<ExRow>,
    pub overwrite: bool,
    pub unchanged_too: bool,
    /// 0: the selected files (the open one by default), 1: every edited file
    pub scope: usize,
    pub ops: Vec<ExOp>,
    pub msg: String,
}

/// `<folder>/_geo/<name>_geo.ams` for a source file.
pub(super) fn geo_path(src: &Path) -> PathBuf {
    let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    src.parent().unwrap_or(Path::new(".")).join("_geo").join(format!("{stem}_geo.ams"))
}

impl App {
    /// The files of scope 0: the selection in the browser, else the open file.
    fn selected_or_open(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self.tree_sel.clone();
        if files.is_empty() {
            if let Some(d) = self.doc.as_ref().filter(|d| !d.synthetic) {
                files.push(d.path.clone());
            }
        }
        files
    }

    pub(super) fn open_export(&mut self) {
        if self.selected_or_open().is_empty() && self.edited_files().is_empty() {
            self.info = Some("Select files in the file browser (or open a file) first.\nA data set without a file behind it is saved with File > Save Active Records As.".into());
            return;
        }
        self.ex.scope = if self.selected_or_open().is_empty() { 1 } else { 0 };
        self.ex.msg.clear();
        self.build_rows();
        self.open_win(W_EXPORT);
    }

    /// The rows for the chosen scope, with what each file would do.
    fn build_rows(&mut self) {
        let files = if self.ex.scope == 0 { self.selected_or_open() } else { self.edited_files() };
        let open_path = self.doc.as_ref().filter(|d| !d.synthetic).map(|d| (d.path.clone(), d.modified));
        self.ex.rows = files
            .into_iter()
            .map(|src| {
                let out = geo_path(&src);
                let changed = self.pending.contains_key(&src) || self.stash.contains_key(&src) || open_path.as_ref().map_or(false, |(p, m)| *p == src && *m);
                let status = if !changed {
                    Status::Unchanged
                } else if out.exists() {
                    Status::Exists(std::fs::metadata(&out).ok().and_then(|m| m.created().or_else(|_| m.modified()).ok()).map(doc::fmt_time).unwrap_or_default())
                } else {
                    Status::New
                };
                ExRow { src, out, status }
            })
            .collect();
    }

    /// Bytes to write for a source file: the open or an edited file as it is now (active records), any other with its rotations.
    fn export_bytes(&self, src: &Path) -> Option<Vec<u8>> {
        if let Some(d) = self.doc.as_ref().filter(|d| !d.synthetic && d.path == src) {
            return Some(d.records((0..d.specs.len()).filter(|&i| d.active[i])));
        }
        if let Some(d) = self.stash.get(src) {
            return Some(d.records((0..d.specs.len()).filter(|&i| d.active[i])));
        }
        let mut bytes = std::fs::read(src).ok()?;
        if let Some(v) = self.pending.get(src) {
            for (i, r) in v.iter().enumerate() {
                if let (Some(r), Some(rec)) = (r, bytes.get_mut(i * RECORD_LEN..(i + 1) * RECORD_LEN)) {
                    ams::rotate_record(rec, r);
                }
            }
        }
        Some(bytes)
    }

    fn do_export(&mut self) {
        let (overwrite, unchanged_too) = (self.ex.overwrite, self.ex.unchanged_too);
        let rows: Vec<(PathBuf, PathBuf, Status)> = self.ex.rows.iter().map(|r| (r.src.clone(), r.out.clone(), r.status.clone())).collect();
        let (mut ops, mut skipped, mut failed) = (vec![], 0, vec![]);
        for (src, out, status) in rows {
            let skip = match &status {
                Status::Unchanged => !unchanged_too,
                Status::Exists(_) => !overwrite,
                Status::New => false,
            };
            if skip {
                skipped += 1;
                continue;
            }
            let Some(bytes) = self.export_bytes(&src) else {
                failed.push(format!("{} (could not be read)", src.display()));
                continue;
            };
            let dir = out.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let new_dir = if dir.exists() { None } else { Some(dir.clone()) };
            if new_dir.is_some() {
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    failed.push(format!("{}: {e}", dir.display()));
                    continue;
                }
            }
            let backup = if out.exists() { std::fs::read(&out).ok() } else { None };
            match std::fs::write(&out, &bytes) {
                Ok(()) => {
                    // the volume information that was entered for the file is stored next to the saved file
                    let json = self.vol_pending.get(&src).and_then(|info| doc::write_volume_json(&out, info, bytes.len() / RECORD_LEN).ok());
                    ops.push(ExOp { src, out, len: bytes.len() as u64, backup, new_dir, json });
                }
                Err(e) => failed.push(format!("{}: {e}", out.display())),
            }
        }
        let n = ops.len();
        for o in &ops {
            self.exported.insert(o.src.clone());
        }
        self.ex.ops = ops;
        self.ex.msg = format!("Saved {n} file(s) into _geo folders{}{}.", if skipped > 0 { format!("; {skipped} skipped") } else { String::new() }, if failed.is_empty() { String::new() } else { format!("; failed: {}", failed.join(", ")) });
        self.refresh_set();
        self.open_export_refresh();
    }

    /// Re-reads the status of the rows (after an export or an undo).
    fn open_export_refresh(&mut self) {
        self.build_rows();
    }

    fn undo_export(&mut self) {
        let (mut removed, mut restored) = (0, 0);
        for op in std::mem::take(&mut self.ex.ops).into_iter().rev() {
            // only a file that is still what was written is touched
            if std::fs::metadata(&op.out).map_or(false, |m| m.len() == op.len) && std::fs::remove_file(&op.out).is_ok() {
                removed += 1;
                if let Some(b) = &op.backup {
                    if std::fs::write(&op.out, b).is_ok() {
                        restored += 1;
                    }
                }
            }
            if let Some(j) = &op.json {
                let _ = std::fs::remove_file(j);
            }
            if let Some(d) = &op.new_dir {
                let _ = std::fs::remove_dir(d);
            }
            self.exported.remove(&op.src);
        }
        self.refresh_set();
        self.open_export_refresh();
        self.ex.msg = format!("The last save was undone: {removed} file(s) removed, {restored} earlier file(s) put back.");
    }

    pub(super) fn export_window(&mut self, ctx: &egui::Context) {
        if !self.win_export {
            return;
        }
        let dark = self.dark;
        let ok = Color32::from_rgb(70, 170, 90);
        let warn = Color32::from_rgb(214, 150, 40);
        let bad = Color32::from_rgb(214, 90, 90);
        let mut ex = std::mem::take(&mut self.ex);
        let root = self.set.as_ref().map(|s| s.root.clone());
        let n_new = ex.rows.iter().filter(|r| r.status == Status::New).count();
        let n_exists = ex.rows.iter().filter(|r| matches!(r.status, Status::Exists(_))).count();
        let n_unchanged = ex.rows.iter().filter(|r| r.status == Status::Unchanged).count();
        let n_go = n_new + if ex.overwrite { n_exists } else { 0 } + if ex.unchanged_too { n_unchanged } else { 0 };
        let (mut open, mut go, mut undo, mut close, mut rebuild) = (true, false, false, false, false);
        let n_sel = self.selected_or_open().len();
        let n_edit = self.edited_files().len();
        let rel = |p: &Path| -> String {
            root.as_ref().and_then(|r| p.strip_prefix(r).ok()).map(|x| x.display().to_string()).unwrap_or_else(|| p.display().to_string())
        };
        self.base_window(ctx, "Save as .ams", W_EXPORT, Vec2::new(820.0, 560.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.2);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            ui.label(RichText::new("Which files should be saved?").strong());
            ui.horizontal(|ui| {
                rebuild |= ui.radio_value(&mut ex.scope, 0, format!("The selected files only ({n_sel})")).on_hover_text("The open file, or the files selected in the file browser with Shift (range) and Ctrl (single)").changed();
                rebuild |= ui.radio_value(&mut ex.scope, 1, format!("Every edited file ({n_edit})")).on_hover_text("Every file with changes that are not saved: rotations, deleted records, names, volume").changed();
            });
            ui.label("Each file is written as  <name>_geo.ams  into a sub-folder  _geo  of its specimen folder (made if it is not there). The source files stay where they are.");
            ui.label(RichText::new("Nothing is saved by editing: rotations, deleted records and the like only live in memory until they are saved here.").weak().small());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let h = (ui.available_height() - 150.0).max(120.0);
                egui::ScrollArea::both().max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("ex-grid").striped(true).num_columns(4).spacing([14.0, 5.0]).show(ui, |ui| {
                        for hd in ["#", "Source", "Written to", "Status"] {
                            ui.label(RichText::new(hd).strong());
                        }
                        ui.end_row();
                        for (i, r) in ex.rows.iter().enumerate() {
                            ui.label(RichText::new((i + 1).to_string()).weak());
                            ui.label(rel(&r.src));
                            ui.label(RichText::new(rel(&r.out)).small());
                            match &r.status {
                                Status::New => ui.label(RichText::new("new").color(ok)),
                                Status::Exists(t) => ui.label(RichText::new(format!("exists, created {t} UTC")).color(if ex.overwrite { bad } else { warn })),
                                Status::Unchanged => ui.label(RichText::new("no rotation held").weak()),
                            };
                            ui.end_row();
                        }
                    });
                });
            });
            if n_exists > 0 {
                ui.checkbox(&mut ex.overwrite, format!("Overwrite the {n_exists} file(s) that exist already")).on_hover_text("The files that are replaced are kept in memory, so Undo last save brings them back while the program stays open.");
            }
            if n_unchanged > 0 {
                ui.checkbox(&mut ex.unchanged_too, format!("Also export the {n_unchanged} file(s) without a rotation"));
            }
            ui.horizontal(|ui| {
                let b = egui::Button::new(RichText::new(format!("Save {n_go} file(s)")).strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(150.0, 0.0));
                go = ui.add_enabled(n_go > 0, b).clicked();
                if !ex.ops.is_empty() {
                    undo = ui.add(egui::Button::new(RichText::new("Undo last save").small()).small().fill(Color32::TRANSPARENT).stroke(ui.visuals().widgets.inactive.bg_stroke)).clicked();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                });
            });
            if !ex.msg.is_empty() {
                ui.label(RichText::new(&ex.msg).small().color(ok));
            }
        });
        self.ex = ex;
        if rebuild {
            self.build_rows();
        }
        if go {
            self.do_export();
        }
        if undo {
            self.undo_export();
        }
        self.win_export = open && !close;
    }
}
