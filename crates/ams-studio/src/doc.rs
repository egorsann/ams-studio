//! The open document: specimens, which of them are active/selected, and folder navigation.

use ams_core::ams::{self, Axes, Specimen, NAME_LEN, OFF_NAME, RECORD_LEN};
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
                // geographic coordinates come from the reorientation stored in the record (AMS Studio's block, exact),
                // else from the orientation Anisoft stores (Edit Data), else from the global parameters
                let r = match (cs, &s.georot, &s.orient) {
                    (CoordSys::Geographic, Some(g), _) => g.matrix(),
                    (CoordSys::Geographic, None, Some(ro)) => ams_core::orient::geo_rotation(ro),
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

    /// Stores the reorientation of record `i` (or clears it): the specimen coordinates stay exactly as measured, the
    /// geographic view is computed from the stored values. `modified` is set unless `quiet` (a reorientation that was
    /// only found in the `_geo` partner file is not an edit).
    pub fn set_georot(&mut self, i: usize, g: Option<ams_core::georot::GeoRot>, quiet: bool) {
        if i >= self.specs.len() {
            return;
        }
        ams_core::georot::write(&mut self.raw[i * RECORD_LEN..(i + 1) * RECORD_LEN], g.as_ref());
        self.specs[i].georot = g;
        if !quiet {
            self.modified = true;
        }
    }

    /// Takes the stored reorientations of the `_geo` partner file over (records are matched by name, in order; a
    /// record that has its own keeps it). Not an edit: the file is not marked modified. Returns how many were taken.
    pub fn apply_partner(&mut self, partner: &[u8]) -> usize {
        let Ok(theirs) = ams::parse(partner) else { return 0 };
        let mut used = vec![false; self.specs.len()];
        let mut n = 0;
        for t in &theirs {
            let Some(i) = (0..self.specs.len()).find(|&i| !used[i] && self.specs[i].name == t.name) else { continue };
            used[i] = true;
            if let (Some(g), None) = (t.georot, self.specs[i].georot) {
                self.set_georot(i, Some(g), true);
                n += 1;
            }
        }
        n
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

/// `<folder>/<file name without .ams>.owens.json`: written by an earlier version (which rotated the tensors itself) to
/// say which records were Owens-rotated; still read so that such files are not rotated twice.
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
    fn legacy_owens_marks_are_still_read() {
        let (_d, p) = tmp_doc();
        assert!(read_owens_marks(&p).is_empty());
        std::fs::write(owens_json_path(&p), "{\n  \"rotated\": [1, 0]\n}\n").unwrap();
        assert_eq!(read_owens_marks(&p), vec![true, false]);
        assert!(owens_json_path(&p).ends_with("ZZ01.owens.json"));
    }

    #[test]
    fn dates() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
    }

    #[test]
    fn the_reorientation_is_stored_and_the_specimen_coordinates_stay() {
        let (_d, p) = tmp_doc();
        let mut doc = Doc::open(&p).unwrap();
        let before = doc.raw.clone();
        let g = ams_core::georot::GeoRot::Owens { fs: 30.0, fd: 20.0, ds: 10.0, dd: 15.0 };
        doc.set_georot(1, Some(g), false);
        assert!(doc.modified);
        // only the trailing block of record 1 changed: tensors, Km and everything else are as measured
        let (a, b) = (&before, &doc.raw);
        assert_eq!(&a[..640 + ams_core::georot::OFF_GEOROT], &b[..640 + ams_core::georot::OFF_GEOROT]);
        assert_eq!(&a[..640], &b[..640]);
        let again = ams::parse(&doc.raw).unwrap();
        assert!(again[0].georot.is_none() && again[1].georot == Some(g));
        // the specimen view is untouched; the geographic view is rotated for record 1 only
        let s = doc.derive(false, CoordSys::Specimen, &Orientation::default());
        let ge = doc.derive(false, CoordSys::Geographic, &Orientation::default());
        assert!((s[0].axes.dec_inc[0].0 - ge[0].axes.dec_inc[0].0).abs() < 1e-9);
        assert!((s[1].axes.dec_inc[0].0 - ge[1].axes.dec_inc[0].0).abs() > 1.0);
        // eigenvalues survive a rotation
        assert!((s[1].axes.k_raw[0] - ge[1].axes.k_raw[0]).abs() < 1e-9);
        // clearing it restores the bytes
        doc.set_georot(1, None, false);
        assert_eq!(doc.raw, before);
    }

    #[test]
    fn the_geo_partner_gives_the_view_without_making_an_edit() {
        let (_d, p) = tmp_doc();
        let mut src = Doc::open(&p).unwrap();
        let g = ams_core::georot::GeoRot::Core(ams_core::core::CoreSetup { dip_dir: 305.0, plunge: 69.0, offset: 45.0, ..Default::default() });
        // the partner is a copy of the source with the block in record 1 only (names are matched, as when records were disregarded)
        src.set_georot(1, Some(g), false);
        let partner = src.raw.clone();
        let mut fresh = Doc::open(&p).unwrap();
        assert!(fresh.specs.iter().all(|s| s.georot.is_none()));
        assert_eq!(fresh.apply_partner(&partner), 1);
        assert!(!fresh.modified, "taking the partner over is not an edit");
        assert_eq!(fresh.specs[1].georot, Some(g));
        // the specimen coordinates of the opened file are still its own; only the geographic view changed
        let s = fresh.derive(false, CoordSys::Specimen, &Orientation::default());
        let ge = fresh.derive(false, CoordSys::Geographic, &Orientation::default());
        assert!((s[1].axes.dec_inc[0].0 - ge[1].axes.dec_inc[0].0).abs() > 1.0);
        // a second take-over changes nothing (the record already has its reorientation)
        assert_eq!(fresh.apply_partner(&partner), 0);
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
