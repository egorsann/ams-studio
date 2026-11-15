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

    /// Writes the specimen volume (cm3) into the records `rows` (the field Anisoft's Edit Data shows as Volume).
    /// Only the records `rows` (0-based, end excluded) when the file does not have one volume throughout.
    pub fn set_volume_range(&mut self, rows: std::ops::Range<usize>, v: f64) {
        for i in rows.start..rows.end.min(self.specs.len()) {
            self.raw[i * RECORD_LEN + ams_core::ams::OFF_VOLUME..i * RECORD_LEN + ams_core::ams::OFF_VOLUME + 4].copy_from_slice(&(v as f32).to_le_bytes());
            self.specs[i].volume_cm3 = v as f32 as f64;
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

/// Sample volume information kept next to an `.ams` file (the volume itself is also in the records; the shape has
/// no place in a record).
#[derive(Debug, Clone, PartialEq)]
pub struct VolInfo {
    pub volume_cm3: f64,
    /// "cubic" or "cylindrical"
    pub shape: String,
    /// Date the correction was entered, `YYYY-MM-DD`.
    pub applied: String,
    /// Shape of each record when the records were not all given the same shape (empty = `shape` for all of them).
    pub rec_shapes: Vec<String>,
}

/// `<folder>/<file name without .ams>.volume.json`
pub fn volume_json_path(ams: &Path) -> PathBuf {
    let stem = ams.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    ams.with_file_name(format!("{stem}.volume.json"))
}

/// Today's date as `YYYY-MM-DD` (local time).
pub fn today() -> String {
    fmt_time(std::time::SystemTime::now())[..10].to_string()
}

/// `YYYY-MM-DD HH:MM` of a time, in the computer's local time zone.
pub fn fmt_time(t: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string()
}

pub fn write_volume_json(ams: &Path, info: &VolInfo, volumes: &[f64]) -> std::io::Result<PathBuf> {
    let p = volume_json_path(ams);
    let file = ams.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut text = format!(
        "{{\n  \"file\": \"{}\",\n  \"volume_cm3\": {},\n  \"shape\": \"{}\",\n  \"applied\": \"{}\",\n  \"records\": {}",
        esc(&file), info.volume_cm3, esc(&info.shape), esc(&info.applied), volumes.len()
    );
    // records that did not all get the same volume / shape are listed one by one (row 1 first)
    let (lo, hi) = volumes.iter().fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    if volumes.len() > 1 && hi - lo > 1e-6 {
        let v: Vec<String> = volumes.iter().map(|v| format!("{}", (v * 1e4).round() / 1e4)).collect();
        text += &format!(",\n  \"record_volumes_cm3\": [{}]", v.join(", "));
    }
    if !info.rec_shapes.is_empty() {
        let v: Vec<String> = info.rec_shapes.iter().map(|s| format!("\"{}\"", esc(s))).collect();
        text += &format!(",\n  \"record_shapes\": [{}]", v.join(", "));
    }
    text += "\n}\n";
    std::fs::write(&p, text)?;
    Ok(p)
}

pub fn read_volume_json(ams: &Path) -> Option<VolInfo> {
    let text = std::fs::read_to_string(volume_json_path(ams)).ok()?;
    let after = |key: &str| -> Option<String> {
        let i = text.find(&format!("\"{key}\""))?;
        let rest = text[i + key.len() + 2..].trim_start().strip_prefix(':')?.trim_start();
        if let Some(r) = rest.strip_prefix('"') {
            Some(r[..r.find('"')?].to_string())
        } else {
            Some(rest.chars().take_while(|c| !matches!(c, ',' | '}' | '\n')).collect::<String>().trim().to_string())
        }
    };
    let rec_shapes = text
        .find("\"record_shapes\"")
        .and_then(|i| {
            let r = &text[i..];
            let (a, b) = (r.find('[')?, r.find(']')?);
            Some(r[a + 1..b].split('"').skip(1).step_by(2).map(|s| s.to_string()).collect::<Vec<_>>())
        })
        .unwrap_or_default();
    Some(VolInfo { volume_cm3: after("volume_cm3")?.parse().ok()?, shape: after("shape").unwrap_or_default(), applied: after("applied").unwrap_or_default(), rec_shapes })
}

/// `<folder>/<file name without .ams>.owens.json`: which records of a file were already Owens-rotated.
pub fn owens_json_path(ams: &Path) -> PathBuf {
    let stem = ams.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    ams.with_file_name(format!("{stem}.owens.json"))
}

/// The records of `ams` that were Owens-rotated before (from its `.owens.json`; empty when there is none).
pub fn read_owens_marks(ams: &Path) -> Vec<bool> {
    let Ok(text) = std::fs::read_to_string(owens_json_path(ams)) else { return vec![] };
    let Some(i) = text.find("\"rotated\"") else { return vec![] };
    let r = &text[i..];
    let (Some(a), Some(b)) = (r.find('['), r.find(']')) else { return vec![] };
    r[a + 1..b].split(',').filter_map(|t| match t.trim() { "1" => Some(true), "0" => Some(false), _ => None }).collect()
}

/// Writes the marks next to the file (`rotated[i]` = record i was Owens-rotated). With no mark set the file is removed.
pub fn write_owens_marks(ams: &Path, rotated: &[bool]) -> std::io::Result<()> {
    let p = owens_json_path(ams);
    if !rotated.iter().any(|r| *r) {
        let _ = std::fs::remove_file(&p);
        return Ok(());
    }
    let file = ams.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default().replace('\\', "\\\\").replace('"', "\\\"");
    let list: Vec<&str> = rotated.iter().map(|r| if *r { "1" } else { "0" }).collect();
    std::fs::write(&p, format!("{{\n  \"file\": \"{file}\",\n  \"applied\": \"{}\",\n  \"records\": {},\n  \"rotated\": [{}]\n}}\n", today(), rotated.len(), list.join(", ")))
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
    fn volume_goes_into_the_records_and_the_json() {
        let (d, p) = tmp_doc();
        let mut doc = Doc::open(&p).unwrap();
        assert_eq!(doc.specs[0].volume_cm3, 10.0);
        doc.set_volume_range(0..2, 3.375);
        let again = ams::parse(&doc.raw).unwrap();
        assert!(again.iter().all(|s| (s.volume_cm3 - 3.375).abs() < 1e-6));
        let info = VolInfo { volume_cm3: 3.375, shape: "cubic".into(), applied: "2026-11-15".into(), rec_shapes: vec![] };
        let jp = write_volume_json(&p, &info, &[3.375, 3.375]).unwrap();
        assert!(jp.ends_with("ZZ01.volume.json"));
        assert_eq!(read_volume_json(&p), Some(info));
        assert!(read_volume_json(&d.0.join("nothing.ams")).is_none());
    }

    #[test]
    fn volume_for_some_rows_only() {
        let (_d, p) = tmp_doc();
        let mut doc = Doc::open(&p).unwrap();
        doc.set_volume_range(1..2, 8.0);
        assert_eq!(doc.specs[0].volume_cm3, 10.0);
        assert_eq!(doc.specs[1].volume_cm3, 8.0);
        let again = ams::parse(&doc.raw).unwrap();
        assert_eq!((again[0].volume_cm3, again[1].volume_cm3), (10.0, 8.0));
        // out-of-range ends are cut off
        doc.set_volume_range(0..99, 5.0);
        assert!(doc.specs.iter().all(|s| s.volume_cm3 == 5.0));
        // per-record json round trip
        let info = VolInfo { volume_cm3: 8.0, shape: "cubic".into(), applied: "2026-11-15".into(), rec_shapes: vec!["cylindrical".into(), "cubic".into()] };
        write_volume_json(&p, &info, &[10.0, 8.0]).unwrap();
        let text = std::fs::read_to_string(volume_json_path(&p)).unwrap();
        assert!(text.contains("record_volumes_cm3"));
        assert_eq!(read_volume_json(&p), Some(info));
    }

    #[test]
    fn owens_marks_round_trip() {
        let (_d, p) = tmp_doc();
        assert!(read_owens_marks(&p).is_empty());
        write_owens_marks(&p, &[true, false]).unwrap();
        assert_eq!(read_owens_marks(&p), vec![true, false]);
        assert!(owens_json_path(&p).ends_with("ZZ01.owens.json"));
        // nothing marked: the file goes away
        write_owens_marks(&p, &[false, false]).unwrap();
        assert!(!owens_json_path(&p).exists());
    }

    #[test]
    fn dates() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
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
