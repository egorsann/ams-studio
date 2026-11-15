//! The link between a specimen and its cube photo folder, kept in the `core_viewer` section of the specimen's
//! dependency file (`<specimen folder>_geo/<specimen folder>_geo_dependancy.json`, see `ams_core::depjson`): with the
//! data, so it is synced with them and not lost with the program's own settings.
//!
//! ```json
//! "core_viewer": { "photo_folder": "/data/core_samples_SCAN/core_scan/DC26AB002_stitch/DC26AB002_cube_crops",
//!                  "photo_folder_relative_to_this_file": "../../core_samples_SCAN/core_scan/DC26AB002_stitch/DC26AB002_cube_crops" }
//! ```
//!
//! The relative path is tried first: it keeps working when the whole data tree is somewhere else on another machine
//! (a synced folder under a different user name, Windows against Linux).

use ams_core::depjson;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Link {
    /// The photo folder as it was linked.
    pub dir: PathBuf,
    /// The photo folder relative to the folder of the dependency file, with `/` as separator (empty when there is none,
    /// for example on another drive).
    pub rel: String,
}

/// `to` relative to the folder `from` (both absolute): `../../x/y`. Empty when they have nothing in common.
fn relative_path(from: &Path, to: &Path) -> String {
    fn parts(p: &Path) -> Vec<Component<'_>> {
        p.components().filter(|c| !matches!(c, Component::CurDir)).collect()
    }
    let (a, b) = (parts(from), parts(to));
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    // sharing nothing but the root (or the drive) is no relation: only the absolute path then
    if !a.iter().zip(&b).take(common).any(|(x, _)| !matches!(x, Component::Prefix(_) | Component::RootDir)) {
        return String::new();
    }
    let mut out: Vec<String> = vec!["..".to_string(); a.len() - common];
    out.extend(b[common..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    out.join("/")
}

impl Link {
    /// A link from the dependency file in `json_dir` to the photo folder `dir`.
    pub fn new(dir: &Path, json_dir: &Path) -> Link {
        Link { dir: dir.to_path_buf(), rel: relative_path(json_dir, dir) }
    }

    /// The photo folder on this machine: the relative path from the dependency file, else the absolute one.
    pub fn resolve(&self, json_dir: &Path) -> Option<PathBuf> {
        if !self.rel.is_empty() {
            let p = self.rel.split('/').filter(|s| !s.is_empty()).fold(json_dir.to_path_buf(), |a, s| a.join(s));
            if p.is_dir() {
                return Some(p);
            }
        }
        self.dir.is_dir().then(|| self.dir.clone())
    }
}

/// The links of the first version, kept in the preferences folder (`core_links.json`): specimen id -> photo folder.
/// They are moved into the specimen's own dependency file when the specimen uses one.
pub(super) fn read_legacy() -> std::collections::BTreeMap<String, PathBuf> {
    let file = super::config_dir().join("ams-studio").join("core_links.json");
    let v: Value = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let mut out = std::collections::BTreeMap::new();
    if let Some(m) = v.get("links").and_then(|l| l.as_object()) {
        for (id, l) in m {
            if let Some(d) = l.get("dir").and_then(|x| x.as_str()) {
                out.insert(id.clone(), PathBuf::from(d));
            }
        }
    }
    out
}

/// The link stored in `json_path`, if any.
pub(super) fn read(json_path: &Path) -> Option<Link> {
    let v = depjson::read_section(json_path, "core_viewer")?;
    let dir = v.get("photo_folder").and_then(|x| x.as_str())?;
    let rel = v.get("photo_folder_relative_to_this_file").and_then(|x| x.as_str()).unwrap_or("").to_string();
    Some(Link { dir: PathBuf::from(dir), rel })
}

/// Stores (or, with `None`, removes) the link in `json_path`, making the `_geo` folder and the file when needed.
pub(super) fn write(json_path: &Path, link: Option<&Link>) -> std::io::Result<()> {
    let v: Option<Value> = link.map(|l| json!({ "photo_folder": l.dir.display().to_string(), "photo_folder_relative_to_this_file": l.rel }));
    depjson::write_section(json_path, "core_viewer", v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_is_stored_in_the_dependency_file_and_survives_moving_the_whole_tree() {
        let base = std::env::temp_dir().join(format!("cv-links-test-{}", std::process::id()));
        let geo = base.join("AMS").join("DC26AB002_AMS").join("DC26AB002_AMS_geo");
        let photos = base.join("SCAN").join("DC26AB002_stitch").join("DC26AB002_cube_crops");
        std::fs::create_dir_all(&photos).unwrap();
        let src = base.join("AMS").join("DC26AB002_AMS").join("DC26AB002_use.ams");
        let json = depjson::for_source(&src);
        assert_eq!(json.parent().unwrap(), geo);
        assert!(!geo.exists(), "nothing is made before a link is stored");
        let l = Link::new(&photos, &geo);
        assert_eq!(l.rel, "../../../SCAN/DC26AB002_stitch/DC26AB002_cube_crops");
        write(&json, Some(&l)).unwrap();
        assert_eq!(read(&json), Some(l.clone()));
        assert_eq!(l.resolve(&geo), Some(geo.join("../../../SCAN/DC26AB002_stitch/DC26AB002_cube_crops")));
        // the whole tree moves: the absolute path is gone, the relative one still finds the photos
        let moved = std::env::temp_dir().join(format!("cv-links-moved-{}", std::process::id()));
        std::fs::rename(&base, &moved).unwrap();
        let geo2 = moved.join("AMS").join("DC26AB002_AMS").join("DC26AB002_AMS_geo");
        let l2 = read(&geo2.join("DC26AB002_AMS_geo_dependancy.json")).unwrap();
        assert!(l2.resolve(&geo2).unwrap().is_dir() && !l2.dir.is_dir());
        write(&geo2.join("DC26AB002_AMS_geo_dependancy.json"), None).unwrap();
        assert!(!geo2.join("DC26AB002_AMS_geo_dependancy.json").exists());
        let _ = std::fs::remove_dir_all(&moved);
    }

    #[test]
    fn relative_paths_climb_out_of_the_common_part() {
        assert_eq!(relative_path(Path::new("/a/b/c"), Path::new("/a/x/y")), "../../x/y");
        assert_eq!(relative_path(Path::new("/a/b"), Path::new("/a/b/c")), "c");
        assert_eq!(relative_path(Path::new("/tmp/x"), Path::new("/home/y")), "");
    }
}
