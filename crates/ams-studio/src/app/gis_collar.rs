//! Drill core on the map: specimens that share one position (the collar of a hole, different depths) are not drawn on
//! top of each other. They go into a small box beside the black collar dot, listed in order of depth, with a lead from
//! the box to the dot. The box is moved around the dot (all the way round, further out when it has to) so that it never
//! covers another symbol, collar dot, hole line or box. The maths of this (grouping, order, size, place) is here and
//! tested without a window; the drawing is in `gis_ui.rs`.

use super::gis_loc::Hole;
use super::gis_ui::Placed;
use egui::{Pos2, Rect, Vec2};
use std::collections::BTreeMap;

/// One position on the map: the specimens taken there (in order of depth) and the direction of the hole, if known.
#[derive(Clone, Debug, PartialEq)]
pub struct Collar {
    pub merc: (f64, f64),
    /// Indices into the placed specimens, shallowest first, specimens without a depth last.
    pub members: Vec<usize>,
    pub hole: Option<Hole>,
    /// Drill core: a member has a depth or the hole direction. Such a collar gets its box by itself when it has two or
    /// more specimens; any other place only when the user switches the boxes on.
    pub core: bool,
}

/// Positions closer than this (Web Mercator metres) are one position.
const SAME_SPOT: f64 = 0.1;

/// Groups the placed specimens by position. Every distinct position is returned (the caller draws a box for the ones with
/// two or more specimens and a hole line for the ones that have a hole direction). Order of the result: by position, so
/// that the layout does not flicker from one frame to the next.
pub fn collars(placed: &[Placed]) -> Vec<Collar> {
    let mut by_spot: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, p) in placed.iter().enumerate() {
        by_spot.entry(((p.merc.0 / SAME_SPOT).round() as i64, (p.merc.1 / SAME_SPOT).round() as i64)).or_default().push(i);
    }
    by_spot
        .into_values()
        .map(|mut members| {
            let core = members.iter().any(|&i| placed[i].depth.is_some() || placed[i].hole.is_some());
            // drill core: depth, shallowest first; otherwise elevation, highest first (the highest is the top of the list)
            let key = |i: usize| if core { placed[i].depth } else { placed[i].elev.map(|e| -e) };
            members.sort_by(|&a, &b| {
                let (pa, pb) = (&placed[a], &placed[b]);
                // key ascending, none last, then the name, then the original order
                match (key(a), key(b)) {
                    (Some(x), Some(y)) => x.total_cmp(&y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
                .then_with(|| pa.name.cmp(&pb.name))
                .then(a.cmp(&b))
            });
            let first = members[0];
            Collar { merc: placed[first].merc, hole: members.iter().find_map(|&i| placed[i].hole), core, members }
        })
        .collect()
}

/// At most this many rows in a box: a longer list continues in a second column and so on, so a collar with many
/// specimens gets a wide box instead of a tall one.
pub const MAX_ROWS: usize = 6;

/// The cells of a box with `n` entries: rows and columns, entries going down each column in order of depth.
pub fn grid(n: usize) -> (usize, usize) {
    let rows = n.clamp(1, MAX_ROWS);
    (rows, n.max(1).div_ceil(rows))
}

/// Row and column of entry `k`.
pub fn cell_of(k: usize, rows: usize) -> (usize, usize) {
    (k % rows, k / rows)
}

/// The size of a box: `n` entries, each `icon` square with a label `text_w` wide beside it.
pub fn box_size(n: usize, icon: f32, text_w: f32, pad: f32, gap: f32) -> Vec2 {
    let (rows, cols) = grid(n);
    let cell_w = icon + gap + text_w;
    Vec2::new(cols as f32 * cell_w + (cols as f32 - 1.0) * gap * 2.0 + 2.0 * pad, rows as f32 * icon + (rows as f32 - 1.0) * gap + 2.0 * pad)
}

/// Something the box must not cover (screen coordinates).
#[derive(Clone, Copy, Debug)]
pub enum Obstacle {
    Circle(Pos2, f32),
    Rect(Rect),
    /// A thin line (a hole direction) with half its width.
    Segment(Pos2, Pos2, f32),
}

impl Obstacle {
    fn hits(&self, r: Rect) -> bool {
        match *self {
            Obstacle::Circle(c, rad) => {
                let nearest = Pos2::new(c.x.clamp(r.min.x, r.max.x), c.y.clamp(r.min.y, r.max.y));
                nearest.distance(c) < rad
            }
            Obstacle::Rect(o) => o.intersects(r),
            Obstacle::Segment(a, b, w) => {
                let n = ((a.distance(b) / 2.0).ceil() as usize).max(1);
                let grown = r.expand(w);
                (0..=n).any(|i| grown.contains(a + (b - a) * (i as f32 / n as f32)))
            }
        }
    }
}

/// Distances from the dot (to the near edge of the box) tried one after the other, in screen points.
const RINGS: [f32; 9] = [26.0, 42.0, 62.0, 88.0, 120.0, 160.0, 210.0, 270.0, 340.0];

/// Where the box goes: it is moved all the way round the dot at the first distance, and when every direction is blocked
/// one ring further out. The preferred side is up and to the right; the order of the other directions is by how far
/// they turn from it, so the same picture gives the same answer. `canvas` is the visible map: a place outside it is only
/// used when none inside is free. Returns the box and whether it is clear of everything (false: nothing was free, the
/// box is put where it covers the least).
pub fn place_box(anchor: Pos2, size: Vec2, obstacles: &[Obstacle], canvas: Rect) -> (Rect, bool) {
    let mut angles: Vec<f32> = (0..24).map(|k| k as f32 * 15.0).collect();
    let turn = |a: f32| {
        let d = (a - 45.0).rem_euclid(360.0);
        d.min(360.0 - d)
    };
    angles.sort_by(|a, b| turn(*a).total_cmp(&turn(*b)).then(a.total_cmp(b)));
    let at = |ring: f32, angle: f32| -> Rect {
        let (s, c) = angle.to_radians().sin_cos();
        let u = Vec2::new(c, -s);
        let support = u.x.abs() * size.x / 2.0 + u.y.abs() * size.y / 2.0;
        Rect::from_center_size(anchor + u * (ring + support), size)
    };
    let inner = canvas.shrink(4.0);
    for need_inside in [true, false] {
        for &ring in &RINGS {
            for &angle in &angles {
                let r = at(ring, angle);
                if (!need_inside || inner.contains_rect(r)) && !obstacles.iter().any(|o| o.hits(r)) {
                    return (r, true);
                }
            }
        }
    }
    // nothing free: the candidate that covers the fewest things
    let mut best = (usize::MAX, at(RINGS[0], 45.0));
    for &angle in &angles {
        let r = at(RINGS[0], angle);
        let n = obstacles.iter().filter(|o| o.hits(r)).count();
        if n < best.0 {
            best = (n, r);
        }
    }
    (best.1, false)
}

/// The point of `r` nearest to `p`: where the lead from the dot meets the box.
pub fn nearest_on(r: Rect, p: Pos2) -> Pos2 {
    Pos2::new(p.x.clamp(r.min.x, r.max.x), p.y.clamp(r.min.y, r.max.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ams_core::gis::Fabric;

    fn placed(name: &str, x: f64, y: f64, depth: Option<f64>, hole: Option<Hole>) -> Placed {
        Placed { name: name.into(), path: std::path::PathBuf::new(), n: 1, lon: 0.0, lat: 0.0, merc: (x, y), fabric: Fabric { strike: 0.0, dip_dir: 90.0, dip: 30.0, trend: 90.0, plunge: 10.0 }, depth, hole, elev: None }
    }

    #[test]
    fn specimens_of_one_collar_are_grouped_and_listed_by_depth() {
        let hole = Hole::new(316.1, 67.0);
        let p = vec![
            placed("C", 100.0, 200.0, Some(310.5), None),
            placed("X", 500.0, 600.0, None, None),
            placed("A", 100.0, 200.0, Some(12.0), hole),
            placed("D", 100.0, 200.0, None, None),
            placed("B", 100.02, 200.03, Some(155.25), None),
            placed("E", 100.0, 200.0, None, None),
        ];
        let c = collars(&p);
        assert_eq!(c.len(), 2);
        let (first, second) = if c[0].members.len() > 1 { (&c[0], &c[1]) } else { (&c[1], &c[0]) };
        // shallowest first, no depth last (by name), 2 cm apart is the same spot
        assert_eq!(first.members.iter().map(|&i| p[i].name.as_str()).collect::<Vec<_>>(), vec!["A", "B", "C", "D", "E"]);
        // the hole direction of any member is the collar's
        assert_eq!(first.hole, hole);
        assert_eq!((second.members.clone(), second.hole), (vec![1], None));
        // a metre apart is another spot
        assert_eq!(collars(&[placed("A", 0.0, 0.0, None, None), placed("B", 1.0, 0.0, None, None)]).len(), 2);
        assert!(collars(&[]).is_empty());
    }

    #[test]
    fn a_place_of_block_samples_is_not_core_and_is_listed_by_elevation_highest_first() {
        let mut p = vec![placed("A", 5.0, 5.0, None, None), placed("B", 5.0, 5.0, None, None), placed("C", 5.0, 5.0, None, None), placed("D", 5.0, 5.0, None, None)];
        p[0].elev = Some(120.0);
        p[1].elev = Some(-3.0);
        p[3].elev = Some(480.5);
        let c = collars(&p);
        assert_eq!(c.len(), 1);
        assert!(!c[0].core);
        assert_eq!(c[0].members.iter().map(|&i| p[i].name.as_str()).collect::<Vec<_>>(), vec!["D", "A", "B", "C"]);
        // one core member makes the place a collar, listed by depth
        p[2].depth = Some(2.0);
        let c = collars(&p);
        assert!(c[0].core);
        assert_eq!(c[0].members[0], 2);
    }

    #[test]
    fn a_long_list_makes_a_wide_box_not_a_tall_one() {
        assert_eq!(grid(1), (1, 1));
        assert_eq!(grid(6), (6, 1));
        assert_eq!(grid(7), (6, 2));
        assert_eq!(grid(12), (6, 2));
        assert_eq!(grid(13), (6, 3));
        // entries run down a column in order of depth, then on in the next
        assert_eq!((0..8).map(|k| cell_of(k, 6)).collect::<Vec<_>>(), vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (0, 1), (1, 1)]);
        let one = box_size(1, 20.0, 40.0, 5.0, 4.0);
        let twelve = box_size(12, 20.0, 40.0, 5.0, 4.0);
        assert!(twelve.y < 6.0 * 30.0 && twelve.x > one.x);
    }

    fn canvas() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))
    }

    #[test]
    fn the_box_goes_up_and_right_when_nothing_is_in_the_way_and_keeps_off_the_dot() {
        let anchor = Pos2::new(400.0, 300.0);
        let (r, clear) = place_box(anchor, Vec2::new(90.0, 80.0), &[], canvas());
        assert!(clear);
        assert!(r.center().x > anchor.x && r.center().y < anchor.y, "{r:?}");
        // a lead of about the first ring between the dot and the box
        let d = nearest_on(r, anchor).distance(anchor);
        assert!((20.0..40.0).contains(&d), "{d}");
        assert!(!r.contains(anchor));
    }

    #[test]
    fn the_box_dodges_other_markers_all_the_way_round() {
        let anchor = Pos2::new(400.0, 300.0);
        let size = Vec2::new(90.0, 80.0);
        // the favourite side is taken by a symbol and a box, so another direction is used and nothing is covered
        let obstacles = vec![Obstacle::Circle(Pos2::new(470.0, 230.0), 45.0), Obstacle::Rect(Rect::from_min_size(Pos2::new(430.0, 150.0), Vec2::new(120.0, 90.0))), Obstacle::Segment(anchor, Pos2::new(430.0, 330.0), 2.0)];
        let (r, clear) = place_box(anchor, size, &obstacles, canvas());
        assert!(clear);
        assert!(obstacles.iter().all(|o| !o.hits(r)), "{r:?}");
        // surrounded on every side: the box is pushed out to a further ring, still clear
        let mut ring = vec![];
        for k in 0..16 {
            let a = (k as f32 * 22.5).to_radians();
            ring.push(Obstacle::Circle(anchor + Vec2::new(a.cos(), -a.sin()) * 70.0, 28.0));
        }
        let (r2, clear2) = place_box(anchor, size, &ring, canvas());
        assert!(clear2 && ring.iter().all(|o| !o.hits(r2)));
        assert!(nearest_on(r2, anchor).distance(anchor) > 90.0);
        // the same picture gives the same answer
        assert_eq!(place_box(anchor, size, &obstacles, canvas()).0, r);
    }

    #[test]
    fn the_box_stays_on_the_map_near_its_edge_and_says_so_when_it_cannot_be_clear() {
        // dot at the top right corner: the box goes down and left, inside the canvas
        let anchor = Pos2::new(780.0, 20.0);
        let (r, clear) = place_box(anchor, Vec2::new(90.0, 80.0), &[], canvas());
        assert!(clear && canvas().contains_rect(r) && r.center().x < anchor.x && r.center().y > anchor.y, "{r:?}");
        // a box bigger than the whole map cannot be anywhere clear; it is still returned, flagged
        let huge = Obstacle::Rect(Rect::from_min_size(Pos2::new(-10_000.0, -10_000.0), Vec2::splat(30_000.0)));
        let (_, clear) = place_box(Pos2::new(400.0, 300.0), Vec2::new(90.0, 80.0), &[huge], canvas());
        assert!(!clear);
    }
}
