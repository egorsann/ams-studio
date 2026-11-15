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
fn band(ui: &mut Ui, resp: &Response, key: &str) -> (Option<Rect>, Option<Rect>) {
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

fn paint_band(ui: &Ui, clip: Rect, r: Rect, th: &Theme) {
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
}

impl Default for ColorCfg {
    fn default() -> Self {
        ColorCfg { on: false, param: 0, auto: true, min: 0.0, max: 1.0, log: false, palette: 0 }
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
    pa.rect_stroke(bar, 0.0, Stroke::new(1.0, th.ink()), egui::StrokeKind::Outside);
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
        if self.dark { Color32::from_gray(220) } else { Color32::from_gray(30) }
    }
    pub fn faint(&self) -> Color32 {
        if self.dark { Color32::from_gray(90) } else { Color32::from_gray(190) }
    }
    pub fn canvas(&self) -> Color32 {
        if self.dark { Color32::from_gray(24) } else { Color32::WHITE }
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
    let pa = ui.painter_at(outer);
    let plot = Rect::from_min_max(
        Pos2::new(outer.min.x + if ylog { 62.0 } else { 54.0 }, outer.min.y + 12.0),
        Pos2::new(outer.max.x - 16.0, outer.max.y - 46.0),
    );
    let f = Frame2D { plot, x, y };
    pa.rect_filled(plot, 2.0, th.canvas());
    let font = FontId::proportional(11.0);
    if xlog {
        log_ticks(&pa, x.0, x.1, |v, major, label| {
            let s = f.to_screen(v, y.0);
            pa.line_segment([Pos2::new(s.x, plot.max.y), Pos2::new(s.x, plot.max.y + if major { 5.0 } else { 3.0 })], Stroke::new(1.0, th.ink()));
            pa.line_segment([Pos2::new(s.x, plot.min.y), Pos2::new(s.x, plot.max.y)], Stroke::new(0.5, th.faint().gamma_multiply(if major { 0.5 } else { 0.25 })));
            if let Some(l) = label {
                pa.text(Pos2::new(s.x, plot.max.y + 7.0), Align2::CENTER_TOP, l, font.clone(), th.ink());
            }
        });
    } else {
        let (xt, xd) = nice_ticks(x.0, x.1, (plot.width() / 70.0) as usize);
        for v in xt {
            let s = f.to_screen(v, y.0);
            pa.line_segment([Pos2::new(s.x, plot.max.y), Pos2::new(s.x, plot.max.y + 4.0)], Stroke::new(1.0, th.ink()));
            pa.line_segment([Pos2::new(s.x, plot.min.y), Pos2::new(s.x, plot.max.y)], Stroke::new(0.5, th.faint().gamma_multiply(0.5)));
            pa.text(Pos2::new(s.x, plot.max.y + 6.0), Align2::CENTER_TOP, format!("{v:.xd$}"), font.clone(), th.ink());
        }
    }
    if ylog {
        log_ticks(&pa, y.0, y.1, |v, major, label| {
            let s = f.to_screen(x.0, v);
            pa.line_segment([Pos2::new(plot.min.x - if major { 5.0 } else { 3.0 }, s.y), Pos2::new(plot.min.x, s.y)], Stroke::new(1.0, th.ink()));
            pa.line_segment([Pos2::new(plot.min.x, s.y), Pos2::new(plot.max.x, s.y)], Stroke::new(0.5, th.faint().gamma_multiply(if major { 0.5 } else { 0.25 })));
            if let Some(l) = label {
                pa.text(Pos2::new(plot.min.x - 7.0, s.y), Align2::RIGHT_CENTER, l, font.clone(), th.ink());
            }
        });
    } else {
        let (yt, yd) = nice_ticks(y.0, y.1, (plot.height() / 36.0) as usize);
        for v in yt {
            let s = f.to_screen(x.0, v);
            pa.line_segment([Pos2::new(plot.min.x - 4.0, s.y), Pos2::new(plot.min.x, s.y)], Stroke::new(1.0, th.ink()));
            pa.line_segment([Pos2::new(plot.min.x, s.y), Pos2::new(plot.max.x, s.y)], Stroke::new(0.5, th.faint().gamma_multiply(0.5)));
            pa.text(Pos2::new(plot.min.x - 7.0, s.y), Align2::RIGHT_CENTER, format!("{v:.yd$}"), font.clone(), th.ink());
        }
    }
    pa.rect_stroke(plot, 2.0, Stroke::new(1.0, th.ink()), egui::StrokeKind::Inside);
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

pub fn stereonet(ui: &mut Ui, rect: Rect, pts: &[Pt], opts: &StereoOpts, th: &Theme, label: &str) -> PlotOut {
    let resp = ui.interact(rect, ui.scope_id().with("stereo"), Sense::click_and_drag());
    let mask = axis_mask(ui.input(|i| i.modifiers));
    let (live, done) = band(ui, &resp, "band");
    let pa = ui.painter_at(rect);
    let r = (rect.width().min(rect.height()) / 2.0 - 26.0).max(20.0);
    let c = rect.center();
    pa.circle_filled(c, r, th.canvas());
    pa.circle_stroke(c, r, Stroke::new(1.2, th.ink()));
    // tick marks every 10 deg along the N-S and E-W diameters, like the original net
    for deg in (10..90).step_by(10) {
        let (x, _) = project(90.0, deg as f64);
        let rr = r * x as f32;
        for s in [-1.0f32, 1.0] {
            for (dx, dy) in [(1.0f32, 0.0f32), (0.0, 1.0)] {
                let q = Pos2::new(c.x + s * dx * rr, c.y - s * dy * rr);
                let (tx, ty) = (dy * 3.0, dx * 3.0);
                pa.line_segment([Pos2::new(q.x - tx, q.y - ty), Pos2::new(q.x + tx, q.y + ty)], Stroke::new(1.0, th.ink()));
            }
        }
    }
    pa.line_segment([Pos2::new(c.x - 4.0, c.y), Pos2::new(c.x + 4.0, c.y)], Stroke::new(1.0, th.ink()));
    pa.line_segment([Pos2::new(c.x, c.y - 4.0), Pos2::new(c.x, c.y + 4.0)], Stroke::new(1.0, th.ink()));
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
                let fill = p.color.unwrap_or(th.k_colors[a]);
                if opts.data || p.selected {
                    marker(&pa, s, a as u8, opts.size, fill, th.ink(), p.selected);
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
            marker(&pa, Pos2::new(rect.min.x + 18.0, y), a as u8, 4.0, th.k_colors[a], th.ink(), false);
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
    if let Some((_, pi, a)) = best {
        let p = &pts[pi];
        out.hover = Some(format!("Dec = {:5.1}   Inc = {:4.1}", p.di[a].0, p.di[a].1));
        resp.clone().on_hover_text(format!("{} K{}", p.name, a + 1));
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
pub fn histogram(ui: &mut Ui, rect: Rect, pts: &[Pt], bins: usize, cfg: &AxisCfg, th: &Theme) -> PlotOut {
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
    let resp = ui.interact(f.plot, ui.scope_id().with("hist"), Sense::hover());
    let w = (hi - lo) / nb as f64;
    for (i, n) in counts.iter().enumerate() {
        if *n == 0 {
            continue;
        }
        let x0 = lo + i as f64 * w;
        let r = Rect::from_min_max(f.to_screen(x0, *n as f64), f.to_screen(x0 + w, 0.0));
        pa.rect_filled(r, 0.0, th.accent.gamma_multiply(0.75));
        pa.rect_stroke(r, 0.0, Stroke::new(1.0, th.ink()), egui::StrokeKind::Inside);
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
    pub salt: &'a str,
}

pub fn scatter(ui: &mut Ui, rect: Rect, pts: &[Pt], spec: &ScatterSpec, size: f32, th: &Theme) -> PlotOut {
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
    let f = draw_axes(ui, rect, xr, yr, &axis_label(spec.x, xexp), &axis_label(spec.y, yexp), th, spec.x.log, spec.y.log);
    let pa = ui.painter_at(rect);
    if xh + yh > 0 {
        pa.text(Pos2::new(f.plot.max.x - 6.0, f.plot.min.y + 6.0), Align2::RIGHT_TOP, format!("{} values <= 0 not shown", xh + yh), FontId::proportional(11.0), th.faint());
    }
    let clip = ui.painter_at(f.plot);
    if spec.zero_line && !spec.y.log && yr.0 < 0.0 && yr.1 > 0.0 {
        clip.line_segment([f.to_screen(xr.0, 0.0), f.to_screen(xr.1, 0.0)], Stroke::new(1.2, th.ink()));
    }
    if spec.diagonal {
        // y = x in data units: valid when both axes use the same transformation
        let (a, b) = (xr.0.max(yr.0), xr.1.min(yr.1));
        if b > a {
            clip.line_segment([f.to_screen(a, a), f.to_screen(b, b)], Stroke::new(1.2, th.faint()));
        }
    }
    if spec.connect {
        let pts2: Vec<Pos2> = xs.iter().map(|(_, x, y)| f.to_screen(*x, *y)).collect();
        if pts2.len() > 1 {
            clip.add(Shape::line(pts2, egui::epaint::PathStroke::new(1.0, th.faint())));
        }
    }
    let resp = ui.interact(f.plot, ui.scope_id().with(spec.salt), Sense::click_and_drag());
    let (live, done) = band(ui, &resp, "band");
    let mut inside: Vec<usize> = vec![];
    let hover = resp.hover_pos();
    let mut best: Option<(f32, usize)> = None;
    for pass in [false, true] {
        for (i, x, y) in xs.iter().filter(|(i, _, _)| pts[*i].selected == pass) {
            let s = f.to_screen(*x, *y);
            if spec.conf && !spec.y.log && (9..=11).contains(&spec.y.param) {
                let sd = pts[*i].sds[spec.y.param - 9] * 10f64.powi(-yexp);
                let (a, b) = (f.to_screen(*x, y - sd), f.to_screen(*x, y + sd));
                clip.line_segment([a, b], Stroke::new(1.0, th.ink()));
            }
            marker(&clip, s, 2, size, pts[*i].color.unwrap_or(th.accent), th.ink(), pts[*i].selected);
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
    if let Some((_, i)) = best {
        let p = &pts[i];
        out.hover = Some(format!("{}   {} = {:.4}   {} = {:.4}", p.name, spec.x.label(), p.vals[spec.x.param], spec.y.label(), p.vals[spec.y.param]));
        if resp.clicked() {
            out.click = Some(p.idx);
        }
    }
    out
}

/// Centre and radius of the net drawn by [`stereonet`] inside `rect`.
pub fn net_geometry(rect: Rect) -> (Pos2, f32) {
    (rect.center(), (rect.width().min(rect.height()) / 2.0 - 26.0).max(20.0))
}
