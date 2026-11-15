//! cargo run -p ams-core --example validate -- file.ams anisoft_export.csv
//! Compares the decoded .ams against a csv exported by the original Anisoft.
use ams_core::ams;
use std::collections::HashMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let specs = ams::read_file(std::path::Path::new(&a[1])).expect("read ams");
    let csv = std::fs::read_to_string(&a[2]).expect("read csv");
    let mut lines = csv.lines();
    let head: Vec<&str> = lines.next().unwrap().split(',').map(|s| s.trim()).collect();
    let col = |n: &str| head.iter().position(|h| *h == n).unwrap_or_else(|| panic!("no column {n}"));
    let mut rows: HashMap<String, Vec<f64>> = HashMap::new();
    for l in lines {
        let f: Vec<&str> = l.split(',').collect();
        if f.len() < head.len() - 1 { continue; }
        let name = f[col("Specimen")].trim().to_string();
        rows.insert(name, f.iter().map(|s| s.trim().parse::<f64>().unwrap_or(f64::NAN)).collect());
    }
    let names = ["Km","K1","K2","K3","K1_Dec","K1_Inc","K2_Dec","K2_Inc","K3_Dec","K3_Inc","L","F","P","Pj","T","U","Q","E","K1_SD","K2_SD","K3_SD","FTest_F","FTest_F12","FTest_F23"];
    let mut worst: HashMap<&str, (f64, String)> = HashMap::new();
    let (mut matched, mut missing, mut sentinel) = (0, 0, 0);
    for s in &specs {
        // join on Km (names can differ between the .ams and an older csv, e.g. after an A/B rename)
        let km_col = col("Km");
        let Some(row) = rows.values().filter(|r| (r[km_col] - s.inphase.km).abs() <= 1e-4 * s.inphase.km.abs()).min_by(|a, b| (a[km_col] - s.inphase.km).abs().partial_cmp(&(b[km_col] - s.inphase.km).abs()).unwrap()) else { missing += 1; continue };
        matched += 1;
        let ax = s.inphase.axes();
        let sh = ax.shape;
        let ang = |x: f64, y: f64| { let d = (x - y).abs() % 360.0; d.min(360.0 - d) };
        for n in names {
            let want = row[col(n)];
            let got = match n {
                "Km" => s.inphase.km,
                "K1" => ax.k_norm[0], "K2" => ax.k_norm[1], "K3" => ax.k_norm[2],
                "K1_Dec" => ax.dec_inc[0].0, "K1_Inc" => ax.dec_inc[0].1,
                "K2_Dec" => ax.dec_inc[1].0, "K2_Inc" => ax.dec_inc[1].1,
                "K3_Dec" => ax.dec_inc[2].0, "K3_Inc" => ax.dec_inc[2].1,
                "K1_SD" => s.inphase.sd[0], "K2_SD" => s.inphase.sd[1], "K3_SD" => s.inphase.sd[2],
                "FTest_F" => s.inphase.ftest[0], "FTest_F12" => s.inphase.ftest[1], "FTest_F23" => s.inphase.ftest[2],
                "L"|"F"|"P"|"Pj"|"T"|"U"|"Q"|"E" => match sh {
                    Some(sh) => match n { "L"=>sh.l,"F"=>sh.f,"P"=>sh.p,"Pj"=>sh.pj,"T"=>sh.t,"U"=>sh.u,"Q"=>sh.q,_=>sh.e },
                    None => { if want == -2.0 { sentinel += 1; } continue }
                },
                _ => unreachable!(),
            };
            let err = if n.ends_with("Dec") { ang(got, want) } else if n.ends_with("Inc") { (got - want).abs() }
                      else { (got - want).abs() / want.abs().max(1e-30) };
            let e = worst.entry(n).or_insert((0.0, String::new()));
            if err > e.0 { *e = (err, s.name.clone()); }
        }
    }
    println!("specimens in .ams: {}, matched in csv: {matched}, not in csv: {missing}, sentinel(-2) agreements: {sentinel}", specs.len());
    for n in names {
        let (e, who) = worst.get(n).cloned().unwrap_or((f64::NAN, String::new()));
        println!("{n:10} worst {} {e:.3e}  ({who})", if n.ends_with("Dec")||n.ends_with("Inc") {"abs deg"} else {"rel    "});
    }
}
