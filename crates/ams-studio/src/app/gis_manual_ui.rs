//! "Input manually" of the GIS viewer: a small window that asks for the specimen ID and its two coordinates, as they
//! are written in a company datasheet. The program says what they look like (degrees, degrees-minutes-seconds, UTM
//! metres, Gauss-Krueger), the user answers yes or no, and on no searches the coordinate systems by name. The tick
//! box *Enter for all specimens* turns the window into a list (a row per specimen: ID and the two coordinates) so
//! that everything can be typed in one go; the coordinate system of the list is confirmed once.

use super::gis_data::CrsChoice;
use super::gis_georef::crs_picker;
use super::gis_loc::Location;
use super::gis_loc_ui::{chain, split_pair, LINK_SAME, LINK_WEAK};
use super::{info_icon, tools_ui, App, W_GIS_MANUAL};
use ams_core::gis::{self, Crs};
use egui::{Color32, Key, RichText, Vec2};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BRow {
    pub id: String,
    pub a: String,
    pub b: String,
}

#[derive(Default)]
pub struct ManualState {
    pub win: bool,
    /// The list for all specimens instead of the single entry.
    pub all: bool,
    pub id: String,
    pub tx: String,
    pub ty: String,
    /// The user said no to the detected system: the picker is shown.
    pub choose: bool,
    pub pick: CrsChoice,
    /// The first number is the latitude / northing.
    pub swap: bool,
    pub last_crs: Option<Crs>,
    /// The specimen the single entry showed last, and the numbers that were filled in from its saved place (to tell them
    /// from what the user typed).
    pub shown_for: Option<usize>,
    pub auto_vals: Option<(String, String)>,
    /// UTM zone and hemisphere offered while the numbers look like UTM metres (they are not in the numbers).
    pub zone: u8,
    pub north: bool,
    pub rows: Vec<BRow>,
    /// The coordinate system of the list once confirmed: the system and whether the first number is y.
    pub bulk_sys: Option<(Crs, bool)>,
    pub bulk_choose: bool,
    pub bulk_pick: CrsChoice,
    pub bulk_swap: bool,
    pub msg: String,
}

/// The place two typed numbers give in a system; the first number is y when `first_is_y`.
pub fn eval_pair(a: &str, b: &str, crs: Crs, first_is_y: bool) -> Result<Location, String> {
    let (Some(va), Some(vb)) = (gis::parse_angle(a), gis::parse_angle(b)) else { return Err("not two numbers".into()) };
    let (x, y) = if first_is_y { (vb, va) } else { (va, vb) };
    Location::new(x, y, crs, "typed").ok_or_else(|| "not a point on the earth in this system".to_string())
}

/// The specimen an ID typed by the user stands for: the row, and the match score (6 = same name). A name that fits
/// several specimens equally well (`ZK1` for `ZK101` and `ZK102`) stands for none.
pub fn resolve_id(id: &str, ids: &[String]) -> Result<(usize, u8), String> {
    if id.trim().is_empty() {
        return Err("Type the ID of the specimen.".into());
    }
    let ranked = gis::rank_names(id, ids);
    match ranked.as_slice() {
        [(i, 6), ..] => Ok((*i, 6)),
        [(i, s), rest @ ..] if *s >= 4 => {
            if rest.first().is_some_and(|r| r.1 == *s) {
                Err(format!("{id} fits several specimens: type more of the ID."))
            } else {
                Ok((*i, *s))
            }
        }
        _ => Err(format!("No specimen of the file tree is called {id}.")),
    }
}

fn falls_at(l: &Location) -> String {
    let (ns, ew) = (if l.lat >= 0.0 { "N" } else { "S" }, if l.lon >= 0.0 { "E" } else { "W" });
    format!("{:.5} {ns}, {:.5} {ew}", l.lat.abs(), l.lon.abs())
}

const RED: Color32 = Color32::from_rgb(200, 80, 40);

/// Puts the saved place of a specimen into the number boxes, with the system it was saved in (the UTM zone and
/// hemisphere in their boxes, any other system in the picker), so it can be looked at and changed.
fn fill_from_saved(m: &mut ManualState, loc: &Location) {
    m.swap = false;
    m.choose = false;
    match loc.crs {
        // degrees: latitude first, the written order
        Crs::LonLat => {
            m.tx = format!("{}", loc.y);
            m.ty = format!("{}", loc.x);
        }
        Crs::Utm { zone, north } => {
            m.tx = format!("{}", loc.x);
            m.ty = format!("{}", loc.y);
            m.zone = zone;
            m.north = north;
        }
        other => {
            m.tx = format!("{}", loc.x);
            m.ty = format!("{}", loc.y);
            m.choose = true;
            m.pick = CrsChoice::from_crs(&other);
        }
    }
    m.auto_vals = Some((m.tx.clone(), m.ty.clone()));
}

/// The two settings UTM numbers do not carry: the zone (a text box that can be typed in) and the hemisphere.
fn utm_controls(ui: &mut egui::Ui, zone: &mut u8, north: &mut bool) {
    ui.horizontal(|ui| {
        ui.label("UTM zone");
        ui.add(egui::DragValue::new(zone).range(1..=60).speed(0.1));
        ui.selectable_value(north, true, "North");
        ui.selectable_value(north, false, "South");
        info_icon(ui, "Metres in UTM do not say which zone they belong to. The zone number is printed on the map or in the datasheet header (for example 33 in the Alps, 30 in Scotland); North is the usual hemisphere. Change them and the place below follows; if it lands in the wrong region, the zone or the hemisphere is wrong.");
    });
}

impl App {
    /// Opens the window for the specimen `id`, or the first specimen without a place.
    pub(super) fn gis_open_manual(&mut self, id: Option<String>) {
        self.loc_sync_rows();
        let first_free = self.gis.loc.rows.iter().find(|r| r.saved.is_none()).map(|r| r.id.clone());
        let zone = self.zone_hint();
        let m = &mut self.gis.manual;
        if m.zone == 0 {
            m.zone = zone;
            m.north = true;
        }
        let _ = zone;
        m.win = true;
        m.all = false;
        m.id = id.or(first_free).unwrap_or_default();
        m.tx.clear();
        m.ty.clear();
        m.choose = false;
        m.swap = false;
        m.bulk_sys = None;
        m.bulk_choose = false;
        m.shown_for = None;
        m.auto_vals = None;
        m.msg.clear();
        self.win_serial[W_GIS_MANUAL] += 1;
    }

    pub(super) fn gis_manual_window(&mut self, ctx: &egui::Context) {
        if !self.gis.manual.win {
            return;
        }
        self.loc_sync_rows();
        let ids: Vec<String> = self.gis.loc.rows.iter().map(|r| r.id.clone()).collect();
        let saved: Vec<Option<String>> = self.gis.loc.rows.iter().map(|r| r.saved.as_ref().map(|s| s.summary())).collect();
        let saved_locs: Vec<Option<Location>> = self.gis.loc.rows.iter().map(|r| r.saved.clone()).collect();
        let mut m = std::mem::take(&mut self.gis.manual);
        // the list: one row per specimen of the file tree, what was typed before is kept by ID
        if m.all {
            let old: HashMap<String, BRow> = m.rows.drain(..).map(|r| (r.id.clone(), r)).collect();
            m.rows = ids.iter().map(|id| old.get(id).cloned().unwrap_or(BRow { id: id.clone(), ..Default::default() })).collect();
            // rows the user retyped the ID of stay as they are
            for (k, r) in old.into_values().enumerate() {
                if !ids.contains(&r.id) && !(r.a.is_empty() && r.b.is_empty()) && k < 500 {
                    m.rows.push(r);
                }
            }
        }
        if self.test_bulk_yes && m.all && m.bulk_sys.is_none() {
            if let Some(f) = m.rows.iter().find(|r| gis::parse_angle(&r.a).is_some() && gis::parse_angle(&r.b).is_some()) {
                if let Ok(d) = gis::detect_crs(&f.a, &f.b, m.zone.max(1), m.north, None) {
                    m.bulk_sys = Some((d.crs, d.swapped));
                    self.test_bulk_yes = false;
                }
            }
        }
        let mut close = false;
        let mut save_all = false;
        let mut save_one: Option<(usize, Location)> = None;
        let mut save_many: Vec<(usize, Location)> = vec![];
        let scr = ctx.content_rect();
        let size = if m.all {
            Vec2::new((scr.width() - 80.0).clamp(760.0, 900.0), (300.0 + 46.0 * ids.len() as f32).clamp(480.0, scr.height() - 70.0))
        } else {
            Vec2::new(720.0, 360.0)
        };
        // the window follows its content: the single entry is small, the list makes it grow towards the bottom
        let win = self.base_window(ctx, "Enter coordinates", W_GIS_MANUAL, size).auto_sized();
        let mut open = true;
        let list_max = (scr.height() - 470.0).clamp(180.0, 46.0 * ids.len().max(1) as f32 + 40.0);
        win.open(&mut open).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.15);
            ui.set_width(900.0);
            let mut all = m.all;
            ui.spacing_mut().item_spacing = Vec2::new(14.0, 14.0);
            ui.spacing_mut().button_padding = Vec2::new(16.0, 7.0);
            ui.spacing_mut().interact_size.y = 32.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(if m.all { "What are the coordinates of the specimens?" } else { "What are the coordinates of this specimen?" }).strong().size(20.0));
                info_icon(ui, "Type the ID of the specimen and its two coordinates as they are written in your datasheet: 47.12345 and 10.54321, or 47°07'24\"N and 10°32'35\"E, or 412345.678 and 4851234.567. Both numbers can go into the first box when they are separated by a space, a semicolon or a comma and a space. The program says what the numbers look like and where that is, and you answer yes or no. The place is saved in the dependency file of the specimen (its _geo folder); the .ams file is not changed.");
                if m.all {
                    ui.add_space((ui.available_width() - 215.0).max(8.0));
                    ui.checkbox(&mut all, "Enter for all specimens").on_hover_text("Back to one specimen at a time");
                }
            });
            if !m.all {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Specimen ID").small());
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut m.id).desired_width(150.0).hint_text("ZK101"));
                            egui::ComboBox::from_id_salt("man-id-pick").selected_text("▾").width(10.0).show_ui(ui, |ui| {
                                for (i, id) in ids.iter().enumerate() {
                                    let mark = if saved[i].is_some() { "   ✓ has a place" } else { "" };
                                    if ui.selectable_label(m.id == *id, format!("{id}{mark}")).clicked() {
                                        m.id = id.clone();
                                    }
                                }
                            });
                        });
                    });
                    ui.vertical(|ui| {
                        ui.label(RichText::new("First number").small());
                        let r = ui.add(egui::TextEdit::singleline(&mut m.tx).desired_width(190.0).hint_text("47.12345  or  412345.678"));
                        if m.ty.trim().is_empty() && r.changed() {
                            if let Some((a, b)) = split_pair(&m.tx) {
                                m.tx = a;
                                m.ty = b;
                            }
                        }
                    });
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Second number").small());
                        ui.add(egui::TextEdit::singleline(&mut m.ty).desired_width(190.0).id(egui::Id::unique("man-second")));
                    });
                    // the tick box at the right end of the row
                    ui.add_space((ui.available_width() - 215.0).max(8.0));
                    ui.vertical(|ui| {
                        ui.add_space(24.0);
                        ui.checkbox(&mut all, "Enter for all specimens").on_hover_text("Turns this window into a list with a row for every specimen of the file tree, so you can type all IDs and coordinates in one go");
                    });
                });
            }
            if all != m.all {
                if all {
                    // the numbers typed for one specimen go into its row of the list instead of being dropped
                    if !m.tx.trim().is_empty() || !m.ty.trim().is_empty() {
                        if let Ok((i, _)) = resolve_id(&m.id, &ids) {
                            let row = BRow { id: ids[i].clone(), a: m.tx.clone(), b: m.ty.clone() };
                            match m.rows.iter_mut().find(|r| r.id == row.id) {
                                Some(r) => *r = row,
                                None => m.rows.push(row),
                            }
                            m.bulk_swap = m.swap;
                        }
                    }
                } else if m.tx.trim().is_empty() && m.ty.trim().is_empty() {
                    // and back: the row of the specimen in the single entry
                    if let Ok((i, _)) = resolve_id(&m.id, &ids) {
                        if let Some(r) = m.rows.iter().find(|r| r.id == ids[i]) {
                            m.tx = r.a.clone();
                            m.ty = r.b.clone();
                        }
                    }
                }
                m.all = all;
                m.bulk_sys = None;
                m.bulk_choose = false;
                m.choose = false;
            }

            if !m.all {
                // ---- one specimen
                let resolved = resolve_id(&m.id, &ids);
                // the specimen changed: a place that is saved for it shows up in the boxes (system and zone included)
                let cur = resolved.as_ref().ok().map(|(i, _)| *i);
                if cur != m.shown_for {
                    let typed_by_user = match &m.auto_vals {
                        Some((a, b)) => (m.tx != *a || m.ty != *b) && !(m.tx.is_empty() && m.ty.is_empty()),
                        None => !(m.tx.trim().is_empty() && m.ty.trim().is_empty()),
                    };
                    if !typed_by_user {
                        match cur.and_then(|i| saved_locs[i].as_ref()) {
                            Some(loc) => fill_from_saved(&mut m, loc),
                            None if m.auto_vals.is_some() => {
                                // the numbers of the specimen before are not meant for this one
                                m.tx.clear();
                                m.ty.clear();
                                m.auto_vals = None;
                                m.choose = false;
                            }
                            None => {}
                        }
                    }
                    m.shown_for = cur;
                }
                match &resolved {
                    Ok((i, s)) => {
                        let how = if *s == 6 { "same name".to_string() } else { format!("close name, {}", ids[*i]) };
                        let old = saved[*i].as_ref().map_or(String::new(), |t| format!("   It has a place already ({t}); it will be replaced."));
                        ui.label(RichText::new(format!("Specimen {} ({how}).{old}", ids[*i])).small().color(if *s == 6 { LINK_SAME } else { LINK_WEAK }));
                    }
                    Err(e) if !m.id.trim().is_empty() => {
                        ui.label(RichText::new(e).small().color(RED));
                    }
                    Err(_) => {}
                }
                let (a, b) = if m.swap { (m.ty.clone(), m.tx.clone()) } else { (m.tx.clone(), m.ty.clone()) };
                let both = !m.tx.trim().is_empty() && !m.ty.trim().is_empty();
                let enter = ui.input(|i| i.key_pressed(Key::Enter)) && ui.memory(|mem| mem.had_focus_last_frame(egui::Id::unique("man-second")) || mem.has_focus(egui::Id::unique("man-second")));
                if both {
                    if !m.choose {
                        let last = m.last_crs.filter(|c| !matches!(c, Crs::Utm { .. }));
                        match gis::detect_crs(&a, &b, m.zone.max(1), m.north, last) {
                            Ok(d) => match Location::new(d.x, d.y, d.crs, "typed") {
                                Some(loc) => {
                                    ui.label(format!("These look like {}.", d.what));
                                    if matches!(d.crs, Crs::Utm { .. }) {
                                        utm_controls(ui, &mut m.zone, &mut m.north);
                                    }
                                    ui.label(RichText::new(format!("That is at {}. Is the system right?", falls_at(&loc))).strong());
                                    ui.horizontal(|ui| {
                                        let yes = ui.add_enabled(resolved.is_ok(), egui::Button::new("Yes, save").min_size(Vec2::new(140.0, 36.0))).clicked() || (enter && resolved.is_ok());
                                        if ui.add(egui::Button::new("No, choose the system ...").min_size(Vec2::new(220.0, 36.0))).clicked() {
                                            m.choose = true;
                                            m.pick = CrsChoice { zone: m.zone.max(1), north: m.north, ..Default::default() };
                                        }
                                        ui.checkbox(&mut m.swap, "Swap the two numbers");
                                        if yes {
                                            if let Ok((i, _)) = resolved {
                                                save_one = Some((i, loc.clone()));
                                            }
                                        }
                                    });
                                }
                                None => {
                                    ui.label(RichText::new(format!("Read as {}, but that is not a point on the earth: choose the system.", d.what)).color(RED));
                                    if ui.button("Choose the system ...").clicked() {
                                        m.choose = true;
                                        m.pick = CrsChoice { zone: m.zone.max(1), north: m.north, ..Default::default() };
                                    }
                                }
                            },
                            Err(e) => {
                                ui.label(RichText::new(e).color(RED));
                                if ui.button("Choose the system ...").clicked() {
                                    m.choose = true;
                                    m.pick = CrsChoice { zone: m.zone.max(1), north: m.north, ..Default::default() };
                                }
                            }
                        }
                    } else {
                        ui.label(RichText::new("Which coordinate system are they in?").strong());
                        crs_picker(ui, "man-pick", &mut m.pick);
                        let loc = m.pick.crs().ok().and_then(|c| eval_pair(&a, &b, c, false).ok());
                        match &loc {
                            Some(l) => {
                                ui.label(RichText::new(format!("That is at {}.", falls_at(l))).small());
                            }
                            None => {
                                ui.label(RichText::new("With this system the numbers are not a point on the earth yet.").small().color(LINK_WEAK));
                            }
                        }
                        ui.horizontal(|ui| {
                            let go = ui.add_enabled(loc.is_some() && resolved.is_ok(), egui::Button::new("Use this system and save").min_size(Vec2::new(240.0, 36.0))).clicked();
                            if ui.add(egui::Button::new("Back").min_size(Vec2::new(90.0, 36.0))).clicked() {
                                m.choose = false;
                            }
                            ui.checkbox(&mut m.swap, "Swap the two numbers");
                            if go {
                                if let (Some(l), Ok((i, _))) = (&loc, &resolved) {
                                    save_one = Some((*i, l.clone()));
                                }
                            }
                        });
                    }
                }
            } else {
                // ---- all specimens in one list
                ui.label(RichText::new("A row per specimen: its ID and the two coordinates. Rows left empty are skipped; all rows are in the same coordinate system, which you confirm once below the list.").small());
                let first = m.rows.iter().find(|r| gis::parse_angle(&r.a).is_some() && gis::parse_angle(&r.b).is_some()).cloned();
                let list_h = list_max;
                let sys = m.bulk_sys;
                egui::ScrollArea::vertical().max_height(list_h).auto_shrink([false, true]).id_salt("man-list").show(ui, |ui| {
                    egui::Grid::new("man-grid").num_columns(5).spacing([20.0, 12.0]).show(ui, |ui| {
                        for h in ["", "Specimen ID", "First number", "Second number", "Result"] {
                            ui.label(RichText::new(h).strong().small());
                        }
                        ui.end_row();
                        for (k, r) in m.rows.iter_mut().enumerate() {
                            let resolved = resolve_id(&r.id, &ids);
                            let typed = !(r.a.trim().is_empty() && r.b.trim().is_empty());
                            let result: Option<Result<Location, String>> = match (typed, sys) {
                                (true, Some((c, y))) => Some(eval_pair(&r.a, &r.b, c, y)),
                                _ => None,
                            };
                            let ok = matches!((&resolved, &result), (Ok(_), Some(Ok(_))));
                            chain(ui, ok, LINK_SAME);
                            ui.add_sized([150.0, 34.0], egui::TextEdit::singleline(&mut r.id).id(egui::Id::unique(("man-id", k))));
                            ui.add_sized([190.0, 34.0], egui::TextEdit::singleline(&mut r.a).id(egui::Id::unique(("man-a", k))));
                            ui.add_sized([190.0, 34.0], egui::TextEdit::singleline(&mut r.b).id(egui::Id::unique(("man-b", k))));
                            match (&resolved, &result) {
                                (Err(e), _) if typed => ui.label(RichText::new(e).small().color(RED)),
                                (Ok(_), Some(Err(e))) => ui.label(RichText::new(e).small().color(RED)),
                                (Ok(_), Some(Ok(l))) => ui.label(RichText::new(format!("✓  {}", falls_at(l))).small().color(LINK_SAME)),
                                (Ok((i, _)), None) if !typed => match &saved[*i] {
                                    Some(t) => ui.label(RichText::new(format!("saved: {t}")).small().weak()),
                                    None => ui.label(RichText::new("no place yet").small().weak()),
                                },
                                _ => ui.label(RichText::new("waiting for the coordinate system").small().weak()),
                            };
                            ui.end_row();
                        }
                    });
                });
                // the one coordinate system of the list
                match (&first, m.bulk_sys) {
                    (None, _) => {
                        ui.label(RichText::new("Type the coordinates of at least one specimen.").weak());
                    }
                    (Some(f), None) => {
                        let (a, b) = if m.bulk_swap { (f.b.clone(), f.a.clone()) } else { (f.a.clone(), f.b.clone()) };
                        if !m.bulk_choose {
                            let last = m.last_crs.filter(|c| !matches!(c, Crs::Utm { .. }));
                            match gis::detect_crs(&a, &b, m.zone.max(1), m.north, last) {
                                Ok(d) => {
                                    ui.label(format!("The first row ({}) looks like {}.", f.id, d.what));
                                    if matches!(d.crs, Crs::Utm { .. }) {
                                        utm_controls(ui, &mut m.zone, &mut m.north);
                                    }
                                    ui.label(RichText::new("Are all rows in this system?").strong());
                                    ui.horizontal(|ui| {
                                        if ui.add(egui::Button::new("Yes, all rows").min_size(Vec2::new(160.0, 36.0))).clicked() {
                                            m.bulk_sys = Some((d.crs, d.swapped));
                                        }
                                        if ui.add(egui::Button::new("No, choose the system ...").min_size(Vec2::new(220.0, 36.0))).clicked() {
                                            m.bulk_choose = true;
                                            m.bulk_pick = CrsChoice { zone: m.zone.max(1), north: m.north, ..Default::default() };
                                        }
                                        ui.checkbox(&mut m.bulk_swap, "Swap the two numbers");
                                    });
                                }
                                Err(e) => {
                                    ui.label(RichText::new(e).color(RED));
                                    if ui.button("Choose the system ...").clicked() {
                                        m.bulk_choose = true;
                                        m.bulk_pick = CrsChoice { zone: m.zone.max(1), north: m.north, ..Default::default() };
                                    }
                                }
                            }
                        } else {
                            ui.label(RichText::new("Which coordinate system are all rows in?").strong());
                            crs_picker(ui, "man-bulk-pick", &mut m.bulk_pick);
                            let crs = m.bulk_pick.crs();
                            ui.horizontal(|ui| {
                                let swap = m.bulk_swap;
                                if ui.add_enabled(crs.is_ok(), egui::Button::new("Use this system for all rows").min_size(Vec2::new(260.0, 36.0))).clicked() {
                                    if let Ok(c) = crs {
                                        m.bulk_sys = Some((c, swap));
                                    }
                                }
                                if ui.add(egui::Button::new("Back").min_size(Vec2::new(90.0, 36.0))).clicked() {
                                    m.bulk_choose = false;
                                }
                                ui.checkbox(&mut m.bulk_swap, "Swap the two numbers");
                            });
                        }
                    }
                    (Some(_), Some((c, y))) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("All rows are read as {}.", c.label())).small());
                            if ui.small_button("Change ...").clicked() {
                                m.bulk_sys = None;
                                m.bulk_choose = false;
                            }
                        });
                        // UTM: the zone and the hemisphere can be edited right here
                        if let Crs::Utm { zone: z, north: n } = c {
                            let (mut z2, mut n2) = (z, n);
                            utm_controls(ui, &mut z2, &mut n2);
                            if (z2, n2) != (z, n) {
                                m.bulk_sys = Some((Crs::Utm { zone: z2, north: n2 }, y));
                                m.zone = z2;
                                m.north = n2;
                            }
                        }
                    }
                }
                if let Some((c, y)) = m.bulk_sys {
                    for r in &m.rows {
                        if r.a.trim().is_empty() && r.b.trim().is_empty() {
                            continue;
                        }
                        if let (Ok((i, _)), Ok(l)) = (resolve_id(&r.id, &ids), eval_pair(&r.a, &r.b, c, y)) {
                            save_many.push((i, l));
                        }
                    }
                }
            }
            if !m.msg.is_empty() {
                ui.label(RichText::new(&m.msg).small().color(LINK_SAME));
            }
            ui.separator();
            ui.label(RichText::new(if m.all { "Save the places of the rows that are in order?" } else { "Done with this one?" }).strong());
            ui.horizontal(|ui| {
                if m.all {
                    let n = save_many.len();
                    save_all = ui.add_enabled(n > 0, egui::Button::new(format!("Save all (n={n})")).min_size(Vec2::new(170.0, 38.0))).clicked();
                    if save_all {
                        m.msg = format!("Saved the places of (n={n}) specimens.");
                    }
                }
                close = ui.add(egui::Button::new("Close").min_size(Vec2::new(120.0, 38.0))).clicked();
            });
        });
        self.gis.manual = m;
        if let Some((i, loc)) = save_one {
            let crs = loc.crs;
            self.loc_store(i, Some(loc));
            let next = {
                let rows = &self.gis.loc.rows;
                (0..rows.len()).cycle().skip(i + 1).take(rows.len()).find(|&k| rows[k].saved.is_none())
            };
            let id_next = next.map(|k| self.gis.loc.rows[k].id.clone()).unwrap_or_default();
            let m = &mut self.gis.manual;
            m.last_crs = Some(crs);
            if let Crs::Utm { zone, north } = crs {
                m.zone = zone;
                m.north = north;
            }
            m.tx.clear();
            m.ty.clear();
            m.choose = false;
            m.swap = false;
            m.id = id_next;
            m.shown_for = None;
            m.auto_vals = None;
            m.msg = "Saved.".into();
        }
        if save_all && !save_many.is_empty() {
            let crs = save_many[0].1.crs;
            for (i, loc) in save_many {
                self.loc_store(i, Some(loc));
            }
            self.gis.manual.last_crs = Some(crs);
        }
        if !open || close {
            self.gis.manual.win = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_typed_id_finds_its_specimen_or_says_why_not() {
        let list = ids(&["ZK101", "ZK102", "ZK110", "PB7"]);
        assert_eq!(resolve_id("zk101", &list), Ok((0, 6)));
        assert_eq!(resolve_id("ZK-102", &list), Ok((1, 6)));
        assert_eq!(resolve_id("SITE-PB7", &list).unwrap().0, 3);
        assert!(resolve_id("ZK1", &list).unwrap_err().contains("several"));
        assert!(resolve_id("QQ9", &list).unwrap_err().contains("No specimen"));
        assert!(resolve_id("  ", &list).is_err());
    }

    #[test]
    fn two_typed_numbers_become_a_place() {
        let utm = Crs::Utm { zone: 33, north: true };
        let l = eval_pair("412345.678", "4851234.567", utm, false).unwrap();
        assert!((l.lon - 13.9102).abs() < 1e-3 && (l.lat - 43.8090).abs() < 1e-3);
        // the first number is the northing
        assert!(eval_pair("4851234.567", "412345.678", utm, true).is_ok());
        assert!(eval_pair("4851234.567", "412345.678", utm, false).is_err());
        assert!(eval_pair("x", "1", utm, false).is_err());
        let g = eval_pair("47.12345", "10.4221", Crs::LonLat, true).unwrap();
        assert!((g.lat - 47.12345).abs() < 1e-9 && (g.lon - 10.4221).abs() < 1e-9);
    }
}
