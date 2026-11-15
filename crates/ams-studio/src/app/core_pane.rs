//! View > Enable core viewer: a pane above the plots with the cube photos on the core. After the option is ticked a
//! window asks for the folder; the photos are searched for below it, however many folders down they are (a folder whose
//! name contains a string, `cube_crops` by default, with cube photos `<ID>_<letter><slice>.png` in it). The photos are
//! decoded into memory by background threads and appear in their places as they arrive; nothing is written to disk.

use super::core_links::{self, Link};
use super::{info_icon, tools_ui, App, W_COREVIEW};
use crate::core_view::{self, Cube};
use egui::{Align, Color32, ColorImage, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

/// Longest side of a decoded photo in memory; the crops are about 1000 px.
const TEX_PX: u32 = 256;
const DEFAULT_NEEDLE: &str = "cube_crops";

/// A folder with cube photos found below the folder the user pointed at.
#[derive(Clone)]
pub(super) struct Found {
    pub dir: PathBuf,
    /// Path below the search folder.
    pub rel: String,
    /// Cube photos in it.
    pub n: usize,
    /// The specimen id the photos are named after (the part before `_<letter><slice>`), when they agree.
    pub id: String,
}

type ScanResult = (Vec<Found>, Vec<Found>, Option<String>);

/// A specimen of the file tree in the table of the window.
pub(super) struct CvRow {
    path: PathBuf,
    id: String,
    /// Ticked: part of the linking; unticked rows are grayed out and left alone.
    on: bool,
    /// Indexes into `CoreViewState.list`, the closest folder first.
    cands: Vec<usize>,
    /// The closest folder is sure enough to be filled in by the automatic matching.
    top_ok: bool,
    /// What the user chose in the row (`Some(None)` = no link); `None` = not touched.
    manual: Option<Option<PathBuf>>,
    /// The folder of the saved link, when it exists on this machine.
    saved: Option<PathBuf>,
}

pub(super) struct CoreViewState {
    pub on: bool,
    /// The folder the search starts in (what the user picked), and the string the cube folder's name contains.
    pub root: Option<PathBuf>,
    pub needle: String,
    /// The cube photo folder in use.
    pub dir: Option<PathBuf>,
    pub cubes: Vec<Cube>,
    photos: BTreeMap<(u32, char), PathBuf>,
    textures: HashMap<(u32, char), TextureHandle>,
    pub view: core_view::View,
    specimen: String,
    /// The file the cubes were laid out for, and its number of records (a change redoes the layout).
    for_file: Option<(PathBuf, usize)>,
    rx: Option<Receiver<((u32, char), ColorImage)>>,
    total: usize,
    loaded: usize,
    // the window
    pub win: bool,
    scan: Option<Receiver<ScanResult>>,
    /// The search was started without the window (a new specimen): pick the best folder when it ends.
    auto: bool,
    found: Vec<Found>,
    by_content: Vec<Found>,
    suggestion: Option<String>,
    /// The folders offered in the table (the named ones, else every folder with photos, plus the saved links), the
    /// rows of the table, and whether `list` holds every folder with photos rather than the ones named by the string.
    list: Vec<Found>,
    from_content: bool,
    rows: Vec<CvRow>,
    rows_stale: bool,
    /// Match every specimen of the file tree at once (off: only the selected ones).
    pub fit_all: bool,
    /// The saved links: specimen id -> cube photo folder (`core_links.json`).
    pub links: BTreeMap<String, Link>,
    /// The file the automatic link / search was last tried for (so a missing link does not retry every frame).
    tried: Option<PathBuf>,
    pub note: String,
    /// Height of the pane as a fraction of the plot area.
    pub frac: f32,
    pub export: bool,
    /// What the pointer was over last (the pane's title).
    pub hover: String,
}

impl Default for CoreViewState {
    fn default() -> Self {
        CoreViewState {
            on: false,
            root: None,
            needle: DEFAULT_NEEDLE.to_string(),
            dir: None,
            cubes: vec![],
            photos: BTreeMap::new(),
            textures: HashMap::new(),
            view: core_view::View::default(),
            specimen: String::new(),
            for_file: None,
            rx: None,
            total: 0,
            loaded: 0,
            win: false,
            scan: None,
            auto: false,
            found: vec![],
            by_content: vec![],
            suggestion: None,
            list: vec![],
            from_content: false,
            rows: vec![],
            rows_stale: false,
            fit_all: false,
            links: BTreeMap::new(),
            tried: None,
            note: String::new(),
            frac: 0.28,
            export: true,
            hover: String::new(),
        }
    }
}

/// `(letter, slice)` of a photo name like `DC26AB002_A12` (the stem), with the part before it.
fn parse_photo(stem: &str) -> Option<(String, char, u32)> {
    let (id, tail) = stem.rsplit_once('_')?;
    let mut ch = tail.chars();
    let letter = ch.next()?.to_ascii_uppercase();
    let digits: String = ch.collect();
    if !matches!(letter, 'A'..='D') || digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((id.to_string(), letter, digits.parse().ok()?))
}

/// The photo formats that are read, in the order that is preferred when one cube has the same photo in several.
/// HEIC / HEIF is not among them (it needs the libheif C library); convert those photos to PNG or JPG first.
const PHOTO_EXT: [&str; 5] = ["png", "tif", "tiff", "jpg", "jpeg"];

/// Position of the file's extension in `PHOTO_EXT`, `None` when it is not a readable photo format.
fn photo_rank(p: &Path) -> Option<usize> {
    let x = p.extension()?.to_string_lossy().to_lowercase();
    PHOTO_EXT.iter().position(|e| *e == x)
}

/// Decodes a photo, turned upright by its EXIF orientation (phone / camera JPEGs and TIFFs carry one).
fn open_photo(path: &Path) -> Option<image::DynamicImage> {
    let mut dec = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    let orient = image::ImageDecoder::orientation(&mut dec).unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(dec).ok()?;
    img.apply_orientation(orient);
    Some(img)
}

/// The cube photos directly in `dir`: (specimen id, letter, slice, path).
fn photos_in(dir: &Path) -> Vec<(String, char, u32, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    // one photo per cube: when the same cube exists in several formats the preferred format wins
    let mut best: HashMap<(String, char, u32), (usize, PathBuf)> = HashMap::new();
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        let Some(rank) = photo_rank(&p).filter(|_| p.is_file()) else { continue };
        let Some((id, l, s)) = p.file_stem().and_then(|s| parse_photo(&s.to_string_lossy())) else { continue };
        let slot = best.entry((id.to_uppercase(), l, s)).or_insert((rank, p.clone()));
        if rank < slot.0 || (rank == slot.0 && p < slot.1) {
            *slot = (rank, p);
        }
    }
    let mut out: Vec<_> = best.into_iter().map(|((_, l, s), (_, p))| (p.file_stem().and_then(|f| parse_photo(&f.to_string_lossy())).map(|x| x.0).unwrap_or_default(), l, s, p)).collect();
    out.sort_by(|a, b| a.3.cmp(&b.3));
    out
}

fn most_common(ids: impl Iterator<Item = String>) -> String {
    let mut c: HashMap<String, usize> = HashMap::new();
    for i in ids {
        *c.entry(i).or_default() += 1;
    }
    c.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map(|x| x.0).unwrap_or_default()
}

/// Walks below `root` (any depth up to 14, hidden folders skipped). Returns the folders whose name contains `needle`
/// and that hold cube photos, every folder that holds at least three cube photos, and a string to suggest when no
/// name contains `needle`: what the names of those folders have in common once the specimen id is taken off.
fn scan(root: &Path, needle: &str) -> ScanResult {
    let needle = needle.trim().to_lowercase();
    let (mut named, mut content) = (vec![], vec![]);
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        visited += 1;
        if visited > 60_000 {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut ids: Vec<String> = vec![];
        let mut cubes: HashSet<(String, char, u32)> = HashSet::new();
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if !name.starts_with('.') && depth < 14 {
                    stack.push((p, depth + 1));
                }
            } else if photo_rank(&p).is_some() {
                if let Some((id, l, s)) = p.file_stem().and_then(|s| parse_photo(&s.to_string_lossy())) {
                    if cubes.insert((id.to_uppercase(), l, s)) {
                        ids.push(id);
                    }
                }
            }
        }
        let n = cubes.len();
        if n >= 1 {
            let rel = dir.strip_prefix(root).map(|r| r.display().to_string()).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| ".".into());
            let f = Found { dir: dir.clone(), rel, n, id: most_common(ids.into_iter()) };
            let dir_name = dir.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !needle.is_empty() && dir_name.contains(&needle) {
                named.push(f.clone());
            }
            if n >= 3 {
                content.push(f);
            }
        }
    }
    named.sort_by(|a, b| a.rel.cmp(&b.rel));
    content.sort_by(|a, b| a.rel.cmp(&b.rel));
    // a string to suggest: the folder names without the specimen id in front of them
    let mut stripped: Vec<String> = vec![];
    for f in &content {
        let name = f.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let s = name.strip_prefix(&f.id).unwrap_or(&name).trim_start_matches('_').to_string();
        if !s.is_empty() {
            stripped.push(s);
        }
    }
    let suggestion = if stripped.is_empty() { None } else { Some(most_common(stripped.into_iter())) };
    (named, content, suggestion)
}

/// The specimen id of a file: its name up to the first `_` (`DC26AB002_use.ams` -> `DC26AB002`).
fn spec_id(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    stem.split('_').next().unwrap_or("").to_string()
}



fn trailing_number(s: &str) -> Option<(String, i64)> {
    let digits: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect();
    if digits.is_empty() {
        return None;
    }
    Some((s[..s.len() - digits.len()].to_string(), digits.parse().ok()?))
}

fn folder_name(f: &Found) -> String {
    f.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| f.rel.clone())
}

/// How close a cube folder is to a specimen: photos named after it, its id in the folder name or path, then the
/// shared start of the ids and the distance of their trailing numbers (DC26AB003 is closer to DC26AB002 than DC26AB012).
fn score(id: &str, f: &Found) -> i64 {
    let (a, b) = (id.to_lowercase(), f.id.to_lowercase());
    let mut s = 0i64;
    if !a.is_empty() {
        if a == b {
            s += 10_000;
        } else {
            s += a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count() as i64 * 100;
            if let (Some((ha, na)), Some((hb, nb))) = (trailing_number(&a), trailing_number(&b)) {
                if ha == hb {
                    s += 500 - (na - nb).abs().min(400);
                }
            }
        }
        if folder_name(f).to_lowercase().contains(&a) {
            s += 3_000;
        } else if f.rel.to_lowercase().contains(&a) {
            s += 1_500;
        }
    }
    s
}

/// Indexes of `list`, the folder closest to the specimen first.
fn rank(id: &str, list: &[Found]) -> Vec<usize> {
    let mut v: Vec<usize> = (0..list.len()).collect();
    v.sort_by(|&x, &y| score(id, &list[y]).cmp(&score(id, &list[x])).then_with(|| list[x].rel.cmp(&list[y].rel)));
    v
}

/// Photos named after the specimen, or its id in the folder name: sure enough for the automatic matching.
fn confident(id: &str, f: &Found) -> bool {
    !id.is_empty() && (f.id.eq_ignore_ascii_case(id) || folder_name(f).to_lowercase().contains(&id.to_lowercase()))
}

/// The folder a row ends up with: what the user chose, else the saved link, else (when the row is part of the
/// automatic matching) the closest folder if it is sure enough.
fn row_pick(row: &CvRow, list: &[Found], eligible: bool) -> Option<usize> {
    let dir = match &row.manual {
        Some(m) => m.clone(),
        None => row.saved.clone().or_else(|| if eligible && row.top_ok { row.cands.first().map(|&k| list[k].dir.clone()) } else { None }),
    };
    dir.and_then(|d| list.iter().position(|f| f.dir == d))
}

fn link_symbol(ui: &mut Ui, linked: bool) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(36.0, 22.0), Sense::hover());
    let col = if linked { Color32::from_rgb(52, 130, 230) } else { ui.visuals().weak_text_color().gamma_multiply(0.7) };
    let (w, h) = (16.0, 10.0);
    // two chain links: joined when there is a link, apart when there is none
    let off = if linked { 5.0 } else { 10.0 };
    for dx in [-off, off] {
        ui.painter().rect_stroke(Rect::from_center_size(Pos2::new(r.center().x + dx, r.center().y), Vec2::new(w, h)), 5.0, Stroke::new(2.2, col), StrokeKind::Inside);
    }
}


impl App {
    /// Starts the search for the cube folder below the saved root (in the background).
    fn cv_start_scan(&mut self, auto: bool) {
        let Some(root) = self.cv.root.clone() else { return };
        let needle = self.cv.needle.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(scan(&root, &needle));
        });
        self.cv.scan = Some(rx);
        self.cv.auto = auto;
        self.cv.found.clear();
        self.cv.by_content.clear();
        self.cv.suggestion = None;
    }

    pub(super) fn cv_open_id(&self) -> String {
        self.doc.as_ref().filter(|d| !d.synthetic).map(|d| spec_id(&d.path)).unwrap_or_default()
    }

    fn cv_save_links(&self) {
        let _ = core_links::write(&core_links::path(), &self.cv.links);
    }

    /// The window with the table, searching at once when a folder is known.
    pub(super) fn cv_open_window(&mut self) {
        self.cv.win = true;
        self.open_win(W_COREVIEW);
        self.cv.rows_stale = true;
        if self.cv.root.is_none() {
            // start where the open file is: its folder and up
            self.cv.root = self.doc.as_ref().filter(|d| !d.synthetic).and_then(|d| d.path.parent().map(|p| p.to_path_buf()));
        }
        self.cv_start_scan(false);
    }

    /// The window was cancelled or closed: when no link exists at all and nothing is shown, the viewer stays off.
    pub(super) fn cv_dismiss(&mut self) {
        self.cv.win = false;
        if self.cv.dir.is_none() && self.cv.links.is_empty() {
            self.cv.on = false;
        }
    }

    /// View > Enable core viewer: the open specimen's saved link is used when there is one, else the window asks.
    pub(super) fn cv_toggle(&mut self) {
        if self.cv.on {
            self.cv.on = false;
            self.cv.textures.clear();
            self.cv.rx = None;
            self.cv.win = false;
            self.cv.tried = None;
            self.save_state();
            return;
        }
        self.cv.on = true;
        self.cv.note.clear();
        self.cv.tried = self.doc.as_ref().filter(|d| !d.synthetic).map(|d| d.path.clone());
        self.cv.for_file = None;
        if !self.cv_try_link() {
            self.cv.dir = None;
            self.cv_open_window();
        }
        self.save_state();
    }

    /// Loads the photos of the open specimen from its saved link; false when there is none (or its folder is gone).
    fn cv_try_link(&mut self) -> bool {
        let id = self.cv_open_id();
        let Some(dir) = self.cv.links.get(&id).and_then(|l| l.resolve(self.cv.root.as_deref())) else { return false };
        self.cv_load(dir);
        true
    }

    /// No photos for the open specimen: the pane shows its cubes as empty places.
    fn cv_clear_photos(&mut self) {
        self.cv.photos.clear();
        self.cv.textures.clear();
        self.cv.rx = None;
        self.cv.total = 0;
        self.cv.loaded = 0;
        self.cv.dir = None;
        self.cv.specimen = self.cv_open_id();
    }

    /// The specimens (their main files) of the file tree, else the open file.
    fn cv_files(&self) -> Vec<PathBuf> {
        let v = self.tree_specimen_files();
        if !v.is_empty() {
            return v;
        }
        self.doc.as_ref().filter(|d| !d.synthetic).map(|d| vec![d.path.clone()]).unwrap_or_default()
    }

    /// The specimens the automatic matching works for when not every one is fitted at once.
    fn cv_selection(&self) -> Vec<PathBuf> {
        if !self.tree_sel.is_empty() {
            return self.tree_sel.clone();
        }
        self.doc.as_ref().filter(|d| !d.synthetic).map(|d| vec![d.path.clone()]).unwrap_or_default()
    }

    /// The folders offered (found ones plus the saved links of the specimens) and the table rows, closest folder first.
    fn cv_rebuild(&mut self) {
        let files = self.cv_files();
        let root = self.cv.root.clone();
        let mut list = if self.cv.found.is_empty() { self.cv.by_content.clone() } else { self.cv.found.clone() };
        self.cv.from_content = self.cv.found.is_empty();
        let mut saved: Vec<Option<PathBuf>> = vec![];
        for p in &files {
            let dir = self.cv.links.get(&spec_id(p)).and_then(|l| l.resolve(root.as_deref()));
            if let Some(d) = &dir {
                if !list.iter().any(|f| f.dir == *d) {
                    let ph = photos_in(d);
                    let rel = root.as_ref().and_then(|r| d.strip_prefix(r).ok()).map(|r| r.display().to_string()).unwrap_or_else(|| d.display().to_string());
                    list.push(Found { dir: d.clone(), rel, n: ph.len(), id: most_common(ph.iter().map(|x| x.0.clone())) });
                }
            }
            saved.push(dir);
        }
        let old: HashMap<PathBuf, (bool, Option<Option<PathBuf>>)> = self.cv.rows.drain(..).map(|r| (r.path, (r.on, r.manual))).collect();
        self.cv.rows = files
            .into_iter()
            .zip(saved)
            .map(|(path, saved)| {
                let id = spec_id(&path);
                let cands = rank(&id, &list);
                let top_ok = cands.first().map_or(false, |&k| confident(&id, &list[k]));
                let (on, manual) = old.get(&path).cloned().unwrap_or((true, None));
                CvRow { path, id, on, cands, top_ok, manual, saved }
            })
            .collect();
        self.cv.list = list;
    }

    /// Decodes the photos of the folder in the background and lays the cubes out for the open file.
    fn cv_load(&mut self, dir: PathBuf) {
        let id = self.cv_open_id();
        let all = photos_in(&dir);
        // only the photos named after this specimen when there are any (a folder may hold several)
        let mine: Vec<_> = if all.iter().any(|p| p.0.eq_ignore_ascii_case(&id)) { all.iter().filter(|p| p.0.eq_ignore_ascii_case(&id)).collect() } else { all.iter().collect() };
        self.cv.specimen = mine.first().map(|p| p.0.clone()).unwrap_or(id);
        self.cv.photos = mine.iter().map(|p| ((p.2, p.1), p.3.clone())).collect();
        self.cv.dir = Some(dir);
        self.cv.textures.clear();
        self.cv.for_file = None;
        self.cv_layout();
        // the workers: four threads take the photos in turn and send them as small pictures
        let specimen = self.cv.specimen.clone();
        let items: Vec<((u32, char), PathBuf, i32)> = self.cv.cubes.iter().filter_map(|c| c.photo.clone().map(|p| ((c.slice, c.letter), p, core_view::photo_turn(&specimen, c.letter, c.slice)))).collect();
        self.cv.total = items.len();
        self.cv.loaded = 0;
        let (tx, rx) = channel();
        self.cv.rx = Some(rx);
        let items = Arc::new(items);
        let next = Arc::new(AtomicUsize::new(0));
        for _ in 0..4 {
            let (items, next, tx) = (items.clone(), next.clone(), tx.clone());
            std::thread::spawn(move || loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some((key, path, turn)) = items.get(i) else { break };
                let Some(img) = open_photo(path) else { continue };
                let mut rgba = img.thumbnail(TEX_PX, TEX_PX).to_rgba8();
                rgba = match turn.rem_euclid(360) {
                    90 => image::imageops::rotate270(&rgba),
                    180 => image::imageops::rotate180(&rgba),
                    270 => image::imageops::rotate90(&rgba),
                    _ => rgba,
                };
                let ci = ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                if tx.send((*key, ci)).is_err() {
                    break;
                }
            });
        }
    }

    /// The layout of the cubes for the open file (names of its records, the photos found).
    fn cv_layout(&mut self) {
        let Some(d) = &self.doc else { return };
        let names: Vec<String> = d.specs.iter().map(|s| s.name.clone()).collect();
        self.cv.cubes = core_view::build(&self.cv.specimen, &names, &self.cv.photos);
        self.cv.for_file = Some((d.path.clone(), d.specs.len()));
    }

    /// Every frame: new photos become textures, a new file gets its link (or a search), the search ends.
    pub(super) fn cv_poll(&mut self, ctx: &egui::Context) {
        if !self.cv.on {
            return;
        }
        if let Some(rx) = &self.cv.rx {
            let mut n = 0;
            while n < 6 {
                match rx.try_recv() {
                    Ok((key, img)) => {
                        let t = ctx.load_texture(format!("cube-{}{}", key.1, key.0), img, TextureOptions::LINEAR);
                        self.cv.textures.insert(key, t);
                        self.cv.loaded += 1;
                        n += 1;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        ctx.request_repaint_after(std::time::Duration::from_millis(40));
                        break;
                    }
                    Err(_) => {
                        self.cv.rx = None;
                        break;
                    }
                }
            }
            if n == 6 {
                ctx.request_repaint();
            }
        }
        // the search finished
        if let Some(rx) = &self.cv.scan {
            match rx.try_recv() {
                Ok((named, content, sugg)) => {
                    self.cv.scan = None;
                    self.cv.suggestion = sugg;
                    self.cv.found = named;
                    self.cv.by_content = content;
                    self.cv_rebuild();
                    if self.cv.auto {
                        self.cv.auto = false;
                        let id = self.cv_open_id();
                        let top = rank(&id, &self.cv.list).first().copied().filter(|&k| confident(&id, &self.cv.list[k]));
                        match top {
                            Some(k) => {
                                // an established link: kept for the next time
                                let dir = self.cv.list[k].dir.clone();
                                self.cv.links.insert(id, Link::new(&dir, self.cv.root.as_deref()));
                                self.cv_save_links();
                                self.cv_load(dir);
                            }
                            None => {
                                self.cv.note = format!("No cube photos named after {id} were found below the chosen folder.");
                                self.cv.win = true;
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(80)),
                Err(_) => self.cv.scan = None,
            }
        }
        let cur = self.doc.as_ref().filter(|d| !d.synthetic).map(|d| (d.path.clone(), d.specs.len()));
        let Some(cur) = cur else { return };
        if self.cv.dir.is_none() {
            // nothing shown yet (enabled again after a restart, or a specimen without photos): its saved link, else a search
            if cur.0 != self.cv.for_file.as_ref().map(|c| c.0.clone()).unwrap_or_default() || self.cv.for_file.as_ref().map_or(false, |c| c.1 != cur.1) {
                self.cv.specimen = self.cv_open_id();
                self.cv_layout();
            }
            if !self.cv.win && self.cv.scan.is_none() && self.cv.tried.as_ref() != Some(&cur.0) {
                self.cv.tried = Some(cur.0.clone());
                if !self.cv_try_link() {
                    if self.cv.root.is_some() {
                        self.cv_start_scan(true);
                    } else {
                        self.cv_open_window();
                    }
                }
            }
        } else if Some(&cur) != self.cv.for_file.as_ref() {
            let new_file = self.cv.for_file.as_ref().map(|c| &c.0) != Some(&cur.0);
            let id = spec_id(&cur.0);
            if new_file && !id.eq_ignore_ascii_case(&self.cv.specimen) {
                // another specimen: its photos come from its own link (next frame), the old ones go
                self.cv_clear_photos();
                self.cv_layout();
                self.cv.tried = None;
            } else {
                self.cv_layout();
            }
        }
    }

    /// The window: the specimens of the file tree on the left, a link symbol, the cube folder on the right.
    pub(super) fn cv_window(&mut self, ctx: &egui::Context) {
        if !self.cv.win {
            return;
        }
        if self.cv.rows_stale {
            self.cv.rows_stale = false;
            self.cv_rebuild();
        }
        let dark = self.dark;
        let (mut open, mut cancel, mut load, mut choose, mut search) = (true, false, false, false, false);
        let mut use_sugg = false;
        let scr = ctx.content_rect();
        let size = Vec2::new((scr.width() - 40.0).clamp(620.0, 900.0), (scr.height() - 60.0).clamp(520.0, 760.0));
        let scanning = self.cv.scan.is_some();
        let mut needle = self.cv.needle.clone();
        let mut fit_all = self.cv.fit_all;
        let sel = self.cv_selection();
        let mut rows = std::mem::take(&mut self.cv.rows);
        let n_linked = rows.iter().filter(|r| r.on && row_pick(r, &self.cv.list, fit_all || sel.contains(&r.path)).is_some()).count();
        self.base_window(ctx, "Core viewer", W_COREVIEW, size).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.2);
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
            ui.spacing_mut().button_padding = Vec2::new(12.0, 6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Which folder holds the cube photos?").strong().size(19.0));
                info_icon(ui, "Pick a folder; the cube photos are searched for below it, however many folders down they are. A cube folder is a folder whose name contains the string below (cube_crops by default) and that holds photos named like DC26AB002_A12.png (specimen id, cube letter, slice). PNG, TIFF and JPG / JPEG photos are read (EXIF rotation is applied); HEIC / HEIF is not supported because it needs the libheif C library, so convert those photos to PNG or JPG first (one cube in several formats: PNG, then TIFF, then JPG is used). If no folder name contains the string, the folders that hold cube photos are listed and a string is suggested. The photos are only read: they are decoded into memory, nothing is written.");
            });
            ui.horizontal(|ui| {
                choose = ui.add(egui::Button::new("Choose folder ...").min_size(Vec2::new(170.0, 34.0))).clicked();
                ui.label(RichText::new(self.cv.root.as_ref().map(|r| r.display().to_string()).unwrap_or_else(|| "no folder chosen yet".into())).monospace().small());
            });
            ui.label(RichText::new("The name of the cube folder contains:").strong());
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut needle).desired_width(240.0).hint_text(DEFAULT_NEEDLE).margin(egui::Margin::symmetric(8, 6)));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                search = ui.add_enabled(self.cv.root.is_some(), egui::Button::new(RichText::new("Search").strong()).min_size(Vec2::new(100.0, 34.0))).clicked() || enter;
            });
            if scanning {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Looking for cube photos below the folder ...");
                });
            } else if self.cv.root.is_some() {
                if self.cv.list.is_empty() {
                    ui.label(RichText::new("No folder with cube photos was found below this folder.").color(Color32::from_rgb(214, 90, 90)).strong());
                    ui.label(RichText::new("The photos are named like DC26AB002_A12.png (.png, .tif, .tiff, .jpg and .jpeg are read; HEIC is not: convert those photos to PNG or JPG first). Choose a folder above the cube folder.").weak().small());
                } else if self.cv.from_content {
                    ui.label(RichText::new(format!("No folder name contains \u{201c}{}\u{201d}. The folders that hold cube photos are offered instead (n={}).", needle.trim(), self.cv.list.len())).color(Color32::from_rgb(214, 150, 40)).strong());
                    if let Some(s) = &self.cv.suggestion {
                        if use_sugg_button(ui, s) {
                            use_sugg = true;
                        }
                    }
                } else {
                    ui.label(RichText::new(format!("Cube folders found (n={})", self.cv.list.len())).strong());
                }
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut fit_all, RichText::new("Fit all specimens in the file tree at once").strong());
                info_icon(ui, "Off: only the specimens you selected in the file tree (the open file when none is selected) get their closest folder filled in automatically. On: every ticked specimen of the file tree does. A folder is only filled in when the photos are named after the specimen or the folder name holds its id. You can pick any folder for any row yourself; a saved link is kept either way.");
            });
            ui.label(RichText::new(format!("Link each specimen to its cube folder (n={}); untick the ones you do not want", rows.len())).strong());
            let h = (ui.available_height() - 74.0 - if self.cv.note.is_empty() { 0.0 } else { 34.0 }).max(100.0);
            egui::ScrollArea::vertical().max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(RichText::new("No specimens are loaded in the file tree. Open a file or a folder of specimens first.").weak());
                }
                egui::Grid::new("cv_table").num_columns(3).spacing([14.0, 10.0]).striped(true).show(ui, |ui| {
                    for (ri, row) in rows.iter_mut().enumerate() {
                        let eligible = fit_all || sel.contains(&row.path);
                        let pick = row_pick(row, &self.cv.list, eligible);
                        ui.horizontal(|ui| {
                            ui.set_min_width(190.0);
                            ui.checkbox(&mut row.on, "");
                            let t = RichText::new(&row.id).size(15.0);
                            ui.label(if row.on { t.strong() } else { t.weak().strikethrough() });
                        });
                        link_symbol(ui, row.on && pick.is_some());
                        ui.add_enabled_ui(row.on, |ui| {
                            let label_of = |k: usize| {
                                let f = &self.cv.list[k];
                                let pos = row.cands.iter().position(|&c| c == k).unwrap_or(0);
                                let tag = if pos == 0 { "Top suggestion".to_string() } else { format!("Next closest ({})", pos + 1) };
                                let other = if !f.id.eq_ignore_ascii_case(&row.id) { format!(", photos of {}", f.id) } else { String::new() };
                                format!("{tag}: {}  (n={}{other})", folder_name(f), f.n)
                            };
                            let text = match pick {
                                Some(k) => label_of(k),
                                None if self.cv.list.is_empty() => "no cube folder found".to_string(),
                                None => "no link".to_string(),
                            };
                            egui::ComboBox::from_id_salt(("cv-row", ri)).width(470.0).selected_text(text).show_ui(ui, |ui| {
                                if ui.selectable_label(pick.is_none(), "no link").clicked() {
                                    row.manual = Some(None);
                                }
                                for &k in &row.cands {
                                    let f = &self.cv.list[k];
                                    if ui.selectable_label(pick == Some(k), label_of(k)).on_hover_text(&f.rel).clicked() {
                                        row.manual = Some(Some(f.dir.clone()));
                                    }
                                }
                            });
                        });
                        ui.end_row();
                    }
                });
            });
            if !self.cv.note.is_empty() {
                ui.label(RichText::new(&self.cv.note).color(Color32::from_rgb(214, 150, 40)));
            }
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                cancel = ui.add(egui::Button::new("Cancel").min_size(Vec2::new(0.0, 38.0))).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let go = egui::Button::new(RichText::new(format!("Link and load the cube photos (n={n_linked})")).strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(300.0, 38.0));
                    load = ui.add_enabled(n_linked > 0 && !scanning, go).clicked();
                });
            });
        });
        self.cv.rows = rows;
        self.cv.fit_all = fit_all;
        if use_sugg {
            if let Some(s) = self.cv.suggestion.clone() {
                needle = s;
                search = true;
            }
        }
        self.cv.needle = needle;
        if choose {
            let mut dlg = rfd::FileDialog::new().set_title("Folder that holds (or is above) the cube photos");
            if let Some(r) = self.cv.root.as_ref().filter(|r| r.is_dir()) {
                dlg = dlg.set_directory(r);
            }
            // headless test hook: AMS_STUDIO_PICK_DIR names the folder instead of the dialog
            let picked = std::env::var_os("AMS_STUDIO_PICK_DIR").map(PathBuf::from).or_else(|| dlg.pick_folder());
            if let Some(p) = picked {
                self.cv.root = Some(p);
                search = true;
            }
        }
        if search {
            self.cv.note.clear();
            self.cv_start_scan(false);
        }
        if load {
            self.cv_link_and_load();
            return;
        }
        if cancel || !open {
            self.cv_dismiss();
            self.save_state();
        }
    }

    /// Keeps the links of the ticked rows (and drops the ones set to "no link"), then shows the open specimen's photos.
    fn cv_link_and_load(&mut self) {
        let sel = self.cv_selection();
        let root = self.cv.root.clone();
        let mut changes: Vec<(String, Option<Link>)> = vec![];
        for row in self.cv.rows.iter().filter(|r| r.on) {
            match row_pick(row, &self.cv.list, self.cv.fit_all || sel.contains(&row.path)) {
                Some(k) => changes.push((row.id.clone(), Some(Link::new(&self.cv.list[k].dir, root.as_deref())))),
                None if matches!(row.manual, Some(None)) => changes.push((row.id.clone(), None)),
                None => {}
            }
        }
        let mut changed = false;
        for (id, l) in changes {
            match l {
                Some(l) => {
                    if self.cv.links.get(&id) != Some(&l) {
                        self.cv.links.insert(id, l);
                        changed = true;
                    }
                }
                None => changed |= self.cv.links.remove(&id).is_some(),
            }
        }
        if changed {
            self.cv_save_links();
        }
        // rows that were only suggested are links now
        self.cv.rows_stale = true;
        self.cv.win = false;
        self.cv.note.clear();
        self.cv.tried = self.doc.as_ref().filter(|d| !d.synthetic).map(|d| d.path.clone());
        if !self.cv_try_link() {
            self.cv_clear_photos();
            self.cv_layout();
        }
        self.save_state();
    }

    /// The pane itself, above the plots.
    pub(super) fn cv_pane(&mut self, ui: &mut Ui, rect: Rect, th: &crate::plots::Theme) -> crate::plots::PlotOut {
        let visuals = ui.visuals().clone();
        ui.painter().rect(rect, 8.0, if th.dark { th.canvas() } else { visuals.extreme_bg_color }, Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
        let header = Rect::from_min_size(rect.min + Vec2::new(6.0, 4.0), Vec2::new(rect.width() - 12.0, 24.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(header).layout(Layout::left_to_right(Align::Center)));
        let (mut change, mut reset, mut close) = (false, false, false);
        child.menu_button("\u{25b8}", |ui| {
            if ui.button("Link cube folders ...").clicked() {
                change = true;
                ui.close();
            }
            if ui.button("Reset zoom").clicked() {
                reset = true;
                ui.close();
            }
            if ui.button("Close the core viewer").clicked() {
                close = true;
                ui.close();
            }
        });
        let title = if self.cv.hover.is_empty() { format!("Core viewer {}", self.cv.specimen) } else { self.cv.hover.clone() };
        let loading = self.cv.total > 0 && self.cv.loaded < self.cv.total;
        let right = if loading {
            format!("loading photos (n={} of {})", self.cv.loaded, self.cv.total)
        } else if self.cv.dir.is_none() {
            "no cube folder linked".to_string()
        } else {
            format!("cubes (n={}), photos (n={})", self.cv.cubes.len(), self.cv.photos.len())
        };
        child.add_sized([(header.width() - 34.0 - 190.0).max(40.0), 18.0], egui::Label::new(if self.cv.hover.is_empty() { RichText::new(title).weak() } else { RichText::new(title) }).truncate());
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(right).weak().small());
        });
        if change {
            self.cv_open_window();
        }
        if reset {
            self.cv.view.reset();
        }
        if close {
            self.cv_toggle();
        }
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 4.0, header.max.y + 2.0), Pos2::new(rect.max.x - 4.0, rect.max.y - 4.0));
        let Some(d) = &self.doc else { return Default::default() };
        let out = core_view::draw(ui, body, &self.cv.cubes, &self.cv.textures, &d.selected, &d.active, &mut self.cv.view, th, &self.cv.specimen);
        if self.cv.dir.is_none() && !self.cv.win && self.cv.scan.is_none() {
            ui.painter().text(body.center(), egui::Align2::CENTER_CENTER, format!("No cube photos are linked to {}.   \u{25b8} Link cube folders ...", self.cv_open_id()), egui::FontId::proportional(14.0), th.ink());
        }
        self.cv.hover = out.hover.clone().unwrap_or_default();
        out
    }
}

fn use_sugg_button(ui: &mut Ui, s: &str) -> bool {
    ui.add(egui::Button::new(format!("Search for \u{201c}{s}\u{201d} instead")).min_size(Vec2::new(0.0, 32.0))).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(dir: &str, id: &str, n: usize) -> Found {
        Found { dir: PathBuf::from(dir), rel: dir.to_string(), n, id: id.to_string() }
    }

    #[test]
    fn the_folder_of_the_specimen_is_the_top_suggestion_and_the_neighbours_follow() {
        let list = vec![found("/s/DC26AB012_stitch/DC26AB012_cube_crops", "DC26AB012", 80), found("/s/DC26AB003_stitch/DC26AB003_cube_crops", "DC26AB003", 80), found("/s/DC26AB002_stitch/DC26AB002_cube_crops", "DC26AB002", 89)];
        let r = rank("DC26AB002", &list);
        assert_eq!(r, vec![2, 1, 0]); // exact, then DC26AB003 (number 1 away), then DC26AB012 (10 away)
        assert!(confident("DC26AB002", &list[2]));
        assert!(!confident("DC26AB002", &list[1]));
        // an id with no folder of its own: nothing is sure enough for the automatic matching
        assert!(!confident("DC26AB099", &list[r_top("DC26AB099", &list)]));
    }

    fn r_top(id: &str, list: &[Found]) -> usize {
        rank(id, list)[0]
    }

    #[test]
    fn a_row_takes_its_choice_then_its_saved_link_then_the_automatic_top_folder() {
        let list = vec![found("/a", "X1", 3), found("/b", "X2", 3)];
        let mut row = CvRow { path: PathBuf::from("X1_use.ams"), id: "X1".into(), on: true, cands: vec![0, 1], top_ok: true, manual: None, saved: None };
        assert_eq!(row_pick(&row, &list, false), None); // not selected, not fit-all: left alone
        assert_eq!(row_pick(&row, &list, true), Some(0)); // automatic: top folder
        row.saved = Some(PathBuf::from("/b"));
        assert_eq!(row_pick(&row, &list, true), Some(1)); // the saved link wins over the automatic one
        row.manual = Some(None);
        assert_eq!(row_pick(&row, &list, true), None); // "no link" chosen by hand
        row.manual = Some(Some(PathBuf::from("/a")));
        assert_eq!(row_pick(&row, &list, false), Some(0));
        row.manual = None;
        row.saved = None;
        row.top_ok = false;
        assert_eq!(row_pick(&row, &list, true), None); // closest folder not sure enough
    }

    #[test]
    fn png_tiff_jpg_and_jpeg_are_photos_heic_is_not() {
        for (n, ok) in [("a.png", true), ("a.PNG", true), ("a.tif", true), ("a.TIFF", true), ("a.jpg", true), ("a.JPEG", true), ("a.heic", false), ("a.txt", false)] {
            assert_eq!(photo_rank(Path::new(n)).is_some(), ok, "{n}");
        }
    }

    #[test]
    fn one_photo_per_cube_in_the_preferred_format_and_every_format_decodes() {
        let dir = std::env::temp_dir().join(format!("cv-photos-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = image::RgbImage::from_pixel(8, 6, image::Rgb([200, 30, 30]));
        for n in ["DC26AB999_A1.jpg", "DC26AB999_A1.png", "DC26AB999_B1.JPEG", "DC26AB999_A2.tiff", "DC26AB999_C3.heic"] {
            if n.ends_with("heic") {
                std::fs::write(dir.join(n), b"x").unwrap();
            } else {
                img.save(dir.join(n)).unwrap();
            }
        }
        let mut got = photos_in(&dir);
        got.sort_by(|a, b| (a.1, a.2).cmp(&(b.1, b.2)));
        let names: Vec<String> = got.iter().map(|g| g.3.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, ["DC26AB999_A1.png", "DC26AB999_A2.tiff", "DC26AB999_B1.JPEG"]);
        for g in &got {
            let d = open_photo(&g.3).expect("decodes");
            assert_eq!((d.width(), d.height()), (8, 6));
        }
        assert_eq!(scan(&dir, "").1.first().map(|f| f.n), Some(3));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

