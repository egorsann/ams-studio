//! Flat vector toolbar icons, painted with egui primitives (no image assets).

use egui::{epaint::PathStroke, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Open,
    Prev,
    Next,
    Reload,
    Save,
    Append,
    Export,
    EditData,
    SetClasses,
    Invert,
    Disregard,
    Restore,
    ClearSel,
    CopyGraphics,
    SaveGraphics,
    DataTable,
    InPhase,
    OutPhase,
    Coord(char),
    PaleoH,
    PaleoV,
    GroupStats,
    Contour,
}

fn p(r: Rect, x: f32, y: f32) -> Pos2 {
    Pos2::new(r.min.x + r.width() * x, r.min.y + r.height() * y)
}

fn poly(pa: &Painter, pts: Vec<Pos2>, fill: Color32, stroke: Stroke) {
    pa.add(Shape::convex_polygon(pts, fill, stroke));
}

fn line(pa: &Painter, pts: Vec<Pos2>, w: f32, c: Color32) {
    pa.add(Shape::line(pts, PathStroke::new(w, c)));
}

fn arc(r: Rect, cx: f32, cy: f32, rad: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    (0..=24)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / 24.0;
            p(r, cx + rad * a.cos(), cy + rad * a.sin())
        })
        .collect()
}

fn rows(pa: &Painter, r: Rect, marks: [Color32; 4], extra: Option<Color32>) {
    for i in 0..4 {
        let y = 0.12 + 0.2 * i as f32;
        let row = Rect::from_min_max(p(r, 0.08, y), p(r, 0.92, y + 0.15));
        pa.rect_filled(row, 1.5, Color32::from_gray(238));
        pa.rect_stroke(row, 1.5, Stroke::new(0.8, Color32::from_gray(120)), egui::StrokeKind::Inside);
        pa.rect_filled(Rect::from_min_max(p(r, 0.12, y + 0.025), p(r, 0.3, y + 0.125)), 1.0, marks[i]);
    }
    if let Some(c) = extra {
        pa.circle_filled(p(r, 0.78, 0.52), r.width() * 0.2, c);
    }
}

pub fn paint(pa: &Painter, r: Rect, icon: Icon, dark: bool) {
    let ink = if dark { Color32::from_gray(225) } else { Color32::from_gray(50) };
    let thin = Stroke::new(1.0, Color32::from_gray(90));
    match icon {
        Icon::Open => {
            let yellow = Color32::from_rgb(242, 190, 60);
            poly(pa, vec![p(r, 0.08, 0.28), p(r, 0.38, 0.28), p(r, 0.46, 0.38), p(r, 0.9, 0.38), p(r, 0.9, 0.8), p(r, 0.08, 0.8)], Color32::from_rgb(214, 160, 40), thin);
            poly(pa, vec![p(r, 0.2, 0.5), p(r, 0.98, 0.5), p(r, 0.86, 0.82), p(r, 0.08, 0.82)], yellow, thin);
        }
        Icon::Prev | Icon::Next => {
            let (c, s) = if icon == Icon::Prev { (Color32::from_rgb(232, 90, 90), 1.0) } else { (Color32::from_rgb(40, 110, 220), -1.0) };
            let x = |v: f32| if s > 0.0 { v } else { 1.0 - v };
            poly(pa, vec![p(r, x(0.08), 0.5), p(r, x(0.5), 0.12), p(r, x(0.5), 0.34), p(r, x(0.92), 0.34), p(r, x(0.92), 0.66), p(r, x(0.5), 0.66), p(r, x(0.5), 0.88)], c, Stroke::new(1.0, c.gamma_multiply(0.6)));
        }
        Icon::Reload => {
            let g = Color32::from_rgb(40, 160, 90);
            line(pa, arc(r, 0.5, 0.52, 0.3, -2.2, 3.6), 4.0, g);
            poly(pa, vec![p(r, 0.62, 0.05), p(r, 0.95, 0.3), p(r, 0.55, 0.38)], g, Stroke::NONE);
        }
        Icon::Save | Icon::Append => {
            let body = Color32::from_rgb(70, 90, 110);
            poly(pa, vec![p(r, 0.1, 0.1), p(r, 0.78, 0.1), p(r, 0.92, 0.24), p(r, 0.92, 0.9), p(r, 0.1, 0.9)], body, thin);
            pa.rect_filled(Rect::from_min_max(p(r, 0.26, 0.1), p(r, 0.7, 0.4)), 0.0, Color32::from_gray(235));
            pa.rect_filled(Rect::from_min_max(p(r, 0.22, 0.56), p(r, 0.8, 0.9)), 0.0, Color32::from_rgb(245, 205, 70));
            if icon == Icon::Append {
                pa.circle_filled(p(r, 0.78, 0.76), r.width() * 0.22, Color32::from_rgb(40, 160, 90));
                line(pa, vec![p(r, 0.78, 0.64), p(r, 0.78, 0.88)], 2.0, Color32::WHITE);
                line(pa, vec![p(r, 0.66, 0.76), p(r, 0.9, 0.76)], 2.0, Color32::WHITE);
            }
        }
        Icon::Export => {
            let o = Color32::from_rgb(238, 150, 40);
            poly(pa, vec![p(r, 0.08, 0.18), p(r, 0.62, 0.18), p(r, 0.62, 0.3), p(r, 0.2, 0.3), p(r, 0.2, 0.8), p(r, 0.7, 0.8), p(r, 0.7, 0.6), p(r, 0.82, 0.6), p(r, 0.82, 0.92), p(r, 0.08, 0.92)], o, thin);
            poly(pa, vec![p(r, 0.5, 0.5), p(r, 0.72, 0.5), p(r, 0.72, 0.36), p(r, 0.96, 0.58), p(r, 0.72, 0.8), p(r, 0.72, 0.66), p(r, 0.5, 0.66)], Color32::from_rgb(250, 220, 90), thin);
        }
        Icon::EditData => {
            line(pa, vec![p(r, 0.22, 0.8), p(r, 0.78, 0.24)], 6.0, Color32::from_rgb(240, 175, 60));
            poly(pa, vec![p(r, 0.12, 0.92), p(r, 0.2, 0.68), p(r, 0.34, 0.82)], Color32::from_rgb(235, 210, 170), thin);
            line(pa, vec![p(r, 0.7, 0.16), p(r, 0.86, 0.32)], 6.0, Color32::from_rgb(230, 90, 120));
        }
        Icon::SetClasses => {
            let cols = [Color32::from_rgb(230, 70, 70), Color32::from_rgb(240, 190, 50), Color32::from_rgb(70, 190, 100), Color32::from_rgb(60, 130, 230)];
            for (i, c) in cols.iter().enumerate() {
                let a0 = i as f32 * std::f32::consts::FRAC_PI_2;
                let mut pts = vec![p(r, 0.5, 0.5)];
                pts.extend(arc(r, 0.5, 0.5, 0.42, a0, a0 + std::f32::consts::FRAC_PI_2));
                poly(pa, pts, *c, Stroke::NONE);
            }
            pa.circle_stroke(p(r, 0.5, 0.5), r.width() * 0.42, Stroke::new(1.0, Color32::from_gray(90)));
        }
        Icon::Invert => rows(pa, r, [Color32::from_rgb(240, 200, 60), Color32::from_gray(200), Color32::from_rgb(240, 200, 60), Color32::from_gray(200)], None),
        Icon::Disregard => rows(pa, r, [Color32::from_rgb(240, 200, 60); 4], Some(Color32::from_rgb(220, 60, 60))),
        Icon::Restore => rows(pa, r, [Color32::from_rgb(240, 200, 60); 4], Some(Color32::from_rgb(50, 170, 90))),
        Icon::ClearSel => rows(pa, r, [Color32::from_rgb(240, 200, 60); 4], None),
        Icon::CopyGraphics => {
            pa.rect_filled(Rect::from_min_max(p(r, 0.08, 0.2), p(r, 0.6, 0.9)), 1.5, Color32::from_gray(245));
            pa.rect_stroke(Rect::from_min_max(p(r, 0.08, 0.2), p(r, 0.6, 0.9)), 1.5, thin, egui::StrokeKind::Inside);
            pa.rect_filled(Rect::from_min_max(p(r, 0.36, 0.08), p(r, 0.92, 0.78)), 1.5, Color32::from_rgb(120, 200, 130));
            pa.rect_stroke(Rect::from_min_max(p(r, 0.36, 0.08), p(r, 0.92, 0.78)), 1.5, thin, egui::StrokeKind::Inside);
            for i in 0..3 {
                let y = 0.26 + 0.14 * i as f32;
                line(pa, vec![p(r, 0.46, y), p(r, 0.82, y)], 1.2, Color32::from_gray(60));
            }
        }
        Icon::SaveGraphics => {
            poly(pa, vec![p(r, 0.06, 0.3), p(r, 0.36, 0.3), p(r, 0.44, 0.4), p(r, 0.9, 0.4), p(r, 0.9, 0.86), p(r, 0.06, 0.86)], Color32::from_rgb(150, 130, 40), thin);
            poly(pa, vec![p(r, 0.16, 0.5), p(r, 0.98, 0.5), p(r, 0.86, 0.86), p(r, 0.06, 0.86)], Color32::from_rgb(200, 175, 60), thin);
            poly(pa, vec![p(r, 0.42, 0.04), p(r, 0.66, 0.04), p(r, 0.66, 0.34), p(r, 0.78, 0.34), p(r, 0.54, 0.62), p(r, 0.3, 0.34), p(r, 0.42, 0.34)], Color32::from_rgb(70, 170, 90), thin);
        }
        Icon::DataTable => {
            let b = Color32::from_rgb(60, 120, 210);
            pa.rect_filled(Rect::from_min_max(p(r, 0.06, 0.16), p(r, 0.94, 0.84)), 1.5, Color32::from_gray(250));
            pa.rect_filled(Rect::from_min_max(p(r, 0.06, 0.16), p(r, 0.94, 0.34)), 1.5, b);
            for i in 1..4 {
                line(pa, vec![p(r, 0.06 + 0.22 * i as f32, 0.16), p(r, 0.06 + 0.22 * i as f32, 0.84)], 1.0, b);
            }
            for i in 0..3 {
                let y = 0.34 + 0.16 * i as f32;
                line(pa, vec![p(r, 0.06, y), p(r, 0.94, y)], 1.0, b);
            }
            pa.rect_stroke(Rect::from_min_max(p(r, 0.06, 0.16), p(r, 0.94, 0.84)), 1.5, Stroke::new(1.2, b), egui::StrokeKind::Inside);
        }
        Icon::InPhase | Icon::OutPhase => {
            let c = if icon == Icon::InPhase { Color32::from_rgb(60, 80, 220) } else { Color32::from_rgb(220, 70, 130) };
            pa.rect_filled(r.shrink(2.0), 2.0, Color32::from_gray(250));
            pa.rect_stroke(r.shrink(2.0), 2.0, thin, egui::StrokeKind::Inside);
            line(pa, vec![p(r, 0.1, 0.5), p(r, 0.9, 0.5)], 0.8, Color32::from_gray(150));
            let ph = if icon == Icon::InPhase { 0.0 } else { 1.2 };
            let pts: Vec<Pos2> = (0..=30).map(|i| { let t = i as f32 / 30.0; p(r, 0.1 + 0.8 * t, 0.5 - 0.28 * (t * 6.28 + ph).sin()) }).collect();
            line(pa, pts, 1.8, c);
        }
        Icon::Coord(ch) => {
            let col = Color32::from_rgb(214, 48, 48);
            pa.circle_filled(r.center(), r.width() * 0.46, col);
            pa.circle_stroke(r.center(), r.width() * 0.46, Stroke::new(1.5, Color32::from_rgb(150, 25, 25)));
            pa.text(r.center(), Align2::CENTER_CENTER, ch.to_string(), FontId::proportional(r.width() * 0.62), Color32::WHITE);
        }
        Icon::PaleoH => {
            pa.add(Shape::convex_polygon(arc(r, 0.5, 0.3, 0.4, 0.0, std::f32::consts::PI), Color32::from_rgb(40, 130, 200), Stroke::new(1.0, thin.color)));
            pa.rect_filled(Rect::from_min_max(p(r, 0.1, 0.28), p(r, 0.9, 0.38)), 1.0, Color32::from_rgb(245, 205, 70));
        }
        Icon::PaleoV => {
            pa.add(Shape::convex_polygon(arc(r, 0.5, 0.5, 0.42, -std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2), Color32::from_rgb(245, 205, 70), Stroke::new(1.0, thin.color)));
            pa.rect_filled(Rect::from_min_max(p(r, 0.46, 0.08), p(r, 0.56, 0.92)), 1.0, Color32::from_rgb(40, 100, 190));
        }
        Icon::GroupStats => {
            pa.circle_filled(r.center(), r.width() * 0.44, Color32::from_rgb(240, 160, 50));
            let mut pts = vec![r.center()];
            pts.extend(arc(r, 0.5, 0.5, 0.44, -1.2, 0.9));
            poly(pa, pts, Color32::from_rgb(70, 170, 90), Stroke::NONE);
            let mut pts = vec![r.center()];
            pts.extend(arc(r, 0.5, 0.5, 0.44, 0.9, 2.2));
            poly(pa, pts, Color32::from_rgb(60, 120, 220), Stroke::NONE);
            pa.circle_stroke(r.center(), r.width() * 0.44, thin);
        }
        Icon::Contour => {
            pa.circle_filled(r.center(), r.width() * 0.44, Color32::from_gray(245));
            pa.circle_stroke(r.center(), r.width() * 0.44, Stroke::new(1.2, ink));
            line(pa, arc(r, 0.5, 0.5, 0.3, 0.2, 4.4), 1.4, Color32::from_rgb(60, 120, 220));
            line(pa, arc(r, 0.46, 0.54, 0.16, 0.0, 5.5), 1.4, Color32::from_rgb(220, 70, 70));
        }
    }
}

pub fn size() -> Vec2 {
    Vec2::splat(22.0)
}
