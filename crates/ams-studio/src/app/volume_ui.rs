//! Edit > Volume Correction: the sample volume (and whether the sample is a cube or a cylinder).
//!
//! The volume goes into the `Volume` field of the chosen rows (preset: all rows; first and last row like the Owens
//! window) of the open file (the field Anisoft's Edit Data shows). The shape has no
//! place in an `.ams` record, so it is kept in `<file>.volume.json` next to the file, after the user has been told.
//! Km as displayed stays as stored; the parameters marked * (corrected mean susceptibility and unnormalised K) use
//! `10 / volume`, because Km is reported for the Kappabridge's 10 cm3.

use crate::util::SafeClamp;
use super::{tools_ui, App, W_VOL};
use crate::doc::{self, VolInfo};
use ams_core::params::REF_VOLUME;
use egui::{Align, Color32, Layout, RichText, Vec2};

pub(super) struct VolState {
    pub volume: f64,
    /// 0 cubic, 1 cylindrical
    pub shape: usize,
    /// side (cube) or diameter and height (cylinder), in mm
    pub dims: [f64; 3],
    /// the "this will be stored in ..." prompt is showing
    pub confirm: bool,
    /// what the json next to the open file says, read when the file was opened
    pub info: Option<VolInfo>,
    pub msg: String,
    /// 1-based first and last row the volume is applied to (preset to the whole file)
    pub first: usize,
    pub last: usize,
}

impl Default for VolState {
    fn default() -> Self {
        VolState { volume: REF_VOLUME, shape: 0, dims: [20.0, 25.0, 22.0], confirm: false, info: None, msg: String::new(), first: 1, last: 1 }
    }
}

const SHAPES: [&str; 2] = ["cubic", "cylindrical"];

impl App {
    /// Reads the json next to the open file (called when a file is opened).
    pub(super) fn load_volume_info(&mut self) {
        self.vol.info = self.doc.as_ref().filter(|d| !d.synthetic).and_then(|d| self.vol_pending.get(&d.path).cloned().or_else(|| doc::read_volume_json(&d.path)));
    }

    /// What is known about the volume of the open file, for the menu's hover text and the window.
    pub(super) fn volume_summary(&self) -> String {
        let Some(d) = &self.doc else { return String::new() };
        let vols: Vec<f64> = d.specs.iter().map(|s| s.volume_cm3).collect();
        let (lo, hi) = vols.iter().fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
        let fmt = |v: f64| format!("{v:.3}").trim_end_matches('0').trim_end_matches('.').to_string();
        let extra = match &self.vol.info {
            Some(i) => format!(" ({}{}{})", if i.shape.is_empty() { "shape not recorded".to_string() } else { i.shape.clone() }, if i.applied.is_empty() { "" } else { ", set " }, i.applied),
            None => String::new(),
        };
        if vols.is_empty() {
            String::new()
        } else if hi - lo > 1e-6 {
            format!("The records have different volumes: {} to {} cm\u{b3}.{extra}", fmt(lo), fmt(hi))
        } else if (lo - REF_VOLUME).abs() < 1e-6 && self.vol.info.is_none() {
            "No volume correction applied yet: every record has the reference volume of 10 cm\u{b3}.".to_string()
        } else {
            format!("Volume applied: {} cm\u{b3} in all {} records{extra}.", fmt(lo), vols.len())
        }
    }

    pub(super) fn open_volume(&mut self) {
        let Some(d) = &self.doc else { return };
        if let Some(s) = d.specs.first() {
            self.vol.volume = s.volume_cm3;
        }
        // preselected: the whole file
        self.vol.first = 1;
        self.vol.last = d.specs.len().max(1);
        if let Some(i) = &self.vol.info {
            self.vol.shape = SHAPES.iter().position(|s| *s == i.shape).unwrap_or(0);
        }
        self.vol.confirm = false;
        self.vol.msg.clear();
        self.open_win(W_VOL);
    }

    pub(super) fn volume_window(&mut self, ctx: &egui::Context) {
        if !self.win_vol {
            return;
        }
        let Some(d) = &self.doc else {
            self.win_vol = false;
            return;
        };
        let has_path = !d.synthetic;
        let n = d.specs.len().max(1);
        let row_name = |r: usize| d.specs.get(r.wrapping_sub(1)).map(|s| s.name.clone()).unwrap_or_default();
        let json_path = doc::volume_json_path(&d.path);
        let summary = self.volume_summary();
        let dark = self.dark;
        let mut st = std::mem::replace(&mut self.vol, VolState::default());
        st.first = st.first.clamp_safe(1, n);
        st.last = st.last.clamp_safe(st.first, n);
        let (mut open, mut apply, mut store, mut back) = (true, false, false, false);
        self.base_window(ctx, "Volume Correction", W_VOL, Vec2::new(640.0, 560.0)).open(&mut open).resizable(false).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.25);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            ui.label(RichText::new(&summary).strong());
            ui.separator();
            if st.confirm {
                let whole = st.first == 1 && st.last == n;
                let what = if whole { "this file".to_string() } else { format!("rows {} to {} of this file", st.first, st.last) };
                ui.label(RichText::new(format!("Apply {} cm\u{b3} ({}) to {what}?", st.volume, SHAPES[st.shape])).strong());
                ui.label(format!("The volume is written into the {} record(s) of the open file ({}); use File > Save as .ams afterwards to keep it in a file.", st.last - st.first + 1, if whole { "all of them".to_string() } else { format!("{} ... {}", row_name(st.first), row_name(st.last)) }));
                if has_path {
                    ui.label("An .ams file has no place for the sample shape. The shape, the volume and today's date are kept with the file and written to a .json file next to the saved .ams file when you save it (File > Save as .ams); nothing is written to disk now. For the original file that would be:");
                    ui.label(RichText::new(json_path.display().to_string()).monospace().small());
                    if !whole {
                        ui.label(RichText::new("The volume (and shape) of each record is listed one by one in the json, because the records do not all have the same volume.").weak().small());
                    }
                } else {
                    ui.label(RichText::new("This data set has no file behind it yet, so the shape cannot be stored next to it.").color(Color32::from_rgb(214, 150, 40)));
                }
                ui.horizontal(|ui| {
                    let go = egui::Button::new(RichText::new(if has_path { "Apply to the file" } else { "Apply" }).strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(130.0, 0.0));
                    store = ui.add(go).clicked();
                    back = ui.button("Back").clicked();
                });
                return;
            }
            ui.label(RichText::new("Sample shape").strong());
            ui.horizontal(|ui| {
                ui.radio_value(&mut st.shape, 0, "Cubic");
                ui.radio_value(&mut st.shape, 1, "Cylindrical");
            });
            ui.label(RichText::new("Dimensions (optional, to calculate the volume)").weak().small());
            ui.horizontal(|ui| {
                let mut changed = false;
                if st.shape == 0 {
                    ui.label("side");
                    changed |= ui.add(egui::DragValue::new(&mut st.dims[0]).range(1.0..=200.0).speed(0.1).suffix(" mm")).changed();
                } else {
                    ui.label("diameter");
                    changed |= ui.add(egui::DragValue::new(&mut st.dims[1]).range(1.0..=200.0).speed(0.1).suffix(" mm")).changed();
                    ui.label("height");
                    changed |= ui.add(egui::DragValue::new(&mut st.dims[2]).range(1.0..=200.0).speed(0.1).suffix(" mm")).changed();
                }
                if changed {
                    st.volume = if st.shape == 0 { st.dims[0].powi(3) / 1000.0 } else { std::f64::consts::PI * (st.dims[1] / 2.0).powi(2) * st.dims[2] / 1000.0 };
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Volume of the sample").strong());
                ui.add(egui::DragValue::new(&mut st.volume).range(0.01..=1000.0).speed(0.01).max_decimals(3).suffix(" cm\u{b3}"));
                super::info_icon(ui, "Volume of the specimen in cm\u{b3} (a 20 mm cube is 8 cm\u{b3}, a 25.4 mm x 22 mm cylinder about 11.1 cm\u{b3}). It is written into the Volume field of the chosen rows.");
            });
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Apply to rows").strong());
                    super::info_icon(ui, "The rows (records of the open file, numbered as in the Edit Data table) that get this volume. Preset to the whole file; narrow the range to give only some rows this volume and the others another one.");
                    ui.add_space(10.0);
                    ui.label("from the first row");
                    ui.add(egui::DragValue::new(&mut st.first).range(1..=n).speed(0.2));
                    ui.add_space(6.0);
                    ui.label("to the last row");
                    let first = st.first;
                    ui.add(egui::DragValue::new(&mut st.last).range(first..=n).speed(0.2));
                });
                ui.label(RichText::new(format!(
                    "Row {} ({}) to row {} ({}), the file has {n}: {} of {n} rows get this volume.",
                    st.first, row_name(st.first), st.last, row_name(st.last), st.last - st.first + 1
                )).weak().small());
            });
            ui.label(RichText::new("The Kappabridge reports Km for 10 cm\u{b3}. The parameters marked * (corrected mean susceptibility, unnormalised K1-K3) are multiplied by 10 / volume; Km as shown stays as stored in the file.").weak().small());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let go = egui::Button::new(RichText::new("Apply").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(110.0, 0.0));
                apply = ui.add_enabled(st.volume > 0.0, go).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        back = true;
                    }
                });
            });
            if !st.msg.is_empty() {
                ui.label(RichText::new(&st.msg).color(Color32::from_rgb(70, 170, 90)).small());
            }
        });
        if apply {
            st.confirm = true;
        }
        if store {
            let (shape, volume) = (SHAPES[st.shape].to_string(), st.volume);
            if let Some(d) = &mut self.doc {
                let (first, last) = (st.first, st.last);
                let whole = first == 1 && last == d.specs.len();
                d.set_volume_range(first - 1..last, volume);
                let mut msg = format!("{} cm\u{b3} written into {} of the {} records (rows {first} to {last}).", volume, last - first + 1, d.specs.len());
                if has_path {
                    // shapes per record, when the rows do not all share one
                    let before = self.vol_pending.get(&d.path).cloned().or_else(|| st.info.clone());
                    let rec_shapes = if whole {
                        vec![]
                    } else {
                        let base = before.as_ref().map(|i| i.shape.clone()).unwrap_or_default();
                        let mut v = before.as_ref().filter(|i| i.rec_shapes.len() == d.specs.len()).map(|i| i.rec_shapes.clone()).unwrap_or_else(|| vec![base; d.specs.len()]);
                        v[first - 1..last].iter_mut().for_each(|x| *x = shape.clone());
                        v
                    };
                    let info = VolInfo { volume_cm3: volume, shape, applied: doc::today(), rec_shapes };
                    self.vol_pending.insert(d.path.clone(), info.clone());
                    msg += " Shape and date are kept in memory and written next to the file when you save it.";
                    st.info = Some(info);
                }
                st.msg = msg;
            }
            st.confirm = false;
            self.dirty = true;
        }
        if back {
            if st.confirm {
                st.confirm = false;
            } else {
                open = false;
            }
        }
        self.vol = st;
        self.win_vol = open;
    }
}
