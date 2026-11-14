//! Writes a made-up Safyr7-style .ams file (no real data) for demos and screenshots.
//! cargo run -p anisoft-core --example make_demo -- demo.ams [n]
use anisoft_core::ams::{build_record, Phase};
use anisoft_core::tensor::{mat_mul, sym, Mat3};

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn gauss(&mut self) -> f64 {
        let (u, v) = (self.next().max(1e-12), self.next());
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

fn rot_z(a: f64) -> Mat3 {
    let (s, c) = a.to_radians().sin_cos();
    [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
}
fn rot_x(a: f64) -> Mat3 {
    let (s, c) = a.to_radians().sin_cos();
    [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).expect("usage: make_demo out.ams [n]");
    let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let mut rng = Lcg(20261114);
    let mut bytes = vec![];
    for i in 0..n {
        let site = if i < n / 2 { "ZZ01" } else { "ZZ02" };
        let (dec, dip) = if i < n / 2 { (110.0, 30.0) } else { (250.0, 55.0) };
        let km = 1e-3 * (0.2 + 3.0 * rng.next().powi(2));
        // random P in 1.03..1.6 and shape T in -0.6..0.9, then normalise to a mean of 1
        let p = 1.03 + 0.57 * rng.next();
        let t_shape = -0.6 + 1.5 * rng.next();
        let lnp = p.ln();
        let (l1, l3) = (lnp / 2.0, -lnp / 2.0);
        let l2 = t_shape * lnp / 2.0;
        let (e1, e2, e3) = (l1.exp(), l2.exp(), l3.exp());
        let m = (e1 + e2 + e3) / 3.0;
        let (k1, k2, k3) = (e1 / m, e2 / m, e3 / m);
        let base: Mat3 = sym(k1, k2, k3, 0.0, 0.0, 0.0);
        let r = mat_mul(&rot_z(dec + 12.0 * rng.gauss()), &rot_x(dip + 8.0 * rng.gauss()));
        let t = anisoft_core::tensor::rotate_tensor(&base, &r);
        let scaled = |m: f64| -> Mat3 { let mut x = t; for row in x.iter_mut() { for v in row.iter_mut() { *v *= m; } } x };
        // like real files: the stored tensor is normalised (trace 3), Km is stored separately
        let ip = Phase { km, sd: [2e-3, 2e-3, 2e-3], tensor: t, ftest: [20.0 + 60.0 * rng.next(), 5.0, 8.0] };
        let op = Phase { km: km * 0.01, sd: [3e-2; 3], tensor: scaled(km * 0.01 * (1.0 + 0.5 * rng.gauss())), ftest: [1.5, 1.0, 1.0] };
        bytes.extend(build_record(&format!("{site}-{}{}", ["A", "B", "C"][i % 3], i / 3 + 1), &ip, &op, 400.0, 1220.0));
    }
    std::fs::write(out, bytes).expect("write");
    println!("wrote {n} made-up specimens to {out}");
}
