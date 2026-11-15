//! CSV export. `csv_both` (used by the program) writes in-phase and out-of-phase side by side;
//! `csv` writes one phase in the column order of Anisoft's own csv.

use crate::ams::{Phase, Specimen};

pub const HEADER: &str = "Specimen,Field,Freq,Km,K1,K1_SD,K2,K2_SD,K3,K3_SD,K1_Dec,K1_Inc,K2_Dec,K2_Inc,K3_Dec,K3_Inc,L,F,P,Pj,T,U,Q,E,FTest_F,FTest_F12,FTest_F23";

pub fn csv(specimens: &[&Specimen], out_of_phase: bool) -> String {
    let mut s = String::from(HEADER);
    s.push('\n');
    for sp in specimens {
        let ph: &Phase = if out_of_phase { &sp.outphase } else { &sp.inphase };
        let ax = ph.axes();
        let shape = |f: fn(&crate::tensor::Shape) -> f64| ax.shape.map(|x| format!("{:.7e}", f(&x))).unwrap_or_default();
        s += &format!(
            "{},{},{},{:.5e},{:.7},{:.6e},{:.7},{:.6e},{:.7},{:.6e},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{},{},{},{},{},{},{},{},{:.5e},{:.5e},{:.5e}\n",
            sp.name, sp.field_am, sp.freq_hz, ph.km,
            ax.k_norm[0], ph.sd[0], ax.k_norm[1], ph.sd[1], ax.k_norm[2], ph.sd[2],
            ax.dec_inc[0].0, ax.dec_inc[0].1, ax.dec_inc[1].0, ax.dec_inc[1].1, ax.dec_inc[2].0, ax.dec_inc[2].1,
            shape(|x| x.l), shape(|x| x.f), shape(|x| x.p), shape(|x| x.pj), shape(|x| x.t), shape(|x| x.u), shape(|x| x.q), shape(|x| x.e),
            ph.ftest[0], ph.ftest[1], ph.ftest[2],
        );
    }
    s
}

/// Per-phase columns, in this order (the `ip_` / `op_` prefix is added in the header).
pub const PHASE_COLUMNS: [&str; 65] = [
    "Km", "K1", "K1_SD", "K2", "K2_SD", "K3", "K3_SD", "K1_Dec", "K1_Inc", "K2_Dec", "K2_Inc", "K3_Dec", "K3_Inc",
    "L", "F", "P", "Pj", "T", "U", "Q", "E", "FTest_F", "FTest_F12", "FTest_F23", "E12", "E23", "E13",
    "Tyy", "Txx", "Tzz",
    // M3ORE lab sheet: volume-corrected values, Khan / Owens parameters, foliation and lineation
    "Km_vol", "K1_un", "K2_un", "K3_un", "L_Khan", "F_Khan", "H_Owens", "Lin_Plunge", "Fol_Dip", "Fol_DipDir",
    // and the lab sheet's errors: fractional (and absolute) errors of the values above
    "K1_frac", "K2_frac", "K3_frac", "Km_frac", "K1_un_frac", "K2_un_frac", "K3_un_frac",
    "L_frac", "L_abs", "L_Khan_frac", "L_Khan_abs", "F_frac", "F_abs", "F_Khan_frac", "F_Khan_abs",
    "P_frac", "P_abs", "H_frac", "H_abs", "Pj_frac", "Pj_abs", "T_frac", "T_abs", "U_frac", "U_abs",
];

/// Header of [`csv_both`]: `Specimen,Field,Freq,ip_*...,op_*...,opKm_over_ipKm_percent`.
pub fn header_both() -> String {
    let mut h = String::from("Specimen,Field,Freq");
    for pre in ["ip_", "op_"] {
        for c in PHASE_COLUMNS {
            h.push(',');
            h.push_str(pre);
            h.push_str(c);
        }
    }
    h.push_str(",opKm_over_ipKm_percent");
    h
}

fn phase_fields(ph: &Phase, vol_cm3: f64) -> Vec<String> {
    let ax = ph.axes();
    let v = crate::params::param_values_vol(ph.km, &ax, ph, vol_cm3);
    let g = |x: f64, d: usize| if x.is_finite() { format!("{x:.d$e}") } else { String::new() };
    let f = |x: f64| if x.is_finite() { format!("{x}") } else { String::new() };
    let mut o = vec![g(ph.km, 6)];
    for a in 0..3 {
        o.push(g(ax.k_norm[a], 7));
        o.push(g(ph.sd[a], 6));
    }
    for a in 0..3 {
        o.push(format!("{:.4}", ax.dec_inc[a].0));
        o.push(format!("{:.4}", ax.dec_inc[a].1));
    }
    for i in 1..9 {
        o.push(g(v[i], 7)); // L F P Pj T U Q E (blank when undefined)
    }
    for i in [18, 19, 20, 21, 22, 23] {
        o.push(f(v[i]));
    }
    // raw tensor components as stored (specimen coordinates)
    o.push(g(ph.tensor[1][1], 7));
    o.push(g(ph.tensor[0][0], 7));
    o.push(g(ph.tensor[2][2], 7));
    // lab sheet
    let l = crate::lab::lab(ph.km, ax.k_norm, ph.sd, vol_cm3, 0.0);
    for x in [l.km_vol, l.k_un[0], l.k_un[1], l.k_un[2]] {
        o.push(g(x, 6));
    }
    for i in [25, 26, 27] {
        o.push(g(v[i], 7));
    }
    o.push(format!("{:.4}", v[31]));
    o.push(format!("{:.4}", v[32]));
    o.push(format!("{:.4}", v[33]));
    for x in l.k_frac.iter().chain([&l.km_frac]).chain(l.k_un_frac.iter()) {
        o.push(g(*x, 6));
    }
    for val in [l.l, l.l_khan, l.f, l.f_khan, l.p, l.h_owens, l.pj, l.t, l.u] {
        o.push(g(val.frac, 6));
        o.push(g(val.abs, 6));
    }
    o
}

/// One row per specimen with the in-phase (`ip_`) and out-of-phase (`op_`) results side by side.
/// Values are in specimen coordinates, Km as stored (no volume correction); parameters that are
/// undefined (K3 <= 0) are left empty.
pub fn csv_both(specimens: &[&Specimen]) -> String {
    let mut s = header_both();
    s.push('\n');
    for sp in specimens {
        let mut row = vec![sp.name.clone(), format!("{}", sp.field_am), format!("{}", sp.freq_hz)];
        row.extend(phase_fields(&sp.inphase, sp.volume_cm3));
        row.extend(phase_fields(&sp.outphase, sp.volume_cm3));
        row.push(if sp.inphase.km != 0.0 { format!("{:.5}", 100.0 * sp.outphase.km / sp.inphase.km) } else { String::new() });
        s += &row.join(",");
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ams::{parse, build_record};
    use crate::tensor::sym;

    #[test]
    fn exports_header_and_row() {
        let p = Phase { km: 1e-3, sd: [1e-3; 3], tensor: sym(1.2e-3, 1.0e-3, 0.8e-3, 0.0, 0.0, 0.0), ftest: [5.0, 1.0, 1.0] };
        let sp = parse(&build_record("ZZ01-A1", &p, &p, 400.0, 1220.0)).unwrap();
        let out = csv(&[&sp[0]], false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], HEADER);
        assert_eq!(lines[1].split(',').count(), HEADER.split(',').count());
        assert!(lines[1].starts_with("ZZ01-A1,400,1220,"));
    }

    #[test]
    fn both_phases_side_by_side() {
        let ip = Phase { km: 2e-3, sd: [1e-3; 3], tensor: sym(1.2, 1.0, 0.8, 0.0, 0.0, 0.0), ftest: [5.0, 1.0, 1.0] };
        let op = Phase { km: 1e-5, sd: [2e-2; 3], tensor: sym(0.9, 1.0, 1.1, 0.1, 0.0, 0.0), ftest: [2.0, 1.0, 1.0] };
        let sp = parse(&build_record("ZZ01-A1", &ip, &op, 400.0, 1220.0)).unwrap();
        let out = csv_both(&[&sp[0]]);
        let lines: Vec<&str> = out.lines().collect();
        let head: Vec<&str> = lines[0].split(',').collect();
        let row: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(head.len(), row.len());
        assert_eq!(head.len(), 3 + 2 * PHASE_COLUMNS.len() + 1);
        assert_eq!(head[3], "ip_Km");
        assert_eq!(head[3 + PHASE_COLUMNS.len()], "op_Km");
        let col = |n: &str| head.iter().position(|h| *h == n).unwrap();
        let km_ip: f64 = row[col("ip_Km")].parse().unwrap();
        let km_op: f64 = row[col("op_Km")].parse().unwrap();
        assert!((km_ip - 2e-3).abs() < 1e-8 && (km_op - 1e-5).abs() < 1e-10);
        // in-phase P = 1.2/0.8 = 1.5; out-of-phase differs
        let p_ip: f64 = row[col("ip_P")].parse().unwrap();
        let p_op: f64 = row[col("op_P")].parse().unwrap();
        assert!((p_ip - 1.5).abs() < 1e-5 && (p_op - p_ip).abs() > 1e-6);
        let ratio: f64 = row[col("opKm_over_ipKm_percent")].parse().unwrap();
        assert!((ratio - 0.5).abs() < 1e-4);
    }

    #[test]
    fn lab_sheet_columns_and_errors_are_exported() {
        let ip = Phase { km: 2e-3, sd: [0.012, 0.01, 0.008], tensor: sym(1.2, 1.0, 0.8, 0.0, 0.0, 0.0), ftest: [5.0, 1.0, 1.0] };
        let sp = parse(&build_record("ZZ01-A1", &ip, &ip, 400.0, 1220.0)).unwrap();
        let out = csv_both(&[&sp[0]]);
        let lines: Vec<&str> = out.lines().collect();
        let head: Vec<&str> = lines[0].split(',').collect();
        let row: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(head.len(), row.len());
        let get = |n: &str| -> f64 { row[head.iter().position(|h| *h == n).unwrap()].parse().unwrap() };
        // 1 % error on every K: L = K1/K2 has 2 %, Khan's L is (K1 - K2) / Km = 0.2
        assert!((get("ip_L_frac") - 0.02).abs() < 1e-6 && (get("ip_L_abs") - 0.02 * 1.2 / 1.0).abs() < 1e-5);
        assert!((get("ip_L_Khan") - 0.2).abs() < 1e-6 && (get("ip_H_Owens") - 0.4).abs() < 1e-6);
        assert!((get("ip_Km_vol") - 2e-3).abs() < 1e-9, "the build_record volume is 10 cm3");
        assert!((get("ip_Fol_Dip") - 90.0 + get("ip_K3_Inc")).abs() < 1e-3);
        assert!(get("op_Pj_abs") > 0.0 && get("op_T_frac") > 0.0 && get("op_U_frac") > 0.0);
    }

    #[test]
    fn undefined_parameters_are_left_empty() {
        let neg = Phase { km: 1e-6, sd: [0.0; 3], tensor: sym(1.2, 1.0, -0.2, 0.0, 0.0, 0.0), ftest: [0.0; 3] };
        let sp = parse(&build_record("ZZ01-A1", &neg, &neg, 400.0, 1220.0)).unwrap();
        let out = csv_both(&[&sp[0]]);
        let lines: Vec<&str> = out.lines().collect();
        let head: Vec<&str> = lines[0].split(',').collect();
        let row: Vec<&str> = lines[1].split(',').collect();
        let i = head.iter().position(|h| *h == "ip_P").unwrap();
        assert_eq!(row[i], "");
    }
}
