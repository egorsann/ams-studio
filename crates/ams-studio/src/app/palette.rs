//! The command palette (Ctrl+Shift+P): a box at the top of the window with a text field and the list of everything the
//! program can do (every menu item, with its shortcut), narrowed as you type. Arrow keys move, Enter or a click runs,
//! Esc closes. The list is built from the same actions the menus push (`Action`), so a command does exactly what its
//! menu item does; commands that cannot run now (no file open) are shown grey and are skipped.

use super::{Action, App, PaneId};
use ams_core::coords::CoordSys;
use egui::{Align2, Color32, Key, Modifiers, RichText, Vec2};

#[derive(Clone, Debug)]
pub struct Cmd {
    /// The menu it belongs to.
    pub group: &'static str,
    pub label: String,
    pub shortcut: &'static str,
    pub enabled: bool,
    pub action: Action,
}

#[derive(Default)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    /// Index into the filtered list.
    pub sel: usize,
    /// The field takes the keyboard when the palette opens.
    pub fresh: bool,
}

/// Does `query` fit `text`? Every word of the query must be somewhere in the text (letters in a row, any case);
/// the score is lower for words found earlier and at the start of a word, so the better match comes first.
pub fn score(query: &str, text: &str) -> Option<usize> {
    let hay = text.to_lowercase();
    let mut total = 0;
    for w in query.to_lowercase().split_whitespace() {
        let at = hay.find(w)?;
        let word_start = at == 0 || !hay[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric());
        total += at + if word_start { 0 } else { 40 };
    }
    Some(total)
}

/// The commands that fit the query, best first (the original order for an empty query and for equal scores).
pub fn filter(cmds: &[Cmd], query: &str) -> Vec<usize> {
    let mut hits: Vec<(usize, usize)> = cmds.iter().enumerate().filter_map(|(i, c)| score(query, &format!("{} {}", c.group, c.label)).map(|s| (s, i))).collect();
    hits.sort_by_key(|&(s, i)| (s, i));
    hits.into_iter().map(|(_, i)| i).collect()
}

impl App {
    /// Everything the program can do, in the order of the menus.
    pub(super) fn commands(&self) -> Vec<Cmd> {
        let has = self.doc.is_some();
        let mut v: Vec<Cmd> = vec![];
        let mut add = |group: &'static str, label: &str, shortcut: &'static str, enabled: bool, action: Action| v.push(Cmd { group, label: label.to_string(), shortcut, enabled, action });
        add("File", "Open Data File(s) ...", "Ctrl+O", true, Action::Open);
        add("File", "Open Folder of Specimens ...", "Ctrl+Shift+O", true, Action::OpenSet);
        add("File", "Open Previous Data File", "Ctrl+P", has, Action::Prev);
        add("File", "Open Next Data File", "Ctrl+N", has, Action::Next);
        add("File", "Reload Current Data File(s)", "Ctrl+R", has, Action::Reload);
        add("File", "Import Data File(s)", "Ctrl+I", true, Action::Import);
        add("File", "Save Active Records As", "Ctrl+S", has, Action::SaveActive);
        add("File", "Append Selected Records Into", "Ctrl+A", has, Action::AppendSel);
        add("File", "Save as .ams (selected or edited files) ...", "Ctrl+Shift+S", has, Action::ExportAms);
        add("File", "Export Data File (CSV, in- and out-of-phase)", "Ctrl+E", has, Action::ExportCsv);
        add("File", "Export Selection (CSV, in- and out-of-phase)", "Ctrl+Shift+E", has, Action::ExportSelection);
        add("File", "Close Current Data File(s)", "Ctrl+Q", has, Action::Close);
        add("File", "Exit Program", "Ctrl+X", true, Action::Exit);
        add("Edit", "Edit Data", "F1", has, Action::EditData);
        add("Edit", "Set Data Classes", "Ctrl+F1", has, Action::SetClasses);
        add("Edit", "Spec to Geo (Owens)", "", has, Action::Owens);
        add("Edit", "Drill Core to Geo (cubes) ...", "", has, Action::CoreGeo);
        add("Edit", "Volume Correction ...", "", has, Action::Volume);
        add("Edit", "Recombine Split AMS Files ...", "", true, Action::Recombine);
        add("Edit", "Invert Selection", "Shift+Ins", has, Action::InvertSel);
        add("Edit", "Disregard Selected Record(s)", "Shift+Del", has, Action::Disregard);
        add("Edit", "Restore All Records", "Alt+Bksp", has, Action::RestoreAll);
        add("Edit", "Clear Selection", "Shift+Ctrl+F12", has, Action::ClearSel);
        add("Graphics", "Copy Graphics To Clipboard", "Ctrl+C", has, Action::CopyGraphics);
        add("Graphics", "Save Graphics To File", "Ctrl+F", has, Action::SaveGraphics);
        add("View", "Data Table", "F2", has, Action::DataTable);
        add("View", "In-Phase AMS", "F3", has, Action::Phase(false));
        add("View", "Out-of-Phase AMS", "F4", has, Action::Phase(true));
        add("View", "Switch In / Out-of-Phase", "W", has, Action::TogglePhase);
        add("View", "Previous Specimen", "Q", has, Action::StepSpecimen(-1));
        add("View", "Next Specimen", "E", has, Action::StepSpecimen(1));
        add("View", "Specimen Coordinates (SPEC)", "F5", has, Action::Coord(CoordSys::Specimen));
        add("View", "Geographic Coordinates (GEO)", "F6", has, Action::Coord(CoordSys::Geographic));
        add("View", "Paleogeographic Coordinates (PALEO)", "F7", has, Action::Coord(CoordSys::Paleo));
        add("View", "Tectonic Coordinates (TECTO)", "F8", has, Action::Coord(CoordSys::Tectonic));
        add("View", "File Browser", "Ctrl+B", true, Action::ToggleBrowser);
        add("View", "Enable core viewer", "", has, Action::EnableCoreViewer);
        if self.cv_current_has_link() {
            add("View", if self.cv.on { "Hide core viewer" } else { "Show core viewer" }, "", true, Action::ToggleCoreViewer);
        }
        add("View", "Add pane", "Shift+Ctrl+N", true, Action::AddPane);
        for id in self.layout_order.clone() {
            if id == PaneId::Stereo {
                continue;
            }
            let name = match id {
                PaneId::Plot(k) => self.pane_names()[k].clone(),
                PaneId::Core => "Core viewer".to_string(),
                _ => "GIS viewer".to_string(),
            };
            add("View", &format!("Remove pane: {name}"), "", true, Action::RemovePaneAt(id));
        }
        add("View", "Reset pane layout", "", true, Action::ResetLayout);
        add("View", "Reset all panes to default", "", true, Action::ResetPanes);
        if self.reset_undo_left().is_some() {
            add("View", "Undo reset of the panes", "", true, Action::UndoResetPanes);
        }
        add("View", if self.dark { "Light Theme" } else { "Dark Theme" }, "", true, Action::ToggleTheme);
        // the GIS menu
        add("GIS", if self.gis_on { "Hide the GIS viewer pane" } else { "Show the GIS viewer pane" }, "", true, Action::ToggleGis);
        add("GIS", if self.gis.show_layers { "Hide the layers sidebar" } else { "Show the layers sidebar" }, "Shift+Ctrl+B", self.gis_on, Action::ToggleLayerBar);
        add("GIS", if self.gis_popped { "Dock the GIS viewer back into the main window" } else { "GIS viewer in its own window" }, "Shift+Ctrl+Space", true, Action::PopOutGis);
        add("GIS", if self.gis.base_on { "Backdrop off" } else { "Backdrop on" }, "", true, Action::GisBasemap);
        add("GIS", "Backdrop: street map", "", true, Action::GisBackdropSource(super::gis_data::BaseSource::Streets));
        add("GIS", "Backdrop: satellite imagery", "", true, Action::GisBackdropSource(super::gis_data::BaseSource::Satellite));
        add("GIS", "Add layer (map, shapefile, GeoPackage, GeoJSON) ...", "", true, Action::GisAddLayer);
        add("GIS", "Add layers of a QGIS or ArcGIS project (.qgz, .qgs, .aprx, .lyrx, .mapx, .mxd) ...", "", true, Action::GisImportQgis);
        add("GIS", "Undo the last layer removal", "", true, Action::GisUndoLayers);
        add("GIS", "Fit the map to the data", "", self.gis_on, Action::GisFit);
        add("GIS", "Specimen locations (from a datasheet) ...", "", true, Action::GisLocations);
        add("GIS", "Enter coordinates manually ...", "", true, Action::GisManual);
        add("GIS", "GIS viewer settings ...", "", true, Action::Gis);
        for (id, name, visible, kind) in self.gis.layers.items.iter().map(|i| (i.id, i.name.clone(), i.visible, i.kind)) {
            if kind == super::gis_layers::Kind::Group {
                continue;
            }
            let on = if kind == super::gis_layers::Kind::Basemap { self.gis.base_on } else { visible };
            add("GIS", &format!("{} layer: {name}", if on { "Hide" } else { "Show" }), "", true, Action::GisToggleLayer(id));
        }
        add("Analysis", "Group Statistics", "F9", has, Action::GroupStats);
        add("Analysis", "Contour Plot", "F12", has, Action::Contour);
        for n in ["GED Editor", "Tensor Addition / Subtraction", "Fabric Overprint", "Intersection Toolbox", "Deform AMS"] {
            add("Tools", n, "", true, Action::Tool(n));
        }
        add("Settings", "Anisotropy Quantitative Factors", "", true, Action::Factors);
        add("Settings", "Orientation Parameters", "", true, Action::OrientParams);
        add("Settings", "Default Colors", "", true, Action::Colors);
        add("Help", "About", "", true, Action::About);
        v
    }

    pub(super) fn palette_toggle(&mut self) {
        self.palette.open = !self.palette.open;
        self.palette.query.clear();
        self.palette.sel = 0;
        self.palette.fresh = true;
    }

    /// The palette box. Runs the chosen command through the same path as the menus.
    pub(super) fn palette_window(&mut self, ctx: &egui::Context) {
        if !self.palette.open {
            return;
        }
        let cmds = self.commands();
        let mut st = std::mem::take(&mut self.palette);
        let hits = filter(&cmds, &st.query);
        st.sel = st.sel.min(hits.len().saturating_sub(1));
        let (mut run, mut close) = (None::<Action>, false);
        // keys first, so the text field does not keep Up, Down and Enter for itself
        let (up, down, enter, esc) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
        if up && st.sel > 0 {
            st.sel -= 1;
        }
        if down && st.sel + 1 < hits.len() {
            st.sel += 1;
        }
        if esc {
            close = true;
        }
        if enter {
            if let Some(&i) = hits.get(st.sel) {
                if cmds[i].enabled {
                    run = Some(cmds[i].action);
                }
            }
        }
        let scr = ctx.content_rect();
        let width = (scr.width() - 40.0).clamp(360.0, 640.0);
        egui::Area::new(egui::Id::unique("command-palette")).anchor(Align2::CENTER_TOP, Vec2::new(0.0, 60.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                let field = ui.add(egui::TextEdit::singleline(&mut st.query).hint_text("Type a command ...  (Esc closes)").desired_width(f32::INFINITY).font(egui::TextStyle::Heading));
                if st.fresh {
                    field.request_focus();
                    st.fresh = false;
                }
                if field.changed() {
                    st.sel = 0;
                }
                ui.add_space(2.0);
                if hits.is_empty() {
                    ui.label(RichText::new("No command fits").weak());
                }
                egui::ScrollArea::vertical().max_height((scr.height() - 220.0).clamp(160.0, 420.0)).auto_shrink([false, true]).show(ui, |ui| {
                    for (row, &i) in hits.iter().enumerate() {
                        let c = &cmds[i];
                        let selected = row == st.sel;
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), egui::Sense::click());
                        if selected {
                            ui.painter().rect_filled(rect, 5.0, ui.visuals().selection.bg_fill);
                            ui.scroll_to_rect(rect, None);
                        } else if resp.hovered() && c.enabled {
                            ui.painter().rect_filled(rect, 5.0, ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.6));
                        }
                        let ink = if c.enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color().gamma_multiply(0.7) };
                        let weak = ui.visuals().weak_text_color();
                        let p = ui.painter();
                        p.text(rect.left_center() + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, c.group, egui::FontId::proportional(12.0), weak);
                        p.text(rect.left_center() + Vec2::new(86.0, 0.0), Align2::LEFT_CENTER, &c.label, egui::FontId::proportional(14.5), ink);
                        if !c.shortcut.is_empty() {
                            p.text(rect.right_center() - Vec2::new(10.0, 0.0), Align2::RIGHT_CENTER, c.shortcut, egui::FontId::proportional(12.0), weak);
                        }
                        if resp.hovered() && ui.input(|i| i.pointer.delta() != Vec2::ZERO) {
                            st.sel = row;
                        }
                        if resp.clicked() && c.enabled {
                            run = Some(c.action);
                        }
                    }
                });
                ui.label(RichText::new(format!("(n={}) commands   Up / Down to move, Enter to run", hits.len())).small().color(Color32::GRAY));
            });
        });
        if run.is_some() || close {
            st.open = false;
        }
        self.palette = st;
        if let Some(a) = run {
            self.run(ctx, a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(group: &'static str, label: &str) -> Cmd {
        Cmd { group, label: label.into(), shortcut: "", enabled: true, action: Action::About }
    }

    #[test]
    fn every_word_of_the_query_must_fit_and_earlier_matches_come_first() {
        let c = vec![cmd("File", "Open Data File(s) ..."), cmd("File", "Open Folder of Specimens ..."), cmd("GIS", "Show the GIS viewer pane"), cmd("Edit", "Disregard Selected Record(s)"), cmd("View", "Dark Theme")];
        // empty: everything, in order
        assert_eq!(filter(&c, ""), vec![0, 1, 2, 3, 4]);
        assert_eq!(filter(&c, "open"), vec![0, 1]);
        assert_eq!(filter(&c, "OPEN fol"), vec![1]);
        // the group counts: typing the menu name lists its commands
        assert_eq!(filter(&c, "gis"), vec![2]);
        // a word at the start of a word beats the same letters in the middle of one
        assert_eq!(filter(&c, "sel"), vec![3]);
        assert!(filter(&c, "xyz").is_empty());
        assert!(filter(&c, "open xyz").is_empty());
        // score: earlier and word-start matches are lower
        assert!(score("dark", "View Dark Theme").unwrap() < score("ark", "View Dark Theme").unwrap());
    }
}
