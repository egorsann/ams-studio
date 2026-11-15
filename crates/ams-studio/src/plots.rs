//! The four plots of the main window, drawn directly with egui's painter.

use ams_core::density::project;
use ams_core::params::{N_PARAMS, PARAM_LABELS};
use egui::{Align2, Color32, FontId, Modifiers, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

#[derive(Clone, Debug)]
pub struct Pt {
    pub idx: usize,
    pub name: String,
    pub di: [(f64, f64); 3],
    /// Every plottable parameter (see `ams_core::params`), `NaN` where undefined.
    pub vals: [f64; N_PARAMS],
    /// Standard deviations of K1..K3 (for confidence intervals).
    pub sds: [f64; 3],
    pub selected: bool,
    /// Colour from colour-coding, when enabled.
    pub color: Option<Color32>,
}

#[derive(Default)]
pub struct PlotOut {
    pub click: Option<usize>,
    pub hover: Option<String>,
    /// Specimen indices inside a rubber band that was just released.
    pub band: Option<Vec<usize>>,
}

/// Marker types a selection may touch: no modifier = all shown types; Ctrl = K1, Alt = K2, Shift = K3
/// (combinations select the union).
pub fn axis_mask(m: Modifiers) -> [bool; 3] {
    let (a, b, c) = (m.ctrl || m.command, m.alt, m.shift);
    if !(a || b || c) {
        [true; 3]
    } else {
        [a, b, c]
    }
}

/// Rubber-band state for a drag on `resp`: (rect while dragging, rect when released).
pub(crate) fn band(ui: &mut Ui, resp: &Response, key: &str) -> (Option<Rect>, Option<Rect>) {
    let id = resp.id.with(key);
    if resp.drag_started() {
        // where the button went down (the pointer has already moved on by the time a drag is detected)
        if let Some(p) = ui.input(|i| i.pointer.press_origin()).or_else(|| resp.interact_pointer_pos()) {
            ui.memory_mut(|m| m.data.insert_temp(id, p));
        }
    }
    let start: Option<Pos2> = ui.memory(|m| m.data.get_temp(id));
    let cur = resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.latest_pos()));
    let rect = |a: Pos2, b: Pos2| Rect::from_two_pos(a, b);
    let live = if resp.dragged() { start.zip(cur).map(|(a, b)| rect(a, b)) } else { None };
    let done = if resp.drag_stopped() {
        let r = start.zip(cur).map(|(a, b)| rect(a, b)).filter(|r| r.width() > 3.0 || r.height() > 3.0);
        ui.memory_mut(|m| m.data.remove::<Pos2>(id));
        r
    } else {
        None
    };
    (live, done)
}

pub(crate) fn paint_band(ui: &Ui, clip: Rect, r: Rect, th: &Theme) {
    let pa = ui.painter_at(clip);
    pa.rect_filled(r, 0.0, th.accent.gamma_multiply(0.18));
    pa.rect_stroke(r, 0.0, Stroke::new(1.2, th.accent), egui::StrokeKind::Inside);
}

/// One plot axis: which parameter, optional fixed range, linear or log10.
#[derive(Clone, Debug)]
pub struct AxisCfg {
    pub param: usize,
    pub auto: bool,
    pub min: f64,
    pub max: f64,
    pub log: bool,
}

impl AxisCfg {
    pub fn new(param: usize) -> AxisCfg {
        AxisCfg { param, auto: true, min: 0.0, max: 1.0, log: false }
    }
    pub fn label(&self) -> &'static str {
        PARAM_LABELS[self.param.min(N_PARAMS - 1)]
    }
}

/// Colour-coding of the markers by a parameter.
#[derive(Clone, Debug)]
pub struct ColorCfg {
    pub on: bool,
    pub param: usize,
    pub auto: bool,
    pub min: f64,
    pub max: f64,
    pub log: bool,
    /// 0 rainbow, 1 shades, 2 grayscales
    pub palette: u8,
    /// Colour by the data classes of the file instead of a parameter.
    pub by_class: bool,
}

impl Default for ColorCfg {
    fn default() -> Self {
        ColorCfg { on: false, param: 0, auto: true, min: 0.0, max: 1.0, log: false, palette: 0, by_class: false }
    }
}

impl ColorCfg {
    /// (low, high) of the colour scale for the given values (log10 applied when `log`).
    pub fn range(&self, vals: impl Iterator<Item = f64>) -> (f64, f64) {
        let f = |v: f64| if self.log { if v > 0.0 { v.log10() } else { f64::NAN } } else { v };
        if !self.auto {
            let (a, b) = (f(self.min), f(self.max));
            if a.is_finite() && b.is_finite() && b > a {
                return (a, b);
            }
        }
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for v in vals.map(f).filter(|v| v.is_finite()) {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) }
    }

    /// Colour for a value inside `range` (or `None` when the value is undefined).
    pub fn color(&self, v: f64, range: (f64, f64)) -> Option<Color32> {
        let x = if self.log { if v > 0.0 { v.log10() } else { return None } } else { v };
        if !x.is_finite() {
            return None;
        }
        let t = if range.1 > range.0 { ((x - range.0) / (range.1 - range.0)).clamp(0.0, 1.0) } else { 0.5 };
        Some(palette_color(self.palette, t))
    }
}

pub fn palette_color(palette: u8, t: f64) -> Color32 {
    match palette {
        0 => rainbow(t),
        1 => shades(t),
        _ => gray(t),
    }
}

/// Horizontal colour scale with the end values and the parameter name.
pub fn color_bar(ui: &Ui, rect: Rect, cfg: &ColorCfg, range: (f64, f64), th: &Theme) {
    let pa = ui.painter_at(rect.expand(2.0));
    let bar = Rect::from_min_max(Pos2::new(rect.min.x + 56.0, rect.min.y + 3.0), Pos2::new(rect.max.x - 56.0, rect.min.y + 13.0));
    let n = 120;
    for s in 0..n {
        let (x0, x1) = (bar.min.x + bar.width() * s as f32 / n as f32, bar.min.x + bar.width() * (s + 1) as f32 / n as f32 + 0.6);
        pa.rect_filled(Rect::from_min_max(Pos2::new(x0, bar.min.y), Pos2::new(x1, bar.max.y)), 0.0, palette_color(cfg.palette, s as f64 / (n - 1) as f64));
    }
    pa.rect_stroke(bar, 0.0, Stroke::new(1.0, th.line()), egui::StrokeKind::Outside);
    let f = FontId::proportional(10.5);
    let txt = |v: f64| {
        let v = if cfg.log { 10f64.powf(v) } else { v };
        if v != 0.0 && (v.abs() < 0.01 || v.abs() >= 10000.0) { format!("{v:.2E}") } else { format!("{v:.3}").trim_end_matches('0').trim_end_matches('.').to_string() }
    };
    pa.text(Pos2::new(bar.min.x - 5.0, bar.center().y), Align2::RIGHT_CENTER, txt(range.0), f.clone(), th.ink());
    pa.text(Pos2::new(bar.max.x + 5.0, bar.center().y), Align2::LEFT_CENTER, txt(range.1), f.clone(), th.ink());
    let name = format!("{}{}", PARAM_LABELS[cfg.param.min(N_PARAMS - 1)], if cfg.log { " (log)" } else { "" });
    pa.text(Pos2::new(bar.center().x, bar.max.y + 2.0), Align2::CENTER_TOP, name, f, th.faint());
}

pub struct Theme {
    pub dark: bool,
    pub k_colors: [Color32; 3],
    pub accent: Color32,
}

impl Theme {
    pub fn ink(&self) -> Color32 {
        if self.dark { Color32::from_rgb(230, 232, 235) } else { Color32::from_gray(30) }
    }
    pub fn faint(&self) -> Color32 {
        if self.dark { Color32::from_rgb(111, 119, 130) } else { Color32::from_gray(190) }
    }
    pub fn canvas(&self) -> Color32 {
        if self.dark { Color32::from_rgb(17, 19, 23) } else { Color32::WHITE }
    }
}

impl Theme {
    /// Axis frames, ticks and the net circle: softer than the text on the dark background.
    pub fn line(&self) -> Color32 {
        if self.dark { Color32::from_rgb(138, 146, 158) } else { Color32::from_gray(30) }
    }
    /// Thin outline around markers.
    pub fn edge(&self) -> Color32 {
        if self.dark { Color32::from_rgb(14, 16, 20) } else { Color32::from_gray(30) }
    }
    /// Colour-coded markers are a little softer on the dark background.
    pub fn soft(&self, c: Color32) -> Color32 {
        if !self.dark {
            return c;
        }
        let lum = 0.3 * c.r() as f32 + 0.59 * c.g() as f32 + 0.11 * c.b() as f32;
        let m = |x: u8| ((x as f32 * 0.78 + lum * 0.22) * 0.93 + 14.0).min(255.0) as u8;
        Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
    }
}

pub fn rainbow(t: f64) -> Color32 {
    let h = (1.0 - t.clamp(0.0, 1.0)) * 0.72; // red (high) .. blue (low)
    let (r, g, b) = hsv(h, 0.85, 0.92);
    Color32::from_rgb(r, g, b)
}
pub fn shades(t: f64) -> Color32 {
    let t = t.clamp(0.0, 1.0) as f32;
    Color32::from_rgb((200.0 - 170.0 * t) as u8, (215.0 - 150.0 * t) as u8, (250.0 - 90.0 * t) as u8)
}
pub fn gray(t: f64) -> Color32 {
    let v = (230.0 - 200.0 * t.clamp(0.0, 1.0)) as u8;
    Color32::from_gray(v)
}

fn hsv(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match (i as i64).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> (Vec<f64>, usize) {
    let span = (hi - lo).abs().max(1e-12);
    let raw = span / target.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = mag * if norm < 1.5 { 1.0 } else if norm < 3.5 { 2.0 } else if norm < 7.5 { 5.0 } else { 10.0 };
    let mut v = vec![];
    let mut x = (lo / step).ceil() * step;
    while x <= hi + step * 1e-9 {
        v.push(if x.abs() < step * 1e-9 { 0.0 } else { x });
        x += step;
    }
    let dec = (-step.log10().floor()).max(0.0) as usize;
    (v, dec)
}

/// Axis range with a little padding, never degenerate.
pub fn range_of(vals: impl Iterator<Item = f64>, pad: f64) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in vals.filter(|v| v.is_finite()) {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if !lo.is_finite() {
        return (0.0, 1.0);
    }
    if (hi - lo).abs() < 1e-12 {
        return (lo - 0.5, hi + 0.5);
    }
    let d = (hi - lo) * pad;
    (lo - d, hi + d)
}

/// The plot rectangle `draw_axes` leaves inside `outer`.
pub fn plot_rect(outer: Rect, ylog: bool) -> Rect {
    Rect::from_min_max(Pos2::new(outer.min.x + if ylog { 62.0 } else { 54.0 }, outer.min.y + 12.0), Pos2::new(outer.max.x - 16.0, outer.max.y - 46.0))
}

/// Zoom and pan of an x-y plot: the visible part of the full range as fractions (x from the left, y from the bottom).
/// Ctrl + scroll zooms about the pointer, scroll or middle-drag pans a zoomed plot, Esc resets.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ZoomView {
    pub x: (f64, f64),
    pub y: (f64, f64),
}

impl Default for ZoomView {
    fn default() -> Self {
        ZoomView { x: (0.0, 1.0), y: (0.0, 1.0) }
    }
}

impl ZoomView {
    pub const MAX: f64 = 60.0;

    pub fn is_zoomed(&self) -> bool {
        self.x.0 > 1e-9 || self.x.1 < 1.0 - 1e-9 || self.y.0 > 1e-9 || self.y.1 < 1.0 - 1e-9
    }

    pub fn reset(&mut self) {
        *self = ZoomView::default();
    }

    fn zoom_axis(a: (f64, f64), u: f64, factor: f64) -> (f64, f64) {
        let w = a.1 - a.0;
        let nw = (w / factor).clamp(1.0 / Self::MAX, 1.0);
        let na = (a.0 + u * w - u * nw).clamp(0.0, 1.0 - nw);
        (na, na + nw)
    }

    fn pan_axis(a: (f64, f64), d: f64) -> (f64, f64) {
        let w = a.1 - a.0;
        let na = (a.0 + d * w).clamp(0.0, 1.0 - w);
        (na, na + w)
    }

    /// Zooms by `factor` about the point (`u`, `v`) of the visible window (0..1, v from the bottom).
    pub fn zoom_about(&mut self, u: f64, v: f64, factor: f64) {
        self.x = Self::zoom_axis(self.x, u, factor);
        self.y = Self::zoom_axis(self.y, v, factor);
    }

    /// Moves the window by fractions of itself.
    pub fn pan(&mut self, du: f64, dv: f64) {
        self.x = Self::pan_axis(self.x, du);
        self.y = Self::pan_axis(self.y, dv);
    }

    /// The visible ranges inside the full ones.
    pub fn apply(&self, xr: (f64, f64), yr: (f64, f64)) -> ((f64, f64), (f64, f64)) {
        let at = |r: (f64, f64), f: (f64, f64)| (r.0 + (r.1 - r.0) * f.0, r.0 + (r.1 - r.0) * f.1);
        (at(xr, self.x), at(yr, self.y))
    }
}

/// Ctrl + scroll (or a pinch) zooms about the pointer, scroll and middle-drag pan a zoomed plot.
fn handle_zoom(ui: &Ui, resp: &Response, plot: Rect, view: &mut ZoomView) {
    let mods = ui.input(|i| i.modifiers);
    if resp.hovered() {
        if let Some(h) = resp.hover_pos() {
            let (zd, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            if (zd - 1.0).abs() > 1e-4 {
                let u = ((h.x - plot.min.x) / plot.width()).clamp(0.0, 1.0) as f64;
                let v = ((plot.max.y - h.y) / plot.height()).clamp(0.0, 1.0) as f64;
                view.zoom_about(u, v, (zd as f64).powf(0.7));
            } else if view.is_zoomed() && scroll != Vec2::ZERO && !(mods.ctrl || mods.command) {
                view.pan(-(scroll.x / plot.width()) as f64, (scroll.y / plot.height()) as f64);
            }
        }
    }
    if resp.dragged_by(egui::PointerButton::Middle) {
        let d = resp.drag_delta();
        view.pan(-(d.x / plot.width()) as f64, (d.y / plot.height()) as f64);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
}

/// A small label box next to a marker (Alt + hover), kept inside `bounds`.
pub(crate) fn label_box(pa: &egui::Painter, bounds: Rect, at: Pos2, text: String, th: &Theme) {
    let galley = pa.layout_no_wrap(text, FontId::proportional(12.5), th.ink());
    let size = galley.size() + Vec2::new(12.0, 6.0);
    let mut pos = at + Vec2::new(11.0, -size.y - 6.0);
    pos.x = pos.x.min(bounds.max.x - size.x - 2.0).max(bounds.min.x + 2.0);
    pos.y = pos.y.max(bounds.min.y + 2.0).min(bounds.max.y - size.y - 2.0);
    let r = Rect::from_min_size(pos, size);
    pa.rect(r, 4.0, th.canvas(), Stroke::new(1.0, th.line()), egui::StrokeKind::Inside);
    pa.galley(pos + Vec2::new(6.0, 3.0), galley, th.ink());
    pa.circle_stroke(at, 7.5, Stroke::new(1.6, th.accent));
}

struct Frame2D {
    plot: Rect,
    x: (f64, f64),
    y: (f64, f64),
}

impl Frame2D {
    fn to_screen(&self, x: f64, y: f64) -> Pos2 {
        Pos2::new(
            self.plot.min.x + ((x - self.x.0) / (self.x.1 - self.x.0)) as f32 * self.plot.width(),
            self.plot.max.y - ((y - self.y.0) / (self.y.1 - self.y.0)) as f32 * self.plot.height(),
        )
    }
}

fn log_ticks(pa: &egui::Painter, lo: f64, hi: f64, mut draw: impl FnMut(f64, bool, Option<String>)) {
    let span = hi - lo;
    for k in (lo.floor() as i32)..=(hi.ceil() as i32) {
        for m in 1..=9 {
            let v = k as f64 + (m as f64).log10();
            if v < lo - 1e-9 || v > hi + 1e-9 {
                continue;
            }
            let major = m == 1;
            if !major && span > 3.0 {
                continue;
            }
            let label = (major || (span < 2.0 && (m == 2 || m == 5))).then(|| format!("{m}E{k:+}"));
            draw(v, major, label);
        }
    }
    let _ = pa;
}

fn draw_axes(ui: &Ui, outer: Rect, x: (f64, f64), y: (f64, f64), xlabel: &str, ylabel: &str, th: &Theme, xlog: bool, ylog: bool) -> Frame2D {
    draw_axes_ex(ui, outer, x, y, xlabel, ylabel, th, xlog, ylog, false)
}

/// `xcat`: the X axis holds categories, which the caller labels itself (no numeric ticks).
fn draw_axes_ex(ui: &Ui, outer: Rect, x: (f64, f64), y: (f64, f64), xlabel: &str, ylabel: &str, th: &Theme, xlog: bool, ylog: bool, xcat: bool) -> Frame2D {
    let pa = ui.painter_at(outer);
    let plot = plot_rect(outer, ylog);
    let f = Frame2D { plot, x, y };
    pa.rect_filled(plot, 2.0, th.canvas());
    let font = FontId::proportional(11.0);
    if xcat {
        // categories: the caller writes the labels
    } else if xlog {
        log_ticks(&pa, x.0, x.1, |v, major, label| {
            let s = f.to_screen(v, y.0);
            pa.line_segment([Pos2::new(s.x, plot.max.y), Pos2::new(s.x, plot.max.y + if major { 5.0 } else { 3.0 })], Stroke::new(1.0, th.line()));
            pa.line_segment([Pos2::new(s.x, plot.min.y), Pos2::new(s.x, plot.max.y)], Stroke::new(0.5, th.faint().gamma_multiply(if major { 0.5 } else { 0.25 })));
            if let Some(l) = label {
                pa.text(Pos2::new(s.x, plot.max.y + 7.0), Align2::CENTER_TOP, l, font.clone(), th.ink());
            }
        });
    } else {
        let (xt, xd) = nice_ticks(x.0, x.1, (plot.width() / 70.0) as usize);
        for v in xt {
            let s = f.to_screen(v, y.0);
            pa.line_segment([Pos2::new(s.x, plot.max.y), Pos2::new(s.x, plot.max.y + 4.0)], Stroke::new(1.0, th.line()));
            pa.line_segment([Pos2::new(s.x, plot.min.y), Pos2::new(s.x, plot.max.y)], Stroke::new(0.5, th.faint().gamma_multiply(0.5)));
            pa.text(Pos2::new(s.x, plot.max.y + 6.0), Align2::CENTER_TOP, format!("{v:.xd$}"), font.clone(), th.ink());
        }
    }
    if ylog {
        log_ticks(&pa, y.0, y.1, |v, major, label| {
            let s = f.to_screen(x.0, v);
            pa.line_segment([Pos2::new(plot.min.x - if major { 5.0 } else { 3.0 }, s.y), Pos2::new(plot.min.x, s.y)], Stroke::new(1.0, th.line()));
            pa.line_segment([Pos2::new(plot.min.x, s.y), Pos2::new(plot.max.x, s.y)], Stroke::new(0.5, th.faint().gamma_multiply(if major { 0.5 } else { 0.25 })));
            if let Some(l) = label {
                pa.text(Pos2::new(plot.min.x - 7.0, s.y), Align2::RIGHT_CENTER, l, font.clone(), th.ink());
            }
        });
    } else {
        let (yt, yd) = nice_ticks(y.0, y.1, (plot.height() / 36.0) as usize);
        for v in yt {
            let s = f.to_screen(x.0, v);
            pa.line_segment([Pos2::new(plot.min.x - 4.0, s.y), Pos2::new(plot.min.x, s.y)], Stroke::new(1.0, th.line()));
            pa.line_segment([Pos2::new(plot.min.x, s.y), Pos2::new(plot.max.x, s.y)], Stroke::new(0.5, th.faint().gamma_multiply(0.5)));
            pa.text(Pos2::new(plot.min.x - 7.0, s.y), Align2::RIGHT_CENTER, format!("{v:.yd$}"), font.clone(), th.ink());
        }
    }
    pa.rect_stroke(plot, 2.0, Stroke::new(1.0, th.line()), egui::StrokeKind::Inside);
    pa.text(Pos2::new(plot.center().x, outer.max.y - 6.0), Align2::CENTER_BOTTOM, xlabel, FontId::proportional(12.5), th.ink());
    pa.text(Pos2::new(outer.min.x + 8.0, plot.center().y), Align2::CENTER_CENTER, ylabel, FontId::proportional(12.5), th.ink());
    f
}

fn marker(pa: &egui::Painter, c: Pos2, shape: u8, size: f32, fill: Color32, outline: Color32, selected: bool) {
    let st = Stroke::new(if selected { 2.0 } else { 0.8 }, if selected { Color32::from_rgb(255, 150, 0) } else { outline });
    let s = if selected { size * 1.35 } else { size };
    match shape {
        0 => {
            pa.rect_filled(Rect::from_center_size(c, Vec2::splat(s * 1.7)), 0.0, fill);
            pa.rect_stroke(Rect::from_center_size(c, Vec2::splat(s * 1.7)), 0.0, st, egui::StrokeKind::Middle);
        }
        1 => {
            let pts = vec![Pos2::new(c.x, c.y - s * 1.05), Pos2::new(c.x + s * 1.0, c.y + s * 0.8), Pos2::new(c.x - s * 1.0, c.y + s * 0.8)];
            pa.add(Shape::convex_polygon(pts, fill, st));
        }
        _ => {
            pa.circle_filled(c, s, fill);
            pa.circle_stroke(c, s, st);
        }
    }
}

pub struct StereoOpts {
    pub show: [bool; 3],
    pub size: f32,
    /// Draw the individual data points.
    pub data: bool,
    pub caption: bool,
    pub legend: bool,
    /// Join the points of each axis in file order with a line.
    pub connect: bool,
    /// Magnetic foliation: great circle perpendicular to each K3.
    pub foliation: bool,
}

pub fn stereonet(ui: &mut Ui, rect: Rect, pts: &[Pt], opts: &StereoOpts, th: &Theme, label: &str, view: &mut NetView) -> PlotOut {
    let resp = ui.interact(rect, ui.scope_id().with("stereo"), Sense::click_and_drag());
    let mods = ui.input(|i| i.modifiers);
    let mask = axis_mask(mods);
    // zoom (Ctrl + scroll, or a pinch) about the pointer; plain scroll and middle-drag pan a zoomed net
    if resp.hovered() {
        if let Some(h) = resp.hover_pos() {
            let (zd, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            if (zd - 1.0).abs() > 1e-4 {
                view.zoom_about(rect, h, zd.powf(0.7));
            } else if view.is_zoomed() && scroll != Vec2::ZERO && !(mods.ctrl || mods.command) {
                view.off += scroll;
                view.clamp(rect);
            }
        }
    }
    if resp.dragged_by(egui::PointerButton::Middle) {
        view.off += resp.drag_delta();
        view.clamp(rect);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let (live, done) = if resp.dragged_by(egui::PointerButton::Middle) { (None, None) } else { band(ui, &resp, "band") };
    let pa = ui.painter_at(rect);
    let (c, r) = net_geometry(rect, view);
    pa.circle_filled(c, r, th.canvas());
    pa.circle_stroke(c, r, Stroke::new(1.2, th.line()));
    // tick marks every 10 deg along the N-S and E-W diameters, like the original net
    for deg in (10..90).step_by(10) {
        let (x, _) = project(90.0, deg as f64);
        let rr = r * x as f32;
        for s in [-1.0f32, 1.0] {
            for (dx, dy) in [(1.0f32, 0.0f32), (0.0, 1.0)] {
                let q = Pos2::new(c.x + s * dx * rr, c.y - s * dy * rr);
                let (tx, ty) = (dy * 3.0, dx * 3.0);
                pa.line_segment([Pos2::new(q.x - tx, q.y - ty), Pos2::new(q.x + tx, q.y + ty)], Stroke::new(1.0, th.line()));
            }
        }
    }
    pa.line_segment([Pos2::new(c.x - 4.0, c.y), Pos2::new(c.x + 4.0, c.y)], Stroke::new(1.0, th.line()));
    pa.line_segment([Pos2::new(c.x, c.y - 4.0), Pos2::new(c.x, c.y + 4.0)], Stroke::new(1.0, th.line()));
    let f = FontId::proportional(11.0);
    pa.text(Pos2::new(c.x, c.y - r - 3.0), Align2::CENTER_BOTTOM, "N", f.clone(), th.ink());
    pa.text(Pos2::new(c.x + r + 4.0, c.y), Align2::LEFT_CENTER, "90", f.clone(), th.ink());
    pa.text(Pos2::new(c.x, c.y + r + 3.0), Align2::CENTER_TOP, "180", f.clone(), th.ink());
    pa.text(Pos2::new(c.x - r - 4.0, c.y), Align2::RIGHT_CENTER, "270", f.clone(), th.ink());
    if opts.caption {
        pa.text(Pos2::new(rect.min.x + 10.0, rect.min.y + 10.0), Align2::LEFT_TOP, label, FontId::proportional(12.0), th.ink());
        pa.text(
            Pos2::new(rect.max.x - 10.0, rect.min.y + 10.0),
            Align2::RIGHT_TOP,
            format!("Equal-Area\nProjection\nN = {}", pts.len()),
            FontId::proportional(12.0),
            th.ink(),
        );
    }
    if opts.foliation {
        for p in pts {
            let v = ams_core::tensor::from_dec_inc(p.di[2].0, p.di[2].1);
            let h = if v[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            let a = ams_core::tensor::cross(v, h);
            let na = ams_core::tensor::norm(a);
            let a = [a[0] / na, a[1] / na, a[2] / na];
            let b = ams_core::tensor::cross(v, a);
            let line: Vec<Pos2> = (0..=90)
                .filter_map(|i| {
                    let t = std::f64::consts::TAU * i as f64 / 90.0;
                    let (s, c2) = t.sin_cos();
                    let w = [a[0] * c2 + b[0] * s, a[1] * c2 + b[1] * s, a[2] * c2 + b[2] * s];
                    let (d, inc) = ams_core::tensor::dec_inc(w);
                    let (x, y) = project(d, inc);
                    Some(Pos2::new(c.x + x as f32 * r, c.y - y as f32 * r))
                })
                .collect();
            // the great circle is drawn point by point; break where it re-enters the net on the other side
            let mut seg: Vec<Pos2> = vec![];
            for q in line {
                if seg.last().map_or(false, |l| l.distance(q) > r * 0.6) && seg.len() > 1 {
                    pa.add(Shape::line(std::mem::take(&mut seg), egui::epaint::PathStroke::new(0.7, p.color.unwrap_or(th.k_colors[2]).gamma_multiply(0.55))));
                }
                seg.push(q);
            }
            if seg.len() > 1 {
                pa.add(Shape::line(seg, egui::epaint::PathStroke::new(0.7, p.color.unwrap_or(th.k_colors[2]).gamma_multiply(0.55))));
            }
        }
    }
    if opts.connect {
        for a in 0..3 {
            if !opts.show[a] {
                continue;
            }
            let line: Vec<Pos2> = pts.iter().map(|p| { let (x, y) = project(p.di[a].0, p.di[a].1); Pos2::new(c.x + x as f32 * r, c.y - y as f32 * r) }).collect();
            if line.len() > 1 {
                pa.add(Shape::line(line, egui::epaint::PathStroke::new(0.8, th.k_colors[a].gamma_multiply(0.6))));
            }
        }
    }

    let hover = resp.hover_pos();
    let mut best: Option<(f32, usize, usize)> = None;
    // nearest marker of any shown axis, for the Alt label (the Alt key also restricts selection to K2)
    let mut nearest: Option<(f32, usize, usize, Pos2)> = None;
    let mut inside: Vec<usize> = vec![];
    // draw unselected first, selected on top
    for pass in [false, true] {
        for (pi, p) in pts.iter().enumerate().filter(|(_, p)| p.selected == pass) {
            for a in 0..3 {
                if !opts.show[a] {
                    continue;
                }
                let (x, y) = project(p.di[a].0, p.di[a].1);
                let s = Pos2::new(c.x + x as f32 * r, c.y - y as f32 * r);
                let fill = p.color.map(|c| th.soft(c)).unwrap_or(th.k_colors[a]);
                if opts.data || p.selected {
                    marker(&pa, s, a as u8, opts.size, fill, th.edge(), p.selected);
                }
                if let Some(h) = hover {
                    let d = s.distance(h);
                    if d < 9.0 && nearest.map_or(true, |b| d < b.0) {
                        nearest = Some((d, pi, a, s));
                    }
                }
                if !mask[a] {
                    continue;
                }
                if let Some(h) = hover {
                    let d = s.distance(h);
                    if d < 9.0 && best.map_or(true, |b| d < b.0) {
                        best = Some((d, pi, a));
                    }
                }
                if let Some(b) = done {
                    if b.contains(s) && !inside.contains(&p.idx) {
                        inside.push(p.idx);
                    }
                }
            }
        }
    }
    if opts.legend {
        for (a, name) in ["K1", "K2", "K3"].iter().enumerate() {
            let y = rect.max.y - 54.0 + a as f32 * 15.0 + 8.0;
            marker(&pa, Pos2::new(rect.min.x + 18.0, y), a as u8, 4.0, th.k_colors[a], th.edge(), false);
            pa.text(Pos2::new(rect.min.x + 32.0, y), Align2::LEFT_CENTER, *name, f.clone(), th.ink());
        }
    }
    let mut out = PlotOut::default();
    if let Some(l) = live {
        paint_band(ui, rect, l, th);
    }
    if done.is_some() {
        out.band = Some(inside);
    }
    // Alt + hover: the sub-specimen ID next to the marker, at once
    let mut labelled = false;
    if mods.alt {
        if let Some((_, pi, a, s)) = nearest {
            let p = &pts[pi];
            let text = format!("{}  K{}", p.name, a + 1);
            let font = FontId::proportional(12.5);
            let galley = pa.layout_no_wrap(text, font, th.ink());
            let size = galley.size() + Vec2::new(12.0, 6.0);
            let mut pos = s + Vec2::new(11.0, -size.y - 6.0);
            pos.x = pos.x.min(rect.max.x - size.x - 2.0).max(rect.min.x + 2.0);
            pos.y = pos.y.max(rect.min.y + 2.0).min(rect.max.y - size.y - 2.0);
            let box_rect = Rect::from_min_size(pos, size);
            pa.rect(box_rect, 4.0, th.canvas(), Stroke::new(1.0, th.line()), egui::StrokeKind::Inside);
            pa.galley(pos + Vec2::new(6.0, 3.0), galley, th.ink());
            pa.circle_stroke(s, opts.size + 3.5, Stroke::new(1.6, th.accent));
            labelled = true;
        }
    }
    if view.is_zoomed() {
        pa.text(
            Pos2::new(rect.max.x - 8.0, rect.max.y - 8.0),
            Align2::RIGHT_BOTTOM,
            format!("zoom \u{d7}{:.1}   Esc resets", view.zoom),
            FontId::proportional(11.0),
            th.faint(),
        );
    }
    if let Some((_, pi, a)) = best {
        let p = &pts[pi];
        out.hover = Some(format!("Dec = {:5.1}   Inc = {:4.1}", p.di[a].0, p.di[a].1));
        if !labelled {
            resp.clone().on_hover_text(format!("{} K{}", p.name, a + 1));
        }
        if resp.clicked() {
            out.click = Some(p.idx);
        }
    }
    out
}

/// Exponent used to print a parameter divided by a power of ten (only Kmean).
pub fn scale_exp(param: usize, vals: impl Iterator<Item = f64>) -> i32 {
    if param != 0 {
        return 0;
    }
    let hi = vals.filter(|v| v.is_finite()).map(f64::abs).fold(0.0f64, f64::max);
    if hi > 0.0 { hi.log10().floor() as i32 } else { 0 }
}

fn axis_label(cfg: &AxisCfg, exp: i32) -> String {
    if cfg.param == 0 && !cfg.log {
        format!("Kmean [E{exp:+03} SI]")
    } else if cfg.param == 0 {
        "Kmean [SI]".to_string()
    } else {
        format!("{}{}", cfg.label(), if cfg.log { " (log)" } else { "" })
    }
}

/// Transformed values of one axis: (specimen position in `pts`, value), with the number of values dropped by a log axis.
fn axis_values(pts: &[Pt], cfg: &AxisCfg, scale: f64) -> (Vec<(usize, f64)>, usize) {
    let mut hidden = 0;
    let v = pts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let x = p.vals[cfg.param.min(N_PARAMS - 1)];
            if !x.is_finite() {
                return None;
            }
            if cfg.log {
                if x > 0.0 { Some((i, x.log10())) } else { hidden += 1; None }
            } else {
                Some((i, x * scale))
            }
        })
        .collect();
    (v, hidden)
}

fn axis_range(cfg: &AxisCfg, vals: &[f64], scale: f64, pad: f64) -> (f64, f64) {
    if !cfg.auto {
        let t = |v: f64| if cfg.log { if v > 0.0 { v.log10() } else { f64::NAN } } else { v * scale };
        let (a, b) = (t(cfg.min), t(cfg.max));
        if a.is_finite() && b.is_finite() && b > a {
            return (a, b);
        }
    }
    range_of(vals.iter().copied(), pad)
}

/// Histogram of one parameter (N vs parameter).
pub fn histogram(ui: &mut Ui, rect: Rect, pts: &[Pt], bins: usize, cfg: &AxisCfg, th: &Theme, salt: &str) -> PlotOut {
    let exp = scale_exp(cfg.param, pts.iter().map(|p| p.vals[0]));
    let scale = if cfg.log { 1.0 } else { 10f64.powi(-exp) };
    let (vals, hidden) = axis_values(pts, cfg, scale);
    let (mut lo, mut hi) = axis_range(cfg, &vals.iter().map(|v| v.1).collect::<Vec<_>>(), scale, 0.0);
    if (hi - lo).abs() < 1e-30 {
        lo -= 0.5;
        hi += 0.5;
    }
    let nb = bins.max(1);
    let mut counts = vec![0usize; nb];
    for (_, v) in &vals {
        if *v < lo || *v > hi {
            continue;
        }
        let b = (((v - lo) / (hi - lo)) * nb as f64).floor() as usize;
        counts[b.min(nb - 1)] += 1;
    }
    let ymax = (*counts.iter().max().unwrap_or(&1)).max(1) as f64;
    let f = draw_axes(ui, rect, (lo, hi), (0.0, (ymax * 1.1).ceil()), &axis_label(cfg, exp), "N", th, cfg.log, false);
    let pa = ui.painter_at(rect);
    if hidden > 0 {
        pa.text(Pos2::new(f.plot.max.x - 6.0, f.plot.min.y + 6.0), Align2::RIGHT_TOP, format!("{hidden} values <= 0 not shown"), FontId::proportional(11.0), th.faint());
    }
    let mut out = PlotOut::default();
    let resp = ui.interact(f.plot, ui.scope_id().with(("hist", salt)), Sense::hover());
    let w = (hi - lo) / nb as f64;
    for (i, n) in counts.iter().enumerate() {
        if *n == 0 {
            continue;
        }
        let x0 = lo + i as f64 * w;
        let r = Rect::from_min_max(f.to_screen(x0, *n as f64), f.to_screen(x0 + w, 0.0));
        pa.rect_filled(r, 0.0, th.accent.gamma_multiply(0.75));
        pa.rect_stroke(r, 0.0, Stroke::new(1.0, th.line()), egui::StrokeKind::Inside);
        if resp.hover_pos().map_or(false, |h| r.contains(h)) {
            let mid = x0 + w / 2.0;
            let shown = if cfg.log { 10f64.powf(mid) } else { mid / scale };
            out.hover = Some(format!("{} = {:.3E}   N = {n}", cfg.label(), shown));
        }
    }
    out
}

pub struct ScatterSpec<'a> {
    pub x: &'a AxisCfg,
    pub y: &'a AxisCfg,
    pub zero_line: bool,
    pub diagonal: bool,
    pub conf: bool,
    pub connect: bool,
    /// Join the points in the order of their x values (a line plot) instead of file order.
    pub sort_x: bool,
    pub salt: &'a str,
}

pub fn scatter(ui: &mut Ui, rect: Rect, pts: &[Pt], spec: &ScatterSpec, size: f32, th: &Theme, view: &mut ZoomView) -> PlotOut {
    let xexp = scale_exp(spec.x.param, pts.iter().map(|p| p.vals[0]));
    let yexp = scale_exp(spec.y.param, pts.iter().map(|p| p.vals[0]));
    let (xs_, xh) = axis_values(pts, spec.x, if spec.x.log { 1.0 } else { 10f64.powi(-xexp) });
    let (ys_, yh) = axis_values(pts, spec.y, if spec.y.log { 1.0 } else { 10f64.powi(-yexp) });
    let mut ymap = std::collections::HashMap::new();
    for (i, v) in &ys_ {
        ymap.insert(*i, *v);
    }
    let xs: Vec<(usize, f64, f64)> = xs_.iter().filter_map(|(i, x)| ymap.get(i).map(|y| (*i, *x, *y))).collect();
    let xr = axis_range(spec.x, &xs.iter().map(|v| v.1).collect::<Vec<_>>(), if spec.x.log { 1.0 } else { 10f64.powi(-xexp) }, 0.06);
    let yr = axis_range(spec.y, &xs.iter().map(|v| v.2).collect::<Vec<_>>(), if spec.y.log { 1.0 } else { 10f64.powi(-yexp) }, 0.06);
    // zoom and pan first, so that the axes are drawn for the range that is visible
    let plot0 = plot_rect(rect, spec.y.log);
    let resp = ui.interact(plot0, ui.scope_id().with(spec.salt), Sense::click_and_drag());
    handle_zoom(ui, &resp, plot0, view);
    let (xr, yr) = view.apply(xr, yr);
    let f = draw_axes(ui, rect, xr, yr, &axis_label(spec.x, xexp), &axis_label(spec.y, yexp), th, spec.x.log, spec.y.log);
    let pa = ui.painter_at(rect);
    if xh + yh > 0 {
        pa.text(Pos2::new(f.plot.max.x - 6.0, f.plot.min.y + 6.0), Align2::RIGHT_TOP, format!("{} values <= 0 not shown", xh + yh), FontId::proportional(11.0), th.faint());
    }
    let clip = ui.painter_at(f.plot);
    if spec.zero_line && !spec.y.log && yr.0 < 0.0 && yr.1 > 0.0 {
        clip.line_segment([f.to_screen(xr.0, 0.0), f.to_screen(xr.1, 0.0)], Stroke::new(1.2, th.line()));
    }
    if spec.diagonal {
        // y = x in data units: valid when both axes use the same transformation
        let (a, b) = (xr.0.max(yr.0), xr.1.min(yr.1));
        if b > a {
            clip.line_segment([f.to_screen(a, a), f.to_screen(b, b)], Stroke::new(1.2, th.faint()));
        }
    }
    if spec.connect {
        let mut order: Vec<&(usize, f64, f64)> = xs.iter().collect();
        if spec.sort_x {
            order.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        }
        let pts2: Vec<Pos2> = order.iter().map(|(_, x, y)| f.to_screen(*x, *y)).collect();
        if pts2.len() > 1 {
            clip.add(Shape::line(pts2, egui::epaint::PathStroke::new(1.0, th.faint())));
        }
    }
    let (live, done) = if resp.dragged_by(egui::PointerButton::Middle) { (None, None) } else { band(ui, &resp, "band") };
    let mut inside: Vec<usize> = vec![];
    let hover = resp.hover_pos();
    let mut best: Option<(f32, usize)> = None;
    for pass in [false, true] {
        for (i, x, y) in xs.iter().filter(|(i, _, _)| pts[*i].selected == pass) {
            let s = f.to_screen(*x, *y);
            if spec.conf && !spec.y.log && (9..=11).contains(&spec.y.param) {
                let sd = pts[*i].sds[spec.y.param - 9] * 10f64.powi(-yexp);
                let (a, b) = (f.to_screen(*x, y - sd), f.to_screen(*x, y + sd));
                clip.line_segment([a, b], Stroke::new(1.0, th.line()));
            }
            marker(&clip, s, 2, size, pts[*i].color.map(|c| th.soft(c)).unwrap_or(th.accent), th.edge(), pts[*i].selected);
            if let Some(b) = done {
                if b.contains(s) {
                    inside.push(pts[*i].idx);
                }
            }
            if let Some(h) = hover {
                let d = s.distance(h);
                if d < 9.0 && best.map_or(true, |b| d < b.0) {
                    best = Some((d, *i));
                }
            }
        }
    }
    let mut out = PlotOut::default();
    if let Some(l) = live {
        paint_band(ui, f.plot, l, th);
    }
    if done.is_some() {
        out.band = Some(inside);
    }
    if view.is_zoomed() {
        pa.text(Pos2::new(rect.max.x - 8.0, rect.max.y - 4.0), Align2::RIGHT_BOTTOM, format!("zoom \u{d7}{:.1}   Esc resets", 1.0 / (view.x.1 - view.x.0).min(view.y.1 - view.y.0)), FontId::proportional(11.0), th.faint());
    }
    if xs.is_empty() {
        pa.text(f.plot.center(), Align2::CENTER_CENTER, "No specimens to show", FontId::proportional(13.0), th.faint());
    }
    if let Some((_, i)) = best {
        let p = &pts[i];
        out.hover = Some(format!("{}   {} = {:.4}   {} = {:.4}", p.name, spec.x.label(), p.vals[spec.x.param], spec.y.label(), p.vals[spec.y.param]));
        // Alt + hover: the sub-specimen ID next to the marker, at once
        if ui.input(|i| i.modifiers.alt) {
            if let Some((_, _, y)) = xs.iter().find(|(k, _, _)| *k == i) {
                let x = xs.iter().find(|(k, _, _)| *k == i).map(|v| v.1).unwrap_or(0.0);
                label_box(&pa, f.plot, f.to_screen(x, *y), p.name.clone(), th);
            }
        }
        if resp.clicked() {
            out.click = Some(p.idx);
        }
    }
    out
}

/// Which plot the top right pane draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XyKind {
    Histogram,
    Scatter,
    Line,
    Bar,
    Box,
}

impl XyKind {
    pub const ALL: [XyKind; 5] = [XyKind::Histogram, XyKind::Scatter, XyKind::Line, XyKind::Bar, XyKind::Box];

    pub fn name(self) -> &'static str {
        match self {
            XyKind::Histogram => "Histogram (N vs parameter)",
            XyKind::Scatter => "Scatter plot",
            XyKind::Line => "Line plot",
            XyKind::Bar => "Bar plot (mean of Y per class of X)",
            XyKind::Box => "Box plot (Y per class of X)",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            XyKind::Histogram => "Histogram",
            XyKind::Scatter => "Scatter",
            XyKind::Line => "Line",
            XyKind::Bar => "Bar",
            XyKind::Box => "Box",
        }
    }
}

/// How a bar or box plot forms its groups.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupBy {
    /// Equal classes of the X parameter.
    Classes,
    /// One group per sample (the specimen name without its sub-specimen part, ZZ01 for ZZ01-A1).
    Sample,
}

/// Bar plot (mean and SD of Y) or box plot (quartiles, whiskers at 1.5 IQR, outliers) of Y per group: classes of X,
/// or one group per sample.
pub fn binned_plot(ui: &mut Ui, rect: Rect, pts: &[Pt], kind: XyKind, x: &AxisCfg, y: &AxisCfg, bins: usize, group: GroupBy, th: &Theme, view: &mut ZoomView, salt: &str) -> PlotOut {
    let xexp = scale_exp(x.param, pts.iter().map(|p| p.vals[0]));
    let yexp = scale_exp(y.param, pts.iter().map(|p| p.vals[0]));
    let (xscale, yscale) = (if x.log { 1.0 } else { 10f64.powi(-xexp) }, if y.log { 1.0 } else { 10f64.powi(-yexp) });
    let (xs_, _) = axis_values(pts, x, xscale);
    let (ys_, _) = axis_values(pts, y, yscale);
    let ymap: std::collections::HashMap<usize, f64> = ys_.into_iter().collect();
    let data: Vec<(usize, f64, f64)> = xs_.iter().filter_map(|(i, xv)| ymap.get(i).map(|yv| (*i, *xv, *yv))).collect();
    let by_sample = group == GroupBy::Sample;
    let mut labels: Vec<String> = vec![];
    let (xlo, xhi, nb, mut groups): (f64, f64, usize, Vec<Vec<f64>>) = if by_sample {
        // one group per sample, in the order a person reads the names
        let mut map: Vec<(String, Vec<f64>)> = vec![];
        for (i, _, yv) in &data {
            let sample = ams_core::recombine::split_id(&pts[*i].name).0;
            match map.iter_mut().find(|(n, _)| *n == sample) {
                Some((_, v)) => v.push(*yv),
                None => map.push((sample, vec![*yv])),
            }
        }
        map.sort_by(|a, b| ams_core::recombine::natural_cmp(&a.0, &b.0));
        let n = map.len().max(1);
        labels = map.iter().map(|(n, _)| n.clone()).collect();
        (0.0, n as f64, n, map.into_iter().map(|(_, v)| v).collect())
    } else {
        let (a, b) = axis_range(x, &data.iter().map(|v| v.1).collect::<Vec<_>>(), xscale, 0.0);
        let (lo, hi) = if (b - a).abs() < 1e-30 { (a - 0.5, b + 0.5) } else { (a, b) };
        let nb = bins.clamp(1, 60);
        let w = (hi - lo) / nb as f64;
        let mut g: Vec<Vec<f64>> = vec![vec![]; nb];
        for (_, xv, yv) in &data {
            if *xv < lo || *xv > hi {
                continue;
            }
            g[(((xv - lo) / w).floor() as usize).min(nb - 1)].push(*yv);
        }
        (lo, hi, nb, g)
    };
    let w = (xhi - xlo) / nb as f64;
    for g in &mut groups {
        g.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
    let quant = |g: &[f64], q: f64| -> f64 {
        let pos = q * (g.len() - 1) as f64;
        let (i, fr) = (pos.floor() as usize, pos - pos.floor());
        if i + 1 < g.len() { g[i] * (1.0 - fr) + g[i + 1] * fr } else { g[i] }
    };
    let mean_sd = |g: &[f64]| -> (f64, f64) {
        let m = g.iter().sum::<f64>() / g.len() as f64;
        let sd = if g.len() > 1 { (g.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (g.len() - 1) as f64).sqrt() } else { 0.0 };
        (m, sd)
    };
    // the Y range that holds everything that is drawn
    let mut ext: Vec<f64> = vec![];
    for g in groups.iter().filter(|g| !g.is_empty()) {
        if kind == XyKind::Bar {
            let (m, sd) = mean_sd(g);
            ext.extend([m - sd, m + sd, 0.0]);
        } else {
            ext.extend([g[0], g[g.len() - 1]]);
        }
    }
    let yr = if y.auto || ext.is_empty() { range_of(ext.iter().copied(), 0.08) } else { axis_range(y, &[], yscale, 0.0) };
    let plot0 = plot_rect(rect, y.log);
    let resp = ui.interact(plot0, ui.scope_id().with(salt), Sense::click_and_drag());
    handle_zoom(ui, &resp, plot0, view);
    let (xr, yr) = view.apply((xlo, xhi), yr);
    let xlabel = if by_sample { "Sample (specimen name)".to_string() } else { axis_label(x, xexp) };
    let f = draw_axes_ex(ui, rect, xr, yr, &xlabel, &axis_label(y, yexp), th, x.log && !by_sample, y.log, by_sample);
    let pa = ui.painter_at(f.plot);
    let mut out = PlotOut::default();
    let hover = resp.hover_pos();
    let base = f.to_screen(xr.0, 0.0_f64.clamp(yr.0, yr.1)).y;
    if by_sample {
        // the sample names under their groups (every other one when they would touch)
        let outer = ui.painter_at(rect);
        let step_px = f.to_screen(1.0, 0.0).x - f.to_screen(0.0, 0.0).x;
        let skip = ((62.0 / step_px.max(1.0)).ceil() as usize).max(1);
        for (b, l) in labels.iter().enumerate() {
            let cx = f.to_screen(b as f64 + 0.5, 0.0).x;
            if b % skip == 0 && cx > f.plot.min.x && cx < f.plot.max.x {
                outer.text(Pos2::new(cx, f.plot.max.y + 6.0), Align2::CENTER_TOP, l, FontId::proportional(11.0), th.ink());
            }
        }
    }
    for (b, g) in groups.iter().enumerate() {
        if g.is_empty() {
            continue;
        }
        let (x0, x1) = (xlo + b as f64 * w, xlo + (b + 1) as f64 * w);
        let pad = w * 0.12;
        let (sl, sr) = (f.to_screen(x0 + pad, 0.0).x, f.to_screen(x1 - pad, 0.0).x);
        let mid = (sl + sr) / 2.0;
        let fill = th.accent.gamma_multiply(0.75);
        let stroke = Stroke::new(1.2, th.line());
        let info;
        if kind == XyKind::Bar {
            let (m, sd) = mean_sd(g);
            let top = f.to_screen(x0, m).y;
            let r = Rect::from_min_max(Pos2::new(sl, top.min(base)), Pos2::new(sr, top.max(base)));
            pa.rect_filled(r, 0.0, fill);
            pa.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
            let (a, c) = (f.to_screen(x0, m - sd).y, f.to_screen(x0, m + sd).y);
            pa.line_segment([Pos2::new(mid, a), Pos2::new(mid, c)], stroke);
            for yy in [a, c] {
                pa.line_segment([Pos2::new(mid - 4.0, yy), Pos2::new(mid + 4.0, yy)], stroke);
            }
            info = if by_sample { format!("{}: N = {}   mean = {:.4}   SD = {:.4}", labels.get(b).cloned().unwrap_or_default(), g.len(), m / yscale, sd / yscale) } else { format!("{} {:.3} to {:.3}: N = {}   mean = {:.4}   SD = {:.4}", x.label(), x0 / xscale, x1 / xscale, g.len(), m / yscale, sd / yscale) };
        } else {
            let (q1, med, q3) = (quant(g, 0.25), quant(g, 0.5), quant(g, 0.75));
            let iqr = q3 - q1;
            let lo_w = g.iter().copied().find(|v| *v >= q1 - 1.5 * iqr).unwrap_or(g[0]);
            let hi_w = g.iter().rev().copied().find(|v| *v <= q3 + 1.5 * iqr).unwrap_or(g[g.len() - 1]);
            let r = Rect::from_min_max(Pos2::new(sl, f.to_screen(x0, q3).y), Pos2::new(sr, f.to_screen(x0, q1).y));
            pa.rect_filled(r, 0.0, fill);
            pa.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
            pa.line_segment([Pos2::new(sl, f.to_screen(x0, med).y), Pos2::new(sr, f.to_screen(x0, med).y)], Stroke::new(2.0, th.ink()));
            pa.line_segment([Pos2::new(mid, f.to_screen(x0, q3).y), Pos2::new(mid, f.to_screen(x0, hi_w).y)], stroke);
            pa.line_segment([Pos2::new(mid, f.to_screen(x0, q1).y), Pos2::new(mid, f.to_screen(x0, lo_w).y)], stroke);
            for v in [lo_w, hi_w] {
                pa.line_segment([Pos2::new(mid - 5.0, f.to_screen(x0, v).y), Pos2::new(mid + 5.0, f.to_screen(x0, v).y)], stroke);
            }
            for v in g.iter().filter(|v| **v < lo_w || **v > hi_w) {
                pa.circle_stroke(Pos2::new(mid, f.to_screen(x0, *v).y), 2.5, Stroke::new(1.0, th.line()));
            }
            let head = if by_sample { labels.get(b).cloned().unwrap_or_default() } else { format!("{} {:.3} to {:.3}", x.label(), x0 / xscale, x1 / xscale) };
            info = format!("{head}: N = {}   median = {:.4}   Q1 = {:.4}   Q3 = {:.4}   min = {:.4}   max = {:.4}", g.len(), med / yscale, q1 / yscale, q3 / yscale, g[0] / yscale, g[g.len() - 1] / yscale);
        }
        if hover.map_or(false, |h| h.x >= f.to_screen(x0, 0.0).x && h.x <= f.to_screen(x1, 0.0).x && f.plot.contains(h)) {
            out.hover = Some(info);
        }
    }
    if view.is_zoomed() {
        ui.painter_at(rect).text(Pos2::new(rect.max.x - 8.0, rect.max.y - 4.0), Align2::RIGHT_BOTTOM, format!("zoom \u{d7}{:.1}   Esc resets", 1.0 / (view.x.1 - view.x.0).min(view.y.1 - view.y.0)), FontId::proportional(11.0), th.faint());
    }
    if data.is_empty() {
        pa.text(f.plot.center(), Align2::CENTER_CENTER, "No specimens to show", FontId::proportional(13.0), th.faint());
    }
    out
}

/// Zoom and pan of the stereonet (Ctrl + scroll zooms about the pointer, middle-drag or scroll pans, Esc resets).
#[derive(Clone, Copy, PartialEq)]
pub struct NetView {
    pub zoom: f32,
    /// Shift of the net centre from the middle of the plot, in points.
    pub off: Vec2,
}

impl Default for NetView {
    fn default() -> Self {
        NetView { zoom: 1.0, off: Vec2::ZERO }
    }
}

impl NetView {
    pub const MAX_ZOOM: f32 = 60.0;

    pub fn is_zoomed(&self) -> bool {
        self.zoom > 1.0001
    }

    pub fn reset(&mut self) {
        *self = NetView::default();
    }

    /// Zooms by `factor`, keeping the point `p` (in screen coordinates) where it is.
    fn zoom_about(&mut self, rect: Rect, p: Pos2, factor: f32) {
        let new = (self.zoom * factor).clamp(1.0, Self::MAX_ZOOM);
        let k = new / self.zoom;
        let centre = rect.center();
        let c_old = centre + self.off;
        let c_new = p - (p - c_old) * k;
        self.zoom = new;
        self.off = c_new - centre;
        self.clamp(rect);
    }

    /// Keeps the net from being pushed completely out of the plot.
    fn clamp(&mut self, rect: Rect) {
        if !self.is_zoomed() {
            self.reset();
            return;
        }
        let r = base_radius(rect) * self.zoom;
        self.off.x = self.off.x.clamp(-r, r);
        self.off.y = self.off.y.clamp(-r, r);
    }
}

fn base_radius(rect: Rect) -> f32 {
    (rect.width().min(rect.height()) / 2.0 - 26.0).max(20.0)
}

/// Centre and radius of the net drawn by [`stereonet`] inside `rect` for a zoom / pan state.
pub fn net_geometry(rect: Rect, view: &NetView) -> (Pos2, f32) {
    (rect.center() + view.off, base_radius(rect) * view.zoom)
}
