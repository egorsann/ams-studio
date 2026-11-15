//! Contour Plot (F12): axial density of directions on the equal-area net.
//!
//! Kernel: `exp(k (cos^2 t - 1))` with the smoothing value `k` (default 64), normalised so that a
//! uniform distribution gives 1 ("% per 1 % of area"). Calibrated against the original: its
//! maxima for two test sets were 27.1 and 18.7, this kernel gives 26.4 and 18.8 - close, not exact.
//! The eigenvalue table (eigen-decomposition of the orientation matrix `sum v v^T / N`) matches
//! the original to 5 digits.

use crate::tensor::{dec_inc, dot, eig_sym, Vec3};

pub struct DensityGrid {
    pub n: usize,
    /// Row-major `n x n` over x,y in [-1, 1] (row 0 = top = y 1); `NaN` outside the unit circle.
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

/// Mean of `exp(k (c^2 - 1))` over the sphere: the density of a uniform distribution.
fn uniform_level(k: f64) -> f64 {
    // integral over c in [0, 1]; Simpson with enough panels for any k used here
    let m = 4000;
    let h = 1.0 / m as f64;
    let f = |c: f64| (k * (c * c - 1.0)).exp();
    let mut s = f(0.0) + f(1.0);
    for i in 1..m {
        s += f(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    s * h / 3.0
}

pub fn density(axes: &[Vec3], n: usize, k: f64) -> DensityGrid {
    let mut z = vec![f64::NAN; n * n];
    let mut max: f64 = 0.0;
    let norm = axes.len().max(1) as f64 * uniform_level(k);
    for iy in 0..n {
        for ix in 0..n {
            let x = -1.0 + 2.0 * (ix as f64 + 0.5) / n as f64;
            let y = 1.0 - 2.0 * (iy as f64 + 0.5) / n as f64;
            let Some(p) = unproject(x, y) else { continue };
            let s: f64 = axes.iter().map(|a| { let c = dot(*a, p); (k * (c * c - 1.0)).exp() }).sum();
            let v = s / norm;
            z[iy * n + ix] = v;
            if v > max {
                max = v;
            }
        }
    }
    DensityGrid { n, z, max }
}

/// Density at one point of the net (same scale as the grid).
pub fn density_at(axes: &[Vec3], p: Vec3, k: f64) -> f64 {
    let norm = axes.len().max(1) as f64 * uniform_level(k);
    axes.iter().map(|a| { let c = dot(*a, p); (k * (c * c - 1.0)).exp() }).sum::<f64>() / norm
}

/// Peak density, found on a fine spiral over the lower hemisphere (the grid under-reads sharp peaks).
pub fn peak(axes: &[Vec3], k: f64) -> f64 {
    let m = 6000;
    let mut best = 0.0f64;
    let ga = std::f64::consts::PI * (1.0 + 5f64.sqrt());
    for i in 0..m {
        let z = 1.0 - (i as f64 + 0.5) / m as f64;
        let r = (1.0 - z * z).sqrt();
        let phi = ga * (i as f64 + 0.5);
        best = best.max(density_at(axes, [r * phi.cos(), r * phi.sin(), z], k));
    }
    // refine at the data points themselves: peaks sit on or near them
    for a in axes {
        best = best.max(density_at(axes, *a, k));
    }
    best
}

/// Colour-scale step and the contour levels for a density maximum.
/// Levels run 1, 1+step, 1+2 step ... below the maximum (the original shows 1, 3, 5 ... for maxima 18.7 and 27.1).
pub fn levels_for(max: f64) -> (f64, Vec<f64>) {
    let step = if max < 12.0 { 1.0 } else if max < 45.0 { 2.0 } else if max < 110.0 { 5.0 } else { 10.0 };
    let mut v = vec![];
    let mut l = 1.0;
    while l < max {
        v.push(l);
        l += step;
    }
    (step, v)
}

/// Marching squares: line segments of the iso-line `level`, in net coordinates ([-1,1], y up).
pub fn contour_segments(g: &DensityGrid, level: f64) -> Vec<[(f64, f64); 2]> {
    let n = g.n;
    let at = |ix: usize, iy: usize| g.z[iy * n + ix];
    let pos = |ix: f64, iy: f64| (-1.0 + 2.0 * (ix + 0.5) / n as f64, 1.0 - 2.0 * (iy + 0.5) / n as f64);
    let mut out = vec![];
    for iy in 0..n - 1 {
        for ix in 0..n - 1 {
            let (a, b, c, d) = (at(ix, iy), at(ix + 1, iy), at(ix + 1, iy + 1), at(ix, iy + 1));
            if !(a.is_finite() && b.is_finite() && c.is_finite() && d.is_finite()) {
                continue;
            }
            // edge crossing points: top, right, bottom, left
            let lerp = |u: f64, v: f64| if (v - u).abs() < 1e-300 { 0.5 } else { ((level - u) / (v - u)).clamp(0.0, 1.0) };
            let (fx, fy) = (ix as f64, iy as f64);
            let top = || pos(fx + lerp(a, b), fy);
            let right = || pos(fx + 1.0, fy + lerp(b, c));
            let bottom = || pos(fx + lerp(d, c), fy + 1.0);
            let left = || pos(fx, fy + lerp(a, d));
            let case = (a >= level) as u8 | ((b >= level) as u8) << 1 | ((c >= level) as u8) << 2 | ((d >= level) as u8) << 3;
            let mut push = |p: (f64, f64), q: (f64, f64)| out.push([p, q]);
            match case {
                1 | 14 => push(top(), left()),
                2 | 13 => push(top(), right()),
                3 | 12 => push(left(), right()),
                4 | 11 => push(right(), bottom()),
                6 | 9 => push(top(), bottom()),
                7 | 8 => push(left(), bottom()),
                5 => {
                    push(top(), right());
                    push(left(), bottom());
                }
                10 => {
                    push(top(), left());
                    push(right(), bottom());
                }
                _ => {}
            }
        }
    }
    out
}

/// Orientation matrix `sum v v^T / N` of the axes: eigenvalues (descending), eigenvectors and their dec/inc.
pub fn orientation_eigen(axes: &[Vec3]) -> ([f64; 3], [Vec3; 3], [(f64, f64); 3]) {
    let n = axes.len().max(1) as f64;
    let mut m = [[0.0; 3]; 3];
    for a in axes {
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += a[i] * a[j] / n;
            }
        }
    }
    let (w, v) = eig_sym(&m);
    (w, v, [dec_inc(v[0]), dec_inc(v[1]), dec_inc(v[2])])
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
        let g = density(&pts, 41, 64.0);
        let (px, py) = project(95.0, 20.0);
        let ix = (((px + 1.0) / 2.0) * 41.0) as usize;
        let iy = (((1.0 - py) / 2.0) * 41.0) as usize;
        assert!(g.z[iy * 41 + ix] > 0.8 * g.max);
    }

    #[test]
    fn a_uniform_spread_gives_about_one() {
        // many random-ish directions: density near 1 on average
        let mut pts = vec![];
        let ga = std::f64::consts::PI * (1.0 + 5f64.sqrt());
        for i in 0..4000 {
            let z = 1.0 - (i as f64 + 0.5) / 4000.0 * 2.0;
            let r = (1.0 - z * z).sqrt();
            pts.push([r * (ga * i as f64).cos(), r * (ga * i as f64).sin(), z]);
        }
        let g = density(&pts, 21, 64.0);
        let vals: Vec<f64> = g.z.iter().copied().filter(|v| v.is_finite()).collect();
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        assert!((mean - 1.0).abs() < 0.05, "{mean}");
        assert!(peak(&pts, 64.0) < 1.1);
    }

    #[test]
    fn levels_follow_the_original_scale() {
        let (s, l) = levels_for(27.1);
        assert_eq!(s, 2.0);
        assert_eq!(&l[..4], &[1.0, 3.0, 5.0, 7.0]);
        assert_eq!(*l.last().unwrap(), 27.0);
        let (_, l) = levels_for(18.7);
        assert_eq!(*l.last().unwrap(), 17.0);
    }

    #[test]
    fn contour_lines_surround_the_peak() {
        let pts: Vec<Vec3> = (0..30).map(|i| from_dec_inc(90.0 + (i % 5) as f64, 30.0 + (i / 5) as f64)).collect();
        let g = density(&pts, 80, 64.0);
        let segs = contour_segments(&g, g.max * 0.5);
        assert!(segs.len() > 8);
        let (px, py) = project(92.0, 32.5);
        for s in &segs {
            for p in s {
                assert!(((p.0 - px).powi(2) + (p.1 - py).powi(2)).sqrt() < 0.5);
            }
        }
    }

    #[test]
    fn orientation_eigen_of_a_line_and_a_plane() {
        // all axes along one direction: eigenvalues 1, 0, 0
        let v = from_dec_inc(40.0, 20.0);
        let (w, e, di) = orientation_eigen(&[v, v, v]);
        assert!((w[0] - 1.0).abs() < 1e-12 && w[1].abs() < 1e-12);
        assert!(dot(e[0], v).abs() > 1.0 - 1e-12);
        assert!((di[0].0 - 40.0).abs() < 1e-6 && (di[0].1 - 20.0).abs() < 1e-6);
    }
}
