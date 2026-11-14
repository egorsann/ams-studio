//! Symmetric 3x3 tensor maths: eigen-decomposition, directions, shape parameters.

pub type Vec3 = [f64; 3];
pub type Mat3 = [[f64; 3]; 3];

pub fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

pub fn transpose(a: &Mat3) -> Mat3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = a[j][i];
        }
    }
    r
}

/// R T R^T
pub fn rotate_tensor(t: &Mat3, r: &Mat3) -> Mat3 {
    mat_mul(&mat_mul(r, t), &transpose(r))
}

pub fn mat_vec(a: &Mat3, v: Vec3) -> Vec3 {
    [
        a[0][0] * v[0] + a[0][1] * v[1] + a[0][2] * v[2],
        a[1][0] * v[0] + a[1][1] * v[1] + a[1][2] * v[2],
        a[2][0] * v[0] + a[2][1] * v[1] + a[2][2] * v[2],
    ]
}

pub fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn norm(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}

/// Symmetric tensor from the six independent components.
pub fn sym(xx: f64, yy: f64, zz: f64, xy: f64, xz: f64, yz: f64) -> Mat3 {
    [[xx, xy, xz], [xy, yy, yz], [xz, yz, zz]]
}

/// Eigen-decomposition of a symmetric 3x3 matrix (cyclic Jacobi).
/// Returns eigenvalues in descending order and the matching unit eigenvectors
/// (`vecs[i]` belongs to `vals[i]`).
pub fn eig_sym(m: &Mat3) -> ([f64; 3], [Vec3; 3]) {
    let mut a = *m;
    let mut v: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..64 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off < 1e-300 {
            break;
        }
        for &(p, q) in &[(0usize, 1usize), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-300 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for k in 0..3 {
                let (akp, akq) = (a[k][p], a[k][q]);
                a[k][p] = c * akp - s * akq;
                a[k][q] = s * akp + c * akq;
            }
            for k in 0..3 {
                let (apk, aqk) = (a[p][k], a[q][k]);
                a[p][k] = c * apk - s * aqk;
                a[q][k] = s * apk + c * aqk;
            }
            for k in 0..3 {
                let (vkp, vkq) = (v[k][p], v[k][q]);
                v[k][p] = c * vkp - s * vkq;
                v[k][q] = s * vkp + c * vkq;
            }
        }
    }
    let mut idx = [0usize, 1, 2];
    let vals = [a[0][0], a[1][1], a[2][2]];
    idx.sort_by(|&i, &j| vals[j].partial_cmp(&vals[i]).unwrap_or(std::cmp::Ordering::Equal));
    let out_vals = [vals[idx[0]], vals[idx[1]], vals[idx[2]]];
    let mut out_vecs = [[0.0; 3]; 3];
    for (n, &i) in idx.iter().enumerate() {
        out_vecs[n] = [v[0][i], v[1][i], v[2][i]];
    }
    (out_vals, out_vecs)
}

/// Declination / inclination (degrees) of an axis, as in the vault notes:
/// flip to the z >= 0 hemisphere, `inc = asin(z)`, `dec = atan2(x, y)`.
pub fn dec_inc(v: Vec3) -> (f64, f64) {
    let v = if v[2] < 0.0 { [-v[0], -v[1], -v[2]] } else { v };
    let n = norm(v).max(1e-300);
    let inc = (v[2] / n).clamp(-1.0, 1.0).asin().to_degrees();
    let dec = v[0].atan2(v[1]).to_degrees().rem_euclid(360.0);
    (dec, inc)
}

/// Unit vector for a (dec, inc) pair - the inverse of [`dec_inc`].
pub fn from_dec_inc(dec: f64, inc: f64) -> Vec3 {
    let (d, i) = (dec.to_radians(), inc.to_radians());
    [i.cos() * d.sin(), i.cos() * d.cos(), i.sin()]
}

/// AGICO/Anisoft anisotropy parameters, computed from the (normalised)
/// eigenvalues `k1 >= k2 >= k3`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub l: f64,
    pub f: f64,
    pub p: f64,
    pub pj: f64,
    pub t: f64,
    pub u: f64,
    pub q: f64,
    pub e: f64,
}

impl Shape {
    /// `None` when the tensor is degenerate: `k3 < 0` (AGICO writes -2.0 there)
    /// or `k1 == k3` (no anisotropy to describe).
    pub fn from_k(k: [f64; 3]) -> Option<Shape> {
        let [k1, k2, k3] = k;
        if k3 <= 0.0 || k1 == k3 {
            return None;
        }
        let (n1, n2, n3) = (k1.ln(), k2.ln(), k3.ln());
        let nm = (n1 + n2 + n3) / 3.0;
        let pj = (2.0 * ((n1 - nm).powi(2) + (n2 - nm).powi(2) + (n3 - nm).powi(2))).sqrt().exp();
        let t = (2.0 * n2 - n1 - n3) / (n1 - n3);
        Some(Shape {
            l: k1 / k2,
            f: k2 / k3,
            p: k1 / k3,
            pj,
            t,
            u: (2.0 * k2 - k1 - k3) / (k1 - k3),
            q: (k1 - k2) / ((k1 + k2) / 2.0 - k3),
            e: k2 * k2 / (k1 * k3),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eigen_of_diagonal() {
        let (v, e) = eig_sym(&sym(1.0, 3.0, 2.0, 0.0, 0.0, 0.0));
        assert_eq!(v, [3.0, 2.0, 1.0]);
        assert!((e[0][1].abs() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn eigen_reconstructs_matrix() {
        let m = sym(1.2, 1.0, 0.8, 0.07, -0.03, 0.05);
        let (vals, vecs) = eig_sym(&m);
        for r in 0..3 {
            for c in 0..3 {
                let x: f64 = (0..3).map(|i| vals[i] * vecs[i][r] * vecs[i][c]).sum();
                assert!((x - m[r][c]).abs() < 1e-12);
            }
        }
        assert!(vals[0] >= vals[1] && vals[1] >= vals[2]);
    }

    #[test]
    fn dec_inc_roundtrip() {
        let v = from_dec_inc(123.4, 45.6);
        let (d, i) = dec_inc(v);
        assert!((d - 123.4).abs() < 1e-9 && (i - 45.6).abs() < 1e-9);
    }

    #[test]
    fn shape_isotropic_limit_and_sentinel() {
        assert!(Shape::from_k([1.0, 1.0, 1.0]).is_none());
        assert!(Shape::from_k([1.2, 1.0, -0.1]).is_none());
        let s = Shape::from_k([1.2, 1.0, 0.8]).unwrap();
        assert!((s.p - 1.5).abs() < 1e-12 && (s.l - 1.2).abs() < 1e-12);
        assert!(s.pj > s.p * 0.99);
    }
}
