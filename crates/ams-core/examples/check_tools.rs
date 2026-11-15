//! Compare the tools code with files written by the original Anisoft.
//! cargo run -p ams-core --example check_tools -- A.ams B.ams added.ams subtracted.ams overprint.ams
use ams_core::ams::{parse, RECORD_LEN};
use ams_core::tools::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let rd = |i: usize| std::fs::read(&a[i]).expect("read");
    let (ba, bb, badd, bsub, bfo) = (rd(1), rd(2), rd(3), rd(4), rd(5));
    let (sa, sb) = (parse(&ba).unwrap(), parse(&bb).unwrap());
    let names = |s: &[ams_core::ams::Specimen]| s.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    let pairs = pair_by_name(&names(&sa), &names(&sb));
    for (label, sign, bytes) in [("add", 1.0, &badd), ("subtract", -1.0, &bsub)] {
        let want = parse(bytes).unwrap();
        println!("{label}: {} pairs, original wrote {} records", pairs.len(), want.len());
        let (mut worst_k, mut worst_t, mut name_bad) = (0f64, 0f64, 0);
        for (n, (i, j)) in pairs.iter().enumerate() {
            let rec = combine_records(&ba[i * RECORD_LEN..(i + 1) * RECORD_LEN], &bb[j * RECORD_LEN..(j + 1) * RECORD_LEN], sign);
            let got = &parse(&rec).unwrap()[0];
            let w = &want[n];
            if got.name != w.name { name_bad += 1; }
            for (g, x) in [(&got.inphase, &w.inphase), (&got.outphase, &w.outphase)] {
                worst_k = worst_k.max((g.km - x.km).abs() / x.km.abs().max(1e-30));
                for r in 0..3 { for c in 0..3 { worst_t = worst_t.max((g.tensor[r][c] - x.tensor[r][c]).abs()); } }
            }
        }
        println!("   name mismatches {name_bad}, worst Km rel err {worst_k:.2e}, worst tensor err {worst_t:.2e}");
    }
    // fabric overprint: A (1.0, 1.2, 0.5, 0/0, 0/90) -> B (2.0, 1.3, 0.2, 90/0, 0/90), 11 increments
    let fa = FabricIn { kmean: 1.0, p: 1.2, t: 0.5, kmax: (0.0, 0.0), kmin: (0.0, 90.0) };
    let fb = FabricIn { kmean: 2.0, p: 1.3, t: 0.2, kmax: (90.0, 0.0), kmin: (0.0, 90.0) };
    let want = parse(&bfo).unwrap();
    let series = overprint_series(&fa, &fb, want.len());
    let mut worst = 0f64;
    for (t, w) in series.iter().zip(&want) {
        let rec = simulated_record("x", t);
        let g = &parse(&rec).unwrap()[0];
        worst = worst.max((g.inphase.km - w.inphase.km).abs());
        for r in 0..3 { for c in 0..3 { worst = worst.max((g.inphase.tensor[r][c] - w.inphase.tensor[r][c]).abs()); } }
    }
    println!("overprint: {} records, worst error vs original {worst:.2e}", want.len());
}
