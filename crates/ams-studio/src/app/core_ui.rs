//! Edit > Drill Core to Geo: the cubes of a drill core are rotated into geographic coordinates with the Owens
//! construction applied to the top of the core (see `ams_core::core`). Each question has its own line, the prompt
//! above the choice; the explanations are in the (i) pop-ups.

use super::gis_loc::{self, Hole};
use super::gis_manual_ui::{collar_fields, fill_from_saved, resolve_collar, ManualState};
use super::{info_icon, tools_ui, App, W_CORE};
use ams_core::depjson;
use ams_core::core::{offset_for, slice_of, top_surface_lhr, Arrow, CoreSetup, Face};
use egui::{Pos2, Sense, Shape, Stroke};
use ams_core::georot::GeoRot;
use egui::{Align, Align2, Color32, FontId, Layout, RichText, Vec2};

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
    /// Optional: where the specimen was taken (the collar of the hole and the depth along it), saved for the GIS viewer
    /// when the rotation is applied. `collar` holds the boxes of the entry.
    pub collar_on: bool,
    pub collar: ManualState,
    pub err: String,
}

impl Default for CoreState {
    fn default() -> Self {
        CoreState { setup: CoreSetup { dip_dir: 316.135, plunge: 67.0, ..Default::default() }, first: 1, last: 1, msg: String::new(), intervals_on: false, by_slice: true, intervals: vec![], collar_on: false, collar: ManualState::default(), err: String::new() }
    }
}

impl App {
    pub(super) fn open_core(&mut self) {
        let Some(d) = &self.doc else { return };
        let (n, path) = (d.specs.len().max(1), d.path.clone());
        self.core.first = 1;
        self.core.last = n;
        self.core.msg.clear();
        self.core.err.clear();
        // a position saved for this specimen is shown (and the tick is set); otherwise the entry starts empty and unticked
        let zone = self.zone_hint();
        let mut collar = ManualState { zone, north: true, ..Default::default() };
        let saved = gis_loc::read(&depjson::for_source(&path));
        if let Some(loc) = &saved {
            fill_from_saved(&mut collar, loc);
            collar.depth = loc.depth.map_or(String::new(), |d| format!("{d}"));
        }
        self.core.collar_on = saved.is_some();
        self.core.collar = collar;
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
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            tools_ui::pop_style(ui, 1.25);
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
            ui.spacing_mut().button_padding = Vec2::new(12.0, 6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Built for cubes cut from halved drill core").strong().size(19.0));
                info_icon(ui, "The cubes are turned into geographic coordinates with the Owens construction applied to the top of the core: the end of the core is a surface whose normal is the hole axis, entered as a left-hand-rule surface (strike = dip direction + 90, dip = 90 - plunge). The blue arrow and the orientation line (BOH mark) decide how the cube sits in the core. The same rotation as the AMS dashboard (BOREHOLE_CONFIGS): with the defaults the results agree with it to 0.001 degrees.");
            });
            ui.separator();

            // the face and the direction of the arrow are not asked: halved core always has them the same way
            st.setup.face = Face::UpHole;
            st.setup.arrow = Arrow::Right;
            ui.horizontal_wrapped(|ui| {
                ui.label("This tool was built for halved core. It always assumes that the face of each cube that points up the hole is the one that carries the blue arrow, and that the arrow points to the right when you look at that face from outside.");
                info_icon(ui, "The blue arrow drawn on the cube is the alignment mark for the Kappabridge holder: it fixes the cube's specimen axes (x along the arrow, z into the cube through the up-hole face). For halved core this is always the face that looked up the hole, with the arrow to the right as seen from outside, which is what the AMS dashboard assumes; other faces and directions are not offered.");
            });
            ui.separator();

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

            // 3b. where the hole is (optional): saved for the GIS viewer with the rotation
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.collar_on, "Also enter where this specimen was taken (for the GIS viewer)");
                info_icon(ui, "Optional. The two coordinates of the collar of the hole and, if you like, the depth of this specimen along the hole are saved with the specimen (in its _geo dependency file) when you press Rotate, together with the dip direction and plunge entered above. The GIS viewer then has them ready: the specimen is on the map, in the box of its collar in order of depth, with the thin line for the direction of the hole, and the Drill core tick of the coordinate entry is already set. Leave it unticked or empty to do it later in the GIS viewer.");
            });
            if st.collar_on {
                egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Coordinates of the collar of the hole").strong());
                    collar_fields(ui, &mut st.collar);
                });
            }

            // 4. the orientation line
            ui.label(RichText::new("BOH offset: where is the BOH mark relative to the blue arrow?").strong());
            core_sketches(ui);
            ui.horizontal(|ui| {
                ui.label("BOH offset to enter");
                ui.add(egui::DragValue::new(&mut st.setup.offset).range(0.0..=360.0).speed(0.5).max_decimals(1).suffix(" \u{b0}"));
                info_icon(ui, "The clockwise turn, looking down the hole, that brings the orientation line (the line along the core that marks the bottom of the hole, the BOH mark) to the blue arrow. This is the BOH_ROTA of the AMS dashboard; enter the turn, not the position of the line: a BOH mark that lies a° to the left of the arrow (looking down the hole) is 360 - a, one that lies a° to the right is a, and 0 when the arrow is on the BOH mark.\nThe offsets and everything else of the drill core reorientation are kept in a dependency file, <specimen folder>_geo_dependancy.json, that Save as .ams creates in the destination (_geo) folder.");
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
            ui.label(RichText::new("Rows that will be rotated").strong());
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
            if !st.err.is_empty() {
                ui.label(RichText::new(&st.err).small().color(Color32::from_rgb(200, 80, 40)));
            }
            if !st.msg.is_empty() {
                ui.label(RichText::new(&st.msg).small().color(Color32::from_rgb(70, 170, 90)));
            }
            });
        });
        if rotate {
            // the optional position is checked first: a mistake in it stops the rotation, so nothing is half done
            st.err.clear();
            let collar = if st.collar_on { resolve_collar(&st.collar, Hole::new(st.setup.dip_dir, st.setup.plunge)) } else { Ok(None) };
            if let Err(e) = &collar {
                st.err = e.clone();
            }
            let collar = collar.unwrap_or(None);
            if st.err.is_empty() {
            // every record gets the offset of its interval (the single offset when intervals are not used)
            let names: Vec<String> = self.doc.as_ref().map(|d| d.specs.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
            let jobs = core_jobs(&st.setup, st.intervals_on.then_some((st.by_slice, st.intervals.as_slice())), &names, st.first - 1..st.last);
            match self.rotate_doc(&jobs) {
                Ok(()) => {
                    st.msg = format!("The reorientation of {} record(s) is held; the geographic view (F6) shows it and the specimen coordinates stay as measured. Nothing is saved yet: File > Save as .ams writes the _geo copy and, next to it, the dependency file <specimen folder>_geo_dependancy.json that holds the drill core reorientation.", jobs.len());
                    if let (Some(loc), Some(path)) = (collar, self.doc.as_ref().map(|d| d.path.clone())) {
                        match gis_loc::write(&depjson::for_source(&path), Some(&loc)) {
                            Ok(()) => {
                                self.gis.loc.ver += 1;
                                self.gis.loc.stale = true;
                                self.gis.fit_pending = true;
                                st.msg += " The collar position is saved for the GIS viewer.";
                            }
                            Err(e) => st.err = format!("Could not save the position: {e}"),
                        }
                    }
                }
                Err(e) => self.info = Some(e),
            }
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

/// Three small cross-sections of the halved core, seen looking down the hole with the flat cut face on top: the blue
/// arrow (a line down the middle of the cut face to the bottom of the core), the BOH mark (yellow, on the round
/// surface) to the left of it, to the right of it and on it, and the angle to enter for each.
fn core_sketches(ui: &mut egui::Ui) {
    let ink = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let blue = Color32::from_rgb(0, 170, 230);
    let red = Color32::from_rgb(225, 70, 55);
    let yellow = Color32::from_rgb(255, 215, 0);
    let w = ((ui.available_width() - 24.0) / 3.0).clamp(150.0, 230.0);
    let h = 168.0;
    let cases: [(&str, f32, &str); 3] = [
        ("BOH mark to the left of the arrow", -1.0, "enter 360\u{b0} \u{2212} angle\n(4\u{b0} left: enter 356\u{b0})"),
        ("BOH mark to the right of the arrow", 1.0, "enter the angle\n(4\u{b0} right: enter 4\u{b0})"),
        ("BOH mark on the arrow", 0.0, "enter 0\u{b0}"),
    ];
    ui.horizontal(|ui| {
        for (title, side, caption) in cases {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
            let pa = ui.painter_at(rect);
            pa.text(Pos2::new(rect.center().x, rect.top() + 2.0), Align2::CENTER_TOP, title, FontId::proportional(12.0), ink);
            let r = (w * 0.33).min(62.0);
            let c = Pos2::new(rect.center().x, rect.top() + 30.0);
            // the halved core: flat cut face on top, the round surface below
            let outline = Stroke::new(2.0, ink);
            pa.line_segment([Pos2::new(c.x - r, c.y), Pos2::new(c.x + r, c.y)], outline);
            let arc: Vec<Pos2> = (0..=48).map(|k| { let a = std::f32::consts::PI * k as f32 / 48.0; Pos2::new(c.x + r * a.cos(), c.y + r * a.sin()) }).collect();
            pa.add(Shape::line(arc, outline));
            // the blue arrow: down the middle of the cut face to the bottom of the core
            pa.line_segment([c, Pos2::new(c.x, c.y + r)], Stroke::new(3.5, blue));
            // the BOH mark, 28 degrees off in the sketch (the real offsets are a few degrees)
            let phi = 28.0f32.to_radians() * side.abs();
            let at = |rad: f32| Pos2::new(c.x + side * rad * phi.sin(), c.y + rad * phi.cos());
            if side != 0.0 {
                pa.line_segment([c, at(r)], Stroke::new(1.6, red));
                let ang: Vec<Pos2> = (0..=16).map(|k| { let a = phi * k as f32 / 16.0; Pos2::new(c.x + side * 0.55 * r * a.sin(), c.y + 0.55 * r * a.cos()) }).collect();
                pa.add(Shape::line(ang, Stroke::new(1.6, red)));
                pa.text(Pos2::new(c.x + side * 0.30 * r, c.y + 0.92 * r + 12.0), Align2::CENTER_CENTER, "angle", FontId::proportional(11.5), red);
            }
            let dot = at(r);
            pa.circle_filled(dot, 5.5, yellow);
            pa.circle_stroke(dot, 5.5, Stroke::new(1.0, Color32::from_rgb(120, 100, 0)));
            pa.text(Pos2::new(rect.center().x, c.y + r + 24.0), Align2::CENTER_TOP, caption, FontId::proportional(12.0), weak);
        }
    });
    ui.label(RichText::new("Cross-section of the halved core, looking down the hole, flat cut face on top (angles exaggerated). Blue line = the blue arrow, yellow dot = the BOH mark, the line along the core that marks the bottom of the hole.").weak().small());
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
