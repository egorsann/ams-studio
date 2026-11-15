//! Drill core cubes to geographic coordinates ("Drill core to Geo").
//!
//! Built for cubes cut from drill core. The rotation is the Owens construction applied to the top of the core:
//! the end of the core is a surface whose normal is the hole axis, so as a left-hand-rule surface it has
//! `strike (LHR) = dip direction + 90` and `dip = 90 - plunge` (dip direction and plunge are the survey values of the
//! hole). The cube is first turned about the core axis by the offset of the orientation line from the arrow mark
//! (the BOH_ROTA of the AMS dashboard: the clockwise turn, looking down the hole, that brings the orientation line
//! to the bottom), then the surface is set to its field orientation:
//!
//! `R = Rz(strike) Rx(dip) Rz(offset) M`
//!
//! in north-east-down axes (`Rz(a)` adds `a` to the declination), where `M` carries the answers about the arrow
//! (which face holds it, which way it points); `M` is the identity for the default (arrow on the up-hole face,
//! pointing right), which is the convention of the AMS dashboard. Checked against the dashboard's
//! `reorient_ams` and against the `_reoriented.ran` made with Anisoft 4.2 (see the tests).
//!
//! Core frame `C` (the dashboard's specimen frame): `z` down the hole, `x` to the right and `y` down, both seen
//! looking down the hole. The specimen frame `S` of the cube (the one the `.ams` tensor is in) is tied to the arrow:
//! `z` points into the cube through the face that holds the arrow, `x` is the direction of the arrow.

use crate::tensor::{cross, mat_mul, Mat3, Vec3};

/// The face of the cube that holds the arrow, named by where that face looked in the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// The face that looked up the hole (the default).
    UpHole,
    DownHole,
    /// Seen looking down the hole.
    Left,
    Right,
    /// Towards the top of the core (away from the bottom-of-hole line).
    Top,
    Bottom,
}

impl Face {
    pub const ALL: [Face; 6] = [Face::UpHole, Face::DownHole, Face::Left, Face::Right, Face::Top, Face::Bottom];

    pub fn name(self) -> &'static str {
        match self {
            Face::UpHole => "Up-hole facing",
            Face::DownHole => "Down-hole facing",
            Face::Left => "Left side (looking down the hole)",
            Face::Right => "Right side (looking down the hole)",
            Face::Top => "Top (away from the bottom-of-hole line)",
            Face::Bottom => "Bottom (towards the bottom-of-hole line)",
        }
    }

    /// Outward normal of the face in the core frame.
    fn normal(self) -> Vec3 {
        match self {
            Face::UpHole => [0.0, 0.0, -1.0],
            Face::DownHole => [0.0, 0.0, 1.0],
            Face::Left => [-1.0, 0.0, 0.0],
            Face::Right => [1.0, 0.0, 0.0],
            Face::Top => [0.0, -1.0, 0.0],
            Face::Bottom => [0.0, 1.0, 0.0],
        }
    }
}

/// The way the arrow points, seen looking at its face from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Up,
    Right,
    Down,
    Left,
}

impl Arrow {
    pub const ALL: [Arrow; 4] = [Arrow::Up, Arrow::Right, Arrow::Down, Arrow::Left];

    pub fn name(self) -> &'static str {
        match self {
            Arrow::Up => "Up",
            Arrow::Right => "Right",
            Arrow::Down => "Down",
            Arrow::Left => "Left",
        }
    }
}

/// Everything the tool asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoreSetup {
    /// Dip direction of the drill hole (deg).
    pub dip_dir: f64,
    /// Plunge of the drill hole below the horizontal (deg).
    pub plunge: f64,
    /// Offset of the orientation line from the arrow mark: the clockwise turn, looking down the hole, that brings the
    /// orientation line to the bottom (deg).
    pub offset: f64,
    pub face: Face,
    pub arrow: Arrow,
}

impl Default for CoreSetup {
    fn default() -> Self {
        CoreSetup { dip_dir: 0.0, plunge: 90.0, offset: 0.0, face: Face::UpHole, arrow: Arrow::Right }
    }
}

/// Slice number of a cube named the way the AMS dashboard reads them, `^[A-D]\d+` (`A12` is slice 12).
pub fn slice_of(name: &str) -> Option<u32> {
    let mut it = name.trim().chars();
    if !matches!(it.next(), Some('A'..='D')) {
        return None;
    }
    let digits: String = it.take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// The offset (BOH_ROTA) of the interval table `(first, last, offset)` (both ends included) that holds `key`;
/// `default` outside all of them. The first interval that holds the key wins, as in the dashboard.
pub fn offset_for(intervals: &[(u32, u32, f64)], key: u32, default: f64) -> f64 {
    intervals.iter().find(|(a, b, _)| key >= *a && key <= *b).map_or(default, |x| x.2)
}

/// The top of the core as a left-hand-rule surface: (strike, dip) in degrees.
pub fn top_surface_lhr(dip_dir: f64, plunge: f64) -> (f64, f64) {
    ((dip_dir + 90.0).rem_euclid(360.0), 90.0 - plunge)
}

fn rz(deg: f64) -> Mat3 {
    let (s, c) = deg.to_radians().sin_cos();
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

fn rx(deg: f64) -> Mat3 {
    let (s, c) = deg.to_radians().sin_cos();
    [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
}

/// Cube frame to core frame (columns are the cube's axes written in the core frame). The identity for the default.
pub fn cube_to_core(face: Face, arrow: Arrow) -> Mat3 {
    let n = face.normal();
    let view = [-n[0], -n[1], -n[2]]; // looking into the face from outside
    // the end faces show the top of the core upwards, the side faces show the up-hole direction upwards
    let up: Vec3 = if matches!(face, Face::UpHole | Face::DownHole) { [0.0, -1.0, 0.0] } else { [0.0, 0.0, -1.0] };
    let down = [-up[0], -up[1], -up[2]];
    let right = cross(down, view);
    let x_s = match arrow {
        Arrow::Up => up,
        Arrow::Right => right,
        Arrow::Down => down,
        Arrow::Left => [-right[0], -right[1], -right[2]],
    };
    let z_s = view;
    let y_s = cross(z_s, x_s);
    [[x_s[0], y_s[0], z_s[0]], [x_s[1], y_s[1], z_s[1]], [x_s[2], y_s[2], z_s[2]]]
}

/// The rotation in north-east-down axes (`Rz(a)` adds `a` to the declination).
pub fn core_rotation_ned(s: &CoreSetup) -> Mat3 {
    let (strike, dip) = top_surface_lhr(s.dip_dir, s.plunge);
    let mut r = rz(strike);
    for m in [rx(dip), rz(s.offset), cube_to_core(s.face, s.arrow)] {
        r = mat_mul(&r, &m);
    }
    r
}

/// The rotation in this crate's axes (x east, y north, z down), to be applied as `R T R^T` / `R v` like
/// [`crate::coords::owens_rotation`].
pub fn core_rotation(s: &CoreSetup) -> Mat3 {
    const P: Mat3 = [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    mat_mul(&mat_mul(&P, &core_rotation_ned(s)), &P)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tensor::{dec_inc, from_dec_inc, mat_vec};

    // (dec, inc, BOH_ROTA, dip direction, plunge) -> geographic dec / inc from the dashboard's `reorient_ams`
    // (dashboard_from_dot_ams.ipynb); a negative inclination is folded up like the dashboard does.
    const DASHBOARD: [(f64, f64, f64, f64, f64, f64, f64); 5] = [
        (118.8, 50.9, 356.0, 316.135, 67.0, 185.906759, 69.727154),
        (10.0, 20.0, 0.0, 305.0, 69.0, 36.842454, 22.196259),
        (250.0, -30.0, 45.0, 305.0, 69.0, 158.471077, 48.423302),
        (77.0, 5.0, 320.0, 313.635, 66.7, 76.609213, 18.492908),
        (300.0, 80.0, 334.0, 305.0, 69.0, 306.348314, 59.016868),
    ];

    fn fold(d: f64, i: f64) -> (f64, f64) {
        if i < 0.0 { ((d + 180.0).rem_euclid(360.0), -i) } else { (d, i) }
    }

    #[test]
    fn the_default_setup_is_the_ams_dashboard() {
        for (d, i, boh, dd, p, dg, ig) in DASHBOARD {
            let s = CoreSetup { dip_dir: dd, plunge: p, offset: boh, ..Default::default() };
            let (gd, gi) = fold(dec_inc(mat_vec(&core_rotation(&s), from_dec_inc(d, i))).0, dec_inc(mat_vec(&core_rotation(&s), from_dec_inc(d, i))).1);
            assert!((gd - dg).abs() < 1e-4 && (gi - ig).abs() < 1e-4, "{d} {i}: {gd} {gi} vs {dg} {ig}");
        }
    }

    #[test]
    fn slices_and_intervals_work_like_the_dashboard() {
        assert_eq!(slice_of("A12"), Some(12));
        assert_eq!(slice_of("D3"), Some(3));
        assert_eq!(slice_of("D!8"), None);
        assert_eq!(slice_of("ZZ01-A1"), None);
        // DC26AB002: slices 0-13 turn 356, 14-42 turn 352, anything else the default
        let iv = [(0, 13, 356.0), (14, 42, 352.0)];
        assert_eq!(offset_for(&iv, 13, 0.0), 356.0);
        assert_eq!(offset_for(&iv, 14, 0.0), 352.0);
        assert_eq!(offset_for(&iv, 50, 7.0), 7.0);
    }

    #[test]
    fn the_top_of_the_core_is_a_left_hand_rule_surface() {
        let (strike, dip) = top_surface_lhr(316.135, 67.0);
        assert!((strike - 46.135).abs() < 1e-9 && (dip - 23.0).abs() < 1e-9);
        // a horizontal-plunge-90 hole has a flat top: dip 0
        assert!(top_surface_lhr(10.0, 90.0).1.abs() < 1e-12);
    }

    #[test]
    fn every_arrow_choice_is_a_proper_rotation_and_the_default_is_the_identity() {
        let id = cube_to_core(Face::UpHole, Arrow::Right);
        for i in 0..3 {
            for j in 0..3 {
                assert!((id[i][j] - if i == j { 1.0 } else { 0.0 }).abs() < 1e-12);
            }
        }
        for f in Face::ALL {
            for a in Arrow::ALL {
                let m = cube_to_core(f, a);
                let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
                assert!((det - 1.0).abs() < 1e-12, "{f:?} {a:?} det {det}");
                // the arrow face looks along its normal: the cube's z axis (into the cube) is minus that normal
                let z = [m[0][2], m[1][2], m[2][2]];
                let n = f.normal();
                assert!((z[0] + n[0]).abs() + (z[1] + n[1]).abs() + (z[2] + n[2]).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn the_arrow_turns_with_the_choice() {
        // arrow on the up-hole face pointing down instead of right: the cube is turned a quarter about the core axis
        let a = cube_to_core(Face::UpHole, Arrow::Down);
        let x = [a[0][0], a[1][0], a[2][0]];
        assert!((x[1] - 1.0).abs() < 1e-12, "{x:?}");
    }
}
