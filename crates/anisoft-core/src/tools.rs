//! The Tools menu: tensor addition / subtraction, fabric overprint, intersection
//! toolbox and Deform AMS. Every rule here was calibrated against the original program
//! (see `reference/tools-spec.md` in the repository checkout).

use crate::ams::{build_record, parse_phase_pub, put_phase, Axes, Phase, OFF_KIM, OFF_KRE, RECORD_LEN};
use crate::tensor::{cross, dec_inc, dot, eig_sym, from_dec_inc, mat_mul, norm, Mat3, Vec3};

// ------------------------------------------------------------------ add / subtract

/// Combine one phase of two specimens. `sign` is +1 for addition, -1 for subtraction.
/// Km adds (signed); the tensors are weighted by |Km| and divided by |Km_c|.
pub fn combine_phase(a: &Phase, b: &Phase, sign: f64) -> (f64, Mat3) {
    let kc = a.km + sign * b.km;
    let mut t = [[0.0; 3]; 3];
    if kc != 0.0 {
        for i in 0..3 {
            for j in 0..3 {
                t[i][j] = (a.km.abs() * a.tensor[i][j] + sign * b.km.abs() * b.tensor[i][j]) / kc.abs();
            }
        }
    }
    (kc, t)
}

/// Combined 640-byte record: A's record with both phases' Km and tensor replaced.
pub fn combine_records(ra: &[u8], rb: &[u8], sign: f64) -> Vec<u8> {
    assert!(ra.len() == RECORD_LEN && rb.len() == RECORD_LEN);
    let mut out = ra.to_vec();
    for base in [OFF_KRE, OFF_KIM] {
        let (pa, pb) = (parse_phase_pub(ra, base), parse_phase_pub(rb, base));
        let (km, t) = combine_phase(&pa, &pb, sign);
        put_phase(&mut out, base, km, &t);
    }
    out
}

/// Pair indices of A and B with identical names, in A order; each B record is used once.
pub fn pair_by_name(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let mut used = vec![false; b.len()];
    let mut out = vec![];
    for (i, n) in a.iter().enumerate() {
        if let Some(j) = (0..b.len()).find(|&j| !used[j] && b[j] == *n) {
            used[j] = true;
            out.push((i, j));
        }
    }
    out
}

// ------------------------------------------------------------------ fabric overprint

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FabricIn {
    pub kmean: f64,
    pub p: f64,
    pub t: f64,
    pub kmax: (f64, f64),
    pub kmin: (f64, f64),
}

impl Default for FabricIn {
    fn default() -> Self {
        FabricIn { kmean: 1.0, p: 1.2, t: 0.0, kmax: (0.0, 0.0), kmin: (0.0, 90.0) }
    }
}

/// Normalised (mean 1) eigenvalues from P = K1/K3 and T.
pub fn eigenvalues_from_pt(p: f64, t: f64) -> [f64; 3] {
    let lnp = p.max(1.0).ln();
    let k = [(lnp / 2.0).exp(), (t * lnp / 2.0).exp(), (-lnp / 2.0).exp()];
    let s = k[0] + k[1] + k[2];
    [3.0 * k[0] / s, 3.0 * k[1] / s, 3.0 * k[2] / s]
}

pub fn fabric_tensor(f: &FabricIn) -> Mat3 {
    let k = eigenvalues_from_pt(f.p, f.t);
    let e1 = from_dec_inc(f.kmax.0, f.kmax.1);
    let e3 = from_dec_inc(f.kmin.0, f.kmin.1);
    let mut e2 = cross(e3, e1);
    let n = norm(e2);
    if n > 1e-12 {
        e2 = [e2[0] / n, e2[1] / n, e2[2] / n];
    }
    let mut t = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            t[i][j] = f.kmean * (k[0] * e1[i] * e1[j] + k[1] * e2[i] * e2[j] + k[2] * e3[i] * e3[j]);
        }
    }
    t
}

/// `n` tensors linearly mixed from A (f = 0) to B (f = 1).
pub fn overprint_series(a: &FabricIn, b: &FabricIn, n: usize) -> Vec<Mat3> {
    let (ta, tb) = (fabric_tensor(a), fabric_tensor(b));
    (0..n)
        .map(|i| {
            let f = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
            let mut c = [[0.0; 3]; 3];
            for r in 0..3 {
                for s in 0..3 {
                    c[r][s] = (1.0 - f) * ta[r][s] + f * tb[r][s];
                }
            }
            c
        })
        .collect()
}

/// Record for a simulated specimen (instrument text "SimulDat"); Km = trace / 3, stored tensor normalised.
pub fn simulated_record(name: &str, t: &Mat3) -> Vec<u8> {
    let km = (t[0][0] + t[1][1] + t[2][2]) / 3.0;
    let mut tn = *t;
    if km != 0.0 {
        for r in tn.iter_mut() {
            for v in r.iter_mut() {
                *v /= km;
            }
        }
    }
    let ph = Phase { km, sd: [0.0; 3], tensor: tn, ftest: [0.0; 3] };
    let zero = Phase { km: 0.0, sd: [0.0; 3], tensor: [[0.0; 3]; 3], ftest: [0.0; 3] };
    let mut r = build_record(name, &ph, &zero, 0.0, 0.0);
    r[44..60].fill(b' ');
    r[44..52].copy_from_slice(b"SimulDat");
    r
}

/// Kmean, P, T and the Kmax / Kmin directions of a tensor with the given Km (used by the Load buttons).
pub fn fabric_from_tensor(km: f64, t: &Mat3) -> Option<FabricIn> {
    let ax = Axes::from_tensor(t);
    let s = ax.shape?;
    Some(FabricIn { kmean: km, p: s.p, t: s.t, kmax: ax.dec_inc[0], kmin: ax.dec_inc[2] })
}

// ------------------------------------------------------------------ intersection toolbox

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intersection {
    /// Two lineations -> the plane through both: (dip direction, dip) and the angle between them.
    Plane { dip_dir: f64, dip: f64, angle: f64 },
    /// Two foliations -> their intersection lineation: (trend, plunge) and the angle between the planes.
    Line { trend: f64, plunge: f64, angle: f64 },
}

fn pole_of_plane(dip_dir: f64, dip: f64) -> Vec3 {
    from_dec_inc((dip_dir + 180.0).rem_euclid(360.0), 90.0 - dip)
}

fn unit(v: Vec3) -> Vec3 {
    let n = norm(v);
    [v[0] / n, v[1] / n, v[2] / n]
}

/// `a`, `b` are (trend, plunge) for lineations or (dip direction, dip) for foliations.
pub fn intersect_lineations(a: (f64, f64), b: (f64, f64)) -> Option<Intersection> {
    let (va, vb) = (from_dec_inc(a.0, a.1), from_dec_inc(b.0, b.1));
    let n = cross(va, vb);
    if norm(n) < 1e-9 {
        return None;
    }
    let (dec, inc) = dec_inc(unit(n));
    Some(Intersection::Plane {
        dip_dir: (dec + 180.0).rem_euclid(360.0),
        dip: 90.0 - inc,
        angle: dot(va, vb).clamp(-1.0, 1.0).acos().to_degrees(),
    })
}

pub fn intersect_foliations(a: (f64, f64), b: (f64, f64)) -> Option<Intersection> {
    let (pa, pb) = (pole_of_plane(a.0, a.1), pole_of_plane(b.0, b.1));
    let l = cross(pa, pb);
    if norm(l) < 1e-9 {
        return None;
    }
    let (trend, plunge) = dec_inc(unit(l));
    Some(Intersection::Line { trend, plunge, angle: dot(pa, pb).clamp(-1.0, 1.0).acos().to_degrees() })
}

/// Points of the great circle of a plane (dip direction, dip) on the lower hemisphere, as (dec, inc).
pub fn great_circle(dip_dir: f64, dip: f64, n: usize) -> Vec<(f64, f64)> {
    let strike = (dip_dir - 90.0).rem_euclid(360.0);
    let u = from_dec_inc(strike, 0.0); // horizontal, along strike
    let dh = from_dec_inc(dip_dir, 0.0); // horizontal, toward the dip direction
    let (cd, sd) = (dip.to_radians().cos(), dip.to_radians().sin());
    (0..=n)
        .map(|i| {
            let a = std::f64::consts::PI * i as f64 / n as f64;
            let (sa, ca) = a.sin_cos();
            // v = cos(a) * strike + sin(a) * down-dip: runs along the lower half of the plane
            dec_inc(unit([ca * u[0] + sa * cd * dh[0], ca * u[1] + sa * cd * dh[1], sa * sd]))
        })
        .collect()
}

// ------------------------------------------------------------------ deform AMS

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrainKind {
    UniaxialShortening,
    UniaxialExtension,
    AxiallySymmetricShortening,
    AxiallySymmetricExtension,
    PureShear,
    SimpleShear,
    Transpression,
}

impl StrainKind {
    pub const ALL: [StrainKind; 7] = [
        StrainKind::UniaxialShortening,
        StrainKind::UniaxialExtension,
        StrainKind::AxiallySymmetricShortening,
        StrainKind::AxiallySymmetricExtension,
        StrainKind::PureShear,
        StrainKind::SimpleShear,
        StrainKind::Transpression,
    ];
    pub fn label(self) -> &'static str {
        match self {
            StrainKind::UniaxialShortening => "Uniaxial Shortening (Volume Loss)",
            StrainKind::UniaxialExtension => "Uniaxial Extension (Volume Increase)",
            StrainKind::AxiallySymmetricShortening => "Axially Symmetric Shortening",
            StrainKind::AxiallySymmetricExtension => "Axially Symmetric Extension",
            StrainKind::PureShear => "Pure Shear",
            StrainKind::SimpleShear => "Simple Shear",
            StrainKind::Transpression => "Transpression (Shear plane YZ / Shear direction Y)",
        }
    }
    /// Labels of the second combo box (empty: no combo).
    pub fn axes(self) -> &'static [&'static str] {
        match self {
            StrainKind::PureShear => &[
                "Shorten X / Extend Z", "Shorten X / Extend Y", "Shorten Y / Extend X",
                "Shorten Y / Extend Z", "Shorten Z / Extend X", "Shorten Z / Extend Y",
            ],
            StrainKind::SimpleShear => &[
                "Strike-slip (Shear plane XZ / Shear direction X)",
                "Strike-slip (Shear plane YZ / Shear direction Y)",
                "Thrust in X (Shear plane XY / Shear direction X)",
                "Thrust in Y (Shear plane XY / Shear direction Y)",
            ],
            StrainKind::Transpression => &[],
            _ => &["Along X", "Along Y", "Along Z"],
        }
    }
}

pub fn strain_matrix(kind: StrainKind, axis: usize, s: f64, g: f64) -> Mat3 {
    let mut m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let axis = axis.min(2);
    match kind {
        StrainKind::UniaxialShortening => m[axis][axis] = 1.0 / s,
        StrainKind::UniaxialExtension => m[axis][axis] = s,
        StrainKind::AxiallySymmetricShortening => {
            for i in 0..3 {
                m[i][i] = if i == axis { s.powf(-2.0 / 3.0) } else { s.powf(1.0 / 3.0) };
            }
        }
        StrainKind::AxiallySymmetricExtension => {
            for i in 0..3 {
                m[i][i] = if i == axis { s.powf(2.0 / 3.0) } else { s.powf(-1.0 / 3.0) };
            }
        }
        StrainKind::PureShear => {
            // (shortened, extended) axes for the six menu entries
            let (sh, ex) = [(0, 2), (0, 1), (1, 0), (1, 2), (2, 0), (2, 1)][axis.min(5)];
            let _ = axis;
            m[sh][sh] = 1.0 / s.sqrt();
            m[ex][ex] = s.sqrt();
        }
        StrainKind::SimpleShear => match axis.min(3) {
            0 => m[0][1] = g,
            1 => m[1][0] = g,
            2 => m[0][2] = g,
            _ => m[1][2] = g,
        },
        StrainKind::Transpression => {
            let a = 1.0 / s.sqrt();
            let e = s.sqrt().ln();
            m[0][0] = a;
            m[2][2] = 1.0 / a;
            m[1][0] = if e.abs() < 1e-12 { g } else { g * (s.sqrt() - 1.0) / e };
        }
    }
    m
}

/// Equal-area lattice of grain axes: 20 x 20 cell-centred square grid inside the unit circle.
pub fn grain_lattice() -> Vec<Vec3> {
    let n = 20;
    let h = 2.0 / n as f64;
    let mut v = vec![];
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (-1.0 + (i as f64 + 0.5) * h, -1.0 + (j as f64 + 0.5) * h);
            let r2 = x * x + y * y;
            if r2 <= 1.0 {
                let hh = (2.0 - r2).max(0.0).sqrt();
                // specimen (X, Y, Z); core vectors are (Y, X, Z), see `from_dec_inc`
                v.push(unit([x * hh, y * hh, 1.0 - r2]));
            }
        }
    }
    v
}

fn inverse(m: &Mat3) -> Mat3 {
    let d = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let c = |r1: usize, c1: usize, r2: usize, c2: usize| m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1];
    let id = 1.0 / d;
    [
        [c(1, 1, 2, 2) * id, -c(0, 1, 2, 2) * id, c(0, 1, 1, 2) * id],
        [-c(1, 0, 2, 2) * id, c(0, 0, 2, 2) * id, -c(0, 0, 1, 2) * id],
        [c(1, 0, 2, 1) * id, -c(0, 0, 2, 1) * id, c(0, 0, 1, 1) * id],
    ]
}

fn mat_vec3(m: &Mat3, v: Vec3) -> Vec3 {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// State of the Deform AMS toolbox.
#[derive(Debug, Clone)]
pub struct Deform {
    pub oblate: bool,
    pub p_grain: f64,
    /// Accumulated deformation of all steps so far.
    pub total: Mat3,
    base: Vec<Vec3>,
}

impl Deform {
    pub fn new(oblate: bool, p_grain: f64) -> Deform {
        Deform { oblate, p_grain: p_grain.max(1.0), total: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], base: grain_lattice() }
    }
    pub fn reset(&mut self) {
        self.total = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    pub fn add_step(&mut self, step: &Mat3) {
        self.total = mat_mul(step, &self.total);
    }
    /// Current orientations of the grain axes (specimen X, Y, Z), unit vectors.
    pub fn axes(&self) -> Vec<Vec3> {
        let fit = {
            let i = inverse(&self.total);
            [[i[0][0], i[1][0], i[2][0]], [i[0][1], i[1][1], i[2][1]], [i[0][2], i[1][2], i[2][2]]]
        };
        self.base.iter().map(|p| unit(if self.oblate { mat_vec3(&fit, *p) } else { mat_vec3(&self.total, *p) })).collect()
    }
    /// Resultant fabric tensor (mean eigenvalue 1) in specimen coordinates (X, Y, Z).
    pub fn fabric_tensor(&self) -> Mat3 {
        let (axis_k, other_k) = if self.oblate {
            let a = 3.0 / (2.0 + 1.0 / self.p_grain);
            (a / self.p_grain, a)
        } else {
            let c = 3.0 / (2.0 + self.p_grain);
            (c * self.p_grain, c)
        };
        let q = self.axes();
        let n = q.len() as f64;
        let mut t = [[0.0; 3]; 3];
        for v in &q {
            for i in 0..3 {
                for j in 0..3 {
                    t[i][j] += (axis_k - other_k) * v[i] * v[j] / n;
                }
            }
        }
        for (i, row) in t.iter_mut().enumerate() {
            row[i] += other_k;
        }
        t
    }
    /// Axes in the core library's vector order (Y, X, Z), so that `Axes::from_tensor` gives the
    /// same dec/inc the original prints.
    pub fn fabric_axes(&self) -> Axes {
        let t = self.fabric_tensor();
        let p = [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        Axes::from_tensor(&mat_mul(&mat_mul(&p, &t), &p))
    }
}

pub fn eigenvalues(t: &Mat3) -> [f64; 3] {
    eig_sym(t).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ams::parse;
    use crate::tensor::sym;

    fn ph(km: f64, t: Mat3) -> Phase {
        Phase { km, sd: [1e-3; 3], tensor: t, ftest: [5.0, 1.0, 1.0] }
    }

    #[test]
    fn add_and_subtract_follow_the_km_weighting() {
        let a = ph(2e-3, sym(1.2, 1.0, 0.8, 0.0, 0.0, 0.0));
        let b = ph(1e-3, sym(0.9, 1.0, 1.1, 0.1, 0.0, 0.0));
        let (k, t) = combine_phase(&a, &b, 1.0);
        assert!((k - 3e-3).abs() < 1e-12);
        assert!((t[0][0] - (2.0 * 1.2 + 0.9) / 3.0).abs() < 1e-12);
        assert!((t[0][1] - 0.1 / 3.0).abs() < 1e-12);
        let (k, t) = combine_phase(&a, &b, -1.0);
        assert!((k - 1e-3).abs() < 1e-12);
        assert!((t[0][0] - (2.0 * 1.2 - 0.9)).abs() < 1e-12);
        // a - a: zero Km, zero tensor
        let (k, t) = combine_phase(&a, &a, -1.0);
        assert_eq!(k, 0.0);
        assert!(t.iter().flatten().all(|v| *v == 0.0));
    }

    #[test]
    fn negative_km_uses_absolute_weights() {
        let a = ph(-4e-6, sym(1.0, 1.0, 1.0, 0.0, 0.0, 0.0));
        let b = ph(1.5e-6, sym(2.0, 2.0, 2.0, 0.0, 0.0, 0.0));
        let (k, t) = combine_phase(&a, &b, 1.0);
        assert!((k - -2.5e-6).abs() < 1e-18);
        // (|-4|*1 + 1.5*2) / |-2.5| = 2.8
        assert!((t[0][0] - 2.8).abs() < 1e-9);
    }

    #[test]
    fn combine_records_keeps_a_header_and_parses_back() {
        let a = ph(2e-3, sym(1.2, 1.0, 0.8, 0.0, 0.0, 0.0));
        let b = ph(1e-3, sym(0.9, 1.0, 1.1, 0.1, 0.0, 0.0));
        let ra = build_record("ZZ01-A1", &a, &a, 400.0, 1220.0);
        let rb = build_record("ZZ01-A1", &b, &b, 400.0, 1220.0);
        let rc = combine_records(&ra, &rb, 1.0);
        let s = &parse(&rc).unwrap()[0];
        assert_eq!(s.name, "ZZ01-A1");
        assert!((s.inphase.km - 3e-3).abs() < 1e-8);
        assert!((s.outphase.km - 3e-3).abs() < 1e-8);
        assert_eq!(rc[..180], ra[..180]);
    }

    #[test]
    fn pairing_uses_each_b_once_in_a_order() {
        let a: Vec<String> = ["C1", "B1", "A2", "B1"].iter().map(|s| s.to_string()).collect();
        let b: Vec<String> = ["B1", "C1", "B1"].iter().map(|s| s.to_string()).collect();
        assert_eq!(pair_by_name(&a, &b), vec![(0, 1), (1, 0), (3, 2)]);
    }

    #[test]
    fn overprint_endpoints_are_the_inputs() {
        let a = FabricIn { kmean: 1.0, p: 1.2, t: 0.5, kmax: (0.0, 0.0), kmin: (0.0, 90.0) };
        let b = FabricIn { kmean: 2.0, p: 1.3, t: 0.2, kmax: (90.0, 0.0), kmin: (0.0, 90.0) };
        let s = overprint_series(&a, &b, 11);
        assert_eq!(s.len(), 11);
        let ax = Axes::from_tensor(&s[0]);
        assert!((ax.shape.unwrap().p - 1.2).abs() < 1e-9 && (ax.shape.unwrap().t - 0.5).abs() < 1e-9);
        let km: f64 = (s[10][0][0] + s[10][1][1] + s[10][2][2]) / 3.0;
        assert!((km - 2.0).abs() < 1e-12);
        let mid = (s[5][0][0] + s[5][1][1] + s[5][2][2]) / 3.0;
        assert!((mid - 1.5).abs() < 1e-12);
        let rec = simulated_record("3", &s[3]);
        let sp = &parse(&rec).unwrap()[0];
        assert_eq!(sp.instrument, "SimulDat");
        assert!((sp.inphase.km - 1.3).abs() < 1e-6);
    }

    #[test]
    fn intersection_matches_the_original() {
        match intersect_lineations((30.0, 40.0), (120.0, 20.0)).unwrap() {
            Intersection::Plane { dip_dir, dip, angle } => {
                assert!((dip_dir - 53.4).abs() < 0.05 && (dip - 42.4).abs() < 0.05 && (angle - 77.3).abs() < 0.05);
            }
            _ => panic!(),
        }
        match intersect_foliations((100.0, 30.0), (200.0, 60.0)).unwrap() {
            Intersection::Line { trend, plunge, angle } => {
                assert!((trend - 127.2).abs() < 0.05 && (plunge - 27.2).abs() < 0.05 && (angle - 69.0).abs() < 0.05);
            }
            _ => panic!(),
        }
        assert!(intersect_lineations((30.0, 40.0), (30.0, 40.0)).is_none());
    }

    #[test]
    fn great_circle_lies_in_its_plane() {
        let pole = pole_of_plane(53.4, 42.4);
        for (d, i) in great_circle(53.4, 42.4, 40) {
            assert!(dot(from_dec_inc(d, i), pole).abs() < 1e-9);
        }
    }

    #[test]
    fn strain_matrices_match_the_original() {
        let m = strain_matrix(StrainKind::AxiallySymmetricShortening, 0, 2.0, 0.0);
        assert!((m[0][0] - 0.62996).abs() < 1e-5 && (m[1][1] - 1.25992).abs() < 1e-5);
        let m = strain_matrix(StrainKind::PureShear, 2, 2.0, 0.0); // Shorten Y / Extend X
        assert!((m[0][0] - 1.41421).abs() < 1e-5 && (m[1][1] - 0.70711).abs() < 1e-5 && m[2][2] == 1.0);
        let m = strain_matrix(StrainKind::Transpression, 0, 2.0, 0.5);
        assert!((m[1][0] - 0.59758).abs() < 1e-5 && (m[2][2] - 1.41421).abs() < 1e-5);
        let m = strain_matrix(StrainKind::Transpression, 0, 3.0, 0.8);
        assert!((m[1][0] - 1.06615).abs() < 1e-5);
        let m = strain_matrix(StrainKind::SimpleShear, 2, 7.0, 0.5);
        assert_eq!(m[0][2], 0.5);
    }

    fn eig_of(d: &Deform) -> [f64; 3] {
        eigenvalues(&d.fabric_tensor())
    }

    #[test]
    fn deform_ams_matches_measured_values() {
        let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-6);
        let mut d = Deform::new(true, 1.3);
        d.add_step(&strain_matrix(StrainKind::UniaxialShortening, 0, 2.0, 0.5));
        assert!(near(eig_of(&d), [1.024580, 1.024011, 0.951409]));
        d.add_step(&strain_matrix(StrainKind::UniaxialShortening, 0, 2.0, 0.5));
        assert!(near(eig_of(&d), [1.046507, 1.046039, 0.907455]));
        // order matters
        let mut d = Deform::new(true, 1.3);
        d.add_step(&strain_matrix(StrainKind::SimpleShear, 0, 2.0, 0.5));
        d.add_step(&strain_matrix(StrainKind::UniaxialShortening, 0, 2.0, 0.5));
        assert!(near(eig_of(&d), [1.027519, 1.025151, 0.947330]));
        let mut d = Deform::new(true, 1.3);
        d.add_step(&strain_matrix(StrainKind::UniaxialShortening, 0, 2.0, 0.5));
        d.add_step(&strain_matrix(StrainKind::SimpleShear, 0, 2.0, 0.5));
        assert!(near(eig_of(&d), [1.035558, 1.026758, 0.937684]));
        // prolate grains
        let mut d = Deform::new(false, 1.3);
        d.add_step(&strain_matrix(StrainKind::UniaxialShortening, 0, 2.0, 0.5));
        assert!(near(eig_of(&d), [1.022105, 1.021120, 0.956775]));
        // transpression
        let mut d = Deform::new(true, 1.3);
        d.add_step(&strain_matrix(StrainKind::Transpression, 0, 3.0, 0.8));
        assert!(near(eig_of(&d), [1.051427, 1.044856, 0.903717]));
        // reset restores the lattice fabric; 316 grains
        d.reset();
        assert_eq!(d.axes().len(), 316);
        assert!((eig_of(&d).iter().sum::<f64>() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn deform_fabric_axes_directions() {
        // shortening along X: Kmin along X (dec 0 or 180, inc 0)
        let mut d = Deform::new(true, 1.3);
        d.add_step(&strain_matrix(StrainKind::UniaxialShortening, 0, 2.0, 0.5));
        let ax = d.fabric_axes();
        assert!(ax.dec_inc[2].1.abs() < 1e-6);
        assert!(ax.dec_inc[2].0.abs() < 1e-6 || (ax.dec_inc[2].0 - 360.0).abs() < 1e-6 || (ax.dec_inc[2].0 - 180.0).abs() < 1e-6);
    }
}
