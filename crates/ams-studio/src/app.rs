use crate::doc::{self, Derived, Doc};
use crate::icons::{self, Icon};
use pane_cfg::PaneCfg;
use crate::plots::{self, AxisCfg, ColorCfg, NetView, Pt, ScatterSpec, StereoOpts, Theme, XyKind, ZoomView};
use crate::plots::GroupBy;
use ams_core::params::{param_class, param_sd, param_values_vol, N_PARAMS, PARAM_LABELS};
mod analysis_ui;
mod dialogs_ui;
mod core_ui;
mod export_ui;
mod pane_cfg;
mod owens_file_ui;
mod recombine_ui;
mod specset_ui;
mod volume_ui;
mod tools_ui;
use ams_core::coords::{CoordSys, Orientation};
use ams_core::export;
use egui::{Align, Align2, Color32, CornerRadius, FontId, Key, Layout, Modifiers, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Open,
    ExportAms,
    Recombine,
    Volume,
    OpenSet,
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
    Owens,
    CoreGeo,
    Colors,
    About,
    ToggleBrowser,
    ToggleTheme,
    StepSpecimen(i32),
    TogglePhase,
    ResetPanes,
    UndoResetPanes,
}

impl PartialEq for AxisCfg {
    fn eq(&self, o: &Self) -> bool {
        self.param == o.param && self.auto == o.auto && self.min == o.min && self.max == o.max && self.log == o.log
    }
}
impl PartialEq for ColorCfg {
    fn eq(&self, o: &Self) -> bool {
        self.on == o.on && self.param == o.param && self.auto == o.auto && self.min == o.min && self.max == o.max && self.log == o.log && self.palette == o.palette && self.by_class == o.by_class
    }
}

/// Which part of the window was used last (for Esc).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    None,
    Files,
    Specimens,
    Graphs,
}

/// K1 / K2 / K3 marker colours: strong ones on white, softer ones on the dark blue-grey.
const LIGHT_K: [Color32; 3] = [Color32::from_rgb(220, 30, 30), Color32::from_rgb(30, 170, 60), Color32::from_rgb(30, 60, 220)];
const DARK_K: [Color32; 3] = [Color32::from_rgb(222, 112, 118), Color32::from_rgb(104, 196, 140), Color32::from_rgb(120, 160, 238)];

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
    ("Magnitude", &[0, 24]),
    ("Shape", &[1, 2, 3, 4, 5, 6, 7, 8, 25, 26, 27]),
    ("K1 (max)", &[9, 12, 13, 28, 31]),
    ("K2 (int)", &[10, 14, 15, 29]),
    ("K3 (min)", &[11, 16, 17, 30, 32, 33]),
    ("F-tests", &[18, 19, 20]),
    ("Confidence", &[21, 22, 23]),
];

/// What a plottable parameter is (shown by the (i) next to every parameter drop-down and when hovering its entry).
fn param_info(i: usize) -> &'static str {
    match i {
        0 => "Kmean: mean (bulk) susceptibility, (K1 + K2 + K3) / 3, in SI. The size of the magnetic signal, not its anisotropy.",
        1 => "L: magnetic lineation, K1 / K2 (Balsley & Buddington). 1 = no lineation; larger = more linear fabric.",
        2 => "F: magnetic foliation, K2 / K3 (Stacey). 1 = no foliation; larger = more planar fabric.",
        3 => "P: degree of anisotropy, K1 / K3. 1 = isotropic. It ignores the intermediate axis; Pj is the corrected version.",
        4 => "Pj: corrected degree of anisotropy (Jelinek), exp(sqrt(2 * sum (ln Ki - mean ln K)^2)). 1 = isotropic; uses all three axes. The default in the plots.",
        5 => "T: shape parameter (Jelinek), (2 ln K2 - ln K1 - ln K3) / (ln K1 - ln K3). +1 = purely oblate (flattened, K1 = K2), -1 = purely prolate (elongated, K2 = K3), 0 = neutral.",
        6 => "U: shape parameter (Jelinek 1981), (2 K2 - K1 - K3) / (K1 - K3). +1 oblate, -1 prolate, like T but on the linear eigenvalues.",
        7 => "Q: anisotropy factor, (K1 - K2) / ((K1 + K2) / 2 - K3). Large for lineated fabrics, small for foliated ones.",
        8 => "E: ellipticity, K2^2 / (K1 K3). Above 1 the intermediate axis is closer to K1 (prolate side), below 1 closer to K3 (oblate side).",
        9 => "K1: the largest normalised principal susceptibility (maximum axis); the three are scaled to a mean of 1.",
        10 => "K2: the intermediate normalised principal susceptibility.",
        11 => "K3: the smallest normalised principal susceptibility (minimum axis, the pole of the magnetic foliation).",
        12 => "K1_Dec: declination (azimuth, 0-360 deg) of the K1 axis, in the coordinate system that is selected (S, G, P or T).",
        13 => "K1_Inc: inclination (plunge below the horizontal, -90 to 90 deg) of the K1 axis.",
        14 => "K2_Dec: declination (azimuth, 0-360 deg) of the K2 axis.",
        15 => "K2_Inc: inclination (plunge) of the K2 axis.",
        16 => "K3_Dec: declination (azimuth, 0-360 deg) of the K3 axis.",
        17 => "K3_Inc: inclination (plunge) of the K3 axis.",
        18 => "FTest: F-test of the anisotropy as a whole (is the specimen anisotropic at all?). Larger = more clearly anisotropic.",
        19 => "FTest_F12: F-test for the difference between K1 and K2 (is there a real lineation?).",
        20 => "FTest_F23: F-test for the difference between K2 and K3 (is there a real foliation?).",
        21 => "E12: confidence angle (deg) of the K1-K2 plane from the measurement scatter (approximate, about 5 % from the original's value). Small = well defined.",
        22 => "E23: confidence angle (deg) of the K2-K3 plane (approximate). Small = well defined.",
        23 => "E13: confidence angle (deg) of the K1-K3 plane (approximate). Small = well defined.",
        24 => "Kmean_vol*: mean susceptibility corrected for the sample volume, Kmean * 10 / volume (the Kappabridge reports Km for 10 cm3). Lab sheet quantity (Edit > Volume Correction).",
        25 => "L_Khan*: lineation after Khan, (K1 - K2) / Kmean, on the volume-corrected values. Lab sheet quantity.",
        26 => "F_Khan*: foliation after Khan, (K2 - K3) / Kmean, on the volume-corrected values. Lab sheet quantity.",
        27 => "H_Owens*: total anisotropy after Owens, (K1 - K3) / Kmean, on the volume-corrected values. Lab sheet quantity.",
        28 => "K1_unnorm*: K1 multiplied back to SI (normalised K1 * volume-corrected Kmean). Lab sheet quantity.",
        29 => "K2_unnorm*: K2 in SI (normalised K2 * volume-corrected Kmean). Lab sheet quantity.",
        30 => "K3_unnorm*: K3 in SI (normalised K3 * volume-corrected Kmean). Lab sheet quantity.",
        31 => "Lin_Plunge*: plunge of the magnetic lineation (the K1 axis) in deg. Lab sheet quantity.",
        32 => "Fol_Dip*: dip of the magnetic foliation (the plane perpendicular to K3) in deg. Lab sheet quantity.",
        33 => "Fol_DipDir*: dip direction of the magnetic foliation in deg. Lab sheet quantity.",
        _ => "",
    }
}

/// Drop-down of all plottable parameters. It opens as one wide panel with the parameters
/// grouped by type in columns, each on the background colour the original uses.
fn param_combo(ui: &mut Ui, id: &str, v: &mut usize, dark: bool) -> bool {
    let mut changed = false;
    let cur = RichText::new(PARAM_LABELS[(*v).min(N_PARAMS - 1)]);
    let cur = match class_color(param_class(*v), dark) { Some(c) => cur.background_color(c).color(Color32::BLACK), None => cur };
    ui.horizontal(|ui| {
    egui::ComboBox::from_id_salt(id).width(120.0).selected_text(cur).show_ui(ui, |ui| {
        ui.horizontal_top(|ui| {
            for (title, items) in PARAM_GROUPS {
                ui.vertical(|ui| {
                    ui.set_min_width(88.0);
                    ui.label(RichText::new(title).small().strong());
                    for &i in items {
                        let t = RichText::new(PARAM_LABELS[i]);
                        let t = match class_color(param_class(i), dark) { Some(c) => t.background_color(c).color(Color32::BLACK), None => t };
                        if ui.add(egui::Button::selectable(*v == i, t).min_size(Vec2::new(84.0, 0.0))).on_hover_text(param_info(i)).clicked() {
                            *v = i;
                            changed = true;
                            ui.close();
                        }
                    }
                });
                ui.add_space(6.0);
            }
        });
        ui.separator();
        ui.label(RichText::new("* belongs to the M3ORE lab sheet (not in Anisoft). Corrected for the sample volume where it applies: Edit > Volume Correction.").small().weak());
    });
    // what the chosen parameter is
    info_icon(ui, param_info((*v).min(N_PARAMS - 1)));
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

fn color_ui(ui: &mut Ui, id: &str, c: &mut ColorCfg, dark: bool, title: &str) {
    ui.checkbox(&mut c.on, RichText::new(title).strong());
    // the settings only show while colour-coding is on (keeps the pop-outs short enough for the screen)
    if c.on {
        ui.horizontal(|ui| {
            ui.radio_value(&mut c.by_class, false, "Parameter");
            ui.radio_value(&mut c.by_class, true, "Data Classes").on_hover_text("Colour by the classes set in Edit > Set Data Classes");
        });
        ui.add_enabled_ui(!c.by_class, |ui| {
            param_combo(ui, id, &mut c.param, dark);
            range_fields(ui, &mut c.auto, &mut c.min, &mut c.max, &mut c.log, id);
            ui.horizontal(|ui| {
                ui.radio_value(&mut c.palette, 0, "Rainbow");
                ui.radio_value(&mut c.palette, 1, "Shades");
                ui.radio_value(&mut c.palette, 2, "Grayscales");
            });
        });
    }
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
const W_OWENS: usize = 13;
const W_CLASSES: usize = 14;
const W_SET: usize = 15;
const W_VOL: usize = 16;
const W_RECOMB: usize = 17;
const W_EXPORT: usize = 18;
const W_CORE: usize = 19;

struct ShotJob {
    copy: bool,
    base: Option<PathBuf>,
    rects: Vec<(&'static str, Rect)>,
}

/// A switch like the ones on a phone: a rounded track with a knob that slides to the right when on.
fn phone_switch(ui: &mut Ui, on: &mut bool) -> egui::Response {
    let size = Vec2::new(46.0, 25.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let dark = ui.visuals().dark_mode;
    let on_col = if dark { Color32::from_rgb(106, 160, 255) } else { Color32::from_rgb(47, 111, 222) };
    let off_col = if dark { Color32::from_rgb(58, 64, 76) } else { Color32::from_rgb(190, 196, 206) };
    let track = off_col.lerp_to_gamma(on_col, t);
    let r = rect.height() / 2.0;
    ui.painter().rect_filled(rect, r, track);
    let x = egui::lerp((rect.min.x + r)..=(rect.max.x - r), t);
    let c = Pos2::new(x, rect.center().y);
    ui.painter().circle_filled(c + Vec2::new(0.0, 1.0), r - 2.5, Color32::from_black_alpha(50));
    ui.painter().circle_filled(c, r - 2.5, Color32::WHITE);
    if resp.hovered() {
        ui.painter().rect_stroke(rect, r, Stroke::new(1.0, on_col.gamma_multiply(0.7)), StrokeKind::Outside);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, *on, ""));
    resp
}

/// Top row of the Owens window: a back arrow (one step back; greyed at the first step), the classic / new switch and the
/// short notice. Returns (the switch was flipped, the back arrow was clicked).
fn owens_head(ui: &mut Ui, old: &mut bool, toast: &Option<(String, f64)>, now: f64, accent: Color32, back_on: bool, show_switch: bool) -> (bool, bool) {
    let mut switched = false;
    let mut back = false;
    ui.horizontal(|ui| {
        back = ui.add_enabled(back_on, egui::Button::new(RichText::new("\u{2190}").strong()).min_size(Vec2::new(30.0, 0.0))).on_hover_text("Back one step").on_disabled_hover_text("This is the first step").clicked();
        if !show_switch {
            // the steps of a correction from a file: classic and new Owens do not apply there, so the switch is not shown
            ui.label(RichText::new("Owens correction from a file").strong());
            return;
        }
        let mut new_mode = !*old;
        if phone_switch(ui, &mut new_mode).on_hover_text("Left: classic Owens of Anisoft 4.2 (a table with one row per specimen).\nRight: new Owens (four values and a row range).").changed() {
            *old = !new_mode;
            switched = true;
        }
        ui.label(RichText::new(if *old { "Classic Owens" } else { "New Owens" }).strong());
        ui.label(RichText::new(if *old { "(Anisoft 4.2)" } else { "(one set of values)" }).weak());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some((msg, t0)) = toast {
                let age = now - t0;
                if age < 3.0 {
                    let fade = (3.0 - age).min(0.5) as f32 / 0.5;
                    ui.label(RichText::new(msg).small().color(accent.gamma_multiply(fade)));
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
                }
            }
        });
    });
    (switched, back)
}

/// Drag box for a strike (0-360, k even) or a dip (0-180, k odd).
fn angle_drag(v: &mut f64, k: usize) -> egui::DragValue<'_> {
    egui::DragValue::new(v).range(if k % 2 == 0 { 0.0..=360.0 } else { 0.0..=180.0 }).speed(0.5).max_decimals(1).suffix(" \u{b0}")
}

/// A small "i" in a circle that explains something when hovered.
fn info_icon(ui: &mut Ui, text: &str) {
    // small and raised like a superscript; the whole cell around it answers to the pointer
    let row = ui.spacing().interact_size.y;
    let s = (row * 0.44).max(9.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new((s + 5.0).max(18.0), row), Sense::hover());
    let col = if resp.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
    let c = Pos2::new(rect.min.x + 2.5 + s * 0.5, rect.center().y - row * 0.2);
    ui.painter().circle_stroke(c, s * 0.5, Stroke::new(1.0, col));
    ui.painter().text(c + Vec2::new(0.0, 0.3), Align2::CENTER_CENTER, "i", FontId::proportional(s * 0.72), col);
    resp.on_hover_text(text);
}

/// The open file as it was before the last rotation (and the pending rotations of all files).
pub(super) struct OwensUndo {
    raw: Vec<u8>,
    specs: Vec<ams_core::ams::Specimen>,
    pending: std::collections::HashMap<PathBuf, Vec<Option<ams_core::tensor::Mat3>>>,
    done: std::collections::HashMap<PathBuf, Vec<bool>>,
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
    export: [bool; 4],
    /// The Graphics > Export graphics menu is open: the panes it would export are highlighted.
    export_menu_open: bool,
    /// The state before the last "Reset all panes" (undo is offered for ten minutes).
    reset_snap: Option<pane_cfg::ResetSnap>,
    /// The pane that was clicked last, and whether the selection in the plots came from a drag.
    last_focus: Focus,
    last_band: bool,
    /// What the three plot panes draw (top right, bottom left, bottom right), and the settings that only belong to one file.
    panes: [PaneCfg; 3],
    spec_cfg: [std::collections::HashMap<PathBuf, PaneCfg>; 3],
    /// The arrow button of each pane's header (the options pop-out points at it).
    arrow_rect: [Rect; 4],
    /// Height of each pane's pop-out when it was last open (to decide whether it fits below the header).
    popup_h: [f32; 4],
    /// Zoom of the three plot panes.
    views: [ZoomView; 3],
    /// P and not Pj in the plots that show it.
    use_p: bool,
    color: [ColorCfg; 4],
    color_master: ColorCfg,
    /// The pane has colour-coding of its own (set in its options); the others follow `color_master`.
    color_own: [bool; 4],
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
    /// The K1/K2/K3 marker colours of the theme that is not shown right now (dark mode has softer ones).
    k_other: [Color32; 3],
    dark: bool,

    win_table: bool,
    win_group: bool,
    win_contour: bool,
    win_orient: bool,
    win_colors: bool,
    win_factors: bool,
    win_about: bool,
    win_edit: bool,
    /// Files that were edited and left again without saving: kept so that nothing is lost while browsing.
    stash: std::collections::HashMap<PathBuf, Doc>,
    /// Volume information (shape, date) of files whose volume was set but not saved yet.
    vol_pending: std::collections::HashMap<PathBuf, doc::VolInfo>,
    win_export: bool,
    /// The "unsaved edits" question that opens when the program is asked to close.
    quit_prompt: bool,
    /// The user chose to leave without saving: the next close request goes through.
    quit_ok: bool,
    ex: export_ui::ExportState,
    /// Files selected in the browser tree (Shift: range, Ctrl: single) and the anchor of a range.
    tree_sel: Vec<PathBuf>,
    tree_anchor: Option<PathBuf>,
    /// Files that were exported with their rotation.
    exported: std::collections::HashSet<PathBuf>,
    /// Folder set and file of the last session, restored at start.
    restore_root: Option<PathBuf>,
    restore_file: Option<PathBuf>,
    /// The file the browser tree last showed as current (to scroll to it when it changes).
    tree_cur: Option<PathBuf>,
    win_recomb: bool,
    rc: recombine_ui::RecState,
    win_vol: bool,
    win_core: bool,
    core: core_ui::CoreState,
    vol: volume_ui::VolState,
    win_set: bool,
    /// The parent-folder dialog, the confirmed set shown in the browser, and the identifier used last.
    sd: specset_ui::SetDialog,
    set: Option<specset_ui::SpecSet>,
    last_ident: String,
    /// Zoom and pan of the stereonet.
    st_view: NetView,
    win_classes: bool,
    edit: dialogs_ui::EditState,
    /// Data classes of the open file: definitions and, per specimen, an index into them.
    classes: Vec<dialogs_ui::ClassDef>,
    class_of: Vec<Option<usize>>,
    cl: dialogs_ui::ClassState,
    win_owens: bool,
    /// Owens correction with the values read from a file.
    of: owens_file_ui::OwensFile,
    /// Owens rows (FS, FD, DS, DD per specimen) and the file as it was before the last rotation.
    owens: Vec<[f64; 4]>,
    /// New-style Owens input: one set of FS / FD / DS / DD and a 1-based first/last row.
    owens_one: [f64; 4],
    owens_range: (usize, usize),
    /// Old style = the per-row table of Anisoft 4.2.
    owens_old: bool,
    /// Short notice next to the Owens toggle: (text, time it appeared).
    owens_toast: Option<(String, f64)>,
    owens_undo: Option<OwensUndo>,
    /// Records that were Owens-rotated in memory (per file); the ones rotated before are in the file's `.owens.json`.
    owens_done: std::collections::HashMap<PathBuf, Vec<bool>>,
    /// The rectangle of the file browser (the Owens window keeps clear of it and points at it).
    tree_rect: Rect,
    /// The pointer was over "the files selected in the file tree" last frame: the rest of the window is dimmed.
    tree_hint: bool,
    /// Rotations of files that are not saved yet, per file and record (applied when the file is opened, used by Export as .ams).
    pending: std::collections::HashMap<PathBuf, Vec<Option<ams_core::tensor::Mat3>>>,
    ana: analysis_ui::AnaState,
    info: Option<String>,
    hover: [String; 4],
    last_anchor: Option<usize>,
    win_serial: [u64; 24],
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
            export: [true; 4],
            export_menu_open: false,
            reset_snap: None,
            last_focus: Focus::None,
            last_band: false,
            panes: [PaneCfg::default_for(0, false), PaneCfg::default_for(1, false), PaneCfg::default_for(2, false)],
            spec_cfg: Default::default(),
            arrow_rect: [Rect::NOTHING; 4],
            popup_h: [0.0; 4],
            views: [ZoomView::default(); 3],
            use_p: false,
            color: Default::default(),
            color_master: ColorCfg::default(),
            color_own: [false; 4],
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
            k_colors: LIGHT_K,
            k_other: DARK_K,
            dark: false,
            win_table: false,
            win_group: false,
            win_contour: false,
            win_orient: false,
            win_colors: false,
            win_factors: false,
            win_about: false,
            win_edit: false,
            stash: Default::default(),
            vol_pending: Default::default(),
            win_export: false,
            quit_prompt: false,
            quit_ok: false,
            ex: Default::default(),
            tree_sel: vec![],
            tree_anchor: None,
            exported: Default::default(),
            restore_root: None,
            restore_file: None,
            tree_cur: None,
            win_recomb: false,
            rc: Default::default(),
            win_vol: false,
            win_core: false,
            core: Default::default(),
            vol: Default::default(),
            win_set: false,
            sd: Default::default(),
            set: None,
            last_ident: String::new(),
            st_view: NetView::default(),
            win_classes: false,
            edit: Default::default(),
            classes: vec![],
            class_of: vec![],
            cl: Default::default(),
            win_owens: false,
            of: Default::default(),
            owens: vec![],
            owens_one: [0.0; 4],
            owens_range: (1, 1),
            owens_old: true,
            owens_toast: None,
            owens_undo: None,
            owens_done: Default::default(),
            tree_rect: Rect::NOTHING,
            tree_hint: false,
            pending: Default::default(),
            ana: Default::default(),
            info: None,
            hover: Default::default(),
            last_anchor: None,
            win_serial: [0; 24],
            list_drag: None,
            tools: Default::default(),
            tool_copy: None,
            shot: None,
            shot_box: Default::default(),
            last_title: String::new(),
        };
        app.load_state();
        app.load_plots();
        if app.dark {
            std::mem::swap(&mut app.k_colors, &mut app.k_other);
        }
        apply_style(&cc.egui_ctx, app.dark);
        if open.is_none() {
            // like TX Studio: start where the last session ended
            if let Some(root) = app.restore_root.take() {
                let f = app.restore_file.take();
                app.restore_set(&cc.egui_ctx, root, f);
            } else if let Some(f) = app.restore_file.take() {
                app.open_path(f);
            }
        }
        if let Some(p) = open {
            if p.is_dir() {
                // a folder on the command line is treated as the parent of the specimen folders
                app.begin_scan(&cc.egui_ctx, p);
            } else {
                app.open_path(p);
            }
        }
        // the last Save as .ams of the previous session can still be undone
        app.ex.ops = export_ui::load_journal();
        for o in &app.ex.ops {
            app.exported.insert(o.src.clone());
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
                "use_p" => {
                    self.use_p = v == "1";
                    self.apply_p_choice();
                }
                "marker" => self.marker = v.parse().unwrap_or(DEFAULT_MARKER).clamp(2.0, 9.0),
                "browser_w" => self.browser_w = v.parse().unwrap_or(230.0f32).clamp(150.0, 520.0),
                "spec_w" => self.spec_w = v.parse().unwrap_or(170.0f32).clamp(110.0, 420.0),
                "win_w" | "win_h" => {
                    let x: f32 = v.parse().unwrap_or(0.0);
                    let cur = self.win_size.get_or_insert([1280.0, 820.0]);
                    cur[usize::from(k == "win_h")] = x.clamp(760.0, 8000.0);
                }
                "maximized" => self.win_max = v == "1",
                "owens_new" => self.owens_old = v != "1",
                "set_ident" => self.last_ident = v.to_string(),
                "set_root" if Path::new(v).is_dir() => self.restore_root = Some(PathBuf::from(v)),
                "last_file" if Path::new(v).is_file() => self.restore_file = Some(PathBuf::from(v)),
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
                "last_dir={}\ndark={}\nbrowser={}\nhfrac={}\nvfrac={}\nuse_p={}\nmarker={}\nbrowser_w={}\nspec_w={}\nwin_w={}\nwin_h={}\nmaximized={}\nowens_new={}\nset_ident={}\nset_root={}\nlast_file={}\n",
                self.browser_dir.display(),
                self.dark as u8,
                self.browser_open as u8,
                self.hfrac,
                self.vfrac,
                self.use_p as u8,
                self.marker,
                self.browser_w,
                self.spec_w,
                self.win_size.map_or(1280.0, |s| s[0]),
                self.win_size.map_or(820.0, |s| s[1]),
                self.win_max as u8,
                (!self.owens_old) as u8,
                self.last_ident.replace(['\n', '\r'], ""),
                self.set.as_ref().map(|s| s.root.display().to_string()).unwrap_or_default(),
                self.doc.as_ref().filter(|d| !d.synthetic).map(|d| d.path.display().to_string()).unwrap_or_default()
            ),
        );
    }

    /// Keeps the open file with its edits (rotation, deleted records, names, volume ...) while another is opened.
    fn stash_current(&mut self) {
        if self.doc.as_ref().map_or(false, |d| d.modified && !d.synthetic) {
            if let Some(d) = self.doc.take() {
                self.stash.insert(d.path.clone(), d);
            }
        }
    }

    /// Every file with edits that are not saved: the open one, the stashed ones and those with only a held rotation.
    pub(super) fn edited_files(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = vec![];
        if let Some(d) = self.doc.as_ref().filter(|d| d.modified && !d.synthetic) {
            v.push(d.path.clone());
        }
        for k in self.stash.keys().chain(self.pending.keys()) {
            if !v.contains(k) {
                v.push(k.clone());
            }
        }
        v.sort();
        v
    }

    /// Edits that live in memory only: the files from `edited_files` plus an open data set without a file.
    pub(super) fn unsaved_count(&self) -> usize {
        self.edited_files().len() + usize::from(self.doc.as_ref().map_or(false, |d| d.synthetic && d.modified))
    }

    /// Closing the program (window button, Alt+F4, File > Exit): asks first when edits would be lost.
    fn guard_quit(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_ok && self.unsaved_count() > 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.quit_prompt = true;
        }
    }

    fn quit_window(&mut self, ctx: &egui::Context) {
        if !self.quit_prompt {
            return;
        }
        let n = self.unsaved_count();
        let files = self.edited_files();
        let (mut save, mut leave, mut stay) = (false, false, false);
        egui::Window::new("Unsaved edits").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.3);
            ui.set_max_width(620.0);
            ui.label(RichText::new(format!("{n} file(s) have edits that are not saved.")).strong());
            ui.label("Rotations, deleted records, names, orientation and volume are only kept in memory until they are saved; closing the program loses them.");
            for f in files.iter().take(8) {
                ui.label(RichText::new(f.display().to_string()).small().monospace());
            }
            if files.len() > 8 {
                ui.label(RichText::new(format!("... and {} more", files.len() - 8)).small().weak());
            }
            ui.horizontal(|ui| {
                save = ui.add(egui::Button::new(RichText::new("Save as .ams ...").strong().color(Color32::WHITE)).fill(Color32::from_rgb(47, 111, 222))).clicked();
                leave = ui.add(egui::Button::new(RichText::new("Quit without saving").color(Color32::from_rgb(214, 90, 90)))).clicked();
                stay = ui.button("Cancel").clicked();
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            stay = true;
        }
        if save {
            self.quit_prompt = false;
            self.ex.scope = 1;
            self.open_export();
            self.ex.scope = 1;
            self.build_rows_pub();
        } else if leave {
            self.quit_prompt = false;
            self.quit_ok = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if stay {
            self.quit_prompt = false;
        }
    }

    /// Rotates records of the open file now and remembers the rotation for the file: it is applied again when the
    /// file is opened later and used by Export as .ams. Nothing is written.
    pub(super) fn rotate_doc(&mut self, jobs: &[(usize, ams_core::tensor::Mat3)]) -> Result<(), String> {
        let Some(d) = &self.doc else { return Ok(()) };
        // a sample that is already Owens-rotated is not rotated again
        if !d.synthetic {
            let names: Vec<String> = d.specs.iter().map(|s| s.name.clone()).collect();
            self.owens_check(&d.path.clone(), jobs.iter().map(|j| j.0), Some(&names))?;
        }
        let Some(d) = &mut self.doc else { return Ok(()) };
        self.owens_undo = Some(OwensUndo { raw: d.raw.clone(), specs: d.specs.clone(), pending: self.pending.clone(), done: self.owens_done.clone() });
        for (i, r) in jobs {
            d.rotate_specimen(*i, r);
        }
        let (path, n, synthetic) = (d.path.clone(), d.specs.len(), d.synthetic);
        if !synthetic {
            self.add_pending(&path, n, jobs);
        }
        self.dirty = true;
        Ok(())
    }

    /// Error text when any of the records `idx` of `path` is already Owens-rotated (the correction is not applied then).
    pub(super) fn owens_check(&self, path: &Path, idx: impl Iterator<Item = usize>, names: Option<&[String]>) -> Result<(), String> {
        let marks = doc::read_owens_marks(path);
        let bad: Vec<usize> = idx.filter(|&i| self.owens_done.get(path).and_then(|v| v.get(i)).copied().unwrap_or(false) || marks.get(i).copied().unwrap_or(false)).collect();
        if bad.is_empty() {
            return Ok(());
        }
        let list: Vec<String> = bad.iter().take(6).map(|&i| names.and_then(|n| n.get(i)).cloned().unwrap_or_else(|| format!("record {}", i + 1))).collect();
        Err(format!(
            "Not rotated: {} record(s) of {} are already rotated to geographic coordinates (Owens or drill core) ({}{}). Rotating them again would turn the data twice.\nUse Undo to take the earlier rotation back, or open the original, unrotated file.",
            bad.len(),
            path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            list.join(", "),
            if bad.len() > 6 { ", ..." } else { "" }
        ))
    }

    pub(super) fn undo_doc_rotation(&mut self) {
        if let (Some(d), Some(u)) = (&mut self.doc, self.owens_undo.take()) {
            d.raw = u.raw;
            d.specs = u.specs;
            self.pending = u.pending;
            self.owens_done = u.done;
            self.dirty = true;
        }
    }

    /// Adds rotations to the pending ones of a file (a later one acts after an earlier one).
    pub(super) fn add_pending(&mut self, path: &Path, n: usize, jobs: &[(usize, ams_core::tensor::Mat3)]) {
        // these records now count as Owens-rotated
        let done = self.owens_done.entry(path.to_path_buf()).or_insert_with(|| vec![false; n]);
        done.resize(n, false);
        for (i, _) in jobs {
            if let Some(m) = done.get_mut(*i) {
                *m = true;
            }
        }
        if let Some(d) = self.stash.get_mut(path) {
            for (i, r) in jobs {
                if *i < d.specs.len() {
                    d.rotate_specimen(*i, r);
                }
            }
            return;
        }
        let v = self.pending.entry(path.to_path_buf()).or_insert_with(|| vec![None; n]);
        v.resize(n, None);
        for (i, r) in jobs {
            if let Some(slot) = v.get_mut(*i) {
                *slot = Some(match slot {
                    Some(old) => ams_core::tensor::mat_mul(r, old),
                    None => *r,
                });
            }
        }
    }

    fn open_path(&mut self, p: PathBuf) {
        // the file that is open now stays as it is, with its edits, until it is saved
        if self.doc.as_ref().map_or(false, |d| d.path == p && !d.synthetic && d.modified) {
            return;
        }
        self.stash_current();
        let restored = self.stash.remove(&p);
        let opened = match restored {
            Some(d) => Ok(d),
            None => Doc::open(&p),
        };
        let fresh = !self.stash.contains_key(&p);
        match opened {
            Ok(mut d) => {
                // rotations that were made earlier but not saved are applied (a restored file has them already)
                let apply_pending = d.raw.len() > 0 && !d.modified;
                let _ = fresh;
                if let (true, Some(v)) = (apply_pending, self.pending.get(&p)) {
                    for (i, r) in v.iter().enumerate() {
                        if let (Some(r), true) = (r, i < d.specs.len()) {
                            d.rotate_specimen(i, r);
                        }
                    }
                }
                if let Some(parent) = p.parent() {
                    self.browser_dir = parent.to_path_buf();
                }
                self.doc = Some(d);
                self.classes.clear();
                self.class_of.clear();
                self.load_volume_info();
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
        Theme { dark: self.dark, k_colors: self.k_colors, accent: if self.dark { Color32::from_rgb(106, 160, 255) } else { Color32::from_rgb(59, 130, 246) } }
    }

    fn refresh(&mut self) {
        if self.dirty {
            self.derived = self.doc.as_ref().map(|d| d.derive(self.out_phase, self.coord, &self.orient)).unwrap_or_default();
            self.dirty = false;
        }
    }

    /// The Specimens-panel / toolbar colour-coding is the general one: it applies to every plot that has markers and no
    /// colour-coding of its own. A plot with its own setting keeps it.
    fn apply_master_color(&mut self) {
        for i in [0usize, 1, 2, 3] {
            if !self.color_own[i] {
                self.color[i] = self.color_master.clone();
            }
        }
    }

    /// The colour-coding block of a plot's options: this plot only, or back to the general one.
    fn pane_color_ui(&mut self, ui: &mut Ui, i: usize, id: &str) {
        let dark = self.dark;
        let before = self.color[i].clone();
        color_ui(ui, id, &mut self.color[i], dark, "Color-coding (this plot only)");
        if before != self.color[i] {
            self.color_own[i] = true;
        }
        if self.color_own[i] {
            ui.horizontal(|ui| {
                ui.label(RichText::new("This plot has its own colour-coding.").weak().small());
                if ui.small_button("Use the general one").on_hover_text("Follow the colour-coding above the specimen list (all plots) again").clicked() {
                    self.color_own[i] = false;
                    self.color[i] = self.color_master.clone();
                }
            });
        } else {
            ui.label(RichText::new("Follows the general colour-coding (above the specimen list, all plots) until you change it here.").weak().small());
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
                let vol = if doc.specs[i].volume_cm3 > 0.0 { doc.specs[i].volume_cm3 } else { ams_core::params::REF_VOLUME };
                let vals = param_values_vol(d.km, &d.axes, ph, vol);
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
    fn colorized(pts: &[Pt], cfg: &ColorCfg, classes: &[Option<Color32>]) -> (Vec<Pt>, Option<(f64, f64)>) {
        if !cfg.on {
            return (pts.to_vec(), None);
        }
        if cfg.by_class {
            // class colours; specimens without a class keep the default marker colour
            let out = pts.iter().map(|p| Pt { color: classes.get(p.idx).copied().flatten(), ..p.clone() }).collect();
            return (out, None);
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
            Action::OpenSet => self.open_folder_of_specimens(ctx),
            Action::Volume => self.open_volume(),
            Action::Recombine => self.open_recombine(),
            Action::ExportAms => self.open_export(),
            Action::Prev | Action::Next => {
                if let Some(d) = self.doc.as_ref().filter(|d| !d.synthetic) {
                    let (prev, next, _, _) = self.neighbours_of(&d.path);
                    if let Some(p) = if a == Action::Prev { prev } else { next } {
                        self.open_path(p);
                    }
                }
            }
            Action::Reload => {
                if let Some(d) = self.doc.as_ref().filter(|d| !d.synthetic) {
                    // reloading throws the edits of this file away
                    let p = d.path.clone();
                    self.stash.remove(&p);
                    self.pending.remove(&p);
                    self.owens_done.remove(&p);
                    self.vol_pending.remove(&p);
                    self.doc.as_mut().unwrap().modified = false;
                    self.open_path(p);
                }
            }
            Action::Import => self.import_files(),
            Action::SaveActive => {
                if let Some(d) = &self.doc {
                    // same naming as Save as .ams: <folder>/_geo/<name>_geo.ams
                    let (name, geo_dir) = if d.synthetic {
                        ("active.ams".to_string(), None)
                    } else {
                        let g = export_ui::geo_path(&d.path);
                        (g.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or("active.ams".into()), g.parent().map(|p| p.to_path_buf()))
                    };
                    let made_dir = geo_dir.as_ref().filter(|g| !g.exists() && std::fs::create_dir_all(g).is_ok()).cloned();
                    let mut dlg = rfd::FileDialog::new().add_filter("AMS files", &["ams"]).set_file_name(name).set_title("Save the active records (default: the _geo folder)");
                    if let Some(g) = geo_dir.as_ref().filter(|g| g.is_dir()) {
                        dlg = dlg.set_directory(g);
                    }
                    let picked = dlg.save_file();
                    if let Some(g) = &made_dir {
                        let _ = std::fs::remove_dir(g); // only goes when nothing was saved into it
                    }
                    if let Some(p) = picked {
                        let bytes = d.records((0..d.specs.len()).filter(|&i| d.active[i]));
                        let n_rec = bytes.len() / ams_core::ams::RECORD_LEN;
                        match std::fs::write(&p, &bytes) {
                            Err(e) => self.info = Some(format!("Could not save: {e}")),
                            Ok(()) => {
                                // the volume information entered for this file goes next to the saved file, as with Save as .ams
                                if let Some(info) = self.vol_pending.get(&d.path).filter(|_| !d.synthetic) {
                                    let vols: Vec<f64> = ams_core::ams::parse(&bytes).map(|v| v.iter().map(|s| s.volume_cm3).collect()).unwrap_or_default();
                                    let _ = doc::write_volume_json(&p, info, &vols);
                                }
                                let others = self.edited_files().into_iter().filter(|f| *f != d.path).count();
                                if others > 0 {
                                    self.info = Some(format!("Saved {n_rec} active record(s) of this file.\n{others} other file(s) still have edits held in memory (rotations, names, volume); File > Save as .ams writes those."));
                                }
                            }
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
                self.stash_current();
                self.doc = None;
                self.dirty = true;
            }
            Action::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::EditData => self.open_edit_data(),
            Action::CoreGeo => {
                if self.doc.is_some() {
                    self.open_core();
                }
            }
            Action::Owens => {
                if let Some(d) = &self.doc {
                    if self.owens.len() != d.specs.len() {
                        self.owens = vec![[0.0; 4]; d.specs.len()];
                        self.owens_undo = None;
                    }
                    self.owens_range = (1, d.specs.len().max(1));
                    self.open_win(W_OWENS);
                }
            }
            Action::SetClasses => self.open_classes(),
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
                            d.modified = true;
                        }
                    }
                }
            }
            Action::RestoreAll => {
                if let Some(d) = &mut self.doc {
                    if d.active.iter().any(|a| !*a) {
                        d.modified = true;
                    }
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
            Action::ResetPanes => self.reset_all_panes(),
            Action::UndoResetPanes => self.undo_reset_panes(),
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
                std::mem::swap(&mut self.k_colors, &mut self.k_other);
                apply_style(ctx, self.dark);
                self.save_state();
            }
        }
    }

    /// A dialog, pop-out or message is open (the egui drop-down menus are asked separately).
    fn any_dialog_open(&self) -> bool {
        self.win_table || self.win_group || self.win_contour || self.win_orient || self.win_colors || self.win_factors || self.win_about || self.win_edit
            || self.win_export || self.win_recomb || self.win_vol || self.win_core || self.win_set || self.win_classes || self.win_owens
            || self.tools.open.iter().any(|o| *o) || self.info.is_some() || self.quit_prompt || self.pane_settings.iter().any(|o| *o)
    }

    /// Esc: closes every dialog and pop-out. The Owens window starts from its beginning next time.
    fn close_all_dialogs(&mut self) {
        self.win_table = false;
        self.win_group = false;
        self.win_contour = false;
        self.win_orient = false;
        self.win_colors = false;
        self.win_factors = false;
        self.win_about = false;
        self.win_edit = false;
        self.win_export = false;
        self.win_recomb = false;
        self.win_vol = false;
        self.win_core = false;
        self.win_set = false;
        self.win_classes = false;
        self.close_owens();
        self.tools.open.iter_mut().for_each(|o| *o = false);
        self.info = None;
        self.quit_prompt = false;
        self.pane_settings = [false; 4];
    }

    /// Esc with nothing open: clears what was selected in the pane that was used last (files or sub-specimens); after
    /// the plots it only undoes a drag selection.
    fn esc_clear_selection(&mut self) {
        let files = !self.tree_sel.is_empty();
        let specs = self.doc.as_ref().map_or(false, |d| d.n_selected() > 0);
        let clear_files = |s: &mut App| s.tree_sel.clear();
        let clear_specs = |s: &mut App| {
            if let Some(d) = &mut s.doc {
                d.selected.iter_mut().for_each(|a| *a = false);
            }
        };
        match self.last_focus {
            Focus::Files if files => clear_files(self),
            Focus::Files if specs => clear_specs(self),
            Focus::Specimens if specs => clear_specs(self),
            Focus::Specimens if files => clear_files(self),
            Focus::Graphs => {
                // only the last drag selection goes
                if self.last_band && specs {
                    clear_specs(self);
                }
                self.last_band = false;
            }
            Focus::None if specs => clear_specs(self),
            _ => {}
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = vec![];
        let c = Modifiers::CTRL;
        let table: Vec<(Modifiers, Key, Action)> = vec![
            (Modifiers::SHIFT | Modifiers::CTRL, Key::F12, Action::ClearSel),
            (c, Key::F1, Action::SetClasses),
            (Modifiers::SHIFT | Modifiers::CTRL, Key::O, Action::OpenSet),
            (Modifiers::SHIFT | Modifiers::CTRL, Key::S, Action::ExportAms),
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
            // Shift+Q / Shift+E: the previous / next specimen folder (its main file) instead of the sub-specimens
            (Modifiers::SHIFT, Key::Q, Action::Prev),
            (Modifiers::SHIFT, Key::E, Action::Next),
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
        // Esc: first everything that is open (dialogs, pop-outs, drop-down menus) closes at once; with nothing open
        // it takes zoomed plots back, and then clears the selection of the pane that was used last
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if egui::Popup::is_any_open(ctx) || self.any_dialog_open() {
                // an open menu closes by itself when the key is left alone
                self.close_all_dialogs();
                return out;
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                if self.st_view.is_zoomed() || self.views.iter().any(|v| v.is_zoomed()) {
                    self.st_view.reset();
                    self.views.iter_mut().for_each(|v| v.reset());
                } else {
                    self.esc_clear_selection();
                }
            }
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

/// The dark theme follows TX Studio: a charcoal with a faint blue cast, never black.
pub fn apply_style(ctx: &egui::Context, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let accent = if dark { Color32::from_rgb(106, 160, 255) } else { Color32::from_rgb(59, 130, 246) };
    v.selection.bg_fill = if dark { Color32::from_rgb(34, 58, 104) } else { accent.gamma_multiply(0.35) };
    v.selection.stroke = Stroke::new(1.0, accent);
    v.hyperlink_color = accent;
    let r = CornerRadius::same(6);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    if dark {
        let c = |h: u32| Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
        v.panel_fill = c(0x16181d);
        v.window_fill = c(0x171a1f);
        v.window_stroke = Stroke::new(1.0, c(0x333842));
        v.extreme_bg_color = c(0x111317);
        v.faint_bg_color = c(0x1b1e24);
        v.code_bg_color = c(0x1e2228);
        v.widgets.noninteractive.bg_fill = c(0x171a1f);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c(0x262a31));
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c(0xd9dce1));
        v.widgets.inactive.bg_fill = c(0x1e2228);
        v.widgets.inactive.weak_bg_fill = c(0x1e2228);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, c(0x2b3039));
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, c(0xd9dce1));
        v.widgets.hovered.bg_fill = c(0x262a31);
        v.widgets.hovered.weak_bg_fill = c(0x262a31);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, c(0x36508a));
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, c(0xf0f2f5));
        v.widgets.active.bg_fill = c(0x333842);
        v.widgets.active.weak_bg_fill = c(0x333842);
        v.widgets.active.bg_stroke = Stroke::new(1.0, accent);
        v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        v.widgets.open.bg_fill = c(0x262a31);
        v.widgets.open.weak_bg_fill = c(0x262a31);
        v.popup_shadow.color = Color32::from_black_alpha(90);
        v.window_shadow.color = Color32::from_black_alpha(90);
    } else {
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
        // tooltips (the (i) explanations) appear the moment the pointer is over their target, also while it still moves
        if ctx.global_style().interaction.tooltip_delay != 0.0 || ctx.global_style().interaction.show_tooltips_only_when_still {
            ctx.all_styles_mut(|st| {
                st.interaction.tooltip_delay = 0.0;
                st.interaction.show_tooltips_only_when_still = false;
            });
        }
        // drop-down lists show all their entries when the screen has the room (no scrollbar)
        let combo_h = (ctx.content_rect().height() - 90.0).max(200.0);
        if (ctx.global_style().spacing.combo_height - combo_h).abs() > 1.0 {
            ctx.all_styles_mut(|st| st.spacing.combo_height = combo_h);
        }
        self.track_layout(&ctx);
        if self.reset_snap.is_some() && self.reset_undo_left().is_none() {
            self.reset_snap = None; // the ten minutes are over
        }
        self.guard_quit(&ctx);
        self.refresh();
        let mut actions = self.shortcuts(&ctx);
        self.handle_screenshot(&ctx);

        let has = self.doc.is_some();
        let th = self.theme();

        // window title like the original: Anisoft - [path (n / total)]
        let title = match &self.doc {
            Some(d) if d.synthetic => format!("AMS Studio - [{}]", d.path.display()),
            Some(d) => {
                let (_, _, pos, total) = self.neighbours_of(&d.path);
                format!("AMS Studio - [{}  ({pos} / {total})]{}", d.path.display(), if d.modified { " *" } else { "" })
            }
            None => "AMS Studio".to_string(),
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        // ------------------------------------------------------------ menu bar
        self.export_menu_open = false;
        egui::Panel::top("menubar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if item(ui, "Open Data File(s)", "Ctrl+O", true) { actions.push(Action::Open) }
                    if item(ui, "Open Folder of Specimens", "Ctrl+Shift+O", true) { actions.push(Action::OpenSet) }
                    if item(ui, "Open Previous Data File", "Ctrl+P", has) { actions.push(Action::Prev) }
                    if item(ui, "Open Next Data File", "Ctrl+N", has) { actions.push(Action::Next) }
                    if item(ui, "Reload Current Data File(s)", "Ctrl+R", has) { actions.push(Action::Reload) }
                    ui.separator();
                    if item(ui, "Import Data File(s)", "Ctrl+I", true) { actions.push(Action::Import) }
                    ui.separator();
                    if item(ui, "Save Active Records As", "Ctrl+S", has) { actions.push(Action::SaveActive) }
                    if item(ui, "Append Selected Records Into", "Ctrl+A", has) { actions.push(Action::AppendSel) }
                    if item(ui, "Save as .ams (selected or edited files) ...", "Ctrl+Shift+S", has) { actions.push(Action::ExportAms) }
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
                    if item(ui, "Spec to Geo (Owens)", "", has) { actions.push(Action::Owens) }
                    if item(ui, "Drill Core to Geo (cubes) ...", "", has) { actions.push(Action::CoreGeo) }
                    {
                        // hovering shows the volume that is already applied to this file
                        let hint = if has { self.volume_summary() } else { String::new() };
                        let r = ui.add_enabled(has, egui::Button::new("Volume Correction ..."));
                        let r = if hint.is_empty() { r } else { r.on_hover_text(hint) };
                        if r.clicked() {
                            actions.push(Action::Volume);
                            ui.close();
                        }
                    }
                    if item(ui, "Recombine Split AMS Files ...", "", true) { actions.push(Action::Recombine) }
                    ui.separator();
                    if item(ui, "Invert Selection", "Shift+Ins", has) { actions.push(Action::InvertSel) }
                    if item(ui, "Disregard Selected Record(s)", "Shift+Del", has) { actions.push(Action::Disregard) }
                    if item(ui, "Restore All Records", "Alt+Bksp", has) { actions.push(Action::RestoreAll) }
                    if item(ui, "Clear Selection", "Shift+Ctrl+F12", has) { actions.push(Action::ClearSel) }
                });
                ui.menu_button("Graphics", |ui| {
                    if item(ui, "Copy Graphics To Clipboard", "Ctrl+C", has) { actions.push(Action::CopyGraphics) }
                    if item(ui, "Save Graphics To File", "Ctrl+F", has) { actions.push(Action::SaveGraphics) }
                    ui.separator();
                    // which panes Copy / Save take; the panes are outlined in the plots while this is open
                    let names = self.pane_names();
                    let cfg = egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                    let (_, sub) = egui::containers::menu::SubMenuButton::new("Export graphics: panes").config(cfg).ui(ui, |ui| {
                        ui.label(RichText::new("Panes that Copy / Save Graphics take").weak().small());
                        for i in 0..4 {
                            ui.checkbox(&mut self.export[i], names[i].as_str());
                        }
                    });
                    self.export_menu_open = sub.is_some();
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
                    ui.separator();
                    // the four panes back to the Anisoft 6 settings; the undo sits in the same row while it is offered
                    let undo_left = self.reset_undo_left();
                    ui.horizontal(|ui| {
                        if ui.button("Reset all panes to default").on_hover_text("The stereonet and the three plots go back to their Anisoft 6 settings (the open file's specimen-specific settings too)").clicked() {
                            actions.push(Action::ResetPanes);
                            ui.close();
                        }
                        if let Some(left) = undo_left {
                            let mins = (left + 59) / 60;
                            if ui.button("Undo reset").on_hover_text(format!("Takes the last reset back; offered for {mins} more minute(s)")).clicked() {
                                actions.push(Action::UndoResetPanes);
                                ui.close();
                            }
                        }
                    });
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
            let (prev, next) = self.doc.as_ref().filter(|dc| !dc.synthetic).map(|dc| { let n = self.neighbours_of(&dc.path); (n.0.is_some(), n.1.is_some()) }).unwrap_or((false, false));
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
                tool(ui, Icon::PaleoH, "Paleo-Horizontal: unavailable, the original's paleo / tectonic rules are only partly decoded (see docs/ORIGINAL_BEHAVIOUR.md, section 13)", false, false, d);
                tool(ui, Icon::PaleoV, "Paleo-Vertical: unavailable, the original's paleo / tectonic rules are only partly decoded (see docs/ORIGINAL_BEHAVIOUR.md, section 13)", false, false, d);
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
                    ui.label(RichText::new("").small()).on_hover_text("General colour-coding parameter (all plots; a plot with its own colour-coding keeps it)");
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
        let mut browser_rect = Rect::NOTHING;
        if self.browser_open {
            let r = egui::Panel::left("browser").resizable(true).default_size(self.browser_w).min_size(150.0).max_size(520.0).show(ui, |ui| {
                self.browser_ui(ui, &mut actions);
            });
            browser_rect = r.response.rect;
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
        self.tree_rect = browser_rect;
        let spec_rect = r.response.rect;
        let w = r.response.rect.width();
        if (w - self.spec_w).abs() > 0.5 {
            self.spec_w = w;
            self.layout_changed = Some(ui.input(|i| i.time));
        }

        // ---------------------------------------------------------------- plots
        let plots_rect = egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(6.0)).show(ui, |ui| {
            self.plots_ui(ui, &th);
        }).response.rect;

        // which pane was clicked last (Esc clears the selection of that one)
        if let Some(p) = ctx.input(|i| if i.pointer.primary_pressed() { i.pointer.press_origin() } else { None }) {
            if ctx.layer_id_at(p).map_or(true, |l| l.order == egui::Order::Background) {
                self.last_focus = if browser_rect.contains(p) {
                    Focus::Files
                } else if spec_rect.contains(p) {
                    Focus::Specimens
                } else if plots_rect.contains(p) {
                    Focus::Graphs
                } else {
                    self.last_focus
                };
            }
        }

        // -------------------------------------------------------------- windows
        self.windows(&ctx);
        self.quit_window(&ctx);
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
                let (p, n, _, _) = self.neighbours_of(&d.path);
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
        if self.set.is_some() {
            self.set_browser_ui(ui);
            if back {
                if let Some(p) = self.browser_back.pop() {
                    self.browser_dir = p;
                }
            } else if let Some(p) = go {
                self.set = None;
                self.browser_dir = p;
            }
            return;
        }
        ui.strong("Files");
        if ui.add_sized([ui.available_width(), 22.0], egui::Button::new(RichText::new("Open folder of specimens ...").small())).on_hover_text("Choose a parent folder with one sub-folder per specimen (Ctrl+Shift+O)").clicked() {
            actions.push(Action::OpenSet);
        }
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
            ui.label(RichText::new("General colour-coding: every plot and this list").small().weak()).on_hover_text("A UI-wide setting. Plots that have their own colour-coding (their options) keep it.");
            color_ui(ui, "metric_panel", &mut self.color_master, dark, "Color-coding (all plots)");
            if before != self.color_master {
                self.apply_master_color();
            }
        }
        ui.separator();
        let row_cols: Vec<Option<Color32>> = match &self.doc {
            Some(d) if self.color_master.on => {
                let th = self.theme();
                let (cp, _) = Self::colorized(&self.points(), &self.color_master, &self.class_colors());
                let mut v = vec![None; d.specs.len()];
                for p in cp {
                    v[p.idx] = p.color.map(|c| th.soft(c));
                }
                v
            }
            _ => vec![],
        };
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
                if let Some(c) = row_cols.get(i).copied().flatten() {
                    // colour-coding of the list: a chip at the left edge of the row
                    let chip = egui::Rect::from_min_max(rect.min + Vec2::new(1.0, 2.0), egui::Pos2::new(rect.min.x + 6.0, rect.max.y - 2.0));
                    ui.painter().rect_filled(chip, 2.0, if doc.active[i] { c } else { c.gamma_multiply(0.35) });
                }
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
        let names = self.pane_names();
        let class_cols = self.class_colors();
        let mut overlay_gs: Option<ams_core::stats::GroupStats> = None;
        if self.st_overlay.mean || self.st_overlay.boot {
            let idx: Vec<usize> = pts.iter().map(|p| p.idx).collect();
            let tensors: Vec<_> = idx.iter().map(|&i| self.derived[i].tensor).collect();
            let kms: Vec<f64> = idx.iter().map(|&i| self.derived[i].km).collect();
            overlay_gs = ams_core::stats::group_stats(&tensors, &kms);
        }
        for i in 0..4 {
            let mut body = self.pane_frame(ui, rects[i], i, &names[i], th);
            // the settings of the three plot panes (the stereonet has none)
            let cfg: Option<PaneCfg> = (i >= 1).then(|| self.cfg(i - 1).clone());
            let coloured = i == 0 || cfg.as_ref().map_or(false, |c| matches!(c.kind, XyKind::Scatter | XyKind::Line));
            let (cpts, crange) = if coloured { Self::colorized(&pts, &self.color[i], &class_cols) } else { (pts.clone(), None) };
            let show_bar = coloured && crange.is_some() && (i == 0 || cfg.as_ref().map_or(false, |c| c.legend));
            if show_bar {
                let bar = Rect::from_min_max(Pos2::new(body.min.x, body.max.y - 30.0), body.max);
                body.max.y -= 30.0;
                plots::color_bar(ui, bar, &self.color[i], crange.unwrap(), th);
            }
            let out = match i {
                0 => {
                    let o = StereoOpts { show: self.show_k, size: self.marker, data: self.st_data, caption: self.st_caption, legend: self.st_legend, connect: self.st_connect, foliation: self.st_mf };
                    let out = plots::stereonet(ui, body, &cpts, &o, th, cs_label, &mut self.st_view);
                    if let Some(gs) = &overlay_gs {
                        let (c, r) = plots::net_geometry(body, &self.st_view);
                        let pa = ui.painter_at(body);
                        analysis_ui::draw_stereo_overlay(&pa, c, r, gs, &self.st_overlay, self.show_k, self.k_colors);
                    }
                    out
                }
                _ => {
                    let c = cfg.unwrap_or_else(|| PaneCfg::default_for(0, false));
                    let k = i - 1;
                    // "Selection only": just the sub-specimens selected in the specimen list
                    let shown: Vec<Pt> = if c.sel_only { cpts.iter().filter(|p| p.selected).cloned().collect() } else { cpts.clone() };
                    const SALT: [&str; 3] = ["xy_a", "xy_b", "xy_c"];
                    const SALT_BIN: [&str; 3] = ["bn_a", "bn_b", "bn_c"];
                    match c.kind {
                        XyKind::Histogram => plots::histogram(ui, body, &shown, c.bins, &c.hist, th, SALT[k]),
                        XyKind::Scatter | XyKind::Line => {
                            let line = c.kind == XyKind::Line;
                            let spec = ScatterSpec { x: &c.x, y: &c.y, zero_line: c.zero, diagonal: c.diag, conf: c.conf, connect: c.connect || line, sort_x: line, salt: SALT[k] };
                            plots::scatter(ui, body, &shown, &spec, self.marker, th, &mut self.views[k])
                        }
                        kind => plots::binned_plot(ui, body, &shown, kind, &c.x, &c.y, c.bins, c.group, th, &mut self.views[k], SALT_BIN[k]),
                    }
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
        if band.is_some() {
            self.last_band = true;
        } else if click.is_some() {
            self.last_band = false;
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
                let tag = |n: &str| -> &'static str { Box::leak(n.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect::<String>().into_boxed_str()) };
                job.rects = names.iter().zip(rects).enumerate().filter(|(i, _)| self.export[*i]).map(|(_, (n, r))| (tag(n), r)).collect();
            }
        }
        // while Graphics > Export graphics is open, the panes that would be exported are outlined
        if self.export_menu_open {
            for i in 0..4 {
                if self.export[i] {
                    ui.painter().rect(rects[i], 8.0, th.accent.gamma_multiply(0.13), Stroke::new(3.0, th.accent), StrokeKind::Inside);
                }
            }
        }
    }

    /// Draws a pane card with its header (collapse arrow, hover text, "Export graphics") and returns the plot rect.
    fn pane_frame(&mut self, ui: &mut Ui, rect: Rect, i: usize, name: &str, th: &Theme) -> Rect {
        let visuals = ui.visuals().clone();
        ui.painter().rect(rect, 8.0, if th.dark { th.canvas() } else { visuals.extreme_bg_color }, Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
        let header = Rect::from_min_size(rect.min + Vec2::new(6.0, 4.0), Vec2::new(rect.width() - 12.0, 24.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(header).layout(Layout::left_to_right(Align::Center)));
        let arrow = if self.pane_settings[i] { "▾" } else { "▸" };
        let arrow_resp = child.small_button(arrow).on_hover_text("Plot options");
        self.arrow_rect[i] = arrow_resp.rect;
        if arrow_resp.clicked() {
            self.pane_settings[i] = !self.pane_settings[i];
        }
        let txt = if self.hover[i].is_empty() { RichText::new(name).weak() } else { RichText::new(&self.hover[i]) };
        // the title / hover text gives way to the tick boxes on the right
        let c0 = (i >= 1).then(|| self.cfg(i - 1).clone());
        let has_p = c0.as_ref().map_or(false, |c| [&c.x, &c.y, &c.hist].iter().any(|a| a.param == 3 || a.param == 4));
        let has_log = i == 2 && c0.as_ref().map_or(false, |c| matches!(c.kind, XyKind::Scatter | XyKind::Line));
        let reserved = if i == 0 { 0.0 } else { 118.0 + if has_p { 34.0 } else { 0.0 } + if has_log { 78.0 } else { 0.0 } };
        child.add_sized([(header.width() - reserved - 34.0).max(40.0), 18.0], egui::Label::new(txt).truncate());
        let mut changed = false;
        let mut p_changed = false;
        let mut cfg_changed = false;
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(c) = &c0 {
                let mut sel = c.sel_only;
                if ui.checkbox(&mut sel, "Selection only").on_hover_text("Show only the sub-specimens that are selected in the specimen list").changed() {
                    self.cfg_mut(i - 1).sel_only = sel;
                    cfg_changed = true;
                }
                if has_p {
                    p_changed |= ui.checkbox(&mut self.use_p, "P").on_hover_text("Use P instead of Pj (the default) wherever a plot shows it").changed();
                }
                if has_log {
                    let lab = if c.x.param == 0 { "Log Km" } else { "Log X" };
                    let mut lg = c.x.log;
                    if ui.checkbox(&mut lg, lab).on_hover_text("Logarithmic X axis").changed() {
                        self.cfg_mut(i - 1).x.log = lg;
                        cfg_changed = true;
                    }
                }
            }
        });
        if p_changed {
            self.apply_p_choice();
            cfg_changed = true;
            changed = true;
        }
        if cfg_changed {
            self.save_plots();
        }
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
    fn pane_options(&mut self, ctx: &egui::Context, _rect: Rect, header: Rect, i: usize) {
        let dark = self.dark;
        let marker_before = self.marker;
        let screen = ctx.content_rect();
        // as tall as the screen allows: the area slides up when it would run off the bottom, so no scrollbar appears while there is room
        let max_h = (screen.height() - 16.0).max(160.0);
        let mut close = false;
        // the stereonet's options stay narrow; the plot panes get a wide pop-out with X and Y side by side. It starts at
        // the arrow that opened it and slides to the left when there is no room on the right.
        let w = if i == 0 { 300.0 } else { 580.0f32.min(screen.width() - 16.0) };
        let arrow = self.arrow_rect[i];
        let x = (arrow.min.x - 12.0).min(screen.max.x - w - 8.0).max(screen.min.x + 8.0);
        // below the header when it fits, else above it (never over the arrow that opened it); the height is the one it had last time
        let h_est = if self.popup_h[i] > 0.0 { self.popup_h[i] } else { 430.0 };
        let below_y = header.max.y + 12.0;
        let above_y = header.min.y - 12.0 - h_est;
        let (py, dir) = if below_y + h_est <= screen.max.y - 8.0 {
            (below_y, 1)
        } else if above_y >= screen.min.y + 8.0 {
            (above_y, -1)
        } else {
            (below_y.min((screen.max.y - h_est - 8.0).max(screen.min.y + 8.0)), 0)
        };
        let mut popup_h = 0.0f32;
        let mut pane_edit: Option<(PaneCfg, PaneCfg, bool, bool, bool)> = None;
        egui::Area::new(egui::Id::unique(("pane-options", i))).order(egui::Order::Foreground).fixed_pos(Pos2::new(x, py)).show(ctx, |ui| {
            let framed = egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(w - 18.0);
                // the area only offers the height it had last time; the pop-out may use what the screen has
                ui.set_max_height(max_h);
                egui::ScrollArea::both().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                    if i != 0 {
                        ui.set_min_width(520.0);
                    }
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
                            ui.add_enabled(false, egui::Checkbox::new(&mut false, "Foliation / Lineation (display rules not decoded yet)"));
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.st_caption, "Caption");
                                ui.checkbox(&mut self.st_legend, "Legend");
                            });
                            ui.separator();
                            self.pane_color_ui(ui, 0, "st-color");
                        }
                        _ => {
                            // every plot pane: any plot type, any X / Y parameter
                            let k = i - 1;
                            let orig = self.cfg(k).clone();
                            let mut c = orig.clone();
                            let specific = self.is_specific(k);
                            let mut spec_tick = specific;
                            let mut reset = false;
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Plot type").strong());
                                egui::ComboBox::from_id_salt(("xy-kind", i)).selected_text(c.kind.name()).width(290.0).show_ui(ui, |ui| {
                                    for kd in XyKind::ALL {
                                        ui.selectable_value(&mut c.kind, kd, kd.name());
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.add_enabled(self.doc.as_ref().map_or(false, |d| !d.synthetic), egui::Checkbox::new(&mut spec_tick, "Specimen specific"))
                                    .on_hover_text("Keep this plot's settings for the open file only. Any other file shows the general settings again; untick to forget the file's own.");
                                reset = ui.button("Reset default").on_hover_text(match k { 0 => "Back to the Anisoft 6 plot of this pane: N vs Kmean (histogram)", 1 => "Back to the Anisoft 6 plot of this pane: P vs Kmean (Pj, or P when the P tick is set)", _ => "Back to the Anisoft 6 plot of this pane: T vs P (Pj, or P when the P tick is set)" }).clicked();
                            });
                            ui.separator();
                            if c.kind == XyKind::Histogram {
                                ui.label(RichText::new("Data").strong());
                                param_combo(ui, &format!("hist-param{i}"), &mut c.hist.param, dark);
                                let h = &mut c.hist;
                                range_fields(ui, &mut h.auto, &mut h.min, &mut h.max, &mut h.log, &format!("hist{i}"));
                                ui.add(egui::Slider::new(&mut c.bins, 3..=60).text("bins"));
                            } else {
                                let grouped = matches!(c.kind, XyKind::Bar | XyKind::Box);
                                if grouped {
                                    // a bar or box plot needs to know what forms its groups
                                    ui.label(RichText::new("Group by").strong());
                                    ui.horizontal_wrapped(|ui| {
                                        ui.radio_value(&mut c.group, GroupBy::Classes, "Classes of the X parameter");
                                        ui.add_enabled_ui(c.group == GroupBy::Classes, |ui| {
                                            ui.add(egui::DragValue::new(&mut c.bins).range(2..=40).speed(0.2).suffix(" classes"));
                                        });
                                        ui.radio_value(&mut c.group, GroupBy::Sample, "Sample (specimen name)").on_hover_text("One group per sample: the sub-specimens ZZ01-A1, ZZ01-B1 ... form the group ZZ01");
                                    });
                                    ui.separator();
                                }
                                ui.columns(2, |cols| {
                                    let by_sample = grouped && c.group == GroupBy::Sample;
                                    cols[0].add_enabled_ui(!by_sample, |ui| axis_ui(ui, if grouped { "X-axis (grouped)" } else { "X-axis" }, &format!("xy-x{i}"), &mut c.x, dark));
                                    axis_ui(&mut cols[1], "Y-axis", &format!("xy-y{i}"), &mut c.y, dark);
                                });
                                match c.kind {
                                    XyKind::Bar | XyKind::Box => {}
                                    _ => {
                                        ui.separator();
                                        ui.horizontal_wrapped(|ui| {
                                            ui.checkbox(&mut c.diag, "Diagonal line");
                                            ui.checkbox(&mut c.zero, "Zero line").on_hover_text("A horizontal line at Y = 0");
                                            ui.checkbox(&mut c.conf, "Conf. intervals").on_hover_text("Error bars (±SD) for K1, K2, K3 on the Y axis");
                                            ui.checkbox(&mut c.connect, "Connect symbols").on_hover_text("Join the points in file order");
                                            ui.checkbox(&mut c.legend, "Legend").on_hover_text("Show the colour scale below the plot");
                                        });
                                        ui.separator();
                                        self.pane_color_ui(ui, i, &format!("xy-color{i}"));
                                    }
                                }
                            }
                            pane_edit = Some((orig, c, spec_tick, specific, reset));
                        }
                    }
                });
            });
            // the tear that ties the pop-out to the arrow that opened it
            let r = framed.response.rect;
            popup_h = r.height();
            if dir != 0 {
                let bx = arrow.center().x.clamp(r.min.x + 16.0, r.max.x - 16.0);
                let (fill, stroke) = (ui.visuals().window_fill(), ui.visuals().window_stroke());
                let pa = ui.painter();
                let (base, sgn) = if dir > 0 { (r.min.y, -1.0f32) } else { (r.max.y, 1.0f32) };
                let tip = Pos2::new(bx, base + sgn * 11.0);
                pa.add(egui::Shape::convex_polygon(vec![tip, Pos2::new(bx - 5.0, base + sgn * 3.5), Pos2::new(bx + 5.0, base + sgn * 3.5)], fill, Stroke::new(1.0, stroke.color)));
                pa.circle(Pos2::new(bx, base + sgn * 2.5), 5.0, fill, Stroke::new(1.0, stroke.color));
                pa.circle_filled(Pos2::new(bx, base + sgn * 1.0), 5.5, fill);
                pa.circle_filled(tip - Vec2::new(0.0, sgn), 1.5, stroke.color);
            }
        });
        if popup_h > 0.0 {
            if (popup_h - self.popup_h[i]).abs() > 1.0 {
                // the content changed size: lay it out again at once, with the new height deciding the place
                ctx.request_repaint();
            }
            self.popup_h[i] = popup_h;
        }
        if let Some((orig, mut c, tick, was, reset)) = pane_edit {
            let k = i - 1;
            if tick != was {
                self.set_specific(k, tick);
            }
            if reset {
                c = PaneCfg::default_for(k, self.use_p);
                self.views[k].reset();
            }
            if (tick || !was) && (c != orig || reset) {
                *self.cfg_mut(k) = c;
                self.views[k].reset();
                self.save_plots();
            }
        }
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
        self.stash_current();
        self.doc = Some(d);
        self.classes.clear();
        self.class_of.clear();
        self.load_volume_info();
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
            W_OWENS => {
                self.reset_owens_file();
                self.win_owens = true;
            }
            W_CLASSES => self.win_classes = true,
            W_SET => self.win_set = true,
            W_VOL => self.win_vol = true,
            W_CORE => self.win_core = true,
            W_RECOMB => self.win_recomb = true,
            W_EXPORT => self.win_export = true,
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
                for (n, v, r, tip) in [
                    ("Core azimuth", &mut orient.azimuth, 0.0..=360.0, "Compass direction (0-360 deg) the core is drilled towards. With a core dip of 0 every declination gains this value."),
                    ("Core dip", &mut orient.dip, -90.0..=90.0, "Tilt of the core axis from the vertical, in degrees: the specimen is tilted by this much about its x axis before the azimuth is added."),
                    ("Bedding strike", &mut orient.bed_strike, 0.0..=360.0, "Strike of the bedding (right-hand rule: it dips to the right of the strike direction), used for tectonic coordinates."),
                    ("Bedding dip", &mut orient.bed_dip, 0.0..=90.0, "Dip of the bedding below the horizontal (0-90 deg), restored to horizontal in tectonic coordinates."),
                    ("Paleo declination", &mut orient.paleo_dec, -360.0..=360.0, "Paleo-declination in degrees; paleogeographic coordinates turn the data by minus this value about the vertical."),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(n);
                        info_icon(ui, tip);
                    });
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

        self.specset_window(ctx);
        self.edit_data_window(ctx);
        self.classes_window(ctx);
        self.poll_restore(ctx);
        self.export_window(ctx);
        self.core_window(ctx);
        self.recombine_window(ctx);
        self.volume_window(ctx);
        self.owens_window(ctx);
        self.table_window(ctx);
        self.group_window(ctx);
        self.contour_window(ctx);
        self.tool_windows(ctx);
    }

    /// Edit > Spec to Geo (Owens): strike/dip of a plane in the field and of the same plane on the drilling
    /// table. Classic (default): the per-row table of Anisoft 4.2. New: one set of four values for a range of rows
    /// (the whole file by default). Rotates the data of the open file (cumulatively, like the original).
    fn owens_window(&mut self, ctx: &egui::Context) {
        if !self.win_owens {
            return;
        }
        if self.doc.is_none() {
            self.close_owens();
            return;
        }
        if self.of.stage != owens_file_ui::Stage::Manual {
            self.owens_file_window(ctx);
            return;
        }
        let doc = self.doc.as_ref().unwrap();
        let n = doc.specs.len();
        if self.owens.len() != n {
            self.owens = vec![[0.0; 4]; n];
        }
        let out_phase = self.out_phase;
        let dark = self.dark;
        let preview: Vec<(String, f64, [(f64, f64); 3])> = doc
            .specs
            .iter()
            .map(|s| {
                let ph = if out_phase { &s.outphase } else { &s.inphase };
                (s.name.clone(), ph.km, ph.axes().dec_inc)
            })
            .collect();
        let mut rows = std::mem::take(&mut self.owens);
        let mut one = self.owens_one;
        let (mut first, mut last) = self.owens_range;
        first = first.clamp(1, n.max(1));
        last = last.clamp(first, n.max(1));
        let mut old = self.owens_old;
        let can_undo = self.owens_undo.is_some();
        let now = ctx.input(|i| i.time);
        let toast = self.owens_toast.clone();
        let (mut open, mut copy, mut rotate, mut undo, mut cancel) = (true, false, false, false, false);
        let mut switched = false;
        let (mut from_file, mut undo_file) = (false, false);
        let file_last = self.of.last;
        let accent = if dark { Color32::from_rgb(106, 160, 255) } else { Color32::from_rgb(47, 111, 222) };
        const TIPS: [&str; 4] = [
            "FS: the strike you measured in the field.\nMeasured with the compass on a marked reference plane (bedding, a vein, a foliation) before the sample was taken out, 0-360 deg. Index finger along the strike, thumb towards the dip direction.",
            "FD: the dip you measured in the field.\nThe dip of that same marked plane in the field before sampling: the angle down from the horizontal, 0-90 deg.",
            "DS: the strike on the drilling table.\nThe strike of that same marked plane measured on the hand sample while it lay on the drilling table, before the core was drilled out. Same convention as FS.",
            "DD: the dip on the drilling table.\nThe dip of that same marked plane on the hand sample on the drilling table, measured before drilling: the angle down from the horizontal, 0-90 deg.",
        ];
        self.base_window(ctx, "Specimen to Geographic Rotation (Owens' method)", W_OWENS, Vec2::new(700.0, 780.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.3);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

            // ---- mode switch (top left) and a short notice on the right
            if owens_head(ui, &mut old, &toast, now, accent, false, true).0 {
                switched = true;
            }
            ui.separator();
            // ---- the values can also come from a spreadsheet / csv (second row, always the same place)
            ui.horizontal(|ui| {
                from_file = ui.add_sized([210.0, 26.0], egui::Button::new("From file ...")).on_hover_text("Read FS, FD, DS and DD of many specimens from an .xlsx or .csv").clicked();
                if let Some(l) = file_last {
                    undo_file = ui.add(egui::Button::new(RichText::new(format!("Undo last correction ({})", if l == owens_file_ui::Scope::Single { "single" } else { "multiple" })).small()).small().fill(Color32::TRANSPARENT).stroke(ui.visuals().widgets.inactive.bg_stroke)).clicked();
                }
            });
            ui.add_space(2.0);

            let preview_h = 220.0;
            if !old {
                // ---- new: four values, then the rows they apply to
                ui.columns(2, |cols| {
                    for (c, (title, ks)) in [("Plane in the field", [0usize, 1]), ("Same plane on the drilling table", [2, 3])].into_iter().enumerate() {
                        egui::Frame::group(cols[c].style()).show(&mut cols[c], |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(title).strong());
                            for k in ks {
                                ui.horizontal(|ui| {
                                    ui.add_sized([34.0, 20.0], egui::Label::new(RichText::new(["FS", "FD", "DS", "DD"][k]).strong()));
                                    ui.add(angle_drag(&mut one[k], k));
                                    info_icon(ui, TIPS[k]);
                                });
                            }
                        });
                    }
                });
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Rotate rows").strong());
                        info_icon(ui, "The rows the four values are applied to, numbered as in the list at the bottom. Preset to the whole file, i.e. all of its sub-specimens.");
                        ui.add_space(10.0);
                        ui.label("from the first row");
                        ui.add(egui::DragValue::new(&mut first).range(1..=n.max(1)).speed(0.2));
                        info_icon(ui, "First row: the first row the rotation is applied to. Preset to row 1, the first row of the file.");
                        ui.add_space(6.0);
                        ui.label("to the last row");
                        ui.add(egui::DragValue::new(&mut last).range(first..=n.max(1)).speed(0.2));
                        info_icon(ui, "Last row: the last row the rotation is applied to. Preset to the last row of the file, so the whole file is rotated. Narrow the range to rotate only part of it.");
                    });
                    ui.label(RichText::new(format!("Row {first} (first of {n}) to row {last} (last is {n}): {} of {n} rows will be rotated.", last - first + 1)).weak().small());
                });
            } else {
                // ---- classic: one row of values per specimen
                let h = (ui.available_height() - preview_h - 90.0).max(110.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().id_salt("owens-table").max_height(h).auto_shrink([false, true]).show(ui, |ui| {
                        egui::Grid::new("owens-grid").striped(true).num_columns(6).min_col_width(96.0).spacing([14.0, 6.0]).show(ui, |ui| {
                            ui.label(RichText::new("#").strong());
                            ui.label(RichText::new("Name").strong());
                            for (k, hd) in ["FS", "FD", "DS", "DD"].iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(*hd).strong());
                                    info_icon(ui, TIPS[k]);
                                });
                            }
                            ui.end_row();
                            for (i, row) in rows.iter_mut().enumerate() {
                                ui.label(RichText::new((i + 1).to_string()).weak());
                                ui.label(&preview[i].0);
                                for (k, v) in row.iter_mut().enumerate() {
                                    ui.add(angle_drag(v, k));
                                }
                                ui.end_row();
                            }
                        });
                    });
                });
            }

            // ---- actions: Rotate (bold, filled) and a small Undo on the left, Cancel on the right
            ui.horizontal(|ui| {
                let go = egui::Button::new(RichText::new("Rotate").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(110.0, 0.0));
                rotate = ui.add(go).on_hover_text("Rotate the data of the open file").clicked();
                let u = egui::Button::new(RichText::new("Undo last rotation").small()).small().fill(Color32::TRANSPARENT).stroke(ui.visuals().widgets.inactive.bg_stroke);
                undo = ui.add_enabled(can_undo, u).clicked();
                if old {
                    copy = ui.add(egui::Button::new(RichText::new("Copy 1st row to all").small()).small().fill(Color32::TRANSPARENT).stroke(ui.visuals().widgets.inactive.bg_stroke)).clicked();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                });
            });
            ui.label(RichText::new("The rotation is applied to the data of the open file (both phases) and is cumulative.").weak().small());
            ui.separator();

            // ---- preview of the whole file at the very bottom
            ui.horizontal(|ui| {
                ui.label(RichText::new("Data of this file").strong());
                ui.label(RichText::new(if out_phase { "out-of-phase, specimen coordinates" } else { "in-phase, specimen coordinates" }).weak().small());
            });
            egui::ScrollArea::vertical().id_salt("owens-preview").auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("owens-preview-grid").striped(true).num_columns(6).min_col_width(96.0).spacing([16.0, 4.0]).show(ui, |ui| {
                    for hd in ["#", "Name", "Km", "K1 Dec / Inc", "K2 Dec / Inc", "K3 Dec / Inc"] {
                        ui.label(RichText::new(hd).strong());
                    }
                    ui.end_row();
                    for (i, (name, km, di)) in preview.iter().enumerate() {
                        let on = old || (i + 1 >= first && i + 1 <= last);
                        let t = |s: String| if on { RichText::new(s) } else { RichText::new(s).weak() };
                        ui.label(t((i + 1).to_string()));
                        ui.label(t(name.clone()));
                        ui.label(t(format!("{:.3e}", km)));
                        for (d, inc) in di {
                            ui.label(t(format!("{d:.1} / {inc:.1}")));
                        }
                        ui.end_row();
                    }
                });
            });
        });
        if from_file {
            self.of.stage = owens_file_ui::Stage::Scope;
        }
        if undo_file {
            self.of_undo();
        }
        if switched {
            let msg = if old { "Classic Owens (Anisoft 4.2): one row of values per specimen" } else { "New Owens: one set of values for a range of rows" };
            self.owens_toast = Some((msg.to_string(), now));
        }
        if copy {
            if let Some(first_row) = rows.first().copied() {
                rows.iter_mut().for_each(|r| *r = first_row);
            }
        }
        // what to rotate: (row, values)
        let jobs: Vec<(usize, [f64; 4])> = if old {
            rows.iter().copied().enumerate().filter(|(_, r)| *r != [0.0; 4]).collect()
        } else if one != [0.0; 4] {
            (first - 1..last).map(|i| (i, one)).collect()
        } else {
            vec![]
        };
        let mut store: Option<()> = None;
        if rotate {
            if self.doc.is_some() {
                if !jobs.is_empty() {
                    let mats: Vec<(usize, ams_core::tensor::Mat3)> = jobs.iter().map(|(i, r)| (*i, ams_core::coords::owens_rotation(r[0], r[1], r[2], r[3]))).collect();
                    match self.rotate_doc(&mats) {
                        Ok(()) => self.of.last = None,
                        Err(e) => self.info = Some(e),
                    }
                }
                rows.iter_mut().for_each(|r| *r = [0.0; 4]);
                one = [0.0; 4];
            }
        }
        if undo {
            self.undo_doc_rotation();
            self.of.last = None;
        }
        let _ = &mut store;
        if old != self.owens_old {
            self.owens_old = old;
            self.save_state();
        }
        self.owens = rows;
        self.owens_one = one;
        self.owens_range = (first, last);
        if !(open && !cancel && !rotate) {
            self.close_owens();
        }
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
