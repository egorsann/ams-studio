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
//! * `stereonet`: planes drawn as great circles in the geographic stereonet, `planes_specimen` (only for this specimen)
//!   and `planes_global` (shown for every specimen; a copy is kept in the file of each specimen of the file tree).

use crate::core::{Arrow, CoreSetup, Face};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// What the file says about itself (the `note` key).
const NOTE: &str = "AMS Studio dependency file of this _geo folder; keep it with the _geo files. files: the drill core reorientation (dip direction and plunge of the hole, the offset of the orientation line from the arrow mark, the face that holds the arrow and its direction) of every record of the .ams files here, which stay exact copies of their sources; the geographic view is computed from it. core_viewer: the folder with the cube photos of this specimen (photo_folder is absolute, photo_folder_relative_to_this_file still works when the data tree is synced elsewhere). stereonet: planes drawn as great circles in the geographic stereonet (dip direction and dip in degrees, a comment, a colour): planes_specimen are for this specimen only, planes_global are shown for every specimen and copied into the file of each specimen of the file tree.";
/// The sections other than `files`.
const SECTIONS: [&str; 2] = ["core_viewer", "stereonet"];

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
}
