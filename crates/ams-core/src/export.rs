//! CSV export in the column order of Anisoft's own csv (without the *_SD columns of derived parameters).

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
}
