//! Import of other formats into `.ams` records: Anisoft 4.2's `.ran` and the csv written by Anisoft 6 / AMS Studio.
//!
//! Both are converted to 640-byte `.ams` records (the same bytes a Safyr7 file holds), so everything else in the
//! program works on them. Fields the source does not have are left at zero (standard deviations, F-tests,
//! orientation); a `.ran` file has no out-of-phase data.
//!
//! The csv carries eigenvalues and directions but not the full tensor, so the tensor is rebuilt from them
//! (`sum k_i v_i v_i^T`); that is exact to the precision of the printed numbers (4 decimals of a degree).

use crate::ams::{build_record_with, Phase};
use crate::tensor::{cross, from_dec_inc, norm, sym, Mat3, Vec3};

fn zero_phase() -> Phase {
    Phase { km: 0.0, sd: [0.0; 3], tensor: [[0.0; 3]; 3], ftest: [0.0; 3] }
}

/// `.ran`: 64-byte header, then 64-byte records (see docs/FORMATS.md). Axes are x north, y east, z down.
pub fn ran_to_records(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 128 || (bytes.len() - 64) % 64 != 0 {
        return Err(format!("not a .ran file: {} bytes is not 64 + n x 64", bytes.len()));
    }
    let f = |r: &[u8], o: usize| f32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]) as f64;
    let mut out = vec![];
    for rec in bytes[64..].chunks_exact(64) {
        let name: String = rec[..12].iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { ' ' }).collect::<String>().trim().to_string();
        let km = f(rec, 16) * 1e-6;
        let (nn, ee, dd, ne, ed, nd) = (f(rec, 20), f(rec, 24), f(rec, 28), f(rec, 32), f(rec, 36), f(rec, 40));
        // this crate's tensor has x east, y north
        let t = sym(ee, nn, dd, ne, ed, nd);
        let p = Phase { km, sd: [0.0; 3], tensor: t, ftest: [0.0; 3] };
        out.extend(build_record_with(&name, &p, &zero_phase(), 0.0, 0.0, "RAN"));
    }
    Ok(out)
}

fn split_csv(line: &str) -> Vec<String> {
    line.split(',').map(|s| s.trim().trim_matches('"').to_string()).collect()
}

fn tensor_from_axes(k: [f64; 3], d: [(f64, f64); 3]) -> Mat3 {
    let v1 = from_dec_inc(d[0].0, d[0].1);
    let mut v2 = from_dec_inc(d[1].0, d[1].1);
    let dp = v1[0] * v2[0] + v1[1] * v2[1] + v1[2] * v2[2];
    v2 = [v2[0] - dp * v1[0], v2[1] - dp * v1[1], v2[2] - dp * v1[2]];
    let n = norm(v2).max(1e-12);
    v2 = [v2[0] / n, v2[1] / n, v2[2] / n];
    let v3: Vec3 = cross(v1, v2);
    let mut t = [[0.0; 3]; 3];
    for (kk, v) in k.iter().zip([v1, v2, v3]) {
        for i in 0..3 {
            for j in 0..3 {
                t[i][j] += kk * v[i] * v[j];
            }
        }
    }
    t
}

/// csv with a `Specimen` column and K1..K3 with directions, either one phase (`Km`, `K1`, ...) or both (`ip_*`, `op_*`).
/// Returns the records and the number of rows skipped (blank or unreadable).
pub fn csv_to_records(text: &str) -> Result<(Vec<u8>, usize), String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let head = split_csv(lines.next().ok_or("empty file")?);
    let col = |n: &str| head.iter().position(|h| h.eq_ignore_ascii_case(n));
    let name_c = col("Specimen").ok_or("no Specimen column: this is not a csv written by Anisoft or AMS Studio")?;
    let both = col("ip_K1").is_some();
    let key = |pre: &str, n: &str| col(&format!("{pre}{n}"));
    let phase_cols = |pre: &str| -> Option<[usize; 10]> {
        Some([key(pre, "Km")?, key(pre, "K1")?, key(pre, "K2")?, key(pre, "K3")?, key(pre, "K1_Dec")?, key(pre, "K1_Inc")?, key(pre, "K2_Dec")?, key(pre, "K2_Inc")?, key(pre, "K3_Dec")?, key(pre, "K3_Inc")?])
    };
    let ip = phase_cols(if both { "ip_" } else { "" }).ok_or("missing Km / K1..K3 / direction columns")?;
    let op = if both { phase_cols("op_") } else { None };
    let (fc, qc) = (col("Field"), col("Freq"));
    let mut out = vec![];
    let mut skipped = 0;
    for line in lines {
        let c = split_csv(line);
        let num = |i: usize| c.get(i).and_then(|s| s.parse::<f64>().ok());
        let phase = |cols: &[usize; 10], sd_pre: &str, ft_pre: &str| -> Option<Phase> {
            let km = num(cols[0])?;
            let k = [num(cols[1])?, num(cols[2])?, num(cols[3])?];
            let d = [(num(cols[4])?, num(cols[5])?), (num(cols[6])?, num(cols[7])?), (num(cols[8])?, num(cols[9])?)];
            let sd = [0, 1, 2].map(|a| col(&format!("{sd_pre}K{}_SD", a + 1)).and_then(&num).unwrap_or(0.0));
            let ftest = ["FTest_F", "FTest_F12", "FTest_F23"].map(|n| col(&format!("{ft_pre}{n}")).and_then(&num).unwrap_or(0.0));
            Some(Phase { km, sd, tensor: tensor_from_axes(k, d), ftest })
        };
        let pre = if both { "ip_" } else { "" };
        let Some(pin) = phase(&ip, pre, pre) else {
            skipped += 1;
            continue;
        };
        let pout = op.as_ref().and_then(|o| phase(o, "op_", "op_")).unwrap_or_else(zero_phase);
        let name = c.get(name_c).cloned().unwrap_or_default();
        let field = fc.and_then(&num).unwrap_or(0.0) as f32;
        let freq = qc.and_then(&num).unwrap_or(0.0) as f32;
        out.extend(build_record_with(&name, &pin, &pout, field, freq, "CSV"));
    }
    if out.is_empty() {
        return Err("no readable rows".into());
    }
    Ok((out, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ams::{build_record, parse};
    use crate::export::{csv, csv_both};

    #[test]
    fn ran_round_trip_of_a_made_up_file() {
        let mut b = vec![b' '; 64];
        b[0] = b'\\';
        let mut rec = vec![0u8; 64];
        rec[..7].copy_from_slice(b"ZZ01-A1");
        rec[7..12].fill(b' ');
        for (o, v) in [(16usize, 850.0f32), (20, 1.2), (24, 1.0), (28, 0.8), (32, 0.05), (36, 0.0), (40, -0.03)] {
            rec[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        b.extend(&rec);
        b.extend(&rec);
        let r = ran_to_records(&b).unwrap();
        let s = parse(&r).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "ZZ01-A1");
        assert!((s[0].inphase.km - 850e-6).abs() < 1e-9);
        // north-north is the first stored component; this crate keeps it in the y row
        assert!((s[0].inphase.tensor[1][1] - 1.2).abs() < 1e-6 && (s[0].inphase.tensor[0][0] - 1.0).abs() < 1e-6);
        assert!(ran_to_records(&b[..100]).is_err());
    }

    fn made_up() -> Vec<crate::ams::Specimen> {
        let mk = |km: f64, t: Mat3| Phase { km, sd: [1e-3, 2e-3, 3e-3], tensor: t, ftest: [5.0, 2.0, 1.5] };
        let t1 = tensor_from_axes([1.15, 1.0, 0.85], [(30.0, 10.0), (120.0, 5.0), (240.0, 78.0)]);
        let t2 = tensor_from_axes([1.3, 0.95, 0.75], [(200.0, 40.0), (310.0, 20.0), (60.0, 43.0)]);
        let mut b = build_record("ZZ01-A1", &mk(2e-3, t1), &mk(2e-5, t2), 400.0, 1220.0);
        b.extend(build_record("ZZ01-B1", &mk(1e-3, t2), &mk(1e-5, t1), 400.0, 1220.0));
        parse(&b).unwrap()
    }

    #[test]
    fn csv_of_both_phases_comes_back() {
        let sp = made_up();
        let refs: Vec<&_> = sp.iter().collect();
        let (r, skipped) = csv_to_records(&csv_both(&refs)).unwrap();
        assert_eq!(skipped, 0);
        let back = parse(&r).unwrap();
        assert_eq!(back.len(), 2);
        for (a, b) in sp.iter().zip(&back) {
            assert_eq!(a.name, b.name);
            for (pa, pb) in [(&a.inphase, &b.inphase), (&a.outphase, &b.outphase)] {
                assert!((pa.km - pb.km).abs() < 1e-8);
                let (x, y) = (pa.axes(), pb.axes());
                for k in 0..3 {
                    assert!((x.k_norm[k] - y.k_norm[k]).abs() < 1e-5);
                    assert!((x.dec_inc[k].0 - y.dec_inc[k].0).abs() < 0.01 || (x.dec_inc[k].1 - y.dec_inc[k].1).abs() < 1e-3);
                }
            }
        }
    }

    #[test]
    fn csv_of_one_phase_and_bad_input() {
        let sp = made_up();
        let (r, _) = csv_to_records(&csv(&[&sp[0]], false)).unwrap();
        let back = parse(&r).unwrap();
        assert_eq!(back.len(), 1);
        assert!((back[0].inphase.km - sp[0].inphase.km).abs() < 1e-8);
        assert!(back[0].outphase.km == 0.0);
        assert!(csv_to_records("a,b\n1,2\n").is_err());
        assert!(csv_to_records("").is_err());
    }
}
