//! A bent power law fitted to two positive quantities, for the plots: in log-log space a straight line of slope `s1`
//! that bends smoothly into a straight line of slope `s2` around `x0`, over a width `w` (all in decades):
//!
//! `log y = y0 + s1 (x - x0) + (s2 - s1) w [softplus((x - x0) / w) - ln 2]`,  `x = log x_in`, `y = log y_in`
//!
//! There is no ceiling parameter: a ceiling shows up only when the fit finds `s2` near 0 (the F-test against Kmean
//! rises as about Km^2 and then flattens), and a floor when it finds `s1` near 0 (the out-of-phase F-test).
//!
//! The model is linear in `(y0, s1, s2)` once the bend `(x0, w)` is fixed, so the fit is a search over the bend with a
//! robust (soft-L1, by iteratively reweighted least squares) linear fit inside, refined once around the best bend.
//! It needs no starting guess and gives the same answer every time.

const LN2: f64 = std::f64::consts::LN_2;
/// Scale of the soft-L1 loss, in decades: residuals much larger than this count less than their square.
const F_SCALE: f64 = 0.25;
/// Narrowest and widest bend, in decades.
const W_MIN: f64 = 0.03;
const W_MAX: f64 = 1.5;

#[derive(Debug, Clone, PartialEq)]
pub struct BentFit {
    pub y0: f64,
    pub x0: f64,
    pub s1: f64,
    pub s2: f64,
    pub w: f64,
    /// Number of points used, and the RMS of the bent curve over them (decades of y).
    pub n: usize,
    pub rms: f64,
    /// The straight power law `log y = slope * log x + intercept` through the same points, and its RMS.
    pub line: (f64, f64),
    pub line_rms: f64,
    /// AIC of the straight line minus AIC of the bent curve (6 against 3 parameters): above 10 the bend is clearly worth it.
    pub d_aic: f64,
}

impl BentFit {
    /// The bend is clearly better than a straight line.
    pub fn bent(&self) -> bool {
        self.d_aic >= 10.0
    }

    /// log10 y of the bent curve at log10 x.
    pub fn log_y_bent(&self, lx: f64) -> f64 {
        model(self.y0, self.x0, self.s1, self.s2, self.w, lx)
    }

    /// log10 y of what is worth drawing: the bent curve, or the straight line when the bend is not worth its parameters.
    pub fn log_y(&self, lx: f64) -> f64 {
        if self.bent() { self.log_y_bent(lx) } else { self.line.0 * lx + self.line.1 }
    }
}

fn softplus_shifted(z: f64) -> f64 {
    z.max(0.0) + (-z.abs()).exp().ln_1p() - LN2
}

fn model(y0: f64, x0: f64, s1: f64, s2: f64, w: f64, x: f64) -> f64 {
    y0 + s1 * (x - x0) + (s2 - s1) * w * softplus_shifted((x - x0) / w)
}

/// Solves the 3x3 system `a c = b` (Gaussian elimination with pivoting); `None` when it is singular.
fn solve3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for i in 0..3 {
        let p = (i..3).max_by(|&r, &s| a[r][i].abs().total_cmp(&a[s][i].abs()))?;
        if a[p][i].abs() < 1e-12 {
            return None;
        }
        a.swap(i, p);
        b.swap(i, p);
        for r in i + 1..3 {
            let f = a[r][i] / a[i][i];
            for c in i..3 {
                a[r][c] -= f * a[i][c];
            }
            b[r] -= f * b[i];
        }
    }
    let mut c = [0.0; 3];
    for i in (0..3).rev() {
        let s: f64 = (i + 1..3).map(|j| a[i][j] * c[j]).sum();
        c[i] = (b[i] - s) / a[i][i];
    }
    Some(c)
}

/// The robust linear fit of `(y0, s1, s2)` for a fixed bend: (soft-L1 cost, [y0, s1, s2]).
fn fit_at(x: &[f64], y: &[f64], x0: f64, w: f64) -> Option<(f64, [f64; 3])> {
    // basis of y0, s1, s2: 1, (x - x0) - w g, w g with g = softplus((x - x0) / w) - ln 2
    let basis: Vec<[f64; 3]> = x
        .iter()
        .map(|&xi| {
            let g = w * softplus_shifted((xi - x0) / w);
            [1.0, (xi - x0) - g, g]
        })
        .collect();
    let mut wt = vec![1.0; x.len()];
    let mut coef = [0.0; 3];
    for _ in 0..8 {
        let mut a = [[0.0; 3]; 3];
        let mut b = [0.0; 3];
        for ((bi, &yi), &wi) in basis.iter().zip(y).zip(&wt) {
            for r in 0..3 {
                b[r] += wi * bi[r] * yi;
                for c in 0..3 {
                    a[r][c] += wi * bi[r] * bi[c];
                }
            }
        }
        coef = solve3(a, b)?;
        for (k, (bi, &yi)) in basis.iter().zip(y).enumerate() {
            let r = bi[0] * coef[0] + bi[1] * coef[1] + bi[2] * coef[2] - yi;
            wt[k] = 1.0 / (1.0 + (r / F_SCALE).powi(2)).sqrt();
        }
    }
    let cost: f64 = basis
        .iter()
        .zip(y)
        .map(|(bi, &yi)| {
            let r = bi[0] * coef[0] + bi[1] * coef[1] + bi[2] * coef[2] - yi;
            F_SCALE * F_SCALE * 2.0 * ((1.0 + (r / F_SCALE).powi(2)).sqrt() - 1.0)
        })
        .sum();
    Some((cost, coef))
}

fn aic(rss: f64, n: usize, k: usize) -> f64 {
    n as f64 * (rss.max(1e-300) / n as f64).ln() + 2.0 * k as f64
}

/// Fits the bent power law to the pairs with both values positive and finite. `None` with fewer than 12 such pairs or
/// when their x values span less than a tenth of a decade.
pub fn bent_power_law(x_in: &[f64], y_in: &[f64]) -> Option<BentFit> {
    let (mut x, mut y) = (vec![], vec![]);
    for (&a, &b) in x_in.iter().zip(y_in) {
        if a > 0.0 && b > 0.0 && a.is_finite() && b.is_finite() {
            x.push(a.log10());
            y.push(b.log10());
        }
    }
    let n = x.len();
    if n < 12 {
        return None;
    }
    let (lo, hi) = x.iter().fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    if hi - lo < 0.1 {
        return None;
    }
    let mut best: Option<(f64, f64, f64, [f64; 3])> = None; // cost, x0, w, coefficients
    let try_bend = |x0: f64, w: f64, best: &mut Option<(f64, f64, f64, [f64; 3])>| {
        if let Some((cost, c)) = fit_at(&x, &y, x0, w) {
            if best.as_ref().map_or(true, |b| cost < b.0) {
                *best = Some((cost, x0, w, c));
            }
        }
    };
    // coarse search: the bend anywhere in the range, six widths
    const STEPS: usize = 60;
    let step = (hi - lo) / STEPS as f64;
    for i in 0..=STEPS {
        for w in [0.03, 0.06, 0.12, 0.25, 0.5, 1.0, 1.5] {
            try_bend(lo + i as f64 * step, w, &mut best);
        }
    }
    // refine around the best bend
    let (_, bx, bw, _) = best.clone()?;
    for i in -8..=8 {
        for f in [0.7, 0.85, 1.0, 1.2, 1.4] {
            try_bend((bx + i as f64 * step / 8.0).clamp(lo, hi), (bw * f).clamp(W_MIN, W_MAX), &mut best);
        }
    }
    let (_, x0, w, c) = best?;
    let rss: f64 = x.iter().zip(&y).map(|(&xi, &yi)| (model(c[0], x0, c[1], c[2], w, xi) - yi).powi(2)).sum();
    // the straight power law, by ordinary least squares
    let (mx, my) = (x.iter().sum::<f64>() / n as f64, y.iter().sum::<f64>() / n as f64);
    let sxx: f64 = x.iter().map(|v| (v - mx).powi(2)).sum();
    let sxy: f64 = x.iter().zip(&y).map(|(a, b)| (a - mx) * (b - my)).sum();
    let slope = sxy / sxx;
    let icpt = my - slope * mx;
    let rss_l: f64 = x.iter().zip(&y).map(|(&xi, &yi)| (slope * xi + icpt - yi).powi(2)).sum();
    Some(BentFit {
        y0: c[0],
        x0,
        s1: c[1],
        s2: c[2],
        w,
        n,
        rms: (rss / n as f64).sqrt(),
        line: (slope, icpt),
        line_rms: (rss_l / n as f64).sqrt(),
        d_aic: aic(rss_l, n, 3) - aic(rss, n, 6),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(s: &mut u64) -> f64 {
        *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*s >> 11) as f64) / (1u64 << 53) as f64
    }

    /// Points along a bent curve, x from 1e-5 to 1e-1, with scatter of about +-0.05 decades (plus one wild point).
    fn bent_data(y0: f64, x0: f64, s1: f64, s2: f64, w: f64) -> (Vec<f64>, Vec<f64>) {
        let mut seed = 42u64;
        let (mut xs, mut ys) = (vec![], vec![]);
        for _ in 0..400 {
            let lx = -5.0 + 4.0 * lcg(&mut seed);
            let noise = (lcg(&mut seed) - 0.5) * 0.1;
            xs.push(10f64.powf(lx));
            ys.push(10f64.powf(model(y0, x0, s1, s2, w, lx) + noise));
        }
        xs.push(1e-3);
        ys.push(1e-3); // far from everything
        (xs, ys)
    }

    #[test]
    fn a_rise_that_flattens_is_recovered_with_its_bend_and_both_slopes() {
        // F against Kmean: about Km^2.5 up to 3e-4, then almost flat
        let (x, y) = bent_data(5.2, -3.5, 2.5, 0.05, 0.1);
        let f = bent_power_law(&x, &y).unwrap();
        assert!((f.s1 - 2.5).abs() < 0.15, "{f:?}");
        assert!((f.s2 - 0.05).abs() < 0.1, "{f:?}");
        assert!((f.x0 + 3.5).abs() < 0.2, "{f:?}");
        // the one wild point is down-weighted by the fit but still counts in the plain RMS
        assert!(f.bent() && f.rms < 0.5 && f.d_aic > 100.0, "{f:?}");
        assert!((f.log_y(f.x0) - f.y0).abs() < 1e-12, "y0 is the value at the bend");
    }

    #[test]
    fn a_floor_that_rises_is_recovered_too() {
        // the out-of-phase shape: flat at low Km, then Km^2
        let (x, y) = bent_data(1.0, -3.0, 0.05, 2.0, 0.1);
        let f = bent_power_law(&x, &y).unwrap();
        assert!(f.s1.abs() < 0.15 && (f.s2 - 2.0).abs() < 0.15, "{f:?}");
        assert!(f.bent());
    }

    #[test]
    fn a_straight_power_law_is_not_given_a_bend() {
        let (x, y) = bent_data(2.0, -3.0, 2.0, 2.0, 0.1);
        let f = bent_power_law(&x, &y).unwrap();
        assert!(!f.bent(), "{f:?}");
        assert!((f.line.0 - 2.0).abs() < 0.05, "the straight line is the one drawn: {f:?}");
        assert!((f.log_y(-3.0) - (f.line.0 * -3.0 + f.line.1)).abs() < 1e-12);
    }

    #[test]
    fn too_little_data_gives_no_fit_and_bad_values_are_skipped() {
        assert!(bent_power_law(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]).is_none());
        // all at one x
        let x = vec![1e-3; 30];
        let y: Vec<f64> = (1..=30).map(|v| v as f64).collect();
        assert!(bent_power_law(&x, &y).is_none());
        // zeros, negatives and NaN are dropped, the rest still fits
        let (mut x, mut y) = bent_data(5.2, -3.5, 2.5, 0.05, 0.1);
        let n = x.len();
        x.extend([0.0, -1.0, f64::NAN]);
        y.extend([1.0, 1.0, 1.0]);
        y.extend([0.0, -2.0, f64::INFINITY]);
        x.extend([1e-3, 1e-3, 1e-3]);
        let f = bent_power_law(&x, &y).unwrap();
        assert_eq!(f.n, n);
    }
}
