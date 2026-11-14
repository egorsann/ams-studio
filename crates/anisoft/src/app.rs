use crate::doc::{self, Derived, Doc};
use crate::icons::{self, Icon};
use crate::plots::{self, Pt, ScatterSpec, StereoOpts, Theme};
use anisoft_core::coords::{CoordSys, Orientation};
use anisoft_core::density;
use anisoft_core::export;
use anisoft_core::stats;
use anisoft_core::tensor::{from_dec_inc, Vec3};
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
}

const METRICS: [&str; 4] = ["Km", "P", "Pj", "T"];

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
    spec_color_open: bool,

    show_k: [bool; 3],
    marker: f32,
    bins: usize,
    export: [bool; 4],
    pane_settings: [bool; 4],
    color_on: bool,
    palette: u8,
    metric: usize,
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
    contour_axis: usize,
    contour_k: f64,
    edit_name: String,
    info: Option<String>,
    hover: [String; 4],
    last_anchor: Option<usize>,
    shot: Option<ShotJob>,
    shot_box: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<egui::ColorImage>>>>,
    last_title: String,
}

fn state_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default()
        .join("anisoft-linux")
        .join("state.txt")
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
            browser_dir: std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default(),
            spec_color_open: false,
            show_k: [true; 3],
            marker: 4.0,
            bins: 12,
            export: [true; 4],
            pane_settings: [false; 4],
            color_on: false,
            palette: 0,
            metric: 0,
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
            contour_axis: 0,
            contour_k: 60.0,
            edit_name: String::new(),
            info: None,
            hover: Default::default(),
            last_anchor: None,
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
        let Ok(s) = std::fs::read_to_string(state_path()) else { return };
        for l in s.lines() {
            let Some((k, v)) = l.split_once('=') else { continue };
            match k {
                "last_dir" if Path::new(v).is_dir() => self.browser_dir = PathBuf::from(v),
                "dark" => self.dark = v == "1",
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
                "last_dir={}\ndark={}\nbrowser={}\nhfrac={}\nvfrac={}\n",
                self.browser_dir.display(),
                self.dark as u8,
                self.browser_open as u8,
                self.hfrac,
                self.vfrac
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

    fn metric_value(&self, i: usize) -> Option<f64> {
        let d = &self.derived[i];
        match self.metric {
            0 => Some(d.km),
            1 => d.axes.shape.map(|s| s.p),
            2 => d.axes.shape.map(|s| s.pj),
            _ => d.axes.shape.map(|s| s.t),
        }
    }

    fn points(&self) -> Vec<Pt> {
        let Some(doc) = &self.doc else { return vec![] };
        let idx: Vec<usize> = (0..doc.specs.len())
            .filter(|&i| doc.active[i] && (self.filter == 0 || doc.selected[i]))
            .collect();
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &i in &idx {
            if let Some(v) = self.metric_value(i) {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        idx.iter()
            .map(|&i| {
                let d = &self.derived[i];
                let color = if self.color_on {
                    let t = self.metric_value(i).map_or(0.0, |v| if hi > lo { (v - lo) / (hi - lo) } else { 0.5 });
                    Some(match self.palette {
                        0 => plots::rainbow(t),
                        1 => plots::shades(t),
                        _ => plots::gray(t),
                    })
                } else {
                    None
                };
                Pt {
                    idx: i,
                    name: doc.specs[i].name.clone(),
                    km: d.km,
                    di: d.axes.dec_inc,
                    p: d.axes.shape.map(|s| s.p),
                    t: d.axes.shape.map(|s| s.t),
                    selected: doc.selected[i],
                    color,
                }
            })
            .collect()
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
                if let Some(d) = &self.doc {
                    let (prev, next, _, _) = doc::neighbours(&d.path);
                    if let Some(p) = if a == Action::Prev { prev } else { next } {
                        self.open_path(p);
                    }
                }
            }
            Action::Reload => {
                if let Some(d) = &self.doc {
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
                        let ok = std::fs::metadata(&p).map_or(false, |m| m.len() % anisoft_core::ams::RECORD_LEN as u64 == 0);
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
                    let suffix = if self.out_phase { "oop" } else { "ip" };
                    let name = d.path.file_stem().map(|s| format!("{}_{suffix}.csv", s.to_string_lossy())).unwrap_or("export.csv".into());
                    if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name(name).save_file() {
                        let rows: Vec<&_> = (0..d.specs.len()).filter(|&i| d.active[i]).map(|i| &d.specs[i]).collect();
                        if let Err(e) = std::fs::write(&p, export::csv(&rows, self.out_phase)) {
                            self.info = Some(format!("Could not export: {e}"));
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
                        self.win_edit = true;
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
                        let stem = self.doc.as_ref().and_then(|d| d.path.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or("anisoft".into());
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
            Action::DataTable => self.win_table = !self.win_table,
            Action::Phase(op) => {
                self.out_phase = op;
                self.dirty = true;
            }
            Action::Coord(c) => {
                self.coord = c;
                self.dirty = true;
            }
            Action::GroupStats => self.win_group = !self.win_group,
            Action::Contour => self.win_contour = !self.win_contour,
            Action::Tool(name) => self.info = Some(format!("{name} is not implemented yet.")),
            Action::Factors => self.win_factors = true,
            Action::OrientParams => self.win_orient = true,
            Action::Colors => self.win_colors = true,
            Action::About => self.win_about = true,
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
            (c, Key::F1, Action::SetClasses),
            (c, Key::O, Action::Open),
            (c, Key::P, Action::Prev),
            (c, Key::N, Action::Next),
            (c, Key::R, Action::Reload),
            (c, Key::I, Action::Import),
            (c, Key::S, Action::SaveActive),
            (c, Key::A, Action::AppendSel),
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
        self.refresh();
        let mut actions = self.shortcuts(&ctx);
        self.handle_screenshot(&ctx);

        let has = self.doc.is_some();
        let th = self.theme();

        // window title like the original: Anisoft - [path (n / total)]
        let title = match &self.doc {
            Some(d) => {
                let (_, _, pos, total) = doc::neighbours(&d.path);
                format!("Anisoft - [{}  ({pos} / {total})]{}", d.path.display(), if d.modified { " *" } else { "" })
            }
            None => "Anisoft".to_string(),
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
                    if item(ui, "Export Data File (CSV)", "Ctrl+E", has) { actions.push(Action::ExportCsv) }
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
            let (prev, next) = self.doc.as_ref().map(|dc| { let n = doc::neighbours(&dc.path); (n.0.is_some(), n.1.is_some()) }).unwrap_or((false, false));
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
                egui::ComboBox::from_id_salt("tb_metric").width(70.0).selected_text(METRICS[self.metric]).show_ui(ui, |ui| {
                    for (i, m) in METRICS.iter().enumerate() {
                        ui.selectable_value(&mut self.metric, i, *m);
                    }
                }).response.on_hover_text("Quantity used for colour-coding");
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
            egui::Panel::left("browser").resizable(true).default_size(230.0).min_size(150.0).max_size(520.0).show(ui, |ui| {
                self.browser_ui(ui, &mut actions);
            });
        } else {
            egui::Panel::left("browser_strip").resizable(false).exact_size(30.0).show(ui, |ui| {
                if ui.button("▶").on_hover_text("Show file browser  Ctrl+B").clicked() {
                    actions.push(Action::ToggleBrowser);
                }
                ui.add_space(6.0);
                ui.label(RichText::new("F\ni\nl\ne\ns").weak());
            });
        }

        // ------------------------------------------------------------ specimens
        egui::Panel::left("specimens").resizable(true).default_size(170.0).min_size(110.0).max_size(420.0).show(ui, |ui| {
            self.specimens_ui(ui);
        });

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
        let mut go: Option<PathBuf> = None;
        let mut open: Option<PathBuf> = None;
        ui.horizontal(|ui| {
            if ui.button("◀").on_hover_text("Hide file browser  Ctrl+B").clicked() {
                actions.push(Action::ToggleBrowser);
            }
            ui.heading("Files");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("⬆").on_hover_text("Parent folder").clicked() {
                    go = self.browser_dir.parent().map(|p| p.to_path_buf());
                }
                if ui.button("🏠").on_hover_text("Home folder").clicked() {
                    go = std::env::var_os("HOME").map(PathBuf::from);
                }
            });
        });
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
        if let Some(p) = go {
            self.browser_dir = p;
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
            ui.checkbox(&mut self.color_on, "Color-coding");
            ui.add_enabled_ui(self.color_on, |ui| {
                egui::ComboBox::from_id_salt("metric_panel").selected_text(METRICS[self.metric]).show_ui(ui, |ui| {
                    for (i, m) in METRICS.iter().enumerate() {
                        ui.selectable_value(&mut self.metric, i, *m);
                    }
                });
                ui.radio_value(&mut self.palette, 0, "Rainbow");
                ui.radio_value(&mut self.palette, 1, "Shades");
                ui.radio_value(&mut self.palette, 2, "Grayscales");
            });
        }
        ui.separator();
        let Some(doc) = &mut self.doc else {
            ui.label(RichText::new("No file open.\nOpen one with Ctrl+O or pick it in the file browser.").weak());
            return;
        };
        let mods = ui.input(|i| i.modifiers);
        let mut clicked: Option<usize> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 20.0, doc.specs.len(), |ui, range| {
            for i in range {
                let mut t = RichText::new(&doc.specs[i].name);
                if !doc.active[i] {
                    t = t.strikethrough().weak();
                }
                if ui.selectable_label(doc.selected[i], t).clicked() {
                    clicked = Some(i);
                }
            }
        });
        if let Some(i) = clicked {
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
        if (rv.drag_stopped() || rh.drag_stopped()) && before != (self.hfrac, self.vfrac) {
            self.save_state();
        }
        for (r, resp) in [(v, &rv), (hbar, &rh)] {
            if resp.hovered() || resp.dragged() {
                ui.painter().rect_filled(r.shrink2(if r == v { Vec2::new(2.0, 0.0) } else { Vec2::new(0.0, 2.0) }), 2.0, th.accent.gamma_multiply(0.5));
            }
        }

        let pts = self.points();
        let mut click: Option<usize> = None;
        let mods = ui.input(|i| i.modifiers);
        let cs_label = match self.coord {
            CoordSys::Specimen => "SPEC",
            CoordSys::Geographic => "GEO",
            CoordSys::Paleo => "PALEO",
            CoordSys::Tectonic => "TECTO",
        };
        let rects = [tl, tr, bl, br];
        let names = ["Stereonet", "N vs Kmean", "P vs Kmean", "T vs P"];
        for i in 0..4 {
            let body = self.pane_frame(ui, rects[i], i, names[i], th);
            let out = match i {
                0 => {
                    let o = StereoOpts { show: self.show_k, size: self.marker };
                    plots::stereonet(ui, body, &pts, &o, th, cs_label)
                }
                1 => plots::hist_km(ui, body, &pts, self.bins, th),
                2 => {
                    let exp = km_exp(&pts);
                    plots::scatter(
                        ui,
                        body,
                        &pts,
                        ScatterSpec { xlabel: &format!("Kmean [E{exp:+03} SI]"), ylabel: "P", x: Box::new(|p| Some(p.km)), y: Box::new(|p| p.p), xr: None, yr: None, zero_line: false, salt: "pkm" },
                        self.marker,
                        th,
                        Some(exp),
                    )
                }
                _ => plots::scatter(
                    ui,
                    body,
                    &pts,
                    ScatterSpec { xlabel: "P", ylabel: "T", x: Box::new(|p| p.p), y: Box::new(|p| p.t), xr: None, yr: Some((-1.0, 1.0)), zero_line: true, salt: "tp" },
                    self.marker,
                    th,
                    None,
                ),
            };
            self.hover[i] = out.hover.unwrap_or_default();
            if out.click.is_some() {
                click = out.click;
            }
        }
        if let (Some(i), Some(doc)) = (click, &mut self.doc) {
            select(doc, i, mods, &mut self.last_anchor);
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
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.checkbox(&mut self.export[i], "Export graphics");
        });
        let mut top = header.max.y + 2.0;
        if self.pane_settings[i] {
            let strip = Rect::from_min_size(Pos2::new(rect.min.x + 8.0, top), Vec2::new(rect.width() - 16.0, 26.0));
            let mut s = ui.new_child(UiBuilder::new().max_rect(strip).layout(Layout::left_to_right(Align::Center)));
            s.add(egui::Slider::new(&mut self.marker, 2.0..=9.0).text("marker").max_decimals(0));
            match i {
                0 => {
                    s.checkbox(&mut self.show_k[0], "K1");
                    s.checkbox(&mut self.show_k[1], "K2");
                    s.checkbox(&mut self.show_k[2], "K3");
                }
                1 => {
                    s.add(egui::Slider::new(&mut self.bins, 3..=40).text("bins"));
                }
                _ => {}
            }
            top += 28.0;
        }
        Rect::from_min_max(Pos2::new(rect.min.x + 4.0, top), Pos2::new(rect.max.x - 4.0, rect.max.y - 4.0))
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(img) = self.shot_box.lock().unwrap().take() else { return };
        let Some(job) = self.shot.take() else { return };
        let ppp = ctx.pixels_per_point();
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

fn km_exp(pts: &[Pt]) -> i32 {
    let hi = pts.iter().map(|p| p.km.abs()).fold(0.0f64, f64::max);
    if hi > 0.0 { hi.log10().floor() as i32 } else { 0 }
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
    fn target_idx(&self) -> Vec<usize> {
        let Some(d) = &self.doc else { return vec![] };
        let sel: Vec<usize> = (0..d.specs.len()).filter(|&i| d.selected[i] && d.active[i]).collect();
        if sel.is_empty() { (0..d.specs.len()).filter(|&i| d.active[i]).collect() } else { sel }
    }

    fn windows(&mut self, ctx: &egui::Context) {
        if let Some(msg) = self.info.clone() {
            let mut open = true;
            egui::Window::new("Anisoft").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
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
        egui::Window::new("About Anisoft").default_pos([480.0, 140.0]).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.heading("Anisoft for Linux");
            ui.label("An independent reimplementation of AGICO's Anisoft 6, written in Rust with egui.");
            ui.label("Reads Safyr7 .ams files; anisotropy results were checked against csv exports of the original program.");
            ui.label(RichText::new("Anisoft is a program of AGICO, Brno. This project is not affiliated with AGICO.").weak());
        });
        self.win_about = open;

        let mut open = self.win_factors;
        egui::Window::new("Anisotropy Quantitative Factors").default_pos([420.0, 140.0]).open(&mut open).resizable(false).show(ctx, |ui| {
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
        egui::Window::new("Orientation Parameters").default_pos([420.0, 140.0]).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new("Not stored in .ams files. Applied to all specimens of the open file.").weak());
            egui::Grid::new("or").show(ui, |ui| {
                for (n, v, r) in [
                    ("Core azimuth", &mut self.orient.azimuth, 0.0..=360.0),
                    ("Core dip", &mut self.orient.dip, -90.0..=90.0),
                    ("Bedding strike", &mut self.orient.bed_strike, 0.0..=360.0),
                    ("Bedding dip", &mut self.orient.bed_dip, 0.0..=90.0),
                    ("Paleo declination", &mut self.orient.paleo_dec, -360.0..=360.0),
                ] {
                    ui.label(n);
                    changed |= ui.add(egui::DragValue::new(v).range(r).suffix(" °").speed(0.5)).changed();
                    ui.end_row();
                }
            });
            if ui.button("Reset").clicked() {
                self.orient = Orientation::default();
                changed = true;
            }
            ui.label(RichText::new("Conventions are not yet verified against Anisoft.").small().weak());
        });
        self.win_orient = open;
        if changed {
            self.dirty = true;
        }

        let mut open = self.win_colors;
        egui::Window::new("Default Colors").default_pos([420.0, 140.0]).open(&mut open).resizable(false).show(ctx, |ui| {
            for (i, n) in ["K1", "K2", "K3"].iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgba(&mut self.k_colors[i]);
                    ui.label(*n);
                });
            }
        });
        self.win_colors = open;

        let mut open = self.win_edit;
        egui::Window::new("Edit Data").default_pos([420.0, 140.0]).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label("Specimen name (up to 24 characters):");
            ui.text_edit_singleline(&mut self.edit_name);
            if ui.button("Apply").clicked() {
                if let Some(d) = &mut self.doc {
                    if let Some(i) = (0..d.specs.len()).find(|&i| d.selected[i]) {
                        d.rename(i, &self.edit_name);
                    }
                }
                self.win_edit = false;
            }
        });
        self.win_edit &= open;

        self.table_window(ctx);
        self.group_window(ctx);
        self.contour_window(ctx);
    }

    fn table_window(&mut self, ctx: &egui::Context) {
        if !self.win_table {
            return;
        }
        let mut open = true;
        let mut clicked = None;
        let mods = ctx.input(|i| i.modifiers);
        egui::Window::new("Data Table").open(&mut open).default_size([900.0, 420.0]).default_pos([300.0, 160.0]).show(ctx, |ui| {
            let Some(doc) = &self.doc else { return };
            let cols = ["Specimen", "Km", "K1", "K2", "K3", "K1 Dec", "K1 Inc", "K2 Dec", "K2 Inc", "K3 Dec", "K3 Inc", "P", "Pj", "T", "F", "F12", "F23"];
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("tbl").striped(true).min_col_width(52.0).show(ui, |ui| {
                    for c in cols {
                        ui.strong(c);
                    }
                    ui.end_row();
                    for i in 0..doc.specs.len() {
                        let d = &self.derived[i];
                        let ph = if self.out_phase { &doc.specs[i].outphase } else { &doc.specs[i].inphase };
                        let sh = d.axes.shape;
                        let f = |v: Option<f64>, p: usize| v.map(|x| format!("{x:.p$}")).unwrap_or_default();
                        let mut name = RichText::new(&doc.specs[i].name);
                        if !doc.active[i] {
                            name = name.strikethrough().weak();
                        }
                        if ui.selectable_label(doc.selected[i], name).clicked() {
                            clicked = Some(i);
                        }
                        let row = [
                            format!("{:.4E}", d.km), format!("{:.5}", d.axes.k_norm[0]), format!("{:.5}", d.axes.k_norm[1]), format!("{:.5}", d.axes.k_norm[2]),
                            format!("{:.1}", d.axes.dec_inc[0].0), format!("{:.1}", d.axes.dec_inc[0].1), format!("{:.1}", d.axes.dec_inc[1].0), format!("{:.1}", d.axes.dec_inc[1].1),
                            format!("{:.1}", d.axes.dec_inc[2].0), format!("{:.1}", d.axes.dec_inc[2].1),
                            f(sh.map(|s| s.p), 4), f(sh.map(|s| s.pj), 4), f(sh.map(|s| s.t), 4),
                            format!("{:.1}", ph.ftest[0]), format!("{:.1}", ph.ftest[1]), format!("{:.1}", ph.ftest[2]),
                        ];
                        for c in row {
                            ui.label(c);
                        }
                        ui.end_row();
                    }
                });
            });
        });
        self.win_table = open;
        if let (Some(i), Some(d)) = (clicked, &mut self.doc) {
            select(d, i, mods, &mut self.last_anchor);
        }
    }

    fn group_window(&mut self, ctx: &egui::Context) {
        if !self.win_group {
            return;
        }
        let mut open = true;
        let idx = self.target_idx();
        let tensors: Vec<_> = idx.iter().map(|&i| self.derived[i].tensor).collect();
        let kms: Vec<f64> = idx.iter().map(|&i| self.derived[i].km).collect();
        let sel = self.doc.as_ref().map_or(false, |d| d.n_selected() > 0);
        egui::Window::new("Group Statistics").open(&mut open).resizable(false).default_pos([330.0, 90.0]).show(ctx, |ui| {
            ui.label(format!("{} specimens ({}), {}", idx.len(), if sel { "selected" } else { "all active" }, if self.out_phase { "out-of-phase" } else { "in-phase" }));
            match stats::group_stats(&tensors, &kms) {
                None => {
                    ui.label("No specimens.");
                }
                Some(g) => {
                    ui.label(format!("Mean Km = {:.4E}", g.mean_km));
                    egui::Grid::new("gs").striped(true).show(ui, |ui| {
                        for h in ["", "Eigenvalue", "SD", "Dec", "Inc"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for a in 0..3 {
                            ui.strong(format!("K{}", a + 1));
                            ui.label(format!("{:.5}", g.k[a]));
                            ui.label(format!("{:.5}", g.k_sd[a]));
                            ui.label(format!("{:.1}", g.dec_inc[a].0));
                            ui.label(format!("{:.1}", g.dec_inc[a].1));
                            ui.end_row();
                        }
                    });
                    ui.separator();
                    let ang = |v: f64| if v.is_finite() { format!("{v:.1}°") } else { "n/a".to_string() };
                    ui.label(format!("Approx. 95% confidence half-angles:  K1-K2 {}   K2-K3 {}   K1-K3 {}", ang(g.eps12), ang(g.eps23), ang(g.eps13)));
                    if g.n < 3 {
                        ui.label(RichText::new("Confidence angles need at least 3 specimens.").small().weak());
                    }
                    ui.label(RichText::new("t-based approximation, not Jelinek's exact F-based formula; not yet verified against Anisoft.").small().weak());
                    ui.separator();
                    ui.label(format!("P  = {:.4} ± {:.4}", g.mean_p.0, g.mean_p.1));
                    ui.label(format!("Pj = {:.4} ± {:.4}", g.mean_pj.0, g.mean_pj.1));
                    ui.label(format!("T  = {:.4} ± {:.4}", g.mean_t.0, g.mean_t.1));
                    if let Some(s) = g.shape {
                        ui.label(RichText::new(format!("Mean tensor: P = {:.4}  Pj = {:.4}  T = {:.4}  L = {:.4}  F = {:.4}", s.p, s.pj, s.t, s.l, s.f)).weak());
                    }
                }
            }
        });
        self.win_group = open;
    }

    fn contour_window(&mut self, ctx: &egui::Context) {
        if !self.win_contour {
            return;
        }
        let mut open = true;
        let idx = self.target_idx();
        let axis = self.contour_axis;
        let vecs: Vec<Vec3> = idx.iter().map(|&i| { let (d, n) = self.derived[i].axes.dec_inc[axis]; from_dec_inc(d, n) }).collect();
        let k = self.contour_k;
        let th = self.theme();
        egui::Window::new("Contour Plot").open(&mut open).resizable(false).default_pos([760.0, 90.0]).default_width(420.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (a, n) in ["K1", "K2", "K3"].iter().enumerate() {
                    ui.selectable_value(&mut self.contour_axis, a, *n);
                }
                ui.add(egui::Slider::new(&mut self.contour_k, 10.0..=300.0).text("sharpness"));
            });
            let n = 70;
            let g = density::density(&vecs, n, k);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(400.0), Sense::hover());
            let pa = ui.painter_at(rect);
            let c = rect.center();
            let r = 175.0;
            let cell = 2.0 * r / n as f32;
            for iy in 0..n {
                for ix in 0..n {
                    let v = g.z[iy * n + ix];
                    if v.is_nan() {
                        continue;
                    }
                    let lvl = ((v / g.max.max(1e-9)) * 8.0).floor().min(7.0);
                    if lvl < 1.0 {
                        continue;
                    }
                    let band = lvl / 7.0;
                    let p = Pos2::new(c.x - r + ix as f32 * cell, c.y - r + iy as f32 * cell);
                    pa.rect_filled(Rect::from_min_size(p, Vec2::splat(cell + 0.6)), 0.0, plots::rainbow(band).gamma_multiply(0.85));
                }
            }
            pa.circle_stroke(c, r, Stroke::new(1.2, th.ink()));
            for (t, pos, al) in [("N", Pos2::new(c.x, c.y - r - 3.0), Align2::CENTER_BOTTOM), ("90", Pos2::new(c.x + r + 4.0, c.y), Align2::LEFT_CENTER), ("180", Pos2::new(c.x, c.y + r + 3.0), Align2::CENTER_TOP), ("270", Pos2::new(c.x - r - 4.0, c.y), Align2::RIGHT_CENTER)] {
                pa.text(pos, al, t, FontId::proportional(11.0), th.ink());
            }
            for (i, v) in vecs.iter().enumerate() {
                let _ = i;
                let (d, n) = anisoft_core::tensor::dec_inc(*v);
                let (x, y) = density::project(d, n);
                pa.circle_filled(Pos2::new(c.x + x as f32 * r, c.y - y as f32 * r), 1.8, Color32::from_black_alpha(160));
            }
            ui.label(format!("{} specimens, peak density {:.1} % per unit area (8 colour bands, % of peak)", idx.len(), g.max));
        });
        self.win_contour = open;
    }
}
