//! The links between specimens and their cube photo folders, kept in `core_links.json` in the program's preferences
//! folder (next to `state.txt`). They are not kept in the data folders: the raw `.ams` and photo folders stay
//! untouched, and the file travels with the user's settings, not with the data.
//!
//! ```json
//! { "kind": "AMS Studio cube photo links", "version": 1,
//!   "links": { "DC26AB002": { "dir": "/data/core_scan/DC26AB002_stitch/DC26AB002_cube_crops",
//!                             "root": "/data", "rel": "core_scan/DC26AB002_stitch/DC26AB002_cube_crops" } } }
//! ```
//!
//! `dir` is tried first. When the data live somewhere else on another machine (a synced folder under a different
//! user name, Windows against Linux), `rel` is joined to the folder the user picked for the search on this machine.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Link {
    pub dir: PathBuf,
    pub root: PathBuf,
    /// The photo folder below `root`, with `/` as separator.
    pub rel: String,
}

impl Link {
    pub fn new(dir: &Path, root: Option<&Path>) -> Link {
        let root = root.filter(|r| dir.starts_with(r)).map(|r| r.to_path_buf()).unwrap_or_else(|| dir.parent().map(|p| p.to_path_buf()).unwrap_or_default());
        let rel = dir.strip_prefix(&root).map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/")).unwrap_or_default();
        Link { dir: dir.to_path_buf(), root, rel }
    }

    /// The photo folder on this machine: the saved path, else the saved relative path below the folder searched now.
    pub fn resolve(&self, search_root: Option<&Path>) -> Option<PathBuf> {
        if self.dir.is_dir() {
            return Some(self.dir.clone());
        }
        let below = |r: &Path| -> Option<PathBuf> {
            let p = self.rel.split('/').filter(|s| !s.is_empty()).fold(r.to_path_buf(), |a, s| a.join(s));
            (!self.rel.is_empty() && p.is_dir()).then_some(p)
        };
        search_root.and_then(below).or_else(|| below(&self.root))
    }
}

pub(super) fn path() -> PathBuf {
    super::config_dir().join("ams-studio").join("core_links.json")
}

pub(super) fn read(file: &Path) -> BTreeMap<String, Link> {
    let v: Value = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let mut out = BTreeMap::new();
    if let Some(m) = v.get("links").and_then(|l| l.as_object()) {
        for (id, l) in m {
            let s = |k: &str| l.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
            if let Some(dir) = s("dir") {
                out.insert(id.clone(), Link { dir: PathBuf::from(dir), root: PathBuf::from(s("root").unwrap_or_default()), rel: s("rel").unwrap_or_default() });
            }
        }
    }
    out
}

pub(super) fn write(file: &Path, links: &BTreeMap<String, Link>) -> std::io::Result<()> {
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    let m: serde_json::Map<String, Value> = links.iter().map(|(id, l)| (id.clone(), json!({ "dir": l.dir.display().to_string(), "root": l.root.display().to_string(), "rel": l.rel }))).collect();
    let v = json!({ "kind": "AMS Studio cube photo links", "version": 1, "links": m });
    std::fs::write(file, serde_json::to_string_pretty(&v).unwrap_or_default() + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_round_trip_and_resolve_below_another_root() {
        let base = std::env::temp_dir().join(format!("cv-links-test-{}", std::process::id()));
        let photos = base.join("scan").join("DC26AB002_stitch").join("DC26AB002_cube_crops");
        std::fs::create_dir_all(&photos).unwrap();
        let l = Link::new(&photos, Some(&base.join("scan")));
        assert_eq!(l.rel, "DC26AB002_stitch/DC26AB002_cube_crops");
        let f = base.join("core_links.json");
        let mut m = BTreeMap::new();
        m.insert("DC26AB002".to_string(), l.clone());
        write(&f, &m).unwrap();
        assert_eq!(read(&f), m);
        assert_eq!(l.resolve(None), Some(photos.clone()));
        // the data moved (another machine): the saved path is gone, the folder picked now has it below
        let gone = Link { dir: PathBuf::from("/nonexistent/x/DC26AB002_cube_crops"), root: PathBuf::from("/nonexistent/x"), rel: l.rel.clone() };
        assert_eq!(gone.resolve(Some(&base.join("scan"))), Some(photos));
        assert_eq!(gone.resolve(Some(&base)), None);
        assert_eq!(gone.resolve(None), None);
        let _ = std::fs::remove_dir_all(&base);
    }
}
