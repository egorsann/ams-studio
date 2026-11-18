//! The file dialogs of the program, behind one type: a native dialog normally, nothing at all while the self-test
//! runs (a dialog would wait for a click that never comes). Same calls as `rfd::FileDialog`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

static BLOCKED: AtomicBool = AtomicBool::new(false);

/// While set, every dialog answers "cancelled" at once.
pub fn block_dialogs(on: bool) {
    BLOCKED.store(on, Ordering::Relaxed);
}

pub struct FileDialog(rfd::FileDialog);

impl FileDialog {
    pub fn new() -> Self {
        FileDialog(rfd::FileDialog::new())
    }
    pub fn add_filter(self, name: impl Into<String>, ext: &[impl ToString]) -> Self {
        FileDialog(self.0.add_filter(name, ext))
    }
    pub fn set_title(self, t: impl Into<String>) -> Self {
        FileDialog(self.0.set_title(t))
    }
    pub fn set_directory(self, d: impl AsRef<Path>) -> Self {
        FileDialog(self.0.set_directory(d))
    }
    pub fn set_file_name(self, n: impl Into<String>) -> Self {
        FileDialog(self.0.set_file_name(n))
    }
    pub fn pick_file(self) -> Option<PathBuf> {
        if BLOCKED.load(Ordering::Relaxed) {
            return None;
        }
        self.0.pick_file()
    }
    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        if BLOCKED.load(Ordering::Relaxed) {
            return None;
        }
        self.0.pick_files()
    }
    pub fn pick_folder(self) -> Option<PathBuf> {
        if BLOCKED.load(Ordering::Relaxed) {
            return None;
        }
        self.0.pick_folder()
    }
    pub fn save_file(self) -> Option<PathBuf> {
        if BLOCKED.load(Ordering::Relaxed) {
            return None;
        }
        self.0.save_file()
    }
}
