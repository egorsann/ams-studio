//! Edit > Drill Core to Geo: the cubes of a drill core are rotated into geographic coordinates with the Owens
//! construction applied to the top of the core (see `ams_core::core`). Each question has its own line, the prompt
//! above the choice; the explanations are in the (i) pop-ups.

use super::{info_icon, tools_ui, App, W_CORE};
use ams_core::core::{offset_for, slice_of, top_surface_lhr, Arrow, CoreSetup, Face};
use ams_core::georot::GeoRot;
use egui::{Align, Color32, Layout, RichText, Vec2};

pub(super) struct CoreState {
    pub setup: CoreSetup,
    /// 1-based first and last row the rotation is applied to (preset to the whole file).
    pub first: usize,
    pub last: usize,
    pub msg: String,
    /// Separate offsets for intervals (as in the dashboard's BOREHOLE_CONFIGS); the list is only shown when ticked.
    pub intervals_on: bool,
    /// The intervals are slice numbers read from the names (A12 = slice 12) instead of rows.
    pub by_slice: bool,
    pub intervals: Vec<(u32, u32, f64)>,
}

impl Default for CoreState {
    fn default() -> Self {
        CoreState { setup: CoreSetup { dip_dir: 316.135, plunge: 67.0, ..Default::default() }, first: 1, last: 1, msg: String::new(), intervals_on: false, by_slice: true, intervals: vec![] }
    }
}

impl App {
    pub(super) fn open_core(&mut self) {
        let Some(d) = &self.doc else { return };
        self.core.first = 1;
        self.core.last = d.specs.len().max(1);
        self.core.msg.clear();
        self.open_win(W_CORE);
    }

    pub(super) fn core_window(&mut self, ctx: &egui::Context) {
        if !self.win_core {
            return;
        }
        let Some(d) = &self.doc else {
            self.win_core = false;
            return;
        };
        let n = d.specs.len().max(1);
        let name_of = |r: usize| d.specs.get(r.wrapping_sub(1)).map(|s| s.name.clone()).unwrap_or_default();
        let (first_name, last_name) = (name_of(self.core.first.clamp(1, n)), name_of(self.core.last.clamp(1, n)));
        let dark = self.dark;
        let can_undo = self.owens_undo.is_some();
        let mut st = std::mem::take(&mut self.core);
        st.first = st.first.clamp(1, n);
        st.last = st.last.clamp(st.first, n);
        let (mut open, mut rotate, mut undo, mut cancel) = (true, false, false, false);
        let scr = ctx.content_rect();
        let size = Vec2::new((scr.width() - 40.0).clamp(560.0, 760.0), (scr.height() - 60.0).clamp(520.0, 760.0));
        self.base_window(ctx, "Drill Core to Geo", W_CORE, size).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.25);
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
            ui.spacing_mut().button_padding = Vec2::new(12.0, 6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Built for cubes cut from drill core").strong().size(19.0));
                info_icon(ui, "The cubes are turned into geographic coordinates with the Owens construction applied to the top of the core: the end of the core is a surface whose normal is the hole axis, entered as a left-hand-rule surface (strike = dip direction + 90, dip = 90 - plunge). The answers about the arrow and the orientation line decide how the cube sits in the core. The same rotation as the AMS dashboard (BOREHOLE_CONFIGS): with the defaults the results agree with it to 0.001 degrees.");
            });
            ui.separator();

            // 1. the face that holds the arrow
            ui.label(RichText::new("Which face of the cube holds the arrow?").strong());
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("core-face").selected_text(st.setup.face.name()).width(360.0).show_ui(ui, |ui| {
                    for f in Face::ALL {
                        ui.selectable_value(&mut st.setup.face, f, f.name());
                    }
                });
                info_icon(ui, "The face of the cube, named by where it looked in the core, that carries the arrow mark. Default: the face that looked up the hole. The cube's z axis points into the cube through this face.");
            });

            // 2. the direction of the arrow
            ui.label(RichText::new("Which way does the arrow point (looking at that face from outside)?").strong());
            ui.horizontal(|ui| {
                for a in Arrow::ALL {
                    ui.radio_value(&mut st.setup.arrow, a, a.name());
                }
                info_icon(ui, "Seen from outside the cube, looking at the face that holds the arrow. On the up-hole and down-hole faces 'up' is towards the top of the core; on the side faces 'up' is towards the top of the hole. Default: right. The cube's x axis is the direction of the arrow.");
            });

            // 3. the drill hole
            ui.label(RichText::new("Drill hole survey").strong());
            ui.horizontal(|ui| {
                ui.label("dip direction");
                ui.add(egui::DragValue::new(&mut st.setup.dip_dir).range(0.0..=360.0).speed(0.2).max_decimals(3).suffix(" \u{b0}"));
                ui.add_space(10.0);
                ui.label("plunge");
                ui.add(egui::DragValue::new(&mut st.setup.plunge).range(0.0..=90.0).speed(0.2).max_decimals(3).suffix(" \u{b0}"));
                info_icon(ui, "Dip direction (azimuth the hole goes down towards) and plunge (angle of the hole below the horizontal) from the drill hole data.");
            });
            let (strike, dip) = top_surface_lhr(st.setup.dip_dir, st.setup.plunge);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Top of the core as a left-hand-rule surface: strike {strike:.1}\u{b0}, dip {dip:.1}\u{b0}")).weak());
                info_icon(ui, "The end of the core is perpendicular to the hole, so its strike (left-hand rule: index finger along the strike, thumb towards the dip direction) is the dip direction + 90 and its dip is 90 - plunge. These are the values the Owens construction uses as the field strike and dip.");
            });

            // 4. the orientation line
            ui.label(RichText::new("Offset of the orientation line from the arrow mark on the front surface").strong());
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut st.setup.offset).range(0.0..=360.0).speed(0.5).max_decimals(1).suffix(" \u{b0}"));
                info_icon(ui, "The clockwise turn, looking down the hole, that brings the orientation line (the line along the core that marks the bottom of the hole) to the bottom, measured from the arrow mark. This is the BOH_ROTA of the AMS dashboard; enter the turn, not the position of the line. 0 when the arrow mark is the bottom of the hole.");
            });

            // 4b. separate offsets for intervals of the core (only when ticked)
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.intervals_on, "Separate offsets for intervals of the core");
                info_icon(ui, "For a core whose orientation line was read in pieces: each interval gets its own offset, like the intervals of BOREHOLE_CONFIGS in the AMS dashboard. The offset above is used for everything outside the intervals.");
            });
            if st.intervals_on {
                egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("What do the intervals count?").strong());
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut st.by_slice, true, "Slice numbers in the names (A12 = slice 12)").on_hover_text("As in the dashboard: the number in a name like A12, B3, C27. Records whose name has no slice number use the offset above.");
                        ui.radio_value(&mut st.by_slice, false, "Rows of the file");
                    });
                    ui.label(RichText::new("Intervals (both ends included)").strong());
                    let mut remove: Option<usize> = None;
                    for (k, iv) in st.intervals.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label("from");
                            ui.add(egui::DragValue::new(&mut iv.0).range(0..=100000).speed(0.2));
                            ui.label("to");
                            let lo = iv.0;
                            ui.add(egui::DragValue::new(&mut iv.1).range(lo..=100000).speed(0.2));
                            ui.label("offset");
                            ui.add(egui::DragValue::new(&mut iv.2).range(0.0..=360.0).speed(0.5).max_decimals(1).suffix(" \u{b0}"));
                            if ui.small_button("\u{2715}").on_hover_text("Remove this interval").clicked() {
                                remove = Some(k);
                            }
                        });
                    }
                    if let Some(k) = remove {
                        st.intervals.remove(k);
                    }
                    if ui.button("Add interval").clicked() {
                        let next = st.intervals.last().map_or(0, |l| l.1 + 1);
                        st.intervals.push((next, next, st.setup.offset));
                    }
                });
            }

            // 5. the rows
            ui.label(RichText::new("Rotate rows").strong());
            ui.horizontal(|ui| {
                ui.label("from the first row");
                ui.add(egui::DragValue::new(&mut st.first).range(1..=n).speed(0.2));
                ui.add_space(6.0);
                ui.label("to the last row");
                let first = st.first;
                ui.add(egui::DragValue::new(&mut st.last).range(first..=n).speed(0.2));
                info_icon(ui, "The rows (records of the open file, numbered as in the Edit Data table) that are rotated. Preset to the whole file. A record that is already rotated to geographic coordinates (Owens or drill core) is refused.");
            });
            ui.label(RichText::new(format!("Row {} ({}) to row {} ({}): {} of {n} rows will be rotated (n={}).", st.first, first_name, st.last, last_name, st.last - st.first + 1, st.last - st.first + 1)).weak().small());
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let go = egui::Button::new(RichText::new("Rotate").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(120.0, 36.0));
                rotate = ui.add(go).on_hover_text("Rotate the cubes of the open file (in memory; Save as .ams writes them)").clicked();
                undo = ui.add_enabled(can_undo, egui::Button::new(RichText::new("Undo last rotation").small()).small()).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| cancel = ui.button("Cancel").clicked());
            });
            if !st.msg.is_empty() {
                ui.label(RichText::new(&st.msg).small().color(Color32::from_rgb(70, 170, 90)));
            }
        });
        if rotate {
            // every record gets the offset of its interval (the single offset when intervals are not used)
            let names: Vec<String> = self.doc.as_ref().map(|d| d.specs.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
            let jobs = core_jobs(&st.setup, st.intervals_on.then_some((st.by_slice, st.intervals.as_slice())), &names, st.first - 1..st.last);
            match self.rotate_doc(&jobs) {
                Ok(()) => st.msg = format!("The reorientation of {} record(s) is stored; the geographic view (F6) shows it and the specimen coordinates stay as measured. Nothing is saved yet: File > Save as .ams writes it into the file.", jobs.len()),
                Err(e) => self.info = Some(e),
            }
        }
        if undo {
            self.undo_doc_rotation();
            st.msg = "The last rotation was undone.".into();
        }
        self.core = st;
        self.win_core = open && !cancel;
    }
}

/// The reorientation of every record of `rows` (0-based): each record gets the offset of the interval that holds its
/// slice number (or row number), the single offset of `setup` outside the intervals and for names without a slice.
pub(super) fn core_jobs(setup: &CoreSetup, intervals: Option<(bool, &[(u32, u32, f64)])>, names: &[String], rows: std::ops::Range<usize>) -> Vec<(usize, GeoRot)> {
    rows.map(|i| {
        let offset = match intervals {
            Some((by_slice, iv)) => {
                let key = if by_slice { names.get(i).and_then(|n| slice_of(n)) } else { Some(i as u32 + 1) };
                key.map_or(setup.offset, |k| offset_for(iv, k, setup.offset))
            }
            None => setup.offset,
        };
        (i, GeoRot::Core(CoreSetup { offset, ..*setup }))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ams_core::core::core_rotation;

    #[test]
    fn intervals_give_every_record_the_offset_of_its_slice() {
        let setup = CoreSetup { dip_dir: 316.135, plunge: 67.0, offset: 0.0, ..Default::default() };
        let names: Vec<String> = ["C1", "B13", "A14", "B42", "D!8", "A50"].iter().map(|s| s.to_string()).collect();
        let iv = [(0, 13, 356.0), (14, 42, 352.0)];
        let jobs = core_jobs(&setup, Some((true, &iv)), &names, 0..names.len());
        let same = |g: &GeoRot, off: f64| {
            let a = g.matrix();
            let b = core_rotation(&CoreSetup { offset: off, ..setup });
            (0..3).all(|i| (0..3).all(|j| (a[i][j] - b[i][j]).abs() < 1e-12))
        };
        // slices 1 and 13 -> 356, 14 and 42 -> 352, no slice / outside the intervals -> the single offset 0
        assert!(same(&jobs[0].1, 356.0) && same(&jobs[1].1, 356.0));
        assert!(same(&jobs[2].1, 352.0) && same(&jobs[3].1, 352.0));
        assert!(same(&jobs[4].1, 0.0) && same(&jobs[5].1, 0.0));
        // intervals off: the single offset for everything; by rows: the row number counts
        assert!(core_jobs(&setup, None, &names, 0..2).iter().all(|j| same(&j.1, 0.0)));
        let by_row = core_jobs(&setup, Some((false, &[(1, 2, 90.0)])), &names, 0..4);
        assert!(same(&by_row[0].1, 90.0) && same(&by_row[1].1, 90.0) && same(&by_row[2].1, 0.0));
    }
}
