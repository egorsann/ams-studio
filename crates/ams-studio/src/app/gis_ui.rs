//! View > GIS viewer: the AMS fabric of every place of a coordinates table drawn on a map. Strike / dip symbols for
//! the magnetic foliation (the plane perpendicular to K3, the dip beside the tick) and arrows for the lineation (K1,
//! the plunge at the arrow head), on an OpenStreetMap basemap and / or a georeferenced geologic map. The
//! georeferencing window and the coordinates table window are in `gis_georef.rs`.

use super::gis_data::{self as gd, CoordKind, CrsChoice, Loc, Mapping, TableFile, Tiles};
use super::gis_layers::{Kind, LayerTree};
use super::{info_icon, App, W_GIS};
use ams_core::gis::{self, Affine, ControlPoint, Fabric, Georef};
use egui::{epaint::Vertex, Align2, Color32, FontId, Mesh, Pos2, Rect, RichText, Sense, Shape, Stroke, TextureHandle, TextureOptions, Ui, Vec2};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

/// What is on the screen: the Web Mercator point at the centre of the canvas and the metres per point.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub cx: f64,
    pub cy: f64,
    pub mpp: f64,
}

pub const MIN_MPP: f64 = 0.02;
pub const MAX_MPP: f64 = 80_000.0;

impl Default for View {
    fn default() -> Self {
        View::world()
    }
}

impl View {
    pub fn world() -> View {
        View { cx: 0.0, cy: 0.0, mpp: 40_000.0 }
    }

    pub fn to_screen(&self, centre: Pos2, m: (f64, f64)) -> Pos2 {
        Pos2::new(centre.x + ((m.0 - self.cx) / self.mpp) as f32, centre.y - ((m.1 - self.cy) / self.mpp) as f32)
    }

    pub fn to_merc(&self, centre: Pos2, p: Pos2) -> (f64, f64) {
        (self.cx + f64::from(p.x - centre.x) * self.mpp, self.cy - f64::from(p.y - centre.y) * self.mpp)
    }

    /// Zoom by `factor` (> 1 shows more) keeping the point under `p` where it is.
    pub fn zoom_at(&mut self, centre: Pos2, p: Pos2, factor: f64) {
        let before = self.to_merc(centre, p);
        self.mpp = (self.mpp * factor).clamp(MIN_MPP, MAX_MPP);
        let after = self.to_merc(centre, p);
        self.cx += before.0 - after.0;
        self.cy += before.1 - after.1;
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.cx -= f64::from(delta.x) * self.mpp;
        self.cy += f64::from(delta.y) * self.mpp;
    }

    /// The view that shows a box of Web Mercator metres with a margin.
    pub fn fit(min: (f64, f64), max: (f64, f64), size: Vec2) -> View {
        let (w, h) = ((max.0 - min.0).max(30.0), (max.1 - min.1).max(30.0));
        let mpp = (w / f64::from(size.x.max(50.0))).max(h / f64::from(size.y.max(50.0))) * 1.5;
        View { cx: (min.0 + max.0) / 2.0, cy: (min.1 + max.1) / 2.0, mpp: mpp.clamp(MIN_MPP, MAX_MPP) }
    }
}

/// A map image on the map of the viewer.
pub struct MapLayer {
    /// The id of its item in the layer tree.
    pub id: u32,
    pub opacity: f32,
    pub path: PathBuf,
    pub orig: (u32, u32),
    pub tex: TextureHandle,
    pub georef: Option<Georef>,
    /// A world file or GeoTIFF transform that still needs its coordinate system.
    pub pending: Option<Affine>,
    pub note: String,
    pub points: Vec<ControlPoint>,
}

impl MapLayer {
    /// The Web Mercator box the image covers.
    pub fn extent(&self) -> Option<((f64, f64), (f64, f64))> {
        let g = self.georef?;
        let (w, h) = (f64::from(self.orig.0), f64::from(self.orig.1));
        let c: Vec<(f64, f64)> = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].iter().map(|&(x, y)| g.pixel_to_merc(x, y)).collect();
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for p in c {
            lo = (lo.0.min(p.0), lo.1.min(p.1));
            hi = (hi.0.max(p.0), hi.1.max(p.1));
        }
        (lo.0.is_finite() && hi.0.is_finite()).then_some((lo, hi))
    }
}

/// One symbol on the map: a table place with the fabric of its specimen(s).
pub struct Placed {
    pub name: String,
    /// The specimen's main file and how many records went into its mean tensor.
    pub path: PathBuf,
    pub n: usize,
    pub lon: f64,
    pub lat: f64,
    pub merc: (f64, f64),
    pub fabric: Fabric,
}

/// What the preferences know about a map layer that is not loaded yet.
#[derive(Clone, Debug, PartialEq)]
pub struct MapSpec {
    pub path: PathBuf,
    pub opacity: f32,
    pub georef: Option<String>,
}

pub struct Gis {
    pub view: View,
    pub canvas: Vec2,
    pub fit_pending: bool,
    pub base_on: bool,
    pub tiles: Tiles,
    /// The map layers (georeferenced images); their place in the list and their switches are in `layers`.
    pub maps: Vec<MapLayer>,
    /// Maps being read: the id of the layer it is for (`None` for a new layer) and where the result comes from.
    pub loads: Vec<(Option<u32>, Receiver<Result<gd::LoadedMap, String>>)>,
    pub map_err: String,
    pub pending_crs: CrsChoice,
    pub layers: LayerTree,
    /// The layer selected in the sidebar, and whether the sidebar is shown.
    pub sel: Option<u32>,
    pub show_layers: bool,
    pub layer_drag: Option<u32>,
    pub renaming: Option<(u32, String)>,
    // the coordinates table in use, and the one being set up in the columns window
    pub table: Option<TableFile>,
    pub mapping: Mapping,
    pub locs: Vec<Loc>,
    pub table_ver: u64,
    pub draft: Option<(TableFile, Mapping)>,
    pub win_table: bool,
    pub table_err: String,
    // symbols
    pub placed: Vec<Placed>,
    /// Specimens without a saved place (or without data) and the specimens looked at.
    pub missing: usize,
    pub n_files: usize,
    pub placed_sig: u64,
    pub placed_check: Option<std::time::Instant>,
    pub loc: super::gis_loc_ui::LocState,
    pub manual: super::gis_manual_ui::ManualState,
    pub show_fol: bool,
    pub show_lin: bool,
    pub show_num: bool,
    pub show_names: bool,
    pub size: f32,
    pub gr: super::gis_georef::GeorefWin,
    pub win_georef: bool,
    // what the preferences file named and has not been loaded yet (it is loaded when the pane or a window is first used)
    pub restore_maps: std::collections::HashMap<u32, MapSpec>,
    pub restore_table: Option<PathBuf>,
    pub restore_mapping: Option<Mapping>,
}

impl Default for Gis {
    fn default() -> Self {
        Gis {
            view: View::world(),
            canvas: Vec2::new(700.0, 500.0),
            fit_pending: true,
            base_on: false,
            tiles: Tiles::default(),
            maps: vec![],
            loads: vec![],
            map_err: String::new(),
            pending_crs: CrsChoice { kind: CoordKind::Utm, zone: 42, ..Default::default() },
            layers: LayerTree::default(),
            sel: None,
            show_layers: false,
            layer_drag: None,
            renaming: None,
            table: None,
            mapping: Mapping::default(),
            locs: vec![],
            table_ver: 0,
            draft: None,
            win_table: false,
            table_err: String::new(),
            placed: vec![],
            missing: 0,
            n_files: 0,
            placed_sig: 0,
            placed_check: None,
            loc: Default::default(),
            manual: Default::default(),
            show_fol: true,
            show_lin: true,
            show_num: true,
            show_names: false,
            size: 16.0,
            gr: Default::default(),
            win_georef: false,
            restore_maps: Default::default(),
            restore_table: None,
            restore_mapping: None,
        }
    }
}

impl Gis {
    /// The settings of the GIS pane as lines of the preferences file: the switches, the basemap, the map layer with its
    /// georeference, the datasheet with its column assignment and where the map was looking.
    pub fn prefs_text(&self) -> String {
        let b = |v: bool| u8::from(v);
        let mut t = format!(
            "gis_osm={}\ngis_fol={}\ngis_lin={}\ngis_num={}\ngis_names={}\ngis_size={}\ngis_view={},{},{}\ngis_zone={},{}\ngis_layerbar={}\n",
            b(self.base_on), b(self.show_fol), b(self.show_lin), b(self.show_num), b(self.show_names), self.size, self.view.cx, self.view.cy, self.view.mpp, self.manual.zone, b(self.manual.north), b(self.show_layers)
        );
        // the layer list with the data of every map layer (loaded ones live, the others as the preferences had them)
        let maps: Vec<serde_json::Value> = self
            .layers
            .items
            .iter()
            .filter(|i| i.kind == Kind::Map)
            .filter_map(|i| {
                let (path, opacity, georef) = match self.maps.iter().find(|m| m.id == i.id) {
                    Some(m) => (m.path.clone(), m.opacity, m.georef.map(|g| gd::georef_line(&g, m.orig, &m.points)).filter(|l| !l.is_empty())),
                    None => {
                        let s = self.restore_maps.get(&i.id)?;
                        (s.path.clone(), s.opacity, s.georef.clone())
                    }
                };
                Some(serde_json::json!({"id": i.id, "path": path.display().to_string(), "opacity": opacity, "georef": georef}))
            })
            .collect();
        t += &format!("gis_layers={}\n", serde_json::json!({"tree": self.layers.to_values(), "maps": maps, "selected": self.sel}).to_string().replace(['\n', '\r'], ""));
        if let Some(p) = self.table.as_ref().map(|t| t.path.clone()).or_else(|| self.restore_table.clone()) {
            t += &format!("gis_table={}\n", p.display().to_string().replace(['\n', '\r'], ""));
            let m = if self.table.is_some() { Some(self.mapping.to_line()) } else { self.restore_mapping.as_ref().map(|m| m.to_line()) };
            if let Some(m) = m {
                t += &format!("gis_mapping={m}\n");
            }
        }
        t
    }

    /// One line of the preferences file; false when the key is not one of the GIS pane.
    pub fn load_pref(&mut self, k: &str, v: &str) -> bool {
        match k {
            "gis_osm" => self.base_on = v == "1",
            "gis_layerbar" => self.show_layers = v == "1",
            "gis_fol" => self.show_fol = v != "0",
            "gis_lin" => self.show_lin = v != "0",
            "gis_num" => self.show_num = v != "0",
            "gis_names" => self.show_names = v == "1",
            "gis_size" => self.size = v.parse().unwrap_or(16.0f32).clamp(8.0, 40.0),
            "gis_view" => {
                let p: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if p.len() == 3 && p.iter().all(|x| x.is_finite()) && p[2] > MIN_MPP && p[2] < 39_000.0 {
                    self.view = View { cx: p[0], cy: p[1], mpp: p[2] };
                    // where the map was looking comes back; a new map or new places fit as before
                    self.fit_pending = false;
                }
            }
            "gis_zone" => {
                let p: Vec<&str> = v.split(',').collect();
                if let (Some(z), Some(n)) = (p.first().and_then(|z| z.parse::<u8>().ok()).filter(|z| (1..=60).contains(z)), p.get(1)) {
                    self.manual.zone = z;
                    self.manual.north = *n != "0";
                }
            }
            "gis_layers" => self.load_layers(v),
            "gis_table" if std::path::Path::new(v).is_file() => self.restore_table = Some(PathBuf::from(v)),
            "gis_mapping" => self.restore_mapping = Mapping::from_line(v),
            _ => return false,
        }
        true
    }

    /// The layer list from the preferences: the tree, and the maps named in it whose files are still there (a layer
    /// whose file is gone is dropped).
    fn load_layers(&mut self, v: &str) {
        let Ok(j) = serde_json::from_str::<serde_json::Value>(v) else { return };
        let Some(mut tree) = j.get("tree").and_then(|t| t.as_array()).and_then(|t| LayerTree::from_values(t)) else { return };
        let mut specs = std::collections::HashMap::new();
        for m in j.get("maps").and_then(|m| m.as_array()).into_iter().flatten() {
            let (Some(id), Some(path)) = (m.get("id").and_then(|i| i.as_u64()), m.get("path").and_then(|p| p.as_str())) else { continue };
            if std::path::Path::new(path).is_file() {
                specs.insert(
                    id as u32,
                    MapSpec { path: PathBuf::from(path), opacity: m.get("opacity").and_then(|o| o.as_f64()).map_or(0.75, |o| (o as f32).clamp(0.05, 1.0)), georef: m.get("georef").and_then(|g| g.as_str()).map(str::to_string) },
                );
            }
        }
        let gone: Vec<u32> = tree.items.iter().filter(|i| i.kind == Kind::Map && !specs.contains_key(&i.id)).map(|i| i.id).collect();
        for id in gone {
            tree.remove(id);
        }
        self.layers = tree;
        self.restore_maps = specs;
        self.sel = j.get("selected").and_then(|s| s.as_u64()).map(|s| s as u32).filter(|s| self.layers.get(*s).is_some());
    }

    /// Reads back the maps and the datasheet the preferences named (once, when the GIS pane or one of its windows is
    /// first used, so a session without the GIS viewer never touches them).
    pub fn restore(&mut self, ctx: &egui::Context) {
        let pending: Vec<(u32, PathBuf)> = self.restore_maps.iter().filter(|(id, _)| !self.maps.iter().any(|m| m.id == **id) && !self.loads.iter().any(|(l, _)| *l == Some(**id))).map(|(id, s)| (*id, s.path.clone())).collect();
        for (id, path) in pending {
            self.start_load(ctx, Some(id), path);
        }
        if let Some(p) = self.restore_table.take() {
            if let Ok(t) = gd::read_table(&p) {
                let m = self.restore_mapping.take().unwrap_or_else(|| gd::guess_mapping(t.grid()));
                let (locs, _) = gd::build_locs(t.grid(), &m);
                self.table = Some(t);
                self.mapping = m;
                self.locs = locs;
                self.table_ver += 1;
            }
            self.restore_mapping = None;
        }
    }

    /// Web Mercator box around the places (and failing that around the georeferenced map).
    pub fn data_box(&self) -> Option<((f64, f64), (f64, f64))> {
        let mut lo = (f64::MAX, f64::MAX);
        let mut hi = (f64::MIN, f64::MIN);
        let pts: Vec<(f64, f64)> = if self.placed.is_empty() { self.locs.iter().map(|l| l.merc).collect() } else { self.placed.iter().map(|p| p.merc).collect() };
        for p in &pts {
            lo = (lo.0.min(p.0), lo.1.min(p.1));
            hi = (hi.0.max(p.0), hi.1.max(p.1));
        }
        for (a, b) in self.maps.iter().filter_map(|m| m.extent()) {
            lo = (lo.0.min(a.0), lo.1.min(a.1));
            hi = (hi.0.max(b.0), hi.1.max(b.1));
        }
        (lo.0 <= hi.0).then_some((lo, hi))
    }

    /// Adds a map as a new layer (when it has been read).
    pub fn load_map_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.start_load(ctx, None, path);
    }

    /// Reads a map on a worker thread; `id` is the layer it belongs to when the layer exists already.
    pub fn start_load(&mut self, ctx: &egui::Context, id: Option<u32>, path: PathBuf) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.loads.push((id, rx));
        self.map_err.clear();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let r = gd::load_map(&path);
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
    }

    /// Takes in the maps the loader threads finished.
    pub fn poll_map(&mut self, ctx: &egui::Context) {
        if self.loads.is_empty() {
            return;
        }
        let mut still = vec![];
        for (id, rx) in std::mem::take(&mut self.loads) {
            match rx.try_recv() {
                Ok(Ok(m)) => {
                    let tex = ctx.load_texture(format!("gis-map-{}", self.maps.len() + 1), m.image, TextureOptions::LINEAR);
                    let spec = id.and_then(|i| self.restore_maps.remove(&i));
                    let (mut georef, mut points, mut pending, mut note) = (m.georef, m.points, m.pending, m.note);
                    if georef.is_none() {
                        // a georeference made with control points and not saved next to the map file: the preferences have it
                        if let Some((g, pts)) = spec.as_ref().and_then(|s| s.georef.as_ref()).and_then(|l| gd::parse_sidecar(l, m.orig).ok()) {
                            georef = Some(g);
                            points = pts;
                            pending = None;
                            note = "Georeference restored from the last session.".into();
                        }
                    }
                    let name = m.path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Map".into());
                    let layer_id = match id {
                        Some(i) if self.layers.get(i).is_some() => i,
                        _ => {
                            let i = self.layers.add_map(&name);
                            self.sel = Some(i);
                            self.show_layers = true;
                            i
                        }
                    };
                    self.maps.retain(|x| x.id != layer_id);
                    self.maps.push(MapLayer { id: layer_id, opacity: spec.map_or(0.75, |s| s.opacity), path: m.path, orig: m.orig, tex, georef, pending, note, points });
                    if id.is_none() {
                        self.fit_pending = true;
                    }
                }
                Ok(Err(e)) => {
                    self.map_err = e;
                    // a layer from the preferences whose file cannot be read any more goes
                    if let Some(i) = id {
                        self.restore_maps.remove(&i);
                        self.layers.remove(i);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => still.push((id, rx)),
                Err(_) => {}
            }
        }
        if !still.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        self.loads.extend(still);
    }

    pub fn map_mut(&mut self, id: u32) -> Option<&mut MapLayer> {
        self.maps.iter_mut().find(|m| m.id == id)
    }
}

/// A file picked in a dialog; `env` names a variable that supplies the path instead (headless test hook).
pub fn pick_file(title: &str, filter_name: &str, exts: &[&str], env: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(env) {
        return Some(PathBuf::from(p));
    }
    rfd::FileDialog::new().add_filter(filter_name, exts).set_title(title).pick_file()
}

fn dirvec(az_deg: f64) -> Vec2 {
    let a = az_deg.to_radians();
    Vec2::new(a.sin() as f32, -a.cos() as f32)
}

/// Text with a thin outline in the halo colour so it reads on any map.
fn halo_text(p: &egui::Painter, pos: Pos2, anchor: Align2, text: &str, size: f32, col: Color32, halo: Color32) {
    let font = FontId::proportional(size);
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.7, -0.7), (0.7, 0.7), (-0.7, 0.7), (0.7, -0.7)] {
        p.text(pos + Vec2::new(dx, dy), anchor, text, font.clone(), halo);
    }
    p.text(pos, anchor, text, font, col);
}

/// Draws a georeferenced image as a mesh (every vertex goes pixel -> coordinates -> Web Mercator -> screen, so a
/// map in UTM is bent correctly, not only shifted and scaled).
pub fn draw_map_image(painter: &egui::Painter, tex: &TextureHandle, orig: (u32, u32), g: &Georef, view: &View, centre: Pos2, alpha: f32) {
    const N: usize = 24;
    let col = Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0) as u8);
    let mut mesh = Mesh::with_texture(tex.id());
    let (w, h) = (f64::from(orig.0), f64::from(orig.1));
    for j in 0..=N {
        for i in 0..=N {
            let (u, v) = (i as f32 / N as f32, j as f32 / N as f32);
            let m = g.pixel_to_merc(f64::from(u) * w, f64::from(v) * h);
            if !(m.0.is_finite() && m.1.is_finite()) {
                return;
            }
            mesh.vertices.push(Vertex { pos: view.to_screen(centre, m), uv: Pos2::new(u, v), color: col });
        }
    }
    let row = (N + 1) as u32;
    for j in 0..N as u32 {
        for i in 0..N as u32 {
            let a = j * row + i;
            mesh.indices.extend([a, a + 1, a + row, a + 1, a + row + 1, a + row]);
        }
    }
    painter.add(Shape::mesh(mesh));
    let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h), (0.0, 0.0)];
    let pts: Vec<Pos2> = corners.iter().map(|&(x, y)| view.to_screen(centre, g.pixel_to_merc(x, y))).collect();
    painter.add(Shape::line(pts, Stroke::new(1.0, Color32::from_rgba_unmultiplied(40, 40, 40, 150))));
}

/// OpenStreetMap tiles for the area of `rect`.
pub fn draw_tiles(painter: &egui::Painter, rect: Rect, view: &View, tiles: &mut Tiles, ppp: f32, dark: bool) {
    let z = gis::zoom_for(view.mpp, ppp);
    let c = rect.center();
    let (a, b) = (view.to_merc(c, rect.left_top()), view.to_merc(c, rect.right_bottom()));
    let (tx0, ty0) = gis::tile_from_merc(a.0, a.1, z);
    let (tx1, ty1) = gis::tile_from_merc(b.0, b.1, z);
    let n = (1i64 << z) - 1;
    let (x0, x1) = ((tx0.floor() as i64).clamp(0, n), (tx1.floor() as i64).clamp(0, n));
    let (y0, y1) = ((ty0.floor() as i64).clamp(0, n), (ty1.floor() as i64).clamp(0, n));
    if (x1 - x0 + 1) * (y1 - y0 + 1) > 400 {
        return;
    }
    let placeholder = if dark { Color32::from_gray(52) } else { Color32::from_gray(214) };
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (xu, yu) = (x as u32, y as u32);
            let tl = view.to_screen(c, gis::merc_from_tile(f64::from(xu), f64::from(yu), z));
            let br = view.to_screen(c, gis::merc_from_tile(f64::from(xu + 1), f64::from(yu + 1), z));
            let r = Rect::from_min_max(tl, br).expand(0.4);
            let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            if let Some(id) = tiles.get((z, xu, yu)).map(|t| t.id()) {
                painter.image(id, r, full, Color32::WHITE);
                continue;
            }
            // while it loads: the blurry tile of a lower zoom level, if there is one
            painter.rect_filled(r, 0.0, placeholder);
            for d in 1..=5u32 {
                if z < d {
                    break;
                }
                let key = (z - d, xu >> d, yu >> d);
                if let Some(id) = tiles.peek(key).map(|t| t.id()) {
                    let s = 1.0 / (1u32 << d) as f32;
                    let (u, v) = ((xu - ((xu >> d) << d)) as f32 * s, (yu - ((yu >> d) << d)) as f32 * s);
                    painter.image(id, r, Rect::from_min_max(Pos2::new(u, v), Pos2::new(u + s, v + s)), Color32::WHITE);
                    break;
                }
            }
        }
    }
}

/// A scale bar, a coordinate read-out and (with the basemap) the attribution OpenStreetMap requires.
fn draw_furniture(painter: &egui::Painter, rect: Rect, view: &View, dark: bool, base_on: bool, hover: Option<Pos2>) {
    let fg = if dark { Color32::from_gray(235) } else { Color32::from_gray(25) };
    let halo = if dark { Color32::from_rgba_unmultiplied(0, 0, 0, 200) } else { Color32::from_rgba_unmultiplied(255, 255, 255, 220) };
    let lat = gis::lonlat_from_merc(view.cx, view.cy).1;
    let ground = view.mpp * gis::merc_scale(lat);
    let target = ground * 110.0;
    let p10 = 10f64.powf(target.log10().floor());
    let len_m = [1.0, 2.0, 5.0, 10.0].iter().map(|k| k * p10).filter(|l| *l <= target).fold(p10, f64::max);
    let px = (len_m / ground) as f32;
    let (x0, y0) = (rect.min.x + 12.0, rect.max.y - 24.0);
    painter.line_segment([Pos2::new(x0, y0), Pos2::new(x0 + px, y0)], Stroke::new(4.0, halo));
    painter.line_segment([Pos2::new(x0, y0 - 4.0), Pos2::new(x0, y0 + 4.0)], Stroke::new(4.0, halo));
    painter.line_segment([Pos2::new(x0 + px, y0 - 4.0), Pos2::new(x0 + px, y0 + 4.0)], Stroke::new(4.0, halo));
    painter.line_segment([Pos2::new(x0, y0), Pos2::new(x0 + px, y0)], Stroke::new(1.6, fg));
    painter.line_segment([Pos2::new(x0, y0 - 4.0), Pos2::new(x0, y0 + 4.0)], Stroke::new(1.6, fg));
    painter.line_segment([Pos2::new(x0 + px, y0 - 4.0), Pos2::new(x0 + px, y0 + 4.0)], Stroke::new(1.6, fg));
    let label = if len_m >= 1000.0 { format!("{} km", (len_m / 1000.0) as f64) } else { format!("{len_m} m") };
    halo_text(painter, Pos2::new(x0 + px + 8.0, y0), Align2::LEFT_CENTER, &label, 12.0, fg, halo);
    if let Some(h) = hover {
        let (lon, lat) = gis::lonlat_from_merc(view.to_merc(rect.center(), h).0, view.to_merc(rect.center(), h).1);
        halo_text(painter, Pos2::new(x0, y0 - 14.0), Align2::LEFT_CENTER, &format!("{lat:.5}, {lon:.5}"), 11.5, fg, halo);
    }
    if base_on {
        let t = "© OpenStreetMap contributors";
        let g = painter.layout_no_wrap(t.to_string(), FontId::proportional(11.0), Color32::from_gray(30));
        let r = Rect::from_min_size(Pos2::new(rect.max.x - g.size().x - 10.0, rect.max.y - g.size().y - 6.0), g.size() + Vec2::new(6.0, 2.0));
        painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 200));
        painter.galley(r.min + Vec2::new(3.0, 1.0), g, Color32::from_gray(30));
    }
}

/// Colour ramps of the symbols: (very shallow, 45 degrees, vertical). The ramp goes through the saturated base colour,
/// so red and blue stay easy to tell apart even for the palest ones. The strike symbol (strike line and dip tick) is
/// red, the lineation arrow blue.
const STRIKE_RAMP: [[u8; 3]; 3] = [[248, 200, 200], [222, 40, 40], [92, 4, 10]];
const LINE_RAMP: [[u8; 3]; 3] = [[198, 216, 252], [40, 92, 228], [6, 22, 108]];

/// The fill for an angle of 0 to 90 degrees (dip or plunge): barely coloured when shallow, intense and dark when steep.
pub fn ramp_colour(ramp: &[[u8; 3]; 3], angle: f64) -> Color32 {
    let t = (angle / 90.0).clamp(0.0, 1.0) as f32;
    let (a, b, u) = if t < 0.5 { (ramp[0], ramp[1], t / 0.5) } else { (ramp[1], ramp[2], (t - 0.5) / 0.5) };
    let mix = |i: usize| (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * u).round() as u8;
    Color32::from_rgb(mix(0), mix(1), mix(2))
}

/// A bar from `a` to `b` of the given width, as a polygon.
fn bar(a: Pos2, b: Pos2, width: f32) -> Vec<Pos2> {
    let d = b - a;
    let n = if d.length() > 1e-6 { Vec2::new(-d.y, d.x).normalized() * (width / 2.0) } else { Vec2::ZERO };
    vec![a + n, b + n, b - n, a - n]
}

/// Shaft and head of an arrow from `start` along the unit vector `t`.
fn arrow_polys(start: Pos2, t: Vec2, len: f32) -> (Vec<Pos2>, Vec<Pos2>) {
    let head_len = (len * 0.38).clamp(5.5, 10.0);
    let shaft_end = start + t * (len - head_len * 0.6).max(1.0);
    let tip = start + t * len;
    let n = Vec2::new(-t.y, t.x);
    let head = vec![tip, tip - t * head_len + n * (head_len * 0.55), tip - t * head_len - n * (head_len * 0.55)];
    (bar(start, shaft_end, 3.6), head)
}

/// Where the lineation arrow starts: in the corner between the strike line and the dip tick, on the dipping side and
/// towards the side the lineation trends to, a little off the intersection and not at the tick's tip. `s` is the unit
/// vector along the strike, `d` the one along the dip direction, `t` the one along the trend.
pub fn lineation_start(c: Pos2, s: Vec2, d: Vec2, t: Vec2, l: f32) -> Pos2 {
    let side = if t.x * s.x + t.y * s.y >= 0.0 { 1.0 } else { -1.0 };
    c + d * (0.30 * l) + s * (side * 0.42 * l)
}

/// The strike / dip and lineation symbols. Returns the index of the symbol under the pointer.
///
/// The strike symbol (strike line and dip tick) is red, the lineation arrow blue, drawn as filled shapes with a thin
/// white outline; the fill goes from barely coloured (shallow) to intense and dark (steep) with the dip of the
/// foliation and the plunge of the lineation. A highlighted specimen (the open file, the selection) gets a thin orange
/// and yellow outline instead of the white one, and no ring.
fn draw_symbols(painter: &egui::Painter, rect: Rect, g: &Gis, dark: bool, selected: &dyn Fn(&Placed) -> bool, hover: Option<Pos2>) -> Option<usize> {
    let view = g.view;
    let centre = rect.center();
    let halo = if dark { Color32::from_rgba_unmultiplied(0, 0, 0, 170) } else { Color32::from_rgba_unmultiplied(255, 255, 255, 215) };
    let l = g.size;
    let (orange, yellow) = (Color32::from_rgb(255, 150, 20), Color32::from_rgb(255, 235, 40));
    let mut hit: Option<(usize, f32)> = None;
    for (idx, p) in g.placed.iter().enumerate() {
        let c = view.to_screen(centre, p.merc);
        if !rect.expand(70.0).contains(c) {
            continue;
        }
        let f = p.fabric;
        let sel = selected(p);
        // the filled shapes in drawing order: polygon, fill
        let mut polys: Vec<(Vec<Pos2>, Color32)> = vec![];
        let mut circles: Vec<(Pos2, f32, Color32)> = vec![];
        let (s, d) = (dirvec(f.strike), dirvec(f.dip_dir));
        let t = dirvec(f.trend);
        let (c_strike, c_line) = (ramp_colour(&STRIKE_RAMP, f.dip), ramp_colour(&LINE_RAMP, f.plunge));
        let tick = l * 0.72;
        let mut start = c;
        let mut labels: Vec<(Pos2, String, Color32)> = vec![];
        if g.show_fol {
            if f.dip <= 5.0 {
                // flat: a circle with a darker cross, all red
                let r = l * 0.40;
                let cross = ramp_colour(&STRIKE_RAMP, 90.0);
                circles.push((c, r, c_strike));
                polys.push((bar(c - Vec2::new(r, 0.0), c + Vec2::new(r, 0.0), 3.0), cross));
                polys.push((bar(c - Vec2::new(0.0, r), c + Vec2::new(0.0, r), 3.0), cross));
                if g.show_num {
                    labels.push((c + Vec2::new(0.0, r + 9.0), format!("{:.0}", f.dip), ramp_colour(&STRIKE_RAMP, 50.0)));
                }
                start = c + t * (0.55 * l);
            } else {
                polys.push((bar(c - s * l, c + s * l, 4.8), c_strike));
                polys.push((bar(c, c + d * tick, 5.0), c_strike));
                if f.dip >= 85.0 {
                    polys.push((bar(c, c - d * tick, 5.0), c_strike));
                }
                if g.show_num {
                    labels.push((c + d * (tick + 9.0), format!("{:.0}", f.dip), ramp_colour(&STRIKE_RAMP, 50.0)));
                }
                start = lineation_start(c, s, d, t, l);
            }
        }
        if g.show_lin {
            if f.plunge >= 85.0 {
                circles.push((start, 4.2, c_line));
                if g.show_num {
                    labels.push((start + Vec2::new(0.0, 11.0), format!("{:.0}", f.plunge), ramp_colour(&LINE_RAMP, 50.0)));
                }
            } else {
                // the length follows the plunge: a flat lineation gets the full arrow, a steep one a short one
                let len = l * 1.7 * (0.35 + 0.65 * (f.plunge.to_radians().cos() as f32));
                let (shaft, head) = arrow_polys(start, t, len);
                polys.push((shaft, c_line));
                polys.push((head, c_line));
                if g.show_num {
                    labels.push((start + t * (len + 10.0), format!("{:.0}", f.plunge), ramp_colour(&LINE_RAMP, 50.0)));
                }
            }
        }
        // outlines first (so shapes that touch share one outline), then the fills
        let outline: Vec<(f32, Color32)> = if sel { vec![(3.2, orange), (1.7, yellow)] } else { vec![(2.4, Color32::WHITE)] };
        for (w, col) in &outline {
            for (pts, _) in &polys {
                painter.add(Shape::convex_polygon(pts.clone(), Color32::TRANSPARENT, Stroke::new(*w, *col)));
            }
            for (cc, r, _) in &circles {
                painter.circle_stroke(*cc, *r, Stroke::new(*w, *col));
            }
        }
        for (cc, r, fill) in &circles {
            painter.circle_filled(*cc, *r, *fill);
        }
        for (pts, fill) in &polys {
            painter.add(Shape::convex_polygon(pts.clone(), *fill, Stroke::NONE));
        }
        for (pos, text, col) in &labels {
            halo_text(painter, *pos, Align2::CENTER_CENTER, text, 12.0, *col, halo);
        }
        // the place itself
        painter.circle_filled(c, 2.2, Color32::BLACK);
        painter.circle_stroke(c, 2.2, Stroke::new(1.0, Color32::WHITE));
        if g.show_names {
            halo_text(painter, c + Vec2::new(0.0, l + 12.0), Align2::CENTER_TOP, &p.name, 11.5, if dark { Color32::from_gray(235) } else { Color32::from_gray(25) }, halo);
        }
        if let Some(h) = hover {
            let dist = h.distance(c);
            if dist < l + 8.0 && hit.map_or(true, |(_, b)| dist < b) {
                hit = Some((idx, dist));
            }
        }
    }
    hit.map(|h| h.0)
}

impl App {
    pub(super) fn gis_open_window(&mut self) {
        self.open_win(W_GIS);
    }

    /// Loads a coordinates file and opens the columns window for it.
    pub(super) fn gis_load_table(&mut self, path: PathBuf) {
        match gd::read_table(&path) {
            Ok(t) => {
                let mut m = gd::guess_mapping(t.grid());
                let c = &self.gis.view;
                m.crs.zone = gis::utm_zone(gis::lonlat_from_merc(c.cx, c.cy).0);
                self.gis.draft = Some((t, m));
                self.gis.table_err.clear();
                self.gis.win_table = true;
                self.win_serial[super::W_GIS_TABLE] += 1;
            }
            Err(e) => self.gis.table_err = e,
        }
    }

    pub(super) fn gis_window(&mut self, ctx: &egui::Context) {
        // the pane, the settings and the helper windows all need the housekeeping below
        let active = self.gis_on || self.win_gis || self.gis.win_georef || self.gis.win_table || self.gis.loc.win || self.gis.manual.win || self.test_manual.is_some() || self.test_pick.is_some();
        if !active {
            return;
        }
        self.gis.restore(ctx);
        self.gis.poll_map(ctx);
        // headless test hook: AMS_STUDIO_GIS_PICK="<specimen id>=<datasheet name>;..." links like a choice in the drop-down
        if self.test_pick.is_some() {
            self.loc_sync_rows();
            if !self.gis.loc.rows.is_empty() && !self.gis.locs.is_empty() {
                let spec = self.test_pick.take().unwrap_or_default();
                let keys: Vec<String> = self.gis.locs.iter().map(|l| l.key.clone()).collect();
                for item in spec.split(';') {
                    if let Some((id, key)) = item.split_once('=') {
                        if let (Some(r), Some(k)) = (self.gis.loc.rows.iter_mut().find(|r| r.id == id), keys.iter().position(|x| x == key)) {
                            r.manual = Some(Some(k));
                            r.link = Some(k);
                        }
                    }
                }
            }
        }
        // headless test hook: the manual window once the specimens of the file tree are listed
        if let Some(all) = self.test_manual {
            self.loc_sync_rows();
            if !self.gis.loc.rows.is_empty() {
                self.test_manual = None;
                self.gis_open_manual(None);
                self.gis.manual.all = all;
                if std::env::var_os("AMS_STUDIO_GIS_YES").is_some() {
                    // test hook: the coordinate system of the list is confirmed as detected from the first row
                    self.test_bulk_yes = true;
                }
                if let Ok(v) = std::env::var("AMS_STUDIO_GIS_TYPE") {
                    // "<id>|<first>|<second>" for one specimen, or "<id>=<first>,<second>;..." for the list
                    if all {
                        self.gis.manual.rows = v.split(';').filter_map(|p| p.split_once('=')).filter_map(|(id, ab)| ab.split_once(',').map(|(a, b)| super::gis_manual_ui::BRow { id: id.to_string(), a: a.to_string(), b: b.to_string() })).collect();
                    } else {
                        let p: Vec<&str> = v.split('|').collect();
                        if p.len() == 3 {
                            self.gis.manual.id = p[0].to_string();
                            self.gis.manual.tx = p[1].to_string();
                            self.gis.manual.ty = p[2].to_string();
                        }
                    }
                }
            }
        }
        if self.test_georef && !self.gis.maps.is_empty() {
            self.test_georef = false;
            let first = self.gis.maps[0].id;
            self.gis_open_georef(first);
            // headless test hook: AMS_STUDIO_GIS_PAIRS="px,py,lon,lat;px,py,lon,lat;..." pre-fills control points
            if let Some(t) = std::env::var("AMS_STUDIO_GIS_PAIRS").ok() {
                for item in t.split(';') {
                    let v: Vec<f64> = item.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    if v.len() == 4 {
                        self.gis.gr.pairs.push(((v[0], v[1]), gis::merc_from_lonlat(v[2], v[3])));
                    }
                }
                self.gis.gr.refit_pub();
            }
        }
        self.gis_refresh_placed();
        if self.win_gis {
            // the settings of the GIS viewer pane
            let mut open = true;
            let scr = ctx.content_rect();
            let size = Vec2::new(340.0, (scr.height() - 120.0).clamp(420.0, 700.0));
            self.base_window(ctx, "GIS viewer settings", W_GIS, size).open(&mut open).resizable(true).show(ctx, |ui| {
                self.gis_ui(ui, ctx);
            });
            self.win_gis = open;
        }
        self.gis_table_window(ctx);
        self.gis_georef_window(ctx);
        self.gis_loc_window(ctx);
        self.gis_manual_window(ctx);
    }

    fn gis_ui(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.set_min_size(Vec2::new(300.0, 380.0));
        let full = ui.available_rect_before_wrap();
        let side_w = full.width();
        let left = full;
        ui.allocate_rect(full, Sense::hover());
        let mut from_file = false;
        let mut manual = false;
        let mut load_map = false;
        let mut edit_cols = false;
        let _ = &mut edit_cols;
        let mut fit = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(side_w - 14.0);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 7.0);
                let g = &mut self.gis;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Where are the specimens?").strong().size(15.5));
                    info_icon(ui, "Every specimen of the file tree (or the open file) gets one place, kept in its own dependency file. Input from file: a datasheet (csv or xlsx) whose names match the specimens; they are linked by name and you can change any link. Input manually: type the specimen ID and its coordinates, one specimen at a time or all in one list. The map then shows one symbol per specimen: the mean fabric of its records. Nothing is written to the datasheet or to the .ams files.");
                });
                ui.label(RichText::new(format!("Places saved for (n={}) of (n={}) specimens", g.placed.len(), g.n_files)).small());
                ui.horizontal(|ui| {
                    from_file = ui.add(egui::Button::new("Input from file").min_size(Vec2::new(118.0, 34.0))).on_hover_text("Take the places from a datasheet (csv or xlsx) and link the specimens to its rows").clicked();
                    manual = ui.add(egui::Button::new("Input manually").min_size(Vec2::new(118.0, 34.0))).on_hover_text("Type the specimen ID and its coordinates").clicked();
                });
                if let Some(t) = &g.table {
                    ui.label(RichText::new(format!("{}  (n={} rows)", t.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), g.locs.len())).small().weak());
                }
                if !g.table_err.is_empty() {
                    ui.label(RichText::new(&g.table_err).small().color(Color32::from_rgb(200, 80, 40)));
                }

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Maps and layers").strong().size(15.5));
                    info_icon(ui, "Pictures of geologic maps (png, jpg) and GeoTIFFs are layers, as in QGIS: add as many as you like, drag them into the order you want, group them and switch them on and off in the layers sidebar of the pane (the Layers button in its header, or Shift+Ctrl+B). A GeoTIFF, or an image with a world file (.pgw .jgw .tfw .wld, with a .prj for the coordinate system), is placed on the earth straight away; otherwise select the layer and use Georeference. The map files are only read.");
                });
                ui.label(RichText::new("Map image or GeoTIFF:").small());
                ui.horizontal(|ui| {
                    load_map = ui.button("Add map ...").clicked();
                    ui.checkbox(&mut g.show_layers, "Layers sidebar").on_hover_text("The list of layers at the left of the GIS pane (Shift+Ctrl+B)");
                });
                if !g.loads.is_empty() {
                    ui.label(RichText::new("Reading the map ...").small().weak());
                }
                if !g.map_err.is_empty() {
                    ui.label(RichText::new(&g.map_err).small().color(Color32::from_rgb(200, 80, 40)));
                }

                ui.separator();
                ui.label(RichText::new("Basemap").strong().size(15.5));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut g.base_on, "OpenStreetMap");
                    info_icon(ui, "Downloads map tiles from tile.openstreetmap.org for the area you are looking at, which tells that server which area that is, so it is off until you tick it. Tiles are kept in the cache folder for 14 days. Only a generic program name is sent, nothing about you or your data. Map data © OpenStreetMap contributors.");
                });
                if g.base_on {
                    if let Some(e) = &g.tiles.error {
                        ui.label(RichText::new(format!("No tiles: {e}")).small().color(Color32::from_rgb(200, 80, 40)));
                    } else if g.tiles.busy() {
                        ui.label(RichText::new("Loading tiles ...").small().weak());
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Symbols").strong().size(15.5));
                    info_icon(ui, "Foliation: the plane perpendicular to K3, drawn as a red strike symbol: the strike line with a tick towards the dip direction and the dip next to it (a circle with a cross when it is flat, a tick on both sides when it is vertical). Lineation: a blue arrow along the trend of K1 with the plunge at its head, starting in the corner between the strike line and the tick; steep lineations get a short arrow, a vertical one a dot. The fill of every shape goes from barely coloured (shallow) to intense and dark (steep) with the dip or the plunge. The open file and the selected specimens get a thin orange and yellow outline. North is up. A click selects a specimen in the file tree, a double-click opens it.");
                });
                ui.checkbox(&mut g.show_fol, "Foliation (strike / dip)");
                ui.checkbox(&mut g.show_lin, "Lineation (K1 arrow)");
                ui.checkbox(&mut g.show_num, "Dip and plunge numbers");
                ui.checkbox(&mut g.show_names, "Names");
                ui.add(egui::Slider::new(&mut g.size, 8.0..=40.0).text("size").show_value(false));
                ui.separator();
                fit = ui.button("Fit view to data").clicked();
                ui.label(RichText::new("Directions: always geographic").small().weak()).on_hover_text("The map only shows geographic directions (north is up), whatever coordinate system is selected in the main window (F5 to F8). Records that carry their orientation are rotated to geographic coordinates (the same as the Geographic view, F6); data that were already corrected to geographic with the Owens method are used as they are.");
                let g = &self.gis;
                if g.n_files == 0 {
                    ui.label(RichText::new("No specimens: open a file or a folder of specimens.").small().weak());
                } else if g.placed.is_empty() {
                    ui.label(RichText::new("No specimen has a place yet: use Specimen locations.").small().weak());
                } else if g.missing > 0 {
                    ui.label(RichText::new(format!("Specimens without a place (n={})", g.missing)).small().color(Color32::from_rgb(200, 120, 30)));
                }
            });
        });
        if from_file {
            if self.gis.table.is_some() {
                self.gis_open_loc();
            } else if let Some(p) = pick_file("Datasheet with the coordinates", "Spreadsheet or csv", &["csv", "xlsx", "xlsm", "xls", "ods", "txt", "tsv", "dat", "asc", "xyz", "pts"], "AMS_STUDIO_GIS_TABLE") {
                // the columns window first, then the link view
                self.gis_load_table(p);
            } else {
                // no file chosen: the link view still offers Load datasheet
                self.gis_open_loc();
            }
        }
        if manual {
            self.gis_open_manual(None);
        }
        if edit_cols {
            if let Some(t) = self.gis.table.clone() {
                self.gis.draft = Some((t, self.gis.mapping.clone()));
                self.gis.win_table = true;
                self.win_serial[super::W_GIS_TABLE] += 1;
            }
        }
        if load_map {
            if let Some(p) = pick_file("Geologic map image or GeoTIFF", "Image", &["png", "jpg", "jpeg", "tif", "tiff"], "AMS_STUDIO_GIS_MAP") {
                self.gis.load_map_file(ctx, p);
            }
        }
        if fit {
            self.gis.fit_pending = true;
        }
    }

    /// The GIS viewer in a window of its own (a second native window that can be moved to another screen), shown while
    /// it is popped out of the panes. Closing that window, the dock button and Shift+Ctrl+Space put it back.
    pub(super) fn gis_popout(&mut self, ctx: &egui::Context) {
        if !(self.gis_on && self.gis_popped) {
            return;
        }
        let [x, y, w, h] = self.gis_win.unwrap_or([f32::NAN, f32::NAN, 1100.0, 760.0]);
        let mut builder = egui::ViewportBuilder::default().with_title("AMS Studio - GIS viewer").with_inner_size([w, h]).with_min_inner_size([520.0, 360.0]);
        if x.is_finite() && y.is_finite() {
            builder = builder.with_position([x, y]);
        }
        let th = self.theme();
        let (mut dock, mut toggle_bar) = (false, false);
        let mut geometry: Option<[f32; 4]> = None;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("ams-studio-gis-popout"), builder, |ui, _class| {
            let rect = ui.available_rect_before_wrap();
            ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
            ui.allocate_rect(rect, Sense::hover());
            self.gis_pane(ui, rect.shrink(6.0), &th);
            let vctx = ui.ctx().clone();
            dock = vctx.input(|i| i.viewport().close_requested()) || vctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT | egui::Modifiers::CTRL, egui::Key::Space));
            toggle_bar = vctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT | egui::Modifiers::CTRL, egui::Key::B));
            vctx.input(|i| {
                let v = i.viewport();
                if let (Some(o), Some(inner)) = (v.outer_rect, v.inner_rect) {
                    geometry = Some([o.min.x, o.min.y, inner.width(), inner.height()]);
                }
            });
        });
        if toggle_bar {
            self.gis.show_layers = !self.gis.show_layers;
        }
        if let Some(g) = geometry.filter(|_| !dock) {
            self.gis_win = Some(g);
        }
        if dock {
            self.gis_popped = false;
            self.save_state();
        }
    }

    /// The GIS viewer pane: a card like the other panes with a small header (settings, basemap, fit) and the map.
    pub(super) fn gis_pane(&mut self, ui: &mut Ui, rect: Rect, th: &crate::plots::Theme) {
        let visuals = ui.visuals().clone();
        ui.painter().rect(rect, 8.0, if th.dark { th.canvas() } else { visuals.extreme_bg_color }, Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
        let header = Rect::from_min_size(rect.min + Vec2::new(6.0, super::PANE_TOP), Vec2::new(rect.width() - 12.0, 24.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(header).layout(egui::Layout::left_to_right(egui::Align::Center)));
        child.label(RichText::new("GIS viewer").weak());
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Settings ...").on_hover_text("Where the specimens are, the geologic map, the basemap and the symbols").clicked() {
                self.open_win(W_GIS);
            }
            if ui.small_button(if self.gis_popped { "Dock" } else { "Pop out" }).on_hover_text("Pop the GIS viewer out into a window of its own, or put it back (Shift+Ctrl+Space)").clicked() {
                self.toggle_popout();
            }
            if ui.small_button("Fit").on_hover_text("Fit the map to the places and the geologic map").clicked() {
                self.gis.fit_pending = true;
            }
            ui.checkbox(&mut self.gis.base_on, "OpenStreetMap").on_hover_text("Downloads map tiles for the area you look at (see the settings); off until ticked");
            // the layers sidebar: one button shows it, the same button hides it again (Shift+Ctrl+B does the same)
            if ui.selectable_label(self.gis.show_layers, "Layers").on_hover_text("Show or hide the list of layers at the left (Shift+Ctrl+B)").clicked() {
                self.gis.show_layers = !self.gis.show_layers;
            }
        });
        let mut body = Rect::from_min_max(Pos2::new(rect.min.x + 4.0, header.max.y + 2.0), Pos2::new(rect.max.x - 4.0, rect.max.y - 4.0));
        if self.gis.show_layers {
            let w = 244.0f32.min(body.width() * 0.6);
            let bar = Rect::from_min_max(body.min, Pos2::new(body.min.x + w, body.max.y));
            body.min.x = bar.max.x + 4.0;
            self.gis_layer_bar(ui, bar, th);
        }
        self.gis_canvas(ui, body);
    }

    fn gis_canvas(&mut self, ui: &mut Ui, rect: Rect) {
        let ppp = ui.ctx().pixels_per_point();
        let dark = self.dark;
        let resp = ui.interact(rect, ui.scope_id().with("gis-canvas"), Sense::click_and_drag());
        let centre = rect.center();
        let g = &mut self.gis;
        g.canvas = rect.size();
        if g.fit_pending {
            // the places may only arrive a moment after the pane opens: the fit waits for them
            if let Some((a, b)) = g.data_box() {
                g.view = View::fit(a, b, rect.size());
                g.fit_pending = false;
            } else if g.view.mpp >= 39_999.0 {
                g.view = View::world();
            } else {
                g.fit_pending = false;
            }
        }
        if resp.dragged() {
            g.view.pan(resp.drag_delta());
            g.fit_pending = false;
        }
        if let Some(h) = resp.hover_pos() {
            let (zd, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            let f = f64::from(zd) / (f64::from(scroll.y) * 0.0025).exp();
            if (f - 1.0).abs() > 1e-6 {
                g.view.zoom_at(centre, h, f);
                g.fit_pending = false;
            }
        }
        if resp.double_clicked() {
            g.fit_pending = true;
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, if dark { Color32::from_rgb(28, 32, 38) } else { Color32::from_rgb(232, 235, 238) });
        g.tiles.begin_frame(ui.ctx());
        // the layers from the bottom to the top; the basemap's switch is the tick box of the settings and the header
        if let Some(b) = g.layers.first_of_kind(Kind::Basemap) {
            g.layers.set_visible(b, g.base_on);
        }
        let (tree_sel, open_path) = (&self.tree_sel, self.doc.as_ref().map(|d| d.path.clone()));
        let selected = |p: &Placed| tree_sel.contains(&p.path) || open_path.as_ref() == Some(&p.path);
        let mut hit = None;
        for (id, kind) in g.layers.draw_order() {
            match kind {
                Kind::Basemap => draw_tiles(&painter, rect, &g.view, &mut g.tiles, ppp, dark),
                Kind::Map => {
                    if let Some((m, gr)) = g.maps.iter().find(|m| m.id == id).and_then(|m| m.georef.as_ref().map(|gr| (m, gr))) {
                        draw_map_image(&painter, &m.tex, m.orig, gr, &g.view, centre, m.opacity);
                    }
                }
                Kind::Specimens => hit = draw_symbols(&painter, rect, g, dark, &selected, resp.hover_pos()),
                Kind::Group => {}
            }
        }
        draw_furniture(&painter, rect, &g.view, dark, g.base_on, resp.hover_pos());
        if g.base_on {
            g.tiles.start(ui.ctx());
        }
        if g.placed.is_empty() && g.locs.is_empty() && g.maps.is_empty() && !g.base_on {
            painter.text(centre, Align2::CENTER_CENTER, "Load the coordinates of your samples, and a geologic map or the OpenStreetMap basemap", FontId::proportional(14.0), if dark { Color32::from_gray(190) } else { Color32::from_gray(90) });
        }
        if let Some(i) = hit {
            let p = &g.placed[i];
            let f = p.fabric;
            let text = format!(
                "{} (n={} records)\nFoliation: strike {:.0}, dip {:.0} towards {:.0}\nLineation (K1): trend {:.0}, plunge {:.0}\n{:.5}, {:.5}\nClick selects it in the file tree, double-click opens it",
                p.name, p.n, f.strike, f.dip, f.dip_dir, f.trend, f.plunge, p.lat, p.lon
            );
            resp.clone().on_hover_text(text);
        }
        let mods = ui.input(|i| i.modifiers);
        let target = hit.map(|i| g.placed[i].path.clone());
        if resp.double_clicked() {
            if let Some(p) = target {
                self.open_path(p);
            }
        } else if resp.clicked() {
            // a click selects the specimen in the file tree (Ctrl adds it), a click on nothing clears the selection
            match target {
                Some(p) => {
                    if mods.ctrl || mods.command {
                        if let Some(k) = self.tree_sel.iter().position(|x| *x == p) {
                            self.tree_sel.remove(k);
                        } else {
                            self.tree_sel.push(p);
                        }
                    } else {
                        self.tree_sel = vec![p];
                    }
                }
                None => {
                    if !(mods.ctrl || mods.command) {
                        self.tree_sel.clear();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_round_trip_and_zoom_keeps_the_point_under_the_pointer() {
        let mut v = View { cx: 1000.0, cy: -500.0, mpp: 10.0 };
        let c = Pos2::new(300.0, 200.0);
        let p = Pos2::new(420.0, 90.0);
        let m = v.to_merc(c, p);
        let s = v.to_screen(c, m);
        assert!((s.x - p.x).abs() < 1e-3 && (s.y - p.y).abs() < 1e-3);
        v.zoom_at(c, p, 0.5);
        assert!((v.mpp - 5.0).abs() < 1e-12);
        let m2 = v.to_merc(c, p);
        assert!((m.0 - m2.0).abs() < 1e-6 && (m.1 - m2.1).abs() < 1e-6);
        v.zoom_at(c, p, 1e-12);
        assert_eq!(v.mpp, MIN_MPP);
    }

    #[test]
    fn fit_shows_the_whole_box_with_a_margin() {
        let v = View::fit((0.0, 0.0), (1000.0, 500.0), Vec2::new(500.0, 500.0));
        assert!(v.mpp * 500.0 > 1000.0 && v.mpp * 500.0 < 1000.0 * 1.6);
        assert_eq!((v.cx, v.cy), (500.0, 250.0));
        // a single point still gets a sane scale
        assert!(View::fit((5.0, 5.0), (5.0, 5.0), Vec2::new(400.0, 300.0)).mpp > 0.0);
    }

    #[test]
    fn the_fill_goes_from_pale_to_dark_and_keeps_its_hue() {
        for ramp in [&STRIKE_RAMP, &LINE_RAMP] {
            let (a, m, b) = (ramp_colour(ramp, 0.0), ramp_colour(ramp, 45.0), ramp_colour(ramp, 90.0));
            let lum = |c: Color32| u32::from(c.r()) * 3 + u32::from(c.g()) * 6 + u32::from(c.b());
            // paler at 0 than at 45, darker at 90 than at 45
            assert!(lum(a) > lum(m) && lum(m) > lum(b), "{a:?} {m:?} {b:?}");
            // out of range angles are clamped
            assert_eq!(ramp_colour(ramp, -5.0), a);
            assert_eq!(ramp_colour(ramp, 200.0), b);
        }
        // red stays red and blue stays blue even for the palest fill
        let (r, b) = (ramp_colour(&STRIKE_RAMP, 0.0), ramp_colour(&LINE_RAMP, 0.0));
        assert!(r.r() > r.b() + 30 && b.b() > b.r() + 30);
        // a shallow dip is far lighter than a steep one
        assert!(ramp_colour(&STRIKE_RAMP, 10.0).g() > ramp_colour(&STRIKE_RAMP, 80.0).g() + 100);
    }

    #[test]
    fn the_lineation_starts_in_the_corner_of_the_strike_symbol_not_on_its_axes() {
        let c = Pos2::new(100.0, 100.0);
        // strike east-west, dipping south (down the screen), lineation trending east
        let (s, d, t) = (dirvec(90.0), dirvec(180.0), dirvec(100.0));
        let p = lineation_start(c, s, d, t, 16.0);
        assert!(p.x > c.x + 4.0 && p.y > c.y + 3.0, "{p:?}");
        // not on the strike line (y = c.y) and not on the dip tick (x = c.x), and short of the tick's tip
        assert!((p.y - c.y).abs() > 3.0 && (p.x - c.x).abs() > 3.0);
        assert!(p.y - c.y < 0.72 * 16.0);
        // a lineation trending to the other side starts in the other corner, on the same (dipping) side
        let q = lineation_start(c, s, d, dirvec(260.0), 16.0);
        assert!(q.x < c.x - 4.0 && (q.y - p.y).abs() < 1e-4);
        // close to the intersection: within the length of the strike symbol
        assert!(p.distance(c) < 16.0);
    }

    #[test]
    fn bars_and_arrows_are_proper_polygons() {
        let b = bar(Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0), 4.0);
        assert_eq!(b.len(), 4);
        assert!((b[0].y - b[3].y).abs() > 3.9);
        let (shaft, head) = arrow_polys(Pos2::ZERO, Vec2::new(1.0, 0.0), 20.0);
        assert_eq!((shaft.len(), head.len()), (4, 3));
        // the tip is at the full length
        assert!((head[0].x - 20.0).abs() < 1e-4);
        // a zero length bar does not blow up
        assert_eq!(bar(Pos2::ZERO, Pos2::ZERO, 4.0).len(), 4);
    }

    #[test]
    fn the_gis_pane_settings_survive_the_preferences_file() {
        use super::super::gis_layers::Drop;
        let dir = std::env::temp_dir().join(format!("gis-prefs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (map, map2, table) = (dir.join("m.png"), dir.join("m2.png"), dir.join("t.csv"));
        std::fs::write(&map, b"x").unwrap();
        std::fs::write(&map2, b"x").unwrap();
        std::fs::write(&table, "ID,E,N\nA,1,2\n").unwrap();
        let mut g = Gis::default();
        g.base_on = true;
        g.show_fol = false;
        g.show_lin = false;
        g.show_num = false;
        g.show_names = true;
        g.show_layers = true;
        g.size = 22.0;
        g.view = View { cx: 1234.5, cy: -678.25, mpp: 3.5 };
        g.manual.zone = 43;
        g.manual.north = false;
        // two maps, one of them in a group and switched off, the second one selected
        let a = g.layers.add_map("Geology A");
        let b = g.layers.add_map("Structure B");
        let grp = g.layers.add_group("Old maps");
        assert!(g.layers.move_item(a, grp, Drop::Inside));
        g.layers.set_visible(a, false);
        g.restore_maps.insert(a, MapSpec { path: map.clone(), opacity: 0.4, georef: Some("{\"x\":1}".into()) });
        g.restore_maps.insert(b, MapSpec { path: map2.clone(), opacity: 0.9, georef: None });
        g.sel = Some(b);
        g.restore_table = Some(table.clone());
        g.restore_mapping = Some(Mapping { header_row: 1, name: Some(0), x: Some(1), y: Some(2), crs: CrsChoice { kind: CoordKind::Utm, zone: 43, ..Default::default() }, ..Default::default() });
        let text = g.prefs_text();
        assert!(text.lines().all(|l| l.contains('=')), "{text}");
        let mut h = Gis::default();
        for l in text.lines() {
            let (k, v) = l.split_once('=').unwrap();
            assert!(h.load_pref(k, v), "{k}");
        }
        assert_eq!((h.base_on, h.show_fol, h.show_lin, h.show_num, h.show_names, h.show_layers, h.size), (true, false, false, false, true, true, 22.0));
        assert_eq!((h.view.cx, h.view.cy, h.view.mpp, h.fit_pending), (1234.5, -678.25, 3.5, false));
        assert_eq!((h.manual.zone, h.manual.north), (43, false));
        // the layers come back as they were: order, group, switches, names, selection, and what the maps need
        assert_eq!(h.layers, g.layers);
        assert_eq!(h.sel, Some(b));
        assert_eq!(h.restore_maps.get(&a).map(|s| (s.path.clone(), s.georef.clone())), Some((map.clone(), Some("{\"x\":1}".to_string()))));
        assert!((h.restore_maps[&b].opacity - 0.9).abs() < 1e-6 && (h.restore_maps[&a].opacity - 0.4).abs() < 1e-6);
        assert_eq!(h.restore_table.as_ref(), Some(&table));
        assert_eq!(h.restore_mapping.as_ref().map(|m| (m.crs.kind, m.crs.zone, m.x)), Some((CoordKind::Utm, 43, Some(1))));
        // a map whose file has gone is dropped from the list when the preferences are read
        std::fs::remove_file(&map2).unwrap();
        let mut k = Gis::default();
        for l in text.lines() {
            let (key, v) = l.split_once('=').unwrap();
            k.load_pref(key, v);
        }
        assert!(k.layers.get(b).is_none() && k.layers.get(a).is_some());
        assert_eq!(k.sel, None);
        // silly values are dropped, not an error
        let mut z = Gis::default();
        z.load_pref("gis_view", "1,2,99999");
        assert!(z.fit_pending);
        z.load_pref("gis_layers", "not json");
        assert_eq!(z.layers, LayerTree::default());
        assert!(!z.load_pref("other_key", "1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn screen_directions_follow_the_compass() {
        let n = dirvec(0.0);
        let e = dirvec(90.0);
        assert!(n.x.abs() < 1e-6 && (n.y + 1.0).abs() < 1e-6);
        assert!((e.x - 1.0).abs() < 1e-6 && e.y.abs() < 1e-6);
        let sw = dirvec(225.0);
        assert!(sw.x < 0.0 && sw.y > 0.0);
    }
}
