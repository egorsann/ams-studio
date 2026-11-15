//! Planes entered by hand (a vein, a fault, bedding measured in the field) that the stereonet draws as great circles
//! in the geographic net, below the data, with a comment written next to each.
//!
//! They are kept with the data, in the `stereonet` section of the dependency file of the specimen's `_geo` folder
//! (`ams_core::depjson`): `planes_specimen` for the planes of that specimen, `planes_global` for the planes of every
//! specimen (a copy of the global ones is kept in the file of each specimen of the file tree). Whether the planes and
//! their comments are shown, how they are entered and whether new planes are specimen specific are display
//! preferences and stay in `state.txt`.

use super::{config_dir, info_icon, App};
use crate::plots::PlaneDraw;
use ams_core::depjson;
use egui::{Color32, RichText, Ui};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, PartialEq, Debug)]
pub(super) struct StPlane {
    /// Identifies the plane when the global ones are merged between specimens.
    pub id: u64,
    pub on: bool,
    /// Dip direction and dip, degrees (a strike entry is converted).
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

fn fresh_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    t.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ COUNTER.fetch_add(1, Ordering::Relaxed)
}

impl StPlane {
    pub fn new(n: usize, spec: Option<String>) -> StPlane {
        StPlane { id: fresh_id(), on: true, dip_dir: 90.0, dip: 60.0, comment: String::new(), color: PALETTE[n % PALETTE.len()], spec }
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

// ---------------------------------------------------------------- entering a plane

/// How a plane is typed in: strike with the left-hand rule (the default), strike with the right-hand rule, or the dip direction.
pub(super) const CONV_NAMES: [&str; 3] = ["Strike / dip (left-hand rule)", "Strike / dip (right-hand rule)", "Dip direction / dip"];

/// Dip direction from what was typed. Left-hand rule: the plane dips to the left when looking along the strike
/// (dip direction = strike - 90); right-hand rule: to the right (strike + 90).
pub(super) fn to_dip_dir(conv: u8, typed: f64) -> f64 {
    match conv {
        0 => (typed - 90.0).rem_euclid(360.0),
        1 => (typed + 90.0).rem_euclid(360.0),
        _ => typed.rem_euclid(360.0),
    }
}

/// What is shown in the field for a stored dip direction (the inverse of `to_dip_dir`).
pub(super) fn from_dip_dir(conv: u8, dip_dir: f64) -> f64 {
    match conv {
        0 => (dip_dir + 90.0).rem_euclid(360.0),
        1 => (dip_dir - 90.0).rem_euclid(360.0),
        _ => dip_dir.rem_euclid(360.0),
    }
}

/// The side the plane dips towards, as shown behind the numbers so the reading of the entry can be checked:
/// E or W, and N or S for a plane that strikes exactly east-west.
pub(super) fn side_letter(dip_dir: f64) -> &'static str {
    let (s, c) = dip_dir.to_radians().sin_cos();
    if s > 1e-9 {
        "E"
    } else if s < -1e-9 {
        "W"
    } else if c > 0.0 {
        "N"
    } else {
        "S"
    }
}

// ---------------------------------------------------------------- the dependency file

fn plane_json(q: &StPlane) -> Value {
    json!({ "id": q.id.to_string(), "on": q.on, "dip_direction": q.dip_dir, "dip": q.dip, "comment": q.comment, "color": q.color })
}

fn parse_planes(list: Option<&Value>, spec: Option<&str>) -> Vec<StPlane> {
    let Some(list) = list.and_then(|p| p.as_array()) else { return vec![] };
    list.iter()
        .enumerate()
        .filter_map(|(n, p)| {
            let num = |k: &str| p.get(k).and_then(|x| x.as_f64());
            let col = p.get("color").and_then(|c| c.as_array()).filter(|c| c.len() == 3).map(|c| [0, 1, 2].map(|i| c[i].as_u64().unwrap_or(128).min(255) as u8)).unwrap_or(PALETTE[n % PALETTE.len()]);
            let id = p.get("id").and_then(|x| x.as_str()).and_then(|x| x.parse::<u64>().ok()).unwrap_or_else(fresh_id);
            Some(StPlane { id, on: p.get("on").and_then(|x| x.as_bool()).unwrap_or(true), dip_dir: num("dip_direction")?.rem_euclid(360.0), dip: num("dip")?.clamp(0.0, 90.0), comment: p.get("comment").and_then(|c| c.as_str()).unwrap_or("").to_string(), color: col, spec: spec.map(|s| s.to_string()) })
        })
        .collect()
}

/// The planes stored in a dependency file: (global ones, the ones of the specimen `spec`); `None` when the file has no
/// `stereonet` section.
pub(super) fn read_file(json_path: &Path, spec: &str) -> Option<(Vec<StPlane>, Vec<StPlane>)> {
    let s = depjson::read_section(json_path, "stereonet")?;
    Some((parse_planes(s.get("planes_global"), None), parse_planes(s.get("planes_specimen"), Some(spec))))
}

/// Stores the global planes and / or the planes of the specimen (`None` leaves that part as it is) in a dependency
/// file. The `_geo` folder and the file are made when there is something to keep; an empty section is removed again.
pub(super) fn write_file(json_path: &Path, globals: Option<&[StPlane]>, specifics: Option<&[StPlane]>) -> std::io::Result<()> {
    let mut s = depjson::read_section(json_path, "stereonet").filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
    if let Some(g) = globals {
        s["planes_global"] = Value::Array(g.iter().map(plane_json).collect());
    }
    if let Some(p) = specifics {
        s["planes_specimen"] = Value::Array(p.iter().map(plane_json).collect());
    }
    let n = |k: &str| s.get(k).and_then(|x| x.as_array()).map_or(0, |a| a.len());
    let empty = n("planes_global") == 0 && n("planes_specimen") == 0;
    depjson::write_section(json_path, "stereonet", if empty { None } else { Some(s) })
}

/// The planes of the first version, kept in the preferences folder (`stereo_planes.json`), with ids made from their
/// content so that the same plane read for several specimens is still one plane.
pub(super) fn read_legacy() -> Vec<StPlane> {
    let file: PathBuf = config_dir().join("ams-studio").join("stereo_planes.json");
    let v: Value = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let Some(list) = v.get("planes").and_then(|p| p.as_array()) else { return vec![] };
    list.iter()
        .enumerate()
        .filter_map(|(n, p)| {
            let num = |k: &str| p.get(k).and_then(|x| x.as_f64());
            let col = p.get("color").and_then(|c| c.as_array()).filter(|c| c.len() == 3).map(|c| [0, 1, 2].map(|i| c[i].as_u64().unwrap_or(128).min(255) as u8)).unwrap_or(PALETTE[n % PALETTE.len()]);
            let spec = p.get("specimen").and_then(|c| c.as_str()).filter(|c| !c.is_empty()).map(|c| c.to_string());
            let mut q = StPlane { id: 0, on: p.get("on").and_then(|x| x.as_bool()).unwrap_or(true), dip_dir: num("dip_direction")?.rem_euclid(360.0), dip: num("dip")?.clamp(0.0, 90.0), comment: p.get("comment").and_then(|c| c.as_str()).unwrap_or("").to_string(), color: col, spec };
            // FNV-1a over what identifies the plane
            let key = format!("{}|{}|{}|{:?}|{:?}", q.dip_dir, q.dip, q.comment, q.color, q.spec);
            q.id = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3));
            Some(q)
        })
        .collect()
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

    /// Every frame: the planes of the specimen that was just opened are read from its dependency file, and edits are
    /// written once the pointer is released and a moment has passed (not at every step of a drag or a keystroke).
    pub(super) fn planes_poll(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let cur = self.doc.as_ref().filter(|d| !d.synthetic).map(|d| d.path.clone());
        if cur != self.planes_for {
            if self.planes_dirty.is_some() {
                if let Some(old) = self.planes_for.clone() {
                    self.planes_flush(&old);
                }
                self.planes_dirty = None;
            }
            self.planes_for = cur.clone();
            match cur {
                Some(p) => self.planes_load(&p, now),
                None => self.st_planes.retain(|q| q.spec.is_none()),
            }
        }
        if let Some(t) = self.planes_dirty {
            if now - t > 0.8 && !ctx.input(|i| i.pointer.any_down()) {
                if let Some(f) = self.planes_for.clone() {
                    self.planes_flush(&f);
                }
                self.planes_dirty = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
    }

    /// The global planes seen so far plus the ones in the specimen's file, and the specimen's own.
    fn planes_load(&mut self, file: &Path, now: f64) {
        let json = depjson::for_source(file);
        let id = super::core_pane::spec_id(file);
        let found = read_file(&json, &id);
        let had = found.is_some();
        let (file_g, mut specific) = found.unwrap_or_default();
        let mut globals: Vec<StPlane> = self.st_planes.iter().filter(|q| q.spec.is_none()).cloned().collect();
        if !had {
            // the first version kept the planes in the preferences folder: they move into this specimen's file
            for q in &self.planes_legacy {
                match &q.spec {
                    None if !globals.iter().any(|g| g.id == q.id) => globals.push(q.clone()),
                    Some(s) if s.eq_ignore_ascii_case(&id) && !specific.iter().any(|g| g.id == q.id) => specific.push(q.clone()),
                    _ => {}
                }
            }
        }
        let missing_in_file = globals.iter().any(|g| !file_g.iter().any(|f| f.id == g.id));
        for g in file_g {
            if !globals.iter().any(|x| x.id == g.id) {
                globals.push(g);
            }
        }
        let changed = missing_in_file || (!had && !specific.is_empty());
        globals.extend(specific);
        self.st_planes = globals;
        if changed {
            self.planes_dirty = Some(now);
        }
    }

    /// Writes the planes into the dependency file of the open specimen and the global ones into the file of every
    /// specimen of the file tree (their `_geo` folders are made when needed).
    fn planes_flush(&mut self, file: &Path) {
        let id = super::core_pane::spec_id(file);
        let globals: Vec<StPlane> = self.st_planes.iter().filter(|q| q.spec.is_none()).cloned().collect();
        let specific: Vec<StPlane> = self.st_planes.iter().filter(|q| q.spec.as_deref().map_or(false, |s| s.eq_ignore_ascii_case(&id))).cloned().collect();
        let json = depjson::for_source(file);
        let _ = write_file(&json, Some(&globals), Some(&specific));
        for p in self.cv_files() {
            let j = depjson::for_source(&p);
            if j != json {
                let _ = write_file(&j, Some(&globals), None);
            }
        }
    }

    /// The "Planes" part of the stereonet's options.
    pub(super) fn planes_ui(&mut self, ui: &mut Ui) {
        let before = self.st_planes.clone();
        let state_before = (self.st_planes_on, self.st_plane_comments, self.st_plane_conv, self.st_plane_new_spec);
        let open_id = self.open_spec_id();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.st_planes_on, RichText::new("Planes").strong());
            info_icon(ui, "Planes you measured in the field (a vein, a fault, bedding), drawn as great circles, not poles. They are shown only while the net is in geographic coordinates (G), below the data. The comment is written next to the circle where it reaches its steepest dip. The planes are kept with the data, in the dependency file of the specimen's _geo folder.");
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
                    egui::ComboBox::from_id_salt("plane-conv").selected_text(CONV_NAMES[self.st_plane_conv.min(2) as usize]).width(220.0).show_ui(ui, |ui| {
                        for (k, n) in CONV_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut self.st_plane_conv, k as u8, *n);
                        }
                    });
                    info_icon(ui, "Left-hand rule: looking along the strike direction the plane dips to the left (dip direction = strike - 90 degrees). Right-hand rule: it dips to the right (strike + 90 degrees). The letter behind the dip is the side the entry is read as dipping towards (E or W; N or S for a plane that strikes exactly east-west), so the reading can be checked.");
                });
                let mut remove: Option<usize> = None;
                let conv = self.st_plane_conv.min(2);
                let n_planes = self.st_planes.len();
                let open = open_id.as_deref();
                for (i, q) in self.st_planes.iter_mut().enumerate().filter(|(_, q)| q.applies_to(open)) {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut q.on, "");
                        let mut a = from_dip_dir(conv, q.dip_dir);
                        ui.label(if conv == 2 { "Dip dir." } else { "Strike" });
                        if ui.add(egui::DragValue::new(&mut a).range(0.0..=360.0).speed(0.5).max_decimals(1).suffix(" \u{b0}")).changed() {
                            q.dip_dir = to_dip_dir(conv, a);
                        }
                        ui.label("Dip");
                        ui.add(egui::DragValue::new(&mut q.dip).range(0.0..=90.0).speed(0.5).max_decimals(1).suffix(" \u{b0}"));
                        let side = side_letter(q.dip_dir);
                        ui.label(RichText::new(side).strong().size(15.0)).on_hover_text(format!("Read as dipping towards {side}: dip direction {:.1}\u{b0} ({}).", q.dip_dir, CONV_NAMES[conv as usize]));
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
                if ui.add_enabled(open_id.is_some(), egui::Button::new(label)).on_hover_text("A file has to be open: the plane is stored next to its data").clicked() {
                    self.st_planes.push(StPlane::new(n_planes, if specific { open_id.clone() } else { None }));
                }
            });
        });
        if self.st_planes != before {
            self.planes_dirty = Some(ui.ctx().input(|i| i.time));
        }
        if state_before != (self.st_planes_on, self.st_plane_comments, self.st_plane_conv, self.st_plane_new_spec) {
            self.save_state();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_plane_gets_the_next_palette_colour_and_its_own_id() {
        assert_ne!(StPlane::new(0, None).color, StPlane::new(1, None).color);
        assert_eq!(StPlane::new(0, None).color, StPlane::new(PALETTE.len(), None).color);
        assert_ne!(StPlane::new(0, None).id, StPlane::new(0, None).id);
    }

    #[test]
    fn a_global_plane_is_for_every_specimen_a_specific_one_only_for_its_own() {
        let g = StPlane::new(0, None);
        let s = StPlane::new(1, Some("DC26AB002".into()));
        assert!(g.applies_to(Some("DC26AB012")) && g.applies_to(None));
        assert!(s.applies_to(Some("dc26ab002")) && !s.applies_to(Some("DC26AB012")) && !s.applies_to(None));
    }

    #[test]
    fn the_left_hand_rule_dips_to_the_left_of_the_strike_and_the_letter_says_which_side() {
        // strike 0 (north): left-hand rule dips to the west, right-hand rule to the east
        assert!((to_dip_dir(0, 0.0) - 270.0).abs() < 1e-9 && side_letter(to_dip_dir(0, 0.0)) == "W");
        assert!((to_dip_dir(1, 0.0) - 90.0).abs() < 1e-9 && side_letter(to_dip_dir(1, 0.0)) == "E");
        assert_eq!(to_dip_dir(2, 135.0), 135.0);
        // strike 45: left-hand rule dips towards 315 (north-west, W), right-hand towards 135 (south-east, E)
        assert_eq!(side_letter(to_dip_dir(0, 45.0)), "W");
        assert_eq!(side_letter(to_dip_dir(1, 45.0)), "E");
        // a plane striking east-west dips north or south
        assert_eq!(side_letter(to_dip_dir(0, 90.0)), "N");
        assert_eq!(side_letter(to_dip_dir(0, 270.0)), "S");
        // the entry converts back to what was typed
        for conv in 0..3u8 {
            for a in [0.0, 17.5, 90.0, 200.0, 359.0] {
                assert!((from_dip_dir(conv, to_dip_dir(conv, a)) - a).abs() < 1e-9, "{conv} {a}");
            }
        }
    }

    #[test]
    fn planes_live_in_the_specimens_dependency_file_next_to_the_data() {
        let base = std::env::temp_dir().join(format!("planes-test-{}", std::process::id()));
        let src = base.join("DC26AB002_AMS").join("DC26AB002_use.ams");
        let json = depjson::for_source(&src);
        let g = StPlane::new(0, None);
        let s = StPlane::new(1, Some("DC26AB002".into()));
        assert!(!json.exists());
        write_file(&json, Some(std::slice::from_ref(&g)), Some(std::slice::from_ref(&s))).unwrap();
        assert!(json.is_file() && json.to_string_lossy().ends_with("DC26AB002_AMS_geo/DC26AB002_AMS_geo_dependancy.json"));
        let (rg, rs) = read_file(&json, "DC26AB002").unwrap();
        assert_eq!(rg, vec![g.clone()]);
        assert_eq!(rs, vec![s.clone()]);
        // writing only the global ones leaves the specimen's own
        write_file(&json, Some(&[]), None).unwrap();
        let (rg, rs) = read_file(&json, "DC26AB002").unwrap();
        assert!(rg.is_empty() && rs == vec![s]);
        // nothing left: the file goes
        write_file(&json, None, Some(&[])).unwrap();
        assert!(!json.exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
