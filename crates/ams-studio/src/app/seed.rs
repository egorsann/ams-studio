//! Taking over the setup of another computer that shares the same Synology Drive folder, once.
//!
//! The layout, the panes and above all the GIS layer list live in the program's preferences, which are per computer.
//! When two computers have the same data in a synced `SynologyDrive` folder (the same paths below it), one of them can
//! write its setup as a *seed* into `SynologyDrive/AMS_Studio_seed/setup.json` (`ams-studio --write-seed`), and the
//! other takes it over **once** at its next start: the seed is applied if
//!
//! * a `SynologyDrive` folder exists in this user's home folder (or a path in the preferences names one),
//! * it holds `AMS_Studio_seed/setup.json`,
//! * this computer has not applied that seed before (the id is remembered in `seed_applied.txt`), and
//! * the seed was not written by this very computer.
//!
//! On any computer without such a folder nothing happens and nothing is read. Paths below the Synology folder are
//! stored with the token `{SYNO}` instead of this computer's path to the folder and filled in again on the other side.
//! The window size and the maximised state stay as they are on the computer that takes the setup over. The preferences
//! it replaces are kept as `state.before-seed.txt`.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const FOLDER: &str = "AMS_Studio_seed";
pub const FILE: &str = "setup.json";
pub const TOKEN: &str = "{SYNO}";
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

/// This computer's Synology folder: `<home>/SynologyDrive`, else the one a path in the preferences lies in.
pub fn find_sync_root(home: &Path, state_text: &str, is_dir: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let direct = home.join(SYNC_DIR);
    if is_dir(&direct) {
        return Some(direct);
    }
    for l in state_text.lines() {
        if let Some((_, v)) = l.split_once('=') {
            if let Some(root) = sync_prefix(v) {
                if is_dir(Path::new(&root)) {
                    return Some(PathBuf::from(root));
                }
            }
        }
    }
    None
}

/// The seed document for these preferences, written on the computer whose Synology folder is `root`.
pub fn make_seed(state: &str, plots: &str, root: &Path, host: &str, id: &str, written: &str) -> Value {
    let r = slashes(root);
    let keep = |t: &str| -> String {
        t.lines().filter(|l| !LOCAL_KEYS.iter().any(|k| l.strip_prefix(k).is_some_and(|rest| rest.starts_with('=')))).map(|l| l.replace(&r, TOKEN)).collect::<Vec<_>>().join("\n") + "\n"
    };
    json!({
        "kind": "AMS Studio setup seed",
        "version": 1,
        "id": id,
        "written": written,
        "from_host": host,
        "note": "The setup (layout, panes, GIS layer list, preferences) of the computer named in from_host, for another computer that shares this Synology Drive folder. AMS Studio on that computer takes it over once at its next start (it is remembered by id) and keeps its own window size. Paths below the Synology folder are written as {SYNO}. Delete this folder to stop it.",
        "state": keep(state),
        "plots": keep(plots),
    })
}

/// The preferences a seed gives for a computer whose Synology folder is `root`; the window lines of `current` stay.
pub fn apply_seed(seed: &Value, root: &Path, current_state: &str) -> Option<(String, String)> {
    if seed.get("kind").and_then(|k| k.as_str()) != Some("AMS Studio setup seed") || seed.get("version").and_then(|v| v.as_u64()) != Some(1) {
        return None;
    }
    let r = slashes(root);
    let fill = |t: &str| t.replace(TOKEN, &r);
    let mut state = fill(seed.get("state")?.as_str()?);
    for l in current_state.lines() {
        if LOCAL_KEYS.iter().any(|k| l.strip_prefix(k).is_some_and(|rest| rest.starts_with('='))) {
            state.push_str(l);
            state.push('\n');
        }
    }
    Some((state, fill(seed.get("plots").and_then(|p| p.as_str()).unwrap_or(""))))
}

/// The name of this computer (never sent anywhere; only compared with the one in the seed).
pub fn host_name() -> String {
    if let Ok(n) = std::env::var("COMPUTERNAME") {
        return n;
    }
    std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).ok().filter(|s| !s.is_empty()).or_else(|| std::env::var("HOSTNAME").ok()).unwrap_or_else(|| "unknown".into())
}

/// What happened at start.
#[derive(Debug, PartialEq)]
pub enum Taken {
    /// Nothing to do (no seed, already applied, or written by this computer).
    No,
    /// The setup of this computer was replaced; the text says from where.
    Yes(String),
}

/// Applies the seed once. `cfg` is the program's preferences folder (`.../ams-studio`).
pub fn take_over(cfg: &Path, home: &Path, host: &str) -> Taken {
    let state_path = cfg.join("state.txt");
    let state = std::fs::read_to_string(&state_path).unwrap_or_default();
    let Some(root) = find_sync_root(home, &state, &|p| p.is_dir()) else { return Taken::No };
    let seed_path = root.join(FOLDER).join(FILE);
    let Ok(text) = std::fs::read_to_string(&seed_path) else { return Taken::No };
    let Ok(seed) = serde_json::from_str::<Value>(&text) else { return Taken::No };
    let id = seed.get("id").and_then(|i| i.as_str()).unwrap_or_default().to_string();
    if id.is_empty() {
        return Taken::No;
    }
    let marker = cfg.join("seed_applied.txt");
    let done = std::fs::read_to_string(&marker).unwrap_or_default();
    if done.lines().any(|l| l.trim() == id) {
        return Taken::No;
    }
    let from = seed.get("from_host").and_then(|h| h.as_str()).unwrap_or("another computer").to_string();
    // the computer that wrote the seed has nothing to take over; it only notes the id so that it never does
    let write_marker = |extra: &str| {
        let _ = std::fs::create_dir_all(cfg);
        let _ = std::fs::write(&marker, format!("{done}{id}{extra}\n"));
    };
    if from == host {
        write_marker("");
        return Taken::No;
    }
    let Some((new_state, new_plots)) = apply_seed(&seed, &root, &state) else { return Taken::No };
    let _ = std::fs::create_dir_all(cfg);
    if state_path.is_file() {
        let _ = std::fs::copy(&state_path, cfg.join("state.before-seed.txt"));
    }
    if std::fs::write(&state_path, new_state).is_err() {
        return Taken::No;
    }
    if !new_plots.trim().is_empty() {
        let plots = cfg.join("plots.txt");
        if plots.is_file() {
            let _ = std::fs::copy(&plots, cfg.join("plots.before-seed.txt"));
        }
        let _ = std::fs::write(&plots, new_plots);
    }
    write_marker("");
    Taken::Yes(format!("Setup taken over once from {from} through the Synology folder (layers, panes and preferences; your window size stays). Your earlier preferences are kept as state.before-seed.txt next to them."))
}

/// `ams-studio --write-seed [--force]`: writes this computer's setup as the seed into its Synology folder. Returns what to
/// tell the user.
pub fn write_seed(cfg: &Path, home: &Path, force: bool, written: &str) -> Result<String, String> {
    let state = std::fs::read_to_string(cfg.join("state.txt")).map_err(|e| format!("No preferences to write ({}: {e}). Start AMS Studio once and set it up first.", cfg.join("state.txt").display()))?;
    let plots = std::fs::read_to_string(cfg.join("plots.txt")).unwrap_or_default();
    let root = find_sync_root(home, &state, &|p| p.is_dir()).ok_or("No SynologyDrive folder found in your home folder or in the paths of the preferences.")?;
    let dir = root.join(FOLDER);
    let file = dir.join(FILE);
    if file.is_file() && !force {
        return Err(format!("{} exists already. A seed is applied once per computer; to write a new one that other computers take over again, use --write-seed --force.", file.display()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let id = format!("{}-{}", written.replace([' ', ':'], "-"), std::process::id());
    let seed = make_seed(&state, &plots, &root, &host_name(), &id, written);
    std::fs::write(&file, serde_json::to_string_pretty(&seed).unwrap_or_default()).map_err(|e| format!("{}: {e}", file.display()))?;
    // this computer already has this setup: it never takes it over
    let marker = cfg.join("seed_applied.txt");
    let done = std::fs::read_to_string(&marker).unwrap_or_default();
    let _ = std::fs::write(&marker, format!("{done}{id}\n"));
    Ok(format!("Wrote the setup of this computer to {}. Another computer with the same SynologyDrive folder takes it over once at its next start.", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("seed-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const STATE_A: &str = "dark=1\nwin_w=1280\nwin_h=820\nmaximized=1\nset_root=/home/a/SynologyDrive/Doc/Kaz\ngis_layers={\"maps\":[{\"id\":1,\"path\":\"/home/a/SynologyDrive/GIS/x.tif\"}]}\nlast_dir=/tmp/elsewhere\n";

    #[test]
    fn the_synology_folder_is_found_in_a_path() {
        assert_eq!(sync_prefix("/home/a/SynologyDrive/x/y").as_deref(), Some("/home/a/SynologyDrive"));
        assert_eq!(sync_prefix("C:\\Users\\a\\SynologyDrive\\x").as_deref(), Some("C:/Users/a/SynologyDrive"));
        assert_eq!(sync_prefix("/home/a/Documents/x"), None);
        assert_eq!(sync_prefix("/home/a/SynologyDriveOld/x"), None);
    }

    #[test]
    fn a_seed_replaces_the_root_and_keeps_the_other_computers_window() {
        let seed = make_seed(STATE_A, "plot=1\n", Path::new("/home/a/SynologyDrive"), "legion-a", "id1", "now");
        let text = seed["state"].as_str().unwrap();
        assert!(text.contains("{SYNO}/Doc/Kaz") && !text.contains("/home/a/SynologyDrive") && !text.contains("win_w"));
        assert!(text.contains("last_dir=/tmp/elsewhere"));
        let (st, plots) = apply_seed(&seed, Path::new("/home/b/SynologyDrive"), "win_w=999\nwin_h=500\nmaximized=0\ndark=0\n").unwrap();
        assert!(st.contains("set_root=/home/b/SynologyDrive/Doc/Kaz") && st.contains("\"path\":\"/home/b/SynologyDrive/GIS/x.tif\""));
        assert!(st.contains("dark=1") && st.contains("win_w=999") && !st.contains("win_w=1280") && st.contains("maximized=0"));
        assert_eq!(plots, "plot=1\n");
        assert!(apply_seed(&json!({"kind": "other"}), Path::new("/x"), "").is_none());
    }

    #[test]
    fn the_seed_is_taken_over_once_and_never_by_its_own_computer() {
        let home_a = tmp("a-home");
        let cfg_a = tmp("a-cfg");
        std::fs::create_dir_all(home_a.join("SynologyDrive")).unwrap();
        let st = STATE_A.replace("/home/a/SynologyDrive", &slashes(&home_a.join("SynologyDrive")));
        std::fs::write(cfg_a.join("state.txt"), &st).unwrap();
        let msg = write_seed(&cfg_a, &home_a, false, "2026-11-18 10:00").unwrap();
        assert!(msg.contains("setup.json"));
        assert!(write_seed(&cfg_a, &home_a, false, "later").is_err(), "an existing seed is not replaced without --force");
        // the writing computer does not take it over
        assert_eq!(take_over(&cfg_a, &home_a, &host_name()), Taken::No);
        // another computer with the same folder (here: the same folder, another host name) takes it once
        let cfg_b = tmp("b-cfg");
        std::fs::write(cfg_b.join("state.txt"), "dark=0\nwin_w=700\n").unwrap();
        let first = take_over(&cfg_b, &home_a, "other-host");
        assert!(matches!(first, Taken::Yes(_)), "{first:?}");
        let got = std::fs::read_to_string(cfg_b.join("state.txt")).unwrap();
        assert!(got.contains("dark=1") && got.contains("win_w=700") && got.contains("SynologyDrive/Doc/Kaz"));
        assert_eq!(std::fs::read_to_string(cfg_b.join("state.before-seed.txt")).unwrap(), "dark=0\nwin_w=700\n");
        assert_eq!(take_over(&cfg_b, &home_a, "other-host"), Taken::No, "only once");
        // changes made later are not overwritten
        std::fs::write(cfg_b.join("state.txt"), "dark=0\n").unwrap();
        assert_eq!(take_over(&cfg_b, &home_a, "other-host"), Taken::No);
        assert_eq!(std::fs::read_to_string(cfg_b.join("state.txt")).unwrap(), "dark=0\n");
        // a computer without a Synology folder is not touched at all
        let (home_c, cfg_c) = (tmp("c-home"), tmp("c-cfg"));
        std::fs::write(cfg_c.join("state.txt"), "dark=0\n").unwrap();
        assert_eq!(take_over(&cfg_c, &home_c, "third"), Taken::No);
        assert!(!cfg_c.join("seed_applied.txt").exists() && !cfg_c.join("state.before-seed.txt").exists());
        // a folder without a seed changes nothing either
        std::fs::create_dir_all(home_c.join("SynologyDrive")).unwrap();
        assert_eq!(take_over(&cfg_c, &home_c, "third"), Taken::No);
        for d in [home_a, cfg_a, cfg_b, home_c, cfg_c] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_folder_named_in_the_preferences_is_used_when_the_home_has_none() {
        let root = tmp("root").join("SynologyDrive");
        std::fs::create_dir_all(&root).unwrap();
        let state = format!("last_dir={}/Doc\n", slashes(&root));
        let found = find_sync_root(Path::new("/nonexistent-home"), &state, &|p| p.is_dir());
        assert_eq!(found.map(|p| slashes(&p)), Some(slashes(&root)));
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }
}
