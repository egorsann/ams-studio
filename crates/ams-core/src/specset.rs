//! A parent folder with one sub-folder per specimen, each holding several `.ams` files.
//!
//! Naming differs between labs (the author's main files end in `_use`), so the identifier of the main file
//! per specimen is detected from the names: the string that picks out exactly one file in as many specimen
//! folders as possible. The user can confirm it, or try another string; [`matches`] says which file each
//! folder would give for a string.

use std::path::{Path, PathBuf};

/// A folder that holds `.ams` files (a specimen), or the files of one specimen inside a folder that holds many.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecFolder {
    /// Folder name, relative to the parent (`A/B` when nested); for specimens found by their ID inside a shared
    /// folder, the ID (`sub/1234567` when the shared folder is `sub`).
    pub name: String,
    pub dir: PathBuf,
    /// Data file names (`.ams` or `.ran`), sorted without regard to case. The out-of-phase halves of split specimens
    /// are not listed: they are in `pairs`.
    pub files: Vec<String>,
    /// Specimens whose in-phase and out-of-phase data are two files: (the file listed in `files`, its out-of-phase file).
    pub pairs: Vec<(String, String)>,
}

/// The kind of data file the specimens are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Ams,
    Ran,
}

impl Kind {
    pub fn ext(self) -> &'static str {
        match self {
            Kind::Ams => "ams",
            Kind::Ran => "ran",
        }
    }
    pub fn has(self, name: &str) -> bool {
        name.rsplit_once('.').map_or(false, |(_, e)| e.eq_ignore_ascii_case(self.ext()))
    }
}

/// How the files lie on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// Look at the names: a folder that holds files of several specimens is split by specimen ID.
    #[default]
    Auto,
    /// Every specimen has its own folder (a folder is one specimen, whatever its files are called).
    OwnFolders,
    /// The files of many specimens lie next to each other: they are told apart by the specimen ID in the name.
    OneFolder,
}

/// Whether a specimen's in-phase and out-of-phase data can be two files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phases {
    /// One file holds both phases: nothing is folded away.
    Off,
    /// Files named like a pair (`X.ran` + `X_Im.ran`, `X_in` + `X_out`) are one specimen.
    #[default]
    ByName,
    /// As `ByName`, and a specimen with exactly two files whose names say nothing is paired by its numbers.
    Yes,
}

/// Where the specimen ID sits in a file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdScheme {
    /// A run of exactly this many digits anywhere in the name (`1234567_use.ams`, `PB1234567.ams`).
    Digits(usize),
    /// The name up to the first `_`, `-`, space or `.`, when it holds a digit (`ER26PB002_use.ams`).
    FirstWord,
    /// The first this many characters.
    Chars(usize),
}

impl IdScheme {
    pub fn describe(self) -> String {
        match self {
            IdScheme::Digits(n) => format!("a {n}-digit number in the name"),
            IdScheme::FirstWord => "the first part of the name (up to the first _ - or space)".to_string(),
            IdScheme::Chars(n) => format!("the first {n} characters of the name"),
        }
    }

    /// The ID in a file name without its extension.
    pub fn id_of(self, stem: &str) -> Option<String> {
        match self {
            IdScheme::Digits(n) => {
                let b = stem.as_bytes();
                let mut i = 0;
                while i < b.len() {
                    if b[i].is_ascii_digit() {
                        let j = (i..b.len()).find(|&k| !b[k].is_ascii_digit()).unwrap_or(b.len());
                        if j - i == n {
                            return Some(stem[i..j].to_string());
                        }
                        i = j;
                    } else {
                        i += 1;
                    }
                }
                None
            }
            IdScheme::FirstWord => {
                let w: &str = stem.split(|c: char| matches!(c, '_' | '-' | ' ' | '.')).next().unwrap_or("");
                (w.chars().count() >= 2 && w.chars().any(|c| c.is_ascii_digit())).then(|| w.to_string())
            }
            IdScheme::Chars(n) => (n > 0 && stem.chars().count() >= n).then(|| stem.chars().take(n).collect()),
        }
    }
}

/// The ID scheme that tells the specimens apart in a list of file names, or `None` when the names show no ID. A
/// 7-digit number wins when most names have one; then any other length of digit run, then the first part of the name.
pub fn detect_scheme(stems: &[String]) -> Option<IdScheme> {
    if stems.len() < 2 {
        return None;
    }
    let covers = |sc: IdScheme| {
        let ids: Vec<Option<String>> = stems.iter().map(|s| sc.id_of(s)).collect();
        let have = ids.iter().flatten().count();
        let mut distinct: Vec<&String> = ids.iter().flatten().collect();
        distinct.sort();
        distinct.dedup();
        (have, distinct.len())
    };
    let good = |have: usize, distinct: usize| have * 10 >= stems.len() * 8 && distinct >= 2;
    let (h7, d7) = covers(IdScheme::Digits(7));
    if good(h7, d7) {
        return Some(IdScheme::Digits(7));
    }
    let mut best: Option<(usize, usize, IdScheme)> = None;
    for n in 4..=12 {
        let (h, d) = covers(IdScheme::Digits(n));
        if good(h, d) && best.map_or(true, |(bh, ..)| h > bh) {
            best = Some((h, d, IdScheme::Digits(n)));
        }
    }
    if let Some((.., sc)) = best {
        return Some(sc);
    }
    let (h, d) = covers(IdScheme::FirstWord);
    good(h, d).then_some(IdScheme::FirstWord)
}

const MAX_DEPTH: usize = 4;
const MAX_FOLDERS: usize = 20000;

/// Folders of exported results: `_raw`, `_corr`, `_geo` and `<specimen folder>_geo` (File > Save as .ams, Drill Core to Geo).
/// They hold copies of specimens, not specimens.
pub fn is_result_folder(name: &str) -> bool {
    matches!(name, "_geo" | "_raw" | "_corr") || name.ends_with("_geo")
}

/// Name of the folder the geographic copies of a specimen folder go into: `DC26AB002_AMS` -> `DC26AB002_AMS_geo`.
pub fn geo_folder_name(specimen_folder: &str) -> String {
    if specimen_folder.is_empty() { "_geo".to_string() } else { format!("{specimen_folder}_geo") }
}

/// Every `.ams` and `.ran` file under a folder, found in one walk (hidden folders and result folders skipped).
#[derive(Debug, Clone, Default)]
pub struct RawScan {
    /// (folder, file name), the folder's files together.
    pub dirs: Vec<(PathBuf, Vec<String>)>,
}

impl RawScan {
    /// Number of files of each kind: (.ams, .ran).
    pub fn counts(&self) -> (usize, usize) {
        let n = |k: Kind| self.dirs.iter().map(|(_, f)| f.iter().filter(|n| k.has(n)).count()).sum();
        (n(Kind::Ams), n(Kind::Ran))
    }
}

pub fn scan_raw(root: &Path) -> RawScan {
    let mut out = RawScan::default();
    walk(root, 0, &mut out);
    out
}

fn walk(dir: &Path, depth: usize, out: &mut RawScan) {
    if depth > MAX_DEPTH || out.dirs.len() > MAX_FOLDERS {
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
            if is_result_folder(&name) {
                continue;
            }
            subs.push(p);
        } else if Kind::Ams.has(&name) || Kind::Ran.has(&name) {
            files.push(name);
        }
    }
    if !files.is_empty() {
        files.sort_by_key(|f| f.to_lowercase());
        out.dirs.push((dir.to_path_buf(), files));
    }
    subs.sort();
    for s in subs {
        walk(&s, depth + 1, out);
    }
}

/// What [`build`] made.
#[derive(Debug, Clone, Default)]
pub struct Built {
    pub folders: Vec<SpecFolder>,
    /// The ID scheme that split a shared folder (the first one that was split).
    pub scheme: Option<IdScheme>,
    /// Shared folders that were split by specimen ID.
    pub split_dirs: usize,
}

fn rel_name(root: &Path, dir: &Path) -> String {
    if dir == root {
        "(this folder)".to_string()
    } else {
        dir.strip_prefix(root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default()
    }
}

/// The specimens of a scan: one entry per specimen folder, or per specimen ID inside a shared folder. `scheme`
/// overrides the detected ID scheme. Files that are the out-of-phase half of a pair named by phase words (`X.ran` and
/// `X_Im.ran`) are folded into `pairs`.
pub fn build(root: &Path, raw: &RawScan, kind: Kind, layout: Layout, scheme: Option<IdScheme>, phases: Phases) -> Built {
    let mut out = Built::default();
    for (dir, all) in &raw.dirs {
        let files: Vec<String> = all.iter().filter(|n| kind.has(n)).cloned().collect();
        if files.is_empty() {
            continue;
        }
        let base = rel_name(root, dir);
        let stems: Vec<String> = files.iter().map(|f| stem(f).to_string()).collect();
        // the phase halves are folded away first: `X_Im` must not look like a second specimen
        let pairs = if phases == Phases::Off { vec![] } else { crate::phases::pair_files(&stems) };
        let halves: Vec<(usize, usize)> = pairs.iter().filter_map(|p| Some((p.ip?, p.op?))).collect();
        let hidden: Vec<usize> = halves.iter().map(|h| h.1).collect();
        let main: Vec<usize> = (0..files.len()).filter(|i| !hidden.contains(i)).collect();
        let main_stems: Vec<String> = main.iter().map(|&i| stems[i].clone()).collect();
        let sc = match layout {
            Layout::OwnFolders => None,
            _ => scheme.or_else(|| detect_scheme(&main_stems)),
        };
        let mut groups: Vec<(String, Vec<usize>)> = vec![];
        let mut split = false;
        if let Some(sc) = sc {
            let mut order: Vec<(String, Vec<usize>)> = vec![];
            for &i in &main {
                let id = sc.id_of(&stems[i]).unwrap_or_else(|| stems[i].clone());
                match order.iter_mut().find(|g| g.0 == id) {
                    Some(g) => g.1.push(i),
                    None => order.push((id, vec![i])),
                }
            }
            // one ID for the whole folder = a specimen folder, nothing to split
            if order.len() >= 2 {
                split = true;
                groups = order;
            }
        }
        let groups = if split { groups } else { vec![(String::new(), main.clone())] };
        if split {
            out.split_dirs += 1;
            out.scheme = out.scheme.or(sc);
        }
        for (id, idx) in groups {
            let name = if !split {
                base.clone()
            } else if dir == root {
                id.clone()
            } else {
                format!("{base}/{id}")
            };
            let mut fl: Vec<String> = idx.iter().map(|&i| files[i].clone()).collect();
            fl.sort_by_key(|f| f.to_lowercase());
            let pr: Vec<(String, String)> = halves.iter().filter(|h| idx.contains(&h.0)).map(|h| (files[h.0].clone(), files[h.1].clone())).collect();
            out.folders.push(SpecFolder { name, dir: dir.clone(), files: fl, pairs: pr });
        }
    }
    out.folders.sort_by_key(|f| f.name.to_lowercase());
    out
}

/// Walks `root` and returns every folder that directly holds `.ams` files, sorted by name (the plain layout, no ID
/// splitting of shared folders).
pub fn scan(root: &Path) -> Vec<SpecFolder> {
    build(root, &scan_raw(root), Kind::Ams, Layout::OwnFolders, None, Phases::Off).folders
}

/// Pairs two files by content when their names do not say which is the out-of-phase one: a specimen with exactly two
/// files, one of them much smaller in Km. Reads the (small) files; `read` gives the 640-byte records of a path.
pub fn pair_two_by_content(folders: &mut [SpecFolder], read: &dyn Fn(&Path) -> Option<Vec<u8>>) -> usize {
    let mut n = 0;
    for f in folders.iter_mut() {
        if f.files.len() != 2 || !f.pairs.is_empty() {
            continue;
        }
        let (Some(a), Some(b)) = (read(&f.dir.join(&f.files[0])), read(&f.dir.join(&f.files[1]))) else { continue };
        if let Some(k) = crate::phases::guess_out_of_phase(&a, &b) {
            let (ip, op) = (f.files[1 - k].clone(), f.files[k].clone());
            f.files = vec![ip.clone()];
            f.pairs.push((ip, op));
            n += 1;
        }
    }
    n
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
        SpecFolder { name: name.into(), dir: PathBuf::from(name), files: files.iter().map(|s| s.to_string()).collect(), pairs: vec![] }
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
        std::fs::create_dir_all(dir.join("ZZ01").join("ZZ01_geo")).unwrap();
        std::fs::create_dir_all(dir.join("ZZ01").join("_geo")).unwrap();
        std::fs::write(dir.join("ZZ01").join("ZZ01_use.ams"), b"").unwrap();
        std::fs::write(dir.join("ZZ01").join("_geo").join("ZZ01_use_geo.ams"), b"").unwrap();
        std::fs::write(dir.join("ZZ01").join("ZZ01_geo").join("ZZ01_use_geo.ams"), b"").unwrap();
        assert!(is_result_folder("DC26AB002_AMS_geo") && is_result_folder("_geo") && !is_result_folder("DC26AB002_AMS"));
        assert_eq!(geo_folder_name("DC26AB002_AMS"), "DC26AB002_AMS_geo");
        let s = scan(&dir);
        assert_eq!(s.len(), 1, "{:?}", s.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
        assert_eq!(s[0].name, "ZZ01");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn flat_dir(files: &[&str]) -> (PathBuf, RawScan) {
        let dir = PathBuf::from("/data/all");
        (dir.clone(), RawScan { dirs: vec![(dir, files.iter().map(|s| s.to_string()).collect())] })
    }

    #[test]
    fn a_seven_digit_id_splits_a_shared_folder() {
        let (root, raw) = flat_dir(&["1234567.ams", "1234567_use.ams", "1234568_use.ams", "1234568_raw.ams", "7654321.ams", "notes 99.ams"]);
        let b = build(&root, &raw, Kind::Ams, Layout::Auto, None, Phases::ByName);
        assert_eq!(b.scheme, Some(IdScheme::Digits(7)));
        let names: Vec<&str> = b.folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["1234567", "1234568", "7654321", "notes 99"]);
        assert_eq!(b.folders[0].files, ["1234567.ams", "1234567_use.ams"]);
        // the identifier of the main file is found inside each specimen as before
        let d = detect(&b.folders, None);
        assert_eq!(d.ident.as_deref(), Some("_use"));
        // the other answer keeps the folder as one specimen
        let own = build(&root, &raw, Kind::Ams, Layout::OwnFolders, None, Phases::ByName);
        assert_eq!(own.folders.len(), 1);
        assert_eq!(own.folders[0].name, "(this folder)");
    }

    #[test]
    fn names_with_letters_and_digits_are_split_by_their_first_word() {
        let (root, raw) = flat_dir(&["ER26PB002_use.ams", "ER26PB002_old.ams", "ER26PB003_use.ams", "ER26PB004.ams"]);
        let b = build(&root, &raw, Kind::Ams, Layout::Auto, None, Phases::ByName);
        assert_eq!(b.scheme, Some(IdScheme::FirstWord));
        assert_eq!(b.folders.len(), 3);
        // one specimen's files only: no split, and a typed scheme still works
        let (root, raw) = flat_dir(&["ZZ01_use.ams", "ZZ01_raw.ams"]);
        let b = build(&root, &raw, Kind::Ams, Layout::Auto, None, Phases::ByName);
        assert_eq!(b.folders.len(), 1);
        assert_eq!(b.split_dirs, 0);
        let (root, raw) = flat_dir(&["abcX1.ams", "abcX2.ams", "defY1.ams"]);
        let b = build(&root, &raw, Kind::Ams, Layout::OneFolder, Some(IdScheme::Chars(3)), Phases::ByName);
        assert_eq!(b.folders.iter().map(|f| (f.name.as_str(), f.files.len())).collect::<Vec<_>>(), [("abc", 2), ("def", 1)]);
    }

    #[test]
    fn digit_runs_must_have_the_exact_length() {
        assert_eq!(IdScheme::Digits(7).id_of("PB1234567_x"), Some("1234567".into()));
        assert_eq!(IdScheme::Digits(7).id_of("12345678"), None);
        assert_eq!(IdScheme::Digits(7).id_of("a123456_b1234567"), Some("1234567".into()));
        assert_eq!(detect_scheme(&["1234567_a".into(), "1234568_a".into()]), Some(IdScheme::Digits(7)));
        assert_eq!(detect_scheme(&["12345_a".into(), "12346_a".into()]), Some(IdScheme::Digits(5)));
        assert_eq!(detect_scheme(&["one".into(), "two".into()]), None);
    }

    #[test]
    fn ran_pairs_are_folded_and_the_kind_is_respected() {
        let dir = PathBuf::from("/data/ET1");
        let raw = RawScan { dirs: vec![(dir.clone(), vec!["ET1.ams".into(), "ET1.ran".into(), "ET1_Im.ran".into(), "ET2.ran".into()])] };
        assert_eq!(raw.counts(), (1, 3));
        let b = build(&PathBuf::from("/data"), &raw, Kind::Ran, Layout::OwnFolders, None, Phases::ByName);
        assert_eq!(b.folders.len(), 1);
        assert_eq!(b.folders[0].files, ["ET1.ran", "ET2.ran"]);
        assert_eq!(b.folders[0].pairs, [("ET1.ran".to_string(), "ET1_Im.ran".to_string())]);
        let a = build(&PathBuf::from("/data"), &raw, Kind::Ams, Layout::OwnFolders, None, Phases::ByName);
        assert_eq!(a.folders[0].files, ["ET1.ams"]);
    }
}
