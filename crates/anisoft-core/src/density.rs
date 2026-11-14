//! Contour Plot (F12): axial density of directions on the equal-area net.

use crate::tensor::{dot, Vec3};

pub struct DensityGrid {
    pub n: usize,
    /// Row-major `n x n` over x,y in [-1, 1]; `NaN` outside the unit circle. Values in % of N.
    pub z: Vec<f64>,
    pub max: f64,
}

/// Equal-area (Schmidt) projection of the lower hemisphere. Returns (x east, y north).
pub fn project(dec: f64, inc: f64) -> (f64, f64) {
    let theta = (90.0 - inc).to_radians();
    let r = 2f64.sqrt() * (theta / 2.0).sin();
    let d = dec.to_radians();
    (r * d.sin(), r * d.cos())
}

/// Inverse of [`project`] for a point inside the unit circle: unit vector
/// (x east, y north, z towards the centre of the lower hemisphere = sin(inc)).
pub fn unproject(x: f64, y: f64) -> Option<Vec3> {
    let r2 = x * x + y * y;
    if r2 > 1.0 {
        return None;
    }
    // cos(theta) = 1 - r^2, sin(theta) = r*sqrt(2 - r^2)
    let h = (2.0 - r2).sqrt();
    Some([x * h, y * h, 1.0 - r2])
}

/// Exponential-kernel density (Robin & Jowett style): weight = exp(k (|cos| - 1)).
pub fn density(axes: &[Vec3], n: usize, k: f64) -> DensityGrid {
    let mut z = vec![f64::NAN; n * n];
    let mut max: f64 = 0.0;
    for iy in 0..n {
        for ix in 0..n {
            let x = -1.0 + 2.0 * (ix as f64 + 0.5) / n as f64;
            let y = 1.0 - 2.0 * (iy as f64 + 0.5) / n as f64;
            let Some(p) = unproject(x, y) else { continue };
            let s: f64 = axes.iter().map(|a| (k * (dot(*a, p).abs() - 1.0)).exp()).sum();
            let v = 100.0 * s / axes.len().max(1) as f64;
            z[iy * n + ix] = v;
            if v > max { max = v; }
        }
    }
    DensityGrid { n, z, max }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tensor::from_dec_inc;

    #[test]
    fn projection_roundtrip() {
        let (x, y) = project(70.0, 35.0);
        let p = unproject(x, y).unwrap();
        let q = from_dec_inc(70.0, 35.0);
        // axial: same line, so compare up to sign
        assert!(dot(p, q).abs() > 1.0 - 1e-9, "{p:?} vs {q:?}");
    }

    #[test]
    fn centre_and_rim() {
        let (x, y) = project(0.0, 90.0);
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12);
        let (x, y) = project(90.0, 0.0);
        assert!((x - 1.0).abs() < 1e-12 && y.abs() < 1e-12);
    }

    #[test]
    fn density_peaks_near_cluster() {
        let pts: Vec<Vec3> = (0..20).map(|i| from_dec_inc(90.0 + i as f64 * 0.5, 20.0)).collect();
        let g = density(&pts, 41, 40.0);
        let (px, py) = project(95.0, 20.0);
        let ix = (((px + 1.0) / 2.0) * 41.0) as usize;
        let iy = (((1.0 - py) / 2.0) * 41.0) as usize;
        assert!(g.z[iy * 41 + ix] > 0.8 * g.max);
    }
}
