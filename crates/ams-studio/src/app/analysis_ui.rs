//! The Group Statistics and Contour Plot windows. Layout and colours follow the original
//! (K1 pink, K2 green, K3 lavender, shape parameters cyan); sizes are about twice as large.

use super::tools_ui::{big_btn, tint};
use super::{App, W_CONTOUR, W_GROUP};
use crate::icons::Icon;
use ams_core::coords::CoordSys;
use ams_core::density::{self, DensityGrid};
use ams_core::stats::{self, Ellipse, GroupStats, PARAM_NAMES};
use ams_core::tensor::{dec_inc, from_dec_inc, Vec3};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2};

const KNAMES: [&str; 3] = ["K1", "K2", "K3"];

#[derive(Clone)]
pub struct GroupUi {
    k: [bool; 3],
    data: bool,
    mean: bool,
    conf: bool,
    planes: [bool; 3],
    boot: bool,
    boot_conf: bool,
    caption: bool,
    legend: bool,
}

impl Default for GroupUi {
    fn default() -> Self {
        GroupUi { k: [true; 3], data: true, mean: true, conf: true, planes: [false; 3], boot: true, boot_conf: true, caption: true, legend: true }
    }
}

struct ContourCalc {
    stamp: u64,
    axes: Vec<Vec3>,
    grid: DensityGrid,
    peak: f64,
    eig: ([f64; 3], [Vec3; 3], [(f64, f64); 3]),
    tex: Option<(bool, TextureHandle)>,
}

pub struct ContourUi {
    input: usize,
    discrete: bool,
    smooth: i32,
    contours: bool,
    points: bool,
    eigen: bool,
    serial: u64,
    calc: Option<ContourCalc>,
}

impl Default for ContourUi {
    fn default() -> Self {
        ContourUi { input: 0, discrete: false, smooth: 64, contours: false, points: false, eigen: false, serial: 0, calc: None }
    }
}

#[derive(Default)]
pub struct AnaState {
    group: GroupUi,
    contour: ContourUi,
}

// ------------------------------------------------------------------------ drawing helpers

fn bg(dark: bool, a: usize) -> Color32 {
    match a {
        0 => tint(dark, 255, 188, 188),
        1 => tint(dark, 190, 250, 190),
        _ => tint(dark, 196, 196, 255),
    }
}
fn cyan(dark: bool) -> Color32 {
    tint(dark, 190, 250, 250)
}

pub(super) fn net_pos(c: Pos2, r: f32, v: Vec3) -> Pos2 {
    let (d, i) = dec_inc(v);
    let (x, y) = density::project(d, i);
    Pos2::new(c.x + x as f32 * r, c.y - y as f32 * r)
}

pub(super) fn k_marker(pa: &egui::Painter, p: Pos2, a: usize, size: f32, fill: Color32, outline: Color32, w: f32) {
    let st = Stroke::new(w, outline);
    match a {
        0 => {
            let r = Rect::from_center_size(p, Vec2::splat(size * 1.7));
            pa.rect_filled(r, 0.0, fill);
            pa.rect_stroke(r, 0.0, st, StrokeKind::Middle);
        }
        1 => {
            pa.add(Shape::convex_polygon(vec![Pos2::new(p.x, p.y - size * 1.1), Pos2::new(p.x + size, p.y + size * 0.8), Pos2::new(p.x - size, p.y + size * 0.8)], fill, st));
        }
        _ => {
            pa.circle_filled(p, size, fill);
            pa.circle_stroke(p, size, st);
        }
    }
}

/// Draw a polyline, breaking it where consecutive points jump (a curve leaving and re-entering the net).
fn polyline(pa: &egui::Painter, pts: &[Pos2], stroke: Stroke, brk: f32) {
    let mut cur: Vec<Pos2> = vec![];
    for p in pts {
        if let Some(l) = cur.last() {
            if l.distance(*p) > brk {
                if cur.len() > 1 {
                    pa.add(Shape::line(std::mem::take(&mut cur), egui::epaint::PathStroke::new(stroke.width, stroke.color)));
                } else {
                    cur.clear();
                }
            }
        }
        cur.push(*p);
    }
    if cur.len() > 1 {
        pa.add(Shape::line(cur, egui::epaint::PathStroke::new(stroke.width, stroke.color)));
    }
}

pub(super) fn outline(pa: &egui::Painter, c: Pos2, r: f32, vs: &[Vec3], stroke: Stroke) {
    let pts: Vec<Pos2> = vs.iter().map(|v| net_pos(c, r, *v)).collect();
    polyline(pa, &pts, stroke, r * 0.6);
}

/// Great circle perpendicular to `v`.
pub(super) fn plane_of(v: Vec3) -> Vec<Vec3> {
    let h = if v[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let a = ams_core::tensor::cross(v, h);
    let na = ams_core::tensor::norm(a);
    let a = [a[0] / na, a[1] / na, a[2] / na];
    let b = ams_core::tensor::cross(v, a);
    (0..=180)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / 180.0;
            let (s, c) = t.sin_cos();
            [a[0] * c + b[0] * s, a[1] * c + b[1] * s, a[2] * c + b[2] * s]
        })
        .collect()
}

struct NetLabels {
    top: &'static str,
    right: &'static str,
    bottom: &'static str,
    left: &'static str,
    sys: &'static str,
}

fn labels(cs: CoordSys) -> NetLabels {
    match cs {
        CoordSys::Specimen => NetLabels { top: "X", right: "Y", bottom: "-X", left: "-Y", sys: "SPEC" },
        CoordSys::Geographic => NetLabels { top: "N", right: "90", bottom: "180", left: "270", sys: "GEO" },
        CoordSys::Paleo => NetLabels { top: "N", right: "90", bottom: "180", left: "270", sys: "PALEO" },
        CoordSys::Tectonic => NetLabels { top: "N", right: "90", bottom: "180", left: "270", sys: "TECTO" },
    }
}

fn draw_net(pa: &egui::Painter, c: Pos2, r: f32, ink: Color32, lb: &NetLabels) {
    pa.circle_stroke(c, r, Stroke::new(1.4, ink));
    for deg in (10..90).step_by(10) {
        let (x, _) = density::project(90.0, deg as f64);
        let rr = r * x as f32;
        for s in [-1.0f32, 1.0] {
            for (dx, dy) in [(1.0f32, 0.0f32), (0.0, 1.0)] {
                let q = Pos2::new(c.x + s * dx * rr, c.y - s * dy * rr);
                pa.line_segment([Pos2::new(q.x - dy * 3.5, q.y - dx * 3.5), Pos2::new(q.x + dy * 3.5, q.y + dx * 3.5)], Stroke::new(1.0, ink));
            }
        }
    }
    pa.line_segment([Pos2::new(c.x - 5.0, c.y), Pos2::new(c.x + 5.0, c.y)], Stroke::new(1.0, ink));
    pa.line_segment([Pos2::new(c.x, c.y - 5.0), Pos2::new(c.x, c.y + 5.0)], Stroke::new(1.0, ink));
    let f = FontId::proportional(13.0);
    pa.text(Pos2::new(c.x, c.y - r - 4.0), Align2::CENTER_BOTTOM, lb.top, f.clone(), ink);
    pa.text(Pos2::new(c.x + r + 6.0, c.y), Align2::LEFT_CENTER, lb.right, f.clone(), ink);
    pa.text(Pos2::new(c.x, c.y + r + 4.0), Align2::CENTER_TOP, lb.bottom, f.clone(), ink);
    pa.text(Pos2::new(c.x - r - 6.0, c.y), Align2::RIGHT_CENTER, lb.left, f, ink);
}

fn sci(x: f64) -> String {
    if !x.is_finite() {
        return "-".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    let e = x.abs().log10().floor() as i32;
    let m = x / 10f64.powi(e);
    format!("{m:.3}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

fn num(x: f64, d: usize) -> String {
    if x.is_finite() { format!("{x:.d$}") } else { "-".into() }
}

fn eig_txt(x: f64) -> String {
    if x.abs() >= 0.01 { format!("{x:.5}") } else { sci(x) }
}

fn cell(ui: &mut Ui, text: &str, fill: Color32, w: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::hover());
    ui.painter().rect(rect, 3.0, fill, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
    let ink = if ui.visuals().dark_mode { Color32::from_gray(238) } else { Color32::from_gray(20) };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.5), ink);
}

fn gap(ui: &mut Ui, w: f32) {
    ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::hover());
}

fn name_cell(ui: &mut Ui, text: &str, w: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::hover());
    ui.painter().text(Pos2::new(rect.min.x + 2.0, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(14.5), ui.visuals().strong_text_color());
}

fn head(ui: &mut Ui, text: &str, w: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::hover());
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.5), ui.visuals().strong_text_color());
}

fn kcheck(ui: &mut Ui, v: &mut bool, label: &str, fill: Color32, w: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 24.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, fill);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(4.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.checkbox(v, label);
    });
}

fn titled<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.group(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).strong().color(Color32::from_rgb(60, 80, 230)));
            add(ui)
        })
        .inner
    })
    .inner
}

// ------------------------------------------------------------------------ App glue

impl App {
    /// Indices of the specimens an analysis window works on: the active ones (or only the selected ones
    /// when the toolbar filter says so), exactly like the plots.
    fn analysis_idx(&self) -> Vec<usize> {
        let Some(d) = &self.doc else { return vec![] };
        if self.derived.len() != d.specs.len() {
            return vec![];
        }
        (0..d.specs.len()).filter(|&i| d.active[i] && (self.filter == 0 || d.selected[i])).collect()
    }

    // ------------------------------------------------------------------ Group Statistics
    pub(super) fn group_window(&mut self, ctx: &egui::Context) {
        if !self.win_group {
            return;
        }
        let dark = self.dark;
        let idx = self.analysis_idx();
        let tensors: Vec<_> = idx.iter().map(|&i| self.derived[i].tensor).collect();
        let kms: Vec<f64> = idx.iter().map(|&i| self.derived[i].km).collect();
        let gs: Option<GroupStats> = stats::group_stats(&tensors, &kms);
        let data_pts: Vec<[(f64, f64); 3]> = idx.iter().map(|&i| self.derived[i].axes.dec_inc).collect();
        let lb = labels(self.coord);
        let kc = self.k_colors;
        let name = self.doc.as_ref().map(|d| d.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()).unwrap_or_default();
        let suffix = if self.out_phase { "oop" } else { "ip" };
        let mut g = self.ana.group.clone();
        let mut open = true;
        let mut close = false;
        let mut msg: Option<String> = None;
        let ink = if dark { Color32::from_gray(225) } else { Color32::from_gray(25) };
        self.base_window(ctx, "Group Statistics", W_GROUP, Vec2::new(1080.0, 740.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            let n = idx.len();
            ui.horizontal_top(|ui| {
                // ------------------------------------------------ left: net and display options
                ui.vertical(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(500.0), Sense::hover());
                    let pa = ui.painter_at(rect.expand(30.0));
                    pa.rect_filled(rect, 2.0, if dark { Color32::from_gray(24) } else { Color32::WHITE });
                    let (c, r) = (rect.center(), 218.0);
                    draw_net(&pa, c, r, ink, &lb);
                    if g.caption {
                        pa.text(Pos2::new(rect.min.x + 12.0, rect.min.y + 14.0), Align2::LEFT_TOP, lb.sys, FontId::proportional(14.0), ink);
                        pa.text(Pos2::new(rect.max.x - 12.0, rect.min.y + 14.0), Align2::RIGHT_TOP, format!("Equal-Area\nProjection\nN = {n}"), FontId::proportional(14.0), ink);
                    }
                    if g.legend {
                        for (a, nm) in KNAMES.iter().enumerate() {
                            let y = rect.max.y - 56.0 + a as f32 * 18.0;
                            k_marker(&pa, Pos2::new(rect.min.x + 22.0, y), a, 5.0, kc[a], Color32::BLACK, 0.8);
                            pa.text(Pos2::new(rect.min.x + 38.0, y), Align2::LEFT_CENTER, *nm, FontId::proportional(13.5), ink);
                        }
                    }
                    let grey = if dark { Color32::from_gray(150) } else { Color32::from_gray(128) };
                    if g.data {
                        for p in &data_pts {
                            for a in 0..3 {
                                if g.k[a] {
                                    let (x, y) = density::project(p[a].0, p[a].1);
                                    k_marker(&pa, Pos2::new(c.x + x as f32 * r, c.y - y as f32 * r), a, 3.4, grey, Color32::from_gray(45), 0.8);
                                }
                            }
                        }
                    }
                    if let Some(s) = &gs {
                        if let (true, Some(b)) = (g.boot, &s.bootstrap) {
                            for a in 0..3 {
                                if g.k[a] {
                                    for v in &b.points[a] {
                                        let p = net_pos(c, r, *v);
                                        pa.circle_filled(p, 1.1, kc[a]);
                                    }
                                }
                            }
                            if g.boot_conf {
                                for a in 0..3 {
                                    if let (true, Some(e)) = (g.k[a], &b.ellipse[a]) {
                                        outline(&pa, c, r, &e.outline(&s.vecs, 120), Stroke::new(1.3, kc[a].gamma_multiply(0.75)));
                                    }
                                }
                            }
                        }
                        if g.mean {
                            for a in 0..3 {
                                if !g.planes[a] {
                                    continue;
                                }
                                // K2-K3 plane (perpendicular to K1), K1-K3 plane, K1-K2 plane
                                outline(&pa, c, r, &plane_of(s.vecs[a]), Stroke::new(1.0, kc[a]));
                            }
                            if g.conf {
                                for a in 0..3 {
                                    if let (true, Some(e)) = (g.k[a], &s.jelinek[a]) {
                                        outline(&pa, c, r, &e.outline(&s.vecs, 120), Stroke::new(2.4, kc[a]));
                                    }
                                }
                            }
                            for a in 0..3 {
                                if g.k[a] {
                                    k_marker(&pa, net_pos(c, r, s.vecs[a]), a, 7.5, kc[a], Color32::BLACK, 1.6);
                                }
                            }
                        }
                    }
                    ui.add_space(6.0);
                    titled(ui, "Display", |ui| {
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                for a in 0..3 {
                                    kcheck(ui, &mut g.k[a], KNAMES[a], bg(dark, a), 96.0);
                                }
                                ui.checkbox(&mut g.caption, "Caption");
                                ui.checkbox(&mut g.legend, "Legend");
                            });
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.checkbox(&mut g.data, "Data points");
                                ui.checkbox(&mut g.mean, "Mean tensor");
                                ui.add_enabled_ui(g.mean, |ui| {
                                    ui.indent("mt", |ui| {
                                        ui.checkbox(&mut g.conf, "Conf. ellipses");
                                        kcheck(ui, &mut g.planes[0], "K2-K3 plane", bg(dark, 0), 130.0);
                                        kcheck(ui, &mut g.planes[1], "K1-K3 plane", bg(dark, 1), 130.0);
                                        kcheck(ui, &mut g.planes[2], "K1-K2 plane", bg(dark, 2), 130.0);
                                    });
                                });
                            });
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.checkbox(&mut g.boot, "Bootstrap");
                                ui.add_enabled_ui(g.boot, |ui| {
                                    ui.indent("bt", |ui| {
                                        ui.checkbox(&mut g.boot_conf, "Conf. ellipses");
                                    });
                                });
                            });
                        });
                    });
                });
                ui.add_space(14.0);
                // ------------------------------------------------ right: tables
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("N = {n}")).strong().size(16.0));
                    ui.add_space(4.0);
                    let Some(s) = &gs else {
                        ui.label("No specimens to analyse.");
                        return;
                    };
                    const W: [f32; 10] = [40.0, 96.0, 62.0, 62.0, 14.0, 62.0, 62.0, 14.0, 62.0, 62.0];
                    let row = |ui: &mut Ui, f: &mut dyn FnMut(&mut Ui)| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            f(ui);
                        });
                    };
                    ui.spacing_mut().item_spacing.y = 3.0;
                    row(ui, &mut |ui| {
                        gap(ui, W[0]);
                        head(ui, "Mean Tensor", W[1] + W[2] + W[3] + 4.0);
                        gap(ui, W[4]);
                        head(ui, "Confidence Ellipses", W[5] + W[6] + W[7] + W[8] + W[9] + 8.0);
                    });
                    row(ui, &mut |ui| {
                        gap(ui, W[0]);
                        head(ui, "Normed Semi-Axes", W[1]);
                        head(ui, "Dec", W[2]);
                        head(ui, "Inc", W[3]);
                        gap(ui, W[4]);
                        head(ui, "Jelinek", W[5] + W[6] + 2.0);
                        gap(ui, W[7]);
                        head(ui, "Bootstrap", W[8] + W[9] + 2.0);
                    });
                    for a in 0..3 {
                        row(ui, &mut |ui| {
                            name_cell(ui, KNAMES[a], W[0]);
                            cell(ui, &eig_txt(s.k[a]), bg(dark, a), W[1]);
                            cell(ui, &num(s.dec_inc[a].0, 1), bg(dark, a), W[2]);
                            cell(ui, &num(s.dec_inc[a].1, 1), bg(dark, a), W[3]);
                            gap(ui, W[4]);
                            let j = s.jelinek[a].map(|e| e.semi_deg());
                            cell(ui, &j.map_or("n/a".into(), |v| num(v[0], 1)), bg(dark, a), W[5]);
                            cell(ui, &j.map_or("n/a".into(), |v| num(v[1], 1)), bg(dark, a), W[6]);
                            gap(ui, W[7]);
                            let b: Option<[f64; 2]> = s.bootstrap.as_ref().and_then(|b| b.ellipse[a]).map(|e: Ellipse| e.semi_deg());
                            cell(ui, &b.map_or("n/a".into(), |v| num(v[0], 1)), bg(dark, a), W[8]);
                            cell(ui, &b.map_or("n/a".into(), |v| num(v[1], 1)), bg(dark, a), W[9]);
                        });
                    }
                    ui.add_space(10.0);
                    row(ui, &mut |ui| {
                        gap(ui, W[0]);
                        head(ui, "Mean Tensor", 112.0);
                        head(ui, "Average", 112.0);
                        head(ui, "SD", 112.0);
                    });
                    let white = if dark { Color32::from_gray(50) } else { Color32::WHITE };
                    row(ui, &mut |ui| {
                        name_cell(ui, "Km", W[0]);
                        cell(ui, &sci(s.km.0), white, 112.0);
                        cell(ui, &sci(s.km.0), white, 112.0);
                        cell(ui, &sci(s.km.1), white, 112.0);
                    });
                    let mv = s.shape.map(|sh| stats::shape_values(&sh));
                    for (p, nm) in PARAM_NAMES.iter().enumerate() {
                        row(ui, &mut |ui| {
                            name_cell(ui, nm, W[0]);
                            cell(ui, &mv.map_or("-".into(), |v| num(v[p], 3)), cyan(dark), 112.0);
                            cell(ui, &num(s.avg[p].0, 3), cyan(dark), 112.0);
                            cell(ui, &num(s.avg[p].1, 3), cyan(dark), 112.0);
                        });
                    }
                    ui.label(RichText::new("SD = standard deviation over the specimens (the original calls it SEM).").small().weak());
                    ui.label(RichText::new("Bootstrap: 500 resamples, 95 % ellipse; this program's own method, so it differs from the original.").small().weak());
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if big_btn(ui, Icon::Save, "Export Data", true, dark) {
                            if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name(format!("{name}_groupstats_{suffix}.csv")).save_file() {
                                let text = group_csv(s, n);
                                msg = Some(match std::fs::write(&p, text) {
                                    Ok(()) => format!("Saved {}", p.display()),
                                    Err(e) => format!("Could not save: {e}"),
                                });
                            }
                        }
                        close = big_btn(ui, Icon::CloseRed, "Close", true, dark);
                    });
                });
            });
        });
        self.ana.group = g;
        self.win_group = open && !close;
        if let Some(m) = msg {
            self.info = Some(m);
        }
    }

    // ------------------------------------------------------------------ Contour Plot
    pub(super) fn contour_window(&mut self, ctx: &egui::Context) {
        if !self.win_contour {
            return;
        }
        let dark = self.dark;
        let idx = self.analysis_idx();
        let lb = labels(self.coord);
        let kc = self.k_colors;
        let th_canvas = if dark { Color32::from_gray(24) } else { Color32::WHITE };
        let ink = if dark { Color32::from_gray(225) } else { Color32::from_gray(25) };
        let serial = self.win_serial[W_CONTOUR];
        let mut ui_state = std::mem::take(&mut self.ana.contour);
        // (re)calculate when the window was just opened, or what it depends on changed
        let axis = ui_state.input.min(2);
        let stamp = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (idx.clone(), self.out_phase, self.coord as u8, axis, ui_state.smooth, serial).hash(&mut h);
            h.finish()
        };
        let mut recalc = ui_state.calc.as_ref().map_or(true, |c| c.stamp != stamp);
        let mut open = true;
        let mut close = false;
        let mut copy_graphics: Option<Rect> = None;
        let mut copy_data = false;
        let (derived, _) = (self.derived.clone(), 0);
        let win = self.base_window(ctx, "Contour Plot", W_CONTOUR, Vec2::new(900.0, 800.0));
        win.open(&mut open).resizable(true).show(ctx, |ui| {
            if recalc {
                let axes: Vec<Vec3> = idx.iter().map(|&i| { let (d, n) = derived[i].axes.dec_inc[axis]; from_dec_inc(d, n) }).collect();
                let k = ui_state.smooth.max(1) as f64;
                let grid = density::density(&axes, 260, k);
                let peak = if axes.is_empty() { 0.0 } else { density::peak(&axes, k).max(grid.max) };
                let eig = density::orientation_eigen(&axes);
                ui_state.calc = Some(ContourCalc { stamp, axes, grid, peak, eig, tex: None });
                recalc = false;
            }
            let _ = recalc;
            ui.horizontal_top(|ui| {
                // ------------------------------------------------ left: net, scale bar
                ui.vertical(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(560.0), Sense::hover());
                    let pa = ui.painter_at(rect.expand(34.0));
                    pa.rect_filled(rect, 2.0, th_canvas);
                    let (c, r) = (rect.center(), 250.0);
                    let color = kc[axis];
                    if let Some(calc) = ui_state.calc.as_mut() {
                        let (_, levels) = density::levels_for(calc.peak);
                        let want = ui_state.discrete;
                        if calc.tex.as_ref().map_or(true, |(d, _)| *d != want) {
                            let n = calc.grid.n;
                            let mut px = vec![Color32::TRANSPARENT; n * n];
                            for (i, z) in calc.grid.z.iter().enumerate() {
                                if !z.is_finite() {
                                    continue;
                                }
                                let v = z.min(calc.peak);
                                let t = if calc.peak <= 0.0 { 0.0 } else if want { levels.iter().filter(|l| **l <= v).count() as f32 / levels.len().max(1) as f32 } else { (v / calc.peak) as f32 };
                                px[i] = lerp_c(th_canvas, color, t);
                            }
                            let img = egui::ColorImage::new([n, n], px);
                            calc.tex = Some((want, ctx.load_texture("contour", img, TextureOptions::LINEAR)));
                        }
                        if let Some((_, tex)) = &calc.tex {
                            pa.image(tex.id(), Rect::from_center_size(c, Vec2::splat(2.0 * r)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        }
                        draw_net(&pa, c, r, ink, &lb);
                        if ui_state.contours {
                            for l in &levels {
                                for s in density::contour_segments(&calc.grid, *l) {
                                    let p = |q: (f64, f64)| Pos2::new(c.x + q.0 as f32 * r, c.y - q.1 as f32 * r);
                                    pa.line_segment([p(s[0]), p(s[1])], Stroke::new(1.0, Color32::from_gray(if dark { 220 } else { 20 })));
                                }
                            }
                        }
                        if ui_state.eigen {
                            let ec = [Color32::from_rgb(235, 30, 30), Color32::from_rgb(30, 200, 50), Color32::from_rgb(40, 60, 235)];
                            for e in 0..3 {
                                outline(&pa, c, r, &plane_of(calc.eig.1[e]), Stroke::new(1.3, ec[e]));
                                pa.text(net_pos(c, r, calc.eig.1[e]) + Vec2::new(5.0, -5.0), Align2::LEFT_BOTTOM, (e + 1).to_string(), FontId::proportional(12.0), ec[e]);
                            }
                        }
                        if ui_state.points {
                            for v in &calc.axes {
                                k_marker(&pa, net_pos(c, r, *v), axis, 3.6, color, Color32::from_gray(30), 0.8);
                            }
                        }
                        pa.text(Pos2::new(rect.max.x - 14.0, rect.max.y - 34.0), Align2::RIGHT_BOTTOM, format!("N =  {}", calc.axes.len()), FontId::proportional(15.0), ink);
                        pa.text(Pos2::new(rect.max.x - 14.0, rect.max.y - 16.0), Align2::RIGHT_BOTTOM, format!("Smoothing =  {}", ui_state.smooth), FontId::proportional(12.5), ink);
                        // colour scale
                        ui.add_space(8.0);
                        let (bar, _) = ui.allocate_exact_size(Vec2::new(520.0, 22.0), Sense::hover());
                        let bar_r = Rect::from_min_size(bar.min + Vec2::new(18.0, 0.0), Vec2::new(470.0, 16.0));
                        let bp = ui.painter_at(bar.expand2(Vec2::new(30.0, 24.0)));
                        let (_, lv) = density::levels_for(calc.peak);
                        if ui_state.discrete {
                            let nb = lv.len() + 1;
                            for b in 0..nb {
                                let (x0, x1) = (bar_r.min.x + bar_r.width() * b as f32 / nb as f32, bar_r.min.x + bar_r.width() * (b + 1) as f32 / nb as f32);
                                let t = b as f32 / (nb - 1).max(1) as f32;
                                bp.rect_filled(Rect::from_min_max(Pos2::new(x0, bar_r.min.y), Pos2::new(x1, bar_r.max.y)), 0.0, lerp_c(th_canvas, color, t));
                            }
                        } else {
                            for s in 0..200 {
                                let (x0, x1) = (bar_r.min.x + bar_r.width() * s as f32 / 200.0, bar_r.min.x + bar_r.width() * (s + 1) as f32 / 200.0 + 0.6);
                                bp.rect_filled(Rect::from_min_max(Pos2::new(x0, bar_r.min.y), Pos2::new(x1, bar_r.max.y)), 0.0, lerp_c(th_canvas, color, s as f32 / 199.0));
                            }
                        }
                        bp.rect_stroke(bar_r, 0.0, Stroke::new(1.0, ink), StrokeKind::Outside);
                        let f = FontId::proportional(12.0);
                        bp.text(Pos2::new(bar_r.min.x, bar_r.max.y + 3.0), Align2::CENTER_TOP, "0", f.clone(), ink);
                        for l in &lv {
                            let x = if ui_state.discrete {
                                let i = lv.iter().position(|q| q == l).unwrap_or(0);
                                bar_r.min.x + bar_r.width() * (i as f32 + 1.0) / (lv.len() + 1) as f32
                            } else {
                                bar_r.min.x + bar_r.width() * (*l / calc.peak) as f32
                            };
                            bp.line_segment([Pos2::new(x, bar_r.max.y), Pos2::new(x, bar_r.max.y + 3.0)], Stroke::new(1.0, ink));
                            if x < bar_r.max.x - 40.0 {
                                bp.text(Pos2::new(x, bar_r.max.y + 3.0), Align2::CENTER_TOP, format!("{l:.0}"), f.clone(), ink);
                            }
                        }
                        bp.text(Pos2::new(bar_r.max.x + 4.0, bar_r.min.y - 1.0), Align2::LEFT_BOTTOM, "Max", f.clone(), ink);
                        bp.text(Pos2::new(bar_r.max.x, bar_r.max.y + 3.0), Align2::CENTER_TOP, format!("{:.1}", calc.peak), f, ink);
                        copy_graphics = Some(rect.union(bar_r.expand2(Vec2::new(24.0, 22.0))));
                    }
                });
                ui.add_space(12.0);
                // ------------------------------------------------ right: options
                ui.vertical(|ui| {
                    ui.set_width(210.0);
                    titled(ui, "Input", |ui| {
                        for a in 0..3 {
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(184.0, 26.0), Sense::hover());
                            ui.painter().rect_filled(rect, 3.0, bg(dark, a));
                            ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(4.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                                ui.radio_value(&mut ui_state.input, a, KNAMES[a]);
                            });
                        }
                        ui.add_space(4.0);
                        for (nm, bgc) in [("Foliation Pole", cyan(dark)), ("Lineation", tint(dark, 255, 190, 250))] {
                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(184.0, 26.0), Sense::hover());
                            ui.painter().rect_filled(rect, 3.0, bgc.gamma_multiply(0.6));
                            ui.painter().text(rect.min + Vec2::new(26.0, 13.0), Align2::LEFT_CENTER, nm, FontId::proportional(14.0), ui.visuals().weak_text_color());
                            resp.on_hover_text("Needs foliation / lineation values from a .ged file: not supported yet");
                        }
                        ui.add_space(4.0);
                        if ui.add_sized([184.0, 26.0], egui::Button::new("Refresh")).clicked() {
                            ui_state.calc = None;
                        }
                    });
                    titled(ui, "Levels", |ui| {
                        ui.radio_value(&mut ui_state.discrete, false, "Continuous");
                        ui.radio_value(&mut ui_state.discrete, true, "Discrete");
                        ui.add_space(4.0);
                        ui.label("Smoothing");
                        ui.add(egui::DragValue::new(&mut ui_state.smooth).range(1..=1000).speed(0.5));
                    });
                    titled(ui, "Show", |ui| {
                        ui.checkbox(&mut ui_state.contours, "Contours");
                        ui.checkbox(&mut ui_state.points, "Data Points");
                        ui.checkbox(&mut ui_state.eigen, "Eigenvectors");
                    });
                });
            });
            ui.add_space(6.0);
            // ------------------------------------------------ eigen table
            if let Some(calc) = &ui_state.calc {
                egui::Grid::new("ct-eig").spacing([2.0, 3.0]).show(ui, |ui| {
                    ui.label("");
                    for (h, w) in [("Eigenvalues", 130.0), ("EigenVec_Dec", 130.0), ("EigenVec_Inc", 130.0)] {
                        head(ui, h, w);
                    }
                    ui.end_row();
                    for e in 0..3 {
                        ui.label(RichText::new(format!("E{}", e + 1)).strong());
                        let w = Color32::from_gray(if dark { 50 } else { 255 });
                        cell(ui, &format!("{:.5}", calc.eig.0[e]), w, 130.0);
                        cell(ui, &num(calc.eig.2[e].0, 1), w, 130.0);
                        cell(ui, &num(calc.eig.2[e].1, 1), w, 130.0);
                        ui.end_row();
                    }
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if big_btn(ui, Icon::Calc, "Calculate", true, dark) {
                    ui_state.calc = None;
                }
                if big_btn(ui, Icon::Save, "Copy Data", ui_state.calc.is_some(), dark) {
                    copy_data = true;
                }
                if big_btn(ui, Icon::Camera, "Copy Graphics", ui_state.calc.is_some(), dark) {
                    if let Some(r) = copy_graphics {
                        self.tool_copy = Some(r);
                        self.request_shot(ctx);
                    }
                }
                close = big_btn(ui, Icon::CloseRed, "Close", true, dark);
            });
        });
        if copy_data {
            if let Some(c) = &ui_state.calc {
                let mut t = String::from("\tEigenvalues\tEigenVec_Dec\tEigenVec_Inc\n");
                for e in 0..3 {
                    t += &format!("E{}\t{:.5}\t{:.1}\t{:.1}\n", e + 1, c.eig.0[e], c.eig.2[e].0, c.eig.2[e].1);
                }
                t += &format!("\nDensity grid ({} x {}, % per 1 % area; rows from y = +1 to -1)\n", c.grid.n, c.grid.n);
                for row in c.grid.z.chunks(c.grid.n) {
                    t += &row.iter().map(|v| if v.is_finite() { format!("{v:.3}") } else { String::new() }).collect::<Vec<_>>().join("\t");
                    t.push('\n');
                }
                ctx.copy_text(t);
            }
        }
        ui_state.serial = serial;
        self.ana.contour = ui_state;
        self.win_contour = open && !close;
    }
}

fn lerp_c(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

fn group_csv(s: &GroupStats, n: usize) -> String {
    let mut t = format!("N,{n}\n\nMean tensor,eigenvalue,dec,inc,jelinek_semi1,jelinek_semi2,bootstrap_semi1,bootstrap_semi2\n");
    for a in 0..3 {
        let j = s.jelinek[a].map(|e| e.semi_deg());
        let b = s.bootstrap.as_ref().and_then(|b| b.ellipse[a]).map(|e| e.semi_deg());
        let f = |v: Option<[f64; 2]>, i: usize| v.map_or(String::new(), |x| format!("{:.2}", x[i]));
        t += &format!("{},{:.6e},{:.2},{:.2},{},{},{},{}\n", KNAMES[a], s.k[a], s.dec_inc[a].0, s.dec_inc[a].1, f(j, 0), f(j, 1), f(b, 0), f(b, 1));
    }
    t += "\nParameter,mean tensor,average,sd\n";
    t += &format!("Km,{:.6e},{:.6e},{:.6e}\n", s.km.0, s.km.0, s.km.1);
    let mv = s.shape.map(|sh| stats::shape_values(&sh));
    for (p, nm) in PARAM_NAMES.iter().enumerate() {
        t += &format!("{},{},{:.5},{:.5}\n", nm, mv.map_or(String::new(), |v| format!("{:.5}", v[p])), s.avg[p].0, s.avg[p].1);
    }
    t
}

/// What to draw on top of the main window's stereonet besides the data points.
#[derive(Clone, Default, PartialEq)]
pub struct StereoOverlay {
    pub mean: bool,
    pub conf: bool,
    pub planes: [bool; 3],
    pub boot: bool,
    pub boot_conf: bool,
}

pub(super) fn draw_stereo_overlay(pa: &egui::Painter, c: Pos2, r: f32, s: &GroupStats, o: &StereoOverlay, show: [bool; 3], kc: [Color32; 3]) {
    if o.boot {
        if let Some(b) = &s.bootstrap {
            for a in 0..3 {
                if !show[a] {
                    continue;
                }
                for v in &b.points[a] {
                    pa.circle_filled(net_pos(c, r, *v), 1.1, kc[a]);
                }
                if o.boot_conf {
                    if let Some(e) = &b.ellipse[a] {
                        outline(pa, c, r, &e.outline(&s.vecs, 120), Stroke::new(1.3, kc[a].gamma_multiply(0.75)));
                    }
                }
            }
        }
    }
    if o.mean {
        for a in 0..3 {
            if o.planes[a] {
                outline(pa, c, r, &plane_of(s.vecs[a]), Stroke::new(1.0, kc[a]));
            }
        }
        if o.conf {
            for a in 0..3 {
                if let (true, Some(e)) = (show[a], &s.jelinek[a]) {
                    outline(pa, c, r, &e.outline(&s.vecs, 120), Stroke::new(2.2, kc[a]));
                }
            }
        }
        for a in 0..3 {
            if show[a] {
                k_marker(pa, net_pos(c, r, s.vecs[a]), a, 7.0, kc[a], Color32::BLACK, 1.5);
            }
        }
    }
}
