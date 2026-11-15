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
    /// Saved into the file itself.
    Native,
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
    /// The `.owens.json` written for the file, and what it held before (None: it did not exist).
    pub marks: Option<PathBuf>,
    pub marks_prev: Option<Vec<u8>>,
}

#[derive(Default)]
pub(super) struct ExportState {
    pub rows: Vec<ExRow>,
    pub overwrite: bool,
    pub unchanged_too: bool,
    /// Save into the .ams files themselves instead of new files in the `_geo` folders.
    pub native: bool,
    /// 0: the selected files (the open one by default), 1: every edited file
    pub scope: usize,
    pub ops: Vec<ExOp>,
    pub msg: String,
}

// ---- the undo journal: the last save, kept on disk so that Undo still works after the program was closed

fn undo_dir() -> PathBuf {
    super::config_dir().join("ams-studio").join("undo")
}

/// Remembers the last save: one line per file in `journal.txt`, replaced files as `<n>.bak` next to it.
fn save_journal(ops: &[ExOp]) {
    save_journal_in(&undo_dir(), ops);
}

fn save_journal_in(dir: &Path, ops: &[ExOp]) {
    let _ = std::fs::remove_dir_all(dir);
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let p = |x: &Option<PathBuf>| x.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "-".into());
    let mut text = String::new();
    for (i, o) in ops.iter().enumerate() {
        let bak = match &o.backup {
            Some(b) if std::fs::write(dir.join(format!("{i}.bak")), b).is_ok() => format!("{i}.bak"),
            _ => "-".into(),
        };
        let mprev = match &o.marks_prev {
            Some(b) if std::fs::write(dir.join(format!("{i}.marks.bak")), b).is_ok() => format!("{i}.marks.bak"),
            _ => "-".into(),
        };
        text += &format!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n", o.src.display(), o.out.display(), o.len, bak, p(&o.new_dir), p(&o.json), p(&o.marks), mprev);
    }
    let _ = std::fs::write(dir.join("journal.txt"), text);
}

fn clear_journal() {
    let _ = std::fs::remove_dir_all(undo_dir());
}

/// The last save as it was left on disk (empty when there is none or the files are gone).
pub(super) fn load_journal() -> Vec<ExOp> {
    load_journal_in(&undo_dir())
}

fn load_journal_in(dir: &Path) -> Vec<ExOp> {
    let Ok(text) = std::fs::read_to_string(dir.join("journal.txt")) else { return vec![] };
    let opt = |s: &str| if s == "-" { None } else { Some(PathBuf::from(s)) };
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() != 6 && f.len() != 8 {
                return None;
            }
            let backup = if f[3] == "-" { None } else { Some(std::fs::read(dir.join(f[3])).ok()?) };
            let marks = f.get(6).and_then(|m| opt(m));
            let marks_prev = f.get(7).filter(|m| **m != "-").and_then(|m| std::fs::read(dir.join(m)).ok());
            Some(ExOp { src: PathBuf::from(f[0]), out: PathBuf::from(f[1]), len: f[2].parse().ok()?, backup, new_dir: opt(f[4]), json: opt(f[5]), marks, marks_prev })
        })
        .collect()
}

/// `<folder>/<folder>_geo/<name>_geo.ams` for a source file (DC26AB002_AMS/DC26AB002_AMS_geo/DC26AB002_use_geo.ams).
pub(super) fn geo_path(src: &Path) -> PathBuf {
    let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let dir = src.parent().unwrap_or(Path::new("."));
    // the folder carries the specimen folder's name: DC26AB002_AMS/DC26AB002_AMS_geo/DC26AB002_use_geo.ams
    let folder = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    dir.join(ams_core::specset::geo_folder_name(&folder)).join(format!("{stem}_geo.ams"))
}

impl App {
    /// The files of scope 0: the selection in the browser, else the open file.
    pub(super) fn selected_or_open(&self) -> Vec<PathBuf> {
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

    pub(super) fn build_rows_pub(&mut self) {
        self.build_rows();
    }

    /// The rows for the chosen scope, with what each file would do.
    fn build_rows(&mut self) {
        let files = if self.ex.scope == 0 { self.selected_or_open() } else { self.edited_files() };
        let open_path = self.doc.as_ref().filter(|d| !d.synthetic).map(|d| (d.path.clone(), d.modified));
        self.ex.rows = files
            .into_iter()
            .map(|src| {
                let out = if self.ex.native { src.clone() } else { geo_path(&src) };
                let changed = self.pending.contains_key(&src) || self.stash.contains_key(&src) || open_path.as_ref().map_or(false, |(p, m)| *p == src && *m);
                let status = if !changed {
                    Status::Unchanged
                } else if self.ex.native {
                    Status::Native
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
    fn export_bytes(&self, src: &Path, all: bool) -> Option<Vec<u8>> {
        // a native save keeps every record (disregarding is a view setting); a new _geo file leaves the disregarded out
        if let Some(d) = self.doc.as_ref().filter(|d| !d.synthetic && d.path == src) {
            return Some(d.records((0..d.specs.len()).filter(|&i| all || d.active[i])));
        }
        if let Some(d) = self.stash.get(src) {
            return Some(d.records((0..d.specs.len()).filter(|&i| all || d.active[i])));
        }
        let mut bytes = std::fs::read(src).ok()?;
        // a file that is not open: its held reorientations go into the records (everything else is as on disk)
        if let Some(v) = self.pending.get(src) {
            for (i, g) in v.iter().enumerate() {
                if let (Some(g), Some(rec)) = (g, bytes.get_mut(i * RECORD_LEN..(i + 1) * RECORD_LEN)) {
                    ams_core::georot::write(rec, Some(g));
                }
            }
        }
        Some(bytes)
    }

    fn do_export(&mut self) {
        let (overwrite, unchanged_too, native) = (self.ex.overwrite, self.ex.unchanged_too, self.ex.native);
        let rows: Vec<(PathBuf, PathBuf, Status)> = self.ex.rows.iter().map(|r| (r.src.clone(), r.out.clone(), r.status.clone())).collect();
        let (mut ops, mut skipped, mut failed) = (vec![], 0, vec![]);
        for (src, out, status) in rows {
            let skip = match &status {
                Status::Unchanged => !unchanged_too,
                Status::Exists(_) => !overwrite,
                Status::New | Status::Native => false,
            };
            if skip {
                skipped += 1;
                continue;
            }
            let Some(bytes) = self.export_bytes(&src, native) else {
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
                    let json = self.vol_pending.get(&src).and_then(|info| {
                        let vols: Vec<f64> = ams::parse(&bytes).map(|v| v.iter().map(|s| s.volume_cm3).collect()).unwrap_or_default();
                        doc::write_volume_json(&out, info, &vols).ok()
                    });
                    ops.push(ExOp { src, out, len: bytes.len() as u64, backup, new_dir, json, marks: None, marks_prev: None });
                }
                Err(e) => failed.push(format!("{}: {e}", out.display())),
            }
        }
        let n = ops.len();
        for o in &ops {
            if native {
                // the edits are in the file now: nothing is held in memory any more, the open file is read again
                let p = o.src.clone();
                self.pending.remove(&p);
                self.stash.remove(&p);
                self.vol_pending.remove(&p);
                self.exported.remove(&p);
                if self.doc.as_ref().map_or(false, |d| !d.synthetic && d.path == p) {
                    if let Some(d) = self.doc.as_mut() {
                        d.modified = false;
                    }
                    self.open_path(p);
                }
            } else {
                self.exported.insert(o.src.clone());
            }
        }
        // the way back survives closing the program (replaced files are kept in the settings folder)
        if !ops.is_empty() {
            save_journal(&ops);
        }
        self.ex.ops = ops;
        self.ex.msg = format!("Saved {n} file(s) {}{}{}.", if native { "into the .ams files themselves" } else { "into the _geo folders" }, if skipped > 0 { format!("; {skipped} skipped") } else { String::new() }, if failed.is_empty() { String::new() } else { format!("; failed: {}", failed.join(", ")) });
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
            // the Owens-rotated marks go back to what they were
            if let Some(m) = &op.marks {
                match &op.marks_prev {
                    Some(prev) => {
                        let _ = std::fs::write(m, prev);
                    }
                    None => {
                        let _ = std::fs::remove_file(m);
                    }
                }
            }
            // a file that was saved natively and is open is read again
            if op.src == op.out && self.doc.as_ref().map_or(false, |d| !d.synthetic && d.path == op.src) {
                let p = op.src.clone();
                if let Some(d) = self.doc.as_mut() {
                    d.modified = false;
                }
                self.open_path(p);
            }
            if let Some(d) = &op.new_dir {
                let _ = std::fs::remove_dir(d);
            }
            self.exported.remove(&op.src);
        }
        clear_journal();
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
        let n_native = ex.rows.iter().filter(|r| r.status == Status::Native).count();
        let n_go = n_new + n_native + if ex.overwrite { n_exists } else { 0 } + if ex.unchanged_too { n_unchanged } else { 0 };
        let (mut open, mut go, mut undo, mut close, mut rebuild) = (true, false, false, false, false);
        let n_sel = self.selected_or_open().len();
        let n_edit = self.edited_files().len();
        let rel = |p: &Path| -> String {
            root.as_ref().and_then(|r| p.strip_prefix(r).ok()).map(|x| x.display().to_string()).unwrap_or_else(|| p.display().to_string())
        };
        let scr = ctx.content_rect();
        let avail = Vec2::new((scr.width() - 24.0).max(420.0), (scr.height() - 56.0).max(360.0));
        self.base_window(ctx, "Save as .ams", W_EXPORT, Vec2::new(avail.x.min(860.0), avail.y.min(680.0))).max_size(avail).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.2);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            ui.label(RichText::new("Which files should be saved?").strong());
            ui.horizontal(|ui| {
                rebuild |= ui.radio_value(&mut ex.scope, 0, format!("The selected files only (n={n_sel})")).on_hover_text("The open file, or the files selected in the file browser with Shift (range) and Ctrl (single)").changed();
                rebuild |= ui.radio_value(&mut ex.scope, 1, format!("Every edited file (n={n_edit})")).on_hover_text("Every file with changes that are not saved: rotations, deleted records, names, volume").changed();
            });
            ui.label(if ex.native {
                "Each file is saved into itself: the file is replaced by its edited version (the replaced version is kept for Undo)."
            } else {
                "Each file is written as  <name>_geo.ams  into a sub-folder named after the specimen folder plus _geo  (DC26AB002_AMS_geo, made if it is not there). The source files stay where they are."
            });
            ui.label(RichText::new("Nothing is saved by editing: rotations, deleted records and the like only live in memory until they are saved here.").weak().small());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                // room is kept below for the last question, the checkboxes, the Save row and the message
                let h = (ui.available_height() - 290.0).max(90.0);
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
                                Status::Exists(t) => ui.label(RichText::new(format!("exists, created {t}")).color(if ex.overwrite { bad } else { warn })),
                                Status::Unchanged => ui.label(RichText::new("no rotation held").weak()),
                                Status::Native => ui.label(RichText::new("replaces the file itself").color(bad)),
                            };
                            ui.end_row();
                        }
                    });
                });
            });
            if n_exists > 0 && !ex.native {
                ui.checkbox(&mut ex.overwrite, format!("Overwrite the {n_exists} file(s) that exist already")).on_hover_text("The files that are replaced are kept in the settings folder, so Undo last save brings them back, also after the program was closed and opened again.");
            }
            if n_unchanged > 0 {
                ui.checkbox(&mut ex.unchanged_too, format!("Also export the {n_unchanged} file(s) without a rotation"));
            }
            // the last question: where the changes go (the prompt first, the choices under it)
            ui.label(RichText::new("How should the changes be saved?").strong());
            rebuild |= ui.radio_value(&mut ex.native, false, "Export as new .ams files (into <specimen folder>_geo folders, the originals stay)").changed();
            rebuild |= ui.radio_value(&mut ex.native, true, "Save into the .ams files themselves (natively, replaces them)").on_hover_text("The original files are overwritten with the rotated / edited records. Every record stays in the file, also the disregarded ones. The replaced version is kept in the settings folder, so Undo last save brings it back, also after a restart. The reorientation (Owens or drill core) is stored inside the records, the tensors are never rotated.").changed();
            if ex.native {
                ui.label(RichText::new("The originals are replaced. Undo last save puts them back.").small().color(bad));
            }
            ui.horizontal(|ui| {
                let b = egui::Button::new(RichText::new(format!("Save {n_go} file(s)")).strong().color(Color32::WHITE)).fill(if ex.native { Color32::from_rgb(190, 70, 70) } else if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(150.0, 0.0));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_undo_journal_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("ams-studio-journal-{}", std::process::id()));
        let ops = vec![
            ExOp { src: "/a/x.ams".into(), out: "/a/_geo/x_geo.ams".into(), len: 1280, backup: Some(vec![1, 2, 3]), new_dir: None, json: Some("/a/_geo/x_geo.volume.json".into()), marks: Some("/a/_geo/x_geo.owens.json".into()), marks_prev: Some(vec![7, 8]) },
            ExOp { src: "/a/y.ams".into(), out: "/a/_geo/y_geo.ams".into(), len: 640, backup: None, new_dir: Some("/a/_geo".into()), json: None, marks: None, marks_prev: None },
        ];
        save_journal_in(&dir, &ops);
        let back = load_journal_in(&dir);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].backup, Some(vec![1, 2, 3]));
        assert_eq!((back[0].len, back[0].json.clone()), (1280, Some("/a/_geo/x_geo.volume.json".into())));
        assert_eq!((back[0].marks.clone(), back[0].marks_prev.clone()), (Some("/a/_geo/x_geo.owens.json".into()), Some(vec![7, 8])));
        assert_eq!((back[1].backup.clone(), back[1].new_dir.clone()), (None, Some("/a/_geo".into())));
        // a newer save replaces the older one completely
        save_journal_in(&dir, &ops[1..]);
        assert_eq!(load_journal_in(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_journal_in(&dir).is_empty());
    }
}
