//! The quantities of the M3ORE lab sheet (`Template_PrinSus_v6`) that Anisoft does not have, with the error
//! propagation of that sheet.
//!
//! Inputs per specimen and phase: the normalised eigenvalues `kn` (mean 1), their absolute errors `sd` (the
//! standard deviations stored in the `.ams` file), the mean susceptibility `km`, the specimen volume in cm3 and an
//! optional percent error of `km` (the sheet's "Optional %_Err", 0 when unknown).
//!
//! The sheet's formulas, as written there (column letters of its `In_Phase` sheet in brackets):
//! * volume-corrected mean susceptibility `X = 10 / vol * km`, unnormalised `K_i = kn_i * X` (AB, AE, AH)
//! * fractional errors of the normalised values `sd_i / kn_i` (N, Q, T) and of `X` (`pct / 100`, Z);
//!   of the unnormalised values `Z + N` (AD, AG, AJ)
//! * L (Balsley & Buddington) `K1/K2`, F (Stacey) `K2/K3`, P `K1/K3`, with the fractional errors added
//! * L (Khan) `(K1-K2)/X`, F (Khan) `(K2-K3)/X`, H (Owens) `(K1-K3)/X`, with
//!   `frac = (abs_i + abs_j) / (K_i - K_j) + Z`
//! * Pj `exp(sqrt(2 * sum (ln K_i - mean ln K)^2))`, T and U, with the error terms of the sheet (CB-CN)
//!
//! Absolute errors are `frac * value`. The sheet can produce negative error values for some inputs; here every
//! error is reported as a magnitude.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lab {
    /// Volume-corrected mean susceptibility.
    pub km_vol: f64,
    /// Unnormalised K1..K3 and their fractional errors.
    pub k_un: [f64; 3],
    pub k_un_frac: [f64; 3],
    /// Fractional errors of the normalised K1..K3 and of the mean susceptibility.
    pub k_frac: [f64; 3],
    pub km_frac: f64,
    pub l: Val,
    pub l_khan: Val,
    pub f: Val,
    pub f_khan: Val,
    pub p: Val,
    pub h_owens: Val,
    pub pj: Val,
    pub t: Val,
    pub u: Val,
}

/// A value with its fractional and absolute error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Val {
    pub v: f64,
    pub frac: f64,
    pub abs: f64,
}

const NAN: f64 = f64::NAN;

fn val(v: f64, frac: f64) -> Val {
    let frac = frac.abs();
    Val { v, frac, abs: frac * v.abs() }
}

fn nanval() -> Val {
    Val { v: NAN, frac: NAN, abs: NAN }
}

/// `None` when the volume is not usable.
pub fn lab(km: f64, kn: [f64; 3], sd: [f64; 3], vol_cm3: f64, km_pct_err: f64) -> Lab {
    let x = if vol_cm3 > 0.0 { 10.0 / vol_cm3 * km } else { NAN };
    let z = km_pct_err / 100.0;
    let k = [kn[0] * x, kn[1] * x, kn[2] * x];
    let frac_n = [0, 1, 2].map(|i| sd[i] / kn[i]);
    let ku_frac = [z + frac_n[0], z + frac_n[1], z + frac_n[2]];
    let ku_abs = [0, 1, 2].map(|i| ku_frac[i] * k[i]);
    // pairs of the three: (value, frac) through the sheet's rules
    let ratio = |a: usize, b: usize| val(k[a] / k[b], ku_frac[a] + ku_frac[b]);
    let diff = |a: usize, b: usize| val((k[a] - k[b]) / x, (ku_abs[a] + ku_abs[b]) / (k[a] - k[b]) + z);
    // Pj, T: from the natural logs of the unnormalised values (the absolute error of ln K is the fractional error of K)
    let n = [k[0].ln(), k[1].ln(), k[2].ln()];
    let nm = (n[0] + n[1] + n[2]) / 3.0;
    let nab = [ku_frac[0], ku_frac[1], ku_frac[2]];
    let nm_ab = nab[0] + nab[1] + nab[2];
    let d = [n[0] - nm, n[1] - nm, n[2] - nm];
    let d_ab = [nab[0] + nm_ab, nab[1] + nm_ab, nab[2] + nm_ab];
    let sq_frac: Vec<f64> = (0..3).map(|i| 2.0 * (d_ab[i] / d[i])).collect();
    let sq_ab: Vec<f64> = (0..3).map(|i| d[i] * d[i] * sq_frac[i]).collect();
    let ca = 2.0 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    let cb = 2.0 * (sq_ab[0] + sq_ab[1] + sq_ab[2]);
    let pj_v = ca.sqrt().exp();
    let pj_frac = cb / (2.0 * ca.sqrt());
    let pj = Val { v: pj_v, frac: pj_frac.abs(), abs: (pj_v * pj_frac).abs() };
    let t_v = (2.0 * n[1] - n[0] - n[2]) / (n[0] - n[2]);
    let t_frac = (2.0 * nab[1] + nab[0] + nab[2]) / (2.0 * n[1] - n[0] - n[2]) + (nab[0] + nab[2]) / (n[0] - n[2]);
    let u_v = (2.0 * k[1] - k[0] - k[2]) / (k[0] - k[2]);
    let u_frac = (2.0 * ku_abs[1] + ku_abs[0] + ku_abs[2]) / (2.0 * k[1] - k[0] - k[2]) + (ku_abs[0] + ku_abs[2]) / (k[0] - k[2]);
    let bad = !(x.is_finite() && k.iter().all(|v| *v > 0.0));
    let pick = |v: Val| if bad { nanval() } else { v };
    Lab {
        km_vol: x,
        k_un: k,
        k_un_frac: ku_frac,
        k_frac: frac_n,
        km_frac: z,
        l: pick(ratio(0, 1)),
        l_khan: pick(diff(0, 1)),
        f: pick(ratio(1, 2)),
        f_khan: pick(diff(1, 2)),
        p: pick(ratio(0, 2)),
        h_owens: pick(diff(0, 2)),
        pj: pick(pj),
        t: pick(val(t_v, t_frac)),
        u: pick(val(u_v, u_frac)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Row 2 of the sheet (a made-up specimen with no errors) computed by the sheet itself:
    /// L 1.00698603, L Khan 0.007, F 1.01314459, F Khan 0.013, P 1.02022245, H 0.02, Pj 1.02053583, T 0.30454553, U 0.3.
    #[test]
    fn matches_the_sheet_for_a_row_without_errors() {
        let l = lab(4.626e-3, [1.009, 1.002, 0.989], [0.0; 3], 10.0, 0.0);
        assert!((l.km_vol - 4.626e-3).abs() < 1e-12);
        assert!((l.k_un[0] - 4.667634e-3).abs() < 1e-9);
        assert!((l.l.v - 1.006986028).abs() < 1e-8 && (l.f.v - 1.01314459).abs() < 1e-8 && (l.p.v - 1.02022245).abs() < 1e-8);
        assert!((l.l_khan.v - 0.007).abs() < 1e-9 && (l.f_khan.v - 0.013).abs() < 1e-9 && (l.h_owens.v - 0.02).abs() < 1e-9);
        assert!((l.pj.v - 1.02053583).abs() < 1e-7 && (l.t.v - 0.304545533).abs() < 1e-7 && (l.u.v - 0.3).abs() < 1e-9);
        assert_eq!(l.p.abs, 0.0);
    }

    #[test]
    fn volume_scales_the_mean_and_the_unnormalised_values() {
        let a = lab(1e-3, [1.2, 1.0, 0.8], [0.0; 3], 10.0, 0.0);
        let b = lab(1e-3, [1.2, 1.0, 0.8], [0.0; 3], 3.375, 0.0);
        assert!((b.km_vol / a.km_vol - 10.0 / 3.375).abs() < 1e-9);
        assert!((b.k_un[0] / a.k_un[0] - 10.0 / 3.375).abs() < 1e-9);
        // the shape parameters do not depend on the volume
        assert!((a.p.v - b.p.v).abs() < 1e-12 && (a.l_khan.v - b.l_khan.v).abs() < 1e-12);
    }

    #[test]
    fn fractional_errors_add_up_as_in_the_sheet() {
        let l = lab(1e-3, [1.2, 1.0, 0.8], [0.012, 0.01, 0.008], 10.0, 0.0);
        // each K has 1 % error, so L = K1/K2 has 2 %, P = K1/K3 2 %
        assert!((l.l.frac - 0.02).abs() < 1e-12 && (l.p.frac - 0.02).abs() < 1e-12);
        assert!((l.l.abs - 0.02 * l.l.v).abs() < 1e-12);
        // Khan's L: (abs1 + abs2) / (K1 - K2), here (0.012 + 0.01) km / (0.2 km)
        assert!((l.l_khan.frac - 0.022 / 0.2).abs() < 1e-9);
        assert!(l.pj.abs > 0.0 && l.t.abs > 0.0 && l.u.abs > 0.0);
        // a percent error of the mean susceptibility adds to every fractional error
        let m = lab(1e-3, [1.2, 1.0, 0.8], [0.012, 0.01, 0.008], 10.0, 5.0);
        assert!((m.km_frac - 0.05).abs() < 1e-12 && (m.k_un_frac[0] - 0.06).abs() < 1e-12);
    }

    #[test]
    fn unusable_input_gives_nan() {
        assert!(lab(1e-3, [1.2, 1.0, 0.8], [0.0; 3], 0.0, 0.0).p.v.is_nan());
        assert!(lab(1e-3, [1.2, 1.0, -0.2], [0.0; 3], 10.0, 0.0).pj.v.is_nan());
    }
}
