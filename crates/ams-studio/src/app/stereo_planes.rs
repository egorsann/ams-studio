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
    /// A plane given by the alpha / beta angles of its trace on the core (and the survey of the hole) instead of typed
    /// in as strike / dip; `dip_dir` and `dip` are then worked out from it (`recompute`).
    pub ab: Option<AlphaBeta>,
}

/// The measurement a vein plane is calculated from: the angles on the whole core and the survey of the hole.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct AlphaBeta {
    /// Angle between the plane and the core axis, 0 to 90 degrees.
    pub alpha: f64,
    /// Clockwise looking downhole from the BOH line to the downhole apex of the trace, 0 to 360 degrees.
    pub beta: f64,
    /// Direction the hole goes down towards, and its plunge below the horizontal.
    pub hole_dip_dir: f64,
    pub plunge: f64,
    /// The name of the vein in the dashboard code it was taken from (`None` = entered here).
    pub src: Option<String>,
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
        StPlane { id: fresh_id(), on: true, dip_dir: 90.0, dip: 60.0, comment: String::new(), color: PALETTE[n % PALETTE.len()], spec, ab: None }
    }

    /// A vein plane from alpha / beta and the survey of the hole.
    pub fn from_alpha_beta(n: usize, spec: Option<String>, ab: AlphaBeta, comment: &str) -> StPlane {
        let mut q = StPlane::new(n, spec);
        q.comment = comment.to_string();
        q.ab = Some(ab);
        q.recompute();
        q
    }

    /// Works the geographic dip direction and dip out of the alpha / beta of a vein plane (nothing for a typed plane).
    pub fn recompute(&mut self) {
        if let Some(a) = &self.ab {
            let (dd, dip) = crate::veins::plane_geographic(a.alpha, a.beta, a.hole_dip_dir, a.plunge);
            self.dip_dir = dd;
            self.dip = dip.clamp(0.0, 90.0);
        }
    }

    /// The same plane as `o`: the same scope and the same numbers (the angles and the survey for a vein plane); colour
    /// and comment do not make a plane a different one.
    pub fn same_as(&self, o: &StPlane) -> bool {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        self.spec.as_deref().map(|s| s.to_lowercase()) == o.spec.as_deref().map(|s| s.to_lowercase())
            && match (&self.ab, &o.ab) {
                (Some(a), Some(b)) => close(a.alpha, b.alpha) && close(a.beta, b.beta) && close(a.hole_dip_dir, b.hole_dip_dir) && close(a.plunge, b.plunge),
                (None, None) => close(self.dip_dir, o.dip_dir) && close(self.dip, o.dip),
                _ => false,
            }
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

/// Dip direction from what was typed. Left-hand rule, as the Owens window reads it (index finger along the strike,
/// thumb towards the dip direction): dip direction = strike + 90, so a strike of 270 dips north and 90 dips south;
/// right-hand rule: strike - 90.
pub(super) fn to_dip_dir(conv: u8, typed: f64) -> f64 {
    match conv {
        0 => (typed + 90.0).rem_euclid(360.0),
        1 => (typed - 90.0).rem_euclid(360.0),
        _ => typed.rem_euclid(360.0),
    }
}

/// What is shown in the field for a stored dip direction (the inverse of `to_dip_dir`).
pub(super) fn from_dip_dir(conv: u8, dip_dir: f64) -> f64 {
    match conv {
        0 => (dip_dir - 90.0).rem_euclid(360.0),
        1 => (dip_dir + 90.0).rem_euclid(360.0),
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
    let mut v = json!({ "id": q.id.to_string(), "on": q.on, "dip_direction": q.dip_dir, "dip": q.dip, "comment": q.comment, "color": q.color });
    if let Some(a) = &q.ab {
        v["alpha"] = json!(a.alpha);
        v["beta"] = json!(a.beta);
        v["hole_dip_direction"] = json!(a.hole_dip_dir);
        v["hole_plunge"] = json!(a.plunge);
        if let Some(n) = &a.src {
            v["dashboard_vein"] = json!(n);
        }
    }
    v
}

fn parse_planes(list: Option<&Value>, spec: Option<&str>) -> Vec<StPlane> {
    let Some(list) = list.and_then(|p| p.as_array()) else { return vec![] };
    list.iter()
        .enumerate()
        .filter_map(|(n, p)| {
            let num = |k: &str| p.get(k).and_then(|x| x.as_f64());
            let col = p.get("color").and_then(|c| c.as_array()).filter(|c| c.len() == 3).map(|c| [0, 1, 2].map(|i| c[i].as_u64().unwrap_or(128).min(255) as u8)).unwrap_or(PALETTE[n % PALETTE.len()]);
            let id = p.get("id").and_then(|x| x.as_str()).and_then(|x| x.parse::<u64>().ok()).unwrap_or_else(fresh_id);
            let ab = match (num("alpha"), num("beta"), num("hole_dip_direction"), num("hole_plunge")) {
                (Some(alpha), Some(beta), Some(hole_dip_dir), Some(plunge)) => Some(AlphaBeta { alpha: alpha.clamp(0.0, 90.0), beta: beta.rem_euclid(360.0), hole_dip_dir: hole_dip_dir.rem_euclid(360.0), plunge: plunge.clamp(0.0, 90.0), src: p.get("dashboard_vein").and_then(|x| x.as_str()).map(|x| x.to_string()) }),
                _ => None,
            };
            // a vein plane without its own dip direction / dip in the file is still worked out from the angles
            let (dd, dp) = match (num("dip_direction"), num("dip")) {
                (Some(a), Some(b)) => (a.rem_euclid(360.0), b.clamp(0.0, 90.0)),
                _ if ab.is_some() => (0.0, 0.0),
                _ => return None,
            };
            let mut q = StPlane { id, on: p.get("on").and_then(|x| x.as_bool()).unwrap_or(true), dip_dir: dd, dip: dp, comment: p.get("comment").and_then(|c| c.as_str()).unwrap_or("").to_string(), color: col, spec: spec.map(|s| s.to_string()), ab };
            q.recompute();
            Some(q)
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
            let mut q = StPlane { id: 0, on: p.get("on").and_then(|x| x.as_bool()).unwrap_or(true), dip_dir: num("dip_direction")?.rem_euclid(360.0), dip: num("dip")?.clamp(0.0, 90.0), comment: p.get("comment").and_then(|c| c.as_str()).unwrap_or("").to_string(), color: col, spec, ab: None };
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
            .map(|q| {
                let comment = q.comment.trim().to_string();
                let name = if !comment.is_empty() {
                    comment.clone()
                } else if let Some(a) = &q.ab {
                    format!("vein plane: alpha {:.0}\u{b0}, beta {:.0}\u{b0} (dip direction {:.0}\u{b0}, dip {:.0}\u{b0})", a.alpha, a.beta, q.dip_dir, q.dip)
                } else {
                    format!("plane: dip direction {:.0}\u{b0}, dip {:.0}\u{b0}", q.dip_dir, q.dip)
                };
                PlaneDraw { dip_dir: q.dip_dir, dip: q.dip, color: Color32::from_rgb(q.color[0], q.color[1], q.color[2]), name, show_label: self.st_plane_comments && !comment.is_empty() }
            })
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
            self.st_dash_msg.clear();
            self.st_dash = cur.as_ref().and_then(|p| crate::hyper::locate(p)).and_then(|d| crate::veins::Dash::load(&d)).filter(|d| !d.is_empty());
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
        let mut changed = missing_in_file || (!had && !specific.is_empty());
        globals.extend(specific);
        // vein planes saved more than once with the same numbers (from adding the same one again) are kept once
        let before = globals.len();
        let mut kept: Vec<StPlane> = Vec::with_capacity(before);
        for q in globals {
            if q.ab.is_some() && kept.iter().any(|k| k.same_as(&q)) {
                continue;
            }
            kept.push(q);
        }
        changed |= kept.len() != before;
        globals = kept;
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

    /// The survey of the open specimen's hole for a new vein plane: from the dashboard code, else the position saved for
    /// the specimen (GIS viewer), else the values of the Drill Core to Geo window. Says where it came from.
    fn hole_survey(&self) -> ((f64, f64), &'static str) {
        let id = self.open_spec_id().unwrap_or_default();
        if let Some(sv) = self.st_dash.as_ref().and_then(|d| d.specimen(&id).survey) {
            return (sv, "the AMS dashboard code");
        }
        if let Some(h) = self.doc.as_ref().filter(|d| !d.synthetic).and_then(|d| super::gis_loc::read_for(&d.path)).and_then(|l| l.hole) {
            return ((h.dip_dir, h.plunge), "the position saved for this specimen");
        }
        ((self.core.setup.dip_dir, self.core.setup.plunge), "the Drill Core to Geo window")
    }

    /// Adds a plane unless the same one is already there (same scope, same numbers); says so in the note under the buttons.
    pub(super) fn push_plane(&mut self, q: StPlane) -> bool {
        if self.st_planes.iter().any(|p| p.same_as(&q)) {
            self.st_dash_msg = "That plane is already there: change the numbers of the one in the list, or remove it with \u{2715}, before adding another.".into();
            return false;
        }
        self.st_dash_msg.clear();
        self.st_planes.push(q);
        true
    }

    /// Adds the veins the dashboard code lists for the open specimen (alpha / beta from `VEIN_PLANES`, the survey from
    /// `BOREHOLE_CONFIGS`), as planes of this specimen; veins that are already there are left as they are.
    pub(super) fn add_dashboard_veins(&mut self) {
        let Some(id) = self.open_spec_id() else { return };
        // read the code again, so a vein added to it since the file was opened is found
        let fresh = self.doc.as_ref().filter(|d| !d.synthetic).and_then(|d| crate::hyper::locate(&d.path)).and_then(|d| crate::veins::Dash::load(&d));
        if fresh.is_some() {
            self.st_dash = fresh;
        }
        let Some(dash) = self.st_dash.clone() else {
            self.st_dash_msg = "The AMS dashboard code was not found above the data.".into();
            return;
        };
        let sp = dash.specimen(&id);
        if sp.veins.is_empty() {
            self.st_dash_msg = format!("The dashboard code lists no veins for {id}.");
            return;
        }
        let ((dd, pl), _) = self.hole_survey();
        let (dd, pl) = sp.survey.unwrap_or((dd, pl));
        let (mut added, mut there) = (0, 0);
        for v in &sp.veins {
            if self.st_planes.iter().any(|q| q.applies_to(Some(&id)) && q.spec.is_some() && q.ab.as_ref().map_or(false, |a| a.src.as_deref() == Some(v.name.as_str()))) {
                there += 1;
                continue;
            }
            let ab = AlphaBeta { alpha: v.alpha, beta: v.beta, hole_dip_dir: dd, plunge: pl, src: Some(v.name.clone()) };
            let n = self.st_planes.len();
            let q = StPlane::from_alpha_beta(n, Some(id.clone()), ab, &v.name);
            if self.st_planes.iter().any(|p| p.same_as(&q)) {
                there += 1;
                continue;
            }
            self.st_planes.push(q);
            added += 1;
        }
        self.st_dash_msg = format!("Added {added} vein plane{} for {id}{}.", if added == 1 { "" } else { "s" }, if there > 0 { format!(" ({there} already there)") } else { String::new() });
    }

    /// The "Planes" part of the stereonet's options.
    pub(super) fn planes_ui(&mut self, ui: &mut Ui) {
        let before = self.st_planes.clone();
        let state_before = (self.st_planes_on, self.st_plane_comments, self.st_plane_conv, self.st_plane_new_spec);
        let open_id = self.open_spec_id();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.st_planes_on, RichText::new("Planes").strong());
            info_icon(ui, "Planes you measured in the field (a vein, a fault, bedding), drawn as great circles, not poles. They are shown only while the net is in geographic coordinates (G), below the data. The comment is written small outside the net at one end of the circle (it takes a zoom in to read it); hold Alt over a plane to see its name at any zoom. The planes are kept with the data, in the dependency file of the specimen's _geo folder.");
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
                    info_icon(ui, "Left-hand rule (the same as in the Owens window: index finger along the strike, thumb towards the dip direction): dip direction = strike + 90 degrees, so a strike of 270 dips north and a strike of 90 dips south. Right-hand rule: dip direction = strike - 90 degrees. The letter behind the dip is the side the entry is read as dipping towards (E or W; N or S for a plane that strikes exactly east-west), so the reading can be checked.");
                });
                let mut remove: Option<usize> = None;
                let conv = self.st_plane_conv.min(2);
                let n_planes = self.st_planes.len();
                let open = open_id.as_deref();
                for (i, q) in self.st_planes.iter_mut().enumerate().filter(|(_, q)| q.applies_to(open)) {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut q.on, "").on_hover_text("Show this plane");
                        // first in the row, so it is never pushed out of sight by a narrow panel
                        if ui.small_button("\u{2715}").on_hover_text("Remove this plane").clicked() {
                            remove = Some(i);
                        }
                        if q.ab.is_some() {
                            // a vein plane: the angles are typed, the strike / dip follow from them and the hole's survey
                            let mut ch = false;
                            if let Some(ab) = q.ab.as_mut() {
                                ui.label("Alpha");
                                ch |= ui.add(egui::DragValue::new(&mut ab.alpha).range(0.0..=90.0).speed(0.5).max_decimals(1).suffix(" \u{b0}")).changed();
                                ui.label("Beta");
                                ch |= ui.add(egui::DragValue::new(&mut ab.beta).range(0.0..=360.0).speed(0.5).max_decimals(1).suffix(" \u{b0}")).changed();
                            }
                            if ch {
                                q.recompute();
                            }
                            ui.label(RichText::new(format!("\u{2192} {} {:.0}\u{b0} / {:.0}\u{b0}", if conv == 2 { "dip dir." } else { "strike" }, from_dip_dir(conv, q.dip_dir), q.dip)).weak());
                        } else {
                        let mut a = from_dip_dir(conv, q.dip_dir);
                        ui.label(if conv == 2 { "Dip dir." } else { "Strike" });
                        if ui.add(egui::DragValue::new(&mut a).range(0.0..=360.0).speed(0.5).max_decimals(1).suffix(" \u{b0}")).changed() {
                            q.dip_dir = to_dip_dir(conv, a);
                        }
                        ui.label("Dip");
                        ui.add(egui::DragValue::new(&mut q.dip).range(0.0..=90.0).speed(0.5).max_decimals(1).suffix(" \u{b0}"));
                        }
                        let side = side_letter(q.dip_dir);
                        ui.label(RichText::new(side).strong().size(15.0)).on_hover_text(format!("Read as dipping towards {side}: dip direction {:.1}\u{b0} ({}).", q.dip_dir, CONV_NAMES[conv as usize]));
                        let mut c = Color32::from_rgb(q.color[0], q.color[1], q.color[2]);
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            q.color = [c.r(), c.g(), c.b()];
                        }
                    });
                    if q.ab.is_some() {
                        let mut ch = false;
                        if let Some(ab) = q.ab.as_mut() {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Hole: dip direction").weak().small());
                                ch |= ui.add(egui::DragValue::new(&mut ab.hole_dip_dir).range(0.0..=360.0).speed(0.5).max_decimals(1).suffix(" \u{b0}")).changed();
                                ui.label(RichText::new("plunge").weak().small());
                                ch |= ui.add(egui::DragValue::new(&mut ab.plunge).range(0.0..=90.0).speed(0.5).max_decimals(1).suffix(" \u{b0}")).changed();
                                info_icon(ui, "The survey of the hole the core came from: the azimuth it goes down towards and its plunge below the horizontal. The vein plane is worked out from alpha, beta and these two numbers only, the same way as the AMS dashboard does it, so the turn of the cubes (BOH rotation) plays no part.");
                                if let Some(n) = ab.src.as_ref() {
                                    ui.label(RichText::new(format!("from the dashboard: {n}")).weak().small());
                                }
                            });
                        }
                        if ch {
                            q.recompute();
                        }
                    }
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
                    self.push_plane(StPlane::new(n_planes, if specific { open_id.clone() } else { None }));
                }
                // a vein plane from the angles measured on the core
                ui.horizontal(|ui| {
                    let label = if specific { "Add a vein plane (alpha / beta) for this specimen only" } else { "Add a vein plane (alpha / beta)" };
                    if ui.add_enabled(open_id.is_some(), egui::Button::new(label)).on_hover_text("A file has to be open: the plane is stored next to its data").clicked() {
                        let (sv, _) = self.hole_survey();
                        let ab = AlphaBeta { alpha: 45.0, beta: 0.0, hole_dip_dir: sv.0, plunge: sv.1, src: None };
                        self.push_plane(StPlane::from_alpha_beta(n_planes, if specific { open_id.clone() } else { None }, ab, ""));
                    }
                    info_icon(ui, "For a vein you measured on the whole core before it was cut: alpha is the angle between the vein and the core axis (0 to 90 degrees), beta is the angle clockwise looking downhole from the line of the BOH mark to the downhole end of the vein's trace on the core (0 to 360 degrees). The plane is worked out with the survey of the hole and drawn like any other plane. The survey is taken from the AMS dashboard code, else from the position saved for the specimen, else from the Drill Core to Geo window; it is shown under the plane and can be changed for each plane.");
                });
                if let Some(id) = open_id.clone() {
                    let sp = self.st_dash.as_ref().map(|d| d.specimen(&id)).unwrap_or_default();
                    if !sp.veins.is_empty() {
                        ui.horizontal(|ui| {
                            if ui.button(format!("Add the veins of the dashboard (n={})", sp.veins.len())).clicked() {
                                self.add_dashboard_veins();
                            }
                            info_icon(ui, "The AMS dashboard code lists the veins you measured for this specimen (VEIN_PLANES: name, alpha, beta) and the survey of its hole (BOREHOLE_CONFIGS). They are the reoriented, geographic veins, the ones the dashboard draws in its last two rows (geographic, and geographic with K3); nothing of the bird's-eye rows is used. This adds each one as a plane of this specimen, named after the vein; a vein that is already there is left as it is, so nothing you changed is overwritten. The code is read from the same place relative to your data on every computer.");
                        });
                    }
                    if !self.st_dash_msg.is_empty() {
                        ui.label(RichText::new(self.st_dash_msg.clone()).weak().small());
                    }
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
    fn the_left_hand_rule_puts_the_dip_direction_90_degrees_after_the_strike() {
        // strike 0 (north): left-hand rule dips to the east, right-hand rule to the west
        assert!((to_dip_dir(0, 0.0) - 90.0).abs() < 1e-9 && side_letter(to_dip_dir(0, 0.0)) == "E");
        assert!((to_dip_dir(1, 0.0) - 270.0).abs() < 1e-9 && side_letter(to_dip_dir(1, 0.0)) == "W");
        assert_eq!(to_dip_dir(2, 135.0), 135.0);
        // strike 45: left-hand rule dips towards 135 (south-east, E), right-hand towards 315 (north-west, W)
        assert_eq!(side_letter(to_dip_dir(0, 45.0)), "E");
        assert_eq!(side_letter(to_dip_dir(1, 45.0)), "W");
        // a plane striking east-west: strike 270 dips towards north (0), strike 90 towards south (180)
        assert!(to_dip_dir(0, 270.0).abs() < 1e-9 && side_letter(to_dip_dir(0, 270.0)) == "N");
        assert!((to_dip_dir(0, 90.0) - 180.0).abs() < 1e-9 && side_letter(to_dip_dir(0, 90.0)) == "S");
        // the entry converts back to what was typed
        for conv in 0..3u8 {
            for a in [0.0, 17.5, 90.0, 200.0, 359.0] {
                assert!((from_dip_dir(conv, to_dip_dir(conv, a)) - a).abs() < 1e-9, "{conv} {a}");
            }
        }
    }

    #[test]
    fn a_vein_plane_keeps_its_angles_in_the_file_and_is_worked_out_again_on_reading() {
        let ab = AlphaBeta { alpha: 35.0, beta: 315.0, hole_dip_dir: 305.0, plunge: 69.0, src: Some("vein 2 (main)".into()) };
        let q = StPlane::from_alpha_beta(0, Some("ER26PB012".into()), ab, "vein 2 (main)");
        assert!((q.dip_dir - 87.18405718301699).abs() < 1e-6 && (q.dip - 70.85845480209002).abs() < 1e-6);
        let back = parse_planes(Some(&Value::Array(vec![plane_json(&q)])), Some("ER26PB012"));
        assert_eq!(back, vec![q.clone()]);
        // a file with the angles but no dip direction / dip (written by hand) still gives the plane
        let mut v = plane_json(&q);
        v.as_object_mut().unwrap().remove("dip_direction");
        v.as_object_mut().unwrap().remove("dip");
        let again = parse_planes(Some(&Value::Array(vec![v])), Some("ER26PB012"));
        assert!(again.len() == 1 && (again[0].dip - q.dip).abs() < 1e-9);
        // a typed plane has no angles
        assert!(parse_planes(Some(&Value::Array(vec![plane_json(&StPlane::new(0, None))])), None)[0].ab.is_none());
    }

    #[test]
    fn the_same_numbers_make_the_same_plane_whatever_the_colour_and_comment() {
        let ab = || AlphaBeta { alpha: 45.0, beta: 0.0, hole_dip_dir: 305.0, plunge: 69.0, src: None };
        let a = StPlane::from_alpha_beta(0, Some("ER26PB012".into()), ab(), "");
        let mut b = StPlane::from_alpha_beta(3, Some("er26pb012".into()), ab(), "a comment");
        assert!(a.same_as(&b));
        b.ab.as_mut().unwrap().beta = 10.0;
        assert!(!a.same_as(&b));
        // another scope is another plane
        assert!(!a.same_as(&StPlane::from_alpha_beta(0, None, ab(), "")));
        // a typed plane and a vein plane never count as the same
        assert!(!StPlane::new(0, None).same_as(&StPlane::from_alpha_beta(0, None, ab(), "")));
        assert!(StPlane::new(0, None).same_as(&StPlane::new(1, None)));
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
