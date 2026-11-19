//! The dependency file of a `_geo` folder: `<specimen folder>_geo_dependancy.json`.
//!
//! The drill core reorientation (survey, the offset of the orientation line of every record, which face holds the
//! arrow and which way it points) does not fit into the `.ams` record: Anisoft's own fields take the arrow positions
//! only in steps of 30 degrees. It is kept in this file in the destination folder, next to the `_geo` `.ams` files
//! it belongs to, which stay exact copies of their source. The geographic view is computed from it, so the file has
//! to stay with the `_geo` files (the program makes both, they keep their relative place when a parent folder moves).
//!
//! Layout (one entry per `.ams` file of the folder, so several files can share it):
//!
//! ```json
//! { "kind": "AMS Studio drill core reorientation", "version": 1, "folder": "DC26AB002_AMS_geo",
//!   "files": { "DC26AB002_use_geo.ams": { "records": [
//!       { "index": 0, "name": "C1", "dip_direction": 316.135, "plunge": 67.0, "offset": 356.0,
//!         "face": "Up-hole facing", "arrow": "Right" }, null, ... ] } } }
//! ```
//!
//! A record without a drill core reorientation is `null`.
//!
//! The same file also holds the other things that belong to the specimen and should travel with the data (one file per
//! `_geo` folder, in the synced data tree, not in the program's preferences):
//!
//! * `core_viewer`: the folder with the cube photos of the specimen (absolute path, and the path relative to this file,
//!   which still works when the whole data tree is synced to another machine);
//! * `location`: where the specimen was taken (x, y and the coordinate system as typed or read from a datasheet, and the
//!   longitude / latitude computed from them), used by the GIS viewer;
//! * `stereonet`: planes drawn as great circles in the geographic stereonet, `planes_specimen` (only for this specimen)
//!   and `planes_global` (shown for every specimen; a copy is kept in the file of each specimen of the file tree).

use crate::core::{Arrow, CoreSetup, Face};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// What the file says about itself (the `note` key).
const NOTE: &str = "AMS Studio dependency file of this _geo folder; keep it with the _geo files. files: the drill core reorientation (dip direction and plunge of the hole, the offset of the orientation line from the arrow mark, the face that holds the arrow and its direction) of every record of the .ams files here, which stay exact copies of their sources; the geographic view is computed from it. core_viewer: the folder with the cube photos of this specimen (photo_folder is absolute, photo_folder_relative_to_this_file still works when the data tree is synced elsewhere). stereonet: planes drawn as great circles in the geographic stereonet (dip direction and dip in degrees, a comment, a colour): planes_specimen are for this specimen only, planes_global are shown for every specimen and copied into the file of each specimen of the file tree. location: where this specimen was taken (x, y and crs as typed or taken from a datasheet, lon and lat computed from them), used by the GIS viewer. view: the coordinate system (specimen, geographic, paleo, tectonic) this specimen was last shown in, restored when its file is opened. specimens: when this folder holds the files of several specimens (told apart by the specimen ID in the file names), location and view are kept per specimen ID in here ({ID: {location, view}}) instead of at the top level, so that each specimen has its own.";
/// The sections other than `files`.
const SECTIONS: [&str; 5] = ["core_viewer", "stereonet", "location", "view", "specimens"];

/// `<specimen folder>_geo/<specimen folder>_geo_dependancy.json` for any file of a specimen: the source file in the
/// specimen folder, or its copy in the `_geo` folder.
pub fn for_source(file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("."));
    let leaf = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if crate::specset::is_result_folder(&leaf) {
        return path_for(file);
    }
    path_for(&dir.join(crate::specset::geo_folder_name(&leaf)).join("x.ams"))
}

fn stamp(v: &mut Value, json_path: &Path) {
    let folder = json_path.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    v["kind"] = json!("AMS Studio dependency file");
    v["version"] = json!(1);
    v["folder"] = json!(folder);
    v["note"] = json!(NOTE);
}

fn is_empty(v: &Value) -> bool {
    v.get("files").and_then(|f| f.as_object()).map_or(true, |f| f.is_empty()) && SECTIONS.iter().all(|k| v.get(*k).map_or(true, |x| x.is_null()))
}

/// One section of the dependency file (`core_viewer`, `stereonet`), `None` when the file or the section is missing.
pub fn read_section(json_path: &Path, key: &str) -> Option<Value> {
    load(json_path).get(key).cloned().filter(|x| !x.is_null())
}

/// Sets or removes (`None`) a section; the other sections stay. The folder and the file are made when needed, and the
/// file is removed again when nothing is left in it. Removing from a file that does not exist does nothing.
pub fn write_section(json_path: &Path, key: &str, val: Option<Value>) -> std::io::Result<()> {
    if val.is_none() && !json_path.is_file() {
        return Ok(());
    }
    let mut v = load(json_path);
    match val {
        Some(x) => v[key] = x,
        None => {
            if let Some(o) = v.as_object_mut() {
                o.remove(key);
            }
        }
    }
    if is_empty(&v) {
        let _ = std::fs::remove_file(json_path);
        return Ok(());
    }
    stamp(&mut v, json_path);
    if !v.get("files").map_or(false, |f| f.is_object()) {
        v["files"] = json!({});
    }
    if let Some(d) = json_path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(json_path, serde_json::to_string_pretty(&v).unwrap_or_default() + "\n")
}

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

type DirCache = HashMap<PathBuf, (Instant, Vec<String>)>;
static DIR_CACHE: Mutex<Option<DirCache>> = Mutex::new(None);

/// File stems (without a trailing `_geo`) of the `.ams` / `.ran` files of a folder, remembered for a few seconds (the
/// GIS pane asks for every file of the tree).
fn data_stems(dir: &Path) -> Vec<String> {
    if let Ok(mut g) = DIR_CACHE.lock() {
        let c = g.get_or_insert_with(HashMap::new);
        if let Some((t, v)) = c.get(dir) {
            if t.elapsed() < Duration::from_secs(3) {
                return v.clone();
            }
        }
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().is_file())
                    .filter_map(|e| {
                        let n = e.file_name().to_string_lossy().to_string();
                        let (st, ext) = n.rsplit_once('.')?;
                        (ext.eq_ignore_ascii_case("ams") || ext.eq_ignore_ascii_case("ran")).then(|| st.strip_suffix("_geo").unwrap_or(st).to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v.dedup();
        c.insert(dir.to_path_buf(), (Instant::now(), v.clone()));
        return v;
    }
    vec![]
}

/// The specimen ID of a file when its folder holds the files of several specimens (two or more different IDs by the
/// rules of `specset::detect_scheme`: a 7-digit number, another digit count or the first part of the name), else `None`:
/// one specimen per folder, whose location and view are the folder's. A copy in a `_geo` folder gets the ID of its source.
pub fn specimen_key(file: &Path) -> Option<String> {
    let dir = file.parent()?;
    let leaf = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let source_dir = if crate::specset::is_result_folder(&leaf) { dir.parent()? } else { dir };
    let stems = data_stems(source_dir);
    // the open set's own choice (one specimen per folder, or an ID scheme picked by hand) wins over the guess
    let sc = match crate::specset::forced_for(source_dir) {
        Some((crate::specset::Layout::OwnFolders, _)) => return None,
        Some((_, Some(sc))) => sc,
        _ => crate::specset::detect_scheme(&stems)?,
    };
    let stem = file.file_stem()?.to_string_lossy().to_string();
    sc.id_of(stem.strip_suffix("_geo").unwrap_or(&stem))
}

/// A section for one file: the folder's section, or, in a folder with several specimens, the section of the specimen
/// this file belongs to (kept under `specimens`).
pub fn read_section_for(file: &Path, key: &str) -> Option<Value> {
    let json = for_source(file);
    match specimen_key(file) {
        None => read_section(&json, key),
        Some(id) => read_section(&json, "specimens")?.get(&id)?.get(key).cloned().filter(|x| !x.is_null()),
    }
}

/// Sets or removes (`None`) a section for one file (see `read_section_for`); the other specimens' sections stay.
pub fn write_section_for(file: &Path, key: &str, val: Option<Value>) -> std::io::Result<()> {
    let json = for_source(file);
    let Some(id) = specimen_key(file) else { return write_section(&json, key, val) };
    let mut all = read_section(&json, "specimens").filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
    let mut mine = all.get(&id).cloned().filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
    match val {
        Some(v) => mine[key] = v,
        None => {
            if let Some(o) = mine.as_object_mut() {
                o.remove(key);
            }
        }
    }
    if mine.as_object().map_or(true, |o| o.is_empty()) {
        if let Some(o) = all.as_object_mut() {
            o.remove(&id);
        }
    } else {
        all[&id] = mine;
    }
    let empty = all.as_object().map_or(true, |o| o.is_empty());
    write_section(&json, "specimens", if empty { None } else { Some(all) })
}

/// The coordinate system this file was last shown in (per specimen where a folder holds several).
pub fn read_view_for(file: &Path) -> Option<crate::coords::CoordSys> {
    read_section_for(file, "view")?.get("coordinates")?.as_str().and_then(crate::coords::CoordSys::from_token)
}

pub fn write_view_for(file: &Path, cs: crate::coords::CoordSys) -> std::io::Result<()> {
    if read_view_for(file) == Some(cs) {
        return Ok(());
    }
    write_section_for(file, "view", Some(json!({ "coordinates": cs.token() })))
}

/// The coordinate system the specimen was last shown in (`view` section), if one was stored.
pub fn read_view(json_path: &Path) -> Option<crate::coords::CoordSys> {
    read_section(json_path, "view")?.get("coordinates")?.as_str().and_then(crate::coords::CoordSys::from_token)
}

/// Stores the coordinate system the specimen is shown in.
pub fn write_view(json_path: &Path, cs: crate::coords::CoordSys) -> std::io::Result<()> {
    if read_view(json_path) == Some(cs) {
        return Ok(());
    }
    write_section(json_path, "view", Some(json!({ "coordinates": cs.token() })))
}

/// `<dir>/<dir name>_dependancy.json` for any `.ams` file of a `_geo` folder (`DC26AB002_AMS_geo/DC26AB002_AMS_geo_dependancy.json`).
pub fn path_for(ams_file: &Path) -> PathBuf {
    let dir = ams_file.parent().unwrap_or(Path::new("."));
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "_geo".to_string());
    dir.join(format!("{name}_dependancy.json"))
}

fn load(json_path: &Path) -> Value {
    std::fs::read_to_string(json_path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(|v| v.is_object()).unwrap_or_else(|| json!({}))
}

fn setup_json(index: usize, name: &str, s: &CoreSetup) -> Value {
    json!({ "index": index, "name": name, "dip_direction": s.dip_dir, "plunge": s.plunge, "offset": s.offset, "face": s.face.name(), "arrow": s.arrow.name() })
}

/// The drill core reorientation of every record of `ams_file` (aligned with its records), empty when there is none.
pub fn read(ams_file: &Path) -> Vec<Option<CoreSetup>> {
    let file = ams_file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let v = load(&path_for(ams_file));
    let Some(recs) = v.get("files").and_then(|f| f.get(&file)).and_then(|f| f.get("records")).and_then(|r| r.as_array()) else { return vec![] };
    recs.iter()
        .map(|r| {
            let r = r.as_object()?;
            let num = |k: &str| r.get(k).and_then(|x| x.as_f64());
            let face = *Face::ALL.iter().find(|f| Some(f.name()) == r.get("face").and_then(|x| x.as_str()))?;
            let arrow = *Arrow::ALL.iter().find(|a| Some(a.name()) == r.get("arrow").and_then(|x| x.as_str()))?;
            Some(CoreSetup { dip_dir: num("dip_direction")?, plunge: num("plunge")?, offset: num("offset")?, face, arrow })
        })
        .collect()
}

/// The direction of the hole from a drill core reorientation already made for this `_geo` folder: (dip direction,
/// plunge) of the first record that has one, in any `.ams` file of the folder. `None` when there is none. Used to
/// offer the hole direction when the position of a specimen is entered.
pub fn read_hole(json_path: &Path) -> Option<(f64, f64)> {
    let v = load(json_path);
    let files = v.get("files")?.as_object()?;
    // sorted by file name so the answer does not depend on the order of the map
    let mut names: Vec<&String> = files.keys().collect();
    names.sort();
    names.into_iter().find_map(|n| {
        files[n].get("records")?.as_array()?.iter().find_map(|r| {
            let r = r.as_object()?;
            Some((r.get("dip_direction")?.as_f64()?, r.get("plunge")?.as_f64()?))
        })
    })
}

/// Writes (or replaces) the entry of `ams_file` in the dependency file of its folder; entries of other files stay.
/// `records` holds, aligned with the records of the file, the names and their drill core reorientation. With none at
/// all the entry is removed (and the file when it is empty). Returns the path of the dependency file.
pub fn write(ams_file: &Path, records: &[(String, Option<CoreSetup>)]) -> std::io::Result<PathBuf> {
    let jp = path_for(ams_file);
    let file = ams_file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut v = load(&jp);
    stamp(&mut v, &jp);
    if !v.get("files").map_or(false, |f| f.is_object()) {
        v["files"] = json!({});
    }
    if records.iter().any(|(_, s)| s.is_some()) {
        let list: Vec<Value> = records.iter().enumerate().map(|(i, (n, s))| s.as_ref().map_or(Value::Null, |s| setup_json(i, n, s))).collect();
        v["files"][&file] = json!({ "records": list });
    } else if let Some(f) = v["files"].as_object_mut() {
        f.remove(&file);
    }
    if is_empty(&v) {
        let _ = std::fs::remove_file(&jp);
        return Ok(jp);
    }
    std::fs::write(&jp, serde_json::to_string_pretty(&v).unwrap_or_default() + "\n")?;
    Ok(jp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hole_direction_of_an_earlier_reorientation_can_be_found() {
        let dir = std::env::temp_dir().join(format!("depjson-hole-{}", std::process::id()));
        let ams = dir.join("ZZ_AMS_geo").join("ZZ_use_geo.ams");
        let jp = path_for(&ams);
        assert_eq!(read_hole(&jp), None);
        // no reorientation in the records: nothing to find
        write_section(&jp, "location", Some(serde_json::json!({"x": 1.0}))).unwrap();
        assert_eq!(read_hole(&jp), None);
        let a = CoreSetup { dip_dir: 316.135, plunge: 67.0, offset: 356.0, face: Face::UpHole, arrow: Arrow::Right };
        write(&ams, &[("A1".to_string(), None), ("A2".to_string(), Some(a))]).unwrap();
        assert_eq!(read_hole(&jp), Some((316.135, 67.0)));
        // the location section written first stays
        assert!(read_section(&jp, "location").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_view_of_a_specimen_is_remembered_and_leaves_the_other_sections_alone() {
        use crate::coords::CoordSys;
        let dir = std::env::temp_dir().join(format!("depjson-view-{}", std::process::id()));
        let jp = path_for(&dir.join("ZZ_AMS_geo").join("ZZ_use_geo.ams"));
        assert_eq!(read_view(&jp), None);
        write_section(&jp, "location", Some(serde_json::json!({"x": 1.0}))).unwrap();
        for cs in [CoordSys::Geographic, CoordSys::Tectonic, CoordSys::Specimen] {
            write_view(&jp, cs).unwrap();
            assert_eq!(read_view(&jp), Some(cs));
        }
        assert!(read_section(&jp, "location").is_some());
        // a reorientation written afterwards keeps it
        let a = CoreSetup { dip_dir: 316.135, plunge: 67.0, offset: 356.0, face: Face::UpHole, arrow: Arrow::Right };
        write(&dir.join("ZZ_AMS_geo").join("ZZ_use_geo.ams"), &[("A1".to_string(), Some(a))]).unwrap();
        assert_eq!(read_view(&jp), Some(CoordSys::Specimen));
        assert_eq!(CoordSys::from_token("paleo"), Some(CoordSys::Paleo));
        assert_eq!(CoordSys::from_token("x"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_is_named_after_the_geo_folder_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("depjson-test-{}", std::process::id())).join("DC26AB002_AMS_geo");
        std::fs::create_dir_all(&dir).unwrap();
        let ams = dir.join("DC26AB002_use_geo.ams");
        assert!(path_for(&ams).ends_with("DC26AB002_AMS_geo_dependancy.json"));
        assert!(read(&ams).is_empty());
        let a = CoreSetup { dip_dir: 316.135, plunge: 67.0, offset: 356.0, face: Face::Left, arrow: Arrow::Down };
        write(&ams, &[("C1".into(), Some(a)), ("D!8".into(), None)]).unwrap();
        let back = read(&ams);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0], Some(a));
        assert!(back[1].is_none());
        // a second file in the same folder keeps the first one's entry
        let other = dir.join("other_geo.ams");
        write(&other, &[("A1".into(), Some(a))]).unwrap();
        assert_eq!(read(&ams)[0], Some(a));
        assert_eq!(read(&other)[0], Some(a));
        // nothing left: the entry goes, and the file with the last one
        write(&ams, &[("C1".into(), None)]).unwrap();
        assert!(read(&ams).is_empty() && read(&other).len() == 1);
        write(&other, &[("A1".into(), None)]).unwrap();
        assert!(!path_for(&ams).exists());
        // other sections share the file: the reorientation going away keeps them, removing the last one removes the file
        write_section(&path_for(&ams), "core_viewer", Some(json!({ "photo_folder": "/x" }))).unwrap();
        write(&ams, &[("C1".into(), Some(a))]).unwrap();
        write(&ams, &[("C1".into(), None)]).unwrap();
        assert_eq!(read_section(&path_for(&ams), "core_viewer").unwrap()["photo_folder"], "/x");
        write_section(&path_for(&ams), "stereonet", None).unwrap();
        write_section(&path_for(&ams), "core_viewer", None).unwrap();
        assert!(!path_for(&ams).exists());
        // the dependency file of a specimen, from its source file or from its _geo copy
        assert_eq!(for_source(Path::new("/d/DC26AB002_AMS/DC26AB002_use.ams")), PathBuf::from("/d/DC26AB002_AMS/DC26AB002_AMS_geo/DC26AB002_AMS_geo_dependancy.json"));
        assert_eq!(for_source(Path::new("/d/DC26AB002_AMS/DC26AB002_AMS_geo/DC26AB002_use_geo.ams")), PathBuf::from("/d/DC26AB002_AMS/DC26AB002_AMS_geo/DC26AB002_AMS_geo_dependancy.json"));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn specimens_that_share_a_folder_keep_their_own_location_and_view() {
        let dir = std::env::temp_dir().join(format!("depjson-shared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["1234567_use.ams", "1234567_raw.ams", "1234568_use.ams", "1234569.ams"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let (a, b, c) = (dir.join("1234567_use.ams"), dir.join("1234568_use.ams"), dir.join("1234569.ams"));
        assert_eq!(specimen_key(&a).as_deref(), Some("1234567"));
        assert_eq!(specimen_key(&dir.join("1234567_raw.ams")).as_deref(), Some("1234567"), "the files of one specimen share it");
        write_section_for(&a, "location", Some(json!({"x": 1.0}))).unwrap();
        write_section_for(&b, "location", Some(json!({"x": 2.0}))).unwrap();
        write_view_for(&a, crate::coords::CoordSys::Geographic).unwrap();
        assert_eq!(read_section_for(&a, "location").unwrap()["x"], 1.0);
        assert_eq!(read_section_for(&b, "location").unwrap()["x"], 2.0);
        assert_eq!(read_section_for(&dir.join("1234567_raw.ams"), "location").unwrap()["x"], 1.0);
        assert!(read_section_for(&c, "location").is_none(), "a third specimen has none of the others'");
        assert_eq!(read_view_for(&a), Some(crate::coords::CoordSys::Geographic));
        assert_eq!(read_view_for(&b), None);
        // a copy in the _geo folder finds the same entries
        let leaf = dir.file_name().unwrap().to_string_lossy().to_string();
        let copy = dir.join(format!("{leaf}_geo")).join("1234567_use_geo.ams");
        assert_eq!(read_section_for(&copy, "location").unwrap()["x"], 1.0);
        // removing one leaves the other; removing both removes the file
        write_section_for(&a, "location", None).unwrap();
        assert!(read_section_for(&a, "location").is_none() && read_section_for(&b, "location").is_some());
        write_section_for(&b, "location", None).unwrap();
        write_section_for(&a, "view", None).unwrap();
        assert!(!for_source(&a).is_file(), "nothing left, no file");
        // one specimen per folder: the folder's own sections as before
        let single = std::env::temp_dir().join(format!("depjson-single-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&single);
        std::fs::create_dir_all(&single).unwrap();
        for f in ["ER01_use.ams", "ER01_old.ams"] {
            std::fs::write(single.join(f), b"").unwrap();
        }
        assert_eq!(specimen_key(&single.join("ER01_use.ams")), None);
        write_section_for(&single.join("ER01_use.ams"), "location", Some(json!({"x": 5.0}))).unwrap();
        assert_eq!(read_section(&for_source(&single.join("ER01_old.ams")), "location").unwrap()["x"], 5.0);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&single);
    }

    #[test]
    fn specimens_in_one_folder_without_a_detectable_id_keep_separate_places() {
        let dir = std::env::temp_dir().join(format!("depjson-lastnum-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["PB-1.ams", "PB-2.ams", "PB-3.ams"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let (a, b) = (dir.join("PB-1.ams"), dir.join("PB-2.ams"));
        assert_eq!(specimen_key(&a).as_deref(), Some("PB-1"));
        write_section_for(&a, "location", Some(json!({"x": 1.0}))).unwrap();
        write_section_for(&b, "location", Some(json!({"x": 2.0}))).unwrap();
        assert_eq!(read_section_for(&a, "location").unwrap()["x"], 1.0);
        assert_eq!(read_section_for(&b, "location").unwrap()["x"], 2.0);
        // a scheme picked by hand for the open set is the one that tells the specimens apart
        crate::specset::set_forced(Some((dir.clone(), crate::specset::Layout::OneFolder, Some(crate::specset::IdScheme::Chars(4)))));
        assert_eq!(specimen_key(&a).as_deref(), Some("PB-1"));
        crate::specset::set_forced(Some((dir.clone(), crate::specset::Layout::OneFolder, Some(crate::specset::IdScheme::Chars(2)))));
        assert_eq!(specimen_key(&a).as_deref(), Some("PB"));
        crate::specset::set_forced(Some((dir.clone(), crate::specset::Layout::OwnFolders, None)));
        assert_eq!(specimen_key(&a), None);
        crate::specset::set_forced(None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
