//! Edit > Recombine Split AMS Files: joins `.ams` files that were exported in parts into one file, in a suggested
//! order by specimen ID (see `ams_core::recombine`). The files can be dragged into the order they should follow each
//! other; the suggestions can be switched off; single records can be left out.

use super::{tools_ui, App, W_RECOMB};
use ams_core::ams::{self, RECORD_LEN};
use ams_core::recombine::{self, Options, RecInfo, Skip, Within};
use egui::{Align, Color32, Layout, RichText, Sense, Stroke, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;

pub(super) struct SrcFile {
    pub path: PathBuf,
    pub name: String,
    pub records: Vec<RecInfo>,
    pub error: Option<String>,
}

pub(super) struct RecState {
    pub files: Vec<SrcFile>,
    pub opts: Options,
    /// Records the user ticked or unticked by hand, by (file, index in the file).
    pub manual: HashMap<(PathBuf, usize), bool>,
    pub open_after: bool,
    pub msg: String,
}

impl Default for RecState {
    fn default() -> Self {
        RecState { files: vec![], opts: Options::default(), manual: HashMap::new(), open_after: true, msg: String::new() }
    }
}

fn load(path: PathBuf) -> SrcFile {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| ams::parse(&b).map(|s| (b, s)).map_err(|e| e.to_string())) {
        Ok((b, specs)) => {
            let records = specs.iter().enumerate().map(|(i, s)| RecInfo { name: s.name.clone(), bytes: b[i * RECORD_LEN..(i + 1) * RECORD_LEN].to_vec() }).collect();
            SrcFile { path, name, records, error: None }
        }
        Err(e) => SrcFile { path, name, records: vec![], error: Some(e) },
    }
}

impl App {
    pub(super) fn open_recombine(&mut self) {
        self.open_win(W_RECOMB);
    }

    fn rc_add(&mut self, paths: Vec<PathBuf>) {
        for p in paths {
            if !self.rc.files.iter().any(|f| f.path == p) {
                self.rc.files.push(load(p));
            }
        }
    }

    pub(super) fn recombine_window(&mut self, ctx: &egui::Context) {
        if !self.win_recomb {
            return;
        }
        let dark = self.dark;
        let accent = if dark { Color32::from_rgb(106, 160, 255) } else { Color32::from_rgb(47, 111, 222) };
        let ok = Color32::from_rgb(70, 170, 90);
        let warn = Color32::from_rgb(214, 150, 40);
        let bad = Color32::from_rgb(214, 90, 90);
        let open_doc_path = self.doc.as_ref().filter(|d| !d.synthetic).map(|d| d.path.clone());
        let mut st = std::mem::take(&mut self.rc);
        let good: Vec<usize> = (0..st.files.len()).filter(|&i| st.files[i].error.is_none()).collect();
        let lists: Vec<Vec<RecInfo>> = good.iter().map(|&i| st.files[i].records.clone()).collect();
        let planned = recombine::plan(&lists, &st.opts);
        let included = |st: &RecState, p: &recombine::Planned| {
            let f = &st.files[good[p.file]];
            st.manual.get(&(f.path.clone(), p.idx)).copied().unwrap_or(p.skip.is_none())
        };
        let n_in = planned.iter().filter(|p| included(&st, p)).count();
        let (mut open, mut add, mut add_open, mut clear, mut save, mut close) = (true, false, false, false, false, false);
        let mut mv: Option<(usize, usize)> = None;
        let mut remove: Option<usize> = None;
        let mut toggles: Vec<((PathBuf, usize), bool)> = vec![];
        self.base_window(ctx, "Recombine Split AMS Files", W_RECOMB, Vec2::new(1120.0, 760.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.15);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            ui.label(RichText::new("Join .ams files that were exported in parts into one file. Pick the parts, drag them into the order they should follow each other, check the suggested order of the records, and save.").weak().small());
            ui.horizontal(|ui| {
                add = ui.button("Add files ...").clicked();
                add_open = ui.add_enabled(open_doc_path.is_some(), egui::Button::new("Add the open file")).clicked();
                clear = ui.add_enabled(!st.files.is_empty(), egui::Button::new("Clear")).clicked();
            });
            ui.columns(2, |cols| {
                // ---------------------------------------------------------------- the parts
                let ui = &mut cols[0];
                ui.label(RichText::new("Parts").strong());
                ui.label(RichText::new("Drag a file above or below another one: the first one comes first.").weak().small());
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().id_salt("rc-files").max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
                        if st.files.is_empty() {
                            ui.label(RichText::new("No files yet.").weak());
                        }
                        for i in 0..st.files.len() {
                            let f = &st.files[i];
                            ui.horizontal(|ui| {
                                let item = ui.dnd_drag_source(ui.make_persistent_id(("rc-file", i)), i, |ui| {
                                    ui.style_mut().interaction.selectable_labels = false;
                                    ui.label(RichText::new("\u{2630}").weak());
                                    ui.label(RichText::new(&f.name).strong());
                                    match &f.error {
                                        Some(e) => {
                                            ui.label(RichText::new(e).small().color(bad));
                                        }
                                        None => {
                                            ui.label(RichText::new(format!("{} records", f.records.len())).small().weak());
                                            if recombine::is_sorted(&f.records, st.opts.within) {
                                                ui.label(RichText::new("already in order").small().color(ok));
                                            } else {
                                                ui.label(RichText::new("order differs").small().color(warn));
                                            }
                                        }
                                    }
                                });
                                let resp = item.response;
                                if let (Some(pos), Some(_)) = (ui.input(|i| i.pointer.interact_pos()), resp.dnd_hover_payload::<usize>()) {
                                    let above = pos.y < resp.rect.center().y;
                                    let y = if above { resp.rect.top() - 2.0 } else { resp.rect.bottom() + 2.0 };
                                    ui.painter().hline(resp.rect.x_range(), y, Stroke::new(2.5, accent));
                                    if let Some(from) = resp.dnd_release_payload::<usize>() {
                                        mv = Some((*from, if above { i } else { i + 1 }));
                                    }
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.small_button("\u{2715}").on_hover_text("Remove from the list").clicked() {
                                        remove = Some(i);
                                    }
                                });
                            });
                        }
                    });
                });
                ui.label(RichText::new("Suggestions (untick what you do not want)").strong());
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.checkbox(&mut st.opts.order, "Order the records by specimen ID");
                    ui.add_enabled_ui(st.opts.order, |ui| {
                        ui.indent("rc-within", |ui| {
                            ui.radio_value(&mut st.opts.within, Within::NumberThenLetter, "A1, B1, C1, then A2, B2, C2 ...");
                            ui.radio_value(&mut st.opts.within, Within::LetterThenNumber, "A1, A2, A3, then B1, B2, B3 ...");
                        });
                    });
                    ui.checkbox(&mut st.opts.skip_duplicates, "Leave out records that are identical to an earlier one");
                    ui.checkbox(&mut st.opts.one_per_name, "Keep one record per specimen name (the file listed last wins)");
                    if !st.opts.order {
                        ui.label(RichText::new("Not ordered: the files follow each other in the order of the list, the records stay as they are in each file.").weak().small());
                    }
                });
                // by example
                ui.label(RichText::new("The logic by example").strong());
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let ex: Vec<RecInfo> = ["ZZ10-A1", "ZZ2-C1", "ZZ2-A2", "ZZ2-A1", "ZZ2-B1", "ZZ2-B2", "ZZ2-C2"].iter().map(|n| RecInfo { name: n.to_string(), bytes: vec![n.len() as u8; 1] }).collect();
                    let exp = recombine::plan(&[ex.clone()], &Options { skip_duplicates: false, ..st.opts });
                    let names = |v: Vec<usize>| v.into_iter().map(|i| ex[i].name.clone()).collect::<Vec<_>>().join("  ");
                    ui.label(RichText::new(format!("in the files:   {}", names((0..ex.len()).collect()))).monospace().small());
                    ui.label(RichText::new(format!("after:          {}", names(exp.iter().map(|p| p.idx).collect()))).monospace().small().color(accent));
                    ui.label(RichText::new(if st.opts.order {
                        "Samples stay together and are read like a person would (ZZ2 before ZZ10). Inside a sample the sub-specimens run through the letters for each number (or the other way round, as chosen). Names that do not end in letters and a number keep their place by name and by their order in the source files; records that compare equal keep the order of the files and of the records in them."
                    } else {
                        "Nothing is reordered."
                    }).weak().small());
                });

                // ---------------------------------------------------------------- the result
                let ui = &mut cols[1];
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Combined file").strong());
                    ui.label(RichText::new(format!("{n_in} of {} records", planned.len())).small().color(if n_in == planned.len() { ok } else { warn }));
                });
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let h = (ui.available_height() - 120.0).max(260.0);
                    egui::ScrollArea::both().id_salt("rc-result").max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                        egui::Grid::new("rc-grid").striped(true).num_columns(5).spacing([14.0, 4.0]).show(ui, |ui| {
                            for hd in ["", "#", "Specimen", "From", "Note"] {
                                ui.label(RichText::new(hd).strong());
                            }
                            ui.end_row();
                            let mut k = 0;
                            for p in &planned {
                                let f = &st.files[good[p.file]];
                                let mut inc = included(&st, p);
                                let was = inc;
                                if ui.checkbox(&mut inc, "").changed() && inc != was {
                                    toggles.push(((f.path.clone(), p.idx), inc));
                                }
                                if inc {
                                    k += 1;
                                }
                                let dim = |s: String| if inc { RichText::new(s) } else { RichText::new(s).weak().strikethrough() };
                                ui.label(if inc { RichText::new(k.to_string()).weak() } else { RichText::new("\u{2013}").weak() });
                                ui.label(dim(f.records[p.idx].name.clone()));
                                ui.label(RichText::new(&f.name).small().weak());
                                ui.label(RichText::new(match p.skip {
                                    Some(Skip::Duplicate) => "identical to an earlier record",
                                    Some(Skip::Superseded) => "a later file has this name",
                                    None => "",
                                }).small().color(warn));
                                ui.end_row();
                            }
                            if planned.is_empty() {
                                ui.label("");
                                ui.label("");
                                ui.label(RichText::new("Add the parts to see the combined list.").weak());
                                ui.end_row();
                            }
                        });
                    });
                });
            });
            ui.horizontal(|ui| {
                let go = egui::Button::new(RichText::new("Save combined file ...").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(190.0, 0.0));
                save = ui.add_enabled(n_in > 0, go).clicked();
                ui.checkbox(&mut st.open_after, "open it afterwards");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                });
            });
            if !st.msg.is_empty() {
                ui.label(RichText::new(&st.msg).small().color(ok));
            }
            let _ = Sense::hover();
        });
        if close {
            open = false;
        }
        for (k, v) in toggles {
            st.manual.insert(k, v);
        }
        if let Some(i) = remove {
            let f = st.files.remove(i);
            st.manual.retain(|k, _| k.0 != f.path);
        }
        if let Some((from, to)) = mv {
            // from / to are positions in the whole list (files with errors included)
            if from < st.files.len() {
                let f = st.files.remove(from);
                let at = if to > from { to - 1 } else { to }.min(st.files.len());
                st.files.insert(at, f);
            }
        }
        if clear {
            st.files.clear();
            st.manual.clear();
            st.msg.clear();
        }
        self.rc = st;
        if add {
            let mut dlg = rfd::FileDialog::new().add_filter("AMS files", &["ams"]).set_title("Parts of a split .ams file");
            if self.browser_dir.is_dir() {
                dlg = dlg.set_directory(&self.browser_dir);
            }
            // headless test hook: AMS_STUDIO_PICK_FILES lists the files (separated by ;) instead of the dialog
            let picked = std::env::var("AMS_STUDIO_PICK_FILES").ok().map(|v| v.split(';').map(PathBuf::from).collect::<Vec<_>>()).or_else(|| dlg.pick_files());
            if let Some(ps) = picked {
                self.rc_add(ps);
            }
        }
        if add_open {
            if let Some(p) = open_doc_path {
                self.rc_add(vec![p]);
            }
        }
        if save {
            self.rc_save();
        }
        self.win_recomb = open;
    }

    fn rc_save(&mut self) {
        let st = &self.rc;
        let good: Vec<&SrcFile> = st.files.iter().filter(|f| f.error.is_none()).collect();
        let lists: Vec<Vec<RecInfo>> = good.iter().map(|f| f.records.clone()).collect();
        let planned = recombine::plan(&lists, &st.opts);
        let mut bytes: Vec<u8> = vec![];
        let mut n = 0;
        for p in &planned {
            let f = good[p.file];
            if st.manual.get(&(f.path.clone(), p.idx)).copied().unwrap_or(p.skip.is_none()) {
                bytes.extend_from_slice(&f.records[p.idx].bytes);
                n += 1;
            }
        }
        let first = good.first().map(|f| f.path.clone());
        let stem = first.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or("recombined".into());
        let mut dlg = rfd::FileDialog::new().add_filter("AMS files", &["ams"]).set_title("Save the combined file").set_file_name(format!("{stem}_recombined.ams"));
        if let Some(d) = first.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(d);
        }
        let Some(p) = dlg.save_file() else { return };
        match std::fs::write(&p, &bytes) {
            Ok(()) => {
                self.rc.msg = format!("Saved {n} records to {}.", p.display());
                if self.rc.open_after {
                    self.open_path(p);
                }
            }
            Err(e) => self.info = Some(format!("Could not save: {e}")),
        }
    }
}
