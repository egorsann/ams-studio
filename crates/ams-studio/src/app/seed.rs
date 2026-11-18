//! Taking over the setup of another computer that works on the same data folder, once.
//!
//! The layout, the panes and above all the GIS layer list live in the program's preferences, which are per computer.
//! When two computers have the same data in a synced folder (the same paths below a common parent, for example a
//! `SynologyDrive` folder), one of them writes its setup into the folder of the specimens: next to the `.ams` files of a
//! folder that holds them all, or at the parent level of the specimen folders (the folder that is opened with *Open
//! Folder of Specimens*) as `AMS_STUDIO_SETUP_dependancy.json` (`ams-studio --write-setup`). The other computer takes it
//! over **once**, the next time it opens that folder (at start the folders of its last session are looked at), if
//!
//! * the file sits in the opened folder or in one of its three parents,
//! * this computer has not applied that setup before (its id is remembered in `seed_applied.txt` next to the
//!   preferences), and
//! * it was not written by this very computer.
//!
//! Nothing else is searched and nothing is written anywhere but there and into the preferences, so a computer that
//! never sees such a file is not affected. Paths below the folder of the file are stored as `{BASE}`, paths below a
//! `SynologyDrive` folder as `{SYNO}`, and filled in again with the other computer's folders. The window size stays as
//! it is on the computer that takes the setup over; the preferences it replaces are kept as `state.before-seed.txt`.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const FILE: &str = "AMS_STUDIO_SETUP_dependancy.json";
pub const BASE: &str = "{BASE}";
pub const SYNO: &str = "{SYNO}";
const SYNC_DIR: &str = "SynologyDrive";
/// Preference lines that describe this computer's window, not the setup.
const LOCAL_KEYS: [&str; 3] = ["win_w", "win_h", "maximized"];

fn slashes(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_string()
}

/// The `SynologyDrive` folder prefix of a path (`/home/a/SynologyDrive/x/y` -> `/home/a/SynologyDrive`).
pub fn sync_prefix(path: &str) -> Option<String> {
    let p = path.replace('\\', "/");
    let i = p.find(&format!("/{SYNC_DIR}/")).or_else(|| p.strip_suffix(&format!("/{SYNC_DIR}")).map(|s| s.len()))?;
    Some(p[..i + 1 + SYNC_DIR.len()].to_string())
}

fn is_local_key(line: &str) -> bool {
    LOCAL_KEYS.iter().any(|k| line.strip_prefix(k).is_some_and(|rest| rest.starts_with('=')))
}

/// Folders of the last session named in the preferences: the specimen folder set, the last folder, the folder of the
/// last file.
pub fn candidates_from_state(state: &str) -> Vec<PathBuf> {
    let get = |key: &str| state.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('='))).filter(|v| !v.is_empty());
    let mut out: Vec<PathBuf> = vec![];
    // the folder of specimens first: that is where the setup file belongs
    let list = [get("set_root").map(PathBuf::from), get("last_file").and_then(|v| PathBuf::from(v).parent().map(|d| d.to_path_buf())), get("last_dir").map(PathBuf::from)];
    for p in list.into_iter().flatten() {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// The setup file in one of `dirs` or in one of their three parents.
pub fn find_in(dirs: &[PathBuf], is_file: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    for d in dirs {
        let mut cur = Some(d.as_path());
        for _ in 0..4 {
            let Some(c) = cur else { break };
            let f = c.join(FILE);
            if is_file(&f) && !out.contains(&f) {
                out.push(f);
            }
            cur = c.parent();
        }
    }
    out
}

/// The setup document for these preferences, written into `base` (the folder the file lies in).
pub fn make_setup(state: &str, plots: &str, base: &Path, host: &str, id: &str, written: &str) -> Value {
    let b = slashes(base);
    let sync = sync_prefix(&b);
    let keep = |t: &str| -> String {
        t.lines()
            .filter(|l| !is_local_key(l))
            .map(|l| {
                let l = l.replace(&b, BASE);
                match &sync {
                    Some(s) => l.replace(s.as_str(), SYNO),
                    None => l,
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };
    json!({
        "kind": "AMS Studio setup",
        "version": 1,
        "id": id,
        "written": written,
        "from_host": host,
        "note": "The setup (layout, panes, GIS layer list, preferences) of the computer named in from_host, for another computer that works on the same data folder. AMS Studio on that computer takes it over once, when it opens this folder (it remembers the id) and keeps its own window size. Paths below this folder are written as {BASE}, paths below a SynologyDrive folder as {SYNO}. Delete this file to stop it.",
        "state": keep(state),
        "plots": keep(plots),
    })
}

/// The preferences a setup gives for a computer where the file lies in `base`; its own window lines stay. `None` when the
/// file is not a setup or its `{SYNO}` paths cannot be filled in (the folder is not below a `SynologyDrive` folder here).
pub fn apply_setup(seed: &Value, base: &Path, current_state: &str) -> Option<(String, String)> {
    if seed.get("kind").and_then(|k| k.as_str()) != Some("AMS Studio setup") || seed.get("version").and_then(|v| v.as_u64()) != Some(1) {
        return None;
    }
    let b = slashes(base);
    let sync = sync_prefix(&b);
    let text = seed.get("state")?.as_str()?;
    let plots = seed.get("plots").and_then(|p| p.as_str()).unwrap_or("");
    if (text.contains(SYNO) || plots.contains(SYNO)) && sync.is_none() {
        return None;
    }
    let fill = |t: &str| {
        let t = t.replace(BASE, &b);
        match &sync {
            Some(s) => t.replace(SYNO, s),
            None => t,
        }
    };
    let mut state = fill(text);
    for l in current_state.lines().filter(|l| is_local_key(l)) {
        state.push_str(l);
        state.push('\n');
    }
    Some((state, fill(plots)))
}

/// The name of this computer (only compared with the one in the setup file; never sent anywhere).
pub fn host_name() -> String {
    if let Ok(n) = std::env::var("COMPUTERNAME") {
        return n;
    }
    std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).ok().filter(|s| !s.is_empty()).or_else(|| std::env::var("HOSTNAME").ok()).unwrap_or_else(|| "unknown".into())
}

/// What happened.
#[derive(Debug, PartialEq)]
pub enum Taken {
    /// Nothing to do (no setup file, already applied, or written by this computer).
    No,
    /// The preferences of this computer were replaced; the text says from where.
    Yes(String),
}

/// Applies a setup file found in `dirs` (or their parents) once. `cfg` is the program's preferences folder
/// (`.../ams-studio`); `state` is the text of its `state.txt` (empty when there is none).
pub fn take_over(cfg: &Path, dirs: &[PathBuf], host: &str) -> Taken {
    let state_path = cfg.join("state.txt");
    let state = std::fs::read_to_string(&state_path).unwrap_or_default();
    let marker = cfg.join("seed_applied.txt");
    let done = std::fs::read_to_string(&marker).unwrap_or_default();
    for file in find_in(dirs, &|p| p.is_file()) {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let Ok(seed) = serde_json::from_str::<Value>(&text) else { continue };
        let id = seed.get("id").and_then(|i| i.as_str()).unwrap_or_default().to_string();
        if id.is_empty() || done.lines().any(|l| l.trim() == id) {
            continue;
        }
        let write_marker = || {
            let _ = std::fs::create_dir_all(cfg);
            let _ = std::fs::write(&marker, format!("{done}{id}\n"));
        };
        let from = seed.get("from_host").and_then(|h| h.as_str()).unwrap_or("another computer").to_string();
        // the computer that wrote it has nothing to take over; it only notes the id so that it never does
        if from == host {
            write_marker();
            continue;
        }
        let Some(base) = file.parent() else { continue };
        let Some((new_state, new_plots)) = apply_setup(&seed, base, &state) else { continue };
        let _ = std::fs::create_dir_all(cfg);
        if state_path.is_file() {
            let _ = std::fs::copy(&state_path, cfg.join("state.before-seed.txt"));
        }
        if std::fs::write(&state_path, new_state).is_err() {
            continue;
        }
        if !new_plots.trim().is_empty() {
            let plots = cfg.join("plots.txt");
            if plots.is_file() {
                let _ = std::fs::copy(&plots, cfg.join("plots.before-seed.txt"));
            }
            let _ = std::fs::write(&plots, new_plots);
        }
        write_marker();
        return Taken::Yes(format!(
            "Setup taken over once from {from} (found in {}): layers, panes and preferences; your window size stays. Your earlier preferences are kept as state.before-seed.txt in the preferences folder.",
            base.display()
        ));
    }
    Taken::No
}

/// `ams-studio --write-setup [folder] [--force]`: this computer's setup as `AMS_STUDIO_SETUP_dependancy.json` in `folder`
/// (default: the folder of specimens of the last session, else the folder of the last file). Returns what to tell the user.
pub fn write_setup(cfg: &Path, folder: Option<&Path>, force: bool, written: &str) -> Result<String, String> {
    let state = std::fs::read_to_string(cfg.join("state.txt")).map_err(|e| format!("No preferences to write ({}: {e}). Start AMS Studio once and set it up first.", cfg.join("state.txt").display()))?;
    let plots = std::fs::read_to_string(cfg.join("plots.txt")).unwrap_or_default();
    let dir: PathBuf = match folder {
        Some(f) => f.to_path_buf(),
        None => candidates_from_state(&state).into_iter().find(|d| d.is_dir()).ok_or("No folder of specimens in the preferences: open one in AMS Studio first, or name the folder: --write-setup <folder>")?,
    };
    if !dir.is_dir() {
        return Err(format!("{} is not a folder.", dir.display()));
    }
    let file = dir.join(FILE);
    if file.is_file() && !force {
        return Err(format!("{} exists already. A setup is applied once per computer; to write a new one that other computers take over again, use --write-setup --force.", file.display()));
    }
    let id = format!("{}-{}", written.replace([' ', ':'], "-"), std::process::id());
    let seed = make_setup(&state, &plots, &dir, &host_name(), &id, written);
    std::fs::write(&file, serde_json::to_string_pretty(&seed).unwrap_or_default() + "\n").map_err(|e| format!("{}: {e}", file.display()))?;
    // this computer already has this setup: it never takes it over
    let marker = cfg.join("seed_applied.txt");
    let done = std::fs::read_to_string(&marker).unwrap_or_default();
    let _ = std::fs::write(&marker, format!("{done}{id}\n"));
    Ok(format!("Wrote the setup of this computer to {}. Another computer that has this folder takes it over once, when it opens the folder.", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("setup-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_synology_folder_is_found_in_a_path() {
        assert_eq!(sync_prefix("/home/a/SynologyDrive/x/y").as_deref(), Some("/home/a/SynologyDrive"));
        assert_eq!(sync_prefix("C:\\Users\\a\\SynologyDrive\\x").as_deref(), Some("C:/Users/a/SynologyDrive"));
        assert_eq!(sync_prefix("/home/a/Documents/x"), None);
        assert_eq!(sync_prefix("/home/a/SynologyDriveOld/x"), None);
    }

    #[test]
    fn candidates_and_search_go_up_three_parents() {
        let st = "last_dir=/s/Doc/Kaz/AMS/ER1_AMS\nset_root=/s/Doc/Kaz/AMS\nlast_file=/s/Doc/Kaz/AMS/ER1_AMS/ER1_use.ams\n";
        assert_eq!(candidates_from_state(st), [PathBuf::from("/s/Doc/Kaz/AMS"), PathBuf::from("/s/Doc/Kaz/AMS/ER1_AMS")]);
        let there = |p: &Path| p == Path::new("/s/Doc/Kaz/AMS").join(FILE);
        assert_eq!(find_in(&[PathBuf::from("/s/Doc/Kaz/AMS/ER1_AMS")], &there), [Path::new("/s/Doc/Kaz/AMS").join(FILE)]);
        assert!(find_in(&[PathBuf::from("/s/Doc/Kaz/AMS/a/b/c/d")], &there).is_empty(), "only three parents up");
    }

    const STATE_A: &str = "dark=1\nwin_w=1280\nwin_h=820\nmaximized=1\nset_root=/home/a/SynologyDrive/Doc/Kaz\ngis_layers={\"maps\":[{\"id\":1,\"path\":\"/home/a/SynologyDrive/GIS/x.tif\"}]}\nlast_dir=/tmp/elsewhere\n";

    #[test]
    fn paths_become_tokens_and_come_back_on_the_other_computer() {
        let seed = make_setup(STATE_A, "plot=1\n", Path::new("/home/a/SynologyDrive/Doc/Kaz"), "host-a", "id1", "now");
        let t = seed["state"].as_str().unwrap();
        assert!(t.contains("set_root={BASE}") && t.contains("{SYNO}/GIS/x.tif") && !t.contains("/home/a/") && !t.contains("win_w") && t.contains("last_dir=/tmp/elsewhere"));
        let (st, plots) = apply_setup(&seed, Path::new("/mnt/b/SynologyDrive/Doc/Kaz"), "win_w=999\nwin_h=500\nmaximized=0\ndark=0\n").unwrap();
        assert!(st.contains("set_root=/mnt/b/SynologyDrive/Doc/Kaz") && st.contains("\"path\":\"/mnt/b/SynologyDrive/GIS/x.tif\""));
        assert!(st.contains("dark=1") && st.contains("win_w=999") && !st.contains("win_w=1280") && st.contains("maximized=0"));
        assert_eq!(plots, "plot=1\n");
        // a folder outside any SynologyDrive folder cannot fill in {SYNO}
        assert!(apply_setup(&seed, Path::new("/other/Doc/Kaz"), "").is_none());
        // a setup with only paths below its own folder needs no Synology folder at all
        let plain = make_setup("set_root=/data/x\nlast_file=/data/x/a/b.ams\n", "", Path::new("/data/x"), "h", "i2", "now");
        let (st, _) = apply_setup(&plain, Path::new("/elsewhere/x"), "").unwrap();
        assert!(st.contains("set_root=/elsewhere/x") && st.contains("last_file=/elsewhere/x/a/b.ams"));
        assert!(apply_setup(&json!({"kind": "other"}), Path::new("/x"), "").is_none());
    }

    #[test]
    fn the_setup_is_taken_over_once_and_never_by_its_own_computer() {
        let data = tmp("data").join("SynologyDrive").join("Doc").join("AMS");
        std::fs::create_dir_all(data.join("ER1_AMS")).unwrap();
        let cfg_a = tmp("a-cfg");
        let st = STATE_A.replace("/home/a/SynologyDrive/Doc/Kaz", &slashes(&data)).replace("/home/a/SynologyDrive", &slashes(data.parent().unwrap().parent().unwrap()));
        std::fs::write(cfg_a.join("state.txt"), &st).unwrap();
        let msg = write_setup(&cfg_a, None, false, "2026-11-18 10:00").unwrap();
        assert!(msg.contains(FILE) && data.join(FILE).is_file(), "written next to the specimen folders: {msg}");
        assert!(write_setup(&cfg_a, None, false, "later").is_err(), "an existing setup is not replaced without --force");
        assert_eq!(take_over(&cfg_a, &[data.clone()], &host_name()), Taken::No, "the writing computer does not take it over");
        // another computer: its last file lies in a specimen folder, one level below the setup file
        let cfg_b = tmp("b-cfg");
        std::fs::write(cfg_b.join("state.txt"), "dark=0\nwin_w=700\n").unwrap();
        let first = take_over(&cfg_b, &[data.join("ER1_AMS")], "other-host");
        assert!(matches!(first, Taken::Yes(_)), "{first:?}");
        let got = std::fs::read_to_string(cfg_b.join("state.txt")).unwrap();
        assert!(got.contains("dark=1") && got.contains("win_w=700") && got.contains(&slashes(&data)));
        assert_eq!(std::fs::read_to_string(cfg_b.join("state.before-seed.txt")).unwrap(), "dark=0\nwin_w=700\n");
        assert_eq!(take_over(&cfg_b, &[data.clone()], "other-host"), Taken::No, "only once");
        std::fs::write(cfg_b.join("state.txt"), "dark=0\n").unwrap();
        assert_eq!(take_over(&cfg_b, &[data.clone()], "other-host"), Taken::No);
        assert_eq!(std::fs::read_to_string(cfg_b.join("state.txt")).unwrap(), "dark=0\n", "later changes stay");
        // folders without a setup file change nothing
        let (other, cfg_c) = (tmp("other"), tmp("c-cfg"));
        std::fs::write(cfg_c.join("state.txt"), "dark=0\n").unwrap();
        assert_eq!(take_over(&cfg_c, &[other.clone()], "third"), Taken::No);
        assert!(!cfg_c.join("seed_applied.txt").exists() && !cfg_c.join("state.before-seed.txt").exists());
        assert_eq!(take_over(&cfg_c, &[], "third"), Taken::No);
        for d in [data.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf(), cfg_a, cfg_b, other, cfg_c] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn all_files_in_one_folder_get_the_setup_next_to_them() {
        let flat = tmp("flat");
        let cfg = tmp("flat-cfg");
        std::fs::write(cfg.join("state.txt"), format!("set_root={}\ndark=1\n", slashes(&flat))).unwrap();
        write_setup(&cfg, None, false, "now").unwrap();
        assert!(flat.join(FILE).is_file());
        let _ = std::fs::remove_dir_all(&flat);
        let _ = std::fs::remove_dir_all(&cfg);
    }
}
