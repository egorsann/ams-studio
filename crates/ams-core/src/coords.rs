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
}
