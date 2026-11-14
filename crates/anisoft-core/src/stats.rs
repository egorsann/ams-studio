//! Group statistics (Group Statistics, F9).
//!
//! Calibrated against the original: the mean tensor is the arithmetic mean of the tensors
//! exactly as stored (no extra normalisation), and the Jelinek confidence ellipse of each
//! principal axis is described by `tan(eps) = sqrt(eigenvalue)` of the sample covariance
//! (n - 1) of the two tilt components `t_ij / (tau_i - tau_j)`, `t_ik / (tau_i - tau_k)`,
//! where `t` are the specimen tensors rotated into the mean tensor's principal frame.
//! That reproduces all printed angles of the original for N = 12 and N = 60.
//! The bootstrap ellipses are this program's own (the original's could not be matched).

use crate::tensor::{dec_inc, dot, eig_sym, mat_mul, norm, transpose, Mat3, Shape, Vec3};

/// Confidence ellipse of one principal axis, in the tangent plane of the axis.
#[derive(Debug, Clone, Copy)]
pub struct Ellipse {
    /// Axis index (0 = K1) this ellipse belongs to.
    pub axis: usize,
    /// tan of the two semi-axes, largest first.
    pub tan_semi: [f64; 2],
    /// Unit directions of the semi-axes in the basis (e_j, e_k) of the other two principal axes.
    pub dirs: [[f64; 2]; 2],
}

impl Ellipse {
    /// Semi-axes in degrees (capped at 90).
    pub fn semi_deg(&self) -> [f64; 2] {
        [self.tan_semi[0].atan().to_degrees().min(90.0), self.tan_semi[1].atan().to_degrees().min(90.0)]
    }

    /// `n + 1` points of the closed outline as unit vectors (core vector order).
    pub fn outline(&self, vecs: &[Vec3; 3], n: usize) -> Vec<Vec3> {
        let (j, k) = others(self.axis);
        let ei = vecs[self.axis];
        let dir = |d: [f64; 2]| -> Vec3 {
            [d[0] * vecs[j][0] + d[1] * vecs[k][0], d[0] * vecs[j][1] + d[1] * vecs[k][1], d[0] * vecs[j][2] + d[1] * vecs[k][2]]
        };
        let (d1, d2) = (dir(self.dirs[0]), dir(self.dirs[1]));
        (0..=n)
            .map(|s| {
                let a = std::f64::consts::TAU * s as f64 / n as f64;
                let (sa, ca) = a.sin_cos();
                let v = [
                    ei[0] + self.tan_semi[0] * ca * d1[0] + self.tan_semi[1] * sa * d2[0],
                    ei[1] + self.tan_semi[0] * ca * d1[1] + self.tan_semi[1] * sa * d2[1],
                    ei[2] + self.tan_semi[0] * ca * d1[2] + self.tan_semi[1] * sa * d2[2],
                ];
                let m = norm(v);
                [v[0] / m, v[1] / m, v[2] / m]
            })
            .collect()
    }
}

fn others(i: usize) -> (usize, usize) {
    match i {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

#[derive(Debug, Clone)]
pub struct Bootstrap {
    /// Resampled mean axes per principal axis (unit vectors, flipped to the mean axis' hemisphere).
    pub points: [Vec<Vec3>; 3],
    /// 95 % ellipse of each cloud (None when degenerate).
    pub ellipse: [Option<Ellipse>; 3],
}

#[derive(Debug, Clone)]
pub struct GroupStats {
    pub n: usize,
    pub mean_tensor: Mat3,
    /// Mean-tensor eigenvalues as stored (these are "normed" for files whose tensors have trace 3).
    pub k: [f64; 3],
    pub vecs: [Vec3; 3],
    pub dec_inc: [(f64, f64); 3],
    /// Jelinek ellipses (None for n < 3 or equal eigenvalues).
    pub jelinek: [Option<Ellipse>; 3],
    pub bootstrap: Option<Bootstrap>,
    /// Shape parameters of the mean tensor.
    pub shape: Option<Shape>,
    /// (mean, standard deviation) of Km over the specimens.
    pub km: (f64, f64),
    /// (mean, standard deviation) over the specimens of L, F, P, Pj, T, U, Q, E.
    pub avg: [(f64, f64); 8],
}

pub const PARAM_NAMES: [&str; 8] = ["L", "F", "P", "Pj", "T", "U", "Q", "E"];

pub fn shape_values(s: &Shape) -> [f64; 8] {
    [s.l, s.f, s.p, s.pj, s.t, s.u, s.q, s.e]
}

pub fn mean_sd(v: &[f64]) -> (f64, f64) {
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

/// Eigen-decomposition of a symmetric 2x2 matrix: (eigenvalues descending, unit eigenvectors).
fn eig2(a: f64, b: f64, c: f64) -> ([f64; 2], [[f64; 2]; 2]) {
    // [[a, b], [b, c]]
    let m = (a + c) / 2.0;
    let d = (((a - c) / 2.0).powi(2) + b * b).sqrt();
    let (l1, l2) = (m + d, m - d);
    let v1 = if b.abs() > 1e-300 { let v = [l1 - c, b]; let n = (v[0] * v[0] + v[1] * v[1]).sqrt(); [v[0] / n, v[1] / n] } else if a >= c { [1.0, 0.0] } else { [0.0, 1.0] };
    ([l1, l2], [v1, [-v1[1], v1[0]]])
}

/// Sample covariance (n - 1) of two series -> ellipse tan-semi-axes scaled by `scale` and directions.
fn ellipse_from_tilts(axis: usize, a: &[f64], b: &[f64], scale: f64) -> Option<Ellipse> {
    let n = a.len();
    if n < 3 {
        return None;
    }
    let (ma, mb) = (a.iter().sum::<f64>() / n as f64, b.iter().sum::<f64>() / n as f64);
    let (mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (x, y) = (a[i] - ma, b[i] - mb);
        saa += x * x;
        sbb += y * y;
        sab += x * y;
    }
    let f = 1.0 / (n - 1) as f64;
    let (ev, dirs) = eig2(saa * f, sab * f, sbb * f);
    if !ev[0].is_finite() || ev[0] < 0.0 {
        return None;
    }
    Some(Ellipse { axis, tan_semi: [scale * ev[0].max(0.0).sqrt(), scale * ev[1].max(0.0).sqrt()], dirs })
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn mean_of(tensors: &[Mat3], idx: impl Iterator<Item = usize>, count: usize) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for i in idx {
        for r in 0..3 {
            for c in 0..3 {
                m[r][c] += tensors[i][r][c];
            }
        }
    }
    for r in m.iter_mut() {
        for v in r.iter_mut() {
            *v /= count as f64;
        }
    }
    m
}

/// Bootstrap of the mean tensor's axes: `b` resamples, deterministic for a given input.
pub fn bootstrap(tensors: &[Mat3], vecs: &[Vec3; 3], b: usize) -> Option<Bootstrap> {
    let n = tensors.len();
    if n < 3 {
        return None;
    }
    let mut rng = Lcg(0x9e3779b97f4a7c15 ^ n as u64);
    let mut pts: [Vec<Vec3>; 3] = [vec![], vec![], vec![]];
    for _ in 0..b {
        let picks: Vec<usize> = (0..n).map(|_| (rng.next() % n as u64) as usize).collect();
        let m = mean_of(tensors, picks.iter().copied(), n);
        let (_, v) = eig_sym(&m);
        for i in 0..3 {
            let s = if dot(v[i], vecs[i]) < 0.0 { -1.0 } else { 1.0 };
            pts[i].push([s * v[i][0], s * v[i][1], s * v[i][2]]);
        }
    }
    // 95 % region of a 2-D normal: Mahalanobis radius sqrt(chi2(2, 0.95)) = sqrt(-2 ln 0.05)
    let rad = (-2.0f64 * 0.05f64.ln()).sqrt();
    let mut ellipse = [None, None, None];
    for i in 0..3 {
        let (j, k) = others(i);
        let (mut a, mut c) = (vec![], vec![]);
        for p in &pts[i] {
            let di = dot(*p, vecs[i]);
            if di.abs() < 1e-9 {
                continue;
            }
            a.push(dot(*p, vecs[j]) / di);
            c.push(dot(*p, vecs[k]) / di);
        }
        ellipse[i] = ellipse_from_tilts(i, &a, &c, rad);
    }
    Some(Bootstrap { points: pts, ellipse })
}

/// `tensors` are the (rotated) specimen tensors as stored, `km` the matching bulk susceptibilities.
pub fn group_stats(tensors: &[Mat3], km: &[f64]) -> Option<GroupStats> {
    let n = tensors.len();
    if n == 0 {
        return None;
    }
    let mean = mean_of(tensors, 0..n, n);
    let (k, vecs) = eig_sym(&mean);
    let dec = [dec_inc(vecs[0]), dec_inc(vecs[1]), dec_inc(vecs[2])];

    // specimen tensors in the principal frame of the mean tensor
    let p: Mat3 = [vecs[0], vecs[1], vecs[2]];
    let pt = transpose(&p);
    let frames: Vec<Mat3> = tensors.iter().map(|t| mat_mul(&mat_mul(&p, t), &pt)).collect();
    let mut jelinek = [None, None, None];
    for i in 0..3 {
        let (j, kk) = others(i);
        let (dj, dk) = (k[i] - k[j], k[i] - k[kk]);
        if dj.abs() < 1e-300 || dk.abs() < 1e-300 {
            continue;
        }
        let a: Vec<f64> = frames.iter().map(|f| f[i][j] / dj).collect();
        let b: Vec<f64> = frames.iter().map(|f| f[i][kk] / dk).collect();
        jelinek[i] = ellipse_from_tilts(i, &a, &b, 1.0);
    }

    let shapes: Vec<[f64; 8]> = tensors.iter().filter_map(|t| Shape::from_k(eig_sym(t).0)).map(|s| shape_values(&s)).collect();
    let mut avg = [(f64::NAN, f64::NAN); 8];
    for (c, a) in avg.iter_mut().enumerate() {
        *a = mean_sd(&shapes.iter().map(|s| s[c]).collect::<Vec<_>>());
    }
    Some(GroupStats {
        n,
        mean_tensor: mean,
        k,
        vecs,
        dec_inc: dec,
        jelinek,
        bootstrap: bootstrap(tensors, &vecs, 500),
        shape: Shape::from_k(k),
        km: mean_sd(km),
        avg,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tensor::sym;

    // 12 made-up specimens: [xx, yy, zz, xy, xz, yz, Km]. The numbers in the assertions below
    // are what the original Anisoft 6 printed for exactly this file.
    const SMALL: [[f64; 7]; 12] = [
        [0.0024182191118597984, 0.002569140400737524, 0.0023129272740334272, 7.758787978673354e-05, 5.8316603826824576e-05, -2.4631690394016914e-05, 0.002433429006487131],
        [0.0025741795543581247, 0.0027806435246020555, 0.0020295046269893646, 5.2614559535868466e-05, 0.00018910120707005262, -4.541092494037002e-05, 0.0024614424910396338],
        [0.0003684732655528933, 0.00039622632903046906, 0.0003119500761386007, 2.2141319277579896e-05, 1.0983663742081262e-05, -6.078772003093036e-06, 0.0003588832332752645],
        [0.00021399073011707515, 0.00025598303182050586, 0.00018758360238280147, 2.897017475333996e-05, 1.3601790669781622e-05, -6.9405177782755345e-06, 0.00021918579295743257],
        [0.0003858681011479348, 0.0004338369471952319, 0.00034272336051799357, 2.7645364752970636e-05, 1.4279138667916413e-05, -6.515793302241946e-06, 0.00038747614598833025],
        [0.0003189097042195499, 0.00044660907587967813, 0.0003054551489185542, 1.685374263615813e-05, 2.7169366148882546e-05, -3.5254342947155237e-06, 0.00035699130967259407],
        [0.0024445944000035524, 0.002766623627394438, 0.0025375927798449993, -0.0003297310322523117, -0.00019607118156272918, -0.00012245409016031772, 0.0025829370133578777],
        [0.0010101665975525975, 0.0011832641903311014, 0.0011479828972369432, -6.372493953676894e-05, -5.044330100645311e-05, -1.656726817600429e-05, 0.0011138045229017735],
        [0.0010932435980066657, 0.0014160322025418282, 0.0010652935598045588, -0.00020294080604799092, -4.521312803262845e-05, -2.1810921680298634e-05, 0.0011915231589227915],
        [0.0004370198003016412, 0.000528408563695848, 0.00042431915062479675, -2.266904994030483e-05, -5.9162863180972636e-05, -1.3868942914996296e-05, 0.00046324916183948517],
        [0.00022726193128619343, 0.0002695871808100492, 0.00026248456561006606, -4.328153590904549e-05, -2.777852932922542e-05, -1.7338930774712935e-05, 0.0002531112404540181],
        [0.0021632560528814793, 0.0029078484512865543, 0.0022275543306022882, -0.00029718727455474436, -5.481487823999487e-05, -1.9195245840819553e-05, 0.002432886278256774],
    ];

    fn small() -> (Vec<Mat3>, Vec<f64>) {
        (SMALL.iter().map(|r| sym(r[0], r[1], r[2], r[3], r[4], r[5])).collect(), SMALL.iter().map(|r| r[6]).collect())
    }

    #[test]
    fn matches_the_original_for_twelve_specimens() {
        let (t, km) = small();
        let g = group_stats(&t, &km).unwrap();
        // mean tensor dec/inc printed by the original: 164.1/4.9, 256.5/26.1, 64.2/63.4
        let want = [(164.1, 4.9), (256.5, 26.1), (64.2, 63.4)];
        for a in 0..3 {
            assert!((g.dec_inc[a].0 - want[a].0).abs() < 0.06 && (g.dec_inc[a].1 - want[a].1).abs() < 0.06, "axis {a}: {:?}", g.dec_inc[a]);
        }
        // Jelinek semi-axes printed: K1 18.7/5.6, K2 73.4/9.9, K3 73.3/6.7
        let want = [[18.7, 5.6], [73.4, 9.9], [73.3, 6.7]];
        for a in 0..3 {
            let s = g.jelinek[a].unwrap().semi_deg();
            assert!((s[0] - want[a][0]).abs() < 0.06 && (s[1] - want[a][1]).abs() < 0.06, "axis {a}: {s:?}");
        }
        // mean-tensor shape: L 1.196 F 1.039 P 1.242 Pj 1.261 T -0.650 U -0.680 Q 1.449 E 0.869
        let s = g.shape.unwrap();
        for (got, want) in [(s.l, 1.196), (s.f, 1.039), (s.p, 1.242), (s.pj, 1.261), (s.t, -0.650), (s.u, -0.680), (s.q, 1.449), (s.e, 0.869)] {
            assert!((got - want).abs() < 6e-4, "{got} vs {want}");
        }
        // averages and standard deviations over the specimens: Km 1.188E-3 / 1.001E-3, P 1.422 / 0.142, T 0.117 / 0.449
        assert!((g.km.0 - 1.188e-3).abs() < 5e-7 && (g.km.1 - 1.001e-3).abs() < 5e-7);
        assert!((g.avg[2].0 - 1.422).abs() < 6e-4 && (g.avg[2].1 - 0.142).abs() < 6e-4);
        assert!((g.avg[4].0 - 0.117).abs() < 6e-4 && (g.avg[4].1 - 0.449).abs() < 6e-4);
    }

    #[test]
    fn identical_tensors_have_zero_scatter() {
        let t = sym(1.2, 1.0, 0.8, 0.0, 0.0, 0.0);
        let g = group_stats(&[t, t, t, t], &[1.0; 4]).unwrap();
        assert!((g.k[0] - 1.2).abs() < 1e-12);
        for e in g.jelinek.iter().flatten() {
            assert!(e.semi_deg()[0] < 1e-9);
        }
        assert!((g.avg[2].0 - 1.5).abs() < 1e-12 && g.avg[2].1.abs() < 1e-12);
    }

    #[test]
    fn small_groups_have_no_ellipses() {
        let t = sym(1.2, 1.0, 0.8, 0.0, 0.0, 0.0);
        let g = group_stats(&[t, t], &[1.0; 2]).unwrap();
        assert!(g.jelinek.iter().all(|e| e.is_none()) && g.bootstrap.is_none());
        assert!(group_stats(&[], &[]).is_none());
    }

    #[test]
    fn ellipse_outline_surrounds_the_axis() {
        let (t, km) = small();
        let g = group_stats(&t, &km).unwrap();
        let e = g.jelinek[0].unwrap();
        let o = e.outline(&g.vecs, 60);
        assert_eq!(o.len(), 61);
        // every point is within the larger semi-axis (plus rounding) of the mean axis
        let max = e.semi_deg()[0] + 1e-6;
        for p in &o {
            let ang = dot(*p, g.vecs[0]).clamp(-1.0, 1.0).acos().to_degrees();
            assert!(ang <= max + 1e-6, "{ang} > {max}");
        }
    }

    #[test]
    fn bootstrap_is_deterministic_and_centred() {
        let (t, km) = small();
        let a = group_stats(&t, &km).unwrap();
        let b = group_stats(&t, &km).unwrap();
        let (ba, bb) = (a.bootstrap.unwrap(), b.bootstrap.unwrap());
        assert_eq!(ba.points[0].len(), 500);
        assert_eq!(ba.points[0][17], bb.points[0][17]);
        // the cloud of K3 axes sits in the neighbourhood of the mean K3
        let m = ba.points[2].iter().filter(|p| dot(**p, a.vecs[2]) > 0.7).count();
        assert!(m > 400, "{m}");
    }
}
