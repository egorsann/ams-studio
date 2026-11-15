//! The open document: specimens, which of them are active/selected, and folder navigation.

use ams_core::ams::{self, put_phase, Axes, Specimen, NAME_LEN, OFF_KIM, OFF_KRE, OFF_NAME, RECORD_LEN};
use ams_core::coords::{CoordSys, Orientation};
use ams_core::tensor::{rotate_tensor, Mat3};
use std::path::{Path, PathBuf};

pub struct Doc {
    pub path: PathBuf,
    /// Original bytes, so that saved subsets stay lossless.
    pub raw: Vec<u8>,
    pub specs: Vec<Specimen>,
    pub active: Vec<bool>,
    pub selected: Vec<bool>,
    pub modified: bool,
    /// Built in memory by a tool (e.g. Fabric Overprint); has no file behind it.
    pub synthetic: bool,
}

#[derive(Clone)]
pub struct Derived {
    pub km: f64,
    pub axes: Axes,
    pub tensor: Mat3,
}

impl Doc {
    pub fn open(path: &Path) -> Result<Doc, String> {
        let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let specs = ams::parse(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
        let n = specs.len();
        Ok(Doc { path: path.to_path_buf(), raw, specs, active: vec![true; n], selected: vec![false; n], modified: false, synthetic: false })
    }

    /// A document made by a tool from records already in memory.
    pub fn simulated(name: &str, raw: Vec<u8>) -> Result<Doc, String> {
        let specs = ams::parse(&raw).map_err(|e| e.to_string())?;
        let n = specs.len();
        Ok(Doc { path: PathBuf::from(name), raw, specs, active: vec![true; n], selected: vec![false; n], modified: false, synthetic: true })
    }

    /// Specimen indices in list order: active ones first, disregarded ones at the bottom.
    pub fn order(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.specs.len()).filter(|&i| self.active[i]).collect();
        v.extend((0..self.specs.len()).filter(|&i| !self.active[i]));
        v
    }

    pub fn n_active(&self) -> usize {
        self.active.iter().filter(|a| **a).count()
    }
    pub fn n_selected(&self) -> usize {
        self.selected.iter().filter(|a| **a).count()
    }

    pub fn derive(&self, out_of_phase: bool, cs: CoordSys, o: &Orientation) -> Vec<Derived> {
        let r = o.rotation(cs);
        self.specs
            .iter()
            .map(|s| {
                let ph = if out_of_phase { &s.outphase } else { &s.inphase };
                // geographic coordinates come from the orientation stored in the record when it has one
                let r = match (cs, &s.orient) {
                    (CoordSys::Geographic, Some(ro)) => ams_core::orient::geo_rotation(ro),
                    _ => r,
                };
                let tensor = rotate_tensor(&ph.tensor, &r);
                Derived { km: ph.km, axes: Axes::from_tensor(&tensor), tensor }
            })
            .collect()
    }

    /// Raw 640-byte records of the given indices, concatenated.
    pub fn records(&self, idx: impl Iterator<Item = usize>) -> Vec<u8> {
        let mut out = vec![];
        for i in idx {
            out.extend_from_slice(&self.raw[i * RECORD_LEN..(i + 1) * RECORD_LEN]);
        }
        out
    }

    /// Rotates the tensors of specimen `i` (both phases) by `r`, in the records too, so a saved file carries them.
    /// Km, the standard deviations and the F-tests are unchanged by a rotation.
    pub fn rotate_specimen(&mut self, i: usize, r: &Mat3) {
        let rec = &mut self.raw[i * RECORD_LEN..(i + 1) * RECORD_LEN];
        let s = &mut self.specs[i];
        for (ph, base) in [(&mut s.inphase, OFF_KRE), (&mut s.outphase, OFF_KIM)] {
            ph.tensor = rotate_tensor(&ph.tensor, r);
            put_phase(rec, base, ph.km, &ph.tensor);
        }
        self.modified = true;
    }

    /// Applies the Edit Data table: names and the orientation of every record (`None` clears it).
    pub fn apply_edit(&mut self, rows: Vec<(String, Option<ams_core::orient::RecOrient>)>) {
        for (i, (name, o)) in rows.into_iter().enumerate().take(self.specs.len()) {
            if name.trim() != self.specs[i].name {
                self.rename(i, &name);
            }
            if self.specs[i].orient != o {
                ams_core::orient::write(&mut self.raw[i * RECORD_LEN..(i + 1) * RECORD_LEN], o.as_ref());
                self.specs[i].orient = o;
                self.modified = true;
            }
        }
    }

    pub fn rename(&mut self, i: usize, name: &str) {
        let name: String = name.chars().filter(|c| c.is_ascii() && !c.is_control()).take(NAME_LEN).collect();
        let rec = &mut self.raw[i * RECORD_LEN + OFF_NAME..i * RECORD_LEN + OFF_NAME + NAME_LEN];
        rec.fill(b' ');
        rec[..name.len()].copy_from_slice(name.as_bytes());
        self.specs[i].name = name.trim().to_string();
        self.modified = true;
    }
}

pub fn is_ams(p: &Path) -> bool {
    p.extension().map_or(false, |e| e.eq_ignore_ascii_case("ams"))
}

/// Sorted list of `.ams` files in `dir`.
pub fn ams_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file() && is_ams(p)).collect())
        .unwrap_or_default();
    v.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    v
}

/// (previous, next, position (1-based), total) of `path` within its folder.
pub fn neighbours(path: &Path) -> (Option<PathBuf>, Option<PathBuf>, usize, usize) {
    let dir = path.parent().unwrap_or(Path::new("."));
    let all = ams_in(dir);
    match all.iter().position(|p| p == path) {
        Some(i) => (
            i.checked_sub(1).map(|j| all[j].clone()),
            all.get(i + 1).cloned(),
            i + 1,
            all.len(),
        ),
        None => (None, None, 0, all.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ams_core::ams::{build_record, Phase};
    use ams_core::tensor::sym;

    fn tmp_doc() -> (tempdir::D, PathBuf) {
        let d = tempdir::D::new();
        let ph = Phase { km: 1e-3, sd: [1e-3; 3], tensor: sym(1.2e-3, 1.0e-3, 0.8e-3, 0.0, 0.0, 0.0), ftest: [5.0, 1.0, 1.0] };
        let mut b = build_record("ZZ01-A1", &ph, &ph, 400.0, 1220.0);
        b.extend(build_record("ZZ01-B1", &ph, &ph, 400.0, 1220.0));
        let p = d.0.join("ZZ01.ams");
        std::fs::write(&p, b).unwrap();
        (d, p)
    }

    mod tempdir {
        pub struct D(pub std::path::PathBuf);
        impl D {
            pub fn new() -> D {
                let p = std::env::temp_dir().join(format!("ams-studio-test-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')', ' '], ""));
                std::fs::create_dir_all(&p).unwrap();
                D(p)
            }
        }
        impl Drop for D {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn open_rename_and_subset() {
        let (_d, p) = tmp_doc();
        let mut doc = Doc::open(&p).unwrap();
        assert_eq!(doc.specs.len(), 2);
        doc.rename(1, "ZZ01-X9");
        let re = ams::parse(&doc.records([1usize].into_iter())).unwrap();
        assert_eq!(re.len(), 1);
        assert_eq!(re[0].name, "ZZ01-X9");
        assert!((re[0].inphase.km - 1e-3).abs() < 1e-9);
    }

    #[test]
    fn edit_data_writes_orientation_and_names() {
        let (_d, p) = tmp_doc();
        let mut doc = Doc::open(&p).unwrap();
        assert!(doc.specs[0].orient.is_none());
        let o = ams_core::orient::RecOrient { op: [9, 90, 3, 90], alpha: 41.0, phi: 31.0, fol_code: ['7', '5'], fol: [(61.0, 21.0), (151.0, 44.0)], ..Default::default() };
        doc.apply_edit(vec![("ZZ01-A1".into(), Some(o.clone())), ("ZZ01-X2".into(), None)]);
        assert!(doc.modified);
        let again = ams::parse(&doc.raw).unwrap();
        assert_eq!(again[0].orient, Some(o));
        assert_eq!(again[1].name, "ZZ01-X2");
        assert!(again[1].orient.is_none());
        // the geographic view now uses the record's own rotation
        let d = doc.derive(false, CoordSys::Geographic, &Orientation::default());
        let s = doc.derive(false, CoordSys::Specimen, &Orientation::default());
        assert!((d[0].axes.dec_inc[0].0 - s[0].axes.dec_inc[0].0).abs() > 1.0);
        assert!((d[1].axes.dec_inc[0].0 - s[1].axes.dec_inc[0].0).abs() < 1e-9);
    }

    #[test]
    fn rotation_reaches_the_records() {
        let (_d, p) = tmp_doc();
        let mut doc = Doc::open(&p).unwrap();
        let r = ams_core::coords::owens_rotation(30.0, 20.0, 10.0, 15.0);
        doc.rotate_specimen(1, &r);
        assert!(doc.modified);
        let again = ams::parse(&doc.raw).unwrap();
        let (a, b) = (again[1].inphase.axes(), doc.specs[1].inphase.axes());
        for k in 0..3 {
            assert!((a.k_norm[k] - b.k_norm[k]).abs() < 1e-5);
        }
        assert!((a.dec_inc[0].0 - b.dec_inc[0].0).abs() < 0.05);
        // the first specimen and Km are untouched; the eigenvalues survive the rotation
        let first = ams::parse(&doc.raw).unwrap();
        assert!((first[0].inphase.tensor[0][0] - 1.2e-3).abs() < 1e-8);
        assert!((first[1].inphase.km - 1e-3).abs() < 1e-9);
        assert!((first[1].inphase.axes().k_raw[0] - 1.2e-3).abs() < 1e-7);
    }

    #[test]
    fn neighbours_in_folder() {
        let (d, p) = tmp_doc();
        let q = d.0.join("ZZ02.ams");
        std::fs::copy(&p, &q).unwrap();
        let (prev, next, pos, total) = neighbours(&p);
        assert!(prev.is_none() && next == Some(q.clone()) && pos == 1 && total == 2);
        let (prev, next, pos, _) = neighbours(&q);
        assert!(prev == Some(p) && next.is_none() && pos == 2);
    }
}
