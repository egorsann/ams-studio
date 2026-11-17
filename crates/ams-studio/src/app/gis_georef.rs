//! The two helper windows of the GIS viewer: the coordinates table (preview of the file, the user says which column
//! is what) and the georeferencing of a map image (matching points on the image and on the OpenStreetMap basemap,
//! like QGIS' georeferencer).

use super::gis_data::{self as gd, CoordKind, CrsChoice};
use super::gis_ui::{draw_map_image, draw_tiles, Gis, View};
use super::{info_icon, tools_ui, App, W_GEOREF, W_GIS_TABLE};
use ams_core::gis::{self, Affine, ControlPoint, Crs, Georef};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

pub struct GeorefWin {
    /// The map layer being placed.
    pub layer: u32,
    /// Centre of the image view (image pixels) and points per image pixel.
    pub img_cx: f64,
    pub img_cy: f64,
    pub img_scale: f64,
    pub img_init: bool,
    pub base: View,
    pub base_init: bool,
    pub pairs: Vec<ControlPoint>,
    pub pending_px: Option<(f64, f64)>,
    pub fit: Option<Result<Affine, String>>,
    pub resid: Vec<f64>,
    pub rms: f64,
    pub typed: CrsChoice,
    pub tx: String,
    pub ty: String,
    pub goto_lat: String,
    pub goto_lon: String,
    pub preview: bool,
    pub alpha: f32,
    pub msg: String,
}

impl Default for GeorefWin {
    fn default() -> Self {
        GeorefWin {
            layer: 0,
            img_cx: 0.0,
            img_cy: 0.0,
            img_scale: 1.0,
            img_init: false,
            base: View::world(),
            base_init: false,
            pairs: vec![],
            pending_px: None,
            fit: None,
            resid: vec![],
            rms: 0.0,
            typed: CrsChoice::default(),
            tx: String::new(),
            ty: String::new(),
            goto_lat: String::new(),
            goto_lon: String::new(),
            preview: true,
            alpha: 0.6,
            msg: String::new(),
        }
    }
}

impl GeorefWin {
    pub fn refit_pub(&mut self) {
        self.refit();
    }

    fn refit(&mut self) {
        self.fit = if self.pairs.len() >= 2 { Some(gis::fit_affine(&self.pairs)) } else { None };
        self.resid.clear();
        self.rms = 0.0;
        if let Some(Ok(a)) = &self.fit {
            let (r, rms) = gis::residuals(&self.pairs, a);
            self.resid = r;
            self.rms = rms;
        }
    }
}

fn marker(p: &egui::Painter, c: Pos2, n: usize, col: Color32, dark: bool) {
    let halo = if dark { Color32::BLACK } else { Color32::WHITE };
    p.circle_stroke(c, 7.5, Stroke::new(3.5, halo));
    p.circle_stroke(c, 7.5, Stroke::new(1.6, col));
    p.line_segment([c - Vec2::new(11.0, 0.0), c - Vec2::new(4.0, 0.0)], Stroke::new(1.6, col));
    p.line_segment([c + Vec2::new(4.0, 0.0), c + Vec2::new(11.0, 0.0)], Stroke::new(1.6, col));
    p.line_segment([c - Vec2::new(0.0, 11.0), c - Vec2::new(0.0, 4.0)], Stroke::new(1.6, col));
    p.line_segment([c + Vec2::new(0.0, 4.0), c + Vec2::new(0.0, 11.0)], Stroke::new(1.6, col));
    if n > 0 {
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            p.text(c + Vec2::new(11.0 + dx, -10.0 + dy), Align2::LEFT_CENTER, n.to_string(), FontId::proportional(13.0), halo);
        }
        p.text(c + Vec2::new(11.0, -10.0), Align2::LEFT_CENTER, n.to_string(), FontId::proportional(13.0), col);
    }
}

/// Picks a coordinate system: degrees, UTM, Web Mercator, any system of the EPSG registry (by code or by searching
/// its name), or a PROJ string.
pub fn crs_picker(ui: &mut Ui, id: &str, c: &mut CrsChoice) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut c.kind, CoordKind::LonLat, "Lon / lat (degrees)");
        ui.selectable_value(&mut c.kind, CoordKind::Utm, "UTM");
        ui.selectable_value(&mut c.kind, CoordKind::WebMerc, "Web Mercator");
        ui.selectable_value(&mut c.kind, CoordKind::Epsg, "Other (EPSG code)");
        ui.selectable_value(&mut c.kind, CoordKind::Proj, "PROJ string");
        info_icon(ui, "Lon / lat: decimal degrees or degrees-minutes-seconds on WGS 84 (ETRS89 and NAD83 agree to a metre or two). UTM: zone and hemisphere, on WGS 84 / ETRS89 / NAD83. Other: any of about 6000 geographic and projected systems of the EPSG registry (Gauss-Krueger / Pulkovo 1942, State Plane, Lambert, Albers, polar stereographic, national grids, other datums), by code or by searching the name. The datum shift to WGS 84 is the one the registry gives (three or seven parameters); grid shifts such as OSTN15 or NTv2 are not applied, so a few national grids are off by metres, far below a map symbol. PROJ string: for a system that is not in the registry.");
    });
    match c.kind {
        CoordKind::Utm => {
            ui.horizontal(|ui| {
                ui.label("UTM zone");
                ui.add(egui::DragValue::new(&mut c.zone).range(1..=60));
                ui.selectable_value(&mut c.north, true, "North");
                ui.selectable_value(&mut c.north, false, "South");
                info_icon(ui, "The zone number is printed on the map (for example 33 in the Alps, 30 in Scotland). If the places land in the wrong region on the basemap, the zone or the hemisphere is wrong.");
            });
        }
        CoordKind::Epsg => {
            ui.horizontal(|ui| {
                ui.label("EPSG code");
                ui.add(egui::TextEdit::singleline(&mut c.epsg).desired_width(70.0).hint_text("28412"));
                ui.label("or search by name");
                ui.add(egui::TextEdit::singleline(&mut c.search).desired_width(210.0).hint_text("pulkovo gauss zone 12"));
            });
            if !c.search.trim().is_empty() {
                let hits = gis::epsg_search(&c.search, 8);
                if hits.is_empty() {
                    ui.label(RichText::new("No system with all those words.").small().weak());
                }
                let mut pick = None;
                for (code, name) in &hits {
                    if ui.selectable_label(c.epsg.trim() == code.to_string(), format!("{code}   {name}")).clicked() {
                        pick = Some(*code);
                    }
                }
                if let Some(code) = pick {
                    c.epsg = code.to_string();
                    c.search.clear();
                }
            }
            match c.crs() {
                Ok(x) if !c.epsg.trim().is_empty() => {
                    ui.label(RichText::new(format!("= {}", x.label())).small().weak());
                }
                Err(e) if !c.epsg.trim().is_empty() => {
                    ui.label(RichText::new(e).small().color(Color32::from_rgb(200, 80, 40)));
                }
                _ => {}
            }
        }
        CoordKind::Proj => {
            ui.add(egui::TextEdit::singleline(&mut c.proj).desired_width(460.0).hint_text("+proj=tmerc +lat_0=0 +lon_0=69 +k=1 +x_0=500000 +y_0=0 +ellps=krass +towgs84=23.92,-141.27,-80.9,0,0.35,0.82,-0.12 +units=m"));
            if !c.proj.trim().is_empty() {
                if let Err(e) = c.crs() {
                    ui.label(RichText::new(e).small().color(Color32::from_rgb(200, 80, 40)));
                }
            }
        }
        _ => {}
    }
    let _ = id;
}

const PAIR_COL: Color32 = Color32::from_rgb(220, 40, 40);
const PENDING_COL: Color32 = Color32::from_rgb(255, 150, 20);

impl App {
    /// Opens the georeferencing window for a map layer.
    pub(super) fn gis_open_georef(&mut self, layer: u32) {
        let g = &mut self.gis;
        let Some(m) = g.maps.iter().find(|m| m.id == layer) else { return };
        let pairs = m.points.clone();
        g.gr = Default::default();
        g.gr.layer = layer;
        g.gr.pairs = pairs;
        g.gr.refit();
        g.gr.typed = CrsChoice { zone: gis::utm_zone(gis::lonlat_from_merc(g.view.cx, g.view.cy).0), ..Default::default() };
        g.win_georef = true;
        self.win_serial[W_GEOREF] += 1;
    }

    pub(super) fn gis_georef_window(&mut self, ctx: &egui::Context) {
        if !self.gis.win_georef {
            return;
        }
        let mut open = true;
        let scr = ctx.content_rect();
        let size = Vec2::new((scr.width() - 40.0).clamp(760.0, 1300.0), (scr.height() - 60.0).clamp(560.0, 860.0));
        self.base_window(ctx, "Georeference map", W_GEOREF, size).open(&mut open).resizable(true).show(ctx, |ui| {
            self.georef_ui(ui);
        });
        self.gis.win_georef = open && self.gis.win_georef;
    }

    fn georef_ui(&mut self, ui: &mut Ui) {
        let dark = self.dark;
        let ppp = ui.ctx().pixels_per_point();
        ui.set_min_size(Vec2::new(700.0, 520.0));
        let Some((tex, orig, tex_handle)) = self.gis.maps.iter().find(|m| m.id == self.gis.gr.layer).map(|m| (m.tex.id(), m.orig, m.tex.clone())) else {
            ui.label("This map layer is gone.");
            return;
        };
        let Gis { gr, tiles, base_on, maps, locs, placed, view, .. } = &mut self.gis;
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Which spot on the map image is which spot on the earth?").strong().size(17.0));
            info_icon(ui, "Click a recognisable spot on the map image (left: a road crossing, a corner tick of the map frame, a river bend), then the same spot on the OpenStreetMap basemap (right). Repeat with at least three spots spread over the whole map; two give only a rough fit (scale, rotation, shift), three or more fit a general affine transform and the error of every point is shown. The fit is drawn over the basemap so you can check it. Scroll zooms, drag moves. If a point is not on the basemap but its coordinates are printed on the map, type them instead of clicking.");
        });
        ui.label(RichText::new("1. Click a spot on the map image (left).   2. Click the same spot on the basemap (right).   Repeat for 3 or more spots.").small());
        ui.horizontal(|ui| {
            ui.checkbox(base_on, "OpenStreetMap basemap").on_hover_text("Downloads map tiles for the area you look at from tile.openstreetmap.org (see the (i) in the viewer); off until ticked");
            ui.label("Go to latitude");
            ui.add(egui::TextEdit::singleline(&mut gr.goto_lat).desired_width(80.0));
            ui.label("longitude");
            ui.add(egui::TextEdit::singleline(&mut gr.goto_lon).desired_width(80.0));
            if ui.button("Go").clicked() {
                if let (Some(la), Some(lo)) = (gis::parse_angle(&gr.goto_lat), gis::parse_angle(&gr.goto_lon)) {
                    if la.abs() < gis::MAX_LAT && lo.abs() <= 180.0 {
                        let (mx, my) = gis::merc_from_lonlat(lo, la);
                        gr.base = View { cx: mx, cy: my, mpp: (4000.0 / 400.0f64).max(5.0) };
                        gr.msg.clear();
                    } else {
                        gr.msg = "Latitude or longitude out of range.".into();
                    }
                } else {
                    gr.msg = "Type the latitude and longitude as numbers (56.34 or 56°20'24\"N).".into();
                }
            }
            ui.checkbox(&mut gr.preview, "Show the fit over the basemap");
            if gr.preview {
                ui.add(egui::Slider::new(&mut gr.alpha, 0.1..=1.0).show_value(false));
            }
        });
        let avail = ui.available_rect_before_wrap();
        let canvas_h = (avail.height() - 235.0).max(200.0);
        let half = (avail.width() - 8.0) / 2.0;
        let lrect = Rect::from_min_size(avail.min, Vec2::new(half, canvas_h));
        let rrect = Rect::from_min_size(Pos2::new(avail.min.x + half + 8.0, avail.min.y), Vec2::new(half, canvas_h));
        ui.allocate_rect(Rect::from_min_size(avail.min, Vec2::new(avail.width(), canvas_h)), Sense::hover());
        let bg = if dark { Color32::from_rgb(28, 32, 38) } else { Color32::from_rgb(232, 235, 238) };

        // ---- the map image
        {
            let rect = lrect;
            let resp = ui.interact(rect, ui.scope_id().with("gr-img"), Sense::click_and_drag());
            if !gr.img_init {
                gr.img_init = true;
                gr.img_scale = (f64::from(rect.width()) / f64::from(orig.0)).min(f64::from(rect.height()) / f64::from(orig.1)) * 0.96;
                gr.img_cx = f64::from(orig.0) / 2.0;
                gr.img_cy = f64::from(orig.1) / 2.0;
            }
            let c = rect.center();
            let s = gr.img_scale;
            if resp.dragged() {
                gr.img_cx -= f64::from(resp.drag_delta().x) / s;
                gr.img_cy -= f64::from(resp.drag_delta().y) / s;
            }
            if let Some(h) = resp.hover_pos() {
                let (zd, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
                let f = f64::from(zd) * (f64::from(scroll.y) * 0.0025).exp();
                if (f - 1.0).abs() > 1e-6 {
                    let before = (gr.img_cx + f64::from(h.x - c.x) / s, gr.img_cy + f64::from(h.y - c.y) / s);
                    gr.img_scale = (s * f).clamp(0.005, 60.0);
                    gr.img_cx = before.0 - f64::from(h.x - c.x) / gr.img_scale;
                    gr.img_cy = before.1 - f64::from(h.y - c.y) / gr.img_scale;
                }
            }
            let (s, cx, cy) = (gr.img_scale, gr.img_cx, gr.img_cy);
            let to_screen = |px: f64, py: f64| Pos2::new(c.x + ((px - cx) * s) as f32, c.y + ((py - cy) * s) as f32);
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, bg);
            let ir = Rect::from_min_max(to_screen(0.0, 0.0), to_screen(f64::from(orig.0), f64::from(orig.1)));
            painter.image(tex, ir, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            for (i, ((px, py), _)) in gr.pairs.iter().enumerate() {
                marker(&painter, to_screen(*px, *py), i + 1, PAIR_COL, dark);
            }
            if let Some((px, py)) = gr.pending_px {
                marker(&painter, to_screen(px, py), gr.pairs.len() + 1, PENDING_COL, dark);
            }
            painter.text(rect.left_top() + Vec2::new(8.0, 6.0), Align2::LEFT_TOP, "Map image", FontId::proportional(12.0), if dark { Color32::from_gray(200) } else { Color32::from_gray(70) });
            if resp.clicked() {
                if let Some(h) = resp.interact_pointer_pos() {
                    let (px, py) = (cx + f64::from(h.x - c.x) / s, cy + f64::from(h.y - c.y) / s);
                    if px >= 0.0 && py >= 0.0 && px <= f64::from(orig.0) && py <= f64::from(orig.1) {
                        gr.pending_px = Some((px, py));
                        gr.msg.clear();
                    }
                }
            }
        }

        // ---- the basemap
        {
            let rect = rrect;
            if !gr.base_init {
                gr.base_init = true;
                let boxed = if !placed.is_empty() || !locs.is_empty() {
                    let pts: Vec<(f64, f64)> = if placed.is_empty() { locs.iter().map(|l| l.merc).collect() } else { placed.iter().map(|p| p.merc).collect() };
                    let lo = pts.iter().fold((f64::MAX, f64::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1)));
                    let hi = pts.iter().fold((f64::MIN, f64::MIN), |a, p| (a.0.max(p.0), a.1.max(p.1)));
                    Some((lo, hi))
                } else {
                    maps.iter().find(|m| m.id == gr.layer).and_then(|m| m.extent())
                };
                gr.base = match boxed {
                    Some((a, b)) => View::fit(a, b, rect.size()),
                    None => *view,
                };
            }
            let resp = ui.interact(rect, ui.scope_id().with("gr-base"), Sense::click_and_drag());
            let c = rect.center();
            if resp.dragged() {
                gr.base.pan(resp.drag_delta());
            }
            if let Some(h) = resp.hover_pos() {
                let (zd, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
                let f = f64::from(zd) / (f64::from(scroll.y) * 0.0025).exp();
                if (f - 1.0).abs() > 1e-6 {
                    gr.base.zoom_at(c, h, f);
                }
            }
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, bg);
            tiles.begin_frame(ui.ctx());
            if *base_on {
                draw_tiles(&painter, rect, &gr.base, tiles, ppp, dark);
            }
            if gr.preview {
                if let Some(Ok(a)) = &gr.fit {
                    draw_map_image(&painter, &tex_handle, orig, &Georef { affine: *a, crs: Crs::WebMerc }, &gr.base, c, gr.alpha);
                }
            }
            for (i, (_, m)) in gr.pairs.iter().enumerate() {
                marker(&painter, gr.base.to_screen(c, *m), i + 1, PAIR_COL, dark);
            }
            painter.text(rect.left_top() + Vec2::new(8.0, 6.0), Align2::LEFT_TOP, "Basemap", FontId::proportional(12.0), if dark { Color32::from_gray(200) } else { Color32::from_gray(70) });
            if !*base_on {
                painter.text(c, Align2::CENTER_CENTER, "Tick OpenStreetMap basemap above, or type coordinates below", FontId::proportional(13.0), if dark { Color32::from_gray(190) } else { Color32::from_gray(90) });
            }
            if *base_on {
                let t = "© OpenStreetMap contributors";
                painter.text(rect.right_bottom() - Vec2::new(8.0, 6.0), Align2::RIGHT_BOTTOM, t, FontId::proportional(11.0), if dark { Color32::from_gray(210) } else { Color32::from_gray(40) });
            }
            let hover = resp.hover_pos();
            if let Some(h) = hover {
                let m = gr.base.to_merc(c, h);
                let (lon, lat) = gis::lonlat_from_merc(m.0, m.1);
                painter.text(rect.left_bottom() + Vec2::new(8.0, -6.0), Align2::LEFT_BOTTOM, format!("{lat:.5}, {lon:.5}"), FontId::proportional(11.5), if dark { Color32::from_gray(210) } else { Color32::from_gray(40) });
            }
            if resp.clicked() {
                if let Some(h) = resp.interact_pointer_pos() {
                    match gr.pending_px.take() {
                        Some(px) => {
                            gr.pairs.push((px, gr.base.to_merc(c, h)));
                            gr.refit();
                            gr.msg.clear();
                        }
                        None => gr.msg = "Click the spot on the map image (left) first.".into(),
                    }
                }
            }
            if *base_on {
                tiles.start(ui.ctx());
            }
        }

        // ---- the points
        ui.add_space(2.0);
        let mut remove: Option<usize> = None;
        egui::ScrollArea::vertical().max_height(86.0).auto_shrink([false, false]).id_salt("gr-pairs").show(ui, |ui| {
            if gr.pairs.is_empty() {
                ui.label(RichText::new("No points yet.").weak().small());
            }
            egui::Grid::new("gr-grid").num_columns(5).spacing([14.0, 2.0]).show(ui, |ui| {
                for (i, ((px, py), m)) in gr.pairs.iter().enumerate() {
                    let (lon, lat) = gis::lonlat_from_merc(m.0, m.1);
                    ui.label(RichText::new(format!("{}", i + 1)).strong());
                    ui.label(RichText::new(format!("image {px:.0}, {py:.0}")).small());
                    ui.label(RichText::new(format!("{lat:.5}, {lon:.5}")).small());
                    match gr.resid.get(i) {
                        Some(r) if gr.pairs.len() >= 3 => ui.label(RichText::new(format!("off by {r:.1} m")).small()),
                        _ => ui.label(""),
                    };
                    if ui.small_button("✕").on_hover_text("Remove this point").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(i) = remove {
            gr.pairs.remove(i);
            gr.refit();
        }
        if gr.pending_px.is_some() {
            ui.label(RichText::new("Know the coordinates of the orange spot? Type them, in this system:").small());
            crs_picker(ui, "gr-typed", &mut gr.typed);
            ui.horizontal(|ui| {
                let (lx, ly) = if gr.typed.is_degrees() { ("longitude", "latitude") } else { ("x / easting", "y / northing") };
                ui.label(lx);
                ui.add(egui::TextEdit::singleline(&mut gr.tx).desired_width(100.0));
                ui.label(ly);
                ui.add(egui::TextEdit::singleline(&mut gr.ty).desired_width(100.0));
                if ui.button("Use these coordinates").clicked() {
                    match (gr.typed.crs(), gis::parse_angle(&gr.tx), gis::parse_angle(&gr.ty)) {
                        (Err(e), _, _) => gr.msg = e,
                        (Ok(crs), Some(x), Some(y)) => {
                            let (lo, la) = crs.to_lonlat(x, y);
                            if lo.is_finite() && la.is_finite() && la.abs() < gis::MAX_LAT && lo.abs() <= 180.0 {
                                if let Some(px) = gr.pending_px.take() {
                                    gr.pairs.push((px, gis::merc_from_lonlat(lo, la)));
                                    gr.refit();
                                    gr.msg.clear();
                                }
                            } else {
                                gr.msg = "Those coordinates are outside the range of the chosen system.".into();
                            }
                        }
                        _ => gr.msg = "Type the coordinates as numbers.".into(),
                    }
                }
            });
        }
        let status = match &gr.fit {
            Some(Ok(_)) if gr.pairs.len() >= 3 => format!("Fit from (n={}) points: mean error {:.1} m. Over about 20 m is usually a mis-clicked point.", gr.pairs.len(), gr.rms),
            Some(Ok(_)) => "Two points: a rough fit (scale, rotation, shift) with no error estimate. Add more points.".to_string(),
            Some(Err(e)) => e.clone(),
            None => "Add at least two points.".to_string(),
        };
        ui.label(RichText::new(&status).small().color(if matches!(gr.fit, Some(Err(_))) { Color32::from_rgb(200, 80, 40) } else if gr.rms > 20.0 && gr.pairs.len() >= 3 { Color32::from_rgb(200, 120, 30) } else if dark { Color32::from_gray(200) } else { Color32::from_gray(60) }));
        if !gr.msg.is_empty() {
            ui.label(RichText::new(&gr.msg).small().color(Color32::from_rgb(200, 80, 40)));
        }
        ui.label(RichText::new("Is the placement good?").strong());
        let ok = matches!(gr.fit, Some(Ok(_)));
        let (mut apply, mut save, mut cancel) = (false, false, false);
        ui.horizontal(|ui| {
            apply = ui.add_enabled(ok, egui::Button::new("Apply to the map").min_size(Vec2::new(150.0, 32.0))).clicked();
            save = ui.add_enabled(ok, egui::Button::new("Apply and save next to the map").min_size(Vec2::new(230.0, 32.0))).on_hover_text("Also writes <map file>.georef.json next to the map image, so the map is placed the next time it is loaded. The map image itself is never changed.").clicked();
            if ui.add_enabled(!gr.pairs.is_empty(), egui::Button::new("Undo last point")).clicked() {
                gr.pairs.pop();
                gr.refit();
            }
            cancel = ui.button("Cancel").clicked();
        });
        let mut result: Option<String> = None;
        if apply || save {
            if let (Some(Ok(a)), Some(m)) = (&gr.fit, maps.iter_mut().find(|m| m.id == gr.layer)) {
                let g = Georef { affine: *a, crs: Crs::WebMerc };
                m.georef = Some(g);
                m.points = gr.pairs.clone();
                m.pending = None;
                m.note = if gr.pairs.len() >= 3 { format!("Georeferenced with (n={}) points, mean error {:.1} m.", gr.pairs.len(), gr.rms) } else { "Georeferenced with two points (rough).".to_string() };
                if save {
                    let text = gd::sidecar_text(&g, m.orig, &m.points);
                    match std::fs::write(gd::sidecar_path(&m.path), text) {
                        Ok(()) => m.note += " Saved next to the map file.",
                        Err(e) => m.note += &format!(" Could not save next to the map file: {e}."),
                    }
                }
                result = Some(m.note.clone());
            }
        }
        if result.is_some() {
            self.gis.fit_pending = true;
            self.gis.win_georef = false;
        }
        if cancel {
            self.gis.win_georef = false;
        }
    }

    pub(super) fn gis_table_window(&mut self, ctx: &egui::Context) {
        if !self.gis.win_table {
            return;
        }
        let Some((mut table, mut mapping)) = self.gis.draft.take() else {
            self.gis.win_table = false;
            return;
        };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let before_sheet = table.sheet;
        let (locs, bad) = gd::build_locs(table.grid(), &mapping);
        let scr = ctx.content_rect();
        let size = Vec2::new((scr.width() - 60.0).clamp(680.0, 1050.0), (scr.height() - 60.0).clamp(520.0, 800.0));
        self.base_window(ctx, "Coordinates table", W_GIS_TABLE, size).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.1);
            ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Which column holds what?").strong().size(19.0));
                info_icon(ui, "Choose the header row, then the column with the sample (or block) names, and the two coordinate columns. The preview below shows the file as it was read: the tags above the columns show what you assigned. Longitude / latitude may be decimal degrees (56.34 or 56,34) or degrees-minutes-seconds (56°20'24\"N). For projected coordinates choose UTM and the zone (a map of the area names it). Nothing is written to the file.");
            });
            ui.label(RichText::new(table.path.display().to_string()).monospace().small());
            if table.sheets.len() > 1 {
                ui.horizontal(|ui| {
                    ui.label("Sheet");
                    egui::ComboBox::from_id_salt("gt-sheet").selected_text(table.sheets[table.sheet].0.clone()).show_ui(ui, |ui| {
                        for (i, s) in table.sheets.iter().enumerate() {
                            ui.selectable_value(&mut table.sheet, i, s.0.clone());
                        }
                    });
                });
            }
            ui.horizontal(|ui| {
                ui.label("Header row");
                ui.add(egui::DragValue::new(&mut mapping.header_row).range(0..=40));
                info_icon(ui, "The row number (as in the preview) that holds the column titles; the places start on the next row. 0 means the table has no title row.");
            });
            let ncols = table.n_cols();
            let hdr: Vec<String> = (0..ncols)
                .map(|c| {
                    let t = if mapping.header_row > 0 { table.grid().get(mapping.header_row - 1).and_then(|r| r.get(c)).map(|s| s.trim().to_string()).unwrap_or_default() } else { String::new() };
                    let t: String = t.chars().take(22).collect();
                    if t.is_empty() { gd::col_letter(c) } else { format!("{}: {t}", gd::col_letter(c)) }
                })
                .collect();
            let (xl, yl) = if mapping.crs.is_degrees() { ("Longitude column", "Latitude column") } else { ("Easting (x) column", "Northing (y) column") };
            let col_combo = |ui: &mut Ui, id: &str, label: &str, sel: &mut Option<usize>| {
                ui.horizontal(|ui| {
                    ui.label(label);
                    egui::ComboBox::from_id_salt(id).selected_text(sel.map_or("(choose)".to_string(), |c| hdr.get(c).cloned().unwrap_or_default())).width(220.0).show_ui(ui, |ui| {
                        for (c, h) in hdr.iter().enumerate() {
                            ui.selectable_value(sel, Some(c), h);
                        }
                    });
                });
            };
            ui.horizontal(|ui| {
                ui.label("Sample / block name column");
                egui::ComboBox::from_id_salt("gt-name").selected_text(mapping.name.map_or("(none: number the rows 1, 2, 3 ...)".to_string(), |c| hdr.get(c).cloned().unwrap_or_default())).width(260.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut mapping.name, None, "(none: number the rows 1, 2, 3 ...)");
                    for (c, h) in hdr.iter().enumerate() {
                        ui.selectable_value(&mut mapping.name, Some(c), h);
                    }
                });
                info_icon(ui, "The column with the name of each sample or block, as in the specimen list (or the start of the specimen names). A table with only coordinates has no names: the rows are then numbered and matched to the specimens by their order (see Match names).");
            });
            ui.label("Coordinates are in");
            crs_picker(ui, "gt-crs", &mut mapping.crs);
            col_combo(ui, "gt-x", xl, &mut mapping.x);
            col_combo(ui, "gt-y", yl, &mut mapping.y);
            ui.separator();
            ui.label(RichText::new(format!("Preview of the file (first {} rows)", table.grid().len().min(40))).strong());
            let (cn, cx, cy) = (mapping.name, mapping.x, mapping.y);
            let accent = ui.visuals().hyperlink_color;
            egui::ScrollArea::both().max_height((ui.available_height() - 190.0).max(120.0)).id_salt("gt-preview").auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("gt-grid").striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
                    ui.label("");
                    for c in 0..ncols.min(24) {
                        let (xt, yt) = if mapping.crs.is_degrees() { ("LON", "LAT") } else { ("EAST", "NORTH") };
                        let tag = if Some(c) == cn { "NAME" } else if Some(c) == cx { xt } else if Some(c) == cy { yt } else { "" };
                        ui.label(RichText::new(tag).strong().small().color(accent));
                    }
                    ui.end_row();
                    for (r, row) in table.grid().iter().take(40).enumerate() {
                        let is_hdr = mapping.header_row > 0 && r + 1 == mapping.header_row;
                        let before_hdr = r + 1 < mapping.header_row;
                        ui.label(RichText::new((r + 1).to_string()).weak().small());
                        for c in 0..ncols.min(24) {
                            let t: String = row.get(c).map(|s| s.chars().take(24).collect()).unwrap_or_default();
                            let mut rt = RichText::new(t).small();
                            if is_hdr {
                                rt = rt.strong();
                            } else if before_hdr {
                                rt = rt.weak();
                            }
                            ui.label(rt);
                        }
                        ui.end_row();
                    }
                });
            });

            ui.separator();
            let total = table.grid().len().saturating_sub(mapping.header_row);
            ui.label(RichText::new(format!("Places read (n={}) of (n={total}) rows", locs.len())).strong());
            if let Some(l) = locs.first() {
                let (ns, ew) = (if l.lat >= 0.0 { "N" } else { "S" }, if l.lon >= 0.0 { "E" } else { "W" });
                ui.label(RichText::new(format!("The first place falls at {:.5} {ns}, {:.5} {ew}. Is that the right region? If not, the coordinate system (zone or hemisphere) is wrong.", l.lat.abs(), l.lon.abs())).small());
            }
            if !bad.is_empty() {
                let shown = bad.iter().take(6).cloned().collect::<Vec<_>>().join("\n");
                ui.label(RichText::new(format!("Rows that cannot be used (n={}):\n{shown}{}", bad.len(), if bad.len() > 6 { "\n..." } else { "" })).small().color(Color32::from_rgb(200, 120, 30)));
            }
            ui.label(RichText::new("Use these columns for the map?").strong());
            ui.horizontal(|ui| {
                ok = ui.add_enabled(!locs.is_empty(), egui::Button::new("Use these columns").min_size(Vec2::new(190.0, 36.0))).clicked();
                cancel = ui.add(egui::Button::new("Cancel").min_size(Vec2::new(110.0, 36.0))).clicked();
            });
        });
        if table.sheet != before_sheet {
            let crs = mapping.crs.clone();
            mapping = gd::guess_mapping(table.grid());
            if mapping.crs.kind == crs.kind {
                mapping.crs = crs;
            }
        }
        if ok {
            let g = &mut self.gis;
            g.table = Some(table);
            g.mapping = mapping;
            g.locs = locs;
            g.table_ver += 1;
            g.fit_pending = true;
            g.table_err.clear();
            g.win_table = false;
            // on to linking the specimens to the rows
            g.loc.win = true;
            g.loc.stale = true;
            self.win_serial[super::W_GIS_LOC] += 1;
        } else if cancel || !open {
            self.gis.win_table = false;
        } else {
            self.gis.draft = Some((table, mapping));
        }
    }
}
