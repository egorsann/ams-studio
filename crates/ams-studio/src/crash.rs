//! A program error must not close the window: the panic is written to `crash.log` (next to the preferences), the
//! step that failed is shown in a message and the program carries on. Without a console (the Windows release build),
//! a panic would otherwise end the program with no word.

use std::path::PathBuf;
use std::sync::Mutex;

static LAST: Mutex<Option<String>> = Mutex::new(None);

/// `crash.log`: `%APPDATA%\ams-studio\` on Windows, `~/.config/ams-studio/` on Linux.
pub fn log_path() -> PathBuf {
    crate::app::config_dir_pub().join("ams-studio").join("crash.log")
}

fn payload_text(info: &std::panic::PanicHookInfo<'_>) -> String {
    let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".to_string()
    };
    match info.location() {
        Some(l) => format!("{msg} (at {}:{})", l.file().replace('\\', "/").rsplit("/crates/").next().unwrap_or(l.file()), l.line()),
        None => msg,
    }
}

/// Installs the hook that remembers the message and appends it, with the thread and a backtrace, to `crash.log`.
pub fn install() {
    std::panic::set_hook(Box::new(|info| {
        let text = payload_text(info);
        if let Ok(mut g) = LAST.lock() {
            *g = Some(text.clone());
        }
        let thread = std::thread::current();
        let bt = std::backtrace::Backtrace::force_capture();
        let line = format!("[{}] AMS Studio {} on {} / thread {}: {}\n{}\n\n", stamp(), env!("CARGO_PKG_VERSION"), std::env::consts::OS, thread.name().unwrap_or("?"), text, bt);
        let p = log_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        // keep the file small: start over when it has grown past 256 kB
        if std::fs::metadata(&p).map_or(false, |m| m.len() > 256 * 1024) {
            let _ = std::fs::remove_file(&p);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
    }));
}

fn stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// The message of the last panic (and forgets it).
pub fn take_last() -> Option<String> {
    LAST.lock().ok().and_then(|mut g| g.take())
}
