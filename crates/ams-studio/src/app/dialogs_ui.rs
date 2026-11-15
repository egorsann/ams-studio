//! Edit Data (per-specimen orientation table), Set Data Classes and Import Data File(s).
//! Behaviour follows Anisoft 6 (see docs/ORIGINAL_BEHAVIOUR.md sections 9, 12 and 13).

use super::{tools_ui, App, W_CLASSES, W_EDIT};
use crate::doc::Doc;
use ams_core::ged::GedFile;
use ams_core::orient::RecOrient;
use egui::{Align, Color32, Layout, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};

/// One row of the Edit Data table.
#[derive(Clone)]
pub(super) struct EdRow {
    pub name: String,
    pub o: RecOrient,
    /// The record carries orientation data (or the user has typed some).
    pub has: bool,
}

#[derive(Default)]
pub(super) struct EditState {
    pub rows: Vec<EdRow>,
    /// A `.ged` that was opened and is waiting to be paired with the records.
    pub ged: Option<(String, GedFile)>,
    /// For every record, the row of the `.ged` it is paired with.
    pub pair: Vec<Option<usize>>,
}

/// A data class: a name and a colour; specimens point at classes by index.
#[derive(Clone, PartialEq)]
pub(super) struct ClassDef {
    pub name: String,
    pub color: [u8; 3],
}

pub(super) struct ClassState {
    /// 0 Custom, 1 File name, 2 Specimen name, 3 Aniso mode
    pub criterion: usize,
    pub picked: [u8; 3],
    pub name: String,
}

impl Default for ClassState {
    fn default() -> Self {
        ClassState { criterion: 0, picked: DEFAULT_COLORS[0].1, name: String::new() }
    }
}

/// The twelve default class colours of the original, in its order.
pub(super) const DEFAULT_COLORS: [(&str, [u8; 3]); 12] = [
    ("Red", [230, 25, 75]),
    ("Blue", [0, 130, 200]),
    ("Green", [60, 180, 75]),
    ("Orange", [245, 130, 48]),
    ("Yellow", [255, 225, 25]),
    ("Purple", [145, 30, 180]),
    ("Cyan", [70, 240, 240]),
    ("Pink", [250, 190, 212]),
    ("Magenta", [240, 50, 230]),
    ("Teal", [0, 128, 128]),
    ("Lime", [210, 245, 60]),
    ("Lavender", [220, 190, 255]),
];

const CRITERIA: [&str; 4] = ["Custom", "File name", "Specimen name", "Aniso mode"];

/// The colour palette grid: 12 columns (11 hues and a grey column), 8 rows from dark to light.
fn palette(col: usize, row: usize) -> [u8; 3] {
    let (v, s) = [(0.28, 1.0), (0.5, 1.0), (0.78, 1.0), (1.0, 1.0), (1.0, 0.72), (1.0, 0.5), (1.0, 0.32), (1.0, 0.18)][row];
    if col == 11 {
        let g = (255.0 * (1.0 - row as f32 / 7.0)) as u8;
        return [g, g, g];
    }
    let h = col as f32 * 330.0 / 10.0 / 60.0;
    let (i, f) = (h.floor() as i32, h - h.floor());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

/// A drag box for an angle in a table; true when it was changed.
fn num_cell(ui: &mut Ui, v: &mut f64, max: f64) -> bool {
    ui.add(egui::DragValue::new(v).range(0.0..=max).speed(0.5).max_decimals(1)).changed()
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

impl App {
    // ------------------------------------------------------------------ Edit Data

    pub(super) fn open_edit_data(&mut self) {
        let Some(d) = &self.doc else { return };
        self.edit.rows = d.specs.iter().map(|s| EdRow { name: s.name.clone(), o: s.orient.clone().unwrap_or_default(), has: s.orient.is_some() }).collect();
        self.edit.ged = None;
        self.edit.pair = vec![];
        self.open_win(W_EDIT);
    }

    /// Orientation of a `.ged` row, with the header's OP values.
    fn ged_orient(g: &GedFile, i: usize) -> RecOrient {
        let r = &g.rows[i];
        RecOrient {
            op: [g.op[0] as i16, g.op[1] as i16, g.op[2] as i16, g.op[3] as i16],
            alpha: r.sampling[0],
            phi: r.sampling[1],
            fol_code: [r.codes[0], r.codes[2]],
            fol: [(r.fol1[0], r.fol1[1]), (r.fol2[0], r.fol2[1])],
            lin_code: [r.codes[1], r.codes[3]],
            lin: [(r.lin1[0], r.lin1[1]), (r.lin2[0], r.lin2[1])],
        }
    }

    pub(super) fn edit_data_window(&mut self, ctx: &egui::Context) {
        if !self.win_edit {
            return;
        }
        if self.doc.is_none() {
            self.win_edit = false;
            return;
        }
        let mut st = std::mem::take(&mut self.edit);
        let n_doc = self.doc.as_ref().map_or(0, |d| d.specs.len());
        if st.rows.len() != n_doc {
            st.rows = self.doc.as_ref().unwrap().specs.iter().map(|s| EdRow { name: s.name.clone(), o: s.orient.clone().unwrap_or_default(), has: s.orient.is_some() }).collect();
        }
        let dark = self.dark;
        let accent = if dark { Color32::from_rgb(106, 160, 255) } else { Color32::from_rgb(47, 111, 222) };
        let (mut open, mut save, mut cancel, mut load, mut clear) = (true, false, false, false, false);
        let mut info: Option<String> = None;
        self.base_window(ctx, "Edit Data", W_EDIT, Vec2::new(1180.0, 640.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.15);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 7.0);
            if let Some((gname, g)) = st.ged.clone() {
                // ---- Record Pairing
                ui.label(RichText::new(format!("Pair the records of the file with the rows of {gname}")).strong());
                ui.label(RichText::new("Full Match pairs by specimen name. Match from Left pairs in order: the first GED row with the first record, and so on.").weak().small());
                ui.horizontal(|ui| {
                    if ui.button("Full Match").clicked() {
                        st.pair = st.rows.iter().map(|r| g.rows.iter().position(|x| x.name.trim().eq_ignore_ascii_case(r.name.trim()))).collect();
                    }
                    if ui.button("Match from Left").clicked() {
                        st.pair = (0..st.rows.len()).map(|i| if i < g.rows.len() { Some(i) } else { None }).collect();
                    }
                    let ready = st.pair.iter().any(|p| p.is_some());
                    if ui.add_enabled(ready, egui::Button::new(RichText::new("Continue").strong())).clicked() {
                        for (r, p) in st.rows.iter_mut().zip(&st.pair) {
                            if let Some(j) = p {
                                r.o = Self::ged_orient(&g, *j);
                                r.has = true;
                            }
                        }
                        st.ged = None;
                        st.pair.clear();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Cancel pairing").clicked() {
                            st.ged = None;
                            st.pair.clear();
                        }
                    });
                });
                if st.pair.len() != st.rows.len() {
                    st.pair = vec![None; st.rows.len()];
                }
                egui::ScrollArea::vertical().max_height(ui.available_height() - 4.0).auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("pair-grid").striped(true).min_col_width(150.0).show(ui, |ui| {
                        ui.label(RichText::new("ANISO records").strong());
                        ui.label(RichText::new("Matching GED record").strong());
                        ui.end_row();
                        for (r, p) in st.rows.iter().zip(&st.pair) {
                            ui.label(&r.name);
                            match p {
                                Some(j) => ui.label(RichText::new(g.rows[*j].name.trim()).color(Color32::from_rgb(70, 170, 90))),
                                None => ui.label(RichText::new("NO MATCH!").color(Color32::from_rgb(220, 90, 90))),
                            };
                            ui.end_row();
                        }
                    });
                });
                return;
            }

            // ---- the table
            ui.label(RichText::new("Orientation of the specimens: sampling angles, foliations, lineations and orientation parameters. They are stored in the records of the open file.").weak().small());
            let h = (ui.available_height() - 58.0).max(120.0);
            egui::ScrollArea::both().max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("edit-grid").striped(true).spacing([8.0, 5.0]).show(ui, |ui| {
                    let heads: [(&str, &str); 20] = [
                        ("#", ""),
                        ("Specimen", "Name of the record (up to 24 characters). It can be renamed here."),
                        ("Alpha", "Sampling azimuth (0-360 deg): the compass direction the sampling arrow of the specimen points to in the field."),
                        ("Phi", "Sampling plunge (0-90 deg): how far that arrow points below the horizontal. With Alpha and the orientation parameters it gives the geographic rotation."),
                        ("Fol 1", "One-character code of foliation #1, as in the GED file."),
                        ("Azi", "Azimuth of foliation #1 (0-360 deg)."),
                        ("Dip", "Dip of foliation #1 (0-90 deg)."),
                        ("Lin 1", "One-character code of lineation #1, as in the GED file."),
                        ("Trend", "Trend of lineation #1 (0-360 deg)."),
                        ("Plunge", "Plunge of lineation #1 (0-90 deg)."),
                        ("Fol 2", "One-character code of foliation #2."),
                        ("Azi", "Azimuth of foliation #2 (0-360 deg)."),
                        ("Dip", "Dip of foliation #2 (0-90 deg)."),
                        ("Lin 2", "One-character code of lineation #2."),
                        ("Trend", "Trend of lineation #2 (0-360 deg)."),
                        ("Plunge", "Plunge of lineation #2 (0-90 deg)."),
                        ("OP1", "Orientation parameter 1: clock position of the sampling arrow (3, 6, 9 or 12); it turns the specimen about its own axis in steps of 90 deg."),
                        ("OP2", "Orientation parameter 2: 0 when Phi is measured from the horizontal, 90 when it is measured from the vertical."),
                        ("OP3", "Orientation parameter 3: second clock position (3, 6, 9 or 12); it turns the result about the vertical in steps of 90 deg."),
                        ("OP4", "Orientation parameter 4: how the foliation angles are given (0 or 90). It does not change the geographic rotation."),
                    ];
                    for (h, tip) in heads {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(h).strong());
                            if !tip.is_empty() {
                                super::info_icon(ui, tip);
                            }
                        });
                    }
                    ui.end_row();
                    for (i, r) in st.rows.iter_mut().enumerate() {
                        let mut touched = false;
                        ui.label(RichText::new((i + 1).to_string()).weak());
                        ui.add(egui::TextEdit::singleline(&mut r.name).desired_width(160.0).char_limit(24));
                        touched |= num_cell(ui, &mut r.o.alpha, 360.0);
                        touched |= num_cell(ui, &mut r.o.phi, 90.0);
                        for k in 0..2 {
                            let mut code = r.o.fol_code[k].to_string();
                            if ui.add(egui::TextEdit::singleline(&mut code).desired_width(18.0).char_limit(1)).changed() {
                                r.o.fol_code[k] = code.chars().next().unwrap_or(' ');
                                touched = true;
                            }
                            touched |= num_cell(ui, &mut r.o.fol[k].0, 360.0);
                            touched |= num_cell(ui, &mut r.o.fol[k].1, 90.0);
                            let mut code = r.o.lin_code[k].to_string();
                            if ui.add(egui::TextEdit::singleline(&mut code).desired_width(18.0).char_limit(1)).changed() {
                                r.o.lin_code[k] = code.chars().next().unwrap_or(' ');
                                touched = true;
                            }
                            touched |= num_cell(ui, &mut r.o.lin[k].0, 360.0);
                            touched |= num_cell(ui, &mut r.o.lin[k].1, 90.0);
                        }
                        for k in 0..4 {
                            let mut v = r.o.op[k] as i64;
                            if ui.add(egui::DragValue::new(&mut v).range(0..=360)).changed() {
                                r.o.op[k] = v as i16;
                                touched = true;
                            }
                        }
                        if touched {
                            r.has = true;
                        }
                        ui.end_row();
                    }
                });
            });
            ui.horizontal(|ui| {
                load = ui.button("Open .ged ...").on_hover_text("Fill the table from a GED file made with the GED Editor (Tools menu)").clicked();
                let go = egui::Button::new(RichText::new("Save").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { accent }).min_size(Vec2::new(100.0, 0.0));
                save = ui.add(go).on_hover_text("Write the table into the records of the open file").clicked();
                clear = ui.add(egui::Button::new(RichText::new("Clear all orientation").small()).small().fill(Color32::TRANSPARENT).stroke(ui.visuals().widgets.inactive.bg_stroke)).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                });
            });
            ui.label(RichText::new("Geographic coordinates (G) use these values; use Save Active Records As afterwards to keep them in a file.").weak().small());
        });
        if load {
            if let Some(p) = rfd::FileDialog::new().add_filter("GED files", &["ged"]).set_title("Open GED File(s)").pick_file() {
                match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| GedFile::parse(&t)) {
                    Ok(g) if !g.rows.is_empty() => {
                        st.pair = vec![None; st.rows.len()];
                        st.ged = Some((p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), g));
                    }
                    Ok(_) => info = Some("That GED file has no rows.".into()),
                    Err(e) => info = Some(format!("Could not read the GED file:\n{e}")),
                }
            }
        }
        if clear {
            for r in st.rows.iter_mut() {
                r.o = RecOrient::default();
                r.has = false;
            }
        }
        if save {
            if let Some(d) = &mut self.doc {
                d.apply_edit(st.rows.iter().map(|r| (r.name.clone(), r.has.then(|| r.o.clone()))).collect());
                self.dirty = true;
            }
            open = false;
        }
        if cancel {
            open = false;
        }
        if info.is_some() {
            self.info = info;
        }
        if !open {
            st.ged = None;
            st.pair.clear();
        }
        self.edit = st;
        self.win_edit = open;
    }

    // ------------------------------------------------------------------ Data classes

    /// Colour of every specimen from its class (index = specimen), for colour-coding.
    pub(super) fn class_colors(&self) -> Vec<Option<Color32>> {
        self.class_of.iter().map(|c| c.and_then(|k| self.classes.get(k)).map(|c| c32(c.color))).collect()
    }

    pub(super) fn open_classes(&mut self) {
        let Some(d) = &self.doc else { return };
        if self.class_of.len() != d.specs.len() {
            self.class_of = vec![None; d.specs.len()];
            self.classes.clear();
        }
        self.open_win(W_CLASSES);
    }

    /// Builds the classes of a non-custom criterion (all specimens get one).
    fn classes_by_criterion(&mut self, criterion: usize) {
        let Some(d) = &self.doc else { return };
        self.classes.clear();
        self.class_of = vec![None; d.specs.len()];
        match criterion {
            1 => {
                let name = d.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                self.classes.push(ClassDef { name, color: DEFAULT_COLORS[0].1 });
                self.class_of.iter_mut().for_each(|c| *c = Some(0));
            }
            2 => {
                let mut names: Vec<String> = d.specs.iter().map(|s| s.name.clone()).collect();
                names.sort();
                names.dedup();
                for (k, n) in names.iter().enumerate() {
                    self.classes.push(ClassDef { name: n.clone(), color: DEFAULT_COLORS[k % DEFAULT_COLORS.len()].1 });
                }
                for (i, s) in d.specs.iter().enumerate() {
                    self.class_of[i] = names.iter().position(|n| *n == s.name);
                }
            }
            3 => {
                // the original shows a single class with an empty name for every file tried
                self.classes.push(ClassDef { name: String::new(), color: DEFAULT_COLORS[0].1 });
                self.class_of.iter_mut().for_each(|c| *c = Some(0));
            }
            _ => {}
        }
    }

    pub(super) fn classes_window(&mut self, ctx: &egui::Context) {
        if !self.win_classes {
            return;
        }
        let Some(d) = &self.doc else {
            self.win_classes = false;
            return;
        };
        let n_sel = d.n_selected();
        let mut st = std::mem::take(&mut self.cl);
        let mut classes = self.classes.clone();
        let (mut open, mut apply, mut clear, mut save) = (true, false, false, false);
        let mut crit_changed = false;
        self.base_window(ctx, "Data Classes", W_CLASSES, Vec2::new(760.0, 560.0)).open(&mut open).resizable(false).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.2);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 7.0);
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.label(RichText::new("Class criterion").strong());
                egui::ComboBox::from_id_salt("cl-crit").width(ui.available_width() - 6.0).selected_text(CRITERIA[st.criterion]).show_ui(ui, |ui| {
                    for (k, n) in CRITERIA.iter().enumerate() {
                        if ui.selectable_value(&mut st.criterion, k, *n).clicked() {
                            crit_changed = true;
                        }
                    }
                });
                ui.label(RichText::new("Default colors").strong());
                egui::ComboBox::from_id_salt("cl-def").width(ui.available_width() - 6.0).selected_text(
                    DEFAULT_COLORS.iter().find(|(_, c)| *c == st.picked).map(|(n, _)| *n).unwrap_or("Custom colour"),
                ).show_ui(ui, |ui| {
                    for (n, c) in DEFAULT_COLORS {
                        if ui.selectable_label(st.picked == c, RichText::new(format!("\u{25a0} {n}")).color(c32(c))).clicked() {
                            st.picked = c;
                        }
                    }
                });
                ui.label(RichText::new("Color palette").strong());
                let cell = 22.0;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(cell * 12.0, cell * 8.0), Sense::click());
                for row in 0..8 {
                    for col in 0..12 {
                        let c = palette(col, row);
                        let r = egui::Rect::from_min_size(rect.min + Vec2::new(col as f32 * cell, row as f32 * cell), Vec2::splat(cell));
                        ui.painter().rect_filled(r, 0.0, c32(c));
                        if c == st.picked {
                            ui.painter().rect_stroke(r.shrink(1.0), 0.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
                        }
                    }
                }
                if resp.clicked() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        let (cx, cy) = (((p.x - rect.min.x) / cell) as usize, ((p.y - rect.min.y) / cell) as usize);
                        if cx < 12 && cy < 8 {
                            st.picked = palette(cx, cy);
                        }
                    }
                }
                ui.horizontal(|ui| {
                    for (k, lab) in ["R", "G", "B"].iter().enumerate() {
                        ui.label(RichText::new(*lab).strong());
                        let mut v = st.picked[k] as i32;
                        if ui.add(egui::DragValue::new(&mut v).range(0..=255)).changed() {
                            st.picked[k] = v as u8;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Picked color");
                    let (r, _) = ui.allocate_exact_size(Vec2::new(120.0, 20.0), Sense::hover());
                    ui.painter().rect_filled(r, 3.0, c32(st.picked));
                });
                ui.horizontal(|ui| {
                    ui.label("Class name");
                    ui.add(egui::TextEdit::singleline(&mut st.name).desired_width(150.0));
                });
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(st.criterion == 0, |ui| {
                        let go = egui::Button::new(RichText::new("Apply").strong()).min_size(Vec2::new(90.0, 0.0));
                        apply = ui.add(go).on_hover_text("Give the selected specimens this class (custom criterion)").clicked();
                    });
                    clear = ui.button("Clear").on_hover_text("Remove all classes").clicked();
                    save = ui.button("Save").on_hover_text("Save the specimen classes to a csv file").clicked();
                });
                if st.criterion != 0 {
                    ui.label(RichText::new("The classes of this criterion are set automatically.").weak().small());
                } else {
                    ui.label(RichText::new(format!("{n_sel} specimen(s) selected in the list.")).weak().small());
                }

                let ui = &mut cols[1];
                ui.label(RichText::new("Data classes").strong());
                egui::ScrollArea::vertical().max_height(470.0).auto_shrink([false, false]).show(ui, |ui| {
                    for c in &classes {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::new(30.0, 18.0), Sense::hover());
                            ui.painter().rect_filled(r, 2.0, c32(c.color));
                            ui.label(if c.name.is_empty() { RichText::new("(no name)").weak() } else { RichText::new(&c.name) });
                        });
                    }
                    if classes.is_empty() {
                        ui.label(RichText::new("No classes yet.\nSelect specimens in the list, pick a colour and a name and press Apply, or choose a criterion.").weak());
                    }
                });
            });
        });
        if crit_changed {
            self.cl = std::mem::take(&mut st);
            let c = self.cl.criterion;
            self.classes_by_criterion(c);
            st = std::mem::take(&mut self.cl);
            classes = self.classes.clone();
        }
        if apply {
            if let Some(d) = &self.doc {
                if n_sel == 0 {
                    self.info = Some("Select some specimens in the list first, then press Apply.".into());
                } else {
                    let def = ClassDef { name: st.name.trim().to_string(), color: st.picked };
                    let k = classes.iter().position(|c| *c == def).unwrap_or_else(|| {
                        classes.push(def);
                        classes.len() - 1
                    });
                    if self.class_of.len() != d.specs.len() {
                        self.class_of = vec![None; d.specs.len()];
                    }
                    for i in 0..d.specs.len() {
                        if d.selected[i] {
                            self.class_of[i] = Some(k);
                        }
                    }
                    // drop classes nobody uses any more
                    let used: Vec<bool> = (0..classes.len()).map(|k| self.class_of.iter().any(|c| *c == Some(k))).collect();
                    let remap: Vec<Option<usize>> = used.iter().scan(0, |n, u| Some(if *u { *n += 1; Some(*n - 1) } else { None })).collect();
                    for c in self.class_of.iter_mut() {
                        *c = c.and_then(|k| remap[k]);
                    }
                    classes = classes.into_iter().zip(used).filter(|(_, u)| *u).map(|(c, _)| c).collect();
                }
            }
        }
        if clear {
            classes.clear();
            self.class_of.iter_mut().for_each(|c| *c = None);
        }
        if save {
            if let Some(d) = &self.doc {
                let stem = d.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("data".into());
                if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name(format!("{stem}_classes.csv")).save_file() {
                    let mut s = String::from("Specimen,Class,R,G,B\n");
                    for (i, sp) in d.specs.iter().enumerate() {
                        match self.class_of.get(i).copied().flatten().and_then(|k| classes.get(k)) {
                            Some(c) => s += &format!("{},{},{},{},{}\n", sp.name, c.name, c.color[0], c.color[1], c.color[2]),
                            None => s += &format!("{},,,,\n", sp.name),
                        }
                    }
                    if let Err(e) = std::fs::write(&p, s) {
                        self.info = Some(format!("Could not save: {e}"));
                    }
                }
            }
        }
        let changed = classes != self.classes || crit_changed;
        self.classes = classes;
        self.cl = st;
        self.win_classes = open;
        if changed && !self.classes.is_empty() {
            // like the original, the list and the plots show the classes straight away
            self.color_master.on = true;
            self.color_master.by_class = true;
            self.apply_master_color();
        }
    }

    // ------------------------------------------------------------------ Import

    pub(super) fn import_files(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter("Anisoft 4.2 / AGICO binary (*.ran)", &["ran"])
            .add_filter("Comma-delimited (*.csv)", &["csv"])
            .set_title("Import Data File(s)");
        if self.browser_dir.is_dir() {
            dlg = dlg.set_directory(&self.browser_dir);
        }
        let Some(p) = dlg.pick_file() else { return };
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let read = std::fs::read(&p).map_err(|e| e.to_string());
        let res = read.and_then(|bytes| match ext.as_str() {
            "ran" => ams_core::import::ran_to_records(&bytes).map(|r| (r, 0usize)),
            "csv" => ams_core::import::csv_to_records(&String::from_utf8_lossy(&bytes)),
            _ => Err("Import reads .ran (Anisoft 4.2) and .csv (written by Anisoft or AMS Studio) files. The other lab formats of the original's list (ASC, SI2B, AMSSpin, SushiBar, TorqueZurich, AMSBurgos, IODP, BillMorris, Toulouse, Molyneux) are not supported.".to_string()),
        });
        match res {
            Ok((raw, skipped)) => {
                let name = format!("{} (imported)", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                match Doc::simulated(&name, raw) {
                    Ok(d) => {
                        let n = d.specs.len();
                        self.set_doc(d);
                        self.info = Some(format!("Imported {n} specimen(s) from {}{}.\nUse File > Save Active Records As to keep them as an .ams file.{}", p.display(), if skipped > 0 { format!(" ({skipped} unreadable row(s) skipped)") } else { String::new() }, if ext == "ran" { "\nA .ran file has no out-of-phase data, no standard deviations and no F-tests." } else { "" }));
                    }
                    Err(e) => self.info = Some(e),
                }
            }
            Err(e) => self.info = Some(format!("Could not import {}:\n{e}", p.display())),
        }
    }
}
