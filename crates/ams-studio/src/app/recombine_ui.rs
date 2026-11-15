//! Edit > Recombine Split AMS Files: joins `.ams` files that were exported in parts into one file, in a suggested
//! order by specimen ID (see `ams_core::recombine`). The files can be dragged into the order they should follow each
//! other; the suggestions can be switched off; single records can be left out or dragged to another place in the
//! combined list.

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
    /// The order the user gave the records by dragging (file, index in the file); `None` = the suggested order.
    pub order: Option<Vec<(PathBuf, usize)>>,
    /// The table of duplicates is showing.
    pub show_dups: bool,
}

impl Default for RecState {
    fn default() -> Self {
        RecState { files: vec![], opts: Options::default(), manual: HashMap::new(), open_after: true, msg: String::new(), order: None, show_dups: false }
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

/// The planned list, in the order the user dragged the records into (if they did). Also gives the files that load.
fn plan_ordered(st: &RecState) -> (Vec<usize>, Vec<recombine::Planned>) {
    let good: Vec<usize> = (0..st.files.len()).filter(|&i| st.files[i].error.is_none()).collect();
    let lists: Vec<Vec<RecInfo>> = good.iter().map(|&i| st.files[i].records.clone()).collect();
    let mut planned = recombine::plan(&lists, &st.opts);
    if let Some(o) = &st.order {
        let pos: HashMap<&(PathBuf, usize), usize> = o.iter().enumerate().map(|(i, k)| (k, i)).collect();
        // stable: records the dragged order does not know keep their suggested place at the end
        planned.sort_by_key(|p| pos.get(&(st.files[good[p.file]].path.clone(), p.idx)).copied().unwrap_or(usize::MAX));
    }
    (good, planned)
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
        // the files selected in the file browser (Shift range, Ctrl single), else the open file
        let selected: Vec<PathBuf> = self.selected_or_open().into_iter().filter(|p| crate::doc::is_ams(p)).collect();
        let mut st = std::mem::take(&mut self.rc);
        let opts_before = st.opts;
        let (good, planned) = plan_ordered(&st);
        let included = |st: &RecState, p: &recombine::Planned| {
            let f = &st.files[good[p.file]];
            st.manual.get(&(f.path.clone(), p.idx)).copied().unwrap_or(p.skip.is_none())
        };
        let n_in = planned.iter().filter(|p| included(&st, p)).count();
        // records that are byte for byte the same as an earlier one: (specimen, its file, the earlier record's file and name)
        let dups: Vec<(String, String, String)> = {
            let lists: Vec<Vec<RecInfo>> = good.iter().map(|&i| st.files[i].records.clone()).collect();
            let all = recombine::plan(&lists, &Options { skip_duplicates: true, one_per_name: false, ..st.opts });
            let mut first_seen: HashMap<&[u8], (&str, &str)> = HashMap::new();
            let mut v = vec![];
            for p in &all {
                let (f, r) = (&st.files[good[p.file]], &st.files[good[p.file]].records[p.idx]);
                match (p.skip, first_seen.get(r.bytes.as_slice())) {
                    (Some(Skip::Duplicate), Some((ff, fname))) => v.push((r.name.clone(), f.name.clone(), format!("{fname} in {ff}"))),
                    _ => {
                        first_seen.entry(r.bytes.as_slice()).or_insert((f.name.as_str(), r.name.as_str()));
                    }
                }
            }
            v
        };
        // why a change of the order may not move anything: names that cannot be read, or both orders giving one list
        let n_readable = planned.iter().filter(|p| recombine::split_id(&st.files[good[p.file]].records[p.idx].name).1.is_some()).count();
        let same_both = {
            let lists: Vec<Vec<RecInfo>> = good.iter().map(|&i| st.files[i].records.clone()).collect();
            let other = if st.opts.within == Within::NumberThenLetter { Within::LetterThenNumber } else { Within::NumberThenLetter };
            let a = recombine::plan(&lists, &st.opts);
            let b = recombine::plan(&lists, &Options { within: other, ..st.opts });
            a.iter().map(|p| (p.file, p.idx)).eq(b.iter().map(|p| (p.file, p.idx)))
        };
        let (mut open, mut add, mut add_open, mut clear, mut save, mut close) = (true, false, false, false, false, false);
        let mut mv: Option<(usize, usize)> = None;
        let mut rec_mv: Option<(usize, usize)> = None;
        let mut reset_order = false;
        let mut remove: Option<usize> = None;
        let mut toggles: Vec<((PathBuf, usize), bool)> = vec![];
        // opens in the top left corner of the program window and fills it to the bottom and right
        let cr = ctx.content_rect();
        // never larger than the program window, so the bottom row (Save to ...) stays on screen and the edges can be dragged
        let avail = Vec2::new((cr.width() - 24.0).max(420.0), (cr.height() - 56.0).max(360.0));
        let size = Vec2::new(avail.x.min(1500.0), avail.y.min(900.0));
        self.base_window(ctx, "Recombine Split AMS Files", W_RECOMB, size).max_size(avail).min_size(Vec2::new(420.0, 340.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.15);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            ui.label(RichText::new("Join .ams files that were exported in parts into one file. Pick the parts, drag them into the order they should follow each other, check the suggested order of the records, and save.").weak().small());
            ui.horizontal(|ui| {
                add = ui.button("Add files ...").clicked();
                let label = if selected.len() > 1 { format!("Add the currently selected (n={})", selected.len()) } else { "Add the currently selected".to_string() };
                add_open = ui.add_enabled(!selected.is_empty(), egui::Button::new(label)).on_hover_text("The files selected in the file browser (Shift+click for a range, Ctrl+click for single files); with nothing selected, the open file").clicked();
                clear = ui.add_enabled(!st.files.is_empty(), egui::Button::new("Clear")).clicked();
            });
            // the two columns get the height that is left above the bottom row
            let body_h = (ui.available_height() - 70.0).max(120.0);
            ui.columns(2, |cols| {
                // ---------------------------------------------------------------- the parts
                egui::ScrollArea::vertical().id_salt("rc-left").max_height(body_h).auto_shrink([false, false]).show(&mut cols[0], |ui| {
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
                    ui.label("Records that are identical to an earlier one:");
                    ui.horizontal_wrapped(|ui| {
                        ui.radio_value(&mut st.opts.skip_duplicates, true, "Leave them out").on_hover_text("Only a record that is byte for byte the same as an earlier one is left out");
                        ui.radio_value(&mut st.opts.skip_duplicates, false, "Keep duplicates");
                        let label = if st.show_dups { "(hide duplicates)" } else { "(show duplicates)" };
                        if ui.add(egui::Label::new(RichText::new(label).weak()).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            st.show_dups = !st.show_dups;
                        }
                        ui.label(RichText::new(format!("{} found", dups.len())).weak().small());
                    });
                    if st.show_dups {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::both().id_salt("rc-dups").max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                                if dups.is_empty() {
                                    ui.label(RichText::new("No record is identical to an earlier one.").weak());
                                } else {
                                    egui::Grid::new("rc-dup-grid").striped(true).num_columns(4).spacing([14.0, 4.0]).show(ui, |ui| {
                                        for hd in ["#", "Specimen", "In file", "Identical to"] {
                                            ui.label(RichText::new(hd).strong());
                                        }
                                        ui.end_row();
                                        for (i, (name, file, of)) in dups.iter().enumerate() {
                                            ui.label(RichText::new((i + 1).to_string()).weak());
                                            ui.label(name);
                                            ui.label(RichText::new(file).small().weak());
                                            ui.label(RichText::new(of).small().weak());
                                            ui.end_row();
                                        }
                                    });
                                }
                            });
                        });
                    }
                    ui.checkbox(&mut st.opts.one_per_name, "Keep one record per specimen name (the file listed last wins)")
                        .on_hover_text("For specimens that were measured again: when the same name occurs in several parts with different data, only the record from the part listed last is kept and the older ones are left out. (Leaving out duplicates only drops records that are exactly the same.)");
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

                });

                // ---------------------------------------------------------------- the result
                let ui = &mut cols[1];
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Combined file").strong());
                    ui.label(RichText::new(format!("{n_in} of {} records", planned.len())).small().color(if n_in == planned.len() { ok } else { warn }));
                    if st.order.is_some() && ui.small_button("Back to the suggested order").on_hover_text("Forget the order you dragged the records into").clicked() {
                        reset_order = true;
                    }
                });
                if st.opts.order && !planned.is_empty() {
                    if n_readable < planned.len() {
                        ui.label(RichText::new(format!("{n_readable} of {} names read as letter + number (A1) or number + letter (1A). The others keep their file order, so the A1 B1 C1 / A1 A2 A3 choice cannot move them.", planned.len())).small().color(warn));
                    } else if same_both {
                        ui.label(RichText::new("For these names both orders give the same list (for example only one letter, or one number).").small().color(warn));
                    }
                }
                ui.label(RichText::new("Drag a record by its handle to another place in the list; untick it to leave it out.").weak().small());
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let h = (body_h - 60.0).max(80.0);
                    egui::ScrollArea::both().id_salt("rc-result").max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                        egui::Grid::new("rc-grid").striped(true).num_columns(5).spacing([14.0, 4.0]).show(ui, |ui| {
                            for hd in ["", "#", "Specimen", "From", "Note"] {
                                ui.label(RichText::new(hd).strong());
                            }
                            ui.end_row();
                            let mut k = 0;
                            for (row, p) in planned.iter().enumerate() {
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
                                let item = ui.dnd_drag_source(ui.make_persistent_id(("rc-rec", row)), row, |ui| {
                                    ui.style_mut().interaction.selectable_labels = false;
                                    ui.label(RichText::new("\u{2630}").weak());
                                    ui.label(dim(f.records[p.idx].name.clone()));
                                });
                                let resp = item.response;
                                if let (Some(pos), Some(_)) = (ui.input(|i| i.pointer.interact_pos()), resp.dnd_hover_payload::<usize>()) {
                                    let above = pos.y < resp.rect.center().y;
                                    let y = if above { resp.rect.top() - 2.0 } else { resp.rect.bottom() + 2.0 };
                                    ui.painter().hline(resp.rect.left() - 60.0..=resp.rect.right() + 260.0, y, Stroke::new(2.5, accent));
                                    if let Some(from) = resp.dnd_release_payload::<usize>() {
                                        rec_mv = Some((*from, if above { row } else { row + 1 }));
                                    }
                                }
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
                let go = egui::Button::new(RichText::new("Save to ...").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(190.0, 0.0));
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
        if st.opts != opts_before || reset_order || rec_mv.is_some() || mv.is_some() {
            // the table on the right was drawn before the change: run this pass again before anything is shown
            ctx.request_repaint();
            ctx.request_discard("recombine: options changed");
        }
        if st.opts != opts_before || reset_order {
            // a new suggestion replaces what was dragged by hand
            st.order = None;
        }
        if let Some((from, to)) = rec_mv {
            let mut keys: Vec<(PathBuf, usize)> = planned.iter().map(|p| (st.files[good[p.file]].path.clone(), p.idx)).collect();
            if from < keys.len() {
                let k = keys.remove(from);
                keys.insert(if to > from { to - 1 } else { to }.min(keys.len()), k);
                st.order = Some(keys);
            }
        }
        for (k, v) in toggles {
            st.manual.insert(k, v);
        }
        if let Some(i) = remove {
            let f = st.files.remove(i);
            st.manual.retain(|k, _| k.0 != f.path);
            st.order = None;
        }
        if let Some((from, to)) = mv {
            // from / to are positions in the whole list (files with errors included)
            if from < st.files.len() {
                let f = st.files.remove(from);
                let at = if to > from { to - 1 } else { to }.min(st.files.len());
                st.files.insert(at, f);
                st.order = None;
            }
        }
        if clear {
            st.files.clear();
            st.manual.clear();
            st.order = None;
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
            self.rc_add(selected);
        }
        if save {
            self.rc_save();
        }
        self.win_recomb = open;
    }

    fn rc_save(&mut self) {
        let st = &self.rc;
        let (idx, planned) = plan_ordered(st);
        let good: Vec<&SrcFile> = idx.iter().map(|&i| &st.files[i]).collect();
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
