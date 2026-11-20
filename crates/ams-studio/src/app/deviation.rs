//! The K1 / K3 deviation parameters: the angle of a cube's axis to the mean of the same axis over the background cubes of
//! its specimen (the last two rows of the AMS dashboard). Background = outside the alteration intervals kept with the
//! specimen (`alteration` section of its dependency file); without intervals every cube whose axis passes its F-tests makes
//! the reference, as in the dashboard.

use super::*;
use ams_core::alteration::{self, AltIntervals, RefKind, F_CRITICAL, SUB_F_CRITICAL};

/// The deviations (K1, K3) of every record of `doc` in the phase and coordinate system `derived` was made for, and how the
/// two references came about.
pub(super) fn compute(doc: &Doc, derived: &[Derived], out_phase: bool, alt: Option<&AltIntervals>) -> (Vec<[f64; 2]>, [RefKind; 2]) {
    let n = doc.specs.len().min(derived.len());
    let background: Vec<Option<bool>> = (0..n).map(|i| alt.map_or(Some(false), |a| a.altered(&doc.specs[i].name).map(|x| !x))).collect();
    let mut out = vec![[f64::NAN; 2]; n];
    let mut kinds = [RefKind::None; 2];
    for (slot, (axis, sub)) in [(0usize, 1usize), (2, 2)].into_iter().enumerate() {
        let dirs: Vec<(f64, f64)> = (0..n).map(|i| derived[i].axes.dec_inc[axis]).collect();
        // the axis is resolved when the overall F-test and the test of its pair pass (K1: F12, K3: F23)
        let resolved: Vec<bool> = (0..n)
            .map(|i| {
                let ph = if out_phase { &doc.specs[i].outphase } else { &doc.specs[i].inphase };
                ph.ftest[0] >= F_CRITICAL && ph.ftest[sub] >= SUB_F_CRITICAL
            })
            .collect();
        let (d, kind) = alteration::deviations(&dirs, &resolved, &background);
        for i in 0..n {
            out[i][slot] = d[i];
        }
        kinds[slot] = kind;
    }
    (out, kinds)
}

/// One line saying what the reference of a deviation was (for the hover text of the parameter and the self-test).
pub(super) fn describe(kind: RefKind, has_intervals: bool) -> String {
    match kind {
        RefKind::Background(n) => format!("reference: mean of {n} background cubes"),
        RefKind::AllCubes(n) if has_intervals => format!("reference: mean of all {n} cubes with a resolved axis (fewer than {} background cubes)", alteration::MIN_BACKGROUND),
        RefKind::AllCubes(n) => format!("reference: mean of all {n} cubes with a resolved axis (no alteration intervals stored for this specimen)"),
        RefKind::None => "no reference (no direction)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not a test of its own: writes the directions, F-tests and deviations of a real file for a comparison with the Python
    /// dashboard code. `AMS_DEV_FILE=<file> AMS_DEV_OUT=<csv> cargo test -p ams-studio dev_dump -- --ignored`
    #[test]
    #[ignore]
    fn dev_dump() {
        let (Ok(f), Ok(out)) = (std::env::var("AMS_DEV_FILE"), std::env::var("AMS_DEV_OUT")) else { return };
        let doc = Doc::open(std::path::Path::new(&f)).unwrap();
        let alt = ams_core::alteration::read_alteration(&doc.path);
        let mut csv = String::from("name,phase,k1dec,k1inc,k3dec,k3inc,f,f12,f23,dev1,dev3,altered\n");
        for ph in [false, true] {
            let der = doc.derive(ph, ams_core::coords::CoordSys::Specimen, &Default::default());
            let (dev, kinds) = compute(&doc, &der, ph, alt.as_ref());
            eprintln!("phase {ph}: {kinds:?}");
            for (i, s) in doc.specs.iter().enumerate() {
                let p = if ph { &s.outphase } else { &s.inphase };
                let a = &der[i].axes.dec_inc;
                csv += &format!("{},{},{},{},{},{},{},{},{},{},{},{:?}\n", s.name, ph as u8, a[0].0, a[0].1, a[2].0, a[2].1, p.ftest[0], p.ftest[1], p.ftest[2], dev[i][0], dev[i][1], alt.as_ref().and_then(|x| x.altered(&s.name)));
            }
        }
        std::fs::write(out, csv).unwrap();
    }
}
