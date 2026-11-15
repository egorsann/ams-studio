//! The core viewer: the cube photos of a drill core on the layout of the AMS dashboard's cube map
//! (`hyperspectral_dashboard_row_dependancy.py`: `_cube_layout`, `_cube_y`, `cube_photo_mosaic`). The cubes sit by slice
//! along the core, the rows follow the letters (A at the bottom), and a slice cut with fewer cubes can be centred on the
//! core axis. Everything is kept in memory: the photos are decoded into textures, no file is written.

use crate::plots::{band, label_box, paint_band, PlotOut, Theme};
use ams_core::core::slice_of;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, Vec2};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

/// Gap between two cubes, as a fraction of a cube (`CUBE_GAP` of the dashboard).
const CUBE_GAP: f32 = 0.16;

/// Slices that are centred on the core axis, per specimen (`CUBE_POSITIONS` of the dashboard notebook): a slice with
/// fewer cubes than the usual layout.
pub fn centred_slices(specimen: &str) -> &'static [u32] {
    match specimen {
        "DC26AB002" => &[0, 1, 2, 3, 4, 5, 24],
        "DC26AB012" => &[0, 1, 75, 76, 77],
        _ => &[],
    }
}

/// Single cubes that lie turned in the tray photo, in degrees anticlockwise (`CUBE_PHOTO_ROTATE` of the dashboard).
pub fn photo_turn(specimen: &str, letter: char, slice: u32) -> i32 {
    match (specimen, letter, slice) {
        ("DC26AB013", 'C', 1) => 180,
        _ => 0,
    }
}

/// One cube on the core: where it sits, the records of the open file it belongs to and its photo.
#[derive(Clone, Debug)]
pub struct Cube {
    pub slice: u32,
    pub letter: char,
    /// Row position (0 = the A row; the cube centre is `y + 0.5`).
    pub y: f32,
    /// Indices of the records of the open file with this name (empty for a cube that has a photo only).
    pub recs: Vec<usize>,
    pub photo: Option<PathBuf>,
}

impl Cube {
    pub fn name(&self) -> String {
        format!("{}{}", self.letter, self.slice)
    }
}

/// The cubes of the layout: `names` are the record names of the open file, `photos` the crops found.
/// A cube is every (slice, letter) of the records plus every crop inside the slice range of the records (the physical
/// layout also needs the cubes that were not measured).
pub fn build(specimen: &str, names: &[String], photos: &BTreeMap<(u32, char), PathBuf>) -> Vec<Cube> {
    let mut recs: BTreeMap<(u32, char), Vec<usize>> = BTreeMap::new();
    for (i, n) in names.iter().enumerate() {
        if let Some(sl) = slice_of(n) {
            let l = n.trim().chars().next().unwrap_or('A');
            recs.entry((sl, l)).or_default().push(i);
        }
    }
    let (lo, hi) = match (recs.keys().map(|k| k.0).min(), recs.keys().map(|k| k.0).max()) {
        (Some(a), Some(b)) => (a, b),
        _ => (photos.keys().map(|k| k.0).min().unwrap_or(0), photos.keys().map(|k| k.0).max().unwrap_or(0)),
    };
    let mut keys: HashSet<(u32, char)> = recs.keys().copied().collect();
    keys.extend(photos.keys().filter(|k| k.0 >= lo && k.0 <= hi).copied());
    let mut keys: Vec<(u32, char)> = keys.into_iter().collect();
    keys.sort();
    let ys = layout_rows(&keys, centred_slices(specimen));
    keys.iter()
        .zip(ys)
        .map(|(&(slice, letter), y)| Cube { slice, letter, y, recs: recs.get(&(slice, letter)).cloned().unwrap_or_default(), photo: photos.get(&(slice, letter)).cloned() })
        .collect()
}

/// `_cube_layout` + `_cube_y` of the dashboard: the row of every cube.
/// The usual A-C layout of the specimen (the most common letter set per slice) gets the first rows and its middle is
/// the core axis; other A-C letters follow, then D.
pub fn layout_rows(cubes: &[(u32, char)], centred: &[u32]) -> Vec<f32> {
    let letters: Vec<char> = "ABCD".chars().filter(|l| cubes.iter().any(|c| c.1 == *l)).collect();
    if letters.is_empty() {
        return vec![0.0; cubes.len()];
    }
    let mut per: BTreeMap<u32, String> = BTreeMap::new();
    for (sl, lt) in cubes {
        if matches!(lt, 'A'..='C') {
            per.entry(*sl).or_default().push(*lt);
        }
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for v in per.values() {
        let mut c: Vec<char> = v.chars().collect();
        c.sort();
        *counts.entry(c.into_iter().collect()).or_default() += 1;
    }
    // the most common set, the smallest one when tied (like pandas' mode)
    let dominant: Vec<char> = counts.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|(k, _)| k.chars().collect()).unwrap_or_else(|| letters.iter().copied().filter(|l| *l != 'D').collect());
    let mut order: Vec<char> = dominant.clone();
    order.extend(letters.iter().copied().filter(|l| !dominant.contains(l) && *l != 'D'));
    if letters.contains(&'D') {
        order.push('D');
    }
    let row_of = |l: char| order.iter().position(|o| *o == l).unwrap_or(0) as f32;
    let axis = match (dominant.first(), dominant.last()) {
        (Some(a), Some(b)) => (row_of(*a) + row_of(*b)) / 2.0,
        _ => 0.0,
    };
    let mut at: BTreeMap<u32, Vec<char>> = BTreeMap::new();
    for (sl, lt) in cubes {
        at.entry(*sl).or_default().push(*lt);
    }
    cubes
        .iter()
        .map(|(sl, lt)| {
            if centred.contains(sl) && *lt != 'D' {
                let mut here: Vec<char> = at[sl].iter().copied().filter(|l| *l != 'D').collect();
                here.sort();
                let k = here.iter().position(|l| l == lt).unwrap_or(0) as f32;
                axis + k - (here.len() as f32 - 1.0) / 2.0
            } else {
                row_of(*lt)
            }
        })
        .collect()
}

/// Pan and zoom of the viewer (Ctrl + scroll zooms about the pointer, scroll or middle-drag pans, Esc resets).
#[derive(Clone, Copy, PartialEq)]
pub struct View {
    pub zoom: f32,
    pub off: Vec2,
}

impl Default for View {
    fn default() -> Self {
        View { zoom: 1.0, off: Vec2::ZERO }
    }
}

impl View {
    pub const MAX: f32 = 40.0;
    pub fn is_zoomed(&self) -> bool {
        self.zoom > 1.0001 || self.off.length() > 0.5
    }
    pub fn reset(&mut self) {
        *self = View::default();
    }
}

/// Draws the viewer inside `rect` and returns what the pointer did (click, rubber band, hover text).
/// `textures` hold the decoded photos; `selected` / `active` are per record of the open file.
pub fn draw(ui: &mut Ui, rect: Rect, cubes: &[Cube], textures: &HashMap<(u32, char), TextureHandle>, selected: &[bool], active: &[bool], view: &mut View, th: &Theme, specimen: &str) -> PlotOut {
    let pa = ui.painter_at(rect);
    pa.rect_filled(rect, 2.0, th.canvas());
    let mut out = PlotOut::default();
    if cubes.is_empty() {
        pa.text(rect.center(), Align2::CENTER_CENTER, "No cubes to show", FontId::proportional(13.0), th.faint());
        return out;
    }
    let (smin, smax) = (cubes.iter().map(|c| c.slice).min().unwrap_or(0), cubes.iter().map(|c| c.slice).max().unwrap_or(0));
    let (ylo, yhi) = (cubes.iter().map(|c| c.y).fold(f32::MAX, f32::min), cubes.iter().map(|c| c.y).fold(f32::MIN, f32::max) + 1.0);
    let (n_sl, rows) = ((smax - smin + 1) as f32, yhi - ylo);
    let inner = Rect::from_min_max(Pos2::new(rect.min.x + 22.0, rect.min.y + 4.0), Pos2::new(rect.max.x - 4.0, rect.max.y - 16.0));
    let base = (inner.width() / n_sl).min(inner.height() / rows).max(3.0);

    let resp = ui.interact(rect, ui.scope_id().with("core-viewer"), Sense::click_and_drag());
    let mods = ui.input(|i| i.modifiers);
    // zoom about the pointer (Ctrl + scroll or a pinch); plain scroll and middle-drag pan
    if resp.hovered() {
        if let Some(h) = resp.hover_pos() {
            let (zd, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            if (zd - 1.0).abs() > 1e-4 {
                let new = (view.zoom * zd.powf(0.7)).clamp(1.0, View::MAX);
                let k = new / view.zoom;
                let c_old = inner.center() + view.off;
                view.off = (h - (h - c_old) * k) - inner.center();
                view.zoom = new;
            } else if scroll != Vec2::ZERO && !(mods.ctrl || mods.command) && view.zoom > 1.0 {
                // scrolling moves along the core (the long direction first)
                view.off += Vec2::new(scroll.x + scroll.y, 0.0);
            }
        }
    }
    if resp.dragged_by(egui::PointerButton::Middle) {
        view.off += resp.drag_delta();
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let u = base * view.zoom;
    let (cw, ch) = (n_sl * u, rows * u);
    // keep the core from being pushed out of the pane
    view.off.x = view.off.x.clamp(-(cw / 2.0 + inner.width() / 2.0 - 20.0), cw / 2.0 + inner.width() / 2.0 - 20.0);
    view.off.y = view.off.y.clamp(-(ch / 2.0 + inner.height() / 2.0 - 20.0), ch / 2.0 + inner.height() / 2.0 - 20.0);
    let (x0, y0) = (inner.center().x + view.off.x - cw / 2.0, inner.center().y + view.off.y - ch / 2.0);
    let side = u * (1.0 - CUBE_GAP);
    let cube_rect = |c: &Cube| Rect::from_center_size(Pos2::new(x0 + (c.slice - smin) as f32 * u + u / 2.0, y0 + (yhi - c.y - 0.5) * u), Vec2::splat(side));

    let (live, done) = if resp.dragged_by(egui::PointerButton::Middle) { (None, None) } else { band(ui, &resp, "cband") };
    let clip = ui.painter_at(inner);
    let hover = resp.hover_pos().filter(|p| inner.contains(*p));
    let mut hot: Option<usize> = None;
    let mut picked: Vec<usize> = vec![];
    for (ci, c) in cubes.iter().enumerate() {
        let r = cube_rect(c);
        if !r.intersects(inner) {
            continue;
        }
        let sel = c.recs.iter().any(|&i| selected.get(i).copied().unwrap_or(false));
        let act = c.recs.is_empty() || c.recs.iter().any(|&i| active.get(i).copied().unwrap_or(true));
        match textures.get(&(c.slice, c.letter)) {
            Some(t) => {
                let tint = if act { Color32::WHITE } else { Color32::from_gray(90) };
                clip.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
            }
            None => {
                // the placeholder of a photo that is loading or missing
                clip.rect_filled(r, 2.0, if th.dark { Color32::from_gray(44) } else { Color32::from_gray(228) });
                if c.photo.is_none() && side > 26.0 {
                    clip.text(r.center(), Align2::CENTER_CENTER, "no photo", FontId::proportional(10.0), th.faint());
                }
            }
        }
        if !c.recs.is_empty() && side > 20.0 {
            clip.text(r.left_top() + Vec2::new(3.0, 2.0), Align2::LEFT_TOP, c.name(), FontId::proportional(9.5), Color32::from_rgba_unmultiplied(255, 255, 255, 220));
        }
        if c.recs.is_empty() {
            // a cube that was cut but has no AMS record here: shown, not selectable
            clip.rect_stroke(r, 2.0, Stroke::new(1.0, th.faint()), egui::StrokeKind::Inside);
        }
        if hover.map_or(false, |h| r.contains(h)) {
            hot = Some(ci);
        }
        if let Some(b) = done {
            if !c.recs.is_empty() && b.intersects(r) {
                picked.extend(c.recs.iter().copied());
            }
        }
        if sel {
            clip.rect_stroke(r, 2.0, Stroke::new(2.8, Color32::from_rgb(255, 160, 30)), egui::StrokeKind::Outside);
        }
    }
    if let Some(ci) = hot {
        let c = &cubes[ci];
        let r = cube_rect(c);
        if !c.recs.is_empty() {
            clip.rect_stroke(r, 2.0, Stroke::new(1.6, th.accent), egui::StrokeKind::Outside);
        }
        out.hover = Some(format!("{specimen}  {}{}", c.name(), if c.recs.is_empty() { "   (no AMS record)" } else { "" }));
        if ui.input(|i| i.modifiers.alt) {
            label_box(&pa, rect, r.center(), format!("{}  {specimen}", c.name()), th);
        }
        if resp.clicked() && !c.recs.is_empty() {
            if mods.ctrl || mods.command {
                // Ctrl + click: the cube joins the selection or leaves it
                out.toggle = Some(c.recs.clone());
            } else if c.recs.len() == 1 {
                out.click = Some(c.recs[0]);
            } else {
                out.band = Some(c.recs.clone());
            }
        }
    }
    if let Some(l) = live {
        paint_band(ui, rect, l, th);
    }
    if done.is_some() {
        picked.sort_unstable();
        picked.dedup();
        out.band = Some(picked);
        out.additive = mods.ctrl || mods.command;
    }
    // row letters at the left, slice numbers along the bottom
    let mut seen: HashSet<(i32, char)> = HashSet::new();
    for c in cubes {
        if seen.insert(((c.y * 2.0) as i32, c.letter)) && (c.y.fract() == 0.0) && u > 6.0 {
            let r = cube_rect(c);
            if r.center().y > inner.min.y && r.center().y < inner.max.y {
                pa.text(Pos2::new(rect.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, c.letter.to_string(), FontId::proportional(11.0), th.ink());
            }
        }
    }
    let step = ((30.0 / u).ceil() as u32).max(1);
    for sl in (smin..=smax).filter(|s| s % step == 0) {
        let x = x0 + (sl - smin) as f32 * u + u / 2.0;
        if x > inner.min.x && x < inner.max.x {
            pa.text(Pos2::new(x, rect.max.y - 2.0), Align2::CENTER_BOTTOM, sl.to_string(), FontId::proportional(10.0), th.faint());
        }
    }
    if view.zoom > 1.0 {
        pa.text(Pos2::new(rect.max.x - 6.0, rect.min.y + 4.0), Align2::RIGHT_TOP, format!("zoom \u{d7}{:.1}   Esc resets", view.zoom), FontId::proportional(10.5), th.faint());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rows_follow_the_usual_layout_and_d_comes_last() {
        // two A B slices, one with a D cube: A at row 0, B at row 1, D at row 2
        let cubes = [(1, 'A'), (1, 'B'), (2, 'A'), (2, 'B'), (2, 'D')];
        let y = layout_rows(&cubes, &[]);
        assert_eq!(y, vec![0.0, 1.0, 0.0, 1.0, 2.0]);
    }

    #[test]
    fn a_centred_slice_sits_on_the_core_axis() {
        // DC26AB002: 'A B C' slices; slice 0 holds two cubes and is centred (axis = row 1: the B row)
        let cubes = [(0, 'B'), (0, 'C'), (1, 'A'), (1, 'B'), (1, 'C'), (6, 'A'), (6, 'B'), (6, 'C')];
        let y = layout_rows(&cubes, &[0]);
        assert_eq!(y[2], 0.0); // A of slice 1 keeps its row
        // the two cubes of slice 0 are symmetric about the axis (row 1)
        assert!((y[0] - 0.5).abs() < 1e-6 && (y[1] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn cubes_come_from_the_records_and_the_photos_inside_their_slice_range() {
        let mut photos = BTreeMap::new();
        for k in [(1, 'A'), (1, 'B'), (2, 'A'), (9, 'A')] {
            photos.insert(k, PathBuf::from(format!("{}{}.png", k.1, k.0)));
        }
        let cubes = build("DC26AB999", &names(&["A1", "B1", "A2", "junk"]), &photos);
        // the photo of slice 9 is outside the slice range of the records (1..2): not part of the core shown
        assert_eq!(cubes.len(), 3);
        let a1 = cubes.iter().find(|c| c.name() == "A1").unwrap();
        assert_eq!(a1.recs, vec![0]);
        assert!(a1.photo.is_some());
    }
}
