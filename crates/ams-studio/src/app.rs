use crate::doc::{self, Derived, Doc};
use crate::icons::{self, Icon};
use crate::plots::{self, AxisCfg, ColorCfg, Pt, ScatterSpec, StereoOpts, Theme};
use ams_core::params::{param_class, param_sd, param_values, N_PARAMS, PARAM_LABELS};
mod analysis_ui;
mod tools_ui;
use ams_core::coords::{CoordSys, Orientation};
use ams_core::export;
use egui::{Align, Align2, Color32, CornerRadius, FontId, Key, Layout, Modifiers, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Open,
    Prev,
    Next,
    Reload,
    Import,
    SaveActive,
    AppendSel,
    ExportCsv,
    ExportSelection,
    Close,
    Exit,
    EditData,
    SetClasses,
    InvertSel,
    Disregard,
    RestoreAll,
    ClearSel,
    CopyGraphics,
    SaveGraphics,
    DataTable,
    Phase(bool),
    Coord(CoordSys),
    GroupStats,
    Contour,
    Tool(&'static str),
    Factors,
    OrientParams,
    Colors,
    About,
    ToggleBrowser,
    ToggleTheme,
    StepSpecimen(i32),
    TogglePhase,
}

/// Options of one of the two scatter plots (P vs Kmean, T vs P).
#[derive(Clone, PartialEq)]
struct ScCfg {
    x: AxisCfg,
    y: AxisCfg,
    diag: bool,
    conf: bool,
    connect: bool,
    legend: bool,
}

impl ScCfg {
    fn new(x: usize, y: usize) -> ScCfg {
        ScCfg { x: AxisCfg::new(x), y: AxisCfg::new(y), diag: false, conf: false, connect: false, legend: true }
    }
}

impl PartialEq for AxisCfg {
    fn eq(&self, o: &Self) -> bool {
        self.param == o.param && self.auto == o.auto && self.min == o.min && self.max == o.max && self.log == o.log
    }
}
impl PartialEq for ColorCfg {
    fn eq(&self, o: &Self) -> bool {
        self.on == o.on && self.param == o.param && self.auto == o.auto && self.min == o.min && self.max == o.max && self.log == o.log && self.palette == o.palette
    }
}

fn class_color(class: u8, dark: bool) -> Option<Color32> {
    let c = match class {
        1 => (255, 188, 188),
        2 => (190, 250, 190),
        3 => (196, 196, 255),
        4 => (190, 250, 250),
        5 => (255, 190, 250),
        _ => return None,
    };
    Some(tools_ui::tint(dark, c.0, c.1, c.2))
}

/// Parameters grouped by type, one column per group, in the order of the original's list.
const PARAM_GROUPS: [(&str, &[usize]); 7] = [
    ("Magnitude", &[0]),
    ("Shape", &[1, 2, 3, 4, 5, 6, 7, 8]),
    ("K1 (max)", &[9, 12, 13]),
    ("K2 (int)", &[10, 14, 15]),
    ("K3 (min)", &[11, 16, 17]),
    ("F-tests", &[18, 19, 20]),
    ("Confidence", &[21, 22, 23]),
];

/// Drop-down of all plottable parameters. It opens as one wide panel with the parameters
/// grouped by type in columns, each on the background colour the original uses.
fn param_combo(ui: &mut Ui, id: &str, v: &mut usize, dark: bool) -> bool {
    let mut changed = false;
    let cur = RichText::new(PARAM_LABELS[(*v).min(N_PARAMS - 1)]);
    let cur = match class_color(param_class(*v), dark) { Some(c) => cur.background_color(c).color(Color32::BLACK), None => cur };
    egui::ComboBox::from_id_salt(id).width(120.0).selected_text(cur).show_ui(ui, |ui| {
        ui.horizontal_top(|ui| {
            for (title, items) in PARAM_GROUPS {
                ui.vertical(|ui| {
                    ui.set_min_width(88.0);
                    ui.label(RichText::new(title).small().strong());
                    for &i in items {
                        let t = RichText::new(PARAM_LABELS[i]);
                        let t = match class_color(param_class(i), dark) { Some(c) => t.background_color(c).color(Color32::BLACK), None => t };
                        if ui.add(egui::Button::selectable(*v == i, t).min_size(Vec2::new(84.0, 0.0))).clicked() {
                            *v = i;
                            changed = true;
                            ui.close();
                        }
                    }
                });
                ui.add_space(6.0);
            }
        });
    });
    changed
}

fn range_fields(ui: &mut Ui, auto: &mut bool, min: &mut f64, max: &mut f64, log: &mut bool, id: &str) {
    ui.horizontal(|ui| {
        ui.checkbox(auto, "Auto-scale");
        egui::ComboBox::from_id_salt((id, "scale")).width(96.0).selected_text(if *log { "Log (base 10)" } else { "Linear" }).show_ui(ui, |ui| {
            ui.selectable_value(log, false, "Linear");
            ui.selectable_value(log, true, "Log (base 10)");
        });
    });
    ui.add_enabled_ui(!*auto, |ui| {
        let speed = 0.01 * max.abs().max(min.abs()).max(1e-6);
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(min).speed(speed).max_decimals(8));
            ui.label("to");
            ui.add(egui::DragValue::new(max).speed(speed).max_decimals(8));
        });
    });
}

fn axis_ui(ui: &mut Ui, title: &str, id: &str, cfg: &mut AxisCfg, dark: bool) {
    ui.label(RichText::new(title).strong());
    param_combo(ui, id, &mut cfg.param, dark);
    range_fields(ui, &mut cfg.auto, &mut cfg.min, &mut cfg.max, &mut cfg.log, id);
}

fn color_ui(ui: &mut Ui, id: &str, c: &mut ColorCfg, dark: bool) {
    ui.checkbox(&mut c.on, RichText::new("Color-coding").strong());
    ui.add_enabled_ui(c.on, |ui| {
        param_combo(ui, id, &mut c.param, dark);
        range_fields(ui, &mut c.auto, &mut c.min, &mut c.max, &mut c.log, id);
        ui.horizontal(|ui| {
            ui.radio_value(&mut c.palette, 0, "Rainbow");
            ui.radio_value(&mut c.palette, 1, "Shades");
            ui.radio_value(&mut c.palette, 2, "Grayscales");
        });
    });
}

/// A drag over rows of a list: anchor position, whether it adds to the previous selection, and that selection.
struct ListDrag {
    anchor: usize,
    additive: bool,
    base: Vec<bool>,
}

// window slots (index into App::win_serial)
const W_TABLE: usize = 0;
const W_GROUP: usize = 1;
const W_CONTOUR: usize = 2;
const W_ORIENT: usize = 3;
const W_COLORS: usize = 4;
const W_FACTORS: usize = 5;
const W_ABOUT: usize = 6;
const W_EDIT: usize = 7;

struct ShotJob {
    copy: bool,
    base: Option<PathBuf>,
    rects: Vec<(&'static str, Rect)>,
}

pub struct App {
    doc: Option<Doc>,
    derived: Vec<Derived>,
    dirty: bool,
    out_phase: bool,
    coord: CoordSys,
    orient: Orientation,

    hfrac: f32,
    vfrac: f32,
    browser_open: bool,
    browser_dir: PathBuf,
    browser_back: Vec<PathBuf>,
    spec_color_open: bool,

    show_k: [bool; 3],
    marker: f32,
    bins: usize,
    export: [bool; 4],
    hist_cfg: AxisCfg,
    sc: [ScCfg; 2],
    color: [ColorCfg; 4],
    color_master: ColorCfg,
    st_data: bool,
    st_caption: bool,
    st_legend: bool,
    st_connect: bool,
    st_mf: bool,
    st_overlay: analysis_ui::StereoOverlay,
    browser_w: f32,
    spec_w: f32,
    win_size: Option<[f32; 2]>,
    win_max: bool,
    layout_changed: Option<f64>,
    scroll_to: Option<usize>,
    list_off: f32,
    list_h: f32,
    pane_settings: [bool; 4],
    filter: usize,
    k_colors: [Color32; 3],
    dark: bool,

    win_table: bool,
    win_group: bool,
    win_contour: bool,
    win_orient: bool,
    win_colors: bool,
    win_factors: bool,
    win_about: bool,
    win_edit: bool,
    ana: analysis_ui::AnaState,
    edit_name: String,
    info: Option<String>,
    hover: [String; 4],
    last_anchor: Option<usize>,
    win_serial: [u64; 16],
    list_drag: Option<ListDrag>,
    tools: tools_ui::ToolsState,
    tool_copy: Option<Rect>,
    shot: Option<ShotJob>,
    shot_box: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<egui::ColorImage>>>>,
    last_title: String,
}

/// Window size and maximised flag saved by the previous run (read before the window is created).
pub fn saved_window() -> (Option<[f32; 2]>, bool) {
    let Some(s) = read_state_text() else { return (None, false) };
    let (mut w, mut h, mut max) = (None, None, false);
    for l in s.lines() {
        match l.split_once('=') {
            Some(("win_w", v)) => w = v.parse::<f32>().ok(),
            Some(("win_h", v)) => h = v.parse::<f32>().ok(),
            Some(("maximized", v)) => max = v == "1",
            _ => {}
        }
    }
    (w.zip(h).map(|(w, h)| [w.clamp(760.0, 8000.0), h.clamp(480.0, 8000.0)]), max)
}

/// The user's home folder on any platform ($HOME on Linux/macOS, %USERPROFILE% on Windows).
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

/// Where the layout and preferences are kept: $XDG_CONFIG_HOME or ~/.config on Linux,
/// %APPDATA% on Windows.
fn config_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(x);
    }
    if cfg!(windows) {
        if let Some(a) = std::env::var_os("APPDATA") {
            return PathBuf::from(a);
        }
    }
    home_dir().join(".config")
}

/// Marker size in the plots for a fresh installation.
const DEFAULT_MARKER: f32 = 6.0;

fn state_path() -> PathBuf {
    config_dir().join("ams-studio").join("state.txt")
}

/// Preferences text: the current file, or the one written under the program's former name.
fn read_state_text() -> Option<String> {
    std::fs::read_to_string(state_path()).ok().or_else(|| std::fs::read_to_string(config_dir().join("anisoft-linux").join("state.txt")).ok())
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> App {
        let mut app = App {
            doc: None,
            derived: vec![],
            dirty: true,
            out_phase: false,
            coord: CoordSys::Specimen,
            orient: Orientation::default(),
            hfrac: 0.5,
            vfrac: 0.5,
            browser_open: true,
            browser_dir: home_dir(),
            browser_back: vec![],
            spec_color_open: false,
            show_k: [true; 3],
            marker: DEFAULT_MARKER,
            bins: 12,
            export: [true; 4],
            hist_cfg: AxisCfg::new(0),
            sc: [ScCfg::new(0, 3), { let mut s = ScCfg::new(3, 5); s.y.auto = false; s.y.min = -1.0; s.y.max = 1.0; s }],
            color: Default::default(),
            color_master: ColorCfg::default(),
            st_data: true,
            st_caption: true,
            st_legend: true,
            st_connect: false,
            st_mf: false,
            st_overlay: Default::default(),
            browser_w: 230.0,
            spec_w: 170.0,
            win_size: None,
            win_max: false,
            layout_changed: None,
            scroll_to: None,
            list_off: 0.0,
            list_h: 0.0,
            pane_settings: [false; 4],
            filter: 0,
            k_colors: [Color32::from_rgb(220, 30, 30), Color32::from_rgb(30, 170, 60), Color32::from_rgb(30, 60, 220)],
            dark: false,
            win_table: false,
            win_group: false,
            win_contour: false,
            win_orient: false,
            win_colors: false,
            win_factors: false,
            win_about: false,
            win_edit: false,
            ana: Default::default(),
            edit_name: String::new(),
            info: None,
            hover: Default::default(),
            last_anchor: None,
            win_serial: [0; 16],
            list_drag: None,
            tools: Default::default(),
            tool_copy: None,
            shot: None,
            shot_box: Default::default(),
            last_title: String::new(),
        };
        app.load_state();
        apply_style(&cc.egui_ctx, app.dark);
        if let Some(p) = open {
            app.open_path(p);
        }
        app
    }

    fn load_state(&mut self) {
        let Some(s) = read_state_text() else { return };
        for l in s.lines() {
            let Some((k, v)) = l.split_once('=') else { continue };
            match k {
                "last_dir" if Path::new(v).is_dir() => self.browser_dir = PathBuf::from(v),
                "dark" => self.dark = v == "1",
                "logkm" => self.sc[0].x.log = v == "1",
                "marker" => self.marker = v.parse().unwrap_or(DEFAULT_MARKER).clamp(2.0, 9.0),
                "browser_w" => self.browser_w = v.parse().unwrap_or(230.0f32).clamp(150.0, 520.0),
                "spec_w" => self.spec_w = v.parse().unwrap_or(170.0f32).clamp(110.0, 420.0),
                "win_w" | "win_h" => {
                    let x: f32 = v.parse().unwrap_or(0.0);
                    let cur = self.win_size.get_or_insert([1280.0, 820.0]);
                    cur[usize::from(k == "win_h")] = x.clamp(760.0, 8000.0);
                }
                "maximized" => self.win_max = v == "1",
                "browser" => self.browser_open = v != "0",
                "hfrac" => self.hfrac = v.parse().unwrap_or(0.5f32).clamp(0.2, 0.8),
                "vfrac" => self.vfrac = v.parse().unwrap_or(0.5f32).clamp(0.2, 0.8),
                _ => {}
            }
        }
    }

    fn save_state(&self) {
        let p = state_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(
            p,
            format!(
                "last_dir={}\ndark={}\nbrowser={}\nhfrac={}\nvfrac={}\nlogkm={}\nmarker={}\nbrowser_w={}\nspec_w={}\nwin_w={}\nwin_h={}\nmaximized={}\n",
                self.browser_dir.display(),
                self.dark as u8,
                self.browser_open as u8,
                self.hfrac,
                self.vfrac,
                self.sc[0].x.log as u8,
                self.marker,
                self.browser_w,
                self.spec_w,
                self.win_size.map_or(1280.0, |s| s[0]),
                self.win_size.map_or(820.0, |s| s[1]),
                self.win_max as u8
            ),
        );
    }

    fn open_path(&mut self, p: PathBuf) {
        match Doc::open(&p) {
            Ok(d) => {
                if let Some(parent) = p.parent() {
                    self.browser_dir = parent.to_path_buf();
                }
                self.doc = Some(d);
                self.dirty = true;
                // recompute now: the rest of this frame (plots, table) indexes `derived` per specimen
                self.refresh();
                self.last_anchor = None;
                self.save_state();
            }
            Err(e) => self.info = Some(format!("Could not open file:\n{e}")),
        }
    }

    fn theme(&self) -> Theme {
        Theme { dark: self.dark, k_colors: self.k_colors, accent: Color32::from_rgb(59, 130, 246) }
    }

    fn refresh(&mut self) {
        if self.dirty {
            self.derived = self.doc.as_ref().map(|d| d.derive(self.out_phase, self.coord, &self.orient)).unwrap_or_default();
            self.dirty = false;
        }
    }

    /// The Specimens-panel / toolbar colour-coding applies to every plot that has markers.
    fn apply_master_color(&mut self) {
        for i in [0usize, 2, 3] {
            self.color[i] = self.color_master.clone();
        }
    }

    fn points(&self) -> Vec<Pt> {
        let Some(doc) = &self.doc else { return vec![] };
        if self.derived.len() != doc.specs.len() {
            return vec![];
        }
        (0..doc.specs.len())
            .filter(|&i| doc.active[i] && (self.filter == 0 || doc.selected[i]))
            .map(|i| {
                let d = &self.derived[i];
                let ph = if self.out_phase { &doc.specs[i].outphase } else { &doc.specs[i].inphase };
                let vals = param_values(d.km, &d.axes, ph);
                Pt {
                    idx: i,
                    name: doc.specs[i].name.clone(),
                    di: d.axes.dec_inc,
                    vals,
                    sds: [param_sd(9, ph), param_sd(10, ph), param_sd(11, ph)],
                    selected: doc.selected[i],
                    color: None,
                }
            })
            .collect()
    }

    /// Copy of the points coloured according to `cfg`, and the colour range when colouring is on.
    fn colorized(pts: &[Pt], cfg: &ColorCfg) -> (Vec<Pt>, Option<(f64, f64)>) {
        if !cfg.on {
            return (pts.to_vec(), None);
        }
        let k = cfg.param.min(N_PARAMS - 1);
        let range = cfg.range(pts.iter().map(|p| p.vals[k]));
        let out = pts.iter().map(|p| Pt { color: cfg.color(p.vals[k], range), ..p.clone() }).collect();
        (out, Some(range))
    }

    // ---------------------------------------------------------------- actions

    fn run(&mut self, ctx: &egui::Context, a: Action) {
        let has = self.doc.is_some();
        match a {
            Action::Open => {
                let mut dlg = rfd::FileDialog::new().add_filter("AMS files", &["ams"]).set_title("Open Data File(s)");
                if self.browser_dir.is_dir() {
                    dlg = dlg.set_directory(&self.browser_dir);
                }
                if let Some(p) = dlg.pick_file() {
                    self.open_path(p);
                }
            }
            Action::Prev | Action::Next => {
                if let Some(d) = self.doc.as_ref().filter(|d| !d.synthetic) {
                    let (prev, next, _, _) = doc::neighbours(&d.path);
                    if let Some(p) = if a == Action::Prev { prev } else { next } {
                        self.open_path(p);
                    }
                }
            }
            Action::Reload => {
                if let Some(d) = self.doc.as_ref().filter(|d| !d.synthetic) {
                    let p = d.path.clone();
                    self.open_path(p);
                }
            }
            Action::Import => self.info = Some("Import Data File(s) is not implemented yet: this version reads Safyr7 .ams files only.".into()),
            Action::SaveActive => {
                if let Some(d) = &self.doc {
                    let name = d.path.file_stem().map(|s| format!("{}_active.ams", s.to_string_lossy())).unwrap_or("active.ams".into());
                    if let Some(p) = rfd::FileDialog::new().add_filter("AMS files", &["ams"]).set_file_name(name).save_file() {
                        let bytes = d.records((0..d.specs.len()).filter(|&i| d.active[i]));
                        if let Err(e) = std::fs::write(&p, bytes) {
                            self.info = Some(format!("Could not save: {e}"));
                        }
                    }
                }
            }
            Action::AppendSel => {
                if let Some(d) = &self.doc {
                    if d.n_selected() == 0 {
                        self.info = Some("Select some specimens first.".into());
                    } else if let Some(p) = rfd::FileDialog::new().add_filter("AMS files", &["ams"]).set_title("Append selected records into...").pick_file() {
                        let ok = std::fs::metadata(&p).map_or(false, |m| m.len() % ams_core::ams::RECORD_LEN as u64 == 0);
                        if !ok {
                            self.info = Some("That is not a valid .ams file (size is not a multiple of 640 bytes).".into());
                        } else {
                            use std::io::Write;
                            let bytes = d.records((0..d.specs.len()).filter(|&i| d.selected[i]));
                            let r = std::fs::OpenOptions::new().append(true).open(&p).and_then(|mut f| f.write_all(&bytes));
                            if let Err(e) = r {
                                self.info = Some(format!("Could not append: {e}"));
                            }
                        }
                    }
                }
            }
            Action::ExportCsv => {
                if let Some(d) = &self.doc {
                    let name = d.path.file_stem().map(|s| format!("{}_ip_op.csv", s.to_string_lossy())).unwrap_or("export.csv".into());
                    if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name(name).save_file() {
                        let rows: Vec<&_> = (0..d.specs.len()).filter(|&i| d.active[i]).map(|i| &d.specs[i]).collect();
                        if let Err(e) = std::fs::write(&p, export::csv_both(&rows)) {
                            self.info = Some(format!("Could not export: {e}"));
                        }
                    }
                }
            }
            Action::ExportSelection => {
                if let Some(d) = &self.doc {
                    let rows: Vec<&_> = (0..d.specs.len()).filter(|&i| d.selected[i]).map(|i| &d.specs[i]).collect();
                    if rows.is_empty() {
                        self.info = Some("Nothing is selected. Select specimens first (click, drag, or a rubber band in a plot).".into());
                    } else {
                        let name = d.path.file_stem().map(|s| format!("{}_selection_ip_op.csv", s.to_string_lossy())).unwrap_or("selection.csv".into());
                        if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name(name).save_file() {
                            if let Err(e) = std::fs::write(&p, export::csv_both(&rows)) {
                                self.info = Some(format!("Could not export: {e}"));
                            }
                        }
                    }
                }
            }
            Action::Close => {
                self.doc = None;
                self.dirty = true;
            }
            Action::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::EditData => {
                if let Some(d) = &self.doc {
                    let sel: Vec<usize> = (0..d.specs.len()).filter(|&i| d.selected[i]).collect();
                    if sel.len() == 1 {
                        self.edit_name = d.specs[sel[0]].name.clone();
                        self.open_win(W_EDIT);
                    } else {
                        self.info = Some("Select exactly one specimen to edit its name.".into());
                    }
                }
            }
            Action::SetClasses => self.info = Some("Data classes are not implemented yet. Colour-coding by Km, P, Pj or T is available in the Specimens pane.".into()),
            Action::InvertSel => {
                if let Some(d) = &mut self.doc {
                    for s in d.selected.iter_mut() {
                        *s = !*s;
                    }
                }
            }
            Action::Disregard => {
                if let Some(d) = &mut self.doc {
                    for i in 0..d.specs.len() {
                        if d.selected[i] {
                            d.active[i] = false;
                            d.selected[i] = false;
                        }
                    }
                }
            }
            Action::RestoreAll => {
                if let Some(d) = &mut self.doc {
                    d.active.iter_mut().for_each(|a| *a = true);
                }
            }
            Action::ClearSel => {
                if let Some(d) = &mut self.doc {
                    d.selected.iter_mut().for_each(|a| *a = false);
                }
            }
            Action::CopyGraphics | Action::SaveGraphics => {
                if has {
                    let copy = a == Action::CopyGraphics;
                    let base = if copy {
                        None
                    } else {
                        let stem = self.doc.as_ref().and_then(|d| d.path.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or("ams-studio".into());
                        match rfd::FileDialog::new().add_filter("PNG base name", &["png"]).set_file_name(format!("{stem}.png")).set_title("Save Graphics To File").save_file() {
                            Some(p) => Some(p),
                            None => return,
                        }
                    };
                    self.shot = Some(ShotJob { copy, base, rects: vec![] });
                    let slot = self.shot_box.clone();
                    let c2 = ctx.clone();
                    ctx.request_screenshot(move |img| {
                        *slot.lock().unwrap() = Some(img);
                        c2.request_repaint();
                    });
                }
            }
            Action::DataTable => self.open_win(W_TABLE),
            Action::Phase(op) => {
                self.out_phase = op;
                self.dirty = true;
            }
            Action::TogglePhase => {
                if has {
                    self.out_phase = !self.out_phase;
                    self.dirty = true;
                }
            }
            Action::StepSpecimen(dir) => {
                if let Some(d) = &mut self.doc {
                    // walk the active specimens in list order, starting from the current selection
                    let order: Vec<usize> = d.order().into_iter().filter(|&i| d.active[i]).collect();
                    if !order.is_empty() {
                        let cur = self
                            .last_anchor
                            .filter(|&i| i < d.selected.len() && d.selected[i] && d.active[i])
                            .or_else(|| order.iter().copied().find(|&i| d.selected[i]))
                            .and_then(|i| order.iter().position(|&o| o == i));
                        let pos = match (cur, dir > 0) {
                            (Some(p), true) => (p + 1).min(order.len() - 1),
                            (Some(p), false) => p.saturating_sub(1),
                            (None, true) => 0,
                            (None, false) => order.len() - 1,
                        };
                        let i = order[pos];
                        d.selected.iter_mut().for_each(|s| *s = false);
                        d.selected[i] = true;
                        self.last_anchor = Some(i);
                        self.scroll_to = Some(pos);
                    }
                }
            }
            Action::Coord(c) => {
                self.coord = c;
                self.dirty = true;
            }
            Action::GroupStats => self.open_win(W_GROUP),
            Action::Contour => self.open_win(W_CONTOUR),
            Action::Tool(name) => self.open_tool(name),
            Action::Factors => self.open_win(W_FACTORS),
            Action::OrientParams => self.open_win(W_ORIENT),
            Action::Colors => self.open_win(W_COLORS),
            Action::About => self.open_win(W_ABOUT),
            Action::ToggleBrowser => {
                self.browser_open = !self.browser_open;
                self.save_state();
            }
            Action::ToggleTheme => {
                self.dark = !self.dark;
                apply_style(ctx, self.dark);
                self.save_state();
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = vec![];
        let c = Modifiers::CTRL;
        let table: Vec<(Modifiers, Key, Action)> = vec![
            (Modifiers::SHIFT | Modifiers::CTRL, Key::F12, Action::ClearSel),
            (Modifiers::NONE, Key::Escape, Action::ClearSel),
            (c, Key::F1, Action::SetClasses),
            (c, Key::O, Action::Open),
            (c, Key::P, Action::Prev),
            (c, Key::N, Action::Next),
            (c, Key::R, Action::Reload),
            (c, Key::I, Action::Import),
            (c, Key::S, Action::SaveActive),
            (c, Key::A, Action::AppendSel),
            (Modifiers::CTRL | Modifiers::SHIFT, Key::E, Action::ExportSelection),
            (c, Key::E, Action::ExportCsv),
            (c, Key::Q, Action::Close),
            (c, Key::X, Action::Exit),
            (c, Key::C, Action::CopyGraphics),
            (c, Key::F, Action::SaveGraphics),
            (c, Key::B, Action::ToggleBrowser),
            (Modifiers::SHIFT, Key::Insert, Action::InvertSel),
            (Modifiers::SHIFT, Key::Delete, Action::Disregard),
            (Modifiers::ALT, Key::Backspace, Action::RestoreAll),
            (Modifiers::NONE, Key::F1, Action::EditData),
            (Modifiers::NONE, Key::F2, Action::DataTable),
            (Modifiers::NONE, Key::F3, Action::Phase(false)),
            (Modifiers::NONE, Key::F4, Action::Phase(true)),
            (Modifiers::NONE, Key::Q, Action::StepSpecimen(-1)),
            (Modifiers::NONE, Key::E, Action::StepSpecimen(1)),
            (Modifiers::NONE, Key::W, Action::TogglePhase),
            (Modifiers::NONE, Key::F5, Action::Coord(CoordSys::Specimen)),
            (Modifiers::NONE, Key::F6, Action::Coord(CoordSys::Geographic)),
            (Modifiers::NONE, Key::F7, Action::Coord(CoordSys::Paleo)),
            (Modifiers::NONE, Key::F8, Action::Coord(CoordSys::Tectonic)),
            (Modifiers::NONE, Key::F9, Action::GroupStats),
            (Modifiers::NONE, Key::F12, Action::Contour),
        ];
        if ctx.egui_wants_keyboard_input() {
            return out;
        }
        for (m, k, a) in table {
            if ctx.input_mut(|i| i.consume_key(m, k)) {
                out.push(a);
            }
        }
        out
    }
}

pub fn apply_style(ctx: &egui::Context, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let accent = Color32::from_rgb(59, 130, 246);
    v.selection.bg_fill = accent.gamma_multiply(if dark { 0.55 } else { 0.35 });
    v.selection.stroke = Stroke::new(1.0, accent);
    v.hyperlink_color = accent;
    let r = CornerRadius::same(6);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    if !dark {
        v.panel_fill = Color32::from_rgb(244, 246, 249);
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(6.0, 5.0);
        s.spacing.button_padding = Vec2::new(8.0, 4.0);
    });
}

// -------------------------------------------------------------------- UI

fn tool(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool, selected: bool, dark: bool) -> bool {
    let size = Vec2::splat(30.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let pa = ui.painter();
        if selected {
            pa.rect_filled(rect, 6.0, ui.visuals().selection.bg_fill);
        } else if resp.hovered() && enabled {
            pa.rect_filled(rect, 6.0, ui.visuals().widgets.hovered.bg_fill);
        }
        let inner = Rect::from_center_size(rect.center(), icons::size());
        if enabled {
            icons::paint(pa, inner, icon, dark);
        } else {
            let lay = ui.painter().clone().with_layer_id(ui.layer_id());
            icons::paint(&lay, inner, icon, dark);
            pa.rect_filled(rect, 6.0, ui.visuals().panel_fill.gamma_multiply(0.62));
        }
    }
    resp.on_hover_text(tip).clicked() && enabled
}

fn sep(ui: &mut Ui) {
    ui.add_space(4.0);
    let (r, _) = ui.allocate_exact_size(Vec2::new(1.0, 24.0), Sense::hover());
    ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    ui.add_space(4.0);
}

fn small_icon(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool, dark: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), if enabled { Sense::click() } else { Sense::hover() });
    let pa = ui.painter();
    if resp.hovered() && enabled {
        pa.rect_filled(rect, 6.0, ui.visuals().widgets.hovered.bg_fill);
    }
    icons::paint(pa, Rect::from_center_size(rect.center(), Vec2::splat(18.0)), icon, dark);
    if !enabled {
        pa.rect_filled(rect, 6.0, ui.visuals().panel_fill.gamma_multiply(0.62));
    }
    resp.on_hover_text(tip).clicked() && enabled
}

fn item(ui: &mut Ui, label: &str, sc: &str, enabled: bool) -> bool {
    let b = egui::Button::new(label).shortcut_text(sc);
    let r = ui.add_enabled(enabled, b).clicked();
    if r {
        ui.close();
    }
    r
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.track_layout(&ctx);
        self.refresh();
        let mut actions = self.shortcuts(&ctx);
        self.handle_screenshot(&ctx);

        let has = self.doc.is_some();
        let th = self.theme();

        // window title like the original: Anisoft - [path (n / total)]
        let title = match &self.doc {
            Some(d) if d.synthetic => format!("AMS Studio - [{}]", d.path.display()),
            Some(d) => {
                let (_, _, pos, total) = doc::neighbours(&d.path);
                format!("AMS Studio - [{}  ({pos} / {total})]{}", d.path.display(), if d.modified { " *" } else { "" })
            }
            None => "AMS Studio".to_string(),
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        // ------------------------------------------------------------ menu bar
        egui::Panel::top("menubar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if item(ui, "Open Data File(s)", "Ctrl+O", true) { actions.push(Action::Open) }
                    if item(ui, "Open Previous Data File", "Ctrl+P", has) { actions.push(Action::Prev) }
                    if item(ui, "Open Next Data File", "Ctrl+N", has) { actions.push(Action::Next) }
                    if item(ui, "Reload Current Data File(s)", "Ctrl+R", has) { actions.push(Action::Reload) }
                    ui.separator();
                    if item(ui, "Import Data File(s)", "Ctrl+I", true) { actions.push(Action::Import) }
                    ui.separator();
                    if item(ui, "Save Active Records As", "Ctrl+S", has) { actions.push(Action::SaveActive) }
                    if item(ui, "Append Selected Records Into", "Ctrl+A", has) { actions.push(Action::AppendSel) }
                    if item(ui, "Export Data File (CSV, in- and out-of-phase)", "Ctrl+E", has) { actions.push(Action::ExportCsv) }
                    if item(ui, "Export Selection (CSV, in- and out-of-phase)", "Ctrl+Shift+E", has) { actions.push(Action::ExportSelection) }
                    ui.separator();
                    if item(ui, "Close Current Data File(s)", "Ctrl+Q", has) { actions.push(Action::Close) }
                    if item(ui, "Exit Program", "Ctrl+X", true) { actions.push(Action::Exit) }
                });
                ui.menu_button("Edit", |ui| {
                    if item(ui, "Edit Data", "F1", has) { actions.push(Action::EditData) }
                    if item(ui, "Set Data Classes", "Ctrl+F1", has) { actions.push(Action::SetClasses) }
                    ui.separator();
                    if item(ui, "Invert Selection", "Shift+Ins", has) { actions.push(Action::InvertSel) }
                    if item(ui, "Disregard Selected Record(s)", "Shift+Del", has) { actions.push(Action::Disregard) }
                    if item(ui, "Restore All Records", "Alt+Bksp", has) { actions.push(Action::RestoreAll) }
                    if item(ui, "Clear Selection", "Shift+Ctrl+F12", has) { actions.push(Action::ClearSel) }
                });
                ui.menu_button("Graphics", |ui| {
                    if item(ui, "Copy Graphics To Clipboard", "Ctrl+C", has) { actions.push(Action::CopyGraphics) }
                    if item(ui, "Save Graphics To File", "Ctrl+F", has) { actions.push(Action::SaveGraphics) }
                });
                ui.menu_button("View", |ui| {
                    if item(ui, "Data Table", "F2", has) { actions.push(Action::DataTable) }
                    ui.separator();
                    if item(ui, "In-Phase AMS", "F3", has) { actions.push(Action::Phase(false)) }
                    if item(ui, "Out-of-Phase AMS", "F4", has) { actions.push(Action::Phase(true)) }
                    if item(ui, "Switch In / Out-of-Phase", "W", has) { actions.push(Action::TogglePhase) }
                    ui.separator();
                    if item(ui, "Previous Specimen", "Q", has) { actions.push(Action::StepSpecimen(-1)) }
                    if item(ui, "Next Specimen", "E", has) { actions.push(Action::StepSpecimen(1)) }
                    ui.separator();
                    ui.label(RichText::new("In-Phase AMS Coordinates").weak());
                    if item(ui, "Specimen Coordinates (SPEC)", "F5", has) { actions.push(Action::Coord(CoordSys::Specimen)) }
                    if item(ui, "Geographic Coordinates (GEO)", "F6", has) { actions.push(Action::Coord(CoordSys::Geographic)) }
                    if item(ui, "Paleogeographic Coordinates (PALEO)", "F7", has) { actions.push(Action::Coord(CoordSys::Paleo)) }
                    if item(ui, "Tectonic Coordinates (TECTO)", "F8", has) { actions.push(Action::Coord(CoordSys::Tectonic)) }
                    ui.separator();
                    if item(ui, "Paleo-Horizontal", "", false) {}
                    if item(ui, "Paleo-Vertical", "", false) {}
                    ui.separator();
                    if item(ui, "File Browser", "Ctrl+B", true) { actions.push(Action::ToggleBrowser) }
                    if item(ui, if self.dark { "Light Theme" } else { "Dark Theme" }, "", true) { actions.push(Action::ToggleTheme) }
                });
                ui.menu_button("Analysis", |ui| {
                    if item(ui, "Group Statistics", "F9", has) { actions.push(Action::GroupStats) }
                    if item(ui, "Contour Plot", "F12", has) { actions.push(Action::Contour) }
                });
                ui.menu_button("Tools", |ui| {
                    for n in ["GED Editor", "Tensor Addition / Subtraction", "Fabric Overprint", "Intersection Toolbox", "Deform AMS"] {
                        if item(ui, n, "", true) { actions.push(Action::Tool(n)) }
                    }
                });
                ui.menu_button("Settings", |ui| {
                    if item(ui, "Anisotropy Quantitative Factors", "", true) { actions.push(Action::Factors) }
                    if item(ui, "Orientation Parameters", "", true) { actions.push(Action::OrientParams) }
                    if item(ui, "Default Colors", "", true) { actions.push(Action::Colors) }
                });
                if ui.button("About").clicked() { actions.push(Action::About) }
            });
        });

        // ------------------------------------------------------------- toolbar
        egui::Panel::top("toolbar").show(ui, |ui| {
            let d = self.dark;
            let (prev, next) = self.doc.as_ref().filter(|dc| !dc.synthetic).map(|dc| { let n = doc::neighbours(&dc.path); (n.0.is_some(), n.1.is_some()) }).unwrap_or((false, false));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if tool(ui, Icon::Open, "Open Data File(s)  Ctrl+O", true, false, d) { actions.push(Action::Open) }
                if tool(ui, Icon::Prev, "Open Previous Data File  Ctrl+P", prev, false, d) { actions.push(Action::Prev) }
                if tool(ui, Icon::Next, "Open Next Data File  Ctrl+N", next, false, d) { actions.push(Action::Next) }
                if tool(ui, Icon::Reload, "Reload Current Data File(s)  Ctrl+R", has, false, d) { actions.push(Action::Reload) }
                if tool(ui, Icon::Save, "Save Active Records As  Ctrl+S", has, false, d) { actions.push(Action::SaveActive) }
                if tool(ui, Icon::Append, "Append Selected Records Into  Ctrl+A", has, false, d) { actions.push(Action::AppendSel) }
                if tool(ui, Icon::Export, "Export Data File (CSV)  Ctrl+E", has, false, d) { actions.push(Action::ExportCsv) }
                sep(ui);
                if tool(ui, Icon::EditData, "Edit Data  F1", has, false, d) { actions.push(Action::EditData) }
                if tool(ui, Icon::SetClasses, "Set Data Classes  Ctrl+F1", has, false, d) { actions.push(Action::SetClasses) }
                sep(ui);
                if tool(ui, Icon::Invert, "Invert Selection  Shift+Ins", has, false, d) { actions.push(Action::InvertSel) }
                if tool(ui, Icon::Disregard, "Disregard Selected Record(s)  Shift+Del", has, false, d) { actions.push(Action::Disregard) }
                if tool(ui, Icon::Restore, "Restore All Records  Alt+Bksp", has, false, d) { actions.push(Action::RestoreAll) }
                if tool(ui, Icon::ClearSel, "Clear Selection  Shift+Ctrl+F12", has, false, d) { actions.push(Action::ClearSel) }
                sep(ui);
                if tool(ui, Icon::CopyGraphics, "Copy Graphics To Clipboard  Ctrl+C", has, false, d) { actions.push(Action::CopyGraphics) }
                if tool(ui, Icon::SaveGraphics, "Save Graphics To File  Ctrl+F", has, false, d) { actions.push(Action::SaveGraphics) }
                sep(ui);
                if tool(ui, Icon::DataTable, "Data Table  F2", has, self.win_table, d) { actions.push(Action::DataTable) }
                sep(ui);
                if tool(ui, Icon::InPhase, "In-Phase AMS  F3", has, !self.out_phase, d) { actions.push(Action::Phase(false)) }
                if tool(ui, Icon::OutPhase, "Out-of-Phase AMS  F4", has, self.out_phase, d) { actions.push(Action::Phase(true)) }
                sep(ui);
                for (ch, cs, tip) in [('S', CoordSys::Specimen, "Specimen Coordinates  F5"), ('G', CoordSys::Geographic, "Geographic Coordinates  F6"), ('P', CoordSys::Paleo, "Paleogeographic Coordinates  F7"), ('T', CoordSys::Tectonic, "Tectonic Coordinates  F8")] {
                    if tool(ui, Icon::Coord(ch), tip, has, self.coord == cs, d) { actions.push(Action::Coord(cs)) }
                }
                sep(ui);
                tool(ui, Icon::PaleoH, "Paleo-Horizontal (not implemented yet)", false, false, d);
                tool(ui, Icon::PaleoV, "Paleo-Vertical (not implemented yet)", false, false, d);
                sep(ui);
                egui::ComboBox::from_id_salt("tb_filter").width(110.0).selected_text(["All active", "Selected only"][self.filter]).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter, 0, "All active");
                    ui.selectable_value(&mut self.filter, 1, "Selected only");
                }).response.on_hover_text("Which specimens are plotted");
                {
                    let mut p = self.color_master.param;
                    if param_combo(ui, "tb_metric", &mut p, d) {
                        self.color_master.param = p;
                        self.color_master.on = true;
                        self.apply_master_color();
                    }
                    ui.label(RichText::new("").small()).on_hover_text("Parameter used for colour-coding all plots");
                }
                if tool(ui, Icon::GroupStats, "Group Statistics  F9", has, self.win_group, d) { actions.push(Action::GroupStats) }
                if tool(ui, Icon::Contour, "Contour Plot  F12", has, self.win_contour, d) { actions.push(Action::Contour) }
            });
        });

        // ----------------------------------------------------------- status bar
        egui::Panel::bottom("status").show_separator_line(false).frame(egui::Frame::NONE).show(ui, |ui| {
            self.status_bar(ui);
        });

        // ---------------------------------------------------------- file browser
        if self.browser_open {
            let r = egui::Panel::left("browser").resizable(true).default_size(self.browser_w).min_size(150.0).max_size(520.0).show(ui, |ui| {
                self.browser_ui(ui, &mut actions);
            });
            let w = r.response.rect.width();
            if (w - self.browser_w).abs() > 0.5 {
                self.browser_w = w;
                self.layout_changed = Some(ui.input(|i| i.time));
            }
        } else {
            egui::Panel::left("browser_strip").resizable(false).exact_size(30.0).show(ui, |ui| {
                if small_icon(ui, Icon::Sidebar(false), "Show file browser  Ctrl+B", true, self.dark) {
                    actions.push(Action::ToggleBrowser);
                }
                ui.add_space(6.0);
                ui.label(RichText::new("F\ni\nl\ne\ns").weak());
            });
        }

        // ------------------------------------------------------------ specimens
        let r = egui::Panel::left("specimens").resizable(true).default_size(self.spec_w).min_size(110.0).max_size(420.0).show(ui, |ui| {
            self.specimens_ui(ui);
        });
        let w = r.response.rect.width();
        if (w - self.spec_w).abs() > 0.5 {
            self.spec_w = w;
            self.layout_changed = Some(ui.input(|i| i.time));
        }

        // ---------------------------------------------------------------- plots
        egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(6.0)).show(ui, |ui| {
            self.plots_ui(ui, &th);
        });

        // -------------------------------------------------------------- windows
        self.windows(&ctx);
        for a in actions {
            self.run(&ctx, a);
        }
        if self.dirty {
            ctx.request_repaint();
        }
    }
}

impl App {
    fn status_bar(&mut self, ui: &mut Ui) {
        let (nfile, nact, nsel) = self.doc.as_ref().map_or((0, 0, 0), |d| (d.specs.len(), d.n_active(), d.n_selected()));
        let ngeo = if self.coord != CoordSys::Specimen { nact } else { 0 };
        let (prev, cur, next) = match &self.doc {
            Some(d) if d.synthetic => ("NO PREVIOUS FILE".to_string(), d.path.display().to_string(), "NO NEXT FILE".to_string()),
            Some(d) => {
                let (p, n, _, _) = doc::neighbours(&d.path);
                let nm = |p: Option<PathBuf>, e: &str| p.and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string())).unwrap_or(e.to_string());
                (nm(p, "NO PREVIOUS FILE"), d.path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), nm(n, "NO NEXT FILE"))
            }
            None => ("NO PREVIOUS FILE".into(), "NO FILE OPEN".into(), "NO NEXT FILE".into()),
        };
        let dark = self.dark;
        let cells: Vec<(String, Color32, f32)> = vec![
            (format!("NFile = {nfile}"), Color32::from_rgb(255, 190, 245), 0.0),
            (format!("NAct = {nact}"), Color32::from_rgb(130, 235, 130), 0.0),
            (format!("NDis = {}", nfile - nact), Color32::from_rgb(255, 190, 190), 0.0),
            (format!("NGEO = {ngeo}"), Color32::from_rgb(110, 240, 245), 0.0),
            (format!("NSel = {nsel}"), Color32::from_rgb(255, 245, 90), 0.0),
            (prev, Color32::from_rgb(255, 130, 130), 1.0),
            (cur, Color32::from_rgb(140, 245, 140), 1.0),
            (next, Color32::from_rgb(60, 170, 235), 1.0),
        ];
        let h = 24.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
        let fixed = 5.0 * 92.0;
        let flex = ((rect.width() - fixed) / 3.0).max(60.0);
        let mut x = rect.min.x;
        for (text, col, f) in cells {
            let w = if f == 0.0 { 92.0 } else { flex };
            let r = Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(w, h));
            let col = if dark { col.gamma_multiply(0.62) } else { col };
            ui.painter().rect_filled(r.shrink(1.0), 4.0, col);
            let tc = if dark { Color32::WHITE } else { Color32::BLACK };
            ui.painter().with_clip_rect(r).text(Pos2::new(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(12.5), tc);
            x += w;
        }
    }

    fn browser_ui(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let mut back = false;
        let mut go: Option<PathBuf> = None;
        let mut open: Option<PathBuf> = None;
        let dark = self.dark;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if small_icon(ui, Icon::Back, "Back to the previous folder", !self.browser_back.is_empty(), dark) {
                back = true;
            }
            if small_icon(ui, Icon::Up, "Parent folder", self.browser_dir.parent().is_some(), dark) {
                go = self.browser_dir.parent().map(|p| p.to_path_buf());
            }
            if small_icon(ui, Icon::Home, "Home folder", true, dark) {
                go = Some(home_dir());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if small_icon(ui, Icon::Sidebar(true), "Hide file browser  Ctrl+B", true, dark) {
                    actions.push(Action::ToggleBrowser);
                }
            });
        });
        ui.strong("Files");
        ui.label(RichText::new(self.browser_dir.display().to_string()).small().weak());
        ui.separator();
        let cur = self.doc.as_ref().map(|d| d.path.clone());
        let mut dirs: Vec<PathBuf> = vec![];
        if let Ok(rd) = std::fs::read_dir(&self.browser_dir) {
            for e in rd.filter_map(|e| e.ok()) {
                let p = e.path();
                let hidden = p.file_name().map_or(false, |n| n.to_string_lossy().starts_with('.'));
                if p.is_dir() && !hidden {
                    dirs.push(p);
                }
            }
        }
        dirs.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
        let files = doc::ams_in(&self.browser_dir);
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            for d in &dirs {
                let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if ui.selectable_label(false, format!("📁 {name}")).clicked() {
                    go = Some(d.clone());
                }
            }
            if !dirs.is_empty() && !files.is_empty() {
                ui.separator();
            }
            for f in &files {
                let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if ui.selectable_label(cur.as_ref() == Some(f), format!("🧲 {name}")).clicked() {
                    open = Some(f.clone());
                }
            }
            if dirs.is_empty() && files.is_empty() {
                ui.label(RichText::new("No folders or .ams files here").weak());
            }
        });
        if back {
            if let Some(p) = self.browser_back.pop() {
                self.browser_dir = p;
                self.save_state();
            }
        } else if let Some(p) = go {
            if p != self.browser_dir {
                self.browser_back.push(std::mem::replace(&mut self.browser_dir, p));
                self.browser_back.truncate(64);
            }
            self.save_state();
        }
        if let Some(p) = open {
            self.open_path(p);
        }
    }

    fn specimens_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let arrow = if self.spec_color_open { "▾" } else { "▸" };
            if ui.small_button(arrow).on_hover_text("Colour-coding options").clicked() {
                self.spec_color_open = !self.spec_color_open;
            }
            ui.strong("Specimens");
        });
        if self.spec_color_open {
            let before = self.color_master.clone();
            let dark = self.dark;
            color_ui(ui, "metric_panel", &mut self.color_master, dark);
            if before != self.color_master {
                self.apply_master_color();
            }
        }
        ui.separator();
        let Some(doc) = &mut self.doc else {
            ui.label(RichText::new("No file open.\nOpen one with Ctrl+O or pick it in the file browser.").weak());
            return;
        };
        let order = doc.order();
        let mods = ui.input(|i| i.modifiers);
        let down = ui.input(|i| i.pointer.primary_down());
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if !down {
            self.list_drag = None;
        }
        let dark = self.dark;
        let mut clicked: Option<usize> = None;
        let mut started: Option<usize> = None;
        let mut hot: Option<usize> = None;
        let mut sa = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(pos) = self.scroll_to.take() {
            // bring the row into view (only scroll when it is outside the visible part)
            let pitch = ROW_H + ui.spacing().item_spacing.y;
            let top = pos as f32 * pitch;
            let (off, h) = (self.list_off, self.list_h.max(pitch));
            if top < off {
                sa = sa.vertical_scroll_offset(top);
            } else if top + ROW_H > off + h {
                sa = sa.vertical_scroll_offset(top + ROW_H - h);
            }
        }
        let list_out = sa.show_rows(ui, ROW_H, order.len(), |ui, range| {
            for pos in range {
                let i = order[pos];
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click_and_drag());
                paint_row(ui, rect, doc.selected[i], resp.hovered(), doc.active[i], dark);
                row_text(ui, rect, 8.0, &doc.specs[i].name, doc.active[i], dark);
                if resp.drag_started() {
                    started = Some(pos);
                } else if resp.clicked() {
                    clicked = Some(i);
                }
                if pointer.map_or(false, |p| rect.contains(p)) {
                    hot = Some(pos);
                }
            }
        });
        self.list_off = list_out.state.clamped_offset().y;
        self.list_h = list_out.inner_rect.height();
        if let Some(pos) = started {
            let additive = mods.ctrl || mods.command;
            let base = if additive { doc.selected.clone() } else { vec![false; doc.selected.len()] };
            self.list_drag = Some(ListDrag { anchor: pos, additive, base });
        }
        if let (Some(d), Some(cur)) = (&self.list_drag, hot) {
            let (lo, hi) = (d.anchor.min(cur), d.anchor.max(cur));
            doc.selected = d.base.clone();
            for pos in lo..=hi {
                doc.selected[order[pos]] = true;
            }
            let _ = d.additive;
        } else if let Some(i) = clicked {
            select(doc, i, mods, &mut self.last_anchor);
        }
    }

    fn plots_ui(&mut self, ui: &mut Ui, th: &Theme) {
        let full = ui.available_rect_before_wrap();
        let gap = 8.0;
        let sx = full.min.x + full.width() * self.hfrac;
        let sy = full.min.y + full.height() * self.vfrac;
        let tl = Rect::from_min_max(full.min, Pos2::new(sx - gap / 2.0, sy - gap / 2.0));
        let tr = Rect::from_min_max(Pos2::new(sx + gap / 2.0, full.min.y), Pos2::new(full.max.x, sy - gap / 2.0));
        let bl = Rect::from_min_max(Pos2::new(full.min.x, sy + gap / 2.0), Pos2::new(sx - gap / 2.0, full.max.y));
        let br = Rect::from_min_max(Pos2::new(sx + gap / 2.0, sy + gap / 2.0), full.max);
        let before = (self.hfrac, self.vfrac);

        let v = Rect::from_min_max(Pos2::new(sx - gap / 2.0, full.min.y), Pos2::new(sx + gap / 2.0, full.max.y));
        let hbar = Rect::from_min_max(Pos2::new(full.min.x, sy - gap / 2.0), Pos2::new(full.max.x, sy + gap / 2.0));
        let rv = ui.interact(v, ui.scope_id().with("split_v"), Sense::drag());
        let rh = ui.interact(hbar, ui.scope_id().with("split_h"), Sense::drag());
        if rv.hovered() || rv.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if rh.hovered() || rh.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if rv.dragged() {
            if let Some(p) = rv.interact_pointer_pos() {
                self.hfrac = ((p.x - full.min.x) / full.width()).clamp(0.2, 0.8);
            }
        }
        if rh.dragged() {
            if let Some(p) = rh.interact_pointer_pos() {
                self.vfrac = ((p.y - full.min.y) / full.height()).clamp(0.2, 0.8);
            }
        }
        if before != (self.hfrac, self.vfrac) {
            // saved shortly after the drag ends (see track_layout)
            self.layout_changed = Some(ui.input(|i| i.time));
        }
        for (r, resp) in [(v, &rv), (hbar, &rh)] {
            if resp.hovered() || resp.dragged() {
                ui.painter().rect_filled(r.shrink2(if r == v { Vec2::new(2.0, 0.0) } else { Vec2::new(0.0, 2.0) }), 2.0, th.accent.gamma_multiply(0.5));
            }
        }

        let pts = self.points();
        let mut click: Option<usize> = None;
        let mut band: Option<Vec<usize>> = None;
        let cs_label = match self.coord {
            CoordSys::Specimen => "SPEC",
            CoordSys::Geographic => "GEO",
            CoordSys::Paleo => "PALEO",
            CoordSys::Tectonic => "TECTO",
        };
        let rects = [tl, tr, bl, br];
        let names = ["Stereonet", "N vs Kmean", "P vs Kmean", "T vs P"];
        let mut overlay_gs: Option<ams_core::stats::GroupStats> = None;
        if self.st_overlay.mean || self.st_overlay.boot {
            let idx: Vec<usize> = pts.iter().map(|p| p.idx).collect();
            let tensors: Vec<_> = idx.iter().map(|&i| self.derived[i].tensor).collect();
            let kms: Vec<f64> = idx.iter().map(|&i| self.derived[i].km).collect();
            overlay_gs = ams_core::stats::group_stats(&tensors, &kms);
        }
        for i in 0..4 {
            let mut body = self.pane_frame(ui, rects[i], i, names[i], th);
            let (cpts, crange) = if i == 1 { (pts.clone(), None) } else { Self::colorized(&pts, &self.color[i]) };
            let show_bar = i != 1 && crange.is_some() && (i == 0 || self.sc[i - 2].legend);
            if show_bar {
                let bar = Rect::from_min_max(Pos2::new(body.min.x, body.max.y - 30.0), body.max);
                body.max.y -= 30.0;
                plots::color_bar(ui, bar, &self.color[i], crange.unwrap(), th);
            }
            let out = match i {
                0 => {
                    let o = StereoOpts { show: self.show_k, size: self.marker, data: self.st_data, caption: self.st_caption, legend: self.st_legend, connect: self.st_connect, foliation: self.st_mf };
                    let out = plots::stereonet(ui, body, &cpts, &o, th, cs_label);
                    if let Some(gs) = &overlay_gs {
                        let (c, r) = plots::net_geometry(body);
                        let pa = ui.painter_at(body);
                        analysis_ui::draw_stereo_overlay(&pa, c, r, gs, &self.st_overlay, self.show_k, self.k_colors);
                    }
                    out
                }
                1 => plots::histogram(ui, body, &pts, self.bins, &self.hist_cfg, th),
                _ => {
                    let c = &self.sc[i - 2];
                    let spec = ScatterSpec { x: &c.x, y: &c.y, zero_line: i == 3, diagonal: c.diag, conf: c.conf, connect: c.connect, salt: if i == 2 { "pkm" } else { "tp" } };
                    plots::scatter(ui, body, &cpts, &spec, self.marker, th)
                }
            };
            self.hover[i] = out.hover.unwrap_or_default();
            if out.click.is_some() {
                click = out.click;
            }
            if out.band.is_some() {
                band = out.band;
            }
        }
        if let Some(b) = band {
            if let Some(doc) = &mut self.doc {
                doc.selected.iter_mut().for_each(|s| *s = false);
                for i in b {
                    doc.selected[i] = true;
                }
            }
        } else if let (Some(i), Some(doc)) = (click, &mut self.doc) {
            // a click on a marker selects just that specimen (a second click on it clears)
            let only = doc.selected[i] && doc.n_selected() == 1;
            doc.selected.iter_mut().for_each(|s| *s = false);
            doc.selected[i] = !only;
            self.last_anchor = Some(i);
        }
        if let Some(job) = &mut self.shot {
            if job.rects.is_empty() {
                job.rects = names.iter().zip(rects).enumerate().filter(|(i, _)| self.export[*i]).map(|(_, (n, r))| (*n, r)).map(|(n, r)| (match n { "Stereonet" => "stereonet", "N vs Kmean" => "n_vs_km", "P vs Kmean" => "p_vs_km", _ => "t_vs_p" }, r)).collect();
            }
        }
    }

    /// Draws a pane card with its header (collapse arrow, hover text, "Export graphics") and returns the plot rect.
    fn pane_frame(&mut self, ui: &mut Ui, rect: Rect, i: usize, name: &str, th: &Theme) -> Rect {
        let visuals = ui.visuals().clone();
        ui.painter().rect(rect, 8.0, visuals.extreme_bg_color.gamma_multiply(if th.dark { 0.6 } else { 1.0 }).linear_multiply(1.0), Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
        let header = Rect::from_min_size(rect.min + Vec2::new(6.0, 4.0), Vec2::new(rect.width() - 12.0, 24.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(header).layout(Layout::left_to_right(Align::Center)));
        let arrow = if self.pane_settings[i] { "▾" } else { "▸" };
        if child.small_button(arrow).on_hover_text("Plot options").clicked() {
            self.pane_settings[i] = !self.pane_settings[i];
        }
        let txt = if self.hover[i].is_empty() { RichText::new(name).weak() } else { RichText::new(&self.hover[i]) };
        child.label(txt);
        let mut changed = false;
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.checkbox(&mut self.export[i], "Export graphics");
            if i == 2 {
                let lab = if self.sc[0].x.param == 0 { "Log Km" } else { "Log X" };
                changed |= ui.checkbox(&mut self.sc[0].x.log, lab).on_hover_text("Logarithmic X axis").changed();
            }
        });
        if changed {
            self.save_state();
        }
        let top = header.max.y + 2.0;
        if self.pane_settings[i] {
            self.pane_options(ui.ctx(), rect, header, i);
        }
        Rect::from_min_max(Pos2::new(rect.min.x + 4.0, top), Pos2::new(rect.max.x - 4.0, rect.max.y - 4.0))
    }

    /// The options panel of a plot (opened with the arrow in its header), with the choices the original offers.
    fn pane_options(&mut self, ctx: &egui::Context, rect: Rect, header: Rect, i: usize) {
        let dark = self.dark;
        let marker_before = self.marker;
        let max_h = (rect.height() - 40.0).max(120.0);
        let mut close = false;
        egui::Area::new(egui::Id::unique(("pane-options", i))).order(egui::Order::Foreground).fixed_pos(Pos2::new(rect.min.x + 8.0, header.max.y + 2.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(300.0);
                egui::ScrollArea::vertical().max_height(max_h).auto_shrink([true, true]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Symbol size");
                        ui.add(egui::DragValue::new(&mut self.marker).range(2.0..=15.0).speed(0.2).max_decimals(0));
                        if ui.small_button("Close").clicked() {
                            close = true;
                        }
                    });
                    ui.separator();
                    match i {
                        0 => {
                            ui.horizontal(|ui| {
                                for (a, n) in ["K1", "K2", "K3"].iter().enumerate() {
                                    let (rr, _) = ui.allocate_exact_size(Vec2::new(54.0, 22.0), Sense::hover());
                                    ui.painter().rect_filled(rr, 3.0, class_color(a as u8 + 1, dark).unwrap_or(Color32::GRAY));
                                    ui.scope_builder(UiBuilder::new().max_rect(rr.shrink2(Vec2::new(3.0, 0.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                                        ui.checkbox(&mut self.show_k[a], RichText::new(*n).color(Color32::BLACK));
                                    });
                                }
                                ui.checkbox(&mut self.st_mf, "MF").on_hover_text("Magnetic foliation: a great circle perpendicular to each K3");
                            });
                            ui.checkbox(&mut self.st_data, RichText::new("Data points").strong());
                            ui.checkbox(&mut self.st_connect, "Connect symbols").on_hover_text("Join the points of each axis in file order");
                            ui.separator();
                            let before = self.color[0].clone();
                            color_ui(ui, "st-color", &mut self.color[0], dark);
                            if before != self.color[0] {
                                self.color_master = self.color[0].clone();
                                self.apply_master_color();
                            }
                            ui.separator();
                            ui.checkbox(&mut self.st_overlay.mean, RichText::new("Mean tensor").strong());
                            ui.add_enabled_ui(self.st_overlay.mean, |ui| {
                                ui.indent("mt", |ui| {
                                    ui.checkbox(&mut self.st_overlay.conf, "Conf. ellipses");
                                    for (a, n) in ["K2-K3 plane", "K1-K3 plane", "K1-K2 plane"].iter().enumerate() {
                                        ui.checkbox(&mut self.st_overlay.planes[a], RichText::new(*n).background_color(class_color(a as u8 + 1, dark).unwrap_or(Color32::GRAY)).color(Color32::BLACK));
                                    }
                                });
                            });
                            ui.checkbox(&mut self.st_overlay.boot, RichText::new("Bootstrap").strong());
                            ui.add_enabled_ui(self.st_overlay.boot, |ui| {
                                ui.indent("bt", |ui| {
                                    ui.checkbox(&mut self.st_overlay.boot_conf, "Conf. ellipses");
                                });
                            });
                            ui.add_enabled(false, egui::Checkbox::new(&mut false, "Foliation / Lineation (needs .ged data)"));
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.st_caption, "Caption");
                                ui.checkbox(&mut self.st_legend, "Legend");
                            });
                        }
                        1 => {
                            ui.label(RichText::new("Data").strong());
                            param_combo(ui, "hist-param", &mut self.hist_cfg.param, dark);
                            let h = &mut self.hist_cfg;
                            range_fields(ui, &mut h.auto, &mut h.min, &mut h.max, &mut h.log, "hist");
                            ui.add(egui::Slider::new(&mut self.bins, 3..=60).text("bins"));
                        }
                        _ => {
                            let k = i - 2;
                            let id = if i == 2 { "pk" } else { "tp" };
                            axis_ui(ui, "X-axis", &format!("{id}-x"), &mut self.sc[k].x, dark);
                            ui.separator();
                            axis_ui(ui, "Y-axis", &format!("{id}-y"), &mut self.sc[k].y, dark);
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.sc[k].diag, "Diagonal line");
                                ui.checkbox(&mut self.sc[k].conf, "Conf. intervals").on_hover_text("Error bars (±SD) for K1, K2, K3 on the Y axis");
                            });
                            ui.separator();
                            let before = self.color[i].clone();
                            color_ui(ui, &format!("{id}-color"), &mut self.color[i], dark);
                            if before != self.color[i] {
                                self.color_master = self.color[i].clone();
                                self.apply_master_color();
                            }
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.sc[k].connect, "Connect symbols").on_hover_text("Join the points in file order");
                                ui.checkbox(&mut self.sc[k].legend, "Legend").on_hover_text("Show the colour scale below the plot");
                            });
                        }
                    }
                });
            });
        });
        if self.marker != marker_before {
            self.layout_changed = Some(ctx.input(|t| t.time));
        }
        if close {
            self.pane_settings[i] = false;
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(img) = self.shot_box.lock().unwrap().take() else { return };
        let ppp = ctx.pixels_per_point();
        if let Some(r) = self.tool_copy.take() {
            let x0 = ((r.min.x * ppp) as usize).min(img.width());
            let y0 = ((r.min.y * ppp) as usize).min(img.height());
            let x1 = ((r.max.x * ppp) as usize).clamp(x0, img.width());
            let y1 = ((r.max.y * ppp) as usize).clamp(y0, img.height());
            let mut px = Vec::with_capacity((x1 - x0) * (y1 - y0));
            for y in y0..y1 {
                px.extend_from_slice(&img.pixels[y * img.width() + x0..y * img.width() + x1]);
            }
            ctx.copy_image(egui::ColorImage::new([x1 - x0, y1 - y0], px));
            return;
        }
        let Some(job) = self.shot.take() else { return };
        let crop = |r: Rect| -> egui::ColorImage {
            let x0 = ((r.min.x * ppp) as usize).min(img.width());
            let y0 = ((r.min.y * ppp) as usize).min(img.height());
            let x1 = ((r.max.x * ppp) as usize).clamp(x0, img.width());
            let y1 = ((r.max.y * ppp) as usize).clamp(y0, img.height());
            let (w, h) = (x1 - x0, y1 - y0);
            let mut px = Vec::with_capacity(w * h);
            for y in y0..y1 {
                px.extend_from_slice(&img.pixels[y * img.width() + x0..y * img.width() + x1]);
            }
            egui::ColorImage::new([w, h], px)
        };
        if job.rects.is_empty() {
            self.info = Some("No plot is ticked in 'Export graphics'.".into());
            return;
        }
        if job.copy {
            let union = job.rects.iter().skip(1).fold(job.rects[0].1, |a, (_, r)| a.union(*r));
            ctx.copy_image(crop(union));
        } else if let Some(base) = job.base {
            let stem = base.with_extension("");
            for (name, r) in &job.rects {
                let c = crop(*r);
                let path = PathBuf::from(format!("{}_{name}.png", stem.display()));
                let raw: Vec<u8> = c.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Err(e) = image::save_buffer(&path, &raw, c.width() as u32, c.height() as u32, image::ColorType::Rgba8) {
                    self.info = Some(format!("Could not save {}: {e}", path.display()));
                    return;
                }
            }
        }
    }
}

const ROW_H: f32 = 22.0;

fn faded(c: Color32, active: bool) -> Color32 {
    if active { c } else { c.gamma_multiply(0.28) }
}

/// Full-width row background: selection, hover. Disregarded rows get a much fainter tint.
fn paint_row(ui: &Ui, rect: Rect, selected: bool, hovered: bool, active: bool, _dark: bool) {
    let v = ui.visuals();
    if selected {
        let c = if active { v.selection.bg_fill } else { v.selection.bg_fill.gamma_multiply(0.25) };
        ui.painter().rect_filled(rect.shrink2(Vec2::new(1.0, 0.0)), 3.0, c);
    } else if hovered {
        ui.painter().rect_filled(rect.shrink2(Vec2::new(1.0, 0.0)), 3.0, v.widgets.hovered.bg_fill.gamma_multiply(0.5));
    }
}

/// Row text; a disregarded row is struck through and very faint.
fn row_text(ui: &Ui, rect: Rect, dx: f32, text: &str, active: bool, _dark: bool) -> f32 {
    let col = faded(ui.visuals().text_color(), active);
    let galley_rect = ui.painter().text(Pos2::new(rect.min.x + dx, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(13.5), col);
    if !active {
        let y = galley_rect.center().y;
        ui.painter().line_segment([Pos2::new(galley_rect.min.x, y), Pos2::new(galley_rect.max.x, y)], Stroke::new(1.0, col));
    }
    galley_rect.width()
}

fn select(doc: &mut Doc, i: usize, mods: Modifiers, anchor: &mut Option<usize>) {
    if mods.shift {
        if let Some(a) = *anchor {
            let (lo, hi) = (a.min(i), a.max(i));
            for k in lo..=hi {
                doc.selected[k] = true;
            }
            return;
        }
    }
    if mods.command || mods.ctrl {
        doc.selected[i] = !doc.selected[i];
    } else {
        let was = doc.selected[i] && doc.n_selected() == 1;
        doc.selected.iter_mut().for_each(|s| *s = false);
        doc.selected[i] = !was;
    }
    *anchor = Some(i);
}

// -------------------------------------------------------------- windows

impl App {

    /// Replace the open document (used by tools that build data in memory).
    fn set_doc(&mut self, d: Doc) {
        self.doc = Some(d);
        self.dirty = true;
        self.refresh();
        self.last_anchor = None;
    }

    fn request_shot(&self, ctx: &egui::Context) {
        let slot = self.shot_box.clone();
        let c2 = ctx.clone();
        ctx.request_screenshot(move |img| {
            *slot.lock().unwrap() = Some(img);
            c2.request_repaint();
        });
    }

    /// Remember the window size and write the layout to disk shortly after it stopped changing.
    fn track_layout(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let (rect, maxd) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().maximized.unwrap_or(false)));
        if maxd != self.win_max {
            self.win_max = maxd;
            self.layout_changed = Some(now);
        }
        if let (Some(r), false) = (rect, maxd) {
            let sz = [r.width().round(), r.height().round()];
            if self.win_size.map_or(true, |o| (o[0] - sz[0]).abs() > 1.0 || (o[1] - sz[1]).abs() > 1.0) {
                self.win_size = Some(sz);
                self.layout_changed = Some(now);
            }
        }
        if let Some(t) = self.layout_changed {
            if now - t > 0.5 && !ctx.input(|i| i.pointer.any_down()) {
                self.save_state();
                self.layout_changed = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
    }

    fn open_win(&mut self, k: usize) {
        self.win_serial[k] += 1;
        match k {
            W_TABLE => self.win_table = true,
            W_GROUP => self.win_group = true,
            W_CONTOUR => self.win_contour = true,
            W_ORIENT => self.win_orient = true,
            W_COLORS => self.win_colors = true,
            W_FACTORS => self.win_factors = true,
            W_ABOUT => self.win_about = true,
            W_EDIT => self.win_edit = true,
            _ => {}
        }
    }

    /// Every window of a kind opens with the same id, size and (centred) position, each time it is opened.
    fn base_window(&self, ctx: &egui::Context, title: &'static str, slot: usize, size: Vec2) -> egui::Window<'static> {
        let c = ctx.content_rect().center();
        let pos = Pos2::new((c.x - size.x / 2.0).max(8.0), (c.y - size.y / 2.0).max(40.0));
        egui::Window::new(title).id(egui::Id::unique(("ams-window", title, self.win_serial[slot]))).default_pos(pos).default_size(size)
    }

    fn windows(&mut self, ctx: &egui::Context) {
        if let Some(msg) = self.info.clone() {
            let mut open = true;
            egui::Window::new("AMS Studio").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                tools_ui::pop_style(ui, 1.4);
                ui.set_max_width(640.0);
                ui.label(&msg);
                if ui.button("OK").clicked() {
                    self.info = None;
                }
            });
            if !open {
                self.info = None;
            }
        }

        let mut open = self.win_about;
        self.base_window(ctx, "About AMS Studio", W_ABOUT, Vec2::new(760.0, 220.0)).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.4);
            ui.heading("AMS Studio");
            ui.label("An independent reimplementation of AGICO's Anisoft 6, written in Rust with egui.");
            ui.label("Reads Safyr7 .ams files; anisotropy results were checked against csv exports of the original program.");
            ui.label(RichText::new("Anisoft is a program of AGICO, Brno. This project is not affiliated with AGICO.").weak());
        });
        self.win_about = open;

        let mut open = self.win_factors;
        self.base_window(ctx, "Anisotropy Quantitative Factors", W_FACTORS, Vec2::new(900.0, 480.0)).open(&mut open).resizable(false).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.4);
            egui::Grid::new("fx").striped(true).show(ui, |ui| {
                for (n, f) in [
                    ("L", "K1 / K2"), ("F", "K2 / K3"), ("P", "K1 / K3"),
                    ("Pj", "exp sqrt(2 [(n1-nm)^2+(n2-nm)^2+(n3-nm)^2]),  ni = ln Ki"),
                    ("T", "(2 n2 - n1 - n3) / (n1 - n3)"), ("U", "(2 K2 - K1 - K3) / (K1 - K3)"),
                    ("Q", "(K1 - K2) / ((K1 + K2)/2 - K3)"), ("E", "K2^2 / (K1 K3)"),
                ] {
                    ui.strong(n);
                    ui.label(f);
                    ui.end_row();
                }
            });
            ui.label(RichText::new("Ki are the eigenvalues normalised to a mean of 1. Undefined (shown blank) when K3 < 0.").weak());
        });
        self.win_factors = open;

        let mut open = self.win_orient;
        let mut changed = false;
        let mut orient = self.orient;
        self.base_window(ctx, "Orientation Parameters", W_ORIENT, Vec2::new(640.0, 440.0)).open(&mut open).resizable(false).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.4);
            ui.label(RichText::new("Not stored in .ams files. Applied to all specimens of the open file.").weak());
            egui::Grid::new("or").show(ui, |ui| {
                for (n, v, r) in [
                    ("Core azimuth", &mut orient.azimuth, 0.0..=360.0),
                    ("Core dip", &mut orient.dip, -90.0..=90.0),
                    ("Bedding strike", &mut orient.bed_strike, 0.0..=360.0),
                    ("Bedding dip", &mut orient.bed_dip, 0.0..=90.0),
                    ("Paleo declination", &mut orient.paleo_dec, -360.0..=360.0),
                ] {
                    ui.label(n);
                    changed |= ui.add(egui::DragValue::new(v).range(r).suffix(" \u{b0}").speed(0.5)).changed();
                    ui.end_row();
                }
            });
            if ui.button("Reset").clicked() {
                orient = Orientation::default();
                changed = true;
            }
            ui.label(RichText::new("Approximation: the original reads per-specimen values from the file.").small().weak());
        });
        self.orient = orient;
        self.win_orient = open;
        if changed {
            self.dirty = true;
        }

        let mut open = self.win_colors;
        let mut kc = self.k_colors;
        self.base_window(ctx, "Default Colors", W_COLORS, Vec2::new(360.0, 260.0)).open(&mut open).resizable(false).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.4);
            for (i, n) in ["K1", "K2", "K3"].iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgba(&mut kc[i]);
                    ui.label(*n);
                });
            }
        });
        self.k_colors = kc;
        self.win_colors = open;

        let mut open = self.win_edit;
        let mut name = self.edit_name.clone();
        let mut apply = false;
        self.base_window(ctx, "Edit Data", W_EDIT, Vec2::new(600.0, 260.0)).open(&mut open).resizable(false).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.4);
            ui.label("Specimen name (up to 24 characters):");
            ui.text_edit_singleline(&mut name);
            apply = ui.button("Apply").clicked();
        });
        self.edit_name = name;
        if apply {
            if let Some(d) = &mut self.doc {
                if let Some(i) = (0..d.specs.len()).find(|&i| d.selected[i]) {
                    d.rename(i, &self.edit_name);
                }
            }
            open = false;
        }
        self.win_edit = open;

        self.table_window(ctx);
        self.group_window(ctx);
        self.contour_window(ctx);
        self.tool_windows(ctx);
    }

    fn table_window(&mut self, ctx: &egui::Context) {
        if !self.win_table {
            return;
        }
        let mut open = true;
        let mut clicked = None;
        let mut started: Option<usize> = None;
        let mut hot: Option<usize> = None;
        let mods = ctx.input(|i| i.modifiers);
        let pointer = ctx.input(|i| i.pointer.interact_pos());
        let dark = self.dark;
        const COLS: [(&str, f32); 17] = [
            ("Specimen", 110.0), ("Km", 80.0), ("K1", 66.0), ("K2", 66.0), ("K3", 66.0),
            ("K1 Dec", 56.0), ("K1 Inc", 52.0), ("K2 Dec", 56.0), ("K2 Inc", 52.0), ("K3 Dec", 56.0), ("K3 Inc", 52.0),
            ("P", 60.0), ("Pj", 60.0), ("T", 60.0), ("F", 64.0), ("F12", 64.0), ("F23", 64.0),
        ];
        let total_w: f32 = COLS.iter().map(|c| c.1).sum();
        let win = self.base_window(ctx, "Data Table", W_TABLE, Vec2::new(1290.0, 700.0));
        let (derived, out_phase) = (self.derived.clone(), self.out_phase);
        let mut drag = self.list_drag.take();
        let mut anchor = self.last_anchor;
        if let Some(doc) = &mut self.doc {
            if derived.len() == doc.specs.len() {
                win.open(&mut open).show(ctx, |ui| {
                    let order = doc.order();
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        let (hr, _) = ui.allocate_exact_size(Vec2::new(total_w, 24.0), Sense::hover());
                        let mut x = hr.min.x;
                        ui.painter().rect_filled(hr, 0.0, ui.visuals().faint_bg_color);
                        for (n, w) in COLS {
                            ui.painter().text(Pos2::new(x + 6.0, hr.center().y), Align2::LEFT_CENTER, n, FontId::proportional(12.5), ui.visuals().strong_text_color());
                            x += w;
                        }
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, order.len(), |ui, range| {
                            for pos in range {
                                let i = order[pos];
                                let (rect, resp) = ui.allocate_exact_size(Vec2::new(total_w, ROW_H), Sense::click_and_drag());
                                paint_row(ui, rect, doc.selected[i], resp.hovered(), doc.active[i], dark);
                                let d = &derived[i];
                                let ph = if out_phase { &doc.specs[i].outphase } else { &doc.specs[i].inphase };
                                let sh = d.axes.shape;
                                let f = |v: Option<f64>, p: usize| v.map(|x| format!("{x:.p$}")).unwrap_or_default();
                                let cells = [
                                    doc.specs[i].name.clone(), format!("{:.4E}", d.km),
                                    format!("{:.5}", d.axes.k_norm[0]), format!("{:.5}", d.axes.k_norm[1]), format!("{:.5}", d.axes.k_norm[2]),
                                    format!("{:.1}", d.axes.dec_inc[0].0), format!("{:.1}", d.axes.dec_inc[0].1),
                                    format!("{:.1}", d.axes.dec_inc[1].0), format!("{:.1}", d.axes.dec_inc[1].1),
                                    format!("{:.1}", d.axes.dec_inc[2].0), format!("{:.1}", d.axes.dec_inc[2].1),
                                    f(sh.map(|s| s.p), 4), f(sh.map(|s| s.pj), 4), f(sh.map(|s| s.t), 4),
                                    format!("{:.1}", ph.ftest[0]), format!("{:.1}", ph.ftest[1]), format!("{:.1}", ph.ftest[2]),
                                ];
                                let mut x = rect.min.x;
                                for (c, (_, w)) in cells.iter().zip(COLS) {
                                    row_text(ui, Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(w, ROW_H)), 6.0, c, doc.active[i], dark);
                                    x += w;
                                }
                                if resp.drag_started() {
                                    started = Some(pos);
                                } else if resp.clicked() {
                                    clicked = Some(i);
                                }
                                if pointer.map_or(false, |p| rect.contains(p)) {
                                    hot = Some(pos);
                                }
                            }
                        });
                    });
                    if !ui.input(|i| i.pointer.primary_down()) {
                        drag = None;
                    }
                    if let Some(pos) = started {
                        let additive = mods.ctrl || mods.command;
                        let base = if additive { doc.selected.clone() } else { vec![false; doc.selected.len()] };
                        drag = Some(ListDrag { anchor: pos, additive, base });
                    }
                    if let (Some(d), Some(cur)) = (&drag, hot) {
                        let (lo, hi) = (d.anchor.min(cur), d.anchor.max(cur));
                        doc.selected = d.base.clone();
                        for pos in lo..=hi {
                            doc.selected[order[pos]] = true;
                        }
                    } else if let Some(i) = clicked {
                        select(doc, i, mods, &mut anchor);
                    }
                });
            }
        }
        self.list_drag = drag;
        self.last_anchor = anchor;
        self.win_table = open;
    }
}
