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

use crate::core::{Arrow, CoreSetup, Face};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

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
    let folder = ams_file.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    v["kind"] = json!("AMS Studio drill core reorientation");
    v["version"] = json!(1);
    v["folder"] = json!(folder);
    v["note"] = json!("Holds the drill core reorientation of the .ams files in this folder (dip direction and plunge of the hole, the offset of the orientation line from the arrow mark, the face that holds the arrow and its direction). The .ams files here are exact copies of their sources; the geographic view is computed from this file, so keep it with them.");
    if !v.get("files").map_or(false, |f| f.is_object()) {
        v["files"] = json!({});
    }
    if records.iter().any(|(_, s)| s.is_some()) {
        let list: Vec<Value> = records.iter().enumerate().map(|(i, (n, s))| s.as_ref().map_or(Value::Null, |s| setup_json(i, n, s))).collect();
        v["files"][&file] = json!({ "records": list });
    } else if let Some(f) = v["files"].as_object_mut() {
        f.remove(&file);
    }
    if v["files"].as_object().map_or(true, |f| f.is_empty()) {
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
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
