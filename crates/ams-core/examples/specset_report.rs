//! `specset_report <folder> [ams|ran] [auto|own|one]`: what the folder-of-specimens scan makes of a tree (names only).
use ams_core::specset::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(&a[1]);
    let kind = if a.get(2).map(|s| s.as_str()) == Some("ran") { Kind::Ran } else { Kind::Ams };
    let layout = match a.get(3).map(|s| s.as_str()) { Some("own") => Layout::OwnFolders, Some("one") => Layout::OneFolder, _ => Layout::Auto };
    let raw = scan_raw(&root);
    println!("files: {:?}", raw.counts());
    let b = build(&root, &raw, kind, layout, None, Phases::ByName);
    let d = detect(&b.folders, None);
    println!("{} specimens, scheme {:?}, split dirs {}, identifier {:?} ({}/{})", b.folders.len(), b.scheme, b.split_dirs, d.ident, d.covered, d.multi);
    let m = matches(&b.folders, d.ident.as_deref().unwrap_or(""));
    println!("matched {}", m.iter().filter(|p| p.file().is_some()).count());
    for f in b.folders.iter().take(6) {
        println!("  {} -> {:?} pairs {:?}", f.name, f.files, f.pairs);
    }
}
