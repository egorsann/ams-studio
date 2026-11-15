//! The list of per-specimen quantities that can be plotted or used for colour-coding,
//! in the order of the original's "Data" drop-down lists.

use crate::ams::{Axes, Phase};

pub const PARAM_LABELS: [&str; 24] = [
    "Kmean", "L", "F", "P", "Pj", "T", "U", "Q", "E", "K1", "K2", "K3", "K1_Dec", "K1_Inc", "K2_Dec", "K2_Inc", "K3_Dec", "K3_Inc",
    "FTest", "FTest_F12", "FTest_F23", "E12", "E23", "E13",
];

pub const N_PARAMS: usize = PARAM_LABELS.len();

/// Background colour class in the original's lists: 0 none, 1 K1 (pink), 2 K2 (green), 3 K3 (lavender), 4 shape (cyan).
pub fn param_class(i: usize) -> u8 {
    match i {
        0 => 5,
        1..=8 => 4,
        9 | 12 | 13 => 1,
        10 | 14 | 15 => 2,
        11 | 16 | 17 => 3,
        _ => 0,
    }
}

/// Value of every parameter for one specimen and phase; `NaN` where undefined.
/// `E12`, `E23`, `E13` are approximate confidence angles in degrees:
/// `atan(rms(sd_i, sd_j) / (k_i - k_j))` from the stored standard deviations (the original uses
/// the full covariance of the tensor, which is not stored; this matches it to about 5 %).
pub fn param_values(km: f64, ax: &Axes, ph: &Phase) -> [f64; N_PARAMS] {
    let nan = f64::NAN;
    let mut v = [nan; N_PARAMS];
    v[0] = km;
    if let Some(s) = ax.shape {
        v[1..9].copy_from_slice(&[s.l, s.f, s.p, s.pj, s.t, s.u, s.q, s.e]);
    }
    for a in 0..3 {
        v[9 + a] = ax.k_norm[a];
        v[12 + 2 * a] = ax.dec_inc[a].0;
        v[13 + 2 * a] = ax.dec_inc[a].1;
    }
    v[18] = ph.ftest[0];
    v[19] = ph.ftest[1];
    v[20] = ph.ftest[2];
    let ang = |i: usize, j: usize| {
        let d = ax.k_norm[i] - ax.k_norm[j];
        if d.abs() < 1e-12 {
            return nan;
        }
        (((ph.sd[i].powi(2) + ph.sd[j].powi(2)) / 2.0).sqrt() / d.abs()).atan().to_degrees()
    };
    v[21] = ang(0, 1);
    v[22] = ang(1, 2);
    v[23] = ang(0, 2);
    v
}

/// Standard deviation of a parameter where the file stores one (K1..K3), else `NaN`.
pub fn param_sd(i: usize, ph: &Phase) -> f64 {
    match i {
        9..=11 => ph.sd[i - 9],
        _ => f64::NAN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tensor::sym;

    #[test]
    fn values_follow_the_label_order() {
        let ph = Phase { km: 2e-3, sd: [1e-3; 3], tensor: sym(1.2, 1.0, 0.8, 0.0, 0.0, 0.0), ftest: [11.0, 22.0, 33.0] };
        let ax = Axes::from_tensor(&ph.tensor);
        let v = param_values(ph.km, &ax, &ph);
        assert_eq!(PARAM_LABELS[0], "Kmean");
        assert_eq!(v[0], 2e-3);
        assert!((v[3] - 1.5).abs() < 1e-9, "P = {}", v[3]); // index of "P"
        assert_eq!(PARAM_LABELS[3], "P");
        assert!((v[9] - 1.2).abs() < 1e-9 && (v[11] - 0.8).abs() < 1e-9);
        assert_eq!((v[18], v[19], v[20]), (11.0, 22.0, 33.0));
        assert!(v[21] > 0.0 && v[21] < 1.0);
        assert!(param_sd(10, &ph) == 1e-3 && param_sd(3, &ph).is_nan());
    }

    #[test]
    fn undefined_shape_gives_nan() {
        let ph = Phase { km: 1.0, sd: [0.0; 3], tensor: sym(1.0, 1.0, 1.0, 0.0, 0.0, 0.0), ftest: [0.0; 3] };
        let ax = Axes::from_tensor(&ph.tensor);
        let v = param_values(1.0, &ax, &ph);
        assert!(v[3].is_nan() && v[21].is_nan());
    }
}
