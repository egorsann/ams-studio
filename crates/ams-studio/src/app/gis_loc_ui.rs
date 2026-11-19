//! The "Specimen locations" window of the GIS viewer (input from a file) and the symbols that come out of it.
//!
//! One place per specimen (the main file of each specimen folder of the file tree, or the open file), kept in the
//! specimen's dependency file (see `gis_loc.rs`). This window is for a datasheet (csv / xlsx): the specimens are
//! listed on the left with a link symbol (joined chain = linked, apart = not, coloured by how good the match is),
//! then the three columns of the datasheet row they are linked to (name and the two coordinates). The best match by
//! name is suggested (and linked when it is the same name); another row can be chosen. Typing the coordinates by hand
//! is the other window (`gis_manual_ui.rs`).

use super::gis_loc::{self, Location};
use super::gis_ui::Placed;
use super::{core_pane, info_icon, tools_ui, App, W_GIS_LOC};
use ams_core::coords::CoordSys;
#[cfg(test)]
use ams_core::coords::Orientation;
use ams_core::depjson;
use ams_core::gis;
use ams_core::tensor::{dec_inc, eig_sym, Mat3};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

pub struct LocRow {
    pub path: PathBuf,
    pub id: String,
    pub on: bool,
    pub saved: Option<Location>,
    /// Datasheet rows (index into `Gis::locs`, score), best first.
    pub cands: Vec<(usize, u8)>,
    /// The datasheet row this specimen is linked to.
    pub link: Option<usize>,
    /// What the user chose in the drop-down (`Some(None)` = no link); `None` while it is left to the suggestion.
    pub manual: Option<Option<usize>>,
}

impl LocRow {
    /// How good the link is: the score of the linked datasheet row against the specimen id.
    pub fn link_score(&self) -> Option<u8> {
        let k = self.link?;
        Some(self.cands.iter().find(|c| c.0 == k).map_or(0, |c| c.1))
    }
}

#[derive(Default)]
pub struct LocState {
    pub win: bool,
    pub stale: bool,
    pub rows: Vec<LocRow>,
    pub msg: String,
    /// Bumped whenever a location was saved or removed (the symbols are recomputed).
    pub ver: u64,
}

/// The colours of the link symbol: same name, close name, a weak match the user chose, no link.
pub const LINK_SAME: Color32 = Color32::from_rgb(46, 163, 94);
pub const LINK_CLOSE: Color32 = Color32::from_rgb(52, 130, 230);
pub const LINK_WEAK: Color32 = Color32::from_rgb(226, 140, 30);

/// Two chain links: joined when linked, apart when not.
pub fn chain(ui: &mut Ui, linked: bool, col: Color32) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(42.0, 26.0), Sense::hover());
    let col = if linked { col } else { ui.visuals().weak_text_color().gamma_multiply(0.7) };
    let (w, h) = (18.0, 11.0);
    let off = if linked { 6.0 } else { 11.5 };
    for dx in [-off, off] {
        ui.painter().rect_stroke(Rect::from_center_size(Pos2::new(r.center().x + dx, r.center().y), Vec2::new(w, h)), 5.5, Stroke::new(2.4, col), StrokeKind::Inside);
    }
}

/// A pair of numbers pasted into one box (`47.1 10.5`, `47.1; 10.5`, `47,1, 10,5`): the two numbers.
pub fn split_pair(t: &str) -> Option<(String, String)> {
    let t = t.trim();
    for sep in [';', '\t'] {
        if let Some((a, b)) = t.split_once(sep) {
            if !b.contains(sep) && !a.trim().is_empty() && !b.trim().is_empty() {
                return Some((a.trim().to_string(), b.trim().to_string()));
            }
        }
    }
    if let Some((a, b)) = t.split_once(", ") {
        if !b.contains(", ") && !a.trim().is_empty() && !b.trim().is_empty() {
            return Some((a.trim().to_string(), b.trim().to_string()));
        }
    }
    let tok: Vec<&str> = t.split_whitespace().collect();
    (tok.len() == 2).then(|| (tok[0].to_string(), tok[1].to_string()))
}

/// The tensors of the records that count (not disregarded) from a derived list.
pub fn active_tensors(doc: &crate::doc::Doc, der: &[crate::doc::Derived]) -> Vec<Mat3> {
    (0..doc.specs.len().min(der.len())).filter(|&i| doc.active[i]).map(|i| der[i].tensor).collect()
}

/// The fabric of the mean tensor of some records: the K1 and K3 directions of the mean as the foliation and the lineation.
/// This is the mean point Group Statistics calculates (`ams_core::stats::group_stats`, as the original does) and draws
/// at the centre of the confidence ellipses; a test holds the two equal.
pub fn mean_fabric(t: &[Mat3]) -> Option<gis::Fabric> {
    if t.is_empty() {
        return None;
    }
    let mut m = [[0.0f64; 3]; 3];
    for x in t {
        for r in 0..3 {
            for c in 0..3 {
                m[r][c] += x[r][c];
            }
        }
    }
    // sum first, divide after: the same arithmetic as the mean tensor of Group Statistics, so the same bits
    for row in m.iter_mut() {
        for v in row.iter_mut() {
            *v /= t.len() as f64;
        }
    }
    let (_, v) = eig_sym(&m);
    Some(gis::fabric_from_axes(dec_inc(v[0]), dec_inc(v[2])))
}

/// The fabric of a document in geographic coordinates (what the map computes for every specimen, kept as a function
/// of its own so a test can compare it with the specimen-coordinate one).
#[cfg(test)]
pub fn geographic_fabric(doc: &crate::doc::Doc, out_phase: bool, orient: &Orientation) -> Option<gis::Fabric> {
    mean_fabric(&active_tensors(doc, &doc.derive(out_phase, CoordSys::Geographic, orient)))
}

impl App {
    pub(super) fn gis_open_loc(&mut self) {
        self.gis.loc.win = true;
        self.gis.loc.stale = true;
        self.win_serial[W_GIS_LOC] += 1;
    }

    /// The specimens of the file tree (or the open file) as rows, with the saved locations and the suggested links.
    fn loc_rebuild(&mut self) {
        let files = self.cv_files();
        let keys: Vec<String> = self.gis.locs.iter().map(|l| l.key.clone()).collect();
        let old: HashMap<PathBuf, (bool, Option<Option<usize>>)> = self.gis.loc.rows.drain(..).map(|r| (r.path, (r.on, r.manual))).collect();
        self.gis.loc.rows = files
            .into_iter()
            .map(|path| {
                let id = core_pane::spec_id(&path);
                let saved = gis_loc::read_for(&path);
                let cands = gis::rank_names(&id, &keys);
                let top_ok = cands.first().is_some_and(|c| c.1 >= 4);
                let saved_link = saved.as_ref().filter(|s| s.source == "datasheet").and_then(|s| keys.iter().position(|k| *k == s.datasheet_name));
                let (on, manual) = old.get(&path).cloned().unwrap_or((true, None));
                // a specimen that already has a typed location is not linked to the datasheet by itself
                let link = match manual {
                    Some(m) => m,
                    None => saved_link.or_else(|| if saved.is_none() && top_ok { cands.first().map(|c| c.0) } else { None }),
                };
                LocRow { path, id, on, saved, cands, link, manual }
            })
            .collect();
    }

    /// Lists the specimens again when the file tree changed or something asked for it.
    pub(super) fn loc_sync_rows(&mut self) {
        if self.cv_files() != self.gis.loc.rows.iter().map(|r| r.path.clone()).collect::<Vec<_>>() {
            self.gis.loc.stale = true;
        }
        if self.gis.loc.stale {
            self.gis.loc.stale = false;
            self.loc_rebuild();
        }
    }

    /// The UTM zone to start from: the one of the places already saved, else of the map view.
    pub(super) fn zone_hint(&self) -> u8 {
        let lons: Vec<f64> = self.gis.loc.rows.iter().filter_map(|r| r.saved.as_ref().map(|s| s.lon)).collect();
        if !lons.is_empty() {
            return gis::utm_zone(lons.iter().sum::<f64>() / lons.len() as f64);
        }
        if self.gis.view.mpp < 39_000.0 {
            return gis::utm_zone(gis::lonlat_from_merc(self.gis.view.cx, self.gis.view.cy).0);
        }
        44
    }

    pub(super) fn loc_store(&mut self, row: usize, loc: Option<Location>) {
        let Some(path) = self.gis.loc.rows.get(row).map(|r| r.path.clone()) else { return };
        match gis_loc::write_for(&path, loc.as_ref()) {
            Ok(()) => {
                self.gis.loc.rows[row].saved = loc;
                self.gis.loc.ver += 1;
                self.gis.fit_pending = true;
            }
            Err(e) => self.gis.loc.msg = format!("Could not save the location: {e}"),
        }
    }

    /// Writes the place of every ticked, linked specimen (from its datasheet row) into its dependency file.
    fn loc_save_links(&mut self) {
        let crs = self.gis.mapping.crs();
        let file = self.gis.table.as_ref().map(|t| t.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).unwrap_or_default();
        self.gis.loc.msg.clear();
        match crs {
            Ok(crs) => {
                for i in 0..self.gis.loc.rows.len() {
                    let (on, link) = (self.gis.loc.rows[i].on, self.gis.loc.rows[i].link);
                    let Some(l) = link.filter(|_| on).and_then(|k| self.gis.locs.get(k)).cloned() else { continue };
                    match Location::new(l.x, l.y, crs, "datasheet") {
                        Some(mut loc) => {
                            loc.datasheet_name = l.key.clone();
                            loc.datasheet_file = file.clone();
                            // the depth of the datasheet row when it has one, else what was saved; the hole direction is kept
                            let prev = self.gis.loc.rows[i].saved.as_ref();
                            loc.depth = l.depth.or(prev.and_then(|p| p.depth));
                            loc.elevation = l.elev.or(prev.and_then(|p| p.elevation));
                            loc.hole = prev.and_then(|p| p.hole);
                            if self.gis.loc.rows[i].saved.as_ref() != Some(&loc) {
                                self.loc_store(i, Some(loc));
                            }
                        }
                        None => self.gis.loc.msg = format!("The coordinates of {} are not a point on the earth with this system.", l.key),
                    }
                }
            }
            Err(e) => self.gis.loc.msg = e,
        }
        self.gis.fit_pending = true;
    }

    pub(super) fn gis_loc_window(&mut self, ctx: &egui::Context) {
        if !self.gis.loc.win {
            return;
        }
        self.loc_sync_rows();
        let mut st = std::mem::take(&mut self.gis.loc);
        let locs = self.gis.locs.clone();
        let sheet_name = self.gis.table.as_ref().map(|t| t.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
        let sheet_crs = self.gis.mapping.crs();
        let degrees = sheet_crs.as_ref().is_ok_and(|c| *c == ams_core::gis::Crs::LonLat);
        let (mut open, mut load_sheet, mut edit_cols, mut save_links, mut close) = (true, false, false, false, false);
        let mut remove: Option<usize> = None;
        let mut type_for: Option<String> = None;
        let scr = ctx.content_rect();
        let size = Vec2::new((scr.width() - 60.0).clamp(820.0, 1010.0), (scr.height() - 60.0).clamp(560.0, 840.0));
        self.base_window(ctx, "Specimen locations", W_GIS_LOC, size).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.12);
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
            ui.spacing_mut().button_padding = Vec2::new(14.0, 6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Which datasheet row belongs to which specimen?").strong().size(19.0));
                info_icon(ui, "Every specimen of the file tree gets one place, saved in its own dependency file (the _geo folder of the specimen) so it stays with the data; the .ams files are not changed. The specimens are linked to the rows of the datasheet by name: the best match is suggested, and you can change any link in the drop-down. Link and save writes the places; the datasheet is only read. For a specimen that is not in the datasheet use Type ... (or Input manually in the viewer).");
            });
            ui.label(RichText::new("Datasheet with the coordinates (csv or xlsx):").small());
            ui.horizontal(|ui| {
                load_sheet = ui.add(egui::Button::new(if sheet_name.is_some() { "Load another datasheet ..." } else { "Load datasheet ..." }).min_size(Vec2::new(190.0, 34.0))).clicked();
                if sheet_name.is_some() {
                    edit_cols = ui.add(egui::Button::new("Columns ...").min_size(Vec2::new(120.0, 34.0))).on_hover_text("Which column is the name and the coordinates, and the coordinate system").clicked();
                }
                if let Some(n) = &sheet_name {
                    ui.label(RichText::new(format!("{n}   (n={} rows)", locs.len())).weak());
                }
            });
            ui.separator();
            let n_saved = st.rows.iter().filter(|r| r.saved.is_some()).count();
            let n_linked = st.rows.iter().filter(|r| r.link.is_some()).count();
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Specimens (n={})", st.rows.len())).strong());
                ui.label(RichText::new(format!("linked (n={n_linked})   place saved (n={n_saved})")).weak());
            });
            if st.rows.is_empty() {
                ui.label(RichText::new("No specimens: open a folder of specimens (File > Open Folder of Specimens) or a file.").weak());
            }
            let table_h = (ui.available_height() - 150.0).max(160.0);
            egui::ScrollArea::both().max_height(table_h).auto_shrink([false, true]).id_salt("loc-rows").show(ui, |ui| {
                ui.spacing_mut().interact_size.y = 30.0;
                egui::Grid::new("loc-grid").num_columns(8).spacing([22.0, 11.0]).striped(true).show(ui, |ui| {
                    for h in ["", "Specimen", "", "Datasheet name", if degrees { "Longitude" } else { "x / easting" }, if degrees { "Latitude" } else { "y / northing" }, "Place saved", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, r) in st.rows.iter_mut().enumerate() {
                        ui.checkbox(&mut r.on, "").on_hover_text("Untick to leave this specimen out of Link and save");
                        ui.label(RichText::new(&r.id).strong());
                        let col = match r.link_score() {
                            Some(6) => LINK_SAME,
                            Some(4) | Some(5) => LINK_CLOSE,
                            _ => LINK_WEAK,
                        };
                        chain(ui, r.link.is_some(), col);
                        if locs.is_empty() {
                            ui.label(RichText::new("no datasheet loaded").weak().small());
                            ui.label("");
                            ui.label("");
                        } else {
                            let cur = r.link.and_then(|k| locs.get(k));
                            let text = cur.map_or("(not linked)".to_string(), |l| l.key.clone());
                            let mut pick: Option<Option<usize>> = None;
                            egui::ComboBox::from_id_salt(("loc-link", i)).selected_text(text).width(230.0).show_ui(ui, |ui| {
                                if ui.selectable_label(r.link.is_none(), "(not linked)").clicked() {
                                    pick = Some(None);
                                }
                                for (rank, &(k, score)) in r.cands.iter().enumerate() {
                                    let l = &locs[k];
                                    let tag = if rank == 0 && score >= 4 { "     Top suggestion".to_string() } else if rank < 4 && score >= 2 { format!("     Next closest ({})", rank + 1) } else { String::new() };
                                    if ui.selectable_label(r.link == Some(k), format!("{}      {}    {}{tag}", l.key, l.xs, l.ys)).clicked() {
                                        pick = Some(Some(k));
                                    }
                                }
                            });
                            if let Some(p) = pick {
                                r.manual = Some(p);
                                r.link = p;
                            }
                            ui.label(RichText::new(cur.map_or(String::new(), |l| l.xs.clone())).monospace());
                            ui.label(RichText::new(cur.map_or(String::new(), |l| l.ys.clone())).monospace());
                        }
                        match &r.saved {
                            Some(s) => ui.label(RichText::new(s.summary()).small()).on_hover_text(if s.source == "datasheet" { format!("From the datasheet row {} of {}", s.datasheet_name, s.datasheet_file) } else { "Typed".to_string() }),
                            None => ui.label(RichText::new("not set").small().weak()),
                        };
                        ui.horizontal(|ui| {
                            if ui.button("Type ...").on_hover_text("Type the coordinates of this specimen").clicked() {
                                type_for = Some(r.id.clone());
                            }
                            if r.saved.is_some() && ui.button("✕").on_hover_text("Remove the saved place of this specimen").clicked() {
                                remove = Some(i);
                            }
                        });
                        ui.end_row();
                    }
                });
            });
            // what the colours of the links mean
            ui.horizontal(|ui| {
                for (c, t) in [(LINK_SAME, "same name"), (LINK_CLOSE, "close name"), (LINK_WEAK, "weak match you chose")] {
                    chain(ui, true, c);
                    ui.label(RichText::new(t).small());
                    ui.add_space(10.0);
                }
                chain(ui, false, LINK_SAME);
                ui.label(RichText::new("not linked").small());
            });
            if !st.msg.is_empty() {
                ui.label(RichText::new(&st.msg).small().color(Color32::from_rgb(200, 80, 40)));
            }
            if let Err(e) = &sheet_crs {
                if !locs.is_empty() {
                    ui.label(RichText::new(e).small().color(Color32::from_rgb(200, 80, 40)));
                }
            }
            ui.separator();
            let n_links = st.rows.iter().filter(|r| r.on && r.link.is_some()).count();
            ui.label(RichText::new("Save the links as the places of the specimens?").strong());
            ui.horizontal(|ui| {
                save_links = ui.add_enabled(n_links > 0 && sheet_crs.is_ok(), egui::Button::new(format!("Link and save (n={n_links})")).min_size(Vec2::new(200.0, 38.0))).on_hover_text("Writes the place of every ticked, linked specimen into its dependency file; the datasheet is only read").clicked();
                close = ui.add(egui::Button::new("Close").min_size(Vec2::new(120.0, 38.0))).clicked();
            });
        });
        self.gis.loc = st;
        if let Some(i) = remove {
            self.loc_store(i, None);
        }
        if save_links || (self.test_save_links && !self.gis.loc.rows.is_empty()) {
            self.test_save_links = false;
            self.loc_save_links();
        }
        if let Some(id) = type_for {
            self.gis_open_manual(Some(id));
        }
        if load_sheet {
            if let Some(p) = super::gis_ui::pick_file("Datasheet with the coordinates", "Spreadsheet or csv", &["csv", "xlsx", "xlsm", "xls", "ods", "txt", "tsv", "dat", "asc", "xyz", "pts"], "AMS_STUDIO_GIS_TABLE") {
                self.gis_load_table(p);
            }
        }
        if edit_cols {
            if let Some(t) = self.gis.table.clone() {
                self.gis.draft = Some((t, self.gis.mapping.clone()));
                self.gis.win_table = true;
                self.win_serial[super::W_GIS_TABLE] += 1;
            }
        }
        if !open || close {
            self.gis.loc.win = false;
        }
    }

    /// The symbols: one per specimen of the file tree (or the open file) that has a saved place. Recomputed when the
    /// files, the places or the settings change; checked at most twice a second.
    /// A specimen file read the way opening it does: its rotations that were made but not saved, then the reorientation
    /// of the `_geo` file next to it.
    fn doc_as_opened(&self, p: &std::path::Path) -> Option<crate::doc::Doc> {
        let mut d = crate::doc::Doc::open(p).ok()?;
        if d.raw.len() > 0 && !d.modified {
            if let Some(v) = self.pending.get(p) {
                for (i, g) in v.iter().enumerate() {
                    if let (Some(g), true) = (g, i < d.specs.len()) {
                        d.set_georot(i, Some(*g), false);
                    }
                }
            }
        }
        if !d.synthetic {
            let partner = super::export_ui::geo_path(p);
            if let Ok(bytes) = std::fs::read(&partner) {
                if let Ok(mut theirs) = ams_core::ams::parse(&bytes) {
                    crate::doc::load_dependency(&mut theirs, &partner);
                    d.apply_partner(&theirs);
                }
            }
        }
        Some(d)
    }

    pub(super) fn gis_refresh_placed(&mut self) {
        // checked at most twice a second, except right after a switch of phase: that one should show at once
        let phase_changed = self.gis.placed_phase != self.out_phase;
        let due = phase_changed || self.gis.placed_check.map_or(true, |t| t.elapsed().as_millis() > 500);
        if !due {
            return;
        }
        self.gis.placed_phase = self.out_phase;
        self.gis.placed_check = Some(std::time::Instant::now());
        let files = self.cv_files();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        // the map only ever shows geographic directions: the coordinate system chosen in the main window does not matter
        let geo_open: Vec<crate::doc::Derived> = self.doc.as_ref().map(|d| d.derive(self.out_phase, CoordSys::Geographic, &self.orient)).unwrap_or_default();
        (files.len(), self.out_phase, self.gis.loc.ver, self.pending.len(), self.stash.len()).hash(&mut h);
        format!("{:?}", self.orient).hash(&mut h);
        for f in &files {
            f.hash(&mut h);
            for p in [f.clone(), depjson::for_source(f)] {
                std::fs::metadata(&p).and_then(|m| m.modified()).ok().hash(&mut h);
            }
        }
        if let Some(doc) = &self.doc {
            doc.active.hash(&mut h);
            for d in &geo_open {
                d.km.to_bits().hash(&mut h);
                d.axes.dec_inc[0].0.to_bits().hash(&mut h);
                d.axes.dec_inc[2].1.to_bits().hash(&mut h);
            }
        }
        let sig = h.finish();
        if std::env::var_os("AMS_STUDIO_GIS_DEMO_COLLARS").is_some() {
            // headless test hook: made-up drill core around 47.2 N 12.5 E - a collar with seven specimens and a hole direction,
            // a collar with three, one specimen alone with a hole direction, and two loose specimens close by
            if self.gis.placed.is_empty() && !files.is_empty() {
                let (cx, cy) = gis::merc_from_lonlat(12.5, 47.2);
                let mk = |name: &str, dx: f64, dy: f64, depth: Option<f64>, hole: Option<gis_loc::Hole>, k: usize, file: &PathBuf| {
                    let m = (cx + dx * 12.0, cy + dy * 12.0);
                    let (lon, lat) = gis::lonlat_from_merc(m.0, m.1);
                    let dip_dir = (k as f64 * 53.0 + 20.0) % 360.0;
                    let dip = [8.0, 20.0, 35.0, 52.0, 66.0, 78.0, 88.0][k % 7];
                    Placed { name: name.to_string(), path: file.clone(), n: 4, lon, lat, merc: m, fabric: gis::Fabric { strike: (dip_dir + 270.0) % 360.0, dip_dir, dip, trend: (dip_dir + 25.0 + k as f64 * 9.0) % 360.0, plunge: [3.0, 15.0, 30.0, 45.0, 60.0, 75.0, 86.0][(k + 2) % 7] }, depth, hole, elev: None }
                };
                let h1 = gis_loc::Hole::new(316.1, 67.0);
                let depths = [12.4, 47.0, 88.75, 131.0, 176.2, 240.0, 305.5];
                for (k, d) in depths.iter().enumerate() {
                    self.gis.placed.push(mk(&format!("DH1-{}", k + 1), 0.0, 0.0, Some(*d), h1, k, &files[k % files.len()]));
                }
                for (k, d) in [20.0, 64.0, 119.5].iter().enumerate() {
                    self.gis.placed.push(mk(&format!("DH2-{}", k + 1), 140.0, -30.0, Some(*d), gis_loc::Hole::new(95.0, 55.0), k + 2, &files[k % files.len()]));
                }
                self.gis.placed.push(mk("DH3-1", -120.0, 40.0, Some(33.0), gis_loc::Hole::new(200.0, 80.0), 4, &files[0]));
                self.gis.placed.push(mk("S1", 40.0, 70.0, None, None, 1, &files[0]));
                self.gis.placed.push(mk("S2", -40.0, -60.0, None, None, 5, &files[0]));
                self.gis.n_files = files.len();
                self.gis.fit_pending = true;
            }
            self.gis.placed_sig = sig;
            return;
        }
        if let Ok(spec) = std::env::var("AMS_STUDIO_GIS_DEMO_SYMBOLS") {
            // headless test hook: a grid of made-up symbols with dips and plunges from shallow to steep around 47.2 N 12.5 E
            if self.gis.placed.is_empty() && !files.is_empty() {
                let _ = spec;
                let dips = [3.0, 12.0, 25.0, 40.0, 55.0, 70.0, 82.0, 90.0];
                let plunges = [2.0, 10.0, 22.0, 38.0, 52.0, 68.0, 80.0, 88.0];
                let (cx, cy) = gis::merc_from_lonlat(12.5, 47.2);
                for (k, (dip, pl)) in dips.iter().zip(plunges).enumerate() {
                    let dip_dir = (k as f64 * 47.0 + 20.0) % 360.0;
                    let trend = (dip_dir + 30.0 + k as f64 * 11.0) % 360.0;
                    let (col, row) = ((k % 4) as f64, (k / 4) as f64);
                    let m = (cx + (col - 1.5) * 90.0, cy - (row - 0.5) * 90.0);
                    let (lon, lat) = gis::lonlat_from_merc(m.0, m.1);
                    self.gis.placed.push(Placed { name: format!("S{}", k + 1), path: files[k % files.len()].clone(), n: 4, lon, lat, merc: m, fabric: gis::Fabric { strike: (dip_dir + 270.0) % 360.0, dip_dir, dip: *dip, trend, plunge: pl }, depth: None, hole: None, elev: None });
                }
                self.gis.n_files = files.len();
                self.gis.fit_pending = true;
            }
            self.gis.placed_sig = sig;
            return;
        }
        if sig == self.gis.placed_sig {
            return;
        }
        self.gis.placed_sig = sig;
        self.gis.placed.clear();
        self.gis.missing = 0;
        self.gis.n_files = files.len();
        self.gis.placed_cache.retain(|k, _| files.contains(k));
        for f in &files {
            // the place and the opened file are kept until the file or its dependency file changes on disk
            let dep_path = depjson::for_source(f);
            let partner = super::export_ui::geo_path(f);
            let mt = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
            let (src, dep, par) = (mt(f), mt(&dep_path), mt(&partner));
            {
                let e = self.gis.placed_cache.entry(f.clone()).or_default();
                if src.is_none() || e.src != src || e.dep != dep || e.partner != par {
                    *e = super::gis_ui::FileCache { src, dep, partner: par, loc: gis_loc::read_for(f), doc: None };
                }
            }
            let Some(loc) = self.gis.placed_cache.get(f).and_then(|e| e.loc.clone()) else {
                self.gis.missing += 1;
                continue;
            };
            // the tensors of the specimen in geographic coordinates; a specimen that is not open is read the way opening it
            // would (with its reorientation), so its symbol does not change when it becomes the open one
            let is_open = matches!(&self.doc, Some(d) if d.path == *f && geo_open.len() == d.specs.len());
            if !is_open && !self.stash.contains_key(f) && self.gis.placed_cache.get(f).is_some_and(|e| e.doc.is_none()) {
                let d = self.doc_as_opened(f);
                if let Some(e) = self.gis.placed_cache.get_mut(f) {
                    e.doc = d;
                }
            }
            let tensors: Vec<Mat3> = match &self.doc {
                Some(d) if is_open => active_tensors(d, &geo_open),
                _ => match self.stash.get(f).or_else(|| self.gis.placed_cache.get(f).and_then(|e| e.doc.as_ref())) {
                    Some(d) => active_tensors(d, &d.derive(self.out_phase, CoordSys::Geographic, &self.orient)),
                    None => vec![],
                },
            };
            let Some(fabric) = mean_fabric(&tensors) else {
                self.gis.missing += 1;
                continue;
            };
            self.gis.placed.push(Placed { name: core_pane::spec_id(f), path: f.clone(), n: tensors.len(), lon: loc.lon, lat: loc.lat, merc: loc.merc(), fabric, depth: loc.depth, hole: loc.hole.or_else(|| reorientation_hole(f)), elev: loc.elevation });
        }
    }
}

/// The hole direction of a specimen that has a drill core reorientation (the Owens one of Drill Core to Geo) but no hole
/// direction typed with its place: a reoriented specimen is drill core, so specimens that share a collar get their box
/// (and the hole line) without anybody ticking "Drill core" for each of them.
fn reorientation_hole(f: &std::path::Path) -> Option<gis_loc::Hole> {
    let partner = super::export_ui::geo_path(f);
    depjson::read(&partner).into_iter().flatten().find_map(|s| gis_loc::Hole::new(s.dip_dir, s.plunge))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_uses_geographic_directions_whatever_the_main_window_shows() {
        // an oriented demo file: its geographic fabric differs from the specimen-coordinate one
        let doc = crate::doc::Doc::open(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/demo_12_oriented.ams"))).unwrap();
        let orient = Orientation::default();
        let geo = geographic_fabric(&doc, false, &orient).unwrap();
        // the same mean computed in specimen coordinates
        let in_specimen = mean_fabric(&active_tensors(&doc, &doc.derive(false, CoordSys::Specimen, &orient))).unwrap();
        let moved = (geo.strike - in_specimen.strike).abs().min(360.0 - (geo.strike - in_specimen.strike).abs()) + (geo.dip - in_specimen.dip).abs() + (geo.trend - in_specimen.trend).abs();
        assert!(moved > 1.0, "{geo:?} {in_specimen:?}");
        // the result does not depend on anything but the file, the phase and the orientation parameters
        assert_eq!(geographic_fabric(&doc, false, &orient), Some(geo));
    }

    #[test]
    fn the_symbol_comes_from_the_group_statistics_mean_point_the_centre_of_its_ellipses() {
        // the mean K1 and K3 that Group Statistics (as in Anisoft 6) prints and draws inside the confidence ellipses are
        // the ones the strike / dip and lineation symbol is made of
        let doc = crate::doc::Doc::open(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/demo_12_oriented.ams"))).unwrap();
        let der = doc.derive(false, CoordSys::Geographic, &Orientation::default());
        let tensors = active_tensors(&doc, &der);
        let kms: Vec<f64> = (0..doc.specs.len().min(der.len())).filter(|&i| doc.active[i]).map(|i| der[i].km).collect();
        let gs = ams_core::stats::group_stats(&tensors, &kms).unwrap();
        assert_eq!(mean_fabric(&tensors), Some(gis::fabric_from_axes(gs.dec_inc[0], gs.dec_inc[2])));
        // the symbol's lineation is the mean K1, its plane is perpendicular to the mean K3
        let f = mean_fabric(&tensors).unwrap();
        assert!((f.plunge - gs.dec_inc[0].1.abs()).abs() < 1e-6, "{f:?} {:?}", gs.dec_inc);
        assert!((f.dip - (90.0 - gs.dec_inc[2].1.abs())).abs() < 1e-6, "{f:?} {:?}", gs.dec_inc);
        // and that point is the centre of the Jelinek ellipses: the outline of each is symmetric about the mean axis
        for (axis, e) in gs.jelinek.iter().enumerate().filter_map(|(a, e)| e.map(|e| (a, e))) {
            let out = e.outline(&gs.vecs, 72);
            let mut mid = [0.0f64; 3];
            for v in &out[..72] {
                for c in 0..3 {
                    mid[c] += v[c] / 72.0;
                }
            }
            let m = (mid[0] * mid[0] + mid[1] * mid[1] + mid[2] * mid[2]).sqrt();
            let cos = (mid[0] * gs.vecs[axis][0] + mid[1] * gs.vecs[axis][1] + mid[2] * gs.vecs[axis][2]) / m;
            assert!(cos > 0.999, "axis {axis}: {cos}");
        }
    }

    #[test]
    fn a_reoriented_specimen_gets_its_hole_direction_from_the_reorientation() {
        use ams_core::core::{Arrow, CoreSetup, Face};
        let dir = std::env::temp_dir().join(format!("gis-hole-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("1234567_use.ams"), dir.join("1234568_use.ams"));
        for f in [&a, &b] {
            std::fs::write(f, b"").unwrap();
        }
        assert!(reorientation_hole(&a).is_none());
        let setup = CoreSetup { dip_dir: 316.1, plunge: 67.0, offset: 10.0, face: Face::ALL[0], arrow: Arrow::ALL[0] };
        let geo = crate::app::export_ui::geo_path(&a);
        std::fs::create_dir_all(geo.parent().unwrap()).unwrap();
        ams_core::depjson::write(&geo, &[("A1".to_string(), Some(setup))]).unwrap();
        let h = reorientation_hole(&a).unwrap();
        assert_eq!((h.dip_dir, h.plunge), (316.1, 67.0));
        assert!(reorientation_hole(&b).is_none(), "another file of the folder has its own entry");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pair_pasted_into_one_box_is_split() {
        assert_eq!(split_pair("47.1 10.5"), Some(("47.1".into(), "10.5".into())));
        assert_eq!(split_pair("47,12345; 10,54321"), Some(("47,12345".into(), "10,54321".into())));
        assert_eq!(split_pair("47.1, 10.5"), Some(("47.1".into(), "10.5".into())));
        assert_eq!(split_pair("412345.678\t4851234.567"), Some(("412345.678".into(), "4851234.567".into())));
        assert_eq!(split_pair("47.1"), None);
        assert_eq!(split_pair("47 07 24 N 10 32 35 E"), None);
    }

    #[test]
    fn the_link_colour_follows_the_match_score() {
        let mk = |link: Option<usize>, cands: Vec<(usize, u8)>| LocRow { path: PathBuf::new(), id: "A".into(), on: true, saved: None, cands, link, manual: None };
        assert_eq!(mk(None, vec![(0, 6)]).link_score(), None);
        assert_eq!(mk(Some(0), vec![(0, 6), (1, 2)]).link_score(), Some(6));
        assert_eq!(mk(Some(1), vec![(0, 6), (1, 2)]).link_score(), Some(2));
        assert_eq!(mk(Some(7), vec![(0, 6)]).link_score(), Some(0));
    }
}
