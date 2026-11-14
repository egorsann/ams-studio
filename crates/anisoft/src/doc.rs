//! The open document: specimens, which of them are active/selected, and folder navigation.

use anisoft_core::ams::{self, Axes, Specimen, NAME_LEN, OFF_NAME, RECORD_LEN};
use anisoft_core::coords::{CoordSys, Orientation};
use anisoft_core::tensor::{rotate_tensor, Mat3};
use std::path::{Path, PathBuf};

pub struct Doc {
    pub path: PathBuf,
    /// Original bytes, so that saved subsets stay lossless.
    pub raw: Vec<u8>,
    pub specs: Vec<Specimen>,
    pub active: Vec<bool>,
    pub selected: Vec<bool>,
    pub modified: bool,
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
        Ok(Doc { path: path.to_path_buf(), raw, specs, active: vec![true; n], selected: vec![false; n], modified: false })
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
    use anisoft_core::ams::{build_record, Phase};
    use anisoft_core::tensor::sym;

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
                let p = std::env::temp_dir().join(format!("anisoft-test-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')', ' '], ""));
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
