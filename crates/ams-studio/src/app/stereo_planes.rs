//! Planes entered by hand (a vein, a fault, bedding measured in the field) that the stereonet draws as great circles
//! in the geographic net, below the data, with a comment written next to each. The list is kept in
//! `stereo_planes.json` in the preferences folder (not tied to a specimen); whether they are shown, whether the
//! comments are shown and how they are entered (dip direction or strike) are in `state.txt`.

use super::{config_dir, info_icon, App};
use crate::plots::PlaneDraw;
use egui::{Color32, RichText, Ui};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Clone, PartialEq, Debug)]
pub(super) struct StPlane {
    pub on: bool,
    /// Dip direction and dip, degrees (the strike entry is converted).
    pub dip_dir: f64,
    pub dip: f64,
    pub comment: String,
    pub color: [u8; 3],
    /// The specimen (id of the file, `DC26AB002`) the plane belongs to; `None` = global, drawn for every specimen.
    /// Chosen when the plane is added and kept; it is not changed afterwards.
    pub spec: Option<String>,
}

/// Colours that are not the K1 / K2 / K3 ones.
const PALETTE: [[u8; 3]; 6] = [[230, 120, 30], [150, 70, 200], [0, 150, 140], [210, 50, 120], [140, 100, 40], [120, 135, 30]];

impl StPlane {
    pub fn new(n: usize, spec: Option<String>) -> StPlane {
        StPlane { on: true, dip_dir: 90.0, dip: 60.0, comment: String::new(), color: PALETTE[n % PALETTE.len()], spec }
    }

    /// Drawn for the specimen with this id (`None` = no file open: only the global planes).
    pub fn applies_to(&self, open_id: Option<&str>) -> bool {
        match (&self.spec, open_id) {
            (None, _) => true,
            (Some(s), Some(o)) => s.eq_ignore_ascii_case(o),
            (Some(_), None) => false,
        }
    }
}

fn path() -> PathBuf {
    config_dir().join("ams-studio").join("stereo_planes.json")
}

pub(super) fn read() -> Vec<StPlane> {
    let v: Value = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let Some(list) = v.get("planes").and_then(|p| p.as_array()) else { return vec![] };
    list.iter()
        .enumerate()
        .filter_map(|(n, p)| {
            let num = |k: &str| p.get(k).and_then(|x| x.as_f64());
            let col = p.get("color").and_then(|c| c.as_array()).filter(|c| c.len() == 3).map(|c| [0, 1, 2].map(|i| c[i].as_u64().unwrap_or(128).min(255) as u8)).unwrap_or(PALETTE[n % PALETTE.len()]);
            Some(StPlane { on: p.get("on").and_then(|x| x.as_bool()).unwrap_or(true), dip_dir: num("dip_direction")?.rem_euclid(360.0), dip: num("dip")?.clamp(0.0, 90.0), comment: p.get("comment").and_then(|c| c.as_str()).unwrap_or("").to_string(), color: col, spec: p.get("specimen").and_then(|c| c.as_str()).filter(|c| !c.is_empty()).map(|c| c.to_string()) })
        })
        .collect()
}

fn write(planes: &[StPlane]) {
    let p = path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let list: Vec<Value> = planes.iter().map(|q| json!({ "on": q.on, "dip_direction": q.dip_dir, "dip": q.dip, "comment": q.comment, "color": q.color, "specimen": q.spec })).collect();
    let v = json!({ "kind": "AMS Studio stereonet planes", "version": 1, "note": "Planes drawn as great circles in the geographic stereonet, below the data. Dip direction and dip in degrees. specimen = null: global, else only for that specimen.", "planes": list });
    let _ = std::fs::write(p, serde_json::to_string_pretty(&v).unwrap_or_default() + "\n");
}

impl App {
    /// The id of the open specimen (`None` while no file is open).
    fn open_spec_id(&self) -> Option<String> {
        Some(self.cv_open_id()).filter(|i| !i.is_empty())
    }

    /// What the stereonet draws: the switched-on planes, only when the net shows geographic coordinates.
    pub(super) fn plane_draws(&self) -> Vec<PlaneDraw> {
        if !self.st_planes_on || self.coord != ams_core::coords::CoordSys::Geographic {
            return vec![];
        }
        let open_id = self.open_spec_id();
        self.st_planes
            .iter()
            .filter(|q| q.on && q.applies_to(open_id.as_deref()))
            .map(|q| PlaneDraw { dip_dir: q.dip_dir, dip: q.dip, color: Color32::from_rgb(q.color[0], q.color[1], q.color[2]), comment: if self.st_plane_comments { q.comment.trim().to_string() } else { String::new() } })
            .collect()
    }

    /// The "Planes" part of the stereonet's options.
    pub(super) fn planes_ui(&mut self, ui: &mut Ui) {
        let before = self.st_planes.clone();
        let state_before = (self.st_planes_on, self.st_plane_comments, self.st_plane_strike, self.st_plane_new_spec);
        let open_id = self.open_spec_id();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.st_planes_on, RichText::new("Planes").strong());
            info_icon(ui, "Planes you measured in the field (a vein, a fault, bedding), drawn as great circles, not poles. They are shown only while the net is in geographic coordinates (G), below the data. The comment is written next to the circle where it reaches its steepest dip.");
        });
        let geo = self.coord == ams_core::coords::CoordSys::Geographic;
        if self.st_planes_on && !geo {
            ui.label(RichText::new("Not shown now: the net is not in geographic coordinates.").weak().small());
        }
        ui.add_enabled_ui(self.st_planes_on, |ui| {
            ui.indent("planes", |ui| {
                ui.checkbox(&mut self.st_plane_comments, "Show the comments");
                ui.horizontal(|ui| {
                    ui.label("Enter as");
                    egui::ComboBox::from_id_salt("plane-conv").selected_text(if self.st_plane_strike { "Strike / dip (right-hand rule)" } else { "Dip direction / dip" }).width(200.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.st_plane_strike, false, "Dip direction / dip");
                        ui.selectable_value(&mut self.st_plane_strike, true, "Strike / dip (right-hand rule)");
                    });
                });
                let mut remove: Option<usize> = None;
                let strike = self.st_plane_strike;
                let n_planes = self.st_planes.len();
                let open = open_id.as_deref();
                for (i, q) in self.st_planes.iter_mut().enumerate().filter(|(_, q)| q.applies_to(open)) {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut q.on, "");
                        let mut a = if strike { (q.dip_dir - 90.0).rem_euclid(360.0) } else { q.dip_dir };
                        ui.label(if strike { "Strike" } else { "Dip dir." });
                        if ui.add(egui::DragValue::new(&mut a).range(0.0..=360.0).speed(0.5).max_decimals(1).suffix(" \u{b0}")).changed() {
                            q.dip_dir = if strike { (a + 90.0).rem_euclid(360.0) } else { a.rem_euclid(360.0) };
                        }
                        ui.label("Dip");
                        ui.add(egui::DragValue::new(&mut q.dip).range(0.0..=90.0).speed(0.5).max_decimals(1).suffix(" \u{b0}"));
                        let mut c = Color32::from_rgb(q.color[0], q.color[1], q.color[2]);
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            q.color = [c.r(), c.g(), c.b()];
                        }
                        if ui.small_button("\u{2715}").on_hover_text("Remove this plane").clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut q.comment).hint_text("comment (for example: phyllic vein V3)").desired_width(ui.available_width() - 120.0));
                        // the type is fixed when the plane is added
                        match &q.spec {
                            None => ui.label(RichText::new("global").weak().small()).on_hover_text("Drawn for every specimen"),
                            Some(s) => ui.label(RichText::new(format!("only {s}")).weak().small()).on_hover_text("Drawn only for this specimen"),
                        };
                    });
                }
                if let Some(i) = remove {
                    self.st_planes.remove(i);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add_enabled(open_id.is_some(), egui::Checkbox::new(&mut self.st_plane_new_spec, "Specimen specific"));
                    info_icon(ui, "Chosen before the plane is added and kept with it. On: the new plane belongs to the open specimen and is drawn only for it. Off: the new plane is global and is drawn for every specimen. It cannot be changed once the plane is added.");
                });
                let specific = self.st_plane_new_spec && open_id.is_some();
                let label = match (&open_id, specific) {
                    (Some(id), true) => format!("Add a plane for {id} only"),
                    _ => "Add a global plane".to_string(),
                };
                if ui.button(label).clicked() {
                    self.st_planes.push(StPlane::new(n_planes, if specific { open_id.clone() } else { None }));
                }
            });
        });
        if self.st_planes != before {
            write(&self.st_planes);
        }
        if state_before != (self.st_planes_on, self.st_plane_comments, self.st_plane_strike, self.st_plane_new_spec) {
            self.save_state();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_plane_gets_the_next_palette_colour() {
        assert_ne!(StPlane::new(0, None).color, StPlane::new(1, None).color);
        assert_eq!(StPlane::new(0, None).color, StPlane::new(PALETTE.len(), None).color);
    }

    #[test]
    fn a_global_plane_is_for_every_specimen_a_specific_one_only_for_its_own() {
        let g = StPlane::new(0, None);
        let s = StPlane::new(1, Some("DC26AB002".into()));
        assert!(g.applies_to(Some("DC26AB012")) && g.applies_to(None));
        assert!(s.applies_to(Some("dc26ab002")) && !s.applies_to(Some("DC26AB012")) && !s.applies_to(None));
    }
}
