//! A Gaussian mixture fitted to two positive quantities in log-log space, for the plots (full covariances, by EM).
//!
//! The pairs are taken as `(log10 x, log10 y)`; pairs with a value that is not above 0 (or not finite) are left out. The
//! fit is the best of several starts (k-means++ with a fixed seed, so the answer is the same every time) and the
//! clusters are numbered from the lowest mean `x` up.

#[derive(Debug, Clone, PartialEq)]
pub struct Gmm {
    pub weights: Vec<f64>,
    /// Mean `(log10 x, log10 y)` of each cluster.
    pub means: Vec<[f64; 2]>,
    /// Covariance `[[xx, xy], [xy, yy]]` of each cluster (log10 units squared).
    pub covs: Vec<[[f64; 2]; 2]>,
    pub loglik: f64,
    /// Number of pairs used.
    pub n: usize,
    /// Cluster of each pair used.
    pub labels: Vec<usize>,
    /// Which of the input pairs were used (indices into the input).
    pub used: Vec<usize>,
}

impl Gmm {
    pub fn k(&self) -> usize {
        self.weights.len()
    }

    /// Free parameters: means, covariances and weights.
    pub fn n_params(&self) -> usize {
        6 * self.k() - 1
    }

    pub fn aic(&self) -> f64 {
        -2.0 * self.loglik + 2.0 * self.n_params() as f64
    }

    pub fn bic(&self) -> f64 {
        -2.0 * self.loglik + self.n_params() as f64 * (self.n as f64).ln()
    }

    /// Points of the `ns`-sigma ellipse of cluster `j` (`(log10 x, log10 y)`), `steps` + 1 of them, closed.
    pub fn ellipse(&self, j: usize, ns: f64, steps: usize) -> Vec<[f64; 2]> {
        let [[a, b], [_, c]] = self.covs[j];
        let tr = a + c;
        let det = a * c - b * b;
        let disc = (tr * tr / 4.0 - det).max(0.0).sqrt();
        let (l1, l2) = ((tr / 2.0 + disc).max(0.0), (tr / 2.0 - disc).max(0.0));
        let th = 0.5 * (2.0 * b).atan2(a - c);
        let (st, ct) = th.sin_cos();
        (0..=steps)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / steps as f64;
                let (u, v) = (ns * l1.sqrt() * t.cos(), ns * l2.sqrt() * t.sin());
                [self.means[j][0] + u * ct - v * st, self.means[j][1] + u * st + v * ct]
            })
            .collect()
    }
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / (1u64 << 53) as f64
    }
}

const REG: f64 = 1e-6;

fn log_density(p: [f64; 2], mean: [f64; 2], cov: [[f64; 2]; 2]) -> f64 {
    let det = (cov[0][0] * cov[1][1] - cov[0][1] * cov[0][1]).max(1e-300);
    let (dx, dy) = (p[0] - mean[0], p[1] - mean[1]);
    let q = (cov[1][1] * dx * dx - 2.0 * cov[0][1] * dx * dy + cov[0][0] * dy * dy) / det;
    -0.5 * (q + det.ln() + 2.0 * std::f64::consts::TAU.ln())
}

fn covariance(pts: &[[f64; 2]], w: &[f64], mean: [f64; 2]) -> [[f64; 2]; 2] {
    let s: f64 = w.iter().sum::<f64>().max(1e-12);
    let (mut a, mut b, mut c) = (0.0, 0.0, 0.0);
    for (p, &wi) in pts.iter().zip(w) {
        let (dx, dy) = (p[0] - mean[0], p[1] - mean[1]);
        a += wi * dx * dx;
        b += wi * dx * dy;
        c += wi * dy * dy;
    }
    [[a / s + REG, b / s], [b / s, c / s + REG]]
}

/// One EM run from one start; the best of these is kept by the caller.
fn em(pts: &[[f64; 2]], k: usize, rng: &mut Lcg) -> (f64, Vec<f64>, Vec<[f64; 2]>, Vec<[[f64; 2]; 2]>) {
    let n = pts.len();
    // k-means++ start
    let mut means = vec![pts[(rng.next() * n as f64) as usize % n]];
    while means.len() < k {
        let d: Vec<f64> = pts.iter().map(|p| means.iter().map(|m| (p[0] - m[0]).powi(2) + (p[1] - m[1]).powi(2)).fold(f64::MAX, f64::min)).collect();
        let total: f64 = d.iter().sum();
        let mut r = rng.next() * total;
        let mut pick = n - 1;
        for (i, di) in d.iter().enumerate() {
            r -= di;
            if r <= 0.0 {
                pick = i;
                break;
            }
        }
        means.push(pts[pick]);
    }
    let all = vec![1.0; n];
    let base = covariance(pts, &all, [pts.iter().map(|p| p[0]).sum::<f64>() / n as f64, pts.iter().map(|p| p[1]).sum::<f64>() / n as f64]);
    let mut covs = vec![base; k];
    let mut weights = vec![1.0 / k as f64; k];
    let mut resp = vec![vec![0.0; k]; n];
    let mut prev = f64::NEG_INFINITY;
    let mut ll = prev;
    for _ in 0..400 {
        // E step
        ll = 0.0;
        for (p, r) in pts.iter().zip(resp.iter_mut()) {
            let lp: Vec<f64> = (0..k).map(|j| weights[j].max(1e-300).ln() + log_density(*p, means[j], covs[j])).collect();
            let m = lp.iter().cloned().fold(f64::MIN, f64::max);
            let s: f64 = lp.iter().map(|v| (v - m).exp()).sum();
            ll += m + s.ln();
            for j in 0..k {
                r[j] = (lp[j] - m).exp() / s;
            }
        }
        // M step
        for j in 0..k {
            let rj: Vec<f64> = resp.iter().map(|r| r[j]).collect();
            let nk = rj.iter().sum::<f64>().max(1e-9);
            weights[j] = nk / n as f64;
            means[j] = [pts.iter().zip(&rj).map(|(p, w)| p[0] * w).sum::<f64>() / nk, pts.iter().zip(&rj).map(|(p, w)| p[1] * w).sum::<f64>() / nk];
            covs[j] = covariance(pts, &rj, means[j]);
        }
        if (ll - prev).abs() < 1e-9 * ll.abs().max(1.0) {
            break;
        }
        prev = ll;
    }
    (ll, weights, means, covs)
}

/// Fits `k` clusters to the pairs with both values above 0. `None` with fewer than `10 * k` such pairs.
pub fn fit_log_log(x: &[f64], y: &[f64], k: usize) -> Option<Gmm> {
    let mut pts = vec![];
    let mut used = vec![];
    for (i, (&a, &b)) in x.iter().zip(y).enumerate() {
        if a > 0.0 && b > 0.0 && a.is_finite() && b.is_finite() {
            pts.push([a.log10(), b.log10()]);
            used.push(i);
        }
    }
    let n = pts.len();
    if k == 0 || n < 10 * k {
        return None;
    }
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
    let mut best: Option<(f64, Vec<f64>, Vec<[f64; 2]>, Vec<[[f64; 2]; 2]>)> = None;
    for _ in 0..if k == 1 { 1 } else { 30 } {
        let r = em(&pts, k, &mut rng);
        if r.0.is_finite() && best.as_ref().map_or(true, |b| r.0 > b.0) {
            best = Some(r);
        }
    }
    let (loglik, weights, means, covs) = best?;
    // numbered from the lowest mean x up
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by(|&a, &b| means[a][0].total_cmp(&means[b][0]));
    let (weights, means, covs): (Vec<_>, Vec<_>, Vec<_>) = (order.iter().map(|&j| weights[j]).collect(), order.iter().map(|&j| means[j]).collect(), order.iter().map(|&j| covs[j]).collect());
    let labels = pts
        .iter()
        .map(|p| (0..k).max_by(|&a, &b| (weights[a].ln() + log_density(*p, means[a], covs[a])).total_cmp(&(weights[b].ln() + log_density(*p, means[b], covs[b])))).unwrap_or(0))
        .collect();
    Some(Gmm { weights, means, covs, loglik, n, labels, used })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two blobs in log-log: 30 % around (1e-4, 1e4) and 70 % around (1e-2, 3e5), a little scatter.
    fn blobs() -> (Vec<f64>, Vec<f64>) {
        let mut g = Lcg(7);
        let (mut x, mut y) = (vec![], vec![]);
        for i in 0..500 {
            let (cx, cy, sx, sy) = if i % 10 < 3 { (-4.0, 4.0, 0.2, 0.3) } else { (-2.0, 5.5, 0.3, 0.05) };
            let n = |g: &mut Lcg| (0..12).map(|_| g.next()).sum::<f64>() - 6.0; // about N(0, 1)
            x.push(10f64.powf(cx + sx * n(&mut g)));
            y.push(10f64.powf(cy + sy * n(&mut g)));
        }
        (x, y)
    }

    #[test]
    fn two_blobs_are_found_with_their_weights_and_centres_lowest_x_first() {
        let (x, y) = blobs();
        let g = fit_log_log(&x, &y, 2).unwrap();
        assert_eq!(g.k(), 2);
        assert!((g.means[0][0] + 4.0).abs() < 0.1 && (g.means[0][1] - 4.0).abs() < 0.1, "{g:?}");
        assert!((g.means[1][0] + 2.0).abs() < 0.1 && (g.means[1][1] - 5.5).abs() < 0.05, "{g:?}");
        assert!((g.weights[0] - 0.3).abs() < 0.05 && (g.weights.iter().sum::<f64>() - 1.0).abs() < 1e-9, "{g:?}");
        assert_eq!(g.labels.len(), 500);
        assert!(g.labels.iter().filter(|&&l| l == 0).count() > 130);
        // two clusters describe it much better than one
        let one = fit_log_log(&x, &y, 1).unwrap();
        assert!(one.bic() - g.bic() > 100.0 && one.aic() > g.aic());
        // the same answer every time
        assert_eq!(fit_log_log(&x, &y, 2).unwrap(), g);
    }

    #[test]
    fn the_ellipse_is_centred_on_the_cluster_and_values_not_above_zero_are_left_out() {
        let (mut x, mut y) = blobs();
        let n = x.len();
        x.extend([0.0, -1.0, f64::NAN]);
        y.extend([1.0, 1.0, 1.0]);
        let g = fit_log_log(&x, &y, 2).unwrap();
        assert_eq!(g.n, n);
        assert!(g.used.iter().all(|&i| i < n));
        let e = g.ellipse(0, 2.0, 60);
        assert_eq!(e.len(), 61);
        let (mx, my) = (e[..60].iter().map(|p| p[0]).sum::<f64>() / 60.0, e[..60].iter().map(|p| p[1]).sum::<f64>() / 60.0);
        assert!((mx - g.means[0][0]).abs() < 1e-6 && (my - g.means[0][1]).abs() < 1e-6);
        assert!(fit_log_log(&x[..15], &y[..15], 2).is_none());
    }
}
