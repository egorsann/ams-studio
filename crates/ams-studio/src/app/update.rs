//! Update checker and self-updater.
//!
//! Asks GitHub for the latest release of this program, compares its version with the running one and, when the user
//! agrees, downloads the new executable, checks its SHA-256 against the checksum file attached to the release, swaps it
//! in for the running executable and offers a restart. Only the program itself is touched; no data, no settings.
//!
//! Works on a machine that has nothing but the exe and an internet connection: TLS (rustls) and the certificate roots
//! are built into the executable, nothing is read from the system or installed. No account, no token.
//!
//! Replacing a running executable: Linux lets a file be renamed over while it runs; Windows lets it be renamed but not
//! overwritten, so the running exe is moved to `<name>.old` first and the new one takes its place; the `.old` file is
//! deleted at the next start.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// `owner/name` of the GitHub repository whose releases are the source of updates.
pub const REPO: &str = "egorsann/ams-studio";
/// The version of the running program.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// File name of the executable asset of a release on this platform.
pub fn asset_name() -> &'static str {
    if cfg!(windows) {
        "ams-studio-windows-x64.exe"
    } else {
        "ams-studio-linux-x64"
    }
}

/// A newer release that can be installed.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    pub url: String,
    /// Expected SHA-256 (lowercase hex) when the release carries one: from the `.sha256` asset, else GitHub's digest.
    pub sha_url: Option<String>,
    pub digest: Option<String>,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Nothing asked yet (or checking is switched off).
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading,
    /// Installed; the new version starts after a restart.
    Ready(String),
    /// The check could not be made (no connection, rate limit, no release yet).
    Offline(String),
    /// The download / install failed; the old program is untouched.
    Failed(String),
}

enum Msg {
    Checked(Result<Option<Release>, String>),
    Installed(Result<String, String>),
}

pub struct Updater {
    pub state: State,
    /// Show the window that offers the install.
    pub window: bool,
    /// Restart the program after it closes (set by *Restart now*).
    pub restart_on_exit: bool,
    /// The user asked for the check (a click), so a failure is shown, not hidden.
    pub manual: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// Bytes downloaded / total of the running download.
    pub done: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
}

impl Default for Updater {
    fn default() -> Self {
        let (tx, rx) = channel();
        Updater { state: State::Idle, window: false, restart_on_exit: false, manual: false, tx, rx, done: Arc::new(AtomicU64::new(0)), total: Arc::new(AtomicU64::new(0)) }
    }
}

static EXE: OnceLock<PathBuf> = OnceLock::new();

/// Path of the running executable, taken once at the start (after an update `current_exe` can report the replaced file).
pub fn exe_path() -> Option<PathBuf> {
    if EXE.get().is_none() {
        if let Ok(p) = std::env::current_exe() {
            let _ = EXE.set(p);
        }
    }
    EXE.get().cloned()
}

/// Removes the `.old` file that an earlier update left next to the executable (Windows cannot delete a running exe).
pub fn clean_leftovers() {
    if let Some(exe) = exe_path() {
        let _ = std::fs::remove_file(old_path(&exe));
        let _ = std::fs::remove_file(new_path(&exe));
    }
}

fn old_path(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

fn new_path(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".new");
    PathBuf::from(s)
}

impl Updater {
    /// Starts the check in the background (does nothing while one runs).
    pub fn check(&mut self, ctx: &egui::Context, manual: bool) {
        if matches!(self.state, State::Checking | State::Downloading) {
            return;
        }
        self.state = State::Checking;
        self.manual = manual;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = fetch_latest();
            let _ = tx.send(Msg::Checked(r));
            ctx.request_repaint();
        });
    }

    /// Starts the download and install of `rel` in the background.
    pub fn install(&mut self, ctx: &egui::Context, rel: Release) {
        if matches!(self.state, State::Downloading) {
            return;
        }
        self.state = State::Downloading;
        self.done.store(0, Ordering::Relaxed);
        self.total.store(rel.size, Ordering::Relaxed);
        let (tx, ctx, done, total) = (self.tx.clone(), ctx.clone(), self.done.clone(), self.total.clone());
        std::thread::spawn(move || {
            let r = download_and_replace(&rel, &done, &total).map(|_| rel.version.clone());
            let _ = tx.send(Msg::Installed(r));
            ctx.request_repaint();
        });
    }

    /// Takes in what the background threads have finished; call once per frame.
    pub fn poll(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Checked(Ok(Some(r))) => self.state = State::Available(r),
                Msg::Checked(Ok(None)) => self.state = State::UpToDate,
                Msg::Checked(Err(e)) => self.state = State::Offline(e),
                Msg::Installed(Ok(v)) => self.state = State::Ready(v),
                Msg::Installed(Err(e)) => self.state = State::Failed(e),
            }
        }
    }

}

/// Starts the freshly installed program (used when the old one has closed).
pub fn spawn_new() {
    if let Some(exe) = exe_path() {
        let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        let _ = std::process::Command::new(exe).args(args).spawn();
    }
}

fn agent(secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(secs)))
        .user_agent(format!("AMS-Studio/{CURRENT} (update check)"))
        .build()
        .into()
}

/// The newest release if it is newer than the running program, `None` if not.
fn fetch_latest() -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = agent(15)
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(404) => "no release published yet".to_string(),
            ureq::Error::StatusCode(403) | ureq::Error::StatusCode(429) => "GitHub is limiting requests from this network, try again later".to_string(),
            e => format!("no connection to GitHub ({e})"),
        })?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    parse_release(&body, CURRENT, asset_name())
}

/// Pure part of the check (tested): `json` is GitHub's answer, `current` the running version.
pub fn parse_release(json: &str, current: &str, asset: &str) -> Result<Option<Release>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("unreadable answer from GitHub ({e})"))?;
    let tag = v["tag_name"].as_str().ok_or("the release has no version")?;
    let (Some(new), Some(cur)) = (parse_version(tag), parse_version(current)) else {
        return Err(format!("cannot read the version '{tag}'"));
    };
    if new <= cur || v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return Ok(None);
    }
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let find = |name: &str| assets.iter().find(|a| a["name"].as_str() == Some(name));
    let Some(a) = find(asset) else {
        // a newer release exists but carries no build for this platform: nothing to install
        return Ok(None);
    };
    let url = a["browser_download_url"].as_str().ok_or("the release file has no download link")?.to_string();
    let digest = a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(|d| d.to_ascii_lowercase());
    let sha_url = find(&format!("{asset}.sha256")).and_then(|s| s["browser_download_url"].as_str()).map(|s| s.to_string());
    Ok(Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v["body"].as_str().unwrap_or("").trim().to_string(),
        page: v["html_url"].as_str().unwrap_or("").to_string(),
        url,
        sha_url,
        digest,
        size: a["size"].as_u64().unwrap_or(0),
    }))
}

/// `v1.2.3`, `1.2` or `1.2.3-beta` as numbers (a pre-release suffix is ignored).
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().map_or(Some(0), |p| p.parse().ok())?;
    let patch = it.next().map_or(Some(0), |p| p.parse().ok())?;
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// The hash in a `.sha256` file (`<hash>  <name>` or just `<hash>`).
pub fn parse_sha_file(text: &str) -> Option<String> {
    let h = text.split_whitespace().next()?.to_ascii_lowercase();
    (h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
}

/// Does the file start like an executable of this platform (a cheap guard against saving an error page as the program)?
pub fn looks_like_executable(head: &[u8]) -> bool {
    if cfg!(windows) {
        head.starts_with(b"MZ")
    } else {
        head.starts_with(b"\x7fELF")
    }
}

fn download_and_replace(rel: &Release, done: &AtomicU64, total: &AtomicU64) -> Result<(), String> {
    let exe = exe_path().ok_or("cannot find where the program is")?;
    let want = match (&rel.sha_url, &rel.digest) {
        (Some(u), _) => {
            let t = agent(30).get(u).call().map_err(|e| format!("checksum download failed ({e})"))?.body_mut().read_to_string().map_err(|e| e.to_string())?;
            parse_sha_file(&t).ok_or("the checksum file of the release is unreadable")?
        }
        (None, Some(d)) => d.clone(),
        // never install a file that cannot be checked
        (None, None) => return Err("the release carries no checksum, so it is not installed".into()),
    };

    let tmp = new_path(&exe);
    let got = save_checked(rel, &tmp, done, total);
    let sum = match got {
        Ok(s) => s,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    if sum != want {
        let _ = std::fs::remove_file(&tmp);
        return Err("the downloaded file does not match its checksum, nothing was installed".into());
    }
    let mut head = [0u8; 4];
    let ok = std::fs::File::open(&tmp).and_then(|mut f| f.read_exact(&mut head)).is_ok() && looks_like_executable(&head);
    if !ok {
        let _ = std::fs::remove_file(&tmp);
        return Err("the downloaded file is not a program for this system".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    swap(&exe, &tmp).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Downloads to `tmp` and returns the SHA-256 of what was written.
fn save_checked(rel: &Release, tmp: &Path, done: &AtomicU64, total: &AtomicU64) -> Result<String, String> {
    let dir_hint = "the folder of the program cannot be written to (put the program in a folder you own, or download the new version by hand)";
    let mut out = std::fs::File::create(tmp).map_err(|e| format!("{dir_hint}: {e}"))?;
    let mut resp = agent(600).get(&rel.url).call().map_err(|e| format!("download failed ({e})"))?;
    if let Some(n) = resp.body().content_length() {
        total.store(n, Ordering::Relaxed);
    }
    let mut reader = resp.body_mut().with_config().limit(1 << 30).reader();
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download interrupted ({e})"))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("cannot write the new file ({e})"))?;
        done.fetch_add(n as u64, Ordering::Relaxed);
    }
    out.flush().map_err(|e| e.to_string())?;
    out.sync_all().ok();
    Ok(hex(&hash.finalize()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Puts `new` in the place of `exe`; the old program stays as `<exe>.old` until the next start.
fn swap(exe: &Path, new: &Path) -> Result<(), String> {
    if cfg!(windows) {
        let old = old_path(exe);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).map_err(|e| format!("cannot replace the running program ({e}); put it in a folder you own, or download the new version by hand"))?;
        if let Err(e) = std::fs::rename(new, exe) {
            // put the old one back so the program is never left missing
            let _ = std::fs::rename(&old, exe);
            return Err(format!("cannot put the new program in place ({e})"));
        }
        Ok(())
    } else {
        std::fs::rename(new, exe).map_err(|e| format!("cannot replace the program ({e}); put it in a folder you own, or download the new version by hand"))
    }
}

/// `1.4 MB` style size for the window.
pub fn mb(n: u64) -> String {
    format!("{:.1} MB", n as f64 / 1_048_576.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(parse_version("v0.3.0"), Some((0, 3, 0)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("v0.10.1-beta.2"), Some((0, 10, 1)));
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
        assert!(parse_version("0.2.0") < parse_version("0.2.1"));
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    fn release_json(tag: &str, extra: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/o/r/releases/tag/{tag}","body":"notes","draft":false,"prerelease":false{extra},
            "assets":[
              {{"name":"ams-studio-windows-x64.exe","size":123,"browser_download_url":"https://x/win.exe","digest":"sha256:ABCD"}},
              {{"name":"ams-studio-windows-x64.exe.sha256","size":90,"browser_download_url":"https://x/win.sha256"}},
              {{"name":"ams-studio-linux-x64","size":456,"browser_download_url":"https://x/linux"}}]}}"#
        )
    }

    #[test]
    fn a_newer_release_is_offered_with_its_asset() {
        let r = parse_release(&release_json("v0.3.0", ""), "0.2.0", "ams-studio-windows-x64.exe").unwrap().unwrap();
        assert_eq!(r.version, "0.3.0");
        assert_eq!(r.url, "https://x/win.exe");
        assert_eq!(r.sha_url.as_deref(), Some("https://x/win.sha256"));
        assert_eq!(r.digest.as_deref(), Some("abcd"));
        assert_eq!(r.size, 123);
        let l = parse_release(&release_json("v0.3.0", ""), "0.2.0", "ams-studio-linux-x64").unwrap().unwrap();
        assert_eq!(l.url, "https://x/linux");
        assert!(l.sha_url.is_none() && l.digest.is_none());
    }

    #[test]
    fn same_older_draft_and_foreign_releases_are_not_offered() {
        let a = "ams-studio-windows-x64.exe";
        assert_eq!(parse_release(&release_json("v0.2.0", ""), "0.2.0", a).unwrap(), None);
        assert_eq!(parse_release(&release_json("v0.1.9", ""), "0.2.0", a).unwrap(), None);
        assert_eq!(parse_release(&release_json("v0.3.0", r#","draft":true"#), "0.2.0", a).unwrap(), None);
        assert_eq!(parse_release(&release_json("v0.3.0", ""), "0.2.0", "ams-studio-macos").unwrap(), None);
        assert!(parse_release("not json", "0.2.0", a).is_err());
    }

    #[test]
    fn checksum_files_are_read() {
        let h = "a".repeat(64);
        assert_eq!(parse_sha_file(&format!("{h}  ams-studio-windows-x64.exe\n")), Some(h.clone()));
        assert_eq!(parse_sha_file(&h.to_uppercase()), Some(h));
        assert_eq!(parse_sha_file("<html>404</html>"), None);
        assert_eq!(parse_sha_file(""), None);
    }

    #[test]
    fn sha256_matches_the_known_answer() {
        assert_eq!(hex(&Sha256::digest(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn the_swap_keeps_a_working_program() {
        let dir = std::env::temp_dir().join(format!("ams-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("prog");
        std::fs::write(&exe, b"old").unwrap();
        let new = new_path(&exe);
        std::fs::write(&new, b"new").unwrap();
        swap(&exe, &new).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert!(!new.exists());
        if cfg!(windows) {
            assert_eq!(std::fs::read(old_path(&exe)).unwrap(), b"old");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
