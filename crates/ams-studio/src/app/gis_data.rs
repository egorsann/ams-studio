//! The data side of the GIS viewer: OpenStreetMap tiles (fetched on worker threads, cached on disk), map images
//! (png / jpg / GeoTIFF, with the georeference read from the file, a world file or a saved sidecar) and the
//! coordinates table (csv / xlsx) with the columns the user assigned.

use ams_core::gis::{self, Affine, ControlPoint, Crs, Georef, MatchMode};
use egui::{ColorImage, TextureHandle, TextureOptions};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// OpenStreetMap asks automated clients to identify themselves; nothing about the user or the project goes in here.
const USER_AGENT: &str = "AMS-Studio/0.2 (desktop AMS viewer)";
/// Tiles younger than this are used from the disk cache without asking the server (the OSM policy asks for 7 days).
const CACHE_DAYS: u64 = 14;
const MAX_SIDE: u32 = 8192;

// ------------------------------------------------------------------------------------------------ tiles

pub type TileKey = (u32, u32, u32);

/// Where the tile cache lives: $XDG_CACHE_HOME or ~/.cache on Linux, %LOCALAPPDATA% on Windows.
pub fn cache_dir() -> PathBuf {
    let base = if let Some(x) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(x)
    } else if let Some(l) = cfg!(windows).then(|| std::env::var_os("LOCALAPPDATA")).flatten() {
        PathBuf::from(l)
    } else {
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default().join(".cache")
    };
    base.join("ams-studio").join("tiles")
}

enum TileState {
    Loading,
    Ready(TextureHandle),
    Failed(Instant),
}

struct TileEntry {
    state: TileState,
    used: u64,
}

type TileMsg = (TileKey, Result<ColorImage, String>);

pub struct Tiles {
    entries: HashMap<TileKey, TileEntry>,
    queue: VecDeque<TileKey>,
    inflight: usize,
    tx: Sender<TileMsg>,
    rx: Receiver<TileMsg>,
    frame: u64,
    /// The last network problem, shown in the window.
    pub error: Option<String>,
    pub fetched: usize,
}

impl Default for Tiles {
    fn default() -> Self {
        let (tx, rx) = channel();
        Tiles { entries: HashMap::new(), queue: VecDeque::new(), inflight: 0, tx, rx, frame: 0, error: None, fetched: 0 }
    }
}

fn fetch_tile(key: TileKey) -> Result<ColorImage, String> {
    let (z, x, y) = key;
    let path = cache_dir().join(z.to_string()).join(x.to_string()).join(format!("{y}.png"));
    let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < Duration::from_secs(CACHE_DAYS * 86_400));
    let bytes = match (fresh, std::fs::read(&path)) {
        (true, Ok(b)) => b,
        _ => {
            let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(12))).user_agent(USER_AGENT).build().into();
            let got = agent.get(&format!("https://tile.openstreetmap.org/{z}/{x}/{y}.png")).call().map_err(|e| e.to_string()).and_then(|mut r| r.body_mut().read_to_vec().map_err(|e| e.to_string()));
            match got {
                Ok(b) => {
                    if let Some(d) = path.parent() {
                        let _ = std::fs::create_dir_all(d);
                    }
                    let _ = std::fs::write(&path, &b);
                    b
                }
                // an old cached tile is better than none when the network is down
                Err(e) => match std::fs::read(&path) {
                    Ok(b) => b,
                    Err(_) => return Err(e),
                },
            }
        }
    };
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

impl Tiles {
    /// Call once per frame before the tiles are asked for: takes in what the workers finished and forgets the
    /// requests of the last frame that were not started.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        self.queue.clear();
        while let Ok((key, res)) = self.rx.try_recv() {
            self.inflight = self.inflight.saturating_sub(1);
            let used = self.frame;
            match res {
                Ok(img) => {
                    let t = ctx.load_texture(format!("osm-{}-{}-{}", key.0, key.1, key.2), img, TextureOptions::LINEAR);
                    self.entries.insert(key, TileEntry { state: TileState::Ready(t), used });
                    self.error = None;
                    self.fetched += 1;
                }
                Err(e) => {
                    self.error = Some(e);
                    self.entries.insert(key, TileEntry { state: TileState::Failed(Instant::now()), used });
                }
            }
        }
        // keep the memory in check: drop tiles that were not drawn for a while
        if self.entries.len() > 320 {
            let frame = self.frame;
            self.entries.retain(|_, e| frame.saturating_sub(e.used) < 3 || matches!(e.state, TileState::Loading));
        }
    }

    /// The texture of a tile if it is ready; otherwise it is queued (once the frame is over `start` runs it).
    pub fn get(&mut self, key: TileKey) -> Option<&TextureHandle> {
        let frame = self.frame;
        let n = 1u32 << key.0;
        if key.1 >= n || key.2 >= n {
            return None;
        }
        let retry = matches!(self.entries.get(&key), Some(TileEntry { state: TileState::Failed(t), .. }) if t.elapsed() > Duration::from_secs(20));
        if retry {
            self.entries.remove(&key);
        }
        match self.entries.get_mut(&key) {
            Some(e) => {
                e.used = frame;
            }
            None => {
                self.queue.push_back(key);
                return None;
            }
        }
        match &self.entries[&key].state {
            TileState::Ready(t) => Some(t),
            _ => None,
        }
    }

    /// The texture of a tile only if it is already there (nothing is requested).
    pub fn peek(&self, key: TileKey) -> Option<&TextureHandle> {
        match self.entries.get(&key) {
            Some(TileEntry { state: TileState::Ready(t), .. }) => Some(t),
            _ => None,
        }
    }

    /// Starts the queued tiles, two at a time.
    pub fn start(&mut self, ctx: &egui::Context) {
        while self.inflight < 2 {
            let Some(key) = self.queue.pop_front() else { break };
            if self.entries.contains_key(&key) {
                continue;
            }
            self.entries.insert(key, TileEntry { state: TileState::Loading, used: self.frame });
            self.inflight += 1;
            let (tx, ctx2) = (self.tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let r = fetch_tile(key);
                let _ = tx.send((key, r));
                ctx2.request_repaint();
            });
        }
        if self.inflight > 0 {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }

    pub fn busy(&self) -> bool {
        self.inflight > 0 || !self.queue.is_empty()
    }
}

// ------------------------------------------------------------------------------------------------ map images

/// A map image read from disk, with its georeference when the file (or a saved sidecar) has one.
pub struct LoadedMap {
    pub path: PathBuf,
    /// Size of the file's image in pixels (the georeference is in these pixels).
    pub orig: (u32, u32),
    pub image: ColorImage,
    pub georef: Option<Georef>,
    /// A world file was found but not what coordinate system it is in: the transform waits for the user's answer.
    pub pending: Option<Affine>,
    pub note: String,
    /// The control points saved with a sidecar (pixel, Web Mercator metres).
    pub points: Vec<ControlPoint>,
}

pub fn sidecar_path(map: &Path) -> PathBuf {
    let mut s = map.as_os_str().to_os_string();
    s.push(".georef.json");
    PathBuf::from(s)
}

pub fn crs_json(c: &Crs) -> serde_json::Value {
    match *c {
        Crs::LonLat => serde_json::json!({"type": "lonlat"}),
        Crs::WebMerc => serde_json::json!({"type": "webmerc"}),
        Crs::Utm { zone, north } => serde_json::json!({"type": "utm", "zone": zone, "north": north}),
        Crs::Epsg(code) => serde_json::json!({"type": "epsg", "code": code}),
        Crs::Custom(_) => serde_json::json!({"type": "proj", "definition": c.custom_definition().unwrap_or_default()}),
    }
}

pub fn crs_from_json(v: &serde_json::Value) -> Option<Crs> {
    match v.get("type")?.as_str()? {
        "lonlat" => Some(Crs::LonLat),
        "webmerc" => Some(Crs::WebMerc),
        "utm" => Some(Crs::Utm { zone: v.get("zone")?.as_u64()? as u8, north: v.get("north")?.as_bool()? }),
        "epsg" => Crs::from_epsg(u32::try_from(v.get("code")?.as_u64()?).ok()?),
        "proj" => Crs::from_definition(v.get("definition")?.as_str()?).ok(),
        _ => None,
    }
}

/// The georeference of a map on one line (kept in the preferences for a map whose georeference is not in a sidecar).
pub fn georef_line(g: &Georef, size: (u32, u32), points: &[ControlPoint]) -> String {
    serde_json::from_str::<serde_json::Value>(&sidecar_text(g, size, points)).and_then(|v| serde_json::to_string(&v)).unwrap_or_default()
}

/// The text of a sidecar: the georeference of a map and the control points it was made from.
pub fn sidecar_text(g: &Georef, size: (u32, u32), points: &[ControlPoint]) -> String {
    let a = g.affine;
    let pts: Vec<_> = points
        .iter()
        .map(|((px, py), (mx, my))| {
            let (lon, lat) = gis::lonlat_from_merc(*mx, *my);
            serde_json::json!({"pixel_x": px, "pixel_y": py, "lon": lon, "lat": lat})
        })
        .collect();
    let v = serde_json::json!({
        "kind": "AMS Studio map georeference", "version": 1,
        "note": "Pixel -> coordinates of crs: X = a px + b py + c, Y = d px + e py + f (pixel corner grid of the full-size image). Made by the GIS viewer; delete this file to forget the georeference.",
        "image_size": [size.0, size.1], "crs": crs_json(&g.crs), "affine": [a.a, a.b, a.c, a.d, a.e, a.f], "points": pts,
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

pub fn parse_sidecar(text: &str, size: (u32, u32)) -> Result<(Georef, Vec<ControlPoint>), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if let Some(s) = v.get("image_size").and_then(|s| s.as_array()) {
        if s.len() == 2 && (s[0].as_u64() != Some(u64::from(size.0)) || s[1].as_u64() != Some(u64::from(size.1))) {
            return Err("the saved georeference belongs to an image of another size".into());
        }
    }
    let crs = v.get("crs").and_then(crs_from_json).ok_or("no coordinate system in the saved georeference")?;
    let a: Vec<f64> = v.get("affine").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default();
    if a.len() != 6 {
        return Err("no transform in the saved georeference".into());
    }
    let affine = Affine { a: a[0], b: a[1], c: a[2], d: a[3], e: a[4], f: a[5] };
    affine.inverse().ok_or("the saved transform is flat")?;
    let mut pts = vec![];
    for p in v.get("points").and_then(|p| p.as_array()).into_iter().flatten() {
        let g = |k: &str| p.get(k).and_then(|x| x.as_f64());
        if let (Some(px), Some(py), Some(lon), Some(lat)) = (g("pixel_x"), g("pixel_y"), g("lon"), g("lat")) {
            pts.push(((px, py), gis::merc_from_lonlat(lon, lat)));
        }
    }
    Ok((Georef { affine, crs }, pts))
}

/// The georeference of a GeoTIFF: transform, and the coordinate system when it can be told.
fn geotiff(path: &Path) -> Result<Option<(Affine, Result<Crs, String>)>, String> {
    use tiff::decoder::Decoder;
    use tiff::tags::Tag;
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut d = Decoder::new(std::io::BufReader::new(f)).map_err(|e| e.to_string())?;
    let scale = d.get_tag_f64_vec(Tag::ModelPixelScaleTag).ok();
    let tie = d.get_tag_f64_vec(Tag::ModelTiepointTag).ok();
    let tr = d.get_tag_f64_vec(Tag::ModelTransformationTag).ok();
    let keys = d.get_tag_u16_vec(Tag::GeoKeyDirectoryTag).ok();
    let mut affine = if let Some(t) = tr.filter(|t| t.len() >= 16) {
        Affine { a: t[0], b: t[1], c: t[3], d: t[4], e: t[5], f: t[7] }
    } else if let (Some(s), Some(t)) = (scale.filter(|s| s.len() >= 2), tie.filter(|t| t.len() >= 6)) {
        Affine { a: s[0], b: 0.0, c: t[3] - t[0] * s[0], d: 0.0, e: -s[1], f: t[4] + t[1] * s[1] }
    } else {
        return Ok(None);
    };
    let Some(keys) = keys.filter(|k| k.len() >= 4) else { return Ok(Some((affine, Err("the GeoTIFF names no coordinate system".into())))) };
    let (mut projected, mut geographic, mut model, mut raster) = (None, None, None, 1u16);
    for e in keys[4..].chunks(4).take(keys[3] as usize) {
        if e.len() < 4 {
            break;
        }
        let val = if e[1] == 0 { Some(e[3]) } else { None };
        match e[0] {
            3072 => projected = val,
            2048 => geographic = val,
            1024 => model = val,
            1025 => raster = val.unwrap_or(1),
            _ => {}
        }
    }
    if raster == 2 {
        // pixel-is-point: the tie point is the pixel centre, the transform here wants the corner
        affine.c -= 0.5 * affine.a + 0.5 * affine.b;
        affine.f -= 0.5 * affine.d + 0.5 * affine.e;
    }
    let code = match (model, projected, geographic) {
        (Some(2), _, Some(g)) => Some(u32::from(g)),
        (_, Some(p), _) => Some(u32::from(p)),
        (_, _, Some(g)) => Some(u32::from(g)),
        _ => None,
    };
    let crs = match code {
        Some(c) if c != 32767 => Crs::from_epsg(c).ok_or_else(|| format!("the GeoTIFF is in EPSG:{c}, which AMS Studio does not support (use Georeference to place the map with points)")),
        Some(_) => Err("the GeoTIFF uses a user-defined coordinate system, which AMS Studio does not support".into()),
        None if model == Some(2) => Ok(Crs::LonLat),
        None => Err("the GeoTIFF names no coordinate system".into()),
    };
    Ok(Some((affine, crs)))
}

/// The world file next to an image, with the coordinate system of a `.prj` when there is one.
fn world_file(path: &Path) -> Option<(Affine, Option<Crs>)> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    let short: &[&str] = match ext.as_str() {
        "png" => &["pgw", "pngw"],
        "jpg" | "jpeg" => &["jgw", "jpgw", "jpegw"],
        "tif" | "tiff" => &["tfw", "tifw", "tiffw"],
        _ => &[],
    };
    for e in short.iter().copied().chain(["wld"]) {
        let w = path.with_extension(e);
        if let Ok(text) = std::fs::read_to_string(&w) {
            if let Some(a) = gis::parse_world_file(&text) {
                let crs = std::fs::read_to_string(path.with_extension("prj")).ok().and_then(|t| Crs::from_prj(&t));
                return Some((a, crs));
            }
        }
    }
    None
}

/// Reads a map image and whatever georeference comes with it.
pub fn load_map(path: &Path) -> Result<LoadedMap, String> {
    let mut reader = image::ImageReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?.with_guessed_format().map_err(|e| e.to_string())?;
    reader.no_limits();
    let img = reader.decode().map_err(|e| format!("Could not read the image: {e}"))?;
    let (w, h) = (img.width(), img.height());
    let rgba = img.to_rgba8();
    drop(img);
    let rgba = if w.max(h) > MAX_SIDE {
        let k = f64::from(MAX_SIDE) / f64::from(w.max(h));
        image::imageops::resize(&rgba, ((f64::from(w) * k).round() as u32).max(1), ((f64::from(h) * k).round() as u32).max(1), image::imageops::FilterType::Triangle)
    } else {
        rgba
    };
    let image = ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
    let mut m = LoadedMap { path: path.to_path_buf(), orig: (w, h), image, georef: None, pending: None, note: String::new(), points: vec![] };

    // 1. a georeference saved by the GIS viewer
    if let Ok(t) = std::fs::read_to_string(sidecar_path(path)) {
        match parse_sidecar(&t, (w, h)) {
            Ok((g, pts)) => {
                m.georef = Some(g);
                m.points = pts;
                m.note = "Georeference loaded from the file saved next to the map.".into();
                return Ok(m);
            }
            Err(e) => m.note = format!("Saved georeference ignored: {e}. "),
        }
    }
    // 2. GeoTIFF tags
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "tif" || ext == "tiff" {
        match geotiff(path) {
            Ok(Some((a, Ok(crs)))) => {
                m.georef = Some(Georef { affine: a, crs });
                m.note += &format!("GeoTIFF georeference ({}).", crs.label());
                return Ok(m);
            }
            Ok(Some((a, Err(why)))) => {
                // the transform is there; a world file's .prj may still give the system, otherwise the user chooses
                m.pending = Some(a);
                m.note += &format!("{why}. ");
            }
            Ok(None) => {}
            Err(e) => m.note += &format!("GeoTIFF tags not readable ({e}). "),
        }
    }
    // 3. world file
    if m.pending.is_none() {
        if let Some((a, crs)) = world_file(path) {
            match crs {
                Some(c) => {
                    m.georef = Some(Georef { affine: a, crs: c });
                    m.note += &format!("World file georeference ({}).", c.label());
                }
                None => {
                    m.pending = Some(a);
                    m.note += "A world file was found, but not its coordinate system. ";
                }
            }
        }
    }
    if m.georef.is_none() && m.pending.is_none() {
        m.note += "No georeference found: use Georeference to place the map with points.";
    }
    Ok(m)
}

// ------------------------------------------------------------------------------------------------ the coordinates table

#[derive(Clone)]
pub struct TableFile {
    pub path: PathBuf,
    pub sheets: Vec<(String, Vec<Vec<String>>)>,
    pub sheet: usize,
}

impl TableFile {
    pub fn grid(&self) -> &[Vec<String>] {
        self.sheets.get(self.sheet).map_or(&[], |s| &s.1)
    }

    pub fn n_cols(&self) -> usize {
        self.grid().iter().map(|r| r.len()).max().unwrap_or(0)
    }
}

/// Reads a csv or every sheet of a spreadsheet as grids of text.
pub fn read_table(path: &Path) -> Result<TableFile, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let sheets = if matches!(ext.as_str(), "csv" | "txt" | "tsv" | "dat" | "asc" | "xyz" | "pts") {
        let bytes = std::fs::read(path).map_err(|e| format!("Could not read the file: {e}"))?;
        let text = match String::from_utf8(bytes.clone()) {
            Ok(t) => t,
            // old Excel csv files are Latin-1
            Err(_) => bytes.iter().map(|&b| b as char).collect(),
        };
        vec![("csv".to_string(), gis::grid_from_csv(&text))]
    } else {
        use calamine::{open_workbook_auto, Data, Reader};
        let mut wb = open_workbook_auto(path).map_err(|e| format!("Could not open the spreadsheet: {e}"))?;
        let mut v = vec![];
        for name in wb.sheet_names().to_vec() {
            let Ok(range) = wb.worksheet_range(&name) else { continue };
            let grid: Vec<Vec<String>> = range
                .rows()
                .map(|r| {
                    r.iter()
                        .map(|c| match c {
                            Data::Empty => String::new(),
                            Data::Float(f) => f.to_string(),
                            Data::Int(i) => i.to_string(),
                            other => other.to_string(),
                        })
                        .collect()
                })
                .collect();
            if grid.iter().any(|r| r.iter().any(|c| !c.trim().is_empty())) {
                v.push((name, grid));
            }
        }
        v
    };
    if sheets.is_empty() || sheets.iter().all(|s| s.1.is_empty()) {
        return Err("The file has no rows with content.".into());
    }
    Ok(TableFile { path: path.to_path_buf(), sheets, sheet: 0 })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoordKind {
    LonLat,
    Utm,
    WebMerc,
    /// Any system of the EPSG registry, picked by code or by searching its name.
    Epsg,
    /// A PROJ string.
    Proj,
}

/// A coordinate system as the user picks it in the interface.
#[derive(Clone, PartialEq, Debug)]
pub struct CrsChoice {
    pub kind: CoordKind,
    pub zone: u8,
    pub north: bool,
    pub epsg: String,
    pub search: String,
    pub proj: String,
}

impl Default for CrsChoice {
    fn default() -> Self {
        CrsChoice { kind: CoordKind::LonLat, zone: 38, north: true, epsg: String::new(), search: String::new(), proj: String::new() }
    }
}

impl CrsChoice {
    /// The choice that stands for a system (to show a saved system in the picker).
    pub fn from_crs(c: &Crs) -> CrsChoice {
        let mut ch = CrsChoice::default();
        match *c {
            Crs::LonLat => {}
            Crs::WebMerc => ch.kind = CoordKind::WebMerc,
            Crs::Utm { zone, north } => {
                ch.kind = CoordKind::Utm;
                ch.zone = zone;
                ch.north = north;
            }
            Crs::Epsg(code) => {
                ch.kind = CoordKind::Epsg;
                ch.epsg = code.to_string();
            }
            Crs::Custom(_) => {
                ch.kind = CoordKind::Proj;
                ch.proj = c.custom_definition().unwrap_or_default();
            }
        }
        ch
    }

    /// The system, or what is wrong with the entry.
    pub fn crs(&self) -> Result<Crs, String> {
        match self.kind {
            CoordKind::LonLat => Ok(Crs::LonLat),
            CoordKind::WebMerc => Ok(Crs::WebMerc),
            CoordKind::Utm => Ok(Crs::Utm { zone: self.zone, north: self.north }),
            CoordKind::Epsg => Crs::from_definition(&self.epsg),
            CoordKind::Proj => Crs::from_definition(&self.proj),
        }
    }

    /// Whether x is a longitude and y a latitude in degrees (the interface says Longitude / Latitude then).
    pub fn is_degrees(&self) -> bool {
        match self.kind {
            CoordKind::LonLat => true,
            CoordKind::Epsg | CoordKind::Proj => matches!(self.crs(), Ok(Crs::LonLat)) || self.proj.contains("+proj=longlat"),
            _ => false,
        }
    }
}

/// What the user assigned in the column window.
#[derive(Clone, PartialEq, Debug)]
pub struct Mapping {
    /// 1-based row of the headers, 0 when the table has none.
    pub header_row: usize,
    pub name: Option<usize>,
    /// Longitude or easting.
    pub x: Option<usize>,
    /// Latitude or northing.
    pub y: Option<usize>,
    pub crs: CrsChoice,
    pub mode: MatchMode,
}

impl Default for Mapping {
    fn default() -> Self {
        Mapping { header_row: 1, name: None, x: None, y: None, crs: CrsChoice::default(), mode: MatchMode::StartsWithKey }
    }
}

impl Mapping {
    pub fn crs(&self) -> Result<Crs, String> {
        self.crs.crs()
    }

    /// The assignment on one line, for the preferences.
    pub fn to_line(&self) -> String {
        let kind = match self.crs.kind {
            CoordKind::LonLat => "lonlat",
            CoordKind::Utm => "utm",
            CoordKind::WebMerc => "webmerc",
            CoordKind::Epsg => "epsg",
            CoordKind::Proj => "proj",
        };
        let mode = match self.mode {
            MatchMode::Exact => "exact",
            MatchMode::StartsWithKey => "starts",
            MatchMode::RowOrder => "order",
        };
        serde_json::json!({"header_row": self.header_row, "name": self.name, "x": self.x, "y": self.y, "kind": kind, "zone": self.crs.zone, "north": self.crs.north, "epsg": self.crs.epsg, "proj": self.crs.proj, "mode": mode}).to_string()
    }

    pub fn from_line(text: &str) -> Option<Mapping> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let idx = |k: &str| v.get(k).and_then(|x| x.as_u64()).map(|x| x as usize);
        let kind = match v.get("kind")?.as_str()? {
            "lonlat" => CoordKind::LonLat,
            "utm" => CoordKind::Utm,
            "webmerc" => CoordKind::WebMerc,
            "epsg" => CoordKind::Epsg,
            "proj" => CoordKind::Proj,
            _ => return None,
        };
        let mode = match v.get("mode").and_then(|m| m.as_str()) {
            Some("exact") => MatchMode::Exact,
            Some("order") => MatchMode::RowOrder,
            _ => MatchMode::StartsWithKey,
        };
        let crs = CrsChoice {
            kind,
            zone: v.get("zone").and_then(|z| z.as_u64()).unwrap_or(38).clamp(1, 60) as u8,
            north: v.get("north").and_then(|n| n.as_bool()).unwrap_or(true),
            epsg: v.get("epsg").and_then(|e| e.as_str()).unwrap_or("").to_string(),
            search: String::new(),
            proj: v.get("proj").and_then(|e| e.as_str()).unwrap_or("").to_string(),
        };
        Some(Mapping { header_row: idx("header_row").unwrap_or(1), name: idx("name"), x: idx("x"), y: idx("y"), crs, mode })
    }
}

/// Column letters like a spreadsheet: A, B, ... Z, AA.
pub fn col_letter(mut i: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (i % 26) as u8) as char);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s
}

/// A first guess for the header row and the columns from the header texts.
pub fn guess_mapping(grid: &[Vec<String>]) -> Mapping {
    let mut m = Mapping::default();
    let looks = |c: &str| {
        let l = c.to_lowercase();
        ["lat", "lon", "east", "north", "name", "id", "sample", "specimen", "site", "block", "x", "y"].iter().any(|k| l.contains(k))
    };
    let hr = grid.iter().take(15).position(|r| r.iter().filter(|c| !c.trim().is_empty() && looks(c)).count() >= 2);
    let Some(hr) = hr else {
        m.header_row = 0;
        let ncols = grid.iter().map(|r| r.len()).max().unwrap_or(0);
        if ncols == 2 {
            // two columns of numbers: x and y, no names
            m.x = Some(0);
            m.y = Some(1);
            m.mode = MatchMode::RowOrder;
        } else {
            m.name = Some(0);
            m.x = Some(1);
            m.y = Some(2);
        }
        sniff_kind(grid, &mut m);
        return m;
    };
    m.header_row = hr + 1;
    let h: Vec<String> = grid[hr].iter().map(|c| c.to_lowercase()).collect();
    let find = |pred: &dyn Fn(&str) -> bool| h.iter().position(|c| pred(c));
    m.name = find(&|c| ["name", "specimen", "sample", "site", "block", "station"].iter().any(|k| c.contains(k))).or_else(|| find(&|c| c == "id" || c.starts_with("id ") || c.ends_with(" id")));
    let lat = find(&|c| c.starts_with("lat"));
    let lon = find(&|c| c.starts_with("lon") || c == "lng" || c.starts_with("long"));
    let east = find(&|c| c.contains("east") || c == "x" || c.starts_with("x ") || c.starts_with("x("));
    let north = find(&|c| c.contains("north") || c == "y" || c.starts_with("y ") || c.starts_with("y("));
    if let (Some(lo), Some(la)) = (lon, lat) {
        m.x = Some(lo);
        m.y = Some(la);
    } else if let (Some(e), Some(n)) = (east, north) {
        m.x = Some(e);
        m.y = Some(n);
        m.crs.kind = CoordKind::Utm;
    }
    if m.name.is_none() {
        m.name = (0..h.len()).find(|&i| Some(i) != m.x && Some(i) != m.y);
    }
    if m.name.is_none() {
        // no name column (just coordinates): the rows are numbered and matched by their order
        m.mode = MatchMode::RowOrder;
    }
    sniff_kind(grid, &mut m);
    m
}

/// Looks at the first readable pair of numbers: degrees, or metres of a UTM-like system.
fn sniff_kind(grid: &[Vec<String>], m: &mut Mapping) {
    let (Some(cx), Some(cy)) = (m.x, m.y) else { return };
    for row in grid.iter().skip(m.header_row) {
        let g = |c: usize| row.get(c).and_then(|s| gis::parse_angle(s));
        if let (Some(x), Some(y)) = (g(cx), g(cy)) {
            if x.abs() <= 180.0 && y.abs() <= 90.0 {
                m.crs.kind = CoordKind::LonLat;
            } else if (100_000.0..900_000.0).contains(&x) && (0.0..=10_000_000.0).contains(&y) {
                m.crs.kind = CoordKind::Utm;
            }
            return;
        }
    }
}

/// A place from the table.
#[derive(Clone, Debug)]
pub struct Loc {
    pub key: String,
    /// The two coordinates as numbers (longitude / easting, latitude / northing) and as written in the table.
    pub x: f64,
    pub y: f64,
    pub xs: String,
    pub ys: String,
    pub lon: f64,
    pub lat: f64,
    pub merc: (f64, f64),
}

/// The places the table gives with these columns, and what is wrong with the rows that cannot be used.
pub fn build_locs(grid: &[Vec<String>], m: &Mapping) -> (Vec<Loc>, Vec<String>) {
    let (Some(cx), Some(cy)) = (m.x, m.y) else { return (vec![], vec!["Assign the x and y columns.".into()]) };
    let crs = match m.crs() {
        Ok(c) => c,
        Err(e) => return (vec![], vec![e]),
    };
    let (mut locs, mut bad) = (vec![], vec![]);
    for (r, row) in grid.iter().enumerate().skip(m.header_row) {
        if row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let cell = |c: usize| row.get(c).map(|s| s.as_str()).unwrap_or("");
        // without a name column the places are numbered 1, 2, 3 ... in the order of the rows
        let key = match m.name {
            Some(ci) => cell(ci).trim().to_string(),
            None => (locs.len() + 1).to_string(),
        };
        let (x, y) = (gis::parse_angle(cell(cx)), gis::parse_angle(cell(cy)));
        let mut push_bad = |why: &str| {
            if bad.len() < 200 {
                bad.push(format!("row {}: {why}", r + 1));
            }
        };
        if key.is_empty() {
            push_bad("no name");
            continue;
        }
        let (Some(x), Some(y)) = (x, y) else {
            push_bad(&format!("coordinates not readable ({:?}, {:?})", cell(cx), cell(cy)));
            continue;
        };
        let (lon, lat) = crs.to_lonlat(x, y);
        let valid = match m.crs.kind {
            CoordKind::LonLat => x.abs() <= 180.0 && y.abs() <= 90.0,
            CoordKind::Utm => (100_000.0..900_000.0).contains(&x) && (0.0..=10_000_000.0).contains(&y),
            CoordKind::WebMerc => x.abs() <= gis::WORLD / 2.0 && y.abs() <= gis::WORLD / 2.0,
            _ => true,
        };
        if !valid || !lon.is_finite() || !lat.is_finite() || lat.abs() > gis::MAX_LAT {
            push_bad(&if m.crs.kind == CoordKind::LonLat && x.abs() <= 90.0 && y.abs() > 90.0 { "latitude and longitude look swapped".to_string() } else { format!("coordinates outside the range of {}", crs.label()) });
            continue;
        }
        locs.push(Loc { key, x, y, xs: cell(cx).trim().to_string(), ys: cell(cy).trim().to_string(), lon, lat, merc: gis::merc_from_lonlat(lon, lat) });
    }
    (locs, bad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect()
    }

    #[test]
    fn columns_are_guessed_from_the_headers() {
        let g = grid(&[&["Field notes"], &["Block", "Latitude", "Longitude", "Elev"], &["B1", "56,34", "-2,8", "10"]]);
        let m = guess_mapping(&g);
        assert_eq!((m.header_row, m.name, m.x, m.y, m.crs.kind), (2, Some(0), Some(2), Some(1), CoordKind::LonLat));
        let g = grid(&[&["ID", "Easting", "Northing"], &["B1", "500000", "5600000"]]);
        let m = guess_mapping(&g);
        assert_eq!((m.header_row, m.name, m.x, m.y, m.crs.kind), (1, Some(0), Some(1), Some(2), CoordKind::Utm));
        // no header at all: the first three columns
        let g = grid(&[&["B1", "56.3", "-2.8"]]);
        let m = guess_mapping(&g);
        assert_eq!((m.header_row, m.name, m.x, m.y), (0, Some(0), Some(1), Some(2)));
    }

    #[test]
    fn locations_are_read_with_the_chosen_columns_and_bad_rows_are_explained() {
        let g = grid(&[&["Block", "Lat", "Lon"], &["B1", "56,34", "-2,8"], &["B2", "x", "1"], &["", "1", "1"], &["B3", "-2.8", "56.34"], &["B4", "56°20'24\"N", "2°48'00\"W"]]);
        let mut m = guess_mapping(&g);
        // B3 is in the southern hemisphere at longitude 56.34: both numbers are in range, so it is accepted as typed;
        // a really swapped one has |lat| > 90
        let (locs, bad) = build_locs(&g, &m);
        assert_eq!(locs.len(), 3, "{bad:?}");
        assert_eq!(bad.len(), 2);
        assert!((locs[0].lat - 56.34).abs() < 1e-9 && (locs[0].lon + 2.8).abs() < 1e-9);
        assert!((locs[1].lat + 2.8).abs() < 1e-9 && (locs[1].lon - 56.34).abs() < 1e-9);
        assert!((locs[2].lat - 56.34).abs() < 1e-9 && (locs[2].lon + 2.8).abs() < 1e-9);
        let g2 = grid(&[&["Block", "Lat", "Lon"], &["B1", "120", "10"]]);
        m.header_row = 1;
        let (l, b) = build_locs(&g2, &m);
        assert!(l.is_empty() && b[0].contains("swapped"), "{b:?}");
        // UTM
        let g3 = grid(&[&["ID", "E", "N"], &["B1", "500000", "5600000"]]);
        let mut m3 = Mapping { header_row: 1, name: Some(0), x: Some(1), y: Some(2), crs: CrsChoice { kind: CoordKind::Utm, zone: 42, ..Default::default() }, ..Default::default() };
        let (l, _) = build_locs(&g3, &m3);
        assert!((l[0].lon - 69.0).abs() < 1e-6 && l[0].lat > 50.0);
        m3.crs.zone = 41;
        assert!((build_locs(&g3, &m3).0[0].lon - 63.0).abs() < 1e-6);
        // missing assignment
        assert!(build_locs(&g3, &Mapping::default()).0.is_empty());
    }

    #[test]
    fn a_table_of_only_coordinates_is_numbered_by_row_and_read_as_utm() {
        let g = grid(&[&["EAST", "NORTH"], &["412345.678", "4851234.567"], &["412340.512", "4851437.431"], &["412545.884", "4851240.303"]]);
        let m = guess_mapping(&g);
        assert_eq!((m.header_row, m.name, m.x, m.y, m.crs.kind, m.mode), (1, None, Some(0), Some(1), CoordKind::Utm, MatchMode::RowOrder));
        let mut m = m;
        m.crs.zone = 33;
        let (locs, bad) = build_locs(&g, &m);
        assert!(bad.is_empty() && locs.len() == 3, "{bad:?}");
        assert_eq!(locs.iter().map(|l| l.key.as_str()).collect::<Vec<_>>(), ["1", "2", "3"]);
        let g = grid(&[&["412345.678", "4851234.567"], &["412340.512", "4851437.431"]]);
        let m = guess_mapping(&g);
        assert_eq!((m.header_row, m.name, m.x, m.y, m.crs.kind), (0, None, Some(0), Some(1), CoordKind::Utm));
        let g = grid(&[&["56.3", "-2.8"], &["56.4", "-2.9"]]);
        assert_eq!(guess_mapping(&g).crs.kind, CoordKind::LonLat);
    }

    #[test]
    fn other_systems_by_epsg_code_and_proj_string() {
        let g = grid(&[&["ID", "X", "Y"], &["B1", "12500000", "5600000"]]);
        let mut m = Mapping { header_row: 1, name: Some(0), x: Some(1), y: Some(2), crs: CrsChoice { kind: CoordKind::Epsg, epsg: "28412".into(), ..Default::default() }, ..Default::default() };
        let (locs, bad) = build_locs(&g, &m);
        assert!(bad.is_empty() && (locs[0].lon - 69.0).abs() < 0.01, "{bad:?}");
        m.crs.epsg = "123".into();
        let (locs, bad) = build_locs(&g, &m);
        assert!(locs.is_empty() && bad[0].contains("EPSG:123"), "{bad:?}");
        m.crs = CrsChoice { kind: CoordKind::Proj, proj: "+proj=utm +zone=42 +datum=WGS84".into(), ..Default::default() };
        let g2 = grid(&[&["ID", "X", "Y"], &["B1", "500000", "5600000"]]);
        assert!((build_locs(&g2, &m).0[0].lon - 69.0).abs() < 1e-6);
    }

    #[test]
    fn a_saved_system_comes_back_through_the_picker_choice() {
        for c in [Crs::LonLat, Crs::WebMerc, Crs::Utm { zone: 44, north: true }, Crs::Utm { zone: 12, north: false }, Crs::Epsg(28412)] {
            assert_eq!(CrsChoice::from_crs(&c).crs(), Ok(c));
        }
        let custom = Crs::from_definition("+proj=utm +zone=43 +datum=WGS84").unwrap();
        assert_eq!(CrsChoice::from_crs(&custom).crs(), Ok(custom));
    }

    #[test]
    fn the_column_assignment_survives_the_preferences_line() {
        let m = Mapping { header_row: 2, name: None, x: Some(3), y: Some(4), crs: CrsChoice { kind: CoordKind::Utm, zone: 43, north: false, epsg: "28412".into(), search: "ignored".into(), proj: String::new() }, mode: MatchMode::RowOrder };
        let back = Mapping::from_line(&m.to_line()).unwrap();
        assert_eq!((back.header_row, back.name, back.x, back.y, back.mode), (2, None, Some(3), Some(4), MatchMode::RowOrder));
        assert_eq!((back.crs.kind, back.crs.zone, back.crs.north, back.crs.epsg.as_str()), (CoordKind::Utm, 43, false, "28412"));
        assert!(back.crs.search.is_empty());
        assert!(Mapping::from_line("not json").is_none());
        assert!(Mapping::from_line("{\"kind\": \"nonsense\"}").is_none());
    }

    #[test]
    fn a_georeference_fits_on_one_line_and_comes_back() {
        let g = Georef { affine: Affine { a: 2.0, b: 0.1, c: 100.0, d: 0.1, e: -2.0, f: 5000.0 }, crs: Crs::Utm { zone: 43, north: true } };
        let line = georef_line(&g, (800, 600), &[((1.0, 2.0), gis::merc_from_lonlat(10.5, 47.1))]);
        assert!(!line.contains('\n') && !line.is_empty());
        let (g2, pts) = parse_sidecar(&line, (800, 600)).unwrap();
        assert_eq!((g2.affine, g2.crs, pts.len()), (g.affine, g.crs, 1));
    }

    #[test]
    fn column_letters() {
        assert_eq!([col_letter(0), col_letter(25), col_letter(26), col_letter(27), col_letter(701), col_letter(702)], ["A", "Z", "AA", "AB", "ZZ", "AAA"]);
    }

    #[test]
    fn sidecar_round_trip_and_size_check() {
        let g = Georef { affine: Affine { a: 2.0, b: 0.1, c: 100.0, d: 0.1, e: -2.0, f: 5000.0 }, crs: Crs::Utm { zone: 42, north: true } };
        let pts: Vec<ControlPoint> = vec![((10.0, 20.0), gis::merc_from_lonlat(69.1, 50.2)), ((300.5, 400.25), gis::merc_from_lonlat(69.3, 50.1))];
        let t = sidecar_text(&g, (800, 600), &pts);
        let (g2, p2) = parse_sidecar(&t, (800, 600)).unwrap();
        assert_eq!(g2.affine, g.affine);
        assert_eq!(g2.crs, g.crs);
        assert_eq!(p2.len(), 2);
        assert!((p2[1].0 .0 - 300.5).abs() < 1e-12 && (p2[1].1 .0 - pts[1].1 .0).abs() < 1e-3);
        assert!(parse_sidecar(&t, (801, 600)).is_err());
        assert!(parse_sidecar("{}", (1, 1)).is_err());
    }

    #[test]
    fn a_png_with_a_world_file_and_prj_loads_georeferenced() {
        let dir = std::env::temp_dir().join(format!("gis-map-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("m.png");
        image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 100, 50, 255])).save(&png).unwrap();
        // no world file: nothing, with a hint
        let m = load_map(&png).unwrap();
        assert!(m.georef.is_none() && m.pending.is_none() && m.note.contains("No georeference"));
        // world file without .prj: waits for the coordinate system
        std::fs::write(dir.join("m.pgw"), "10\n0\n0\n-10\n500005\n5599995\n").unwrap();
        let m = load_map(&png).unwrap();
        assert!(m.georef.is_none() && m.pending.is_some());
        // with a .prj
        std::fs::write(dir.join("m.prj"), "PROJCS[\"WGS_1984_UTM_Zone_42N\",PROJECTION[\"Transverse_Mercator\"]]").unwrap();
        let m = load_map(&png).unwrap();
        let g = m.georef.unwrap();
        assert_eq!(g.crs, Crs::Utm { zone: 42, north: true });
        assert_eq!(g.affine.apply(0.0, 0.0), (500000.0, 5600000.0));
        // a saved sidecar wins
        std::fs::write(sidecar_path(&png), sidecar_text(&Georef { affine: Affine { a: 1.0, b: 0.0, c: 0.0, d: 0.0, e: -1.0, f: 0.0 }, crs: Crs::WebMerc }, (40, 20), &[])).unwrap();
        assert_eq!(load_map(&png).unwrap().georef.unwrap().crs, Crs::WebMerc);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_geotiff_is_read_from_its_tags() {
        // a GeoTIFF is a TIFF with the model tags; write one with the tiff crate
        use tiff::encoder::{colortype, TiffEncoder};
        use tiff::tags::Tag;
        let dir = std::env::temp_dir().join(format!("gis-tif-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("g.tif");
        {
            let f = std::fs::File::create(&p).unwrap();
            let mut enc = TiffEncoder::new(f).unwrap();
            let mut img = enc.new_image::<colortype::RGB8>(4, 3).unwrap();
            img.encoder().write_tag(Tag::ModelPixelScaleTag, &[30.0f64, 30.0, 0.0][..]).unwrap();
            img.encoder().write_tag(Tag::ModelTiepointTag, &[0.0f64, 0.0, 0.0, 600000.0, 5700000.0, 0.0][..]).unwrap();
            // GeoKeyDirectory: version 1.1.0, 2 keys: model type projected, ProjectedCSType 32642
            img.encoder().write_tag(Tag::GeoKeyDirectoryTag, &[1u16, 1, 0, 2, 1024, 0, 1, 1, 3072, 0, 1, 32642][..]).unwrap();
            img.write_data(&[120u8; 4 * 3 * 3]).unwrap();
        }
        let m = load_map(&p).unwrap();
        let g = m.georef.expect(&m.note);
        assert_eq!(g.crs, Crs::Utm { zone: 42, north: true });
        assert_eq!(g.affine.apply(0.0, 0.0), (600000.0, 5700000.0));
        assert_eq!(g.affine.apply(4.0, 3.0), (600120.0, 5699910.0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
