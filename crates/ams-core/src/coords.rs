//! Coordinate systems (the S / G / P / T buttons).
//!
//! The `.ams` file does not carry the orientation parameters, so they are user
//! settings (Settings > Orientation Parameters). Conventions used here, which
//! have NOT been cross-checked against Anisoft's own dialog:
//! * Geographic: tilt the specimen by `dip` about its x axis, then rotate by `azimuth`
//!   about the vertical (so with dip = 0 every declination simply gains `azimuth`).
//! * Paleogeographic: geographic, then rotate by `-paleo_dec` about the vertical.
//! * Tectonic: geographic, then restore bedding (strike, right-hand rule, and dip) to horizontal.

use crate::tensor::{cross, dot, mat_mul, norm, Mat3, Vec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordSys {
    Specimen,
    Geographic,
    Paleo,
    Tectonic,
}

impl CoordSys {
    /// The word stored in a dependency file.
    pub fn token(self) -> &'static str {
        match self {
            CoordSys::Specimen => "specimen",
            CoordSys::Geographic => "geographic",
            CoordSys::Paleo => "paleo",
            CoordSys::Tectonic => "tectonic",
        }
    }

    pub fn from_token(t: &str) -> Option<CoordSys> {
        Some(match t {
            "specimen" => CoordSys::Specimen,
            "geographic" => CoordSys::Geographic,
            "paleo" => CoordSys::Paleo,
            "tectonic" => CoordSys::Tectonic,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orientation {
    pub azimuth: f64,
    pub dip: f64,
    pub bed_strike: f64,
    pub bed_dip: f64,
    pub paleo_dec: f64,
}

impl Default for Orientation {
    fn default() -> Self {
        Orientation { azimuth: 0.0, dip: 0.0, bed_strike: 0.0, bed_dip: 0.0, paleo_dec: 0.0 }
    }
}

const I3: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Rotation about the vertical axis taking +y towards +x by `deg` (declination += deg).
fn rot_z(deg: f64) -> Mat3 {
    let (s, c) = deg.to_radians().sin_cos();
    [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
}

fn rot_x(deg: f64) -> Mat3 {
    let (s, c) = deg.to_radians().sin_cos();
    [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
}

/// Rotation that takes unit vector `n` onto +z (shortest way).
fn rot_to_vertical(n: Vec3) -> Mat3 {
    let z = [0.0, 0.0, 1.0];
    let axis = cross(n, z);
    let s = norm(axis);
    if s < 1e-12 {
        return I3;
    }
    let a = [axis[0] / s, axis[1] / s, axis[2] / s];
    let theta = s.atan2(dot(n, z));
    let (st, ct) = theta.sin_cos();
    let k = [[0.0, -a[2], a[1]], [a[2], 0.0, -a[0]], [-a[1], a[0], 0.0]];
    let mut r = I3;
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = I3[i][j] * ct + st * k[i][j] + (1.0 - ct) * a[i] * a[j];
        }
    }
    r
}

impl Orientation {
    pub fn rotation(&self, cs: CoordSys) -> Mat3 {
        let geo = mat_mul(&rot_z(self.azimuth), &rot_x(self.dip));
        match cs {
            CoordSys::Specimen => I3,
            CoordSys::Geographic => geo,
            CoordSys::Paleo => mat_mul(&rot_z(-self.paleo_dec), &geo),
            CoordSys::Tectonic => {
                let (d, dd) = (self.bed_dip.to_radians(), (self.bed_strike + 90.0).to_radians());
                let n = [d.sin() * dd.sin(), d.sin() * dd.cos(), d.cos()];
                mat_mul(&rot_to_vertical(n), &geo)
            }
        }
    }
}

/// Rotation of Anisoft 4.2's "Specimen to Geographic Rotation (Owen's method)" (Edit > Spec to Geo (Owens)).
///
/// `fs`/`fd` are strike and dip of a plane in the field, `ds`/`dd` strike and dip of the same plane on the
/// drilling table (degrees). Rebuilt from the original's behaviour: it was driven with made-up tensors and
/// the formula below reproduces every result to the 0.1 degree the dialog prints, including strikes and dips
/// that are not physically sensible. The original's P1..P4 boxes make no difference to the result.
///
/// In north-east-down axes (x north, y east, z down) with `Rz(a)` adding `a` to the declination:
/// `R = Rz(fs) Rx(fd) Rz(-ds) Rx(-dd) Rz(ds')`, where `tan ds' = tan ds * cos dd`.
/// Returned in this crate's axes (x east, y north, z down), to be applied as `R T R^T` / `R v`.
pub fn owens_rotation(fs: f64, fd: f64, ds: f64, dd: f64) -> Mat3 {
    fn rz(deg: f64) -> Mat3 {
        let (s, c) = deg.to_radians().sin_cos();
        [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
    }
    let ds_r = ds.to_radians();
    let ds_p = (ds_r.sin() * dd.to_radians().cos()).atan2(ds_r.cos()).to_degrees();
    let mut r = rz(fs);
    for m in [rot_x(fd), rz(-ds), rot_x(-dd), rz(ds_p)] {
        r = mat_mul(&r, &m);
    }
    // north-east-down -> east-north-down: conjugate with the x/y swap
    const P: Mat3 = [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    mat_mul(&mat_mul(&P, &r), &P)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tensor::{dec_inc, from_dec_inc, mat_vec};

    #[test]
    fn azimuth_adds_to_declination() {
        let o = Orientation { azimuth: 30.0, ..Default::default() };
        let v = mat_vec(&o.rotation(CoordSys::Geographic), from_dec_inc(100.0, 40.0));
        let (d, i) = dec_inc(v);
        assert!((d - 130.0).abs() < 1e-9 && (i - 40.0).abs() < 1e-9);
    }

    #[test]
    fn tectonic_restores_bedding_normal_to_vertical() {
        let o = Orientation { bed_strike: 40.0, bed_dip: 25.0, ..Default::default() };
        let (d, dd) = (25f64.to_radians(), 130f64.to_radians());
        let n = [d.sin() * dd.sin(), d.sin() * dd.cos(), d.cos()];
        let v = mat_vec(&o.rotation(CoordSys::Tectonic), n);
        assert!((v[2] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn specimen_is_identity() {
        let v = mat_vec(&Orientation { azimuth: 77.0, dip: 12.0, ..Default::default() }.rotation(CoordSys::Specimen), [0.3, 0.4, 0.5]);
        assert_eq!(v, [0.3, 0.4, 0.5]);
    }
    #[test]
    fn owens_matches_anisoft_4_2() {
        // Made-up mean tensor axes in specimen coordinates (dec, inc), and what Anisoft 4.2 printed
        // after rotating them with (FS, FD, DS, DD).
        let spec = [(24.3, 1.3), (294.2, 4.0), (132.8, 85.8)];
        let cases: [([f64; 4], [(f64, f64); 3]); 8] = [
            ([0.0, 0.0, 0.0, 0.0], [(24.3, 1.3), (294.2, 4.0), (132.8, 85.8)]),
            ([0.0, 0.0, 0.0, 30.0], [(202.0, 10.7), (298.5, 31.0), (95.2, 56.8)]),
            ([0.0, 0.0, 45.0, 30.0], [(197.2, 25.7), (295.0, 15.7), (53.3, 59.3)]),
            ([0.0, 0.0, 135.0, 30.0], [(209.9, 7.0), (116.6, 24.7), (314.7, 64.2)]),
            ([123.0, 75.0, 200.0, 55.0], [(94.4, 8.6), (351.5, 55.8), (190.1, 32.8)]),
            ([250.0, 20.0, 330.0, 85.0], [(107.9, 9.4), (223.7, 69.1), (14.7, 18.4)]),
            ([45.0, 100.0, 60.0, 30.0], [(71.5, 21.7), (227.4, 66.5), (338.0, 8.7)]),
            ([310.0, 60.0, 15.0, 130.0], [(129.0, 26.1), (273.3, 58.9), (31.1, 15.7)]),
        ];
        for (a, want) in cases {
            let r = owens_rotation(a[0], a[1], a[2], a[3]);
            for (s, w) in spec.iter().zip(want.iter()) {
                let got = mat_vec(&r, from_dec_inc(s.0, s.1));
                let want = from_dec_inc(w.0, w.1);
                let ang = dot(got, want).abs().clamp(0.0, 1.0).acos().to_degrees();
                assert!(ang < 0.25, "{a:?}: {s:?} -> off by {ang:.2} degrees");
            }
        }
    }

    #[test]
    fn owens_is_a_proper_rotation() {
        let r = owens_rotation(123.0, 75.0, 200.0, 55.0);
        let i = mat_mul(&r, &crate::tensor::transpose(&r));
        for a in 0..3 {
            for b in 0..3 {
                assert!((i[a][b] - if a == b { 1.0 } else { 0.0 }).abs() < 1e-12);
            }
        }
        let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1]) - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0]) + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
        assert!((det - 1.0).abs() < 1e-12);
    }
}
