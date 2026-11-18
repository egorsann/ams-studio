//! The arrangement of the panes of the main window: which panes are shown, in which order, and the rectangles they
//! get. Everything here is plain geometry and list handling, so it is tested without a window.
//!
//! * The stereonet is always there and never moved or removed.
//! * The three plot panes can be removed and added again, the core viewer and the GIS viewer are switched on and off in
//!   the View menu; all of them can be dragged into another place (by the six dots at the top of the pane).
//! * The panes are laid out in rows from the order: two panes of the same kind (plots with plots, the wide core viewer
//!   and GIS viewer with each other) share a row, a pane without a partner takes the whole row. So the defaults come
//!   out as before: the core viewer as a strip across the top, then the stereonet and the plots in two columns, and
//!   the GIS viewer sits next to the core viewer when both are on.

use crate::util::SafeClamp;
use egui::{Pos2, Rect};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PaneId {
    Stereo,
    /// A plot pane, slot 1 to 3 (the slot that holds its settings).
    Plot(usize),
    Core,
    Gis,
}

impl PaneId {
    /// The core viewer and the GIS viewer are wide panes; they get shorter rows than the plots.
    pub fn is_wide(self) -> bool {
        matches!(self, PaneId::Core | PaneId::Gis)
    }

    fn token(self) -> String {
        match self {
            PaneId::Stereo => "stereo".into(),
            PaneId::Plot(k) => format!("p{k}"),
            PaneId::Core => "core".into(),
            PaneId::Gis => "gis".into(),
        }
    }

    fn from_token(t: &str) -> Option<PaneId> {
        match t.trim() {
            "stereo" => Some(PaneId::Stereo),
            "core" => Some(PaneId::Core),
            "gis" => Some(PaneId::Gis),
            p if p.starts_with('p') => p[1..].parse::<usize>().ok().filter(|k| (1..=3).contains(k)).map(PaneId::Plot),
            _ => None,
        }
    }
}

/// The default order: stereonet and the three plots.
pub fn default_order() -> Vec<PaneId> {
    vec![PaneId::Stereo, PaneId::Plot(1), PaneId::Plot(2), PaneId::Plot(3)]
}

/// The order as text for the preferences file: `stereo,p1,p2,p3`.
pub fn order_to_text(order: &[PaneId]) -> String {
    order.iter().map(|p| p.token()).collect::<Vec<_>>().join(",")
}

/// The order from the preferences file; unknown or doubled entries are dropped, the stereonet is put back if missing.
pub fn order_from_text(t: &str) -> Vec<PaneId> {
    let mut v: Vec<PaneId> = vec![];
    for p in t.split(',').filter_map(PaneId::from_token) {
        if !v.contains(&p) {
            v.push(p);
        }
    }
    if !v.contains(&PaneId::Stereo) {
        v.insert(0, PaneId::Stereo);
    }
    v
}

/// Brings the order in line with what is switched on: the core viewer and the GIS viewer are in it exactly when they are
/// on (a new one goes to the front, the GIS viewer right after the core viewer), the stereonet is always in it.
pub fn sync_order(order: &[PaneId], core_on: bool, gis_on: bool) -> Vec<PaneId> {
    let mut o: Vec<PaneId> = order.to_vec();
    o.retain(|p| match p {
        PaneId::Core => core_on,
        PaneId::Gis => gis_on,
        _ => true,
    });
    if !o.contains(&PaneId::Stereo) {
        o.insert(0, PaneId::Stereo);
    }
    if core_on && !o.contains(&PaneId::Core) {
        o.insert(0, PaneId::Core);
    }
    if gis_on && !o.contains(&PaneId::Gis) {
        let at = if o.first() == Some(&PaneId::Core) { 1 } else { 0 };
        o.insert(at, PaneId::Gis);
    }
    o
}

/// The rows: two neighbours of the same kind share a row, anything else stands alone and takes the whole row.
pub fn build_rows(order: &[PaneId]) -> Vec<Vec<PaneId>> {
    let mut rows = vec![];
    let mut i = 0;
    while i < order.len() {
        if i + 1 < order.len() && order[i].is_wide() == order[i + 1].is_wide() {
            rows.push(vec![order[i], order[i + 1]]);
            i += 2;
        } else {
            rows.push(vec![order[i]]);
            i += 1;
        }
    }
    rows
}

/// The height of every row. `total` is the height of the area, `gap` the space between rows. Wide rows get
/// `wide_frac` of the area each (together at most 65 %), two plot rows split the rest by `vfrac`, more or fewer share it
/// equally.
pub fn row_heights(rows: &[Vec<PaneId>], total: f32, gap: f32, wide_frac: f32, vfrac: f32) -> Vec<f32> {
    let n = rows.len();
    if n == 0 {
        return vec![];
    }
    let inner = (total - gap * (n as f32 - 1.0)).max(60.0 * n as f32);
    let wide: Vec<bool> = rows.iter().map(|r| r[0].is_wide()).collect();
    let (nw, np) = (wide.iter().filter(|w| **w).count(), wide.iter().filter(|w| !**w).count());
    let plot_share = |rest: f32| -> Vec<f32> {
        if np == 2 {
            vec![rest * vfrac, rest * (1.0 - vfrac)]
        } else {
            vec![rest / np.max(1) as f32; np]
        }
    };
    if np == 0 {
        return vec![inner / n as f32; n];
    }
    let wide_h = if nw == 0 { 0.0 } else { (wide_frac * total).clamp_safe(90.0, 0.65 * inner / nw as f32).max(60.0) };
    let rest = (inner - wide_h * nw as f32).max(60.0 * np as f32);
    let mut plots = plot_share(rest).into_iter();
    wide.iter().map(|w| if *w { wide_h } else { plots.next().unwrap_or(60.0) }).collect()
}

/// A bar between two panes that the user can drag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splitter {
    pub rect: Rect,
    /// Between the two panes of a row (drag sideways) or between two rows (drag up and down).
    pub between_columns: bool,
    /// The row it belongs to (for a bar between rows: the row above it).
    pub row: usize,
}

/// The rectangle of every pane, the bars between them and the rectangle of every row.
pub struct PaneRects {
    pub panes: Vec<(PaneId, Rect)>,
    pub splitters: Vec<Splitter>,
    pub rows: Vec<Rect>,
}

pub fn place(rows: &[Vec<PaneId>], all: Rect, gap: f32, wide_frac: f32, vfrac: f32, hfrac: f32, gfrac: f32) -> PaneRects {
    let heights = row_heights(rows, all.height(), gap, wide_frac, vfrac);
    let mut out = PaneRects { panes: vec![], splitters: vec![], rows: vec![] };
    let mut y = all.min.y;
    for (r, (row, h)) in rows.iter().zip(&heights).enumerate() {
        let rr = Rect::from_min_max(Pos2::new(all.min.x, y), Pos2::new(all.max.x, y + h));
        out.rows.push(rr);
        if row.len() == 2 {
            let frac = if row[0].is_wide() { gfrac } else { hfrac };
            let sx = rr.min.x + rr.width() * frac;
            out.panes.push((row[0], Rect::from_min_max(rr.min, Pos2::new(sx - gap / 2.0, rr.max.y))));
            out.panes.push((row[1], Rect::from_min_max(Pos2::new(sx + gap / 2.0, rr.min.y), rr.max)));
            out.splitters.push(Splitter { rect: Rect::from_min_max(Pos2::new(sx - gap / 2.0, rr.min.y), Pos2::new(sx + gap / 2.0, rr.max.y)), between_columns: true, row: r });
        } else {
            out.panes.push((row[0], rr));
        }
        if r + 1 < rows.len() {
            out.splitters.push(Splitter { rect: Rect::from_min_max(Pos2::new(all.min.x, y + h), Pos2::new(all.max.x, y + h + gap)), between_columns: false, row: r });
        }
        y += h + gap;
    }
    out
}

/// Dropping the dragged pane on the target pane: they swap places. Dropped on the stereonet it goes right behind it
/// (the stereonet itself never moves).
pub fn move_pane(order: &[PaneId], dragged: PaneId, target: PaneId) -> Vec<PaneId> {
    let mut o = order.to_vec();
    if dragged == PaneId::Stereo || dragged == target {
        return o;
    }
    let (Some(a), Some(b)) = (o.iter().position(|p| *p == dragged), o.iter().position(|p| *p == target)) else { return o };
    if target == PaneId::Stereo {
        o.remove(a);
        let at = o.iter().position(|p| *p == PaneId::Stereo).map_or(0, |i| i + 1);
        o.insert(at, dragged);
    } else {
        o.swap(a, b);
    }
    o
}

/// The pane that "Add pane" brings back or switches on: a removed plot pane first, then the GIS viewer, then the core
/// viewer. `None` when everything is shown.
pub fn next_addable(order: &[PaneId], core_on: bool, gis_on: bool) -> Option<PaneId> {
    (1..=3).map(PaneId::Plot).find(|p| !order.contains(p)).or(if gis_on { None } else { Some(PaneId::Gis) }).or(if core_on { None } else { Some(PaneId::Core) })
}

/// The pane a point lies in.
pub fn pane_at(panes: &[(PaneId, Rect)], p: Pos2) -> Option<PaneId> {
    panes.iter().find(|(_, r)| r.contains(p)).map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use PaneId::*;

    fn rect(w: f32, h: f32) -> Rect {
        Rect::from_min_max(Pos2::ZERO, Pos2::new(w, h))
    }

    #[test]
    fn the_default_order_is_the_classic_two_by_two() {
        assert_eq!(build_rows(&default_order()), vec![vec![Stereo, Plot(1)], vec![Plot(2), Plot(3)]]);
    }

    #[test]
    fn the_core_viewer_is_a_strip_and_the_gis_viewer_sits_next_to_it() {
        let o = sync_order(&default_order(), true, false);
        assert_eq!(o, vec![Core, Stereo, Plot(1), Plot(2), Plot(3)]);
        assert_eq!(build_rows(&o), vec![vec![Core], vec![Stereo, Plot(1)], vec![Plot(2), Plot(3)]]);
        let o = sync_order(&o, true, true);
        assert_eq!(o, vec![Core, Gis, Stereo, Plot(1), Plot(2), Plot(3)]);
        assert_eq!(build_rows(&o), vec![vec![Core, Gis], vec![Stereo, Plot(1)], vec![Plot(2), Plot(3)]]);
        // the GIS viewer alone is a strip too
        let o = sync_order(&default_order(), false, true);
        assert_eq!(build_rows(&o)[0], vec![Gis]);
        // switching them off takes them out again
        assert_eq!(sync_order(&[Core, Gis, Stereo, Plot(2)], false, false), vec![Stereo, Plot(2)]);
        // the stereonet cannot be missing
        assert_eq!(sync_order(&[Plot(1)], false, false), vec![Stereo, Plot(1)]);
    }

    #[test]
    fn a_pane_without_a_partner_takes_the_whole_row() {
        assert_eq!(build_rows(&[Stereo, Plot(1), Plot(2)]), vec![vec![Stereo, Plot(1)], vec![Plot(2)]]);
        assert_eq!(build_rows(&[Stereo]), vec![vec![Stereo]]);
        assert_eq!(build_rows(&[Stereo, Plot(3)]), vec![vec![Stereo, Plot(3)]]);
        assert!(build_rows(&[]).is_empty());
    }

    #[test]
    fn rows_fill_the_area_without_overlap() {
        let rows = build_rows(&sync_order(&default_order(), true, true));
        let all = rect(1000.0, 800.0);
        let p = place(&rows, all, 8.0, 0.28, 0.5, 0.5, 0.5);
        assert_eq!(p.panes.len(), 6);
        // every pane is inside the area, no two overlap, and the rows are stacked from top to bottom
        for (_, r) in &p.panes {
            assert!(all.contains_rect(*r) || (r.min.x >= -0.01 && r.max.x <= 1000.01 && r.min.y >= -0.01 && r.max.y <= 800.01));
        }
        for (i, (_, a)) in p.panes.iter().enumerate() {
            for (_, b) in &p.panes[i + 1..] {
                assert!(!a.shrink(0.5).intersects(b.shrink(0.5)), "{a:?} {b:?}");
            }
        }
        assert!((p.rows.last().unwrap().max.y - 800.0).abs() < 0.5);
        // the wide row is shorter than a plot row
        assert!(p.rows[0].height() < p.rows[1].height());
        // a bar between the two columns of every row that has two panes, and between the rows
        assert_eq!(p.splitters.iter().filter(|s| s.between_columns).count(), 3);
        assert_eq!(p.splitters.iter().filter(|s| !s.between_columns).count(), 2);
    }

    #[test]
    fn row_heights_follow_the_fractions() {
        let rows = vec![vec![Core], vec![Stereo, Plot(1)], vec![Plot(2), Plot(3)]];
        let h = row_heights(&rows, 1000.0, 10.0, 0.25, 0.4);
        assert!((h[0] - 250.0).abs() < 1e-3);
        let rest = 1000.0 - 20.0 - 250.0;
        assert!((h[1] - rest * 0.4).abs() < 1e-3 && (h[2] - rest * 0.6).abs() < 1e-3);
        assert!((h.iter().sum::<f32>() + 20.0 - 1000.0).abs() < 1e-2);
        // three plot rows share equally, the wide ones are capped at 65 % together
        let many = vec![vec![Core, Gis], vec![Gis], vec![Stereo, Plot(1)]];
        let h = row_heights(&many, 1000.0, 0.0, 0.6, 0.5);
        assert!(h[0] + h[1] <= 650.0 + 1e-3 && h[2] >= 350.0 - 1e-3);
        // only wide rows: they share the area
        let h = row_heights(&[vec![Core], vec![Gis]], 500.0, 0.0, 0.2, 0.5);
        assert_eq!(h, vec![250.0, 250.0]);
        assert!(row_heights(&[], 100.0, 0.0, 0.2, 0.5).is_empty());
    }

    #[test]
    fn dragging_swaps_panes_and_never_moves_the_stereonet() {
        let o = vec![Core, Gis, Stereo, Plot(1), Plot(2), Plot(3)];
        assert_eq!(move_pane(&o, Plot(3), Gis), vec![Core, Plot(3), Stereo, Plot(1), Plot(2), Gis]);
        // dropped on the stereonet: right behind it
        assert_eq!(move_pane(&o, Plot(3), Stereo), vec![Core, Gis, Stereo, Plot(3), Plot(1), Plot(2)]);
        // the stereonet cannot be dragged, nothing happens when a pane is dropped on itself
        assert_eq!(move_pane(&o, Stereo, Plot(1)), o);
        assert_eq!(move_pane(&o, Plot(1), Plot(1)), o);
        // a pane that is not shown is ignored
        assert_eq!(move_pane(&o[..3], Plot(1), Core), o[..3].to_vec());
    }

    #[test]
    fn add_pane_brings_back_plots_first_then_gis_then_core() {
        assert_eq!(next_addable(&[Stereo, Plot(1), Plot(3)], false, false), Some(Plot(2)));
        assert_eq!(next_addable(&default_order(), false, false), Some(Gis));
        assert_eq!(next_addable(&sync_order(&default_order(), false, true), false, true), Some(Core));
        assert_eq!(next_addable(&sync_order(&default_order(), true, true), true, true), None);
    }

    #[test]
    fn the_order_survives_the_preferences_file() {
        let o = vec![Core, Stereo, Plot(2), Gis, Plot(1)];
        assert_eq!(order_from_text(&order_to_text(&o)), o);
        // junk, doubles and a missing stereonet
        assert_eq!(order_from_text("p2,p2,p9,zzz,p1"), vec![Stereo, Plot(2), Plot(1)]);
        assert_eq!(order_from_text(""), vec![Stereo]);
    }

    #[test]
    fn the_pane_under_a_point() {
        let p = place(&build_rows(&default_order()), rect(1000.0, 800.0), 8.0, 0.28, 0.5, 0.5, 0.5);
        assert_eq!(pane_at(&p.panes, Pos2::new(100.0, 100.0)), Some(Stereo));
        assert_eq!(pane_at(&p.panes, Pos2::new(900.0, 700.0)), Some(Plot(3)));
        assert_eq!(pane_at(&p.panes, Pos2::new(-5.0, 10.0)), None);
    }
}
