//! The five windows of the Tools menu. Behaviour follows the original (see
//! `ams-core/src/tools.rs`); the look follows the rest of this app.

use super::App;
use crate::doc::Doc;
use crate::icons::{self, Icon};
use ams_core::ams::{Specimen, RECORD_LEN};
use ams_core::density::project;
use ams_core::ged::{GedFile, GedRow};
use ams_core::tensor::dec_inc;
use ams_core::tools::*;
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};

const T_GED: usize = 8;
const T_TENS: usize = 9;
const T_FAB: usize = 10;
const T_INTER: usize = 11;
const T_DEF: usize = 12;

pub const TOOL_NAMES: [&str; 5] = ["GED Editor", "Tensor Addition / Subtraction", "Fabric Overprint", "Intersection Toolbox", "Deform AMS"];

#[derive(Default)]
pub struct ToolsState {
    open: [bool; 5],
    ged: GedState,
    tens: TensState,
    fab: FabState,
    inter: InterState,
    def: DefState,
}

// ------------------------------------------------------------------ shared widgets

/// Scale the text, buttons and spacing of everything inside this `ui` (used to make popups bigger).
pub(super) fn pop_style(ui: &mut Ui, f: f32) {
    let s = ui.style_mut();
    for (_, id) in s.text_styles.iter_mut() {
        id.size *= f;
    }
    s.spacing.item_spacing *= f;
    s.spacing.button_padding *= f;
    s.spacing.interact_size *= f;
    s.spacing.icon_width *= f;
    s.spacing.icon_spacing *= f;
}

/// How much `pop_style` scaled this ui (1.0 = normal).
pub(super) fn zf(ui: &Ui) -> f32 {
    ui.style().text_styles.get(&egui::TextStyle::Body).map_or(1.0, |f| f.size / 14.0)
}

pub(super) fn big_btn(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, dark: bool) -> bool {
    let z = zf(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(104.0 * z, 62.0 * z), if enabled { Sense::click() } else { Sense::hover() });
    let v = ui.visuals();
    let fill = if resp.is_pointer_button_down_on() && enabled {
        v.widgets.active.bg_fill
    } else if resp.hovered() && enabled {
        v.widgets.hovered.bg_fill
    } else {
        v.widgets.inactive.bg_fill
    };
    ui.painter().rect(rect, 8.0, fill, Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
    let ir = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + 21.0 * z), Vec2::splat(26.0 * z));
    icons::paint(ui.painter(), ir, icon, dark);
    ui.painter().text(Pos2::new(rect.center().x, rect.max.y - 11.0 * z), Align2::CENTER_CENTER, label, FontId::proportional(12.5 * z), if enabled { v.text_color() } else { v.text_color().gamma_multiply(0.4) });
    if !enabled {
        ui.painter().rect_filled(rect, 8.0, v.panel_fill.gamma_multiply(0.45));
    }
    resp.clicked() && enabled
}

fn field(ui: &mut Ui, text: &mut String, width: f32, tint: Option<Color32>) -> bool {
    let mut te = egui::TextEdit::singleline(text).desired_width(width);
    if let Some(t) = tint {
        te = te.background_color(t);
    }
    ui.add(te).changed()
}

/// A text cell inside a Grid: the minimum width makes the column grow to `w`.
fn cell_edit(ui: &mut Ui, text: &mut String, w: f32, enabled: bool) {
    ui.scope(|ui| {
        ui.set_min_width(w);
        let edge = ui.visuals().widgets.noninteractive.bg_stroke.color;
        ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, edge);
        ui.add_enabled(enabled, egui::TextEdit::singleline(text).desired_width(w));
    });
}

pub(super) fn tint(dark: bool, r: u8, g: u8, b: u8) -> Color32 {
    let c = Color32::from_rgb(r, g, b);
    if dark { c.gamma_multiply(0.35) } else { c }
}

fn parse_f(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Simple read-only table with full-row selection; returns the clicked row index.
fn table(ui: &mut Ui, id: &str, widths: &[f32], head: &[&str], rows: &[(Vec<String>, bool, bool)], height: f32) -> Option<usize> {
    let total: f32 = widths.iter().sum();
    let mut clicked = None;
    let (hr, _) = ui.allocate_exact_size(Vec2::new(total.max(ui.available_width()), 22.0), Sense::hover());
    ui.painter().rect_filled(hr, 3.0, ui.visuals().faint_bg_color);
    let mut x = hr.min.x;
    for (h, w) in head.iter().zip(widths) {
        ui.painter().text(Pos2::new(x + 6.0, hr.center().y), Align2::LEFT_CENTER, *h, FontId::proportional(12.5), ui.visuals().strong_text_color());
        x += w;
    }
    egui::ScrollArea::vertical().id_salt(id).max_height(height).min_scrolled_height(height).auto_shrink([false, false]).show_rows(ui, super::ROW_H, rows.len(), |ui, range| {
        for i in range {
            let (cells, active, selected) = &rows[i];
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(total.max(ui.available_width()), super::ROW_H), Sense::click());
            super::paint_row(ui, rect, *selected, resp.hovered(), *active, false);
            let mut x = rect.min.x;
            for (c, w) in cells.iter().zip(widths) {
                super::row_text(ui, Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(*w, super::ROW_H)), 6.0, c, *active, false);
                x += w;
            }
            if resp.clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

fn draw_net(pa: &egui::Painter, c: Pos2, r: f32, ink: Color32, labels: bool) {
    pa.circle_stroke(c, r, Stroke::new(1.3, ink));
    for deg in (10..90).step_by(10) {
        let (x, _) = project(90.0, deg as f64);
        let rr = r * x as f32;
        for s in [-1.0f32, 1.0] {
            for (dx, dy) in [(1.0f32, 0.0f32), (0.0, 1.0)] {
                let q = Pos2::new(c.x + s * dx * rr, c.y - s * dy * rr);
                pa.line_segment([Pos2::new(q.x - dy * 3.0, q.y - dx * 3.0), Pos2::new(q.x + dy * 3.0, q.y + dx * 3.0)], Stroke::new(1.0, ink));
            }
        }
    }
    pa.line_segment([Pos2::new(c.x - 4.0, c.y), Pos2::new(c.x + 4.0, c.y)], Stroke::new(1.0, ink));
    pa.line_segment([Pos2::new(c.x, c.y - 4.0), Pos2::new(c.x, c.y + 4.0)], Stroke::new(1.0, ink));
    if labels {
        let f = FontId::proportional(11.0);
        pa.text(Pos2::new(c.x, c.y - r - 3.0), Align2::CENTER_BOTTOM, "X", f.clone(), ink);
        pa.text(Pos2::new(c.x + r + 4.0, c.y), Align2::LEFT_CENTER, "Y", f.clone(), ink);
        pa.text(Pos2::new(c.x, c.y + r + 3.0), Align2::CENTER_TOP, "-X", f.clone(), ink);
        pa.text(Pos2::new(c.x - r - 4.0, c.y), Align2::RIGHT_CENTER, "-Y", f, ink);
    }
}

fn to_screen(c: Pos2, r: f32, dec: f64, inc: f64) -> Pos2 {
    let (x, y) = project(dec, inc);
    Pos2::new(c.x + x as f32 * r, c.y - y as f32 * r)
}

// ====================================================================== GED Editor

const OP13: [u16; 4] = [3, 6, 9, 12];
const OP24: [u16; 2] = [0, 90];

#[derive(Clone)]
struct GRow {
    name: String,
    c: [String; 14],
    deleted: bool,
    selected: bool,
}

impl GRow {
    fn blank() -> GRow {
        GRow { name: String::new(), c: Default::default(), deleted: false, selected: false }
    }
    fn is_blank(&self) -> bool {
        self.name.trim().is_empty() && self.c.iter().all(|s| s.trim().is_empty())
    }
}

struct GedState {
    site: String,
    lat: String,
    lon: String,
    rock: [String; 4],
    op: [usize; 4],
    rows: Vec<GRow>,
    msg: String,
}

impl Default for GedState {
    fn default() -> Self {
        GedState { site: String::new(), lat: String::new(), lon: String::new(), rock: Default::default(), op: [1, 0, 1, 0], rows: vec![GRow::blank(); 10], msg: String::new() }
    }
}

fn numtxt(v: f64) -> String {
    if v == v.trunc() { format!("{}", v as i64) } else { format!("{v}") }
}

impl GedState {
    fn to_file(&self) -> GedFile {
        let ch = |s: &str| s.trim().chars().next().unwrap_or('0');
        let f = |s: &str| parse_f(s).unwrap_or(0.0);
        let rows = self
            .rows
            .iter()
            .filter(|r| !r.deleted && !r.is_blank())
            .map(|r| GedRow {
                name: r.name.trim().to_string(),
                sampling: [f(&r.c[0]), f(&r.c[1])],
                codes: [ch(&r.c[2]), ch(&r.c[5]), ch(&r.c[8]), ch(&r.c[11])],
                fol1: [f(&r.c[3]), f(&r.c[4])],
                lin1: [f(&r.c[6]), f(&r.c[7])],
                fol2: [f(&r.c[9]), f(&r.c[10])],
                lin2: [f(&r.c[12]), f(&r.c[13])],
            })
            .collect();
        GedFile {
            site: self.site.clone(),
            latitude: self.lat.clone(),
            longitude: self.lon.clone(),
            rock: self.rock.clone(),
            op: [OP13[self.op[0]], OP24[self.op[1]], OP13[self.op[2]], OP24[self.op[3]]],
            rows,
        }
    }

    fn load(&mut self, g: &GedFile) {
        self.site = g.site.clone();
        self.lat = g.latitude.clone();
        self.lon = g.longitude.clone();
        self.rock = g.rock.clone();
        let idx = |list: &[u16], v: u16, d: usize| list.iter().position(|x| *x == v).unwrap_or(d);
        self.op = [idx(&OP13, g.op[0], 1), idx(&OP24, g.op[1], 0), idx(&OP13, g.op[2], 1), idx(&OP24, g.op[3], 0)];
        self.rows = g
            .rows
            .iter()
            .map(|r| {
                let n = numtxt;
                let c = [
                    n(r.sampling[0]), n(r.sampling[1]),
                    r.codes[0].to_string(), n(r.fol1[0]), n(r.fol1[1]),
                    r.codes[1].to_string(), n(r.lin1[0]), n(r.lin1[1]),
                    r.codes[2].to_string(), n(r.fol2[0]), n(r.fol2[1]),
                    r.codes[3].to_string(), n(r.lin2[0]), n(r.lin2[1]),
                ];
                GRow { name: r.name.clone(), c, deleted: false, selected: false }
            })
            .collect();
        while self.rows.len() < 10 {
            self.rows.push(GRow::blank());
        }
    }
}

// ====================================================================== Tensor add / subtract

struct SetSt {
    label: String,
    raw: Vec<u8>,
    specs: Vec<Specimen>,
    dis: Vec<bool>,
    sel: Vec<bool>,
    field_f: Option<i64>,
    freq_f: Option<i64>,
}

impl SetSt {
    fn load(path: &std::path::Path) -> Result<SetSt, String> {
        let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let specs = ams_core::ams::parse(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
        let n = specs.len();
        Ok(SetSt { label: path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), raw, specs, dis: vec![false; n], sel: vec![false; n], field_f: None, freq_f: None })
    }
    fn active(&self, i: usize) -> bool {
        !self.dis[i] && self.field_f.map_or(true, |f| self.specs[i].field_am.round() as i64 == f) && self.freq_f.map_or(true, |f| self.specs[i].freq_hz.round() as i64 == f)
    }
    fn n_active(&self) -> usize {
        (0..self.specs.len()).filter(|&i| self.active(i)).count()
    }
    fn order(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.specs.len()).filter(|&i| self.active(i)).collect();
        v.extend((0..self.specs.len()).filter(|&i| !self.active(i)));
        v
    }
    fn uniques(&self, f: impl Fn(&Specimen) -> f64) -> Vec<i64> {
        let mut v: Vec<i64> = self.specs.iter().map(|s| f(s).round() as i64).collect();
        v.sort_unstable();
        v.dedup();
        v
    }
    fn record(&self, i: usize) -> &[u8] {
        &self.raw[i * RECORD_LEN..(i + 1) * RECORD_LEN]
    }
}

impl TensState {
    /// Pair the active records of A and B by identical name (A order).
    fn pair(&mut self) {
        let (Some(a), Some(b)) = (self.a.as_ref(), self.b.as_ref()) else { return };
        let ia: Vec<usize> = (0..a.specs.len()).filter(|&i| a.active(i)).collect();
        let ib: Vec<usize> = (0..b.specs.len()).filter(|&i| b.active(i)).collect();
        let na: Vec<String> = ia.iter().map(|&i| a.specs[i].name.clone()).collect();
        let nb: Vec<String> = ib.iter().map(|&i| b.specs[i].name.clone()).collect();
        self.pairs = pair_by_name(&na, &nb).into_iter().map(|(x, y)| (ia[x], ib[y])).collect();
        self.km = vec![None; self.pairs.len()];
        self.sign = None;
    }

    /// Add (`sign` = 1) or subtract (-1); pairs first when that has not been done.
    fn calc(&mut self, sign: f64) {
        if self.pairs.is_empty() {
            self.pair();
        }
        let (Some(a), Some(b)) = (self.a.as_ref(), self.b.as_ref()) else { return };
        self.km = self.pairs.iter().map(|(i, j)| Some(combine_phase(&a.specs[*i].inphase, &b.specs[*j].inphase, sign).0)).collect();
        self.sign = Some(sign);
    }

    /// The combined records as an .ams file, or None before Add / Subtract.
    fn result_bytes(&self) -> Option<Vec<u8>> {
        let (a, b, sign) = (self.a.as_ref()?, self.b.as_ref()?, self.sign?);
        let mut out = vec![];
        for (i, j) in &self.pairs {
            out.extend(combine_records(a.record(*i), b.record(*j), sign));
        }
        Some(out)
    }
}

#[derive(Default)]
struct TensState {
    a: Option<SetSt>,
    b: Option<SetSt>,
    pairs: Vec<(usize, usize)>,
    km: Vec<Option<f64>>,
    sign: Option<f64>,
    msg: String,
}

// ====================================================================== Fabric overprint

#[derive(Clone, Default)]
struct FabStr {
    kmean: String,
    p: String,
    t: String,
    kmax: [String; 2],
    kmin: [String; 2],
}

impl FabStr {
    fn parse(&self) -> Option<FabricIn> {
        Some(FabricIn {
            kmean: parse_f(&self.kmean)?,
            p: parse_f(&self.p)?,
            t: parse_f(&self.t)?,
            kmax: (parse_f(&self.kmax[0])?, parse_f(&self.kmax[1])?),
            kmin: (parse_f(&self.kmin[0])?, parse_f(&self.kmin[1])?),
        })
    }
    fn from(f: &FabricIn) -> FabStr {
        let r = |v: f64, d: usize| format!("{v:.d$}");
        FabStr { kmean: format!("{:.6}", f.kmean), p: r(f.p, 4), t: r(f.t, 4), kmax: [r(f.kmax.0, 1), r(f.kmax.1, 1)], kmin: [r(f.kmin.0, 1), r(f.kmin.1, 1)] }
    }
}

struct FabState {
    a: FabStr,
    b: FabStr,
    inc: usize,
    msg: String,
}

impl Default for FabState {
    fn default() -> Self {
        FabState { a: FabStr::default(), b: FabStr::default(), inc: 1, msg: String::new() }
    }
}

const INCS: [usize; 4] = [11, 101, 1001, 10001];

// ====================================================================== Intersection toolbox

#[derive(Default)]
struct InterState {
    foliations: bool,
    a: [String; 2],
    b: [String; 2],
    result: Option<Intersection>,
    inputs: Option<((f64, f64), (f64, f64))>,
    msg: String,
}

// ====================================================================== Deform AMS

struct DefState {
    oblate: bool,
    p: String,
    kind: usize,
    axis: usize,
    stretch: String,
    gamma: String,
    model: Deform,
    matrix: Option<ams_core::tensor::Mat3>,
    msg: String,
}

impl Default for DefState {
    fn default() -> Self {
        DefState { oblate: true, p: "1.3".into(), kind: 0, axis: 0, stretch: "2".into(), gamma: "0.5".into(), model: Deform::new(true, 1.3), matrix: None, msg: String::new() }
    }
}

// ====================================================================== App glue

impl App {
    pub(super) fn open_tool(&mut self, name: &str) {
        if let Some(k) = TOOL_NAMES.iter().position(|n| *n == name) {
            self.tools.open[k] = true;
            self.win_serial[T_GED + k] += 1;
        }
    }

    pub(super) fn tool_windows(&mut self, ctx: &egui::Context) {
        if self.tools.open[0] {
            self.ged_window(ctx);
        }
        if self.tools.open[1] {
            self.tens_window(ctx);
        }
        if self.tools.open[2] {
            self.fab_window(ctx);
        }
        if self.tools.open[3] {
            self.inter_window(ctx);
        }
        if self.tools.open[4] {
            self.def_window(ctx);
        }
    }

    // ------------------------------------------------------------------ GED
    fn ged_window(&mut self, ctx: &egui::Context) {
        let dark = self.dark;
        let mut open = true;
        let mut st = std::mem::take(&mut self.tools.ged);
        let mut close = false;
        self.base_window(ctx, "GED Editor", T_GED, Vec2::new(1240.0, 720.0)).open(&mut open).show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Sampling Site").strong().color(Color32::from_rgb(60, 80, 220)));
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| { ui.label("Site Name"); field(ui, &mut st.site, 190.0, None); });
                            ui.vertical(|ui| { ui.label("Latitude"); field(ui, &mut st.lat, 70.0, None); });
                            ui.vertical(|ui| { ui.label("Longitude"); field(ui, &mut st.lon, 70.0, None); });
                        });
                    });
                });
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Rock").strong().color(Color32::from_rgb(60, 80, 220)));
                        ui.horizontal(|ui| {
                            for (i, n) in ["Type", "Strat", "Litho", "Region"].iter().enumerate() {
                                ui.vertical(|ui| {
                                    ui.label(*n);
                                    field(ui, &mut st.rock[i], 48.0, None);
                                    st.rock[i] = st.rock[i].chars().take(4).collect();
                                });
                            }
                        });
                    });
                });
                ui.group(|ui| {
                  ui.vertical(|ui| {
                    ui.label(RichText::new("Orientation Parameters").strong().color(Color32::from_rgb(60, 80, 220)));
                    ui.horizontal(|ui| {
                        let tips = [
                            "Clock value of orientation of the arrow drawn on the frontal side of specimen (X axis of the specimen)",
                            "0 - Dip of the frontal side is measured; 90 - Plunge of the cylinder axis is measured",
                            "Clock value (second orientation parameter)",
                            "0 - Dip direction and Dip; 90 - Strike (right-hand rule) and Dip",
                        ];
                        for k in 0..4 {
                            ui.vertical(|ui| {
                                ui.label(format!("OP{}", k + 1));
                                let list: &[u16] = if k % 2 == 0 { &OP13 } else { &OP24 };
                                egui::ComboBox::from_id_salt(("ged-op", k)).width(48.0).selected_text(list[st.op[k]].to_string()).show_ui(ui, |ui| {
                                    for (i, v) in list.iter().enumerate() {
                                        ui.selectable_value(&mut st.op[k], i, v.to_string());
                                    }
                                }).response.on_hover_text(tips[k]);
                            });
                        }
                    });
                  });
                });
            });
            ui.add_space(6.0);
            const W: [f32; 16] = [30.0, 128.0, 44.0, 44.0, 32.0, 44.0, 44.0, 32.0, 48.0, 50.0, 32.0, 44.0, 44.0, 32.0, 48.0, 50.0];
            egui::ScrollArea::both().auto_shrink([false, false]).max_height(ui.available_height() - 84.0).show(ui, |ui| {
                egui::Grid::new("ged-grid").striped(true).spacing([4.0, 3.0]).show(ui, |ui| {
                    ui.label("");
                    ui.label("");
                    ui.label(RichText::new("Sampling Angles").strong());
                    ui.label("");
                    for t in ["Foliation #1", "", "", "Lineation #1", "", "", "Foliation #2", "", "", "Lineation #2", "", ""] {
                        ui.label(RichText::new(t).strong());
                    }
                    ui.end_row();
                    for h in ["#", "Specimen", "Dec", "Inc", "Code", "Dec", "Dip", "Code", "Trend", "Plunge", "Code", "Dec", "Dip", "Code", "Trend", "Plunge"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (ri, row) in st.rows.iter_mut().enumerate() {
                        let faded = row.deleted;
                        let t = RichText::new((ri + 1).to_string());
                        let t = if faded { t.strikethrough().weak() } else { t };
                        if ui.add_sized([W[0], 20.0], egui::Button::selectable(row.selected, t)).clicked() {
                            row.selected = !row.selected;
                        }
                        cell_edit(ui, &mut row.name, W[1], !faded);
                        for (ci, cell) in row.c.iter_mut().enumerate() {
                            cell_edit(ui, cell, W[ci + 2], !faded);
                        }
                        ui.end_row();
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if big_btn(ui, Icon::Open, "Open", true, dark) {
                    if let Some(p) = rfd::FileDialog::new().add_filter("GED files", &["ged"]).set_title("Open GED File").pick_file() {
                        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| GedFile::parse(&t)) {
                            Ok(g) => {
                                st.load(&g);
                                st.msg.clear();
                            }
                            Err(e) => st.msg = format!("Could not open {}: {e}", p.display()),
                        }
                    }
                }
                ui.add_space(70.0);
                if big_btn(ui, Icon::Eraser, "Clear Table", true, dark) {
                    st.rows = vec![GRow::blank(); 10];
                }
                if big_btn(ui, Icon::AddRow, "Add Row", true, dark) {
                    st.rows.push(GRow::blank());
                }
                let any = st.rows.iter().any(|r| r.selected);
                if big_btn(ui, Icon::DelRow, "Delete Row(s)", any, dark) {
                    for r in st.rows.iter_mut().filter(|r| r.selected) {
                        r.deleted = true;
                        r.selected = false;
                    }
                }
                if big_btn(ui, Icon::Restore, "Restore Row(s)", st.rows.iter().any(|r| r.deleted), dark) {
                    let sel = st.rows.iter().any(|r| r.selected && r.deleted);
                    for r in st.rows.iter_mut() {
                        if r.deleted && (r.selected || !sel) {
                            r.deleted = false;
                            r.selected = false;
                        }
                    }
                }
                ui.add_space(70.0);
                if big_btn(ui, Icon::Save, "Save As", true, dark) {
                    if let Some(p) = rfd::FileDialog::new().add_filter("GED files", &["ged"]).set_title("Save GED File").save_file() {
                        match std::fs::write(&p, st.to_file().to_text()) {
                            Ok(()) => st.msg = format!("Saved {}", p.display()),
                            Err(e) => st.msg = format!("Could not save: {e}"),
                        }
                    }
                }
                close = big_btn(ui, Icon::CloseRed, "Close", true, dark);
            });
            if !st.msg.is_empty() {
                ui.label(RichText::new(&st.msg).small().weak());
            }
        });
        self.tools.ged = st;
        self.tools.open[0] = open && !close;
    }

    // ------------------------------------------------------------------ Tensor addition / subtraction
    fn tens_window(&mut self, ctx: &egui::Context) {
        let dark = self.dark;
        let mut open = true;
        let mut st = std::mem::take(&mut self.tools.tens);
        let mut close = false;
        self.base_window(ctx, "Tensor Addition / Subtraction", T_TENS, Vec2::new(1240.0, 780.0)).open(&mut open).show(ctx, |ui| {
            ui.columns(3, |cols| {
                for (k, ui) in cols.iter_mut().enumerate().take(2) {
                    let tag = if k == 0 { "A" } else { "B" };
                    let slot = if k == 0 { &mut st.a } else { &mut st.b };
                    let title = slot.as_ref().map(|s| format!("{} (N = {})", s.label, s.specs.len())).unwrap_or_else(|| " ".into());
                    ui.label(RichText::new(title).strong());
                    ui.label(format!("Input Data Set ({tag})"));
                    let rows: Vec<(Vec<String>, bool, bool)> = slot
                        .as_ref()
                        .map(|s| s.order().into_iter().map(|i| (vec![format!("{tag}{}", i + 1), s.specs[i].name.clone(), format!("{:.0}", s.specs[i].field_am), format!("{:.0}", s.specs[i].freq_hz), format!("{:.3E}", s.specs[i].inphase.km)], s.active(i), s.sel[i])).collect())
                        .unwrap_or_default();
                    let order: Vec<usize> = slot.as_ref().map(|s| s.order()).unwrap_or_default();
                    let head = [format!("{tag}#"), "Name".into(), "Field".into(), "Freq".into(), "Kmean".into()];
                    let head_ref: Vec<&str> = head.iter().map(|s| s.as_str()).collect();
                    if let Some(row) = table(ui, if k == 0 { "tens-a" } else { "tens-b" }, &[44.0, 90.0, 52.0, 52.0, 80.0], &head_ref, &rows, 380.0) {
                        if let Some(s) = slot.as_mut() {
                            let i = order[row];
                            let multi = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                            if !multi {
                                s.sel.iter_mut().for_each(|v| *v = false);
                            }
                            s.sel[i] = !s.sel[i] || !multi;
                        }
                    }
                    ui.add_space(4.0);
                    let has = slot.is_some();
                    ui.horizontal(|ui| {
                        if big_btn(ui, Icon::Open, "Open", true, dark) {
                            if let Some(p) = rfd::FileDialog::new().add_filter("AMS files", &["ams"]).set_title("Open File(s)").pick_file() {
                                match SetSt::load(&p) {
                                    Ok(s) => {
                                        *slot = Some(s);
                                        st.pairs.clear();
                                        st.km.clear();
                                        st.sign = None;
                                        st.msg.clear();
                                    }
                                    Err(e) => st.msg = e,
                                }
                            }
                        }
                        ui.vertical(|ui| {
                            ui.label("Data Filter");
                            ui.label(RichText::new("Field").small());
                            let fields = slot.as_ref().map(|s| s.uniques(|x| x.field_am)).unwrap_or_default();
                            if let Some(s) = slot.as_mut() {
                                egui::ComboBox::from_id_salt(("tens-field", k)).width(86.0).selected_text(s.field_f.map(|v| v.to_string()).unwrap_or_else(|| "No Filter".into())).show_ui(ui, |ui| {
                                    ui.selectable_value(&mut s.field_f, None, "No Filter");
                                    for f in &fields {
                                        ui.selectable_value(&mut s.field_f, Some(*f), f.to_string());
                                    }
                                });
                            } else {
                                ui.add_enabled(false, egui::Button::new("No Filter"));
                            }
                        });
                        let any_sel = slot.as_ref().map_or(false, |s| s.sel.iter().any(|v| *v));
                        if big_btn(ui, Icon::Disregard, "Disregard", any_sel, dark) {
                            if let Some(s) = slot.as_mut() {
                                for i in 0..s.specs.len() {
                                    if s.sel[i] {
                                        s.dis[i] = true;
                                        s.sel[i] = false;
                                    }
                                }
                            }
                            st.pairs.clear();
                            st.km.clear();
                        }
                    });
                    ui.horizontal(|ui| {
                        if big_btn(ui, Icon::Dup, "Find Duplicates", has, dark) {
                            if let Some(s) = slot.as_mut() {
                                let mut seen = std::collections::HashSet::new();
                                let mut n = 0;
                                for i in 0..s.specs.len() {
                                    if s.active(i) && !seen.insert(s.specs[i].name.clone()) {
                                        s.dis[i] = true;
                                        n += 1;
                                    }
                                }
                                st.msg = format!("{n} duplicate record(s) disregarded in set {tag} (the first of each name is kept).");
                            }
                            st.pairs.clear();
                            st.km.clear();
                        }
                        ui.vertical(|ui| {
                            ui.add_space(14.0);
                            ui.label(RichText::new("Frequency").small());
                            let freqs = slot.as_ref().map(|s| s.uniques(|x| x.freq_hz)).unwrap_or_default();
                            if let Some(s) = slot.as_mut() {
                                egui::ComboBox::from_id_salt(("tens-freq", k)).width(86.0).selected_text(s.freq_f.map(|v| v.to_string()).unwrap_or_else(|| "No Filter".into())).show_ui(ui, |ui| {
                                    ui.selectable_value(&mut s.freq_f, None, "No Filter");
                                    for f in &freqs {
                                        ui.selectable_value(&mut s.freq_f, Some(*f), f.to_string());
                                    }
                                });
                            } else {
                                ui.add_enabled(false, egui::Button::new("No Filter"));
                            }
                        });
                        if big_btn(ui, Icon::Restore, "Restore All", has, dark) {
                            if let Some(s) = slot.as_mut() {
                                s.dis.iter_mut().for_each(|v| *v = false);
                                s.field_f = None;
                                s.freq_f = None;
                            }
                            st.pairs.clear();
                            st.km.clear();
                        }
                    });
                    ui.label(RichText::new(format!("Active Records = {}", slot.as_ref().map_or(0, |s| s.n_active()))).small());
                }
                // ---- combined set C
                let ui = &mut cols[2];
                ui.label(" ");
                ui.label("Combined Data Set (C)");
                let rows: Vec<(Vec<String>, bool, bool)> = st
                    .pairs
                    .iter()
                    .enumerate()
                    .map(|(n, (i, j))| {
                        let name = st.a.as_ref().map(|s| s.specs[*i].name.clone()).unwrap_or_default();
                        (vec![format!("C{}", n + 1), name, format!("A{}", i + 1), format!("B{}", j + 1), st.km.get(n).copied().flatten().map(|k| format!("{k:.3E}")).unwrap_or_default()], true, false)
                    })
                    .collect();
                table(ui, "tens-c", &[44.0, 90.0, 52.0, 52.0, 80.0], &["C#", "Name", "A#", "B#", "Kmean"], &rows, 380.0);
                ui.add_space(4.0);
                let both = st.a.is_some() && st.b.is_some();
                ui.horizontal(|ui| {
                    if big_btn(ui, Icon::Pair, "Pair Records", both, dark) {
                        st.pair();
                    }
                    if big_btn(ui, Icon::Plus, "Add Tensors", both, dark) {
                        st.calc(1.0);
                    }
                    if big_btn(ui, Icon::Save, "Save As", st.sign.is_some(), dark) {
                        if let Some(p) = rfd::FileDialog::new().add_filter("AMS file", &["ams"]).set_title("Save Combined Data Set").save_file() {
                            st.msg = match st.result_bytes() {
                                Some(bytes) => match std::fs::write(&p, bytes) {
                                    Ok(()) => format!("Saved {} records to {}", st.pairs.len(), p.display()),
                                    Err(e) => format!("Could not save: {e}"),
                                },
                                None => "Press Add Tensors or Subtract Tensors first.".into(),
                            };
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if big_btn(ui, Icon::Eraser, "Clear Pairing", !st.pairs.is_empty(), dark) {
                        st.pairs.clear();
                        st.km.clear();
                        st.sign = None;
                    }
                    if big_btn(ui, Icon::Minus, "Subtract Tensors", both, dark) {
                        st.calc(-1.0);
                    }
                    close = big_btn(ui, Icon::CloseRed, "Close", true, dark);
                });
                ui.label(RichText::new(format!("Active Records = {}", st.pairs.len())).small());
            });
            if !st.msg.is_empty() {
                ui.label(RichText::new(&st.msg).small().weak());
            }
        });
        self.tools.tens = st;
        self.tools.open[1] = open && !close;
    }

    // ------------------------------------------------------------------ Fabric overprint
    fn fab_window(&mut self, ctx: &egui::Context) {
        let dark = self.dark;
        let mut open = true;
        let mut st = std::mem::take(&mut self.tools.fab);
        let mut close = false;
        // fabric of the selected record of the main window (for the Load buttons)
        let loaded: Option<FabricIn> = self.doc.as_ref().and_then(|d| {
            if self.derived.len() != d.specs.len() {
                return None;
            }
            let i = (0..d.specs.len()).find(|&i| d.selected[i] && d.active[i])?;
            fabric_from_tensor(self.derived[i].km, &self.derived[i].tensor)
        });
        let mut new_doc: Option<Doc> = None;
        self.base_window(ctx, "Fabric Overprint", T_FAB, Vec2::new(840.0, 700.0)).open(&mut open).resizable(false).show(ctx, |ui| {
            pop_style(ui, 1.45);
            ui.horizontal_top(|ui| {
                for k in 0..2 {
                    let f = if k == 0 { &mut st.a } else { &mut st.b };
                    ui.group(|ui| {
                      ui.vertical(|ui| {
                        ui.set_width(204.0 * 1.45);
                        ui.label(RichText::new(if k == 0 { "Fabric A" } else { "Fabric B" }).strong().color(if k == 0 { Color32::from_rgb(40, 60, 230) } else { Color32::from_rgb(220, 40, 40) }));
                        egui::Grid::new(("fab-grid", k)).num_columns(3).show(ui, |ui| {
                            ui.label("Kmean");
                            field(ui, &mut f.kmean, 110.0, Some(tint(dark, 255, 190, 250)));
                            ui.label("");
                            ui.end_row();
                            ui.label("P");
                            field(ui, &mut f.p, 110.0, Some(tint(dark, 190, 255, 255)));
                            ui.label("");
                            ui.end_row();
                            ui.label("T");
                            field(ui, &mut f.t, 110.0, Some(tint(dark, 190, 255, 255)));
                            ui.label("");
                            ui.end_row();
                            ui.label("");
                            ui.horizontal(|ui| { ui.add_space(14.0); ui.label("Dec"); ui.add_space(26.0); ui.label("Inc"); });
                            ui.label("");
                            ui.end_row();
                            ui.label("Kmax");
                            ui.horizontal(|ui| {
                                field(ui, &mut f.kmax[0], 52.0, Some(tint(dark, 255, 185, 185)));
                                field(ui, &mut f.kmax[1], 52.0, Some(tint(dark, 255, 185, 185)));
                            });
                            ui.label("");
                            ui.end_row();
                            ui.label("Kmin");
                            ui.horizontal(|ui| {
                                field(ui, &mut f.kmin[0], 52.0, Some(tint(dark, 190, 190, 255)));
                                field(ui, &mut f.kmin[1], 52.0, Some(tint(dark, 190, 190, 255)));
                            });
                            ui.label("");
                            ui.end_row();
                        });
                        ui.vertical_centered(|ui| {
                            if big_btn(ui, Icon::Plus, "Load", loaded.is_some(), dark) {
                                if let Some(l) = &loaded {
                                    *f = FabStr::from(l);
                                }
                            }
                        });
                      });
                    });
                }
            });
            if loaded.is_none() {
                ui.label(RichText::new("Load fills a fabric from the selected record of the main window.").small().weak());
            }
            ui.horizontal(|ui| {
                ui.label("Increments");
                egui::ComboBox::from_id_salt("fab-inc").width(80.0).selected_text(INCS[st.inc].to_string()).show_ui(ui, |ui| {
                    for (i, n) in INCS.iter().enumerate() {
                        ui.selectable_value(&mut st.inc, i, n.to_string());
                    }
                });
            });
            ui.horizontal(|ui| {
                if big_btn(ui, Icon::Calc, "Calculate", true, dark) {
                    match (st.a.parse(), st.b.parse()) {
                        (Some(a), Some(b)) if a.p >= 1.0 && b.p >= 1.0 => {
                            let series = overprint_series(&a, &b, INCS[st.inc]);
                            let mut raw = Vec::with_capacity(series.len() * RECORD_LEN);
                            for (i, t) in series.iter().enumerate() {
                                raw.extend(simulated_record(&i.to_string(), t));
                            }
                            match Doc::simulated("Simulated fabric overprint", raw) {
                                Ok(d) => {
                                    new_doc = Some(d);
                                    st.msg.clear();
                                }
                                Err(e) => st.msg = e,
                            }
                        }
                        (Some(_), Some(_)) => st.msg = "P must be 1 or larger.".into(),
                        _ => st.msg = "Fill in every field of both fabrics with numbers.".into(),
                    }
                }
                if big_btn(ui, Icon::Eraser, "Erase", true, dark) {
                    st.a = FabStr::default();
                    st.b = FabStr::default();
                    st.msg.clear();
                }
                close = big_btn(ui, Icon::CloseRed, "Close", true, dark);
            });
            if !st.msg.is_empty() {
                ui.label(RichText::new(&st.msg).small().weak());
            }
        });
        self.tools.fab = st;
        self.tools.open[2] = open && !close;
        if let Some(d) = new_doc {
            self.set_doc(d);
        }
    }

    // ------------------------------------------------------------------ Intersection toolbox
    fn inter_window(&mut self, ctx: &egui::Context) {
        let dark = self.dark;
        let ink = if dark { Color32::from_gray(220) } else { Color32::from_gray(30) };
        let mut open = true;
        let mut st = std::mem::take(&mut self.tools.inter);
        let mut close = false;
        self.base_window(ctx, "Intersection Toolbox", T_INTER, Vec2::new(1000.0, 820.0)).open(&mut open).resizable(false).show(ctx, |ui| {
            pop_style(ui, 1.3);
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(560.0), Sense::hover());
                let pa = ui.painter_at(rect.expand(30.0));
                pa.rect_filled(rect, 2.0, if dark { Color32::from_gray(24) } else { Color32::WHITE });
                let (c, r) = (rect.center(), 245.0);
                draw_net(&pa, c, r, ink, true);
                if let (Some((a, b)), Some(res)) = (st.inputs, st.result) {
                    let mark = |p: Pos2, col: Color32| {
                        pa.circle_filled(p, 4.5, col);
                        pa.circle_stroke(p, 4.5, Stroke::new(1.0, Color32::from_gray(30)));
                    };
                    let curve = |dd: f64, dip: f64, col: Color32| {
                        let pts: Vec<Pos2> = great_circle(dd, dip, 90).into_iter().map(|(d, i)| to_screen(c, r, d, i)).collect();
                        pa.add(egui::Shape::line(pts, egui::epaint::PathStroke::new(2.0, col)));
                    };
                    let (red, blue, green) = (Color32::from_rgb(230, 20, 20), Color32::from_rgb(30, 50, 230), Color32::from_rgb(20, 200, 20));
                    match res {
                        Intersection::Plane { dip_dir, dip, .. } => {
                            curve(dip_dir, dip, green);
                            mark(to_screen(c, r, a.0, a.1), red);
                            mark(to_screen(c, r, b.0, b.1), blue);
                        }
                        Intersection::Line { trend, plunge, .. } => {
                            curve(a.0, a.1, red);
                            curve(b.0, b.1, blue);
                            mark(to_screen(c, r, trend, plunge), green);
                        }
                    }
                }
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    ui.group(|ui| {
                        ui.label(RichText::new("Input").strong().color(Color32::from_rgb(220, 30, 30)));
                        if ui.radio_value(&mut st.foliations, false, "2 Lineations").clicked() || ui.radio_value(&mut st.foliations, true, "2 Foliations").clicked() {
                            st.result = None;
                            st.inputs = None;
                        }
                        let (what, c1, c2) = if st.foliations { ("Foliation", "Dir Dir.", "Dip") } else { ("Lineation", "Trend", "Plunge") };
                        for (k, (lab, col)) in [("A", tint(dark, 255, 190, 190)), ("B", tint(dark, 195, 195, 255))].into_iter().enumerate() {
                            ui.label(RichText::new(format!("{what} {lab}")).strong());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(c1).small());
                                ui.add_space(8.0);
                                ui.label(RichText::new(c2).small());
                            });
                            let f = if k == 0 { &mut st.a } else { &mut st.b };
                            ui.horizontal(|ui| {
                                field(ui, &mut f[0], 56.0, Some(col));
                                field(ui, &mut f[1], 56.0, Some(col));
                            });
                        }
                    });
                    ui.group(|ui| {
                        ui.set_width(150.0);
                        ui.label(RichText::new("Output").strong().color(Color32::from_rgb(30, 170, 30)));
                        let green = tint(dark, 190, 250, 190);
                        match st.result {
                            Some(Intersection::Plane { dip_dir, dip, angle }) => {
                                ui.label(RichText::new("Foliation").strong());
                                ui.label(RichText::new("Dir Dir.        Dip").small());
                                ui.horizontal(|ui| { out_box(ui, &format!("{dip_dir:.1}"), green); out_box(ui, &format!("{dip:.1}"), green); });
                                ui.label("Angle Between");
                                out_box(ui, &format!("{angle:.1} ({:.1})", 180.0 - angle), tint(dark, 255, 225, 190));
                            }
                            Some(Intersection::Line { trend, plunge, angle }) => {
                                ui.label(RichText::new("Lineation").strong());
                                ui.label(RichText::new("Trend         Plunge").small());
                                ui.horizontal(|ui| { out_box(ui, &format!("{trend:.1}"), green); out_box(ui, &format!("{plunge:.1}"), green); });
                                ui.label("Angle Between");
                                out_box(ui, &format!("{angle:.1} ({:.1})", 180.0 - angle), tint(dark, 255, 225, 190));
                            }
                            None => {
                                ui.label(RichText::new(if st.foliations { "Lineation" } else { "Foliation" }).strong());
                                ui.horizontal(|ui| { out_box(ui, "", green); out_box(ui, "", green); });
                                ui.label("Angle Between");
                                out_box(ui, "", tint(dark, 255, 225, 190));
                            }
                        }
                    });
                });
            });
            ui.horizontal(|ui| {
                if big_btn(ui, Icon::Calc, "Calculate", true, dark) {
                    let p = |f: &[String; 2]| Some((parse_f(&f[0])?, parse_f(&f[1])?));
                    match (p(&st.a), p(&st.b)) {
                        (Some(a), Some(b)) => {
                            let r = if st.foliations { intersect_foliations(a, b) } else { intersect_lineations(a, b) };
                            match r {
                                Some(res) => {
                                    st.result = Some(res);
                                    st.inputs = Some((a, b));
                                    st.msg.clear();
                                }
                                None => {
                                    st.result = None;
                                    st.msg = "The two inputs are identical (or opposite): no unique result.".into();
                                }
                            }
                        }
                        _ => st.msg = "Enter all four numbers.".into(),
                    }
                }
                if big_btn(ui, Icon::Eraser, "Clear", true, dark) {
                    st.a = Default::default();
                    st.b = Default::default();
                    st.result = None;
                    st.inputs = None;
                    st.msg.clear();
                }
                close = big_btn(ui, Icon::CloseRed, "Close", true, dark);
            });
            if !st.msg.is_empty() {
                ui.label(RichText::new(&st.msg).small().weak());
            }
        });
        self.tools.inter = st;
        self.tools.open[3] = open && !close;
    }

    // ------------------------------------------------------------------ Deform AMS
    fn def_window(&mut self, ctx: &egui::Context) {
        let dark = self.dark;
        let ink = if dark { Color32::from_gray(220) } else { Color32::from_gray(30) };
        let mut open = true;
        let mut st = std::mem::take(&mut self.tools.def);
        let mut close = false;
        let mut copy: Option<Rect> = None;
        self.base_window(ctx, "Deform AMS", T_DEF, Vec2::new(1180.0, 880.0)).open(&mut open).resizable(false).show(ctx, |ui| {
            pop_style(ui, 1.3);
            ui.horizontal_top(|ui| {
                // ---- stereonet with the grains
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(540.0), Sense::hover());
                copy = Some(rect);
                let pa = ui.painter_at(rect.expand(26.0));
                pa.rect_filled(rect, 2.0, if dark { Color32::from_gray(24) } else { Color32::WHITE });
                let (c, r) = (rect.center(), 235.0);
                draw_net(&pa, c, r, ink, true);
                let to_dec_inc = |q: &ams_core::tensor::Vec3| dec_inc([q[1], q[0], q[2]]);
                for q in st.model.axes() {
                    let (d, i) = to_dec_inc(&q);
                    let p = to_screen(c, r, d, i);
                    if st.oblate {
                        pa.circle_filled(p, 2.4, Color32::from_rgb(40, 40, 200));
                        pa.circle_stroke(p, 2.4, Stroke::new(0.8, Color32::from_rgb(20, 20, 120)));
                    } else {
                        let rc = Rect::from_center_size(p, Vec2::splat(5.0));
                        pa.rect_filled(rc, 0.0, Color32::from_rgb(250, 210, 210));
                        pa.rect_stroke(rc, 0.0, Stroke::new(0.8, Color32::from_rgb(120, 60, 60)), StrokeKind::Inside);
                    }
                }
                let ax = st.model.fabric_axes();
                let kc = [Color32::from_rgb(220, 20, 20), Color32::from_rgb(20, 190, 40), Color32::from_rgb(20, 40, 230)];
                for (a, col) in kc.iter().enumerate() {
                    let p = to_screen(c, r, ax.dec_inc[a].0, ax.dec_inc[a].1);
                    match a {
                        0 => { pa.rect_filled(Rect::from_center_size(p, Vec2::splat(11.0)), 0.0, *col); pa.rect_stroke(Rect::from_center_size(p, Vec2::splat(11.0)), 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Inside); }
                        1 => { pa.add(egui::Shape::convex_polygon(vec![Pos2::new(p.x, p.y - 7.0), Pos2::new(p.x + 6.5, p.y + 5.0), Pos2::new(p.x - 6.5, p.y + 5.0)], *col, Stroke::new(1.0, Color32::BLACK))); }
                        _ => { pa.circle_filled(p, 6.0, *col); pa.circle_stroke(p, 6.0, Stroke::new(1.0, Color32::BLACK)); }
                    }
                }
                // ---- controls
                ui.vertical(|ui| {
                    ui.set_width(350.0 * 1.3);
                    let mut reset_model = false;
                    ui.group(|ui| {
                        ui.set_width(336.0 * 1.3);
                        ui.label(RichText::new("Single grain characteristics").strong().color(Color32::from_rgb(40, 60, 230)));
                        let before = st.oblate;
                        egui::ComboBox::from_id_salt("def-grain").width(318.0 * 1.3).selected_text(if st.oblate { "Oblate Spheroids" } else { "Prolate Spheroids" }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut st.oblate, false, "Prolate Spheroids");
                            ui.selectable_value(&mut st.oblate, true, "Oblate Spheroids");
                        });
                        reset_model |= before != st.oblate;
                        egui::Grid::new("def-grain-g").show(ui, |ui| {
                            ui.label(RichText::new("Pgrain").strong());
                            let ch = field(ui, &mut st.p, 70.0, None);
                            reset_model |= ch;
                            ui.end_row();
                            ui.label(RichText::new("Tgrain").strong());
                            ui.add_enabled(false, egui::TextEdit::singleline(&mut (if st.oblate { "1" } else { "-1" }).to_string()).desired_width(70.0));
                            ui.end_row();
                        });
                    });
                    ui.group(|ui| {
                        ui.set_width(336.0 * 1.3);
                        ui.label(RichText::new("Strain").strong().color(Color32::from_rgb(40, 60, 230)));
                        let kind = StrainKind::ALL[st.kind];
                        egui::ComboBox::from_id_salt("def-kind").width(318.0 * 1.3).selected_text(kind.label()).show_ui(ui, |ui| {
                            for (i, k) in StrainKind::ALL.iter().enumerate() {
                                if ui.selectable_value(&mut st.kind, i, k.label()).clicked() {
                                    st.axis = 0;
                                }
                            }
                        });
                        let axes = StrainKind::ALL[st.kind].axes();
                        if !axes.is_empty() {
                            st.axis = st.axis.min(axes.len() - 1);
                            egui::ComboBox::from_id_salt(("def-axis", st.kind)).width(318.0 * 1.3).selected_text(axes[st.axis]).show_ui(ui, |ui| {
                                for (i, a) in axes.iter().enumerate() {
                                    ui.selectable_value(&mut st.axis, i, *a);
                                }
                            });
                        }
                        egui::Grid::new("def-sg").show(ui, |ui| {
                            ui.label(RichText::new("Stretch").strong());
                            field(ui, &mut st.stretch, 70.0, None);
                            ui.end_row();
                            ui.label(RichText::new("Gamma").strong());
                            field(ui, &mut st.gamma, 70.0, None);
                            ui.end_row();
                        });
                        ui.label(RichText::new("Deformational Matrix").strong());
                        egui::Grid::new("def-matrix").striped(true).min_col_width(96.0).show(ui, |ui| {
                            for r in 0..3 {
                                for c in 0..3 {
                                    ui.label(st.matrix.map(|m| format!("{:.5}", m[r][c])).unwrap_or_default());
                                }
                                ui.end_row();
                            }
                        });
                    });
                    if reset_model {
                        let p = parse_f(&st.p).filter(|v| *v >= 1.0).unwrap_or(1.0);
                        st.model = Deform::new(st.oblate, p);
                        st.matrix = None;
                    }
                });
            });
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Resultant Magnetic Fabric").strong().color(Color32::from_rgb(40, 60, 230)));
                let ax = st.model.fabric_axes();
                let sh = ax.shape;
                let f = |v: Option<f64>| v.map(|x| format!("{x:.3}")).unwrap_or_default();
                egui::Grid::new("def-fabric").spacing([6.0, 3.0]).show(ui, |ui| {
                    ui.label("");
                    ui.label("");
                    ui.label("");
                    ui.label("");
                    for h in ["L", "F", "P", "Pj", "T", "U", "Q", "E"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    let cols = [tint(dark, 255, 180, 180), tint(dark, 190, 255, 190), tint(dark, 195, 195, 255)];
                    for (a, name) in ["Kmax", "Kint", "Kmin"].iter().enumerate() {
                        ui.label(RichText::new(*name).strong());
                        out_box(ui, &format!("{:.6}", ax.k_norm[a]), cols[a]);
                        out_box(ui, &format!("{:.1}", ax.dec_inc[a].0), cols[a]);
                        out_box(ui, &format!("{:.1}", ax.dec_inc[a].1), cols[a]);
                        if a == 0 {
                            let cy = tint(dark, 190, 250, 250);
                            for v in [sh.map(|s| s.l), sh.map(|s| s.f), sh.map(|s| s.p), sh.map(|s| s.pj), sh.map(|s| s.t), sh.map(|s| s.u), sh.map(|s| s.q), sh.map(|s| s.e)] {
                                out_box(ui, &f(v), cy);
                            }
                        }
                        ui.end_row();
                    }
                });
            });
            ui.horizontal(|ui| {
                if big_btn(ui, Icon::Plus, "Add Step", true, dark) {
                    match (parse_f(&st.stretch), parse_f(&st.gamma)) {
                        (Some(s), Some(g)) if s > 0.0 => {
                            let m = strain_matrix(StrainKind::ALL[st.kind], st.axis, s, g);
                            st.model.add_step(&m);
                            st.matrix = Some(m);
                            st.msg.clear();
                        }
                        _ => st.msg = "Stretch must be a positive number and Gamma a number.".into(),
                    }
                }
                ui.add_space(110.0);
                if big_btn(ui, Icon::Eraser, "Reset Isotropic", true, dark) {
                    st.model.reset();
                    st.matrix = None;
                }
                if big_btn(ui, Icon::Camera, "Copy Graphics", true, dark) {
                    self.tool_copy = copy;
                    self.request_shot(ctx);
                }
                close = big_btn(ui, Icon::CloseRed, "Close Toolbox", true, dark);
            });
            if !st.msg.is_empty() {
                ui.label(RichText::new(&st.msg).small().weak());
            }
        });
        self.tools.def = st;
        self.tools.open[4] = open && !close;
    }
}

fn out_box(ui: &mut Ui, text: &str, fill: Color32) {
    let z = zf(ui);
    let w = (text.len() as f32 * 7.5 * z + 16.0 * z).max(58.0 * z);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 22.0 * z), Sense::hover());
    ui.painter().rect(rect, 3.0, fill, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(13.0 * z), Color32::from_gray(if ui.visuals().dark_mode { 235 } else { 20 }));
}


#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "4   AB              11     22     T   S   L   R   6  0  6  0  \r\nC1          30  40  78  101 21  102 22  95  103 23  104 24    \r\nB1          50  60  14  2   3   5   6   71  8   9   2   3     \r\n";

    #[test]
    fn ged_editor_state_round_trips_the_file_format() {
        let g = GedFile::parse(SAMPLE).unwrap();
        let mut st = GedState::default();
        st.load(&g);
        assert_eq!(st.op, [1, 0, 1, 0]);
        assert_eq!(st.rows[0].c[2], "7");
        assert_eq!(st.to_file().to_text(), SAMPLE);
        // deleted rows are not saved
        st.rows[1].deleted = true;
        assert_eq!(st.to_file().rows.len(), 1);
    }

    #[test]
    fn blank_rows_and_text_cells_are_handled() {
        let mut st = GedState::default();
        st.rows[0].name = "ZZ1".into();
        st.rows[0].c[3] = "12.5".into();
        st.rows[0].c[4] = "abc".into();
        let f = st.to_file();
        assert_eq!(f.rows.len(), 1);
        assert_eq!(f.rows[0].fol1, [12.5, 0.0]);
        assert_eq!(f.rows[0].codes, ['0'; 4]);
    }

    fn set_of(names: &[&str], kms: &[f64]) -> SetSt {
        use ams_core::ams::{build_record, Phase};
        use ams_core::tensor::sym;
        let mut raw = vec![];
        for (n, k) in names.iter().zip(kms) {
            let ph = Phase { km: *k, sd: [1e-3; 3], tensor: sym(1.2, 1.0, 0.8, 0.0, 0.0, 0.0), ftest: [5.0, 1.0, 1.0] };
            raw.extend(build_record(n, &ph, &ph, 400.0, 1220.0));
        }
        let specs = ams_core::ams::parse(&raw).unwrap();
        let n = specs.len();
        SetSt { label: "t".into(), raw, specs, dis: vec![false; n], sel: vec![false; n], field_f: None, freq_f: None }
    }

    #[test]
    fn tensor_window_pairs_adds_subtracts_and_saves() {
        let mut st = TensState { a: Some(set_of(&["C1", "B1", "A2"], &[2e-3, 3e-3, 1e-3])), b: Some(set_of(&["B1", "C1", "Z9"], &[1e-3, 1e-3, 5e-3])), ..Default::default() };
        assert!(st.result_bytes().is_none());
        st.calc(1.0); // pairs automatically: C1 (A0,B1), B1 (A1,B0)
        assert_eq!(st.pairs, vec![(0, 1), (1, 0)]);
        assert!((st.km[0].unwrap() - 3e-3).abs() < 1e-9 && (st.km[1].unwrap() - 4e-3).abs() < 1e-9);
        let out = ams_core::ams::parse(&st.result_bytes().unwrap()).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].name, "B1");
        assert!((out[1].inphase.km - 4e-3).abs() < 1e-8);
        st.calc(-1.0);
        assert!((st.km[0].unwrap() - 1e-3).abs() < 1e-9);
        // disregarded records are not paired
        st.a.as_mut().unwrap().dis[0] = true;
        st.pair();
        assert_eq!(st.pairs, vec![(1, 0)]);
        assert!(st.sign.is_none() && st.result_bytes().is_none());
    }

    #[test]
    fn filters_and_order_of_a_set() {
        let mut s = set_of(&["a", "b", "c"], &[1.0, 2.0, 3.0]);
        s.dis[0] = true;
        assert_eq!(s.n_active(), 2);
        assert_eq!(s.order(), vec![1, 2, 0]); // disregarded last
        s.field_f = Some(999);
        assert_eq!(s.n_active(), 0);
        assert_eq!(s.uniques(|x| x.field_am), vec![400]);
    }

    #[test]
    fn fabric_strings_round_trip() {
        let f = FabricIn { kmean: 1.5, p: 1.25, t: -0.3, kmax: (10.0, 20.0), kmin: (100.0, 30.0) };
        let back = FabStr::from(&f).parse().unwrap();
        assert!((back.p - 1.25).abs() < 1e-9 && (back.kmax.1 - 20.0).abs() < 1e-9 && (back.kmean - 1.5).abs() < 1e-9);
        assert!(FabStr::default().parse().is_none());
    }
}
