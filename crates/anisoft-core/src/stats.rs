//! Group statistics (Group Statistics, F9): Jelinek-style mean tensor.
//!
//! Each tensor is normalised to a mean eigenvalue of 1, the arithmetic mean tensor is
//! decomposed, and the scatter of the normalised tensors in the mean's principal frame
//! gives approximate 95 % confidence half-angles. The angles use a Student-t factor, which
//! is an approximation of Jelinek's F-based formula and is not verified against Anisoft.

use crate::tensor::{eig_sym, dec_inc, mat_mul, transpose, Mat3, Shape};

#[derive(Debug, Clone)]
pub struct GroupStats {
    pub n: usize,
    pub mean_tensor: Mat3,
    pub k: [f64; 3],
    pub dec_inc: [(f64, f64); 3],
    pub k_sd: [f64; 3],
    pub shape: Option<Shape>,
    /// Approximate 95 % half-angles (deg) of the K1-K2, K2-K3 and K1-K3 planes.
    pub eps12: f64,
    pub eps23: f64,
    pub eps13: f64,
    /// Mean and standard deviation of P, Pj, T over the specimens (those with valid shape).
    pub mean_p: (f64, f64),
    pub mean_pj: (f64, f64),
    pub mean_t: (f64, f64),
    pub mean_km: f64,
}

fn mean_sd(v: &[f64]) -> (f64, f64) {
    if v.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let m = v.iter().sum::<f64>() / v.len() as f64;
    if v.len() < 2 {
        return (m, 0.0);
    }
    let var = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64;
    (m, var.sqrt())
}

/// Two-sided 95 % Student-t quantile (Cornish-Fisher expansion, fine for nu >= 2).
pub fn t95(nu: f64) -> f64 {
    let z: f64 = 1.959964;
    z + (z.powi(3) + z) / (4.0 * nu)
        + (5.0 * z.powi(5) + 16.0 * z.powi(3) + 3.0 * z) / (96.0 * nu * nu)
        + (3.0 * z.powi(7) + 19.0 * z.powi(5) + 17.0 * z.powi(3) - 15.0 * z) / (384.0 * nu.powi(3))
}

/// `tensors` are raw specimen (or rotated) tensors, `km` the matching bulk susceptibilities.
pub fn group_stats(tensors: &[Mat3], km: &[f64]) -> Option<GroupStats> {
    let n = tensors.len();
    if n == 0 {
        return None;
    }
    let normed: Vec<Mat3> = tensors
        .iter()
        .map(|t| {
            let m = (t[0][0] + t[1][1] + t[2][2]) / 3.0;
            let mut r = *t;
            if m != 0.0 {
                for row in r.iter_mut() {
                    for x in row.iter_mut() {
                        *x /= m;
                    }
                }
            }
            r
        })
        .collect();
    let mut mean = [[0.0; 3]; 3];
    for t in &normed {
        for i in 0..3 {
            for j in 0..3 {
                mean[i][j] += t[i][j] / n as f64;
            }
        }
    }
    let (k, vecs) = eig_sym(&mean);
    // principal-frame matrix: rows are the mean eigenvectors
    let p: Mat3 = [vecs[0], vecs[1], vecs[2]];
    let frames: Vec<Mat3> = normed.iter().map(|t| mat_mul(&mat_mul(&p, t), &transpose(&p))).collect();
    let se = |i: usize, j: usize| -> f64 {
        let v: Vec<f64> = frames.iter().map(|f| f[i][j]).collect();
        let (_, sd) = mean_sd(&v);
        sd / (n as f64).sqrt()
    };
    let k_sd = [se(0, 0), se(1, 1), se(2, 2)];
    let tq = if n > 2 { t95(n as f64 - 1.0) } else { f64::NAN };
    let eps = |i: usize, j: usize| -> f64 {
        let d = (k[i] - k[j]).abs();
        if d == 0.0 { 90.0 } else { (se(i, j) * tq / d).atan().to_degrees() }
    };
    let shapes: Vec<Shape> = normed.iter().filter_map(|t| Shape::from_k(eig_sym(t).0)).collect();
    let col = |f: fn(&Shape) -> f64| mean_sd(&shapes.iter().map(f).collect::<Vec<_>>());
    Some(GroupStats {
        n,
        mean_tensor: mean,
        k,
        dec_inc: [dec_inc(vecs[0]), dec_inc(vecs[1]), dec_inc(vecs[2])],
        k_sd,
        shape: Shape::from_k(k),
        eps12: eps(0, 1),
        eps23: eps(1, 2),
        eps13: eps(0, 2),
        mean_p: col(|s| s.p),
        mean_pj: col(|s| s.pj),
        mean_t: col(|s| s.t),
        mean_km: km.iter().sum::<f64>() / km.len().max(1) as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tensor::sym;

    #[test]
    fn identical_tensors_have_zero_scatter() {
        let t = sym(1.2, 1.0, 0.8, 0.0, 0.0, 0.0);
        let g = group_stats(&[t, t, t, t], &[1.0; 4]).unwrap();
        assert!((g.k[0] - 1.2).abs() < 1e-12 && g.k_sd.iter().all(|s| s.abs() < 1e-12));
        assert!(g.eps12 < 1e-9 && g.eps23 < 1e-9);
        assert!((g.mean_p.0 - 1.5).abs() < 1e-12);
    }

    #[test]
    fn scattered_tensors_widen_the_angles() {
        let a = sym(1.2, 1.0, 0.8, 0.01, 0.0, 0.0);
        let b = sym(1.2, 1.0, 0.8, -0.03, 0.02, 0.0);
        let c = sym(1.2, 1.0, 0.8, 0.0, -0.02, 0.04);
        let g = group_stats(&[a, b, c, a, b, c], &[1.0; 6]).unwrap();
        assert!(g.eps12 > 0.1 && g.eps13 > 0.1);
    }

    #[test]
    fn t_quantile_close_to_tables() {
        assert!((t95(10.0) - 2.228).abs() < 0.01);
        assert!((t95(30.0) - 2.042).abs() < 0.005);
    }
}
