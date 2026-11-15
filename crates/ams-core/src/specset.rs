//! A parent folder with one sub-folder per specimen, each holding several `.ams` files.
//!
//! Naming differs between labs (the author's main files end in `_use`), so the identifier of the main file
//! per specimen is detected from the names: the string that picks out exactly one file in as many specimen
//! folders as possible. The user can confirm it, or try another string; [`matches`] says which file each
//! folder would give for a string.

use std::path::{Path, PathBuf};

/// A folder that holds `.ams` files (a specimen).
#[derive(Debug, Clone, PartialEq)]
pub struct SpecFolder {
    /// Folder name, relative to the parent (`A/B` when nested).
    pub name: String,
    pub dir: PathBuf,
    /// `.ams` file names, sorted without regard to case.
    pub files: Vec<String>,
}

const MAX_DEPTH: usize = 4;
const MAX_FOLDERS: usize = 20000;

/// Walks `root` (hidden folders skipped) and returns every folder that directly holds `.ams` files, sorted by name.
pub fn scan(root: &Path) -> Vec<SpecFolder> {
    let mut out = vec![];
    walk(root, root, 0, &mut out);
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<SpecFolder>) {
    if depth > MAX_DEPTH || out.len() > MAX_FOLDERS {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files = vec![];
    let mut subs = vec![];
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            // folders of exported results (File > Save as .ams, Drill Core to Geo) are not specimens
            if matches!(name.as_str(), "_geo" | "_raw" | "_corr") {
                continue;
            }
            subs.push(p);
        } else if p.extension().map_or(false, |x| x.eq_ignore_ascii_case("ams")) {
            files.push(name);
        }
    }
    if !files.is_empty() {
        files.sort_by_key(|f| f.to_lowercase());
        let name = if dir == root {
            "(this folder)".to_string()
        } else {
            dir.strip_prefix(root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default()
        };
        out.push(SpecFolder { name, dir: dir.to_path_buf(), files });
    }
    for s in subs {
        walk(root, &s, depth + 1, out);
    }
}

fn stem(f: &str) -> &str {
    f.rsplit_once('.').map_or(f, |(s, _)| s)
}

/// What a string gives for one folder.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    /// The folder's only `.ams` file (no string needed).
    Only(usize),
    /// Exactly one file contains the string.
    Match(usize),
    /// No file contains it.
    None,
    /// Several files contain it (their indices).
    Several(Vec<usize>),
}

impl Pick {
    /// The file chosen, if there is exactly one.
    pub fn file(&self) -> Option<usize> {
        match self {
            Pick::Only(i) | Pick::Match(i) => Some(*i),
            _ => None,
        }
    }
}

/// For every folder, which file `ident` (a case-insensitive part of the file name) picks.
/// A folder with a single `.ams` file always gives that file.
pub fn matches(folders: &[SpecFolder], ident: &str) -> Vec<Pick> {
    let needle = ident.trim().to_lowercase();
    folders
        .iter()
        .map(|f| {
            if f.files.len() == 1 {
                return Pick::Only(0);
            }
            if needle.is_empty() {
                return Pick::None;
            }
            let hit: Vec<usize> = f.files.iter().enumerate().filter(|(_, n)| stem(n).to_lowercase().contains(&needle)).map(|(i, _)| i).collect();
            match hit.len() {
                0 => Pick::None,
                1 => Pick::Match(hit[0]),
                _ => Pick::Several(hit),
            }
        })
        .collect()
}

/// Result of [`detect`].
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    /// The identifier found, `None` when no pattern recurs (or none is needed: see `needed`).
    pub ident: Option<String>,
    /// Folders with several `.ams` files, the ones where a string is needed.
    pub multi: usize,
    /// Of those, how many the identifier picks exactly one file in.
    pub covered: usize,
    /// Whether any folder needs an identifier at all.
    pub needed: bool,
}

/// Candidate strings from the file names: what follows the folder name (`_use` in `ER01/ER01_use.ams`) and every
/// name part after a separator, with and without the separator.
fn candidates(folders: &[SpecFolder]) -> Vec<String> {
    let mut c: Vec<String> = vec![];
    let mut add = |s: String| {
        let s = s.trim_matches(|ch: char| ch == '.' || ch == ' ').to_string();
        if s.chars().filter(|ch| ch.is_alphanumeric()).count() >= 2 && !c.contains(&s) {
            c.push(s);
        }
    };
    for f in folders.iter().filter(|f| f.files.len() > 1) {
        let leaf = f.name.rsplit('/').next().unwrap_or(&f.name).to_lowercase();
        for n in &f.files {
            let st = stem(n);
            let low = st.to_lowercase();
            if low.starts_with(&leaf) && st.len() > leaf.len() {
                add(st[leaf.len()..].to_string());
            }
            let mut start = 0;
            for (i, ch) in st.char_indices() {
                if matches!(ch, '_' | '-' | ' ') {
                    if i > start {
                        add(st[start..i].to_string());
                    }
                    start = i;
                    // the part from this separator on, with the separator
                    add(st[i..].to_string());
                    // and each single part with its separator
                    let end = st[i + 1..].find(|x: char| matches!(x, '_' | '-' | ' ')).map_or(st.len(), |e| i + 1 + e);
                    add(st[i..end].to_string());
                    add(st[i + 1..end].to_string());
                }
            }
        }
    }
    c
}

/// Finds the string that picks out exactly one file in most multi-file folders. `hint` (the string used last time)
/// wins when it works about as well as the best candidate.
pub fn detect(folders: &[SpecFolder], hint: Option<&str>) -> Detection {
    let multi = folders.iter().filter(|f| f.files.len() > 1).count();
    if multi == 0 {
        return Detection { ident: None, multi: 0, covered: 0, needed: false };
    }
    let score = |s: &str| matches(folders, s).iter().zip(folders).filter(|(p, f)| f.files.len() > 1 && matches!(p, Pick::Match(_))).count();
    let mut best: Option<(usize, usize, String)> = None; // covered, length, string
    for s in candidates(folders) {
        let cov = score(&s);
        let better = match &best {
            None => true,
            Some((bc, bl, _)) => cov > *bc || (cov == *bc && s.len() > *bl),
        };
        if cov > 0 && better {
            best = Some((cov, s.len(), s));
        }
    }
    if let (Some(h), Some((bc, _, _))) = (hint.map(str::trim).filter(|h| !h.is_empty()), &best) {
        let hc = score(h);
        if hc > 0 && hc + 1 >= *bc && hc * 10 >= multi * 6 {
            return Detection { ident: Some(h.to_string()), multi, covered: hc, needed: true };
        }
    }
    match best {
        // a pattern has to recur: at least two folders and 60 % of the multi-file ones
        Some((cov, _, s)) if cov >= 2 && cov * 10 >= multi * 6 => Detection { ident: Some(s), multi, covered: cov, needed: true },
        Some((cov, ..)) => Detection { ident: None, multi, covered: cov, needed: true },
        None => Detection { ident: None, multi, covered: 0, needed: true },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, files: &[&str]) -> SpecFolder {
        SpecFolder { name: name.into(), dir: PathBuf::from(name), files: files.iter().map(|s| s.to_string()).collect() }
    }

    fn typical() -> Vec<SpecFolder> {
        vec![
            folder("ZZ01", &["ZZ01.ams", "ZZ01_old.ams", "ZZ01_use.ams"]),
            folder("ZZ02", &["ZZ02_raw.ams", "ZZ02_use.ams"]),
            folder("ZZ03", &["ZZ03.ams", "ZZ03_use.AMS", "ZZ03_use_v2_backup.ams"][..2]),
            folder("ZZ04", &["ZZ04.ams"]),
        ]
    }

    #[test]
    fn finds_the_recurring_identifier() {
        let f = typical();
        let d = detect(&f, None);
        assert_eq!(d.ident.as_deref(), Some("_use"));
        assert_eq!((d.multi, d.covered, d.needed), (3, 3, true));
        let m = matches(&f, "_use");
        assert_eq!(m[0], Pick::Match(2));
        assert_eq!(m[3], Pick::Only(0), "a folder with one file needs no string");
        assert_eq!(m.iter().filter_map(|p| p.file()).count(), 4);
    }

    #[test]
    fn nothing_recurs_when_every_folder_names_it_differently() {
        let f = vec![
            folder("A1", &["first.ams", "second.ams"]),
            folder("A2", &["alpha.ams", "beta.ams"]),
            folder("A3", &["x1.ams", "y9.ams"]),
        ];
        let d = detect(&f, None);
        assert!(d.ident.is_none() && d.needed);
        // the user tries another string by hand
        let m = matches(&f, "BETA");
        assert_eq!(m[1], Pick::Match(1));
        assert_eq!(m[0], Pick::None);
        assert_eq!(matches(&f, "a"), vec![Pick::None, Pick::Several(vec![0, 1]), Pick::None]);
    }

    #[test]
    fn single_files_need_nothing_and_the_hint_wins_a_tie() {
        let only = vec![folder("A", &["a.ams"]), folder("B", &["b.ams"])];
        let d = detect(&only, None);
        assert!(!d.needed && d.ident.is_none());
        let f = vec![
            folder("S1", &["S1_use.ams", "S1_final.ams", "S1_x.ams"]),
            folder("S2", &["S2_use.ams", "S2_final.ams", "S2_y.ams"]),
        ];
        assert_eq!(detect(&f, Some("_final")).ident.as_deref(), Some("_final"));
    }

    #[test]
    fn scan_groups_ams_files_per_folder() {
        let dir = std::env::temp_dir().join(format!("specset-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (d, f) in [("ZZ01", "ZZ01_use.ams"), ("ZZ01", "ZZ01.ams"), ("ZZ02", "ZZ02.ams"), (".hidden", "x.ams"), ("ZZ03/sub", "deep.ams")] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
            std::fs::write(dir.join(d).join(f), b"").unwrap();
        }
        std::fs::write(dir.join("ZZ02").join("notes.txt"), b"").unwrap();
        let s = scan(&dir);
        let names: Vec<&str> = s.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["ZZ01", "ZZ02", "ZZ03/sub"]);
        assert_eq!(s[0].files, ["ZZ01.ams", "ZZ01_use.ams"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn folders_of_exported_results_are_not_specimens() {
        let dir = std::env::temp_dir().join(format!("specset-geo-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ZZ01").join("_geo")).unwrap();
        std::fs::write(dir.join("ZZ01").join("ZZ01_use.ams"), b"").unwrap();
        std::fs::write(dir.join("ZZ01").join("_geo").join("ZZ01_use_geo.ams"), b"").unwrap();
        let s = scan(&dir);
        assert_eq!(s.len(), 1, "{:?}", s.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
        assert_eq!(s[0].name, "ZZ01");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
