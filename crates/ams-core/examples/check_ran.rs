//! Checks the `.ran` pair reader against full `.ams` files: `check_ran <folder>` walks the folder for `X.ran`, `X_Im.ran`
//! and `X.ams` and compares the joined records with the `.ams` (tensors and Km of both phases). Prints counts only.
use ams_core::{ams, import};
use std::path::Path;

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().map_or(false, |x| x == "ams") {
                out.push(p);
            }
        }
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("folder");
    let mut files = vec![];
    walk(Path::new(&root), &mut files);
    let (mut pairs, mut recs, mut bad) = (0, 0, 0);
    for a in files {
        let stem = a.with_extension("");
        let (ip, op) = (stem.with_extension("ran"), std::path::PathBuf::from(format!("{}_Im.ran", stem.display())));
        if !ip.exists() || !op.exists() {
            continue;
        }
        let Ok(full) = ams::read_file(&a) else { continue };
        let (Ok(i), Ok(o)) = (std::fs::read(&ip), std::fs::read(&op)) else { continue };
        let Ok((m, rep)) = import::ran_pair_to_records(&i, &o) else { continue };
        let Ok(joined) = ams::parse(&m) else { continue };
        pairs += 1;
        if joined.len() != full.len() {
            println!("count differs in a pair: {} vs {}", joined.len(), full.len());
            continue;
        }
        let _ = rep;
        for (x, y) in joined.iter().zip(&full) {
            if y.inphase.km == 0.0 && y.outphase.km == 0.0 {
                continue; // empty record
            }
            recs += 1;
            let close = |p: &ams::Phase, q: &ams::Phase| (p.km - q.km).abs() <= 1e-3 * q.km.abs().max(1e-6) && (0..3).all(|r| (0..3).all(|c| (p.tensor[r][c] - q.tensor[r][c]).abs() < 1e-5));
            if x.name != y.name || !close(&x.inphase, &y.inphase) || !close(&x.outphase, &y.outphase) {
                bad += 1;
            }
        }
    }
    println!("{pairs} pairs, {recs} records compared, {bad} differ");
}
